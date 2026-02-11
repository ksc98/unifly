//! Async Rust client for UniFi controller APIs.
//!
//! This crate provides the HTTP transport layer for communicating with UniFi
//! Network controllers. It supports two distinct API surfaces:
//!
//! - **Integration API** ([`IntegrationClient`]) — RESTful OpenAPI-based interface
//!   authenticated via `X-API-KEY` header. Primary surface for CRUD operations on
//!   devices, clients, networks, firewall rules, and other managed entities.
//!
//! - **Legacy API** ([`LegacyClient`]) — Session/cookie-authenticated endpoints under
//!   `/api/s/{site}/`. Used for data not yet exposed by the Integration API: events,
//!   traffic stats, admin users, DPI data, system info, and real-time WebSocket events.
//!
//! Both clients share a common [`TransportConfig`] for reqwest-based HTTP transport
//! with configurable TLS ([`TlsMode`]: system CA, custom PEM, or danger-accept for
//! self-signed controllers) and timeout settings.
//!
//! Higher-level consumers (e.g. `unifi-core`) compose both clients behind a unified
//! [`Controller`](../unifi_core/struct.Controller.html) facade and merge their
//! responses into canonical domain types.

pub mod auth;
pub mod error;
pub mod integration;
pub mod legacy;
pub mod transport;
pub mod websocket;

pub use auth::{AuthStrategy, ControllerPlatform, Credentials};
pub use error::Error;
pub use integration::IntegrationClient;
pub use integration::types as integration_types;
pub use legacy::LegacyClient;
pub use legacy::models as legacy_models;
pub use transport::{TlsMode, TransportConfig};
