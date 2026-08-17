//! Data bridge — connects [Controller] streams to TUI actions.
//!
//! Runs as a background task: subscribes to entity streams and connection
//! state from the controller, forwarding every change as an [`Action`]
//! through the TUI's action channel.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use unifly_core::{ConnectionState, Controller};

use crate::action::Action;

/// Spawn the data bridge connecting [`Controller`] reactive streams to the TUI.
///
/// Connects to the controller, sends initial data snapshots, then loops
/// forwarding every entity change and connection-state transition as an
/// [`Action`]. Shuts down cleanly on cancellation.
pub async fn spawn_data_bridge(
    controller: Controller,
    action_tx: mpsc::UnboundedSender<Action>,
    cancel: CancellationToken,
) {
    // Signal connecting state
    let _ = action_tx.send(Action::Reconnecting);

    if let Err(e) = controller.connect().await {
        warn!(error = %e, "failed to connect to controller");
        let _ = action_tx.send(Action::Disconnected(format!("{e}")));
        return;
    }

    let _ = action_tx.send(Action::Connected);

    // Surface any warnings from connect (e.g. Legacy auth failure)
    for warning in controller.take_warnings().await {
        let _ = action_tx.send(Action::Notify(crate::action::Notification {
            message: warning,
            level: crate::action::NotificationLevel::Warning,
        }));
    }

    // Subscribe to entity streams
    let mut devices = controller.devices();
    let mut clients = controller.clients();
    let mut networks = controller.networks();
    let mut fw_policies = controller.firewall_policies();
    let mut fw_zones = controller.firewall_zones();
    let mut acl_rules = controller.acl_rules();
    let mut wifi = controller.wifi_broadcasts();
    let mut events = controller.events();
    let mut conn_state = controller.connection_state();
    let mut site_health = controller.site_health();

    // Push initial snapshots so screens have data immediately
    let _ = action_tx.send(Action::DevicesUpdated(devices.current().clone()));
    let _ = action_tx.send(Action::ClientsUpdated(clients.current().clone()));
    let _ = action_tx.send(Action::NetworksUpdated(networks.current().clone()));
    let _ = action_tx.send(Action::FirewallPoliciesUpdated(
        fw_policies.current().clone(),
    ));
    let _ = action_tx.send(Action::FirewallZonesUpdated(fw_zones.current().clone()));
    let _ = action_tx.send(Action::AclRulesUpdated(acl_rules.current().clone()));
    let _ = action_tx.send(Action::WifiBroadcastsUpdated(wifi.current().clone()));

    // Push initial health snapshot
    let health_snap = site_health.borrow_and_update().clone();
    if !health_snap.is_empty() {
        let _ = action_tx.send(Action::HealthUpdated(health_snap));
    }

    // Stream loop — forward every change until cancelled
    loop {
        tokio::select! {
            biased;

            () = cancel.cancelled() => break,

            Some(d) = devices.changed() => {
                let _ = action_tx.send(Action::DevicesUpdated(d));
            }
            Some(c) = clients.changed() => {
                let _ = action_tx.send(Action::ClientsUpdated(c));
            }
            Some(n) = networks.changed() => {
                let _ = action_tx.send(Action::NetworksUpdated(n));
            }
            Some(p) = fw_policies.changed() => {
                let _ = action_tx.send(Action::FirewallPoliciesUpdated(p));
            }
            Some(z) = fw_zones.changed() => {
                let _ = action_tx.send(Action::FirewallZonesUpdated(z));
            }
            Some(a) = acl_rules.changed() => {
                let _ = action_tx.send(Action::AclRulesUpdated(a));
            }
            Some(w) = wifi.changed() => {
                let _ = action_tx.send(Action::WifiBroadcastsUpdated(w));
            }
            Ok(event) = events.recv() => {
                let _ = action_tx.send(Action::EventReceived(event));
            }
            Ok(()) = site_health.changed() => {
                let h = site_health.borrow_and_update().clone();
                let _ = action_tx.send(Action::HealthUpdated(h));
            }
            Ok(()) = conn_state.changed() => {
                let state = conn_state.borrow_and_update().clone();
                match state {
                    ConnectionState::Connected => {
                        let _ = action_tx.send(Action::Connected);
                    }
                    ConnectionState::Disconnected => {
                        let _ = action_tx.send(Action::Disconnected("disconnected".into()));
                    }
                    ConnectionState::Reconnecting { .. } => {
                        let _ = action_tx.send(Action::Reconnecting);
                    }
                    ConnectionState::Failed => {
                        let _ = action_tx.send(Action::Disconnected("connection failed".into()));
                    }
                    ConnectionState::Connecting => {}
                }
            }
        }
    }

    controller.disconnect().await;
    debug!("data bridge shut down");
}
