// Integration API client for the UniFi Network Application.
//
// Hand-crafted async HTTP client matching the v10.1.84 OpenAPI spec.
// Uses X-API-KEY authentication and RESTful JSON endpoints at /integration/v1/.

pub mod client;
pub mod types;

pub use client::IntegrationClient;
