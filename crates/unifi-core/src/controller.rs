// ── Controller abstraction ──
//
// Full lifecycle management for a UniFi controller connection.
// Handles authentication, background refresh, command routing,
// and reactive data streaming through the DataStore.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, watch, Mutex};
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
use unifi_api::LegacyClient;

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
    legacy_client: Mutex<Option<LegacyClient>>,
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

        Self {
            inner: Arc::new(ControllerInner {
                config,
                store,
                connection_state,
                event_tx,
                command_tx,
                command_rx: Mutex::new(Some(command_rx)),
                cancel,
                legacy_client: Mutex::new(None),
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

        let config = &self.inner.config;
        let transport = build_transport(config);

        // Detect platform (UniFi OS vs standalone)
        let platform = LegacyClient::detect_platform(&config.url).await?;
        debug!(?platform, "detected controller platform");

        // Create the legacy client
        let client =
            LegacyClient::new(config.url.clone(), config.site.clone(), platform, &transport)?;

        // Authenticate if using session credentials
        match &config.auth {
            AuthCredentials::Credentials { username, password } => {
                client.login(username, password).await?;
                debug!("session authentication successful");
            }
            AuthCredentials::ApiKey(_) => {
                debug!("using API key auth -- skipping legacy login");
            }
            AuthCredentials::Cloud { .. } => {
                let _ = self.inner.connection_state.send(ConnectionState::Failed);
                return Err(CoreError::Unsupported {
                    operation: "cloud authentication".into(),
                    required: "Cloud Connector client (not yet implemented)".into(),
                });
            }
        }

        *self.inner.legacy_client.lock().await = Some(client);

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
            let cancel = self.inner.cancel.clone();
            handles.push(tokio::spawn(refresh_task(ctrl, interval_secs, cancel)));
        }

        let _ = self.inner.connection_state.send(ConnectionState::Connected);
        info!("connected to controller");
        Ok(())
    }

    /// Disconnect from the controller.
    ///
    /// Cancels background tasks, logs out if session-based, and resets
    /// the connection state to [`Disconnected`](ConnectionState::Disconnected).
    pub async fn disconnect(&self) {
        self.inner.cancel.cancel();

        // Join all background tasks
        let mut handles = self.inner.task_handles.lock().await;
        for handle in handles.drain(..) {
            let _ = handle.await;
        }

        // Logout if session-based
        if matches!(self.inner.config.auth, AuthCredentials::Credentials { .. }) {
            if let Some(ref client) = *self.inner.legacy_client.lock().await {
                if let Err(e) = client.logout().await {
                    warn!(error = %e, "logout failed (non-fatal)");
                }
            }
        }

        *self.inner.legacy_client.lock().await = None;
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
        let client_guard = self.inner.legacy_client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or(CoreError::ControllerDisconnected)?;

        // Fetch in parallel
        let (devices_res, clients_res, events_res) = tokio::join!(
            client.list_devices(),
            client.list_clients(),
            client.list_events(Some(100)),
        );

        let devices: Vec<Device> = devices_res?.into_iter().map(Device::from).collect();
        let clients: Vec<Client> = clients_res?.into_iter().map(Client::from).collect();
        let events: Vec<Event> = events_res?.into_iter().map(Event::from).collect();

        // Drop the lock before writing to the store
        drop(client_guard);

        // Full replace — Legacy is the only data source for now.
        // Empty vecs for collections that require Integration API.
        self.inner.store.apply_integration_snapshot(
            devices,
            clients,
            Vec::new(), // networks
            Vec::new(), // wifi broadcasts
            Vec::new(), // firewall policies
            Vec::new(), // firewall zones
            Vec::new(), // acl rules
            Vec::new(), // dns policies
            Vec::new(), // vouchers
        );

        // Broadcast events
        for event in events {
            let _ = self.inner.event_tx.send(Arc::new(event));
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
async fn command_processor_task(
    controller: Controller,
    mut rx: mpsc::Receiver<CommandEnvelope>,
) {
    let cancel = controller.inner.cancel.clone();

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

/// Route a command to the appropriate Legacy API call.
///
/// Commands supported by the Legacy API are executed directly.
/// CRUD operations requiring the Integration API return
/// [`CoreError::Unsupported`].
async fn route_command(
    controller: &Controller,
    cmd: Command,
) -> Result<CommandResult, CoreError> {
    let client_guard = controller.inner.legacy_client.lock().await;
    let client = client_guard
        .as_ref()
        .ok_or(CoreError::ControllerDisconnected)?;
    let store = &controller.inner.store;

    match cmd {
        // ── Device operations ────────────────────────────────────

        Command::AdoptDevice { mac } => {
            client.adopt_device(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::RestartDevice { id } => {
            let mac = device_mac(store, &id)?;
            client.restart_device(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::LocateDevice { mac, enable } => {
            client.locate_device(mac.as_str(), enable).await?;
            Ok(CommandResult::Ok)
        }

        Command::UpgradeDevice { mac, firmware_url } => {
            client
                .upgrade_device(mac.as_str(), firmware_url.as_deref())
                .await?;
            Ok(CommandResult::Ok)
        }

        Command::RemoveDevice { .. } => Err(unsupported("RemoveDevice")),
        Command::ProvisionDevice { .. } => Err(unsupported("ProvisionDevice")),
        Command::SpeedtestDevice => Err(unsupported("SpeedtestDevice")),
        Command::PowerCyclePort { .. } => Err(unsupported("PowerCyclePort")),

        // ── Client operations ────────────────────────────────────

        Command::BlockClient { mac } => {
            client.block_client(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::UnblockClient { mac } => {
            client.unblock_client(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::KickClient { mac } => {
            client.kick_client(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::ForgetClient { mac } => {
            client.forget_client(mac.as_str()).await?;
            Ok(CommandResult::Ok)
        }

        Command::AuthorizeGuest {
            client_id,
            time_limit_minutes,
            data_limit_mb,
            rx_rate_kbps,
            tx_rate_kbps,
        } => {
            let mac = client_mac(store, &client_id)?;
            let minutes = time_limit_minutes.unwrap_or(60);

            client
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
            client.archive_alarm(&id.to_string()).await?;
            Ok(CommandResult::Ok)
        }

        Command::ArchiveAllAlarms => Err(unsupported("ArchiveAllAlarms")),

        // ── Backup operations ────────────────────────────────────

        Command::CreateBackup => {
            client.create_backup().await?;
            Ok(CommandResult::Ok)
        }

        Command::DeleteBackup { .. } => Err(unsupported("DeleteBackup")),

        // ── CRUD operations (Integration API only) ───────────────

        Command::CreateNetwork(_)
        | Command::UpdateNetwork { .. }
        | Command::DeleteNetwork { .. } => Err(unsupported("network CRUD")),

        Command::CreateWifiBroadcast(_)
        | Command::UpdateWifiBroadcast { .. }
        | Command::DeleteWifiBroadcast { .. } => Err(unsupported("WiFi broadcast CRUD")),

        Command::CreateFirewallPolicy(_)
        | Command::UpdateFirewallPolicy { .. }
        | Command::DeleteFirewallPolicy { .. }
        | Command::PatchFirewallPolicy { .. }
        | Command::ReorderFirewallPolicies { .. } => Err(unsupported("firewall policy CRUD")),

        Command::CreateFirewallZone(_)
        | Command::UpdateFirewallZone { .. }
        | Command::DeleteFirewallZone { .. } => Err(unsupported("firewall zone CRUD")),

        Command::CreateAclRule(_)
        | Command::UpdateAclRule { .. }
        | Command::DeleteAclRule { .. }
        | Command::ReorderAclRules { .. } => Err(unsupported("ACL rule CRUD")),

        Command::CreateDnsPolicy(_)
        | Command::UpdateDnsPolicy { .. }
        | Command::DeleteDnsPolicy { .. } => Err(unsupported("DNS policy CRUD")),

        Command::CreateTrafficMatchingList(_)
        | Command::UpdateTrafficMatchingList { .. }
        | Command::DeleteTrafficMatchingList { .. } => {
            Err(unsupported("traffic matching list CRUD"))
        }

        Command::CreateVouchers(_)
        | Command::DeleteVoucher { .. }
        | Command::PurgeVouchers { .. } => Err(unsupported("voucher management")),

        // ── System administration ────────────────────────────────

        Command::CreateSite { .. }
        | Command::DeleteSite { .. }
        | Command::RebootController
        | Command::PoweroffController
        | Command::InviteAdmin { .. }
        | Command::RevokeAdmin { .. }
        | Command::UpdateAdmin { .. } => Err(unsupported("system administration")),
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
