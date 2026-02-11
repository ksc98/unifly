// unifi-core: Business logic and shared services

pub mod error;
pub mod model;

pub use error::CoreError;

// Re-export the most commonly used model types at the crate root for ergonomics.
pub use model::{
    Client, ClientType, Device, DeviceState, DeviceType, EntityId, Event, MacAddress, Network,
    Site,
};
