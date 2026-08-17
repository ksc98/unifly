# unifly-core

[![Crates.io](https://img.shields.io/crates/v/unifly-core.svg)](https://crates.io/crates/unifly-core)
[![Documentation](https://docs.rs/unifly-core/badge.svg)](https://docs.rs/unifly-core)
[![License](https://img.shields.io/crates/l/unifly-core.svg)](https://github.com/hyperb1iss/unifly/blob/main/LICENSE)

High-level reactive controller for UniFi networks.

## Overview

`unifly-core` provides business logic and reactive data infrastructure for UniFi Network applications. It sits between the low-level `unifly-api` transport layer and UI consumers (CLI/TUI), offering:

- **Controller** lifecycle management with `connect()`, `disconnect()`, and `oneshot()` modes
- **Reactive DataStore** with lock-free entity collections and watch-based streams for real-time updates
- **Domain model** with 20+ canonical types (`Device`, `Client`, `Network`, `FirewallPolicy`, etc.)
- **Command dispatch** for CRUD operations routed through an `mpsc` channel
- **Automatic data merge** from Integration and Legacy APIs into unified domain types

## Features

- Controller lifecycle with authentication, background refresh, and WebSocket event processing
- Reactive `DataStore` with `EntityStream` subscriptions for live data updates
- Lock-free storage using `DashMap` and `tokio::sync::watch` channels
- Support for both UUID-based (Integration API) and string-based (Legacy API) entity IDs
- 20+ domain model types covering devices, clients, networks, firewall, VPN, DPI, events
- Command processor for mutations with automatic store updates
- Hybrid API support: merges Integration + Legacy API data for complete network view

## Quick Example

```rust
use unifly_core::{Controller, ControllerConfig, AuthCredentials, TlsVerification};
use secrecy::SecretString;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure controller with API key authentication
    let config = ControllerConfig {
        base_url: "https://192.168.1.1".parse()?,
        auth: AuthCredentials::ApiKey(SecretString::from("your-api-key")),
        tls: TlsVerification::DangerAcceptInvalid,
        ..Default::default()
    };

    // Create and connect to controller
    let controller = Controller::new(config);
    controller.connect().await?;

    // Subscribe to device updates (reactive stream)
    let stream = controller.devices();
    let devices = stream.current();
    println!("Found {} devices", devices.len());

    // Watch for changes
    for device in devices.iter() {
        println!("  - {} ({})", device.name, device.mac);
    }

    Ok(())
}
```

For low-level API access without the reactive layer, see [unifly-api](https://crates.io/crates/unifly-api).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/hyperb1iss/unifly/blob/main/LICENSE) for details.
