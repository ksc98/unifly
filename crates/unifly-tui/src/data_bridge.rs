//! Data bridge — connects [Controller] streams to TUI actions.
//!
//! Runs as a background task: subscribes to entity streams and connection
//! state from the controller, forwarding every change as an [`Action`]
//! through the TUI's action channel.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use unifly_core::{ConnectionState, Controller};

use crate::action::Action;

/// Spawn the data bridge connecting [`Controller`] reactive streams to the TUI.
///
/// Subscribes to entity streams first, then spawns `connect()` in the
/// background so poll data flows to the UI as soon as auth completes —
/// no delay waiting for websocket handshake.
pub async fn spawn_data_bridge(
    controller: Controller,
    action_tx: mpsc::UnboundedSender<Action>,
    cancel: CancellationToken,
) {
    info!("TRACE [bridge] data_bridge starting");
    let _ = action_tx.send(Action::Reconnecting);

    // Subscribe to entity streams BEFORE connect — watch channels exist
    // from Controller::new(), so subscribing early is safe.
    info!("TRACE [bridge] subscribing to entity streams");
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
    let mut monthly_wan = controller.monthly_wan_bytes();
    let mut daily_usage = controller.client_daily_usage();
    info!("TRACE [bridge] all streams subscribed");

    // Spawn connect in background — polls start firing as soon as auth completes
    let ctrl = controller.clone();
    let tx = action_tx.clone();
    info!("TRACE [bridge] spawning connect() background task");
    tokio::spawn(async move {
        info!("TRACE [bridge] connect() starting");
        if let Err(e) = ctrl.connect().await {
            warn!(error = %e, "failed to connect to controller");
            let _ = tx.send(Action::Disconnected(format!("{e}")));
            return;
        }
        info!("TRACE [bridge] connect() completed successfully");
        let _ = tx.send(Action::Connected);
        for warning in ctrl.take_warnings().await {
            let _ = tx.send(Action::Notify(crate::action::Notification {
                message: warning,
                level: crate::action::NotificationLevel::Warning,
            }));
        }
    });

    // Enter forwarding loop immediately — data flows as soon as polls fire
    info!("TRACE [bridge] entering forwarding loop");
    loop {
        tokio::select! {
            biased;

            () = cancel.cancelled() => break,

            Some(d) = devices.changed() => {
                info!("TRACE [bridge] DevicesUpdated forwarded, {} items", d.len());
                let _ = action_tx.send(Action::DevicesUpdated(d));
            }
            Some(c) = clients.changed() => {
                info!("TRACE [bridge] ClientsUpdated forwarded, {} items", c.len());
                let _ = action_tx.send(Action::ClientsUpdated(c));
            }
            Some(n) = networks.changed() => {
                info!("TRACE [bridge] NetworksUpdated forwarded, {} items", n.len());
                let _ = action_tx.send(Action::NetworksUpdated(n));
            }
            Some(p) = fw_policies.changed() => {
                info!("TRACE [bridge] FirewallPoliciesUpdated forwarded, {} items", p.len());
                let _ = action_tx.send(Action::FirewallPoliciesUpdated(p));
            }
            Some(z) = fw_zones.changed() => {
                info!("TRACE [bridge] FirewallZonesUpdated forwarded, {} items", z.len());
                let _ = action_tx.send(Action::FirewallZonesUpdated(z));
            }
            Some(a) = acl_rules.changed() => {
                info!("TRACE [bridge] AclRulesUpdated forwarded, {} items", a.len());
                let _ = action_tx.send(Action::AclRulesUpdated(a));
            }
            Some(w) = wifi.changed() => {
                info!("TRACE [bridge] WifiBroadcastsUpdated forwarded, {} items", w.len());
                let _ = action_tx.send(Action::WifiBroadcastsUpdated(w));
            }
            Ok(event) = events.recv() => {
                info!("TRACE [bridge] EventReceived forwarded");
                let _ = action_tx.send(Action::EventReceived(event));
            }
            Ok(()) = site_health.changed() => {
                let h = site_health.borrow_and_update().clone();
                info!("TRACE [bridge] HealthUpdated forwarded, {} entries", h.len());
                let _ = action_tx.send(Action::HealthUpdated(h));
            }
            Ok(()) = monthly_wan.changed() => {
                let (tx, rx) = *monthly_wan.borrow_and_update();
                info!("TRACE [bridge] MonthlyWanUsage forwarded, tx={tx} rx={rx}");
                let _ = action_tx.send(Action::MonthlyWanUsage(tx, rx));
            }
            Ok(()) = daily_usage.changed() => {
                let usage = daily_usage.borrow_and_update().clone();
                info!("TRACE [bridge] ClientDailyUsageUpdated forwarded, {} clients", usage.len());
                let _ = action_tx.send(Action::ClientDailyUsageUpdated(usage));
            }
            Ok(()) = conn_state.changed() => {
                let state = conn_state.borrow_and_update().clone();
                info!("TRACE [bridge] ConnectionState changed: {:?}", state);
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
