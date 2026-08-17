# unifly-api

[![Crates.io](https://img.shields.io/crates/v/unifly-api.svg)](https://crates.io/crates/unifly-api)
[![Documentation](https://docs.rs/unifly-api/badge.svg)](https://docs.rs/unifly-api)
[![License](https://img.shields.io/crates/l/unifly-api.svg)](https://github.com/hyperb1iss/unifly/blob/main/LICENSE)

Async Rust client for UniFi controller APIs.

## Overview

`unifly-api` provides the HTTP transport layer for communicating with Ubiquiti UniFi Network controllers. It supports two distinct API surfaces:

- **Integration API** — RESTful OpenAPI-based interface authenticated via `X-API-KEY` header. Primary surface for CRUD operations on devices, clients, networks, firewall rules, and other managed entities.
- **Legacy API** — Session/cookie-authenticated endpoints under `/api/s/{site}/`. Used for data not yet exposed by the Integration API: events, traffic stats, admin users, DPI data, system info, and real-time WebSocket events.

Both clients share a common `TransportConfig` for reqwest-based HTTP transport with configurable TLS verification (system CA, custom PEM, or danger-accept for self-signed controllers) and timeout settings.

## Features

- Integration API client with API key authentication
- Legacy API client with cookie/CSRF token handling
- WebSocket event stream with auto-reconnect
- Configurable TLS modes (system CA, custom CA bundle, danger-accept-invalid)
- Async/await with `tokio` runtime
- Comprehensive error types with context
- Support for UniFi OS and standalone controller platforms

## Quick Example

```rust
use unifly_api::{IntegrationClient, TransportConfig, TlsMode, ControllerPlatform};
use secrecy::SecretString;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure transport with TLS verification disabled (for self-signed certs)
    let transport = TransportConfig::new(TlsMode::DangerAcceptInvalid);

    // Create Integration API client
    let client = IntegrationClient::from_api_key(
        "https://192.168.1.1",
        &SecretString::from("your-api-key"),
        &transport,
        ControllerPlatform::UnifiOs,
    )?;

    // Fetch devices from the default site
    let devices = client.list_devices("default").await?;
    println!("Found {} devices", devices.len());

    Ok(())
}
```

For a higher-level abstraction with reactive data streams and automatic data merging, see [unifly-core](https://crates.io/crates/unifly-core).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/hyperb1iss/unifly/blob/main/LICENSE) for details.
