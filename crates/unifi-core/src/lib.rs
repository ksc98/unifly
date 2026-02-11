// unifi-core: Reactive data layer between unifi-api and consumers (CLI/TUI).

pub mod config;
pub mod convert;
pub mod error;
pub mod model;
pub mod command;
pub mod controller;
pub mod store;
pub mod stream;

// ── Primary re-exports ──────────────────────────────────────────────
pub use config::{AuthCredentials, ControllerConfig, TlsVerification};
pub use error::CoreError;
pub use controller::{ConnectionState, Controller};
pub use command::{Command, CommandResult};
pub use command::requests::*;
pub use store::DataStore;
pub use stream::EntityStream;

// Re-export model types at the crate root for ergonomics.
pub use model::{
    // Core entities
    Client, ClientType, Device, DeviceState, DeviceType, EntityId, Event, MacAddress, Network,
    Site,
    // Events / alarms
    Alarm, EventCategory, EventSeverity,
    // Firewall
    AclRule, FirewallPolicy, FirewallZone,
    // Supporting types
    TrafficMatchingList, VpnServer, VpnTunnel, WanInterface, RadiusProfile,
    // Legacy resource types
    Admin, Country, DpiApplication, DpiCategory, HealthSummary, SysInfo, SystemInfo,
};
