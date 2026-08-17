# Crate Structure

## unifly-api

**Role:** Async HTTP/WebSocket transport layer.

Handles all network communication with UniFi controllers:

- **Integration API client** — RESTful endpoints with API key authentication
- **Legacy API client** — Session-based with cookie and CSRF token handling
- **WebSocket client** — Real-time event streaming
- **TLS** — Custom `rustls` configuration for self-signed certificates

Key design decisions:

- Uses `reqwest` with cookie jar for session management
- CSRF tokens captured from login response, rotated via `X-Updated-CSRF-Token` header
- WebSocket reconnection handled at the transport level

## unifly-core

**Role:** Business logic and shared services.

The heart of the system:

- **Controller** — Lifecycle management (connect, authenticate, fetch, disconnect)
- **DataStore** — `DashMap`-based entity storage with `tokio::watch` channels
- **Entity models** — Strongly-typed Rust structs for all 20+ UniFi resource types
- **Background tasks** — Periodic refresh (30s) and command processing
- **Data merging** — Integration API + Legacy API data combined per entity

Provides two connection modes:

- `Controller::connect()` — Full lifecycle with background refresh and WebSocket events
- `Controller::oneshot()` — Fire-and-forget for CLI commands (no background tasks)

## unifly-config

**Role:** Configuration and credential management.

- **Profile system** — Named profiles for multiple controllers
- **Keyring integration** — OS-native credential storage via the `keyring` crate
- **TOML config** — File-based settings at `~/.config/unifly/config.toml`
- **Environment overlay** — Environment variables override file config
- **Setup wizard** — Interactive configuration with `dialoguer`

## unifly

**Role:** CLI binary.

Thin shell over `unifly-core`:

- **clap-derived** command tree with 20+ resource commands
- **Output formatting** — Table, JSON, YAML, plain text via `tabled`
- **Shell completions** — Bash, Zsh, Fish via `clap_complete`
- **Man pages** — Generated at build time via `clap_mangen`

## unifly-tui

**Role:** Terminal UI binary.

Real-time dashboard built with `ratatui`:

- **8 screens** — Dashboard, Devices, Clients, Networks, Firewall, Topology, Events, Stats
- **Data bridge** — Translates `Controller` events into TUI actions
- **SilkCircuit theme** — Custom color palette with the project's visual identity
- **Braille charts** — High-resolution terminal graphs using Unicode Braille patterns
- **Reactive rendering** — Only re-renders on data changes via `EntityStream` subscriptions
