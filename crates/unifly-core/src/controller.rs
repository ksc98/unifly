// ── Controller abstraction ──
//
// Full lifecycle management for a UniFi controller connection.
// Handles authentication, background refresh, command routing,
// and reactive data streaming through the DataStore.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::command::{Command, CommandEnvelope, CommandResult};
use crate::config::{AuthCredentials, ControllerConfig, TlsVerification};
use crate::error::CoreError;
use crate::model::{
    AclRule, Admin, Alarm, Client, Country, Device, DnsPolicy, DpiApplication, DpiCategory,
    EntityId, Event, FirewallAction, FirewallPolicy, FirewallZone, HealthSummary, MacAddress,
    Network, NetworkManagement, NetworkPurpose, RadiusProfile, Site, SysInfo, SystemInfo,
    TrafficMatchingList, Voucher, VpnServer, VpnTunnel, WanInterface, WifiBroadcast,
};
use crate::model::device::DeviceStatsUpdate;
use crate::store::DataStore;
use crate::stream::EntityStream;

use unifly_api::transport::{TlsMode, TransportConfig};
use unifly_api::websocket::{ReconnectConfig, WebSocketHandle};
use unifly_api::{IntegrationClient, LegacyClient};

const COMMAND_CHANNEL_SIZE: usize = 64;
const EVENT_CHANNEL_SIZE: usize = 256;

// ── ConnectionState ──────────────────────────────────────────────

/// Connection state observable by consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed,
}

// ── Controller ───────────────────────────────────────────────────

/// The main entry point for consumers.
///
/// Cheaply cloneable via `Arc<ControllerInner>`. Manages the full
/// connection lifecycle: authentication, background data refresh,
/// command routing, and reactive entity streaming.
#[derive(Clone)]
pub struct Controller {
    inner: Arc<ControllerInner>,
}

struct ControllerInner {
    config: ControllerConfig,
    store: Arc<DataStore>,
    connection_state: watch::Sender<ConnectionState>,
    event_tx: broadcast::Sender<Arc<Event>>,
    command_tx: Mutex<mpsc::Sender<CommandEnvelope>>,
    command_rx: Mutex<Option<mpsc::Receiver<CommandEnvelope>>>,
    cancel: CancellationToken,
    /// Child token for the current connection — cancelled on disconnect,
    /// replaced on reconnect (avoids permanent cancellation).
    cancel_child: Mutex<CancellationToken>,
    legacy_client: Mutex<Option<LegacyClient>>,
    integration_client: Mutex<Option<IntegrationClient>>,
    /// Resolved Integration API site UUID (populated on connect).
    site_id: Mutex<Option<uuid::Uuid>>,
    /// WebSocket event stream handle (populated on connect if enabled).
    ws_handle: Mutex<Option<WebSocketHandle>>,
    task_handles: Mutex<Vec<JoinHandle<()>>>,
    /// Warnings accumulated during connect (e.g. Legacy auth failure in Hybrid mode).
    warnings: Mutex<Vec<String>>,
    /// Channel for device stats updates — all sources send partial updates here,
    /// a single merge task applies them to the store.
    stats_tx: mpsc::UnboundedSender<DeviceStatsUpdate>,
    stats_rx: Mutex<Option<mpsc::UnboundedReceiver<DeviceStatsUpdate>>>,
}

impl Controller {
    /// Create a new Controller from configuration. Does NOT connect --
    /// call [`connect()`](Self::connect) to authenticate and start background tasks.
    pub fn new(config: ControllerConfig) -> Self {
        let store = Arc::new(DataStore::new());
        let (connection_state, _) = watch::channel(ConnectionState::Disconnected);
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_SIZE);
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_SIZE);
        let cancel = CancellationToken::new();
        let cancel_child = cancel.child_token();
        let (stats_tx, stats_rx) = mpsc::unbounded_channel();

        Self {
            inner: Arc::new(ControllerInner {
                config,
                store,
                connection_state,
                event_tx,
                command_tx: Mutex::new(command_tx),
                command_rx: Mutex::new(Some(command_rx)),
                cancel,
                cancel_child: Mutex::new(cancel_child),
                legacy_client: Mutex::new(None),
                integration_client: Mutex::new(None),
                warnings: Mutex::new(Vec::new()),
                site_id: Mutex::new(None),
                ws_handle: Mutex::new(None),
                task_handles: Mutex::new(Vec::new()),
                stats_tx,
                stats_rx: Mutex::new(Some(stats_rx)),
            }),
        }
    }

    /// Access the controller configuration.
    pub fn config(&self) -> &ControllerConfig {
        &self.inner.config
    }

    /// Access the underlying DataStore.
    pub fn store(&self) -> &Arc<DataStore> {
        &self.inner.store
    }

    // ── Connection lifecycle ─────────────────────────────────────

    /// Connect to the controller.
    ///
    /// Detects the platform, authenticates, performs an initial data
    /// refresh, and spawns background tasks (periodic refresh, command
    /// processor).
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub async fn connect(&self) -> Result<(), CoreError> {
        let _ = self
            .inner
            .connection_state
            .send(ConnectionState::Connecting);

        // Fresh child token for this connection (supports reconnect).
        let child = self.inner.cancel.child_token();
        *self.inner.cancel_child.lock().await = child.clone();

        let config = &self.inner.config;
        let transport = build_transport(config);

        match &config.auth {
            AuthCredentials::ApiKey(api_key) => {
                // Detect platform so we use the right URL prefix
                let platform = LegacyClient::detect_platform(&config.url).await?;
                debug!(?platform, "detected controller platform");

                // Integration API client (preferred)
                let integration = IntegrationClient::from_api_key(
                    config.url.as_str(),
                    api_key,
                    &transport,
                    platform,
                )?;

                // Resolve site UUID from Integration API
                let site_id = resolve_site_id(&integration, &config.site).await?;
                debug!(site_id = %site_id, "resolved Integration API site UUID");

                *self.inner.integration_client.lock().await = Some(integration);
                *self.inner.site_id.lock().await = Some(site_id);

                // Also set up Legacy client for event streams and supplementary data.
                // API key auth may not work with Legacy API on all controllers,
                // so we swallow errors here — it's optional.
                match setup_legacy_client(config, &transport).await {
                    Ok(client) => {
                        *self.inner.legacy_client.lock().await = Some(client);
                        debug!("legacy client available as supplement");
                    }
                    Err(e) => {
                        debug!(error = %e, "legacy client unavailable (non-fatal with API key auth)");
                    }
                }
            }
            AuthCredentials::Credentials { username, password } => {
                // Legacy-only auth
                let platform = LegacyClient::detect_platform(&config.url).await?;
                debug!(?platform, "detected controller platform");

                let client = LegacyClient::new(
                    config.url.clone(),
                    config.site.clone(),
                    platform,
                    &transport,
                )?;
                client.login(username, password).await?;
                debug!("session authentication successful");

                *self.inner.legacy_client.lock().await = Some(client);
            }
            AuthCredentials::Hybrid {
                api_key,
                username,
                password,
            } => {
                // Hybrid: both Integration API (API key) and Legacy API (session auth)
                let platform = LegacyClient::detect_platform(&config.url).await?;
                debug!(?platform, "detected controller platform (hybrid)");

                // Integration API client
                let integration = IntegrationClient::from_api_key(
                    config.url.as_str(),
                    api_key,
                    &transport,
                    platform,
                )?;

                let site_id = resolve_site_id(&integration, &config.site).await?;
                debug!(site_id = %site_id, "resolved Integration API site UUID");

                *self.inner.integration_client.lock().await = Some(integration);
                *self.inner.site_id.lock().await = Some(site_id);

                // Legacy API client — attempt login but degrade gracefully
                // if it fails. The Integration API is the primary surface;
                // Legacy adds events, stats, and admin ops.
                match LegacyClient::new(
                    config.url.clone(),
                    config.site.clone(),
                    platform,
                    &transport,
                ) {
                    Ok(client) => match client.login(username, password).await {
                        Ok(()) => {
                            debug!("legacy session authentication successful (hybrid)");
                            *self.inner.legacy_client.lock().await = Some(client);
                        }
                        Err(e) => {
                            let msg = format!(
                                "Legacy login failed: {e} — events, health stats, and client traffic will be unavailable"
                            );
                            warn!("{msg}");
                            self.inner.warnings.lock().await.push(msg);
                        }
                    },
                    Err(e) => {
                        let msg = format!("Legacy client setup failed: {e}");
                        warn!("{msg}");
                        self.inner.warnings.lock().await.push(msg);
                    }
                }
            }
            AuthCredentials::Cloud { api_key, host_id } => {
                let integration = IntegrationClient::from_api_key(
                    config.url.as_str(),
                    api_key,
                    &transport,
                    unifly_api::ControllerPlatform::Cloud,
                )?;

                let site_id = if let Ok(uuid) = uuid::Uuid::parse_str(&config.site) {
                    uuid
                } else if let Ok(uuid) = uuid::Uuid::parse_str(host_id) {
                    uuid
                } else {
                    resolve_site_id(&integration, &config.site).await?
                };
                debug!(site_id = %site_id, "resolved cloud Integration API site UUID");

                *self.inner.integration_client.lock().await = Some(integration);
                *self.inner.site_id.lock().await = Some(site_id);

                let msg =
                    "Cloud auth mode active: Legacy API and WebSocket features are unavailable"
                        .to_string();
                self.inner.warnings.lock().await.push(msg);
            }
        }

        // Initial data load
        self.full_refresh().await?;

        // Spawn background tasks
        let mut handles = self.inner.task_handles.lock().await;

        // Stats merge task — single consumer for all device stats updates.
        if let Some(stats_rx) = self.inner.stats_rx.lock().await.take() {
            let store = Arc::clone(&self.inner.store);
            handles.push(tokio::spawn(stats_merge_task(store, stats_rx)));
        }

        if let Some(rx) = self.inner.command_rx.lock().await.take() {
            let ctrl = self.clone();
            handles.push(tokio::spawn(command_processor_task(ctrl, rx)));
        }

        let interval_secs = config.refresh_interval_secs;
        if interval_secs > 0 {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(refresh_task(ctrl, interval_secs, cancel)));
        }

        let health_interval = config.bandwidth_poll_interval;
        if !health_interval.is_zero() {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(health_poll_task(ctrl, health_interval, cancel)));
        }

        // Client data is primarily sourced from WebSocket sta:sync events (real-time).
        // The poll task serves as fallback (stale client cleanup, WS disconnected).
        let client_fallback = std::time::Duration::from_secs(30);
        {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(client_poll_task(ctrl, client_fallback, cancel)));
        }

        // Device stats are primarily sourced from WebSocket device:sync events (real-time).
        // Poll task also enabled as fallback for missing bandwidth data on APs/switches.
        let device_stats_interval = std::time::Duration::from_secs(2);
        {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(device_stats_poll_task(ctrl, device_stats_interval, cancel)));
        }

        // Monthly WAN usage stats (fetches every 60s)
        {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(monthly_stats_task(ctrl, cancel)));
        }

        // Per-client 24h usage stats (fetches every 5 min)
        {
            let ctrl = self.clone();
            let cancel = child.clone();
            handles.push(tokio::spawn(client_daily_usage_task(ctrl, cancel)));
        }

        // WebSocket event stream
        if config.websocket_enabled {
            self.spawn_websocket(&child, &mut handles).await;
        }

        let _ = self.inner.connection_state.send(ConnectionState::Connected);
        info!("connected to controller");
        Ok(())
    }

    /// Spawn the WebSocket event stream and a bridge task that converts
    /// raw [`UnifiEvent`]s into domain [`Event`]s and broadcasts them.
    ///
    /// Non-fatal on failure — the TUI falls back to polling.
    async fn spawn_websocket(&self, cancel: &CancellationToken, handles: &mut Vec<JoinHandle<()>>) {
        let legacy_guard = self.inner.legacy_client.lock().await;
        let Some(ref legacy) = *legacy_guard else {
            debug!("no legacy client — WebSocket unavailable");
            return;
        };

        let platform = legacy.platform();
        let Some(ws_path_template) = platform.websocket_path() else {
            debug!("platform does not support WebSocket");
            return;
        };

        let ws_path = ws_path_template.replace("{site}", &self.inner.config.site);
        let base_url = &self.inner.config.url;
        let scheme = if base_url.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };
        let host = base_url.host_str().unwrap_or("localhost");
        let ws_url_str = match base_url.port() {
            Some(p) => format!("{scheme}://{host}:{p}{ws_path}"),
            None => format!("{scheme}://{host}{ws_path}"),
        };
        let ws_url = match url::Url::parse(&ws_url_str) {
            Ok(u) => u,
            Err(e) => {
                warn!(error = %e, url = %ws_url_str, "invalid WebSocket URL");
                return;
            }
        };

        let cookie = legacy.cookie_header();
        drop(legacy_guard);

        if cookie.is_none() {
            warn!("no session cookie — WebSocket requires legacy auth (skipping)");
            return;
        }

        let ws_cancel = cancel.child_token();
        let insecure = matches!(
            self.inner.config.tls,
            crate::TlsVerification::DangerAcceptInvalid
        );
        let handle = match WebSocketHandle::connect(
            ws_url,
            ReconnectConfig::default(),
            ws_cancel.clone(),
            cookie,
            insecure,
        ) {
            Ok(h) => h,
            Err(e) => {
                warn!(error = %e, "WebSocket connection failed (non-fatal)");
                return;
            }
        };

        // Bridge task: WS events → domain Events → broadcast channel.
        // Also extracts real-time device stats from `device:sync` messages
        // to feed the dashboard chart without waiting for full_refresh().
        let mut ws_rx = handle.subscribe();
        let event_tx = self.inner.event_tx.clone();
        let store = Arc::clone(&self.inner.store);
        let ws_stats_tx = self.inner.stats_tx.clone();
        let bridge_cancel = ws_cancel;

        handles.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    () = bridge_cancel.cancelled() => break,
                    result = ws_rx.recv() => {
                        match result {
                            Ok(ws_event) => {
                                // Extract real-time stats from device:sync messages
                                if ws_event.key == "device:sync" || ws_event.key == "device:update" {
                                    apply_device_sync(&ws_stats_tx, &ws_event.extra);
                                }

                                // Extract real-time client data from sta:sync messages
                                if ws_event.key == "sta:sync" {
                                    apply_sta_sync(&store, &ws_event.extra);
                                }

                                // Filter out sync/state-dump messages from the event log
                                let is_sync = ws_event.key.ends_with(":sync")
                                    || ws_event.key.ends_with(":update");
                                if !is_sync {
                                    let event = crate::model::event::Event::from(
                                        (*ws_event).clone(),
                                    );
                                    let _ = event_tx.send(Arc::new(event));
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                warn!(skipped = n, "WS bridge: receiver lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        }));

        *self.inner.ws_handle.lock().await = Some(handle);
        info!("WebSocket event stream spawned (handshake in progress)");
    }

    /// Disconnect from the controller.
    ///
    /// Cancels background tasks, logs out if session-based, and resets
    /// the connection state to [`Disconnected`](ConnectionState::Disconnected).
    pub async fn disconnect(&self) {
        // Cancel the child token (not the parent — allows reconnect).
        self.inner.cancel_child.lock().await.cancel();

        // Join all background tasks
        let mut handles = self.inner.task_handles.lock().await;
        for handle in handles.drain(..) {
            let _ = handle.await;
        }

        // Logout if session-based (Credentials or Hybrid both have active sessions)
        if matches!(
            self.inner.config.auth,
            AuthCredentials::Credentials { .. } | AuthCredentials::Hybrid { .. }
        ) {
            if let Some(ref client) = *self.inner.legacy_client.lock().await {
                if let Err(e) = client.logout().await {
                    warn!(error = %e, "logout failed (non-fatal)");
                }
            }
        }

        // Shut down WebSocket if active
        if let Some(handle) = self.inner.ws_handle.lock().await.take() {
            handle.shutdown();
        }

        *self.inner.legacy_client.lock().await = None;
        *self.inner.integration_client.lock().await = None;
        *self.inner.site_id.lock().await = None;

        // Recreate command channel so reconnects can spawn a fresh receiver.
        // The previous receiver is consumed by the command processor task.
        {
            let (tx, rx) = mpsc::channel(COMMAND_CHANNEL_SIZE);
            *self.inner.command_tx.lock().await = tx;
            *self.inner.command_rx.lock().await = Some(rx);
        }

        let _ = self
            .inner
            .connection_state
            .send(ConnectionState::Disconnected);
        debug!("disconnected");
    }

    /// Fetch all data from the controller and update the DataStore.
    ///
    /// Pulls devices, clients, and events from the Legacy API, converts
    /// them to domain types, and applies them to the store. Events are
    /// broadcast through the event channel (not stored).
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub async fn full_refresh(&self) -> Result<(), CoreError> {
        let integration_guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(integration), Some(sid)) = (integration_guard.as_ref(), site_id) {
            // ── Integration API path (preferred) ─────────────────
            let page_limit = 200;

            // Client data is handled exclusively by client_poll_task (Legacy API,
            // 2s interval) — the Legacy API is a strict superset of Integration API
            // client data (bandwidth, vendor, wireless, etc.).
            let (devices_res, networks_res, wifi_res) = tokio::join!(
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_devices(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_networks(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_wifi_broadcasts(&sid, off, lim)
                }),
            );

            let (policies_res, zones_res, acls_res, dns_res, vouchers_res) = tokio::join!(
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_firewall_policies(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_firewall_zones(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_acl_rules(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_dns_policies(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_vouchers(&sid, off, lim)
                }),
            );

            let (sites_res, tml_res) = tokio::join!(
                integration.paginate_all(50, |off, lim| { integration.list_sites(off, lim) }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_traffic_matching_lists(&sid, off, lim)
                }),
            );

            // Core endpoints — failure is fatal
            let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
            // Fetch full details for each network (list endpoint omits ipv4/ipv6 config)
            let network_ids: Vec<uuid::Uuid> = networks_res?.into_iter().map(|n| n.id).collect();
            info!(
                network_count = network_ids.len(),
                "fetching network details"
            );
            let networks: Vec<Network> = {
                let futs = network_ids.into_iter().map(|nid| async move {
                    match integration.get_network(&sid, &nid).await {
                        Ok(detail) => Some(Network::from(detail)),
                        Err(e) => {
                            warn!(network_id = %nid, error = %e, "network detail fetch failed");
                            None
                        }
                    }
                });
                futures_util::future::join_all(futs)
                    .await
                    .into_iter()
                    .flatten()
                    .collect()
            };
            let wifi: Vec<WifiBroadcast> = wifi_res?.into_iter().map(WifiBroadcast::from).collect();
            let policies: Vec<FirewallPolicy> = unwrap_or_empty("firewall/policies", policies_res);
            let zones: Vec<FirewallZone> = unwrap_or_empty("firewall/zones", zones_res);
            let sites: Vec<Site> = sites_res?.into_iter().map(Site::from).collect();
            let traffic_matching_lists: Vec<TrafficMatchingList> = tml_res?
                .into_iter()
                .map(TrafficMatchingList::from)
                .collect();

            // Optional endpoints — 404 means the controller doesn't support them
            let acls: Vec<AclRule> = unwrap_or_empty("acl/rules", acls_res);
            let dns: Vec<DnsPolicy> = unwrap_or_empty("dns/policies", dns_res);
            let vouchers: Vec<Voucher> = unwrap_or_empty("vouchers", vouchers_res);

            // Enrich devices with per-device statistics (parallel, non-fatal)
            info!(
                device_count = devices.len(),
                "enriching devices with statistics"
            );
            let mut devices = {
                let futs = devices.into_iter().map(|mut device| async {
                    if let EntityId::Uuid(device_uuid) = &device.id {
                        match integration.get_device_statistics(&sid, device_uuid).await {
                            Ok(stats_resp) => {
                                // Merge stats instead of replacing - keep existing values if new ones are None
                                let new_stats = crate::convert::device_stats_from_integration(&stats_resp);
                                if new_stats.uptime_secs.is_some() {
                                    device.stats.uptime_secs = new_stats.uptime_secs;
                                }
                                if new_stats.cpu_utilization_pct.is_some() {
                                    device.stats.cpu_utilization_pct = new_stats.cpu_utilization_pct;
                                }
                                if new_stats.memory_utilization_pct.is_some() {
                                    device.stats.memory_utilization_pct = new_stats.memory_utilization_pct;
                                }
                                if new_stats.load_average_1m.is_some() {
                                    device.stats.load_average_1m = new_stats.load_average_1m;
                                }
                                if new_stats.load_average_5m.is_some() {
                                    device.stats.load_average_5m = new_stats.load_average_5m;
                                }
                                if new_stats.load_average_15m.is_some() {
                                    device.stats.load_average_15m = new_stats.load_average_15m;
                                }
                                if new_stats.uplink_bandwidth.is_some() {
                                    device.stats.uplink_bandwidth = new_stats.uplink_bandwidth;
                                }
                                if new_stats.last_heartbeat.is_some() {
                                    device.stats.last_heartbeat = new_stats.last_heartbeat;
                                }
                                if new_stats.next_heartbeat.is_some() {
                                    device.stats.next_heartbeat = new_stats.next_heartbeat;
                                }
                            }
                            Err(e) => {
                                warn!(
                                    device = ?device.name,
                                    error = %e,
                                    "device stats fetch failed"
                                );
                            }
                        }
                    }
                    device
                });
                futures_util::future::join_all(futs).await
            };

            drop(integration_guard);

            // Supplement with Legacy API data (events, health, device stats).
            // Client data is NOT fetched here — client_poll_task (2s, Legacy API)
            // is the sole authoritative source for client data.
            //
            // Clone the Legacy client and release the lock before making API calls
            // to avoid blocking other poll tasks (client_poll, health_poll, etc.).
            let legacy_clone = {
                let guard = self.inner.legacy_client.lock().await;
                guard.as_ref().cloned()
            };

            let (legacy_events, legacy_health, legacy_devices): (
                Vec<Event>,
                Vec<HealthSummary>,
                Vec<unifly_api::legacy::models::LegacyDevice>,
            ) = match legacy_clone {
                Some(legacy) => {
                    let (events_res, health_res, devices_res) = tokio::join!(
                        legacy.list_events(Some(100)),
                        legacy.get_health(),
                        legacy.list_devices(),
                    );

                    let events = match events_res {
                        Ok(raw) => {
                            let evts: Vec<Event> = raw.into_iter().map(Event::from).collect();
                            for evt in &evts {
                                let _ = self.inner.event_tx.send(Arc::new(evt.clone()));
                            }
                            evts
                        }
                        Err(e) => {
                            warn!(error = %e, "legacy event fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let health = match health_res {
                        Ok(raw) => convert_health_summaries(raw),
                        Err(e) => {
                            warn!(error = %e, "legacy health fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    let ld = match devices_res {
                        Ok(raw) => raw,
                        Err(e) => {
                            warn!(error = %e, "legacy device fetch failed (non-fatal)");
                            Vec::new()
                        }
                    };

                    (events, health, ld)
                }
                None => (Vec::new(), Vec::new(), Vec::new()),
            };

            // Merge Legacy device num_sta (client counts) into Integration devices
            if !legacy_devices.is_empty() {
                let legacy_by_mac: HashMap<&str, &unifly_api::legacy::models::LegacyDevice> =
                    legacy_devices.iter().map(|d| (d.mac.as_str(), d)).collect();
                for device in &mut devices {
                    if let Some(ld) = legacy_by_mac.get(device.mac.as_str()) {
                        if device.client_count.is_none() {
                            device.client_count = ld.num_sta.and_then(|n| n.try_into().ok());
                        }
                        if device.wan_ipv6.is_none() {
                            device.wan_ipv6 = parse_legacy_device_wan_ipv6(&ld.extra);
                        }
                        if device.uplink_device_mac.is_none() {
                            device.uplink_device_mac = ld
                                .extra
                                .get("uplink")
                                .and_then(|u| u.get("uplink_mac"))
                                .and_then(|v| v.as_str())
                                .map(crate::model::entity_id::MacAddress::new);
                        }
                        // Fill CPU/Mem from Legacy sys_stats when Integration
                        // API didn't provide them (common for APs/switches).
                        if let Some(ref sys) = ld.sys_stats {
                            if device.stats.cpu_utilization_pct.is_none() {
                                device.stats.cpu_utilization_pct =
                                    sys.cpu.as_deref().and_then(|v| v.parse().ok());
                            }
                            if device.stats.memory_utilization_pct.is_none() {
                                #[allow(
                                    clippy::as_conversions,
                                    clippy::cast_precision_loss
                                )]
                                {
                                    device.stats.memory_utilization_pct =
                                        match (sys.mem_used, sys.mem_total) {
                                            (Some(used), Some(total)) if total > 0 => {
                                                Some((used as f64 / total as f64) * 100.0)
                                            }
                                            _ => None,
                                        };
                                }
                            }
                            if device.stats.load_average_1m.is_none() {
                                device.stats.load_average_1m =
                                    sys.load_1.as_deref().and_then(|v| v.parse().ok());
                            }
                            if device.stats.load_average_5m.is_none() {
                                device.stats.load_average_5m =
                                    sys.load_5.as_deref().and_then(|v| v.parse().ok());
                            }
                            if device.stats.load_average_15m.is_none() {
                                device.stats.load_average_15m =
                                    sys.load_15.as_deref().and_then(|v| v.parse().ok());
                            }
                        }
                        // Fill uptime from Legacy when Integration didn't provide it
                        if device.stats.uptime_secs.is_none() {
                            device.stats.uptime_secs =
                                ld.uptime.and_then(|u| u.try_into().ok());
                        }
                    }
                }
            }

            // Push health to DataStore
            if !legacy_health.is_empty() {
                self.inner
                    .store
                    .site_health
                    .send_modify(|h| *h = Arc::new(legacy_health));
            }

            self.inner
                .store
                .apply_integration_snapshot(crate::store::RefreshSnapshot {
                    devices,
                    networks,
                    wifi,
                    policies,
                    zones,
                    acls,
                    dns,
                    vouchers,
                    sites,
                    events: legacy_events,
                    traffic_matching_lists,
                });
        } else {
            // ── Legacy-only path ─────────────────────────────────
            drop(integration_guard);

            let legacy_guard = self.inner.legacy_client.lock().await;
            let legacy = legacy_guard
                .as_ref()
                .ok_or(CoreError::ControllerDisconnected)?;

            let (devices_res, events_res) = tokio::join!(
                legacy.list_devices(),
                legacy.list_events(Some(100)),
            );

            let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
            let events: Vec<Event> = events_res?.into_iter().map(Event::from).collect();

            drop(legacy_guard);

            for event in &events {
                let _ = self.inner.event_tx.send(Arc::new(event.clone()));
            }

            self.inner
                .store
                .apply_integration_snapshot(crate::store::RefreshSnapshot {
                    devices,
                    networks: Vec::new(),
                    wifi: Vec::new(),
                    policies: Vec::new(),
                    zones: Vec::new(),
                    acls: Vec::new(),
                    dns: Vec::new(),
                    vouchers: Vec::new(),
                    sites: Vec::new(),
                    events,
                    traffic_matching_lists: Vec::new(),
                });
        }

        debug!(
            devices = self.inner.store.device_count(),
            clients = self.inner.store.client_count(),
            "data refresh complete"
        );

        Ok(())
    }

    // ── Command execution ────────────────────────────────────────

    /// Execute a command against the controller.
    ///
    /// Sends the command through the internal channel to the command
    /// processor task and awaits the result.
    pub async fn execute(&self, cmd: Command) -> Result<CommandResult, CoreError> {
        if *self.inner.connection_state.borrow() != ConnectionState::Connected {
            return Err(CoreError::ControllerDisconnected);
        }

        let (tx, rx) = tokio::sync::oneshot::channel();

        let command_tx = self.inner.command_tx.lock().await.clone();

        command_tx
            .send(CommandEnvelope {
                command: cmd,
                response_tx: tx,
            })
            .await
            .map_err(|_| CoreError::ControllerDisconnected)?;

        rx.await.map_err(|_| CoreError::ControllerDisconnected)?
    }

    // ── One-shot convenience ─────────────────────────────────────

    /// One-shot: connect, run closure, disconnect.
    ///
    /// Optimized for CLI: disables WebSocket and periodic refresh since
    /// we only need a single request-response cycle.
    pub async fn oneshot<F, Fut, T>(config: ControllerConfig, f: F) -> Result<T, CoreError>
    where
        F: FnOnce(Controller) -> Fut,
        Fut: std::future::Future<Output = Result<T, CoreError>>,
    {
        let mut cfg = config;
        cfg.websocket_enabled = false;
        cfg.refresh_interval_secs = 0;

        let controller = Controller::new(cfg);
        controller.connect().await?;
        let result = f(controller.clone()).await;
        controller.disconnect().await;
        result
    }

    // ── State observation ────────────────────────────────────────

    /// Subscribe to connection state changes.
    pub fn connection_state(&self) -> watch::Receiver<ConnectionState> {
        self.inner.connection_state.subscribe()
    }

    /// Subscribe to the event broadcast stream.
    pub fn events(&self) -> broadcast::Receiver<Arc<Event>> {
        self.inner.event_tx.subscribe()
    }

    // ── Snapshot accessors (delegate to DataStore) ───────────────

    pub fn devices_snapshot(&self) -> Arc<Vec<Arc<Device>>> {
        self.inner.store.devices_snapshot()
    }

    pub fn clients_snapshot(&self) -> Arc<Vec<Arc<Client>>> {
        self.inner.store.clients_snapshot()
    }

    pub fn networks_snapshot(&self) -> Arc<Vec<Arc<Network>>> {
        self.inner.store.networks_snapshot()
    }

    pub fn wifi_broadcasts_snapshot(&self) -> Arc<Vec<Arc<WifiBroadcast>>> {
        self.inner.store.wifi_broadcasts_snapshot()
    }

    pub fn firewall_policies_snapshot(&self) -> Arc<Vec<Arc<FirewallPolicy>>> {
        self.inner.store.firewall_policies_snapshot()
    }

    pub fn firewall_zones_snapshot(&self) -> Arc<Vec<Arc<FirewallZone>>> {
        self.inner.store.firewall_zones_snapshot()
    }

    pub fn acl_rules_snapshot(&self) -> Arc<Vec<Arc<AclRule>>> {
        self.inner.store.acl_rules_snapshot()
    }

    pub fn dns_policies_snapshot(&self) -> Arc<Vec<Arc<DnsPolicy>>> {
        self.inner.store.dns_policies_snapshot()
    }

    pub fn vouchers_snapshot(&self) -> Arc<Vec<Arc<Voucher>>> {
        self.inner.store.vouchers_snapshot()
    }

    pub fn sites_snapshot(&self) -> Arc<Vec<Arc<Site>>> {
        self.inner.store.sites_snapshot()
    }

    pub fn events_snapshot(&self) -> Arc<Vec<Arc<Event>>> {
        self.inner.store.events_snapshot()
    }

    pub fn traffic_matching_lists_snapshot(&self) -> Arc<Vec<Arc<TrafficMatchingList>>> {
        self.inner.store.traffic_matching_lists_snapshot()
    }

    // ── Stream accessors (delegate to DataStore) ─────────────────

    pub fn devices(&self) -> EntityStream<Device> {
        self.inner.store.subscribe_devices()
    }

    pub fn clients(&self) -> EntityStream<Client> {
        self.inner.store.subscribe_clients()
    }

    pub fn networks(&self) -> EntityStream<Network> {
        self.inner.store.subscribe_networks()
    }

    pub fn wifi_broadcasts(&self) -> EntityStream<WifiBroadcast> {
        self.inner.store.subscribe_wifi_broadcasts()
    }

    pub fn firewall_policies(&self) -> EntityStream<FirewallPolicy> {
        self.inner.store.subscribe_firewall_policies()
    }

    pub fn firewall_zones(&self) -> EntityStream<FirewallZone> {
        self.inner.store.subscribe_firewall_zones()
    }

    pub fn acl_rules(&self) -> EntityStream<AclRule> {
        self.inner.store.subscribe_acl_rules()
    }

    pub fn dns_policies(&self) -> EntityStream<DnsPolicy> {
        self.inner.store.subscribe_dns_policies()
    }

    pub fn vouchers(&self) -> EntityStream<Voucher> {
        self.inner.store.subscribe_vouchers()
    }

    pub fn sites(&self) -> EntityStream<Site> {
        self.inner.store.subscribe_sites()
    }

    pub fn traffic_matching_lists(&self) -> EntityStream<TrafficMatchingList> {
        self.inner.store.subscribe_traffic_matching_lists()
    }

    /// Subscribe to site health updates (WAN IP, latency, bandwidth rates).
    pub fn site_health(&self) -> watch::Receiver<Arc<Vec<HealthSummary>>> {
        self.inner.store.subscribe_site_health()
    }

    /// Subscribe to monthly WAN usage updates (tx_bytes, rx_bytes).
    pub fn monthly_wan_bytes(&self) -> watch::Receiver<(u64, u64)> {
        self.inner.store.subscribe_monthly_wan_bytes()
    }

    /// Subscribe to per-client 24h usage updates: MAC -> (tx_bytes, rx_bytes).
    pub fn client_daily_usage(&self) -> watch::Receiver<Arc<HashMap<String, (u64, u64)>>> {
        self.inner.store.subscribe_client_daily_usage()
    }

    /// Drain warnings accumulated during connect (e.g. Legacy auth failure).
    pub async fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.inner.warnings.lock().await)
    }

    // ── Ad-hoc Integration API queries ───────────────────────────
    //
    // These bypass the DataStore and query the Integration API directly.
    // Intended for reference data that doesn't need reactive subscriptions.

    /// Fetch VPN servers from the Integration API.
    pub async fn list_vpn_servers(&self) -> Result<Vec<VpnServer>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "list_vpn_servers")?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_vpn_servers(&sid, off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|s| {
                let id = s
                    .fields
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .map_or_else(|| EntityId::Legacy("unknown".into()), EntityId::Uuid);
                VpnServer {
                    id,
                    name: s
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    server_type: s
                        .fields
                        .get("type")
                        .or_else(|| s.fields.get("serverType"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("UNKNOWN")
                        .to_owned(),
                    enabled: s.fields.get("enabled").and_then(serde_json::Value::as_bool),
                }
            })
            .collect())
    }

    /// Fetch VPN tunnels from the Integration API.
    pub async fn list_vpn_tunnels(&self) -> Result<Vec<VpnTunnel>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "list_vpn_tunnels")?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_vpn_tunnels(&sid, off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|t| {
                let id = t
                    .fields
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .map_or_else(|| EntityId::Legacy("unknown".into()), EntityId::Uuid);
                VpnTunnel {
                    id,
                    name: t
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    tunnel_type: t
                        .fields
                        .get("type")
                        .or_else(|| t.fields.get("tunnelType"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("UNKNOWN")
                        .to_owned(),
                    enabled: t.fields.get("enabled").and_then(serde_json::Value::as_bool),
                }
            })
            .collect())
    }

    /// Fetch WAN interfaces from the Integration API.
    pub async fn list_wans(&self) -> Result<Vec<WanInterface>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "list_wans")?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_wans(&sid, off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|w| {
                let id = w
                    .fields
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .map_or_else(|| EntityId::Legacy("unknown".into()), EntityId::Uuid);
                let parse_ip = |key: &str| -> Option<std::net::IpAddr> {
                    w.fields
                        .get(key)
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                };
                let dns = w
                    .fields
                    .get("dns")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().and_then(|s| s.parse().ok()))
                            .collect()
                    })
                    .unwrap_or_default();
                WanInterface {
                    id,
                    name: w
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    ip: parse_ip("ipAddress").or_else(|| parse_ip("ip")),
                    gateway: parse_ip("gateway"),
                    dns,
                }
            })
            .collect())
    }

    /// Fetch DPI categories from the Integration API.
    pub async fn list_dpi_categories(&self) -> Result<Vec<DpiCategory>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "list_dpi_categories")?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_dpi_categories(&sid, off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|c| {
                #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
                let id = c
                    .fields
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                DpiCategory {
                    id,
                    name: c
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_owned(),
                    tx_bytes: c
                        .fields
                        .get("txBytes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    rx_bytes: c
                        .fields
                        .get("rxBytes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    apps: Vec::new(),
                }
            })
            .collect())
    }

    /// Fetch DPI applications from the Integration API.
    pub async fn list_dpi_applications(&self) -> Result<Vec<DpiApplication>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "list_dpi_applications")?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_dpi_applications(&sid, off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|a| {
                #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
                let id = a
                    .fields
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                DpiApplication {
                    id,
                    name: a
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_owned(),
                    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
                    category_id: a
                        .fields
                        .get("categoryId")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u32,
                    tx_bytes: a
                        .fields
                        .get("txBytes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    rx_bytes: a
                        .fields
                        .get("rxBytes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                }
            })
            .collect())
    }

    /// Fetch RADIUS profiles from the Integration API.
    pub async fn list_radius_profiles(&self) -> Result<Vec<RadiusProfile>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "list_radius_profiles")?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_radius_profiles(&sid, off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|r| {
                let id = r
                    .fields
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .map_or_else(|| EntityId::Legacy("unknown".into()), EntityId::Uuid);
                RadiusProfile {
                    id,
                    name: r
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_owned(),
                }
            })
            .collect())
    }

    /// Fetch countries from the Integration API.
    pub async fn list_countries(&self) -> Result<Vec<Country>, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let ic = guard
            .as_ref()
            .ok_or_else(|| unsupported("list_countries"))?;
        let raw = ic
            .paginate_all(200, |off, lim| ic.list_countries(off, lim))
            .await?;
        Ok(raw
            .into_iter()
            .map(|c| Country {
                code: c
                    .fields
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                name: c
                    .fields
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_owned(),
            })
            .collect())
    }

    /// Fetch references for a specific network (Integration API).
    pub async fn get_network_references(
        &self,
        network_id: &EntityId,
    ) -> Result<serde_json::Value, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "get_network_references")?;
        let uuid = require_uuid(network_id)?;
        let refs = ic.get_network_references(&sid, &uuid).await?;
        Ok(serde_json::to_value(refs).unwrap_or_default())
    }

    /// Fetch firewall policy ordering (Integration API).
    pub async fn get_firewall_policy_ordering(
        &self,
    ) -> Result<unifly_api::integration_types::FirewallPolicyOrdering, CoreError> {
        let guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        let (ic, sid) = require_integration(&guard, site_id, "get_firewall_policy_ordering")?;
        Ok(ic.get_firewall_policy_ordering(&sid).await?)
    }

    /// List pending devices.
    ///
    /// Prefers Integration API pending endpoint, falls back to filtering
    /// the canonical device snapshot by pending adoption state.
    pub async fn list_pending_devices(&self) -> Result<Vec<serde_json::Value>, CoreError> {
        let integration_guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
            let raw = ic
                .paginate_all(200, |off, lim| ic.list_pending_devices(&sid, off, lim))
                .await?;
            return Ok(raw
                .into_iter()
                .map(|v| serde_json::to_value(v).unwrap_or_default())
                .collect());
        }

        let snapshot = self.devices_snapshot();
        Ok(snapshot
            .iter()
            .filter(|d| d.state == crate::model::DeviceState::PendingAdoption)
            .map(|d| serde_json::to_value(d.as_ref()).unwrap_or_default())
            .collect())
    }

    /// List device tags.
    ///
    /// Uses Integration API when available.
    pub async fn list_device_tags(&self) -> Result<Vec<serde_json::Value>, CoreError> {
        let integration_guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;
        if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
            let raw = ic
                .paginate_all(200, |off, lim| ic.list_device_tags(&sid, off, lim))
                .await?;
            return Ok(raw
                .into_iter()
                .map(|v| serde_json::to_value(v).unwrap_or_default())
                .collect());
        }

        Ok(Vec::new())
    }

    /// List controller backups (legacy API).
    pub async fn list_backups(&self) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.list_backups().await?)
    }

    /// Download a controller backup file (legacy API).
    pub async fn download_backup(&self, filename: &str) -> Result<Vec<u8>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.download_backup(filename).await?)
    }

    // ── Statistics (Legacy API) ────────────────────────────────────

    /// Fetch site-level historical statistics.
    pub async fn get_site_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_site_stats(interval, start, end, attrs).await?)
    }

    /// Fetch per-device historical statistics.
    pub async fn get_device_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_device_stats(interval, macs, attrs).await?)
    }

    /// Fetch per-client historical statistics.
    pub async fn get_client_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_client_stats(interval, macs, attrs).await?)
    }

    /// Fetch gateway historical statistics.
    pub async fn get_gateway_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
        attrs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy
            .get_gateway_stats(interval, start, end, attrs)
            .await?)
    }

    /// Fetch DPI statistics.
    pub async fn get_dpi_stats(
        &self,
        group_by: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_dpi_stats(group_by, macs).await?)
    }

    // ── Ad-hoc Legacy API queries ──────────────────────────────────
    //
    // Legacy-only data that doesn't live in the DataStore.

    /// Fetch admin list from the Legacy API.
    pub async fn list_admins(&self) -> Result<Vec<Admin>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        let raw = legacy.list_admins().await?;
        Ok(raw
            .into_iter()
            .map(|v| Admin {
                id: v.get("_id").and_then(|v| v.as_str()).map_or_else(
                    || EntityId::Legacy("unknown".into()),
                    |s| EntityId::Legacy(s.into()),
                ),
                name: v
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                email: v.get("email").and_then(|v| v.as_str()).map(String::from),
                role: v
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_owned(),
                is_super: v
                    .get("is_super")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                last_login: None,
            })
            .collect())
    }

    /// Fetch alarms from the Legacy API.
    pub async fn list_alarms(&self) -> Result<Vec<Alarm>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        let raw = legacy.list_alarms().await?;
        Ok(raw.into_iter().map(Alarm::from).collect())
    }

    /// Fetch controller system info.
    ///
    /// Prefers the Integration API (`GET /v1/info`) when available,
    /// falls back to Legacy `stat/sysinfo`.
    pub async fn get_system_info(&self) -> Result<SystemInfo, CoreError> {
        // Try Integration API first (works with API key auth).
        {
            let guard = self.inner.integration_client.lock().await;
            if let Some(ic) = guard.as_ref() {
                let info = ic.get_info().await?;
                let f = &info.fields;
                return Ok(SystemInfo {
                    controller_name: f
                        .get("applicationName")
                        .or_else(|| f.get("name"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    version: f
                        .get("applicationVersion")
                        .or_else(|| f.get("version"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_owned(),
                    build: f.get("build").and_then(|v| v.as_str()).map(String::from),
                    hostname: f.get("hostname").and_then(|v| v.as_str()).map(String::from),
                    ip: None, // Not available via Integration API
                    uptime_secs: f.get("uptime").and_then(serde_json::Value::as_u64),
                    update_available: f
                        .get("isUpdateAvailable")
                        .or_else(|| f.get("update_available"))
                        .and_then(serde_json::Value::as_bool),
                });
            }
        }

        // Fallback to Legacy API (requires session auth).
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        let raw = legacy.get_sysinfo().await?;
        Ok(SystemInfo {
            controller_name: raw
                .get("controller_name")
                .or_else(|| raw.get("name"))
                .and_then(|v| v.as_str())
                .map(String::from),
            version: raw
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            build: raw.get("build").and_then(|v| v.as_str()).map(String::from),
            hostname: raw
                .get("hostname")
                .and_then(|v| v.as_str())
                .map(String::from),
            ip: raw
                .get("ip_addrs")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok()),
            uptime_secs: raw.get("uptime").and_then(serde_json::Value::as_u64),
            update_available: raw
                .get("update_available")
                .and_then(serde_json::Value::as_bool),
        })
    }

    /// Fetch site health dashboard from the Legacy API.
    pub async fn get_site_health(&self) -> Result<Vec<HealthSummary>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        let raw = legacy.get_health().await?;
        Ok(convert_health_summaries(raw))
    }

    /// Fetch low-level sysinfo from the Legacy API.
    pub async fn get_sysinfo(&self) -> Result<SysInfo, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        let raw = legacy.get_sysinfo().await?;
        Ok(SysInfo {
            timezone: raw
                .get("timezone")
                .and_then(|v| v.as_str())
                .map(String::from),
            autobackup: raw.get("autobackup").and_then(serde_json::Value::as_bool),
            hostname: raw
                .get("hostname")
                .and_then(|v| v.as_str())
                .map(String::from),
            ip_addrs: raw
                .get("ip_addrs")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            live_chat: raw
                .get("live_chat")
                .and_then(|v| v.as_str())
                .map(String::from),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            data_retention_days: raw
                .get("data_retention_days")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as u32),
            extra: raw,
        })
    }
}

// ── Background tasks ─────────────────────────────────────────────

/// Apply a `device:sync` WebSocket message to the DataStore.
///
/// Extracts CPU, memory, load averages, and uplink bandwidth from the
/// raw Legacy API device JSON. Merges stats into the existing device
/// (looked up by MAC) without clobbering Integration API fields.
#[allow(clippy::cast_precision_loss)]
/// Apply a `sta:sync` WebSocket message — upserts a single client with live data.
fn apply_sta_sync(store: &DataStore, data: &serde_json::Value) {
    // Parse the sta:sync JSON as a LegacyClientEntry (same schema as stat/sta)
    let entry: unifly_api::legacy::models::LegacyClientEntry = match serde_json::from_value(data.clone()) {
        Ok(e) => e,
        Err(_) => return,
    };
    let client = Client::from(entry);
    let key = client.mac.as_str().to_owned();
    let id = client.id.clone();
    store.clients.upsert(key, id, client);
}

fn apply_device_sync(
    stats_tx: &mpsc::UnboundedSender<DeviceStatsUpdate>,
    data: &serde_json::Value,
) {
    let Some(mac_str) = data.get("mac").and_then(serde_json::Value::as_str) else {
        return;
    };
    let mac = MacAddress::new(mac_str);

    // Parse sys_stats (UDM Pro) or system-stats (APs/switches)
    let sys = data.get("sys_stats");
    let system_stats = data.get("system-stats");

    // CPU: try sys_stats.cpu, then system-stats.cpu
    let cpu = sys
        .and_then(|s| s.get("cpu"))
        .or_else(|| system_stats.and_then(|s| s.get("cpu")))
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| v.as_f64())
        });

    // Memory: try sys_stats mem_used/mem_total (raw), then system-stats.mem (percentage string)
    #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
    let mem_pct = match (
        sys.and_then(|s| s.get("mem_used"))
            .and_then(serde_json::Value::as_i64),
        sys.and_then(|s| s.get("mem_total"))
            .and_then(serde_json::Value::as_i64),
    ) {
        (Some(used), Some(total)) if total > 0 => Some((used as f64 / total as f64) * 100.0),
        _ => system_stats
            .and_then(|s| s.get("mem"))
            .and_then(|v| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64())),
    };
    let load_averages: [Option<f64>; 3] = [
        sys.and_then(|s| s.get("loadavg_1")).and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_f64())
        }),
        sys.and_then(|s| s.get("loadavg_5")).and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_f64())
        }),
        sys.and_then(|s| s.get("loadavg_15")).and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_f64())
        }),
    ];

    // Uplink bandwidth: check "uplink" object or top-level fields
    let uplink = data.get("uplink");
    let tx_bps = uplink
        .and_then(|u| u.get("tx_bytes-r").or_else(|| u.get("tx_bytes_r")))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| data.get("tx_bytes-r").and_then(serde_json::Value::as_u64));
    let rx_bps = uplink
        .and_then(|u| u.get("rx_bytes-r").or_else(|| u.get("rx_bytes_r")))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| data.get("rx_bytes-r").and_then(serde_json::Value::as_u64));

    let bandwidth = match (tx_bps, rx_bps) {
        (Some(tx), Some(rx)) if tx > 0 || rx > 0 => Some(crate::model::common::Bandwidth {
            tx_bytes_per_sec: tx,
            rx_bytes_per_sec: rx,
        }),
        _ => None,
    };

    // Uptime from top-level `_uptime` or `uptime`
    let uptime = data
        .get("_uptime")
        .or_else(|| data.get("uptime"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|u| u.try_into().ok());

    // Client count from num_sta
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let client_count = data
        .get("num_sta")
        .and_then(serde_json::Value::as_u64)
        .map(|n| n as u32);

    // WAN IPv6
    let wan_ipv6 = data
        .as_object()
        .and_then(parse_legacy_device_wan_ipv6);

    // Uplink device MAC
    let uplink_device_mac = uplink
        .and_then(|u| u.get("uplink_mac"))
        .and_then(serde_json::Value::as_str)
        .map(MacAddress::new);

    let _ = stats_tx.send(DeviceStatsUpdate {
        mac,
        stats: crate::model::device::DeviceStats {
            cpu_utilization_pct: cpu,
            memory_utilization_pct: mem_pct,
            load_average_1m: load_averages[0],
            load_average_5m: load_averages[1],
            load_average_15m: load_averages[2],
            uplink_bandwidth: bandwidth,
            uptime_secs: uptime,
            last_heartbeat: None,
            next_heartbeat: None,
        },
        client_count,
        wan_ipv6,
        uplink_device_mac,
    });
}

/// Single consumer for all device stats updates.
///
/// Receives partial [`DeviceStatsUpdate`] messages from every source
/// (Integration API, Legacy API, WebSocket, health poller) and merges
/// them into the device store. Only `Some` fields are overwritten —
/// this eliminates race conditions between concurrent poll tasks.
async fn stats_merge_task(
    store: Arc<DataStore>,
    mut rx: mpsc::UnboundedReceiver<DeviceStatsUpdate>,
) {
    tracing::info!("stats_merge_task started — waiting for updates");
    while let Some(update) = rx.recv().await {
        let Some(existing) = store.device_by_mac(&update.mac) else {
            tracing::debug!("stats_merge: unknown MAC {}", update.mac.as_str());
            continue;
        };
        let name = existing.name.as_deref().unwrap_or("?");
        tracing::debug!(
            "stats_merge: {} cpu={:?} mem={:?} bw={:?}",
            name,
            update.stats.cpu_utilization_pct,
            update.stats.memory_utilization_pct,
            update.stats.uplink_bandwidth,
        );
        let mut device = (*existing).clone();
        device.stats.merge(&update.stats);
        if let Some(count) = update.client_count {
            device.client_count = Some(count);
        }
        if let Some(ref ipv6) = update.wan_ipv6 {
            device.wan_ipv6 = Some(ipv6.clone());
        }
        if let Some(ref uplink_mac) = update.uplink_device_mac {
            device.uplink_device_mac = Some(uplink_mac.clone());
        }
        let key = update.mac.as_str().to_owned();
        let id = device.id.clone();
        store.devices.upsert(key, id, device);
    }
}

/// Lightweight health-only poller for live bandwidth rates.
///
/// Fetches `stat/health` (~5 JSON objects) at a fast cadence (default 2s)
/// and pushes to `site_health`, which the TUI observes for real-time
/// WAN traffic chart updates.
async fn health_poll_task(controller: Controller, period: Duration, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(period);
    interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                tracing::debug!("health_poll_task tick");
                // Clone legacy client early and release lock before async call
                let legacy = {
                    let guard = controller.inner.legacy_client.lock().await;
                    match &*guard {
                        Some(l) => l.clone(),
                        None => {
                            debug!("health_poll: no legacy client — exiting");
                            break;
                        }
                    }
                };

                match legacy.get_health().await {
                    Ok(raw) => {
                        // Extract gateway CPU/RAM from raw health and send via stats channel
                        for entry in &raw {
                            if let Some(sub) = entry.get("subsystem").and_then(|s| s.as_str()) {
                                if sub == "wan" {
                                    if let Some(sys) = entry.get("gw_system-stats") {
                                        let cpu = sys.get("cpu").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
                                        let mem = sys.get("mem").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok());
                                        if cpu.is_some() || mem.is_some() {
                                            if let Some(mac) = entry.get("gw_mac").and_then(|m| m.as_str()) {
                                                let _ = controller.inner.stats_tx.send(DeviceStatsUpdate {
                                                    mac: MacAddress::new(mac),
                                                    stats: crate::model::device::DeviceStats {
                                                        cpu_utilization_pct: cpu,
                                                        memory_utilization_pct: mem,
                                                        ..Default::default()
                                                    },
                                                    client_count: None,
                                                    wan_ipv6: None,
                                                    uplink_device_mac: None,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let health = convert_health_summaries(raw);
                        if !health.is_empty() {
                            controller.inner.store.site_health.send_modify(|h| *h = Arc::new(health));
                        }
                    }
                    Err(e) => {
                        debug!(error = %e, "health_poll: fetch failed");
                    }
                }
            }
        }
    }
}

/// Fallback poll for client list — handles stale client cleanup.
/// Primary client data comes from WebSocket `sta:sync` events in real-time.
async fn client_poll_task(controller: Controller, period: Duration, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(period);
    interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                tracing::debug!("client_poll_task tick (fallback)");
                let legacy = {
                    let guard = controller.inner.legacy_client.lock().await;
                    match &*guard {
                        Some(l) => l.clone(),
                        None => {
                            debug!("client_poll: no legacy client — will retry");
                            continue;
                        }
                    }
                };

                match legacy.list_clients().await {
                    Ok(raw) => {
                        let clients: Vec<Client> = raw.into_iter().map(Client::from).collect();

                        let items: Vec<(String, EntityId, Client)> = clients
                            .into_iter()
                            .map(|c| {
                                let key = c.mac.as_str().to_owned();
                                let id = c.id.clone();
                                (key, id, c)
                            })
                            .collect();
                        let col = &controller.inner.store.clients;
                        let incoming_keys: std::collections::HashSet<String> =
                            items.iter().map(|(k, _, _)| k.clone()).collect();
                        for (key, id, entity) in items {
                            col.upsert_silent(key, id, entity);
                        }
                        let stale: Vec<String> = col.keys()
                            .into_iter()
                            .filter(|k| !incoming_keys.contains(k))
                            .collect();
                        for key in &stale {
                            col.remove(key);
                        }
                        if stale.is_empty() {
                            col.flush();
                        }
                    }
                    Err(e) => {
                        debug!(error = %e, "client_poll: fetch failed");
                    }
                }
            }
        }
    }
}

/// Periodically poll per-device statistics from the Integration API,
/// with Legacy API fallback for fields the Integration API doesn't
/// provide (e.g. AP CPU/Mem/bandwidth).
///
/// All stats are sent through the stats channel — the merge task
/// handles applying them to the store without race conditions.
#[allow(clippy::too_many_lines)]
async fn device_stats_poll_task(
    controller: Controller,
    period: Duration,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(period);
    interval.tick().await;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                let integration = {
                    let guard = controller.inner.integration_client.lock().await;
                    match &*guard {
                        Some(int) => int.clone(),
                        None => continue,
                    }
                };

                let site_guard = controller.inner.site_id.lock().await;
                let Some(ref sid) = *site_guard else { continue };
                let sid = *sid;
                drop(site_guard);

                let devices = controller.inner.store.devices_snapshot();

                // Fetch Integration API stats for all devices in parallel
                let futs = devices.iter().filter_map(|dev| {
                    if let crate::model::EntityId::Uuid(ref uuid) = dev.id {
                        let uuid = *uuid;
                        let mac = dev.mac.clone();
                        let integration = integration.clone();
                        Some(async move {
                            let result = integration.get_device_statistics(&sid, &uuid).await;
                            (mac, result)
                        })
                    } else {
                        None
                    }
                });
                let results: Vec<_> = futures_util::future::join_all(futs).await;

                // Send Integration API stats through channel; track devices needing Legacy fallback
                let mut legacy_needed_macs: Vec<MacAddress> = Vec::new();
                for (mac, result) in &results {
                    if let Ok(stats_resp) = result {
                        let stats = crate::convert::device_stats_from_integration(stats_resp);

                        if stats.cpu_utilization_pct.is_none()
                            || stats.memory_utilization_pct.is_none()
                            || stats.uplink_bandwidth.is_none()
                        {
                            legacy_needed_macs.push(mac.clone());
                        }

                        let _ = controller.inner.stats_tx.send(DeviceStatsUpdate {
                            mac: mac.clone(),
                            stats,
                            client_count: None,
                            wan_ipv6: None,
                            uplink_device_mac: None,
                        });
                    }
                }

                // Legacy fallback for devices missing CPU/mem/bandwidth
                if !legacy_needed_macs.is_empty() {
                    let legacy_opt = {
                        let guard = controller.inner.legacy_client.lock().await;
                        guard.as_ref().cloned()
                    };
                    if let Some(legacy) = legacy_opt {
                        // Fetch list_devices for CPU/mem/uptime + bandwidth from extra fields
                        if let Ok(legacy_devices) = legacy.list_devices().await {
                            let legacy_by_mac: HashMap<&str, &unifly_api::legacy::models::LegacyDevice> =
                                legacy_devices.iter().map(|d| (d.mac.as_str(), d)).collect();

                            for mac in &legacy_needed_macs {
                                if let Some(ld) = legacy_by_mac.get(mac.as_str()) {
                                    let mut stats = crate::model::device::DeviceStats::default();

                                    #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
                                    if let Some(ref sys) = ld.sys_stats {
                                        stats.cpu_utilization_pct = sys.cpu.as_deref().and_then(|v| v.parse().ok());
                                        stats.memory_utilization_pct = match (sys.mem_used, sys.mem_total) {
                                            (Some(used), Some(total)) if total > 0 => {
                                                Some((used as f64 / total as f64) * 100.0)
                                            }
                                            _ => None,
                                        };
                                        stats.load_average_1m = sys.load_1.as_deref().and_then(|v| v.parse().ok());
                                        stats.load_average_5m = sys.load_5.as_deref().and_then(|v| v.parse().ok());
                                        stats.load_average_15m = sys.load_15.as_deref().and_then(|v| v.parse().ok());
                                    }
                                    stats.uptime_secs = ld.uptime.and_then(|u| u.try_into().ok());

                                    // Bandwidth from extra fields
                                    let tx = ld.extra.get("tx_bytes-r").and_then(|v| v.as_f64());
                                    let rx = ld.extra.get("rx_bytes-r").and_then(|v| v.as_f64());
                                    #[allow(clippy::as_conversions)]
                                    if tx.is_some() || rx.is_some() {
                                        stats.uplink_bandwidth = Some(crate::model::common::Bandwidth {
                                            tx_bytes_per_sec: tx.map(|v| v as u64).unwrap_or(0),
                                            rx_bytes_per_sec: rx.map(|v| v as u64).unwrap_or(0),
                                        });
                                    }

                                    let _ = controller.inner.stats_tx.send(DeviceStatsUpdate {
                                        mac: mac.clone(),
                                        stats,
                                        client_count: None,
                                        wan_ipv6: None,
                                        uplink_device_mac: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Periodically fetch monthly WAN usage from daily site stats.
async fn monthly_stats_task(controller: Controller, cancel: CancellationToken) {
    use chrono::{Datelike, Utc};

    // Fetch once immediately, then every 60 seconds
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                // Clone and release lock before API call
                let legacy = {
                    let guard = controller.inner.legacy_client.lock().await;
                    match guard.as_ref() {
                        Some(l) => l.clone(),
                        None => {
                            debug!("monthly_stats: no legacy client — exiting");
                            break;
                        }
                    }
                };

                // Current month start as Unix millis
                let now = Utc::now();
                let month_start = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp_millis();
                let now_ms = now.timestamp_millis();

                let attrs: Vec<String> = ["wan-tx_bytes", "wan-rx_bytes", "time"]
                    .iter().map(|s| (*s).to_string()).collect();

                match legacy.get_site_stats("daily", Some(month_start), Some(now_ms), Some(&attrs)).await {
                    Ok(entries) => {
                        // Helper: JSON numbers may be float or int
                        let as_u64 = |v: &serde_json::Value| -> Option<u64> {
                            v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))
                        };
                        let mut total_tx = 0u64;
                        let mut total_rx = 0u64;
                        for entry in &entries {
                            if let Some(tx) = entry.get("wan-tx_bytes").and_then(as_u64) {
                                total_tx = total_tx.saturating_add(tx);
                            }
                            if let Some(rx) = entry.get("wan-rx_bytes").and_then(as_u64) {
                                total_rx = total_rx.saturating_add(rx);
                            }
                        }
                        controller.inner.store.monthly_wan_bytes.send_modify(|v| *v = (total_tx, total_rx));
                        info!(tx = total_tx, rx = total_rx, days = entries.len(), "monthly WAN usage updated");
                    }
                    Err(e) => {
                        warn!(error = %e, "monthly_stats: fetch failed");
                    }
                }
            }
        }
    }
}

/// Fetch per-client 24h usage from `stat/report/daily.user` every 60s.
///
/// The stat/report endpoint aggregates historical data — polling it more
/// frequently than ~60s returns the same data since the controller only
/// writes new report rows every few minutes.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation, clippy::as_conversions)]
async fn client_daily_usage_task(controller: Controller, cancel: CancellationToken) {
    use chrono::Utc;

    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                // Clone and release lock before API call
                let legacy = {
                    let guard = controller.inner.legacy_client.lock().await;
                    match guard.as_ref() {
                        Some(l) => l.clone(),
                        None => {
                            debug!("client_daily_usage: no legacy client yet, will retry");
                            continue;
                        }
                    }
                };

                let now_ms = Utc::now().timestamp_millis();
                let start_ms = now_ms - 86_400_000; // 24h ago

                let result = legacy.get_client_daily_usage(start_ms).await;

                match result {
                    Ok(entries) => {
                        let as_u64 = |v: &serde_json::Value| -> Option<u64> {
                            v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))
                        };

                        let mut usage: HashMap<String, (u64, u64)> = HashMap::new();
                        for entry in &entries {
                            // MAC is in the "user" field for daily.user reports
                            let Some(mac) = entry.get("user").and_then(|v| v.as_str()) else {
                                continue;
                            };
                            let tx = entry.get("tx_bytes").and_then(as_u64).unwrap_or(0);
                            let rx = entry.get("rx_bytes").and_then(as_u64).unwrap_or(0);
                            let e = usage.entry(mac.to_lowercase()).or_insert((0, 0));
                            e.0 = e.0.saturating_add(tx);
                            e.1 = e.1.saturating_add(rx);
                        }
                        let count = usage.len();
                        controller.inner.store.client_daily_usage.send_modify(|v| *v = Arc::new(usage));
                        info!(clients = count, "client daily usage updated");
                    }
                    Err(e) => {
                        warn!(error = %e, "client_daily_usage: fetch failed");
                    }
                }
            }
        }
    }
}

/// Periodically refresh data from the controller.
async fn refresh_task(controller: Controller, interval_secs: u64, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            _ = interval.tick() => {
                if let Err(e) = controller.full_refresh().await {
                    warn!(error = %e, "periodic refresh failed");
                }
            }
        }
    }
}

/// Process commands from the mpsc channel, routing each to the
/// appropriate Legacy API call.
async fn command_processor_task(controller: Controller, mut rx: mpsc::Receiver<CommandEnvelope>) {
    let cancel = controller.inner.cancel_child.lock().await.clone();

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            envelope = rx.recv() => {
                let Some(envelope) = envelope else { break };
                let result = route_command(&controller, envelope.command).await;
                let _ = envelope.response_tx.send(result);
            }
        }
    }
}

// ── Command routing ──────────────────────────────────────────────

/// Route a command to the appropriate API call.
///
/// Uses the Integration API for CRUD operations when available,
/// falls back to the Legacy API for session-based commands.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
async fn route_command(controller: &Controller, cmd: Command) -> Result<CommandResult, CoreError> {
    let store = &controller.inner.store;

    // Acquire both clients for routing decisions
    let integration_guard = controller.inner.integration_client.lock().await;
    let legacy_guard = controller.inner.legacy_client.lock().await;
    let site_id = *controller.inner.site_id.lock().await;

    match cmd {
        // ── Device operations ────────────────────────────────────
        Command::AdoptDevice {
            mac,
            ignore_device_limit,
        } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                ic.adopt_device(&sid, mac.as_str(), ignore_device_limit)
                    .await?;
            } else {
                let legacy = require_legacy(&legacy_guard)?;
                legacy.adopt_device(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::RestartDevice { id } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                let device_uuid = require_uuid(&id)?;
                ic.device_action(&sid, &device_uuid, "RESTART").await?;
            } else {
                let legacy = require_legacy(&legacy_guard)?;
                let mac = device_mac(store, &id)?;
                legacy.restart_device(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::LocateDevice { mac, enable } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                let device =
                    store
                        .device_by_mac(&mac)
                        .ok_or_else(|| CoreError::DeviceNotFound {
                            identifier: mac.to_string(),
                        })?;
                let device_uuid = require_uuid(&device.id)?;
                let action = if enable { "LOCATE_ON" } else { "LOCATE_OFF" };
                ic.device_action(&sid, &device_uuid, action).await?;
            } else {
                let legacy = require_legacy(&legacy_guard)?;
                legacy.locate_device(mac.as_str(), enable).await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::UpgradeDevice { mac, firmware_url } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy
                .upgrade_device(mac.as_str(), firmware_url.as_deref())
                .await?;
            Ok(CommandResult::Ok)
        }

        Command::RemoveDevice { id } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "RemoveDevice")?;
            let device_uuid = require_uuid(&id)?;
            ic.remove_device(&sid, &device_uuid).await?;
            Ok(CommandResult::Ok)
        }

        Command::ProvisionDevice { mac } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.provision_device(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }
        Command::SpeedtestDevice => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.speedtest().await?;
            Ok(CommandResult::Ok)
        }

        Command::PowerCyclePort {
            device_id,
            port_idx,
        } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "PowerCyclePort")?;
            let device_uuid = require_uuid(&device_id)?;
            ic.port_action(&sid, &device_uuid, port_idx, "POWER_CYCLE")
                .await?;
            Ok(CommandResult::Ok)
        }

        // ── Client operations ────────────────────────────────────
        Command::BlockClient { mac } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                let client =
                    store
                        .client_by_mac(&mac)
                        .ok_or_else(|| CoreError::ClientNotFound {
                            identifier: mac.to_string(),
                        })?;
                let client_uuid = require_uuid(&client.id)?;
                ic.client_action(&sid, &client_uuid, "BLOCK").await?;
            } else {
                let legacy = require_legacy(&legacy_guard)?;
                legacy.block_client(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::UnblockClient { mac } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                let client =
                    store
                        .client_by_mac(&mac)
                        .ok_or_else(|| CoreError::ClientNotFound {
                            identifier: mac.to_string(),
                        })?;
                let client_uuid = require_uuid(&client.id)?;
                ic.client_action(&sid, &client_uuid, "UNBLOCK").await?;
            } else {
                let legacy = require_legacy(&legacy_guard)?;
                legacy.unblock_client(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::KickClient { mac } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                let client =
                    store
                        .client_by_mac(&mac)
                        .ok_or_else(|| CoreError::ClientNotFound {
                            identifier: mac.to_string(),
                        })?;
                let client_uuid = require_uuid(&client.id)?;
                ic.client_action(&sid, &client_uuid, "RECONNECT").await?;
            } else {
                let legacy = require_legacy(&legacy_guard)?;
                legacy.kick_client(mac.as_str()).await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::ForgetClient { mac } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.forget_client(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::AuthorizeGuest {
            client_id,
            time_limit_minutes,
            data_limit_mb,
            rx_rate_kbps,
            tx_rate_kbps,
        } => {
            let legacy = require_legacy(&legacy_guard)?;
            let mac = client_mac(store, &client_id)?;
            let minutes = time_limit_minutes.unwrap_or(60);
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            {
                legacy
                    .authorize_guest(
                        mac.as_str(),
                        minutes,
                        tx_rate_kbps.map(|r| r as u32),
                        rx_rate_kbps.map(|r| r as u32),
                        data_limit_mb.map(|m| m as u32),
                    )
                    .await?;
            }
            Ok(CommandResult::Ok)
        }

        Command::UnauthorizeGuest { client_id } => {
            let legacy = require_legacy(&legacy_guard)?;
            let mac = client_mac(store, &client_id)?;
            legacy.unauthorize_guest(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        // ── Alarm operations ─────────────────────────────────────
        Command::ArchiveAlarm { id } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.archive_alarm(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }

        Command::ArchiveAllAlarms => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.archive_all_alarms().await?;
            Ok(CommandResult::Ok)
        }

        // ── Backup operations ────────────────────────────────────
        Command::CreateBackup => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.create_backup().await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteBackup { filename } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.delete_backup(&filename).await?;
            Ok(CommandResult::Ok)
        }

        // ── Network CRUD (Integration API) ───────────────────────
        Command::CreateNetwork(req) => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "CreateNetwork")?;
            let crate::command::CreateNetworkRequest {
                name,
                vlan_id,
                subnet,
                management,
                purpose,
                dhcp_enabled,
                enabled,
                dhcp_range_start,
                dhcp_range_stop,
                dhcp_lease_time,
                firewall_zone_id,
                isolation_enabled,
                internet_access_enabled,
            } = req;

            let management = management.unwrap_or_else(|| {
                if matches!(purpose, Some(NetworkPurpose::VlanOnly)) {
                    NetworkManagement::Unmanaged
                } else if purpose.is_some() || subnet.is_some() || dhcp_enabled {
                    NetworkManagement::Gateway
                } else {
                    NetworkManagement::Unmanaged
                }
            });
            let mut extra = HashMap::new();

            if let Some(zone) = firewall_zone_id {
                extra.insert("zoneId".into(), serde_json::Value::String(zone));
            }

            if matches!(management, NetworkManagement::Gateway) {
                extra.insert(
                    "isolationEnabled".into(),
                    serde_json::Value::Bool(isolation_enabled),
                );
                extra.insert(
                    "internetAccessEnabled".into(),
                    serde_json::Value::Bool(internet_access_enabled),
                );

                if let Some(cidr) = subnet {
                    let (host_ip, prefix_len) = parse_ipv4_cidr(&cidr)?;
                    let mut dhcp_cfg = serde_json::Map::new();
                    dhcp_cfg.insert(
                        "mode".into(),
                        serde_json::Value::String(
                            if dhcp_enabled { "SERVER" } else { "NONE" }.into(),
                        ),
                    );
                    if let Some(lease) = dhcp_lease_time {
                        dhcp_cfg.insert(
                            "leaseTimeSeconds".into(),
                            serde_json::Value::Number(serde_json::Number::from(u64::from(lease))),
                        );
                    }

                    if let (Some(start), Some(stop)) = (dhcp_range_start, dhcp_range_stop) {
                        dhcp_cfg.insert(
                            "ipAddressRange".into(),
                            serde_json::json!({
                                "start": start,
                                "end": stop
                            }),
                        );
                    }

                    extra.insert(
                        "ipv4Configuration".into(),
                        serde_json::json!({
                            "hostIpAddress": host_ip.to_string(),
                            "prefixLength": u64::from(prefix_len),
                            "dhcpConfiguration": dhcp_cfg
                        }),
                    );
                }
            }

            let body = unifly_api::integration_types::NetworkCreateUpdate {
                name,
                enabled,
                management: "USER_DEFINED".into(),
                vlan_id: vlan_id.map_or(1, i32::from),
                dhcp_guarding: None,
                extra,
            };
            ic.create_network(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateNetwork { id, update } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "UpdateNetwork")?;
            let uuid = require_uuid(&id)?;
            // Fetch existing to merge partial update
            let existing = ic.get_network(&sid, &uuid).await?;
            // Start from existing extra fields, then apply toggle overrides
            let mut extra = existing.extra;
            if let Some(v) = update.isolation_enabled {
                extra.insert("isolationEnabled".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = update.internet_access_enabled {
                extra.insert("internetAccessEnabled".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = update.mdns_forwarding_enabled {
                extra.insert("mdnsForwardingEnabled".into(), serde_json::Value::Bool(v));
            }
            if let Some(v) = update.ipv6_enabled {
                if v {
                    // Enable IPv6 with prefix delegation if not already configured
                    extra
                        .entry("ipv6Configuration".into())
                        .or_insert_with(|| serde_json::json!({ "type": "PREFIX_DELEGATION" }));
                } else {
                    extra.remove("ipv6Configuration");
                }
            }
            let body = unifly_api::integration_types::NetworkCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                enabled: update.enabled.unwrap_or(existing.enabled),
                management: existing.management,
                vlan_id: update.vlan_id.map_or(existing.vlan_id, i32::from),
                dhcp_guarding: existing.dhcp_guarding,
                extra,
            };
            ic.update_network(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteNetwork { id, force: _ } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "DeleteNetwork")?;
            let uuid = require_uuid(&id)?;
            ic.delete_network(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        // ── WiFi Broadcast CRUD ──────────────────────────────────
        Command::CreateWifiBroadcast(req) => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "CreateWifiBroadcast")?;
            let mut extra = serde_json::Map::new();
            extra.insert("ssid".into(), serde_json::Value::String(req.ssid));
            let security_mode = match req.security_mode {
                crate::model::WifiSecurityMode::Open => "OPEN",
                crate::model::WifiSecurityMode::Wpa2Personal => "WPA2_PERSONAL",
                crate::model::WifiSecurityMode::Wpa3Personal => "WPA3_PERSONAL",
                crate::model::WifiSecurityMode::Wpa2Wpa3Personal => "WPA2_WPA3_PERSONAL",
                crate::model::WifiSecurityMode::Wpa2Enterprise => "WPA2_ENTERPRISE",
                crate::model::WifiSecurityMode::Wpa3Enterprise => "WPA3_ENTERPRISE",
                crate::model::WifiSecurityMode::Wpa2Wpa3Enterprise => "WPA2_WPA3_ENTERPRISE",
            };
            let mut security_configuration = serde_json::Map::new();
            security_configuration.insert(
                "mode".into(),
                serde_json::Value::String(security_mode.into()),
            );
            if let Some(pass) = req.passphrase {
                security_configuration.insert("passphrase".into(), serde_json::Value::String(pass));
            }
            extra.insert(
                "securityConfiguration".into(),
                serde_json::Value::Object(security_configuration),
            );
            if let Some(network_id) = req.network_id {
                extra.insert(
                    "network".into(),
                    serde_json::json!({ "id": network_id.to_string() }),
                );
            }
            extra.insert("hideSsid".into(), serde_json::Value::Bool(req.hide_ssid));
            if req.band_steering {
                extra.insert("bandSteering".into(), serde_json::Value::Bool(true));
            }
            if req.fast_roaming {
                extra.insert("fastRoaming".into(), serde_json::Value::Bool(true));
            }
            if let Some(freqs) = req.frequencies_ghz {
                let values = freqs
                    .into_iter()
                    .map(|f| serde_json::Value::from(f64::from(f)))
                    .collect::<Vec<_>>();
                extra.insert("frequencies".into(), serde_json::Value::Array(values));
            }
            let body = unifly_api::integration_types::WifiBroadcastCreateUpdate {
                name: req.name,
                broadcast_type: req.broadcast_type.unwrap_or_else(|| "STANDARD".into()),
                enabled: req.enabled,
                body: extra,
            };
            ic.create_wifi_broadcast(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateWifiBroadcast { id, update } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "UpdateWifiBroadcast")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_wifi_broadcast(&sid, &uuid).await?;

            let mut body = serde_json::Map::new();
            for (k, v) in existing.extra {
                body.insert(k, v);
            }
            body.insert(
                "securityConfiguration".into(),
                existing.security_configuration.clone(),
            );
            if let Some(network) = existing.network.clone() {
                body.insert("network".into(), network);
            }
            if let Some(filter) = existing.broadcasting_device_filter.clone() {
                body.insert("broadcastingDeviceFilter".into(), filter);
            }

            if let Some(ssid) = update.ssid.clone() {
                body.insert("ssid".into(), serde_json::Value::String(ssid));
            }
            if let Some(hidden) = update.hide_ssid {
                body.insert("hideSsid".into(), serde_json::Value::Bool(hidden));
            }

            let mut security_cfg = existing
                .security_configuration
                .as_object()
                .cloned()
                .unwrap_or_default();
            if let Some(mode) = update.security_mode {
                let mode = match mode {
                    crate::model::WifiSecurityMode::Open => "OPEN",
                    crate::model::WifiSecurityMode::Wpa2Personal => "WPA2_PERSONAL",
                    crate::model::WifiSecurityMode::Wpa3Personal => "WPA3_PERSONAL",
                    crate::model::WifiSecurityMode::Wpa2Wpa3Personal => "WPA2_WPA3_PERSONAL",
                    crate::model::WifiSecurityMode::Wpa2Enterprise => "WPA2_ENTERPRISE",
                    crate::model::WifiSecurityMode::Wpa3Enterprise => "WPA3_ENTERPRISE",
                    crate::model::WifiSecurityMode::Wpa2Wpa3Enterprise => "WPA2_WPA3_ENTERPRISE",
                };
                security_cfg.insert("mode".into(), serde_json::Value::String(mode.into()));
            }
            if let Some(passphrase) = update.passphrase.clone() {
                security_cfg.insert("passphrase".into(), serde_json::Value::String(passphrase));
            }
            body.insert(
                "securityConfiguration".into(),
                serde_json::Value::Object(security_cfg),
            );

            let payload = unifly_api::integration_types::WifiBroadcastCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                broadcast_type: existing.broadcast_type,
                enabled: update.enabled.unwrap_or(existing.enabled),
                body,
            };
            ic.update_wifi_broadcast(&sid, &uuid, &payload).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteWifiBroadcast { id, force: _ } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "DeleteWifiBroadcast")?;
            let uuid = require_uuid(&id)?;
            ic.delete_wifi_broadcast(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        // ── Firewall Policy CRUD ─────────────────────────────────
        Command::CreateFirewallPolicy(req) => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "CreateFirewallPolicy")?;
            let action_str = match req.action {
                FirewallAction::Allow => "ALLOW",
                FirewallAction::Block => "DROP",
                FirewallAction::Reject => "REJECT",
            };
            let body = unifly_api::integration_types::FirewallPolicyCreateUpdate {
                name: req.name,
                description: req.description,
                enabled: req.enabled,
                action: serde_json::json!({ "type": action_str }),
                source: serde_json::json!({ "zoneId": req.source_zone_id.to_string() }),
                destination: serde_json::json!({ "zoneId": req.destination_zone_id.to_string() }),
                ip_protocol_scope: serde_json::json!("ALL"),
                logging_enabled: req.logging_enabled,
                ipsec_filter: None,
                schedule: None,
                connection_state_filter: None,
            };
            ic.create_firewall_policy(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateFirewallPolicy { id, update } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "UpdateFirewallPolicy")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_firewall_policy(&sid, &uuid).await?;

            let mut source = existing
                .extra
                .get("source")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(addr) = update.source_address.clone() {
                if let Some(obj) = source.as_object_mut() {
                    obj.insert("address".into(), serde_json::Value::String(addr));
                }
            }

            let mut destination = existing
                .extra
                .get("destination")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(addr) = update.destination_address.clone() {
                if let Some(obj) = destination.as_object_mut() {
                    obj.insert("address".into(), serde_json::Value::String(addr));
                }
            }
            if let Some(port) = update.destination_port.clone() {
                if let Some(obj) = destination.as_object_mut() {
                    obj.insert("port".into(), serde_json::Value::String(port));
                }
            }

            let action = if let Some(action) = update.action {
                let action_type = match action {
                    FirewallAction::Allow => "ALLOW",
                    FirewallAction::Block => "DROP",
                    FirewallAction::Reject => "REJECT",
                };
                serde_json::json!({ "type": action_type })
            } else {
                existing.action
            };

            let ip_protocol_scope = if let Some(protocol) = update.protocol.clone() {
                serde_json::json!({ "protocol": protocol })
            } else {
                existing
                    .ip_protocol_scope
                    .unwrap_or_else(|| serde_json::json!("ALL"))
            };

            let connection_state_filter = existing
                .extra
                .get("connectionStateFilter")
                .and_then(serde_json::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                });

            let payload = unifly_api::integration_types::FirewallPolicyCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                description: update.description.or(existing.description),
                enabled: update.enabled.unwrap_or(existing.enabled),
                action,
                source,
                destination,
                ip_protocol_scope,
                logging_enabled: existing.logging_enabled,
                ipsec_filter: existing
                    .extra
                    .get("ipsecFilter")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                schedule: existing.extra.get("schedule").cloned(),
                connection_state_filter,
            };

            ic.update_firewall_policy(&sid, &uuid, &payload).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteFirewallPolicy { id } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "DeleteFirewallPolicy")?;
            let uuid = require_uuid(&id)?;
            ic.delete_firewall_policy(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        Command::PatchFirewallPolicy { id, enabled } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "PatchFirewallPolicy")?;
            let uuid = require_uuid(&id)?;
            let body = unifly_api::integration_types::FirewallPolicyPatch {
                enabled: Some(enabled),
                logging_enabled: None,
            };
            ic.patch_firewall_policy(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::ReorderFirewallPolicies {
            zone_pair: _,
            ordered_ids,
        } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "ReorderFirewallPolicies")?;
            let uuids: Result<Vec<uuid::Uuid>, _> = ordered_ids.iter().map(require_uuid).collect();
            let body = unifly_api::integration_types::FirewallPolicyOrdering {
                before_system_defined: uuids?,
                after_system_defined: Vec::new(),
            };
            ic.set_firewall_policy_ordering(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        // ── Firewall Zone CRUD ───────────────────────────────────
        Command::CreateFirewallZone(req) => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "CreateFirewallZone")?;
            let network_uuids: Result<Vec<uuid::Uuid>, _> =
                req.network_ids.iter().map(require_uuid).collect();
            let body = unifly_api::integration_types::FirewallZoneCreateUpdate {
                name: req.name,
                network_ids: network_uuids?,
            };
            ic.create_firewall_zone(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateFirewallZone { id, update } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "UpdateFirewallZone")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_firewall_zone(&sid, &uuid).await?;
            let network_ids = if let Some(ids) = update.network_ids {
                let uuids: Result<Vec<uuid::Uuid>, _> = ids.iter().map(require_uuid).collect();
                uuids?
            } else {
                existing.network_ids
            };
            let body = unifly_api::integration_types::FirewallZoneCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                network_ids,
            };
            ic.update_firewall_zone(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteFirewallZone { id } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "DeleteFirewallZone")?;
            let uuid = require_uuid(&id)?;
            ic.delete_firewall_zone(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        // ── ACL Rule CRUD ────────────────────────────────────────
        Command::CreateAclRule(req) => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "CreateAclRule")?;
            let action_str = match req.action {
                FirewallAction::Allow => "ALLOW",
                FirewallAction::Block => "BLOCK",
                FirewallAction::Reject => "REJECT",
            };
            let mut source_filter = serde_json::Map::new();
            source_filter.insert(
                "zoneId".into(),
                serde_json::Value::String(req.source_zone_id.to_string()),
            );
            if let Some(source_port) = req.source_port {
                source_filter.insert("port".into(), serde_json::Value::String(source_port));
            }
            if let Some(protocol) = req.protocol.clone() {
                source_filter.insert("protocol".into(), serde_json::Value::String(protocol));
            }

            let mut destination_filter = serde_json::Map::new();
            destination_filter.insert(
                "zoneId".into(),
                serde_json::Value::String(req.destination_zone_id.to_string()),
            );
            if let Some(destination_port) = req.destination_port {
                destination_filter
                    .insert("port".into(), serde_json::Value::String(destination_port));
            }
            if let Some(protocol) = req.protocol {
                destination_filter.insert("protocol".into(), serde_json::Value::String(protocol));
            }
            let body = unifly_api::integration_types::AclRuleCreateUpdate {
                name: req.name,
                rule_type: req.rule_type,
                action: action_str.into(),
                enabled: req.enabled,
                description: None,
                source_filter: Some(serde_json::Value::Object(source_filter)),
                destination_filter: Some(serde_json::Value::Object(destination_filter)),
                enforcing_device_filter: None,
            };
            ic.create_acl_rule(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateAclRule { id, update } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "UpdateAclRule")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_acl_rule(&sid, &uuid).await?;
            let action_str = match update.action {
                Some(FirewallAction::Allow) => "ALLOW".into(),
                Some(FirewallAction::Block) => "BLOCK".into(),
                Some(FirewallAction::Reject) => "REJECT".into(),
                None => existing.action,
            };
            let body = unifly_api::integration_types::AclRuleCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                rule_type: existing.rule_type,
                action: action_str,
                enabled: update.enabled.unwrap_or(existing.enabled),
                description: existing.description,
                source_filter: existing.source_filter,
                destination_filter: existing.destination_filter,
                enforcing_device_filter: existing.enforcing_device_filter,
            };
            ic.update_acl_rule(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteAclRule { id } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "DeleteAclRule")?;
            let uuid = require_uuid(&id)?;
            ic.delete_acl_rule(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        Command::ReorderAclRules { ordered_ids } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "ReorderAclRules")?;
            let uuids: Result<Vec<uuid::Uuid>, _> = ordered_ids.iter().map(require_uuid).collect();
            let body = unifly_api::integration_types::AclRuleOrdering {
                ordered_acl_rule_ids: uuids?,
            };
            ic.set_acl_rule_ordering(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        // ── DNS Policy CRUD ──────────────────────────────────────
        Command::CreateDnsPolicy(req) => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "CreateDnsPolicy")?;
            let policy_type_str = match req.policy_type {
                crate::model::DnsPolicyType::ARecord => "A",
                crate::model::DnsPolicyType::AaaaRecord => "AAAA",
                crate::model::DnsPolicyType::CnameRecord => "CNAME",
                crate::model::DnsPolicyType::MxRecord => "MX",
                crate::model::DnsPolicyType::TxtRecord => "TXT",
                crate::model::DnsPolicyType::SrvRecord => "SRV",
                crate::model::DnsPolicyType::ForwardDomain => "FORWARD_DOMAIN",
            };
            let mut fields = serde_json::Map::new();
            if let Some(domains) = req.domains {
                if let Some(first) = domains.first() {
                    fields.insert("domain".into(), serde_json::Value::String(first.clone()));
                }
                fields.insert(
                    "domains".into(),
                    serde_json::Value::Array(
                        domains.into_iter().map(serde_json::Value::String).collect(),
                    ),
                );
            }
            if let Some(upstream) = req.upstream {
                fields.insert("upstream".into(), serde_json::Value::String(upstream));
            }
            if let Some(value) = req.value {
                fields.insert("value".into(), serde_json::Value::String(value));
            }
            if let Some(ttl) = req.ttl_seconds {
                fields.insert(
                    "ttl".into(),
                    serde_json::Value::Number(serde_json::Number::from(ttl)),
                );
            }
            if let Some(priority) = req.priority {
                fields.insert(
                    "priority".into(),
                    serde_json::Value::Number(serde_json::Number::from(priority)),
                );
            }
            fields.insert("name".into(), serde_json::Value::String(req.name));
            let body = unifly_api::integration_types::DnsPolicyCreateUpdate {
                policy_type: policy_type_str.into(),
                enabled: req.enabled,
                fields,
            };
            ic.create_dns_policy(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateDnsPolicy { id, update } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "UpdateDnsPolicy")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_dns_policy(&sid, &uuid).await?;
            let mut fields: serde_json::Map<String, serde_json::Value> =
                existing.extra.into_iter().collect();

            if let Some(domains) = update.domains {
                if let Some(first) = domains.first() {
                    fields.insert("domain".into(), serde_json::Value::String(first.clone()));
                }
                fields.insert(
                    "domains".into(),
                    serde_json::Value::Array(
                        domains.into_iter().map(serde_json::Value::String).collect(),
                    ),
                );
            } else if let Some(domain) = existing.domain {
                fields
                    .entry("domain")
                    .or_insert_with(|| serde_json::Value::String(domain));
            }

            if let Some(name) = update.name {
                fields.insert("name".into(), serde_json::Value::String(name));
            }
            if let Some(upstream) = update.upstream {
                fields.insert("upstream".into(), serde_json::Value::String(upstream));
            }
            if let Some(value) = update.value {
                fields.insert("value".into(), serde_json::Value::String(value));
            }
            if let Some(ttl) = update.ttl_seconds {
                fields.insert(
                    "ttl".into(),
                    serde_json::Value::Number(serde_json::Number::from(ttl)),
                );
            }
            if let Some(priority) = update.priority {
                fields.insert(
                    "priority".into(),
                    serde_json::Value::Number(serde_json::Number::from(priority)),
                );
            }

            let body = unifly_api::integration_types::DnsPolicyCreateUpdate {
                policy_type: existing.policy_type,
                enabled: update.enabled.unwrap_or(existing.enabled),
                fields,
            };
            ic.update_dns_policy(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteDnsPolicy { id } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "DeleteDnsPolicy")?;
            let uuid = require_uuid(&id)?;
            ic.delete_dns_policy(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        // ── Traffic Matching List CRUD ───────────────────────────
        Command::CreateTrafficMatchingList(req) => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "CreateTrafficMatchingList")?;
            let mut fields = serde_json::Map::new();
            fields.insert(
                "entries".into(),
                serde_json::Value::Array(
                    req.entries
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
            if let Some(desc) = req.description {
                fields.insert("description".into(), serde_json::Value::String(desc));
            }
            let body = unifly_api::integration_types::TrafficMatchingListCreateUpdate {
                name: req.name,
                list_type: req.list_type,
                fields,
            };
            ic.create_traffic_matching_list(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateTrafficMatchingList { id, update } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "UpdateTrafficMatchingList")?;
            let uuid = require_uuid(&id)?;
            let existing = ic.get_traffic_matching_list(&sid, &uuid).await?;
            let mut fields = serde_json::Map::new();
            let entries = if let Some(new_entries) = update.entries {
                serde_json::Value::Array(
                    new_entries
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                )
            } else if let Some(existing_entries) = existing.extra.get("entries") {
                existing_entries.clone()
            } else {
                serde_json::Value::Array(Vec::new())
            };
            fields.insert("entries".into(), entries);
            if let Some(desc) = update.description {
                fields.insert("description".into(), serde_json::Value::String(desc));
            } else if let Some(existing_desc) = existing.extra.get("description") {
                fields.insert("description".into(), existing_desc.clone());
            }
            let body = unifly_api::integration_types::TrafficMatchingListCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                list_type: existing.list_type,
                fields,
            };
            ic.update_traffic_matching_list(&sid, &uuid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteTrafficMatchingList { id } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "DeleteTrafficMatchingList")?;
            let uuid = require_uuid(&id)?;
            ic.delete_traffic_matching_list(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        // ── Voucher management ───────────────────────────────────
        Command::CreateVouchers(req) => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "CreateVouchers")?;
            #[allow(clippy::as_conversions, clippy::cast_possible_wrap)]
            let body = unifly_api::integration_types::VoucherCreateRequest {
                name: req.name.unwrap_or_else(|| "Voucher".into()),
                count: Some(req.count as i32),
                time_limit_minutes: i64::from(req.time_limit_minutes.unwrap_or(60)),
                authorized_guest_limit: req.authorized_guest_limit.map(i64::from),
                data_usage_limit_m_bytes: req.data_usage_limit_mb.map(|m| m as i64),
                rx_rate_limit_kbps: req.rx_rate_limit_kbps.map(|r| r as i64),
                tx_rate_limit_kbps: req.tx_rate_limit_kbps.map(|r| r as i64),
            };
            let vouchers = ic.create_vouchers(&sid, &body).await?;
            let domain_vouchers: Vec<Voucher> = vouchers.into_iter().map(Voucher::from).collect();
            Ok(CommandResult::Vouchers(domain_vouchers))
        }

        Command::DeleteVoucher { id } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "DeleteVoucher")?;
            let uuid = require_uuid(&id)?;
            ic.delete_voucher(&sid, &uuid).await?;
            Ok(CommandResult::Ok)
        }

        Command::PurgeVouchers { filter } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "PurgeVouchers")?;
            ic.purge_vouchers(&sid, &filter).await?;
            Ok(CommandResult::Ok)
        }

        // ── System administration ────────────────────────────────
        Command::CreateSite { name, description } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.create_site(&name, &description).await?;
            Ok(CommandResult::Ok)
        }
        Command::DeleteSite { name } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.delete_site(&name).await?;
            Ok(CommandResult::Ok)
        }
        Command::InviteAdmin { name, email, role } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.invite_admin(&name, &email, &role).await?;
            Ok(CommandResult::Ok)
        }
        Command::RevokeAdmin { id } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.revoke_admin(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }
        Command::UpdateAdmin { id, role } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy
                .update_admin(&id.to_string(), role.as_deref())
                .await?;
            Ok(CommandResult::Ok)
        }

        Command::RebootController => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.reboot_controller().await?;
            Ok(CommandResult::Ok)
        }
        Command::PoweroffController => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.poweroff_controller().await?;
            Ok(CommandResult::Ok)
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

fn parse_ipv6_text(raw: &str) -> Option<Ipv6Addr> {
    let candidate = raw.trim().split('/').next().unwrap_or(raw).trim();
    candidate.parse::<Ipv6Addr>().ok()
}

fn pick_ipv6_from_value(value: &serde_json::Value) -> Option<String> {
    let mut first_link_local: Option<String> = None;

    let iter: Box<dyn Iterator<Item = &serde_json::Value> + '_> = match value {
        serde_json::Value::Array(items) => Box::new(items.iter()),
        _ => Box::new(std::iter::once(value)),
    };

    for item in iter {
        if let Some(ipv6) = item.as_str().and_then(parse_ipv6_text) {
            let ip_text = ipv6.to_string();
            if !ipv6.is_unicast_link_local() {
                return Some(ip_text);
            }
            if first_link_local.is_none() {
                first_link_local = Some(ip_text);
            }
        }
    }

    first_link_local
}

fn parse_legacy_device_wan_ipv6(
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    // Primary source on gateways: wan1.ipv6 = ["global", "link-local"].
    if let Some(v) = extra
        .get("wan1")
        .and_then(|wan| wan.get("ipv6"))
        .and_then(pick_ipv6_from_value)
    {
        return Some(v);
    }

    // Fallback source on some firmware: top-level ipv6 array.
    extra.get("ipv6").and_then(pick_ipv6_from_value)
}

/// Convert raw health JSON values into domain `HealthSummary` types.
fn convert_health_summaries(raw: Vec<serde_json::Value>) -> Vec<HealthSummary> {
    raw.into_iter()
        .map(|v| HealthSummary {
            subsystem: v
                .get("subsystem")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            status: v
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            num_adopted: v
                .get("num_adopted")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as u32),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            num_sta: v
                .get("num_sta")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as u32),
            tx_bytes_r: v.get("tx_bytes-r").and_then(serde_json::Value::as_u64),
            rx_bytes_r: v.get("rx_bytes-r").and_then(serde_json::Value::as_u64),
            latency: v.get("latency").and_then(serde_json::Value::as_f64),
            wan_ip: v.get("wan_ip").and_then(|v| v.as_str()).map(String::from),
            gateways: v.get("gateways").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .filter_map(|g| g.as_str().map(String::from))
                    .collect()
            }),
            extra: v,
        })
        .collect()
}

/// Build a [`TransportConfig`] from the controller configuration.
fn build_transport(config: &ControllerConfig) -> TransportConfig {
    TransportConfig {
        tls: tls_to_transport(&config.tls),
        timeout: config.timeout,
        cookie_jar: None, // LegacyClient::new adds one automatically
    }
}

fn tls_to_transport(tls: &TlsVerification) -> TlsMode {
    match tls {
        TlsVerification::SystemDefaults => TlsMode::System,
        TlsVerification::CustomCa(path) => TlsMode::CustomCa(path.clone()),
        TlsVerification::DangerAcceptInvalid => TlsMode::DangerAcceptInvalid,
    }
}

/// Downgrade a paginated result to an empty `Vec` when the endpoint returns 404.
///
/// Some Integration API endpoints (ACL rules, DNS policies, vouchers) are not
/// available on all controller firmware versions. Rather than failing the entire
/// refresh, we log a debug message and return an empty collection.
fn unwrap_or_empty<S, D>(endpoint: &str, result: Result<Vec<S>, unifly_api::Error>) -> Vec<D>
where
    D: From<S>,
{
    match result {
        Ok(items) => items.into_iter().map(D::from).collect(),
        Err(ref e) if e.is_not_found() => {
            debug!("{endpoint}: not available (404), treating as empty");
            Vec::new()
        }
        Err(e) => {
            warn!("{endpoint}: unexpected error {e}, treating as empty");
            Vec::new()
        }
    }
}

/// Resolve the Integration API site UUID from a site name or UUID string.
///
/// If `site_name` is already a valid UUID, returns it directly.
/// Otherwise lists all sites and finds the one matching by `internal_reference`.
async fn resolve_site_id(
    client: &IntegrationClient,
    site_name: &str,
) -> Result<uuid::Uuid, CoreError> {
    // Fast path: if the input is already a UUID, use it directly.
    if let Ok(uuid) = uuid::Uuid::parse_str(site_name) {
        return Ok(uuid);
    }

    let sites = client
        .paginate_all(50, |off, lim| client.list_sites(off, lim))
        .await?;

    sites
        .into_iter()
        .find(|s| s.internal_reference == site_name)
        .map(|s| s.id)
        .ok_or_else(|| CoreError::SiteNotFound {
            name: site_name.to_owned(),
        })
}

/// Try to set up a Legacy client (best-effort for API key auth).
async fn setup_legacy_client(
    config: &ControllerConfig,
    transport: &TransportConfig,
) -> Result<LegacyClient, CoreError> {
    let platform = LegacyClient::detect_platform(&config.url).await?;
    let client = LegacyClient::new(config.url.clone(), config.site.clone(), platform, transport)?;
    Ok(client)
}

fn parse_ipv4_cidr(cidr: &str) -> Result<(Ipv4Addr, u8), CoreError> {
    let (host, prefix) = cidr
        .split_once('/')
        .ok_or_else(|| CoreError::ValidationFailed {
            message: format!("invalid ipv4 host/prefix value '{cidr}'"),
        })?;
    let host_ip = host
        .parse::<Ipv4Addr>()
        .map_err(|_| CoreError::ValidationFailed {
            message: format!("invalid IPv4 host address '{host}'"),
        })?;
    let prefix_len = prefix
        .parse::<u8>()
        .map_err(|_| CoreError::ValidationFailed {
            message: format!("invalid IPv4 prefix length '{prefix}'"),
        })?;
    if prefix_len > 32 {
        return Err(CoreError::ValidationFailed {
            message: format!("IPv4 prefix length must be <= 32, got {prefix_len}"),
        });
    }
    Ok((host_ip, prefix_len))
}

/// Extract a `Uuid` from an `EntityId`, or return an error.
fn require_uuid(id: &EntityId) -> Result<uuid::Uuid, CoreError> {
    id.as_uuid().copied().ok_or_else(|| CoreError::Unsupported {
        operation: "Integration API operation on legacy ID".into(),
        required: "UUID-based entity ID".into(),
    })
}

fn require_legacy<'a>(
    guard: &'a tokio::sync::MutexGuard<'_, Option<LegacyClient>>,
) -> Result<&'a LegacyClient, CoreError> {
    guard.as_ref().ok_or(CoreError::ControllerDisconnected)
}

fn require_integration<'a>(
    guard: &'a tokio::sync::MutexGuard<'_, Option<IntegrationClient>>,
    site_id: Option<uuid::Uuid>,
    operation: &str,
) -> Result<(&'a IntegrationClient, uuid::Uuid), CoreError> {
    let client = guard.as_ref().ok_or_else(|| unsupported(operation))?;
    let sid = site_id.ok_or_else(|| unsupported(operation))?;
    Ok((client, sid))
}

fn unsupported(operation: &str) -> CoreError {
    CoreError::Unsupported {
        operation: operation.into(),
        required: "Integration API".into(),
    }
}

/// Resolve an [`EntityId`] to a device MAC via the DataStore.
fn device_mac(store: &DataStore, id: &EntityId) -> Result<MacAddress, CoreError> {
    store
        .device_by_id(id)
        .map(|d| d.mac.clone())
        .ok_or_else(|| CoreError::DeviceNotFound {
            identifier: id.to_string(),
        })
}

/// Resolve an [`EntityId`] to a client MAC via the DataStore.
fn client_mac(store: &DataStore, id: &EntityId) -> Result<MacAddress, CoreError> {
    store
        .client_by_id(id)
        .map(|c| c.mac.clone())
        .ok_or_else(|| CoreError::ClientNotFound {
            identifier: id.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::parse_ipv4_cidr;

    #[test]
    fn parse_ipv4_cidr_accepts_valid_input() {
        let (host, prefix) = parse_ipv4_cidr("192.168.10.1/24").expect("valid CIDR");
        assert_eq!(host.to_string(), "192.168.10.1");
        assert_eq!(prefix, 24);
    }

    #[test]
    fn parse_ipv4_cidr_rejects_invalid_prefix() {
        assert!(parse_ipv4_cidr("192.168.10.1/40").is_err());
    }

    #[test]
    fn parse_ipv4_cidr_rejects_missing_prefix() {
        assert!(parse_ipv4_cidr("192.168.10.1").is_err());
    }
}
