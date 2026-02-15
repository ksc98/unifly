# Introduction

Unifly is a complete command-line toolkit for managing Ubiquiti UniFi network controllers. It provides two binaries:

- **`unifly`** — a CLI for scripting, automation, and quick lookups
- **`unifly-tui`** — a real-time terminal dashboard for monitoring

Both are powered by a shared async engine that speaks every UniFi API dialect.

## Why Unifly?

UniFi controllers expose multiple APIs with different capabilities:

- **Integration API** — RESTful, API-key authenticated, covers CRUD for most resources
- **Legacy API** — Session-based with cookie/CSRF, required for events, statistics, and device commands

Unifly unifies these into a single, coherent interface. You don't need to know which API endpoint provides what — unifly handles the routing, authentication, and data merging automatically.

## What You Can Do

| Capability | Description |
|---|---|
| **Device Management** | List, inspect, restart, upgrade, and provision devices |
| **Client Monitoring** | See connected clients with signal, traffic, and VLAN info |
| **Network Configuration** | Manage VLANs, subnets, DHCP, and IPv6 settings |
| **WiFi Management** | Create and modify SSIDs, view radio stats |
| **Firewall** | Manage policies, zones, and ACL rules |
| **Events & Alarms** | Stream live events, acknowledge and archive alarms |
| **Statistics** | Query bandwidth, client counts, and DPI data over time |
| **Real-Time Dashboard** | Monitor everything with live Braille charts and status bars |

## Architecture at a Glance

```
                    unifly (CLI)
                         │
                         ▼
  unifly-tui ───▶ unifly-core ───▶ unifly-api
  (TUI)          (business       (HTTP/WS
                   logic)          transport)
                     │
                     ▼
                unifly-config
                (profiles, keyring)
```

Five crates with a clean dependency chain. See the [Architecture](/architecture/) section for the full picture.

## Next Steps

- [Installation](/guide/installation) — get unifly on your system
- [Quick Start](/guide/quick-start) — configure and run your first commands
- [Configuration](/guide/configuration) — deep dive into profiles and settings
