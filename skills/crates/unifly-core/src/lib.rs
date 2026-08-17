//! Reactive data layer between `unifly-api` and UI consumers (CLI / TUI).
//!
//! This crate owns the business logic, domain model, and reactive data
//! infrastructure for the UniFi CLI workspace:
//!
//! - **[`Controller`]** — Central facade managing the full lifecycle:
//!   [`connect()`](Controller::connect) authenticates, fetches an initial data
//!   snapshot, then spawns background tasks for periodic refresh and command
//!   processing. [`Controller::oneshot()`](Controller::oneshot) provides a
//!   lightweight fire-and-forget mode for single CLI invocations.
//!
//! - **[`DataStore`]** — Lock-free reactive storage built on
//!   `EntityCollection<T>` (`DashMap` + `tokio::sync::watch` channels). Merges
//!   Integration and Legacy API responses into canonical domain types.
//!
//! - **[`EntityStream<T>`]** — Subscription handle vended by the `DataStore`.
//!   Exposes `current()` / `latest()` / `changed()` for TUI reactive rendering.
//!
//! - **[`Command`]** — Typed mutation requests routed through an `mpsc` channel
//!   to the controller's command processor. Reads bypass the channel via
//!   direct `DataStore` snapshots or ad-hoc API queries.
//!
//! - **Domain model** ([`model`]) — Canonical types (`Device`, `Client`,
//!   `Network`, `FirewallPolicy`, `Event`, etc.) with [`EntityId`] supporting
//!   both UUID (Integration API) and string-based (Legacy API) identifiers.

pub mod command;
pub mod config;
pub mod controller;
pub mod convert;
pub mod error;
pub mod model;
pub mod store;
pub mod stream;

// ── Primary re-exports ──────────────────────────────────────────────
pub use command::requests::*;
pub use command::{Command, CommandResult};
pub use config::{AuthCredentials, ControllerConfig, TlsVerification};
pub use controller::{ConnectionState, Controller};
pub use error::CoreError;
pub use store::DataStore;
pub use stream::EntityStream;

// Re-export model types at the crate root for ergonomics.
pub use model::{
    // Firewall
    AclRule,
    // Legacy resource types
    Admin,
    // Events / alarms
    Alarm,
    // Core entities
    Client,
    ClientType,
    Country,
    Device,
    DeviceState,
    DeviceType,
    DpiApplication,
    DpiCategory,
    EntityId,
    Event,
    EventCategory,
    EventSeverity,
    FirewallPolicy,
    FirewallZone,
    HealthSummary,
    MacAddress,
    Network,
    RadiusProfile,
    Site,
    SysInfo,
    SystemInfo,
    // Supporting types
    TrafficMatchingList,
    VpnServer,
    VpnTunnel,
    WanInterface,
};
