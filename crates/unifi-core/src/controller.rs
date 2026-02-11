// ── Controller abstraction ──
//
// The main entry point for consumers. Represents a connection to one
// UniFi controller. Holds configuration and will eventually manage
// DataStore, API clients, and background task handles.

use std::sync::Arc;

use crate::command::{Command, CommandResult};
use crate::config::ControllerConfig;
use crate::error::CoreError;
use crate::model::*;

/// The main entry point for consumers. Represents a connection to one
/// UniFi controller.
pub struct Controller {
    config: ControllerConfig,
}

impl Controller {
    /// Create a new Controller from configuration.
    /// Does NOT connect -- call `.connect()` when the full connection
    /// lifecycle is implemented.
    pub fn new(config: ControllerConfig) -> Self {
        Self { config }
    }

    /// Access the controller configuration.
    pub fn config(&self) -> &ControllerConfig {
        &self.config
    }

    /// One-shot convenience: connect, run closure, disconnect.
    ///
    /// Optimized for CLI: skips WebSocket and refresh tasks since we only
    /// need a single operation. This avoids spawning unnecessary background
    /// tasks and reduces latency.
    pub async fn oneshot<F, Fut, T>(config: ControllerConfig, f: F) -> Result<T, CoreError>
    where
        F: FnOnce(Arc<Controller>) -> Fut,
        Fut: std::future::Future<Output = Result<T, CoreError>>,
    {
        let mut cfg = config;
        cfg.websocket_enabled = false;
        cfg.refresh_interval_secs = 0;

        let controller = Arc::new(Controller::new(cfg));
        f(controller).await
    }

    /// Execute a command (routes to Integration or Legacy API).
    pub async fn execute(&self, _cmd: Command) -> Result<CommandResult, CoreError> {
        todo!("command routing")
    }

    // ── Snapshot accessors (return empty vecs for now) ───────────────

    /// Current device snapshot.
    pub fn devices_snapshot(&self) -> Arc<Vec<Arc<Device>>> {
        Arc::new(Vec::new())
    }

    /// Current client snapshot.
    pub fn clients_snapshot(&self) -> Arc<Vec<Arc<Client>>> {
        Arc::new(Vec::new())
    }

    /// Current network snapshot.
    pub fn networks_snapshot(&self) -> Arc<Vec<Arc<Network>>> {
        Arc::new(Vec::new())
    }

    /// Current WiFi broadcast snapshot.
    pub fn wifi_broadcasts_snapshot(&self) -> Arc<Vec<Arc<WifiBroadcast>>> {
        Arc::new(Vec::new())
    }

    /// Current firewall policy snapshot.
    pub fn firewall_policies_snapshot(&self) -> Arc<Vec<Arc<FirewallPolicy>>> {
        Arc::new(Vec::new())
    }

    /// Current firewall zone snapshot.
    pub fn firewall_zones_snapshot(&self) -> Arc<Vec<Arc<FirewallZone>>> {
        Arc::new(Vec::new())
    }

    /// Current ACL rule snapshot.
    pub fn acl_rules_snapshot(&self) -> Arc<Vec<Arc<AclRule>>> {
        Arc::new(Vec::new())
    }

    /// Current DNS policy snapshot.
    pub fn dns_policies_snapshot(&self) -> Arc<Vec<Arc<DnsPolicy>>> {
        Arc::new(Vec::new())
    }

    /// Current voucher snapshot.
    pub fn vouchers_snapshot(&self) -> Arc<Vec<Arc<Voucher>>> {
        Arc::new(Vec::new())
    }
}
