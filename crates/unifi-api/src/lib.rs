// unifi-api: Async Rust client for UniFi controller APIs (Integration + Legacy)

pub mod error;
pub mod auth;
pub mod legacy;
pub mod websocket;

pub use error::Error;
pub use auth::{AuthStrategy, Credentials, ControllerPlatform};
