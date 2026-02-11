// Legacy API client modules
//
// Hand-written client for the UniFi controller's legacy (non-OpenAPI) endpoints.
// Covers stat/, cmd/, rest/, and system-level operations wrapped in the
// standard `{ meta: { rc, msg }, data: [...] }` envelope.

pub mod client;
pub mod models;
pub mod auth;
pub mod devices;
pub mod clients;
pub mod sites;
pub mod stats;
pub mod events;
pub mod system;

pub use client::LegacyClient;
