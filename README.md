# unifi-cli

CLI and TUI for managing UniFi Network controllers.

![Rust](https://img.shields.io/badge/rust-1.86%2B-orange)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

## Features

- **Dual API support** -- Integration API (v10.1.84) for full CRUD operations, Legacy API for events, stats, and device commands
- **Interactive TUI** -- real-time dashboard for monitoring devices, clients, networks, and events
- **Flexible output** -- table, JSON, compact JSON, YAML, and plain text modes
- **Configuration wizard** -- guided `unifi config init` sets up profiles in seconds
- **Keyring-backed credentials** -- API keys and passwords stored securely in your OS keyring
- **Multi-profile support** -- manage multiple controllers with named profiles
- **Shell completions** -- generated completions for bash, zsh, fish, PowerShell, and elvish

## Installation

### From source

```sh
cargo install --git https://github.com/blisshostings/unifi-cli.git unifi-cli
cargo install --git https://github.com/blisshostings/unifi-cli.git unifi-tui
```

### Build from a local clone

```sh
git clone https://github.com/blisshostings/unifi-cli.git
cd unifi-cli
cargo build --release
# Binaries: target/release/unifi, target/release/unifi-tui
```

## Quick Start

Run the interactive setup wizard:

```sh
unifi config init
```

The wizard prompts for your controller URL, authentication method, and site name.
Credentials can be stored in your system keyring or saved to the config file.

Once configured, start querying your controller:

```sh
unifi devices list
unifi clients list
unifi networks list
```

Example output:

```
 ID                                   | Name              | Model           | Status
--------------------------------------+-------------------+-----------------+---------
 a1b2c3d4-e5f6-7890-abcd-ef1234567890 | Office Gateway    | UDM-Pro         | ONLINE
 b2c3d4e5-f6a7-8901-bcde-f12345678901 | Living Room AP    | U6-LR           | ONLINE
 c3d4e5f6-a7b8-9012-cdef-123456789012 | Garage Switch     | USW-Lite-8-PoE  | ONLINE
```

## Authentication

### API Key (recommended)

Generate an API key on your controller under **Settings > Integrations**. This uses the Integration API and supports all CRUD operations.

```sh
unifi config init          # select "API Key" during setup
# or pass directly:
unifi --api-key <KEY> devices list
```

### Username / Password

Legacy session-based authentication using cookie auth. Required for some operations like events and stats that are not yet available in the Integration API.

```sh
unifi config init          # select "Username/Password" during setup
```

### Environment Variables

| Variable         | Description                         |
|------------------|-------------------------------------|
| `UNIFI_API_KEY`  | Integration API key                 |
| `UNIFI_URL`      | Controller URL                      |
| `UNIFI_PROFILE`  | Profile name to use                 |
| `UNIFI_SITE`     | Site name or UUID                   |
| `UNIFI_OUTPUT`   | Default output format               |
| `UNIFI_INSECURE` | Accept self-signed TLS certificates |
| `UNIFI_TIMEOUT`  | Request timeout in seconds          |

## Configuration

Config file location: `~/.config/unifi-cli/config.toml`

```toml
default_profile = "home"

[defaults]
output = "table"
color = "auto"
insecure = false
timeout = 30

[profiles.home]
controller = "https://192.168.1.1"
site = "default"
auth_mode = "integration"
# API key stored in system keyring -- omit from file

[profiles.office]
controller = "https://10.0.0.1"
site = "default"
auth_mode = "legacy"
username = "admin"
# Password stored in system keyring
insecure = true
```

Switch profiles:

```sh
unifi config use office
unifi config profiles       # list profiles (* marks active)
unifi --profile home devices list  # one-off override
```

## Commands

| Command          | Alias | Description                              |
|------------------|-------|------------------------------------------|
| `devices`        | `d`   | Manage adopted and pending devices       |
| `clients`        | `cl`  | Manage connected clients                 |
| `networks`       | `n`   | Manage networks and VLANs                |
| `wifi`           | `w`   | Manage WiFi broadcasts (SSIDs)           |
| `firewall`       | `fw`  | Manage firewall policies and zones       |
| `acl`            |       | Manage ACL rules                         |
| `dns`            |       | Manage DNS policies (local DNS records)  |
| `traffic-lists`  |       | Manage traffic matching lists            |
| `hotspot`        |       | Manage hotspot vouchers                  |
| `vpn`            |       | View VPN servers and tunnels             |
| `sites`          |       | Manage sites                             |
| `events`         |       | View and stream events                   |
| `alarms`         |       | Manage alarms                            |
| `stats`          |       | Query statistics and reports             |
| `system`         | `sys` | System operations and info               |
| `admin`          |       | Administrator management                 |
| `dpi`            |       | DPI reference data                       |
| `radius`         |       | View RADIUS profiles                     |
| `wans`           |       | View WAN interfaces                      |
| `countries`      |       | List available country codes             |
| `config`         |       | Manage CLI configuration and profiles    |
| `completions`    |       | Generate shell completions               |

Most resource commands support `list`, `get`, `create`, `update`, and `delete` subcommands. Use `unifi <command> --help` for full details.

### Global Flags

```
-p, --profile <NAME>     Controller profile to use
-c, --controller <URL>   Controller URL (overrides profile)
-s, --site <SITE>        Site name or UUID
-o, --output <FORMAT>    Output format: table, json, json-compact, yaml, plain
-k, --insecure           Accept self-signed TLS certificates
-v, --verbose            Increase verbosity (-v, -vv, -vvv)
-q, --quiet              Suppress non-error output
-y, --yes                Skip confirmation prompts
    --timeout <SECS>     Request timeout in seconds (default: 30)
    --color <MODE>       Color output: auto, always, never
```

## TUI

The `unifi-tui` binary provides an interactive terminal interface for real-time monitoring.

```sh
unifi-tui
```

Screens accessible via number keys or tab navigation:

1. **Dashboard** -- overview of controller health and summary stats
2. **Devices** -- adopted device list with status and uptime
3. **Clients** -- connected clients with traffic and signal info
4. **Networks** -- VLAN and network configuration
5. **Firewall** -- policies and zones
6. **Topology** -- network topology view
7. **Events** -- live event stream
8. **Stats** -- traffic and performance statistics

## Development

This is a Rust workspace with four crates:

```
crates/
  unifi-api/    # Async HTTP/WS client for UniFi APIs
  unifi-core/   # Business logic, Controller, DataStore
  unifi-cli/    # CLI binary (unifi)
  unifi-tui/    # TUI binary (unifi-tui)
```

Dependency chain: `unifi-api` <- `unifi-core` <- `unifi-cli` / `unifi-tui`

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace
```

Requires Rust 1.86+ (edition 2024).

## License

Licensed under either of [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0) or [MIT License](https://opensource.org/licenses/MIT), at your option.
