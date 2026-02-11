// ── Controller abstraction ──
//
// Full lifecycle management for a UniFi controller connection.
// Handles authentication, background refresh, command routing,
// and reactive data streaming through the DataStore.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::command::{Command, CommandEnvelope, CommandResult};
use crate::config::{AuthCredentials, ControllerConfig, TlsVerification};
use crate::error::CoreError;
use crate::model::*;
use crate::store::DataStore;
use crate::stream::EntityStream;

use unifi_api::transport::{TlsMode, TransportConfig};
use unifi_api::websocket::{ReconnectConfig, WebSocketHandle};
use unifi_api::{IntegrationClient, LegacyClient};

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
    command_tx: mpsc::Sender<CommandEnvelope>,
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

        Self {
            inner: Arc::new(ControllerInner {
                config,
                store,
                connection_state,
                event_tx,
                command_tx,
                command_rx: Mutex::new(Some(command_rx)),
                cancel,
                cancel_child: Mutex::new(cancel_child),
                legacy_client: Mutex::new(None),
                integration_client: Mutex::new(None),
                site_id: Mutex::new(None),
                ws_handle: Mutex::new(None),
                task_handles: Mutex::new(Vec::new()),
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

                // Legacy API client with authenticated session
                let client = LegacyClient::new(
                    config.url.clone(),
                    config.site.clone(),
                    platform,
                    &transport,
                )?;
                client.login(username, password).await?;
                debug!("legacy session authentication successful (hybrid)");

                *self.inner.legacy_client.lock().await = Some(client);
            }
            AuthCredentials::Cloud { .. } => {
                let _ = self.inner.connection_state.send(ConnectionState::Failed);
                return Err(CoreError::Unsupported {
                    operation: "cloud authentication".into(),
                    required: "Cloud Connector client (not yet implemented)".into(),
                });
            }
        }

        // Initial data load
        self.full_refresh().await?;

        // Spawn background tasks
        let mut handles = self.inner.task_handles.lock().await;

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
    async fn spawn_websocket(
        &self,
        cancel: &CancellationToken,
        handles: &mut Vec<JoinHandle<()>>,
    ) {
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
        let scheme = if base_url.scheme() == "https" { "wss" } else { "ws" };
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
        let handle = match WebSocketHandle::connect(
            ws_url,
            ReconnectConfig::default(),
            ws_cancel.clone(),
            cookie,
        )
        .await
        {
            Ok(h) => h,
            Err(e) => {
                warn!(error = %e, "WebSocket connection failed (non-fatal)");
                return;
            }
        };

        // Bridge task: WS events → domain Events → broadcast channel
        let mut ws_rx = handle.subscribe();
        let event_tx = self.inner.event_tx.clone();
        let bridge_cancel = ws_cancel;

        handles.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = bridge_cancel.cancelled() => break,
                    result = ws_rx.recv() => {
                        match result {
                            Ok(ws_event) => {
                                let event = crate::model::event::Event::from(
                                    (*ws_event).clone(),
                                );
                                let _ = event_tx.send(Arc::new(event));
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
    pub async fn full_refresh(&self) -> Result<(), CoreError> {
        let integration_guard = self.inner.integration_client.lock().await;
        let site_id = *self.inner.site_id.lock().await;

        if let (Some(integration), Some(sid)) = (integration_guard.as_ref(), site_id) {
            // ── Integration API path (preferred) ─────────────────
            let page_limit = 200;

            let (devices_res, clients_res, networks_res, wifi_res) = tokio::join!(
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_devices(&sid, off, lim)
                }),
                integration.paginate_all(page_limit, |off, lim| {
                    integration.list_clients(&sid, off, lim)
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
            let clients: Vec<Client> = clients_res?.into_iter().map(Client::from).collect();
            let networks: Vec<Network> = networks_res?.into_iter().map(Network::from).collect();
            let wifi: Vec<WifiBroadcast> = wifi_res?.into_iter().map(WifiBroadcast::from).collect();
            let policies: Vec<FirewallPolicy> = policies_res?
                .into_iter()
                .map(FirewallPolicy::from)
                .collect();
            let zones: Vec<FirewallZone> = zones_res?.into_iter().map(FirewallZone::from).collect();
            let sites: Vec<Site> = sites_res?.into_iter().map(Site::from).collect();
            let traffic_matching_lists: Vec<TrafficMatchingList> = tml_res?
                .into_iter()
                .map(TrafficMatchingList::from)
                .collect();

            // Optional endpoints — 404 means the controller doesn't support them
            let acls: Vec<AclRule> = unwrap_or_empty("acl/rules", acls_res);
            let dns: Vec<DnsPolicy> = unwrap_or_empty("dns/policies", dns_res);
            let vouchers: Vec<Voucher> = unwrap_or_empty("vouchers", vouchers_res);

            drop(integration_guard);

            // Supplement with Legacy API events (not available in Integration API)
            let legacy_events = match *self.inner.legacy_client.lock().await {
                Some(ref legacy) => match legacy.list_events(Some(100)).await {
                    Ok(raw_events) => {
                        let events: Vec<Event> = raw_events.into_iter().map(Event::from).collect();
                        for event in &events {
                            let _ = self.inner.event_tx.send(Arc::new(event.clone()));
                        }
                        events
                    }
                    Err(e) => {
                        debug!(error = %e, "legacy event fetch failed (non-fatal)");
                        Vec::new()
                    }
                },
                None => Vec::new(),
            };

            self.inner
                .store
                .apply_integration_snapshot(crate::store::RefreshSnapshot {
                    devices,
                    clients,
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

            let (devices_res, clients_res, events_res) = tokio::join!(
                legacy.list_devices(),
                legacy.list_clients(),
                legacy.list_events(Some(100)),
            );

            let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
            let clients: Vec<Client> = clients_res?.into_iter().map(Client::from).collect();
            let events: Vec<Event> = events_res?.into_iter().map(Event::from).collect();

            drop(legacy_guard);

            for event in &events {
                let _ = self.inner.event_tx.send(Arc::new(event.clone()));
            }

            self.inner
                .store
                .apply_integration_snapshot(crate::store::RefreshSnapshot {
                    devices,
                    clients,
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

        self.inner
            .command_tx
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
                    .map(EntityId::Uuid)
                    .unwrap_or_else(|| EntityId::Legacy("unknown".into()));
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
                    enabled: s.fields.get("enabled").and_then(|v| v.as_bool()),
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
                    .map(EntityId::Uuid)
                    .unwrap_or_else(|| EntityId::Legacy("unknown".into()));
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
                    enabled: t.fields.get("enabled").and_then(|v| v.as_bool()),
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
                    .map(EntityId::Uuid)
                    .unwrap_or_else(|| EntityId::Legacy("unknown".into()));
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
                let id = c.fields.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
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
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    rx_bytes: c
                        .fields
                        .get("rxBytes")
                        .and_then(|v| v.as_u64())
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
                let id = a.fields.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                DpiApplication {
                    id,
                    name: a
                        .fields
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown")
                        .to_owned(),
                    category_id: a
                        .fields
                        .get("categoryId")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    tx_bytes: a
                        .fields
                        .get("txBytes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    rx_bytes: a
                        .fields
                        .get("rxBytes")
                        .and_then(|v| v.as_u64())
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
                    .map(EntityId::Uuid)
                    .unwrap_or_else(|| EntityId::Legacy("unknown".into()));
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
    // ── Statistics (Legacy API) ────────────────────────────────────

    /// Fetch site-level historical statistics.
    pub async fn get_site_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_site_stats(interval, start, end).await?)
    }

    /// Fetch per-device historical statistics.
    pub async fn get_device_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_device_stats(interval, macs).await?)
    }

    /// Fetch per-client historical statistics.
    pub async fn get_client_stats(
        &self,
        interval: &str,
        macs: Option<&[String]>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_client_stats(interval, macs).await?)
    }

    /// Fetch gateway historical statistics.
    pub async fn get_gateway_stats(
        &self,
        interval: &str,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_gateway_stats(interval, start, end).await?)
    }

    /// Fetch DPI statistics.
    pub async fn get_dpi_stats(&self, group_by: &str) -> Result<Vec<serde_json::Value>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        Ok(legacy.get_dpi_stats(group_by).await?)
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
                id: v
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .map(|s| EntityId::Legacy(s.into()))
                    .unwrap_or_else(|| EntityId::Legacy("unknown".into())),
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
                is_super: v.get("is_super").and_then(|v| v.as_bool()).unwrap_or(false),
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
                    uptime_secs: f.get("uptime").and_then(|v| v.as_u64()),
                    update_available: f
                        .get("isUpdateAvailable")
                        .or_else(|| f.get("update_available"))
                        .and_then(|v| v.as_bool()),
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
            uptime_secs: raw.get("uptime").and_then(|v| v.as_u64()),
            update_available: raw.get("update_available").and_then(|v| v.as_bool()),
        })
    }

    /// Fetch site health dashboard from the Legacy API.
    pub async fn get_site_health(&self) -> Result<Vec<HealthSummary>, CoreError> {
        let guard = self.inner.legacy_client.lock().await;
        let legacy = require_legacy(&guard)?;
        let raw = legacy.get_health().await?;
        Ok(raw
            .into_iter()
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
                num_adopted: v
                    .get("num_adopted")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
                num_sta: v.get("num_sta").and_then(|v| v.as_u64()).map(|n| n as u32),
                tx_bytes_r: v.get("tx_bytes-r").and_then(|v| v.as_u64()),
                rx_bytes_r: v.get("rx_bytes-r").and_then(|v| v.as_u64()),
                latency: v.get("latency").and_then(|v| v.as_f64()),
                wan_ip: v.get("wan_ip").and_then(|v| v.as_str()).map(String::from),
                gateways: v.get("gateways").and_then(|v| v.as_array()).map(|a| {
                    a.iter()
                        .filter_map(|g| g.as_str().map(String::from))
                        .collect()
                }),
                extra: v,
            })
            .collect())
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
            autobackup: raw.get("autobackup").and_then(|v| v.as_bool()),
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
            data_retention_days: raw
                .get("data_retention_days")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            extra: raw,
        })
    }
}

// ── Background tasks ─────────────────────────────────────────────

/// Periodically refresh data from the controller.
async fn refresh_task(controller: Controller, interval_secs: u64, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
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
            _ = cancel.cancelled() => break,
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
async fn route_command(controller: &Controller, cmd: Command) -> Result<CommandResult, CoreError> {
    let store = &controller.inner.store;

    // Acquire both clients for routing decisions
    let integration_guard = controller.inner.integration_client.lock().await;
    let legacy_guard = controller.inner.legacy_client.lock().await;
    let site_id = *controller.inner.site_id.lock().await;

    match cmd {
        // ── Device operations ────────────────────────────────────
        Command::AdoptDevice { mac } => {
            if let (Some(ic), Some(sid)) = (integration_guard.as_ref(), site_id) {
                ic.adopt_device(&sid, mac.as_str()).await?;
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

        Command::ProvisionDevice { .. } => Err(unsupported("ProvisionDevice")),
        Command::SpeedtestDevice => Err(unsupported("SpeedtestDevice")),

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
            legacy
                .authorize_guest(
                    mac.as_str(),
                    minutes,
                    tx_rate_kbps.map(|r| r as u32),
                    rx_rate_kbps.map(|r| r as u32),
                    data_limit_mb.map(|m| m as u32),
                )
                .await?;
            Ok(CommandResult::Ok)
        }

        Command::UnauthorizeGuest { .. } => Err(unsupported("UnauthorizeGuest")),

        // ── Alarm operations ─────────────────────────────────────
        Command::ArchiveAlarm { id } => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.archive_alarm(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }

        Command::ArchiveAllAlarms => Err(unsupported("ArchiveAllAlarms")),

        // ── Backup operations ────────────────────────────────────
        Command::CreateBackup => {
            let legacy = require_legacy(&legacy_guard)?;
            legacy.create_backup().await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteBackup { .. } => Err(unsupported("DeleteBackup")),

        // ── Network CRUD (Integration API) ───────────────────────
        Command::CreateNetwork(req) => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "CreateNetwork")?;
            let body = unifi_api::integration_types::NetworkCreateUpdate {
                name: req.name,
                enabled: req.enabled,
                management: "USER_DEFINED".into(),
                vlan_id: req.vlan_id.map(|v| v as i32).unwrap_or(1),
                dhcp_guarding: None,
            };
            ic.create_network(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateNetwork { id, update } => {
            let (ic, sid) = require_integration(&integration_guard, site_id, "UpdateNetwork")?;
            let uuid = require_uuid(&id)?;
            // Fetch existing to merge partial update
            let existing = ic.get_network(&sid, &uuid).await?;
            let body = unifi_api::integration_types::NetworkCreateUpdate {
                name: update.name.unwrap_or(existing.name),
                enabled: update.enabled.unwrap_or(existing.enabled),
                management: existing.management,
                vlan_id: update.vlan_id.map(|v| v as i32).unwrap_or(existing.vlan_id),
                dhcp_guarding: existing.dhcp_guarding,
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
            if let Some(pass) = &req.passphrase {
                extra.insert("passphrase".into(), serde_json::Value::String(pass.clone()));
            }
            let body = unifi_api::integration_types::WifiBroadcastCreateUpdate {
                name: req.name,
                broadcast_type: "STANDARD".into(),
                enabled: req.enabled,
                body: extra,
            };
            ic.create_wifi_broadcast(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateWifiBroadcast { id, update: _ } => {
            let (ic, sid) =
                require_integration(&integration_guard, site_id, "UpdateWifiBroadcast")?;
            let _uuid = require_uuid(&id)?;
            // WiFi updates are complex — for now return the existing broadcast
            let _ = (ic, sid);
            Err(unsupported(
                "UpdateWifiBroadcast (partial update not yet implemented)",
            ))
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
            let body = unifi_api::integration_types::FirewallPolicyCreateUpdate {
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

        Command::UpdateFirewallPolicy { id, update: _ } => {
            let (_ic, _sid) =
                require_integration(&integration_guard, site_id, "UpdateFirewallPolicy")?;
            let _uuid = require_uuid(&id)?;
            Err(unsupported(
                "UpdateFirewallPolicy (partial update not yet implemented)",
            ))
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
            let body = unifi_api::integration_types::FirewallPolicyPatch {
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
            let body = unifi_api::integration_types::FirewallPolicyOrdering {
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
            let body = unifi_api::integration_types::FirewallZoneCreateUpdate {
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
            let body = unifi_api::integration_types::FirewallZoneCreateUpdate {
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
            let body = unifi_api::integration_types::AclRuleCreateUpdate {
                name: req.name,
                rule_type: "DEVICE".into(),
                action: action_str.into(),
                enabled: req.enabled,
                description: None,
                source_filter: None,
                destination_filter: None,
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
            let body = unifi_api::integration_types::AclRuleCreateUpdate {
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
            let body = unifi_api::integration_types::AclRuleOrdering {
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
            let body = unifi_api::integration_types::DnsPolicyCreateUpdate {
                policy_type: policy_type_str.into(),
                enabled: req.enabled,
                fields: serde_json::Map::new(),
            };
            ic.create_dns_policy(&sid, &body).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpdateDnsPolicy { id, update: _ } => {
            let (_ic, _sid) = require_integration(&integration_guard, site_id, "UpdateDnsPolicy")?;
            let _uuid = require_uuid(&id)?;
            Err(unsupported(
                "UpdateDnsPolicy (partial update not yet implemented)",
            ))
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
            let body = unifi_api::integration_types::TrafficMatchingListCreateUpdate {
                name: req.name,
                list_type: "IPV4".into(),
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
            let body = unifi_api::integration_types::TrafficMatchingListCreateUpdate {
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
            let body = unifi_api::integration_types::VoucherCreateRequest {
                name: req.name.unwrap_or_else(|| "Voucher".into()),
                count: Some(req.count as i32),
                time_limit_minutes: req.time_limit_minutes.unwrap_or(60) as i64,
                authorized_guest_limit: req.authorized_guest_limit.map(|l| l as i64),
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
        Command::CreateSite { .. }
        | Command::DeleteSite { .. }
        | Command::InviteAdmin { .. }
        | Command::RevokeAdmin { .. }
        | Command::UpdateAdmin { .. } => Err(unsupported("system administration")),

        Command::RebootController | Command::PoweroffController => {
            Err(unsupported("controller power management"))
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

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
fn unwrap_or_empty<S, D>(endpoint: &str, result: Result<Vec<S>, unifi_api::Error>) -> Vec<D>
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
