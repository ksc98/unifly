// unifi-api: Async Rust client for UniFi controller APIs (Integration + Legacy)

pub mod error;
pub mod auth;
pub mod integration;
pub mod legacy;
pub mod transport;
pub mod websocket;

pub use error::Error;
pub use auth::{AuthStrategy, Credentials, ControllerPlatform};
pub use integration::IntegrationClient;
pub use integration::types as integration_types;
pub use legacy::LegacyClient;
pub use legacy::models as legacy_models;
pub use transport::{TransportConfig, TlsMode};
