// unifi-api: Async Rust client for UniFi controller APIs (Integration + Legacy)

pub mod error;
pub mod legacy;
pub mod websocket;

pub use error::Error;
