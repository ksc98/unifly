//! Screen implementations. Each screen is a top-level Component.

pub mod clients;
pub mod dashboard;
#[allow(dead_code)]
pub mod devices;
pub mod events;
#[allow(dead_code)]
pub mod firewall;
#[allow(dead_code)]
pub mod networks;
pub mod onboarding;
pub mod settings;
pub mod stats;
pub mod topology;

use crate::component::Component;
use crate::screen::ScreenId;

/// Create screen components for the active tab bar.
pub fn create_screens() -> Vec<(ScreenId, Box<dyn Component>)> {
    vec![
        (
            ScreenId::Dashboard,
            Box::new(dashboard::DashboardScreen::new()),
        ),
        // Devices, Networks, Firewall removed from tab bar (code preserved)
        (ScreenId::Clients, Box::new(clients::ClientsScreen::new())),
        (
            ScreenId::Topology,
            Box::new(topology::TopologyScreen::new()),
        ),
        (ScreenId::Events, Box::new(events::EventsScreen::new())),
        (ScreenId::Stats, Box::new(stats::StatsScreen::new())),
    ]
}
