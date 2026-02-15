---
name: unifly
description: >-
  This skill should be used when the user asks to "manage UniFi devices",
  "configure UniFi networks", "check UniFi status", "create firewall rules",
  "manage WiFi SSIDs", "view UniFi clients", "adopt a device", "create a VLAN",
  "set up DNS records", "manage hotspot vouchers", "check network health",
  "audit firewall policies", "restart a UniFi device", "block a client",
  "run a speed test", "create a backup", "check VPN tunnels",
  "cycle a PoE port", or any task involving UniFi network infrastructure
  management via the unifly CLI. Also triggers on mentions of unifly, UniFi,
  UDM, UCG, USG, USW, UAP, or UniFi controller operations.
---

# unifly — UniFi Network Management

unifly is a CLI and TUI for managing Ubiquiti UniFi network infrastructure.
It provides full CRUD operations across 20+ entity types, real-time monitoring,
and automation-friendly output formats. unifly communicates with UniFi controllers
via dual APIs (Integration API + Legacy API) for maximum coverage.

## Prerequisites

unifly must be installed and configured before use. Verify availability:

```bash
command -v unifly >/dev/null 2>&1 && unifly --version || echo "unifly not installed"
```

**Installation:**

```bash
# Homebrew
brew install hyperb1iss/tap/unifly

# Shell script
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hyperb1iss/unifly/releases/latest/download/unifly-installer.sh | sh

# From source
cargo install --git https://github.com/hyperb1iss/unifly.git unifly
```

After installation, run `unifly config init` to set up a controller profile,
or see `examples/config.toml` for manual configuration.

## When to Use

- Query or modify UniFi infrastructure: devices, clients, networks, WiFi, firewall
- Automate network operations: bulk changes, scheduled tasks, incident response
- Monitor network health: real-time stats, events, alarms, DPI traffic analysis
- Audit configurations: firewall rules, network topology, access controls
- Generate reports: traffic statistics, client usage, device performance

## Quick Start

### Authentication

unifly supports three authentication modes:

| Mode          | Auth Method           | Best For                       |
| ------------- | --------------------- | ------------------------------ |
| `integration` | API key               | Full CRUD via official API     |
| `legacy`      | Username + password   | Events, stats, device commands |
| `hybrid`      | API key + credentials | Maximum coverage (recommended) |

Configure a profile:

```bash
# Interactive setup wizard
unifly config init

# Or set values directly
unifly config set profiles.home.controller "https://192.168.1.1"
unifly config set profiles.home.auth_mode "hybrid"
unifly config set profiles.home.api_key "YOUR_API_KEY"
unifly config set profiles.home.username "admin"
unifly config set-password home  # stores in OS keyring
```

### Configuration

Config lives at `~/.config/unifly/config.toml`. See `examples/config.toml` for a
complete profile example.

Resolution priority: CLI flags > environment variables > config file > defaults.

Key environment variables: `UNIFI_CONTROLLER`, `UNIFI_API_KEY`, `UNIFI_SITE`,
`UNIFI_USERNAME`, `UNIFI_PASSWORD`.

## Command Structure

All commands follow the pattern:

```
unifly [global-flags] <entity> <action> [args] [flags]
```

### Entity Types & Actions

| Entity              | Actions                                                                                                    | Description              |
| ------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------ |
| `devices`           | list, get, adopt, remove, restart, locate, port-cycle, stats, pending, upgrade, provision, speedtest, tags | Network hardware         |
| `clients`           | list, get, authorize, unauthorize, block, unblock, kick, forget                                            | Connected endpoints      |
| `networks`          | list, get, create, update, delete, refs                                                                    | VLANs & subnets          |
| `wifi`              | list, get, create, update, delete                                                                          | SSIDs & broadcasts       |
| `firewall policies` | list, get, create, update, patch, delete, reorder                                                          | Traffic rules            |
| `firewall zones`    | list, get, create, update, delete                                                                          | Security zones           |
| `acl`               | list, get, create, update, delete, reorder                                                                 | Access control lists     |
| `dns`               | list, get, create, update, delete                                                                          | Local DNS records        |
| `traffic-lists`     | list, get, create, update, delete                                                                          | Traffic matching lists   |
| `hotspot`           | list, create, delete, purge                                                                                | Guest vouchers           |
| `vpn`               | servers, tunnels                                                                                           | VPN infrastructure       |
| `sites`             | list, create, delete                                                                                       | Controller sites         |
| `events`            | list, watch                                                                                                | Event log & stream       |
| `alarms`            | list, archive, archive-all                                                                                 | Alert management         |
| `stats`             | site, device, client, gateway, dpi                                                                         | Statistics & reports     |
| `system`            | info, health, sysinfo, backup, reboot, poweroff                                                            | Controller operations    |
| `admin`             | list, invite, revoke, update                                                                               | Administrator management |
| `wans`              | list                                                                                                       | WAN interfaces           |
| `dpi`               | apps, categories                                                                                           | Deep packet inspection   |
| `radius`            | profiles                                                                                                   | RADIUS profiles          |
| `config`            | init, show, set, profiles, use, set-password                                                               | CLI configuration        |
| `completions`       | bash, zsh, fish, powershell, elvish                                                                        | Shell completions        |

For the complete command reference with all flags and arguments, consult
`references/commands.md`.

## Output Formats

All list/get commands support multiple output formats via `--output` / `-o`:

| Format       | Flag              | Use Case                                |
| ------------ | ----------------- | --------------------------------------- |
| table        | `-o table`        | Human-readable display (default)        |
| json         | `-o json`         | Programmatic processing, piping to `jq` |
| json-compact | `-o json-compact` | Single-line JSON for scripting          |
| yaml         | `-o yaml`         | Configuration files, documentation      |
| plain        | `-o plain`        | One value per line, simple scripting    |

**For automation, always use `-o json`** to get structured, parseable output.

```bash
# Get all device MACs as JSON
unifly devices list -o json | jq '.[].mac'

# Get specific device details
unifly devices get "aa:bb:cc:dd:ee:ff" -o json
```

## Filtering & Pagination

All list commands support:

```bash
# Limit results
unifly devices list --limit 10

# Pagination
unifly clients list --limit 25 --offset 50

# Fetch all pages
unifly clients list --all

# Filter (Integration API syntax)
unifly devices list --filter "state.eq('ONLINE')"
unifly networks list --filter "name.contains('IoT')"
```

## Global Flags

```
-p, --profile <NAME>      Use a specific config profile
-c, --controller <URL>    Override controller URL
-s, --site <SITE>         Target site (name or UUID)
-o, --output <FORMAT>     Output format
-k, --insecure            Accept self-signed TLS certificates
-v, --verbose             Increase verbosity (-vvv for max)
-q, --quiet               Suppress non-error output
-y, --yes                 Skip confirmation prompts
--timeout <SECS>          Request timeout (default: 30)
--color <MODE>            Color mode (auto|always|never)
```

## Common Patterns

### Read-Only Inspection

```bash
# Overview
unifly system health -o json
unifly devices list -o json
unifly clients list --all -o json

# Deep dive into a device
unifly devices get "aa:bb:cc:dd:ee:ff" -o json
unifly devices stats "aa:bb:cc:dd:ee:ff" -o json
```

### Network Configuration

```bash
# Create a VLAN network
unifly networks create --name "IoT" --vlan-id 30 \
  --management-type gateway --ipv4-host 10.0.30.1 --ipv4-prefix 24 \
  --dhcp-mode server --dhcp-start 10.0.30.100 --dhcp-end 10.0.30.254

# Create a WiFi SSID on that network
unifly wifi create --name "IoT-WiFi" --security wpa2-personal \
  --passphrase "SecurePass123" --network-id "<network-uuid>"
```

### Firewall Management

```bash
# List policies between zones
unifly firewall policies list -o json

# Create a block rule
unifly firewall policies create --action block \
  --description "Block IoT to LAN" --logging true

# Reorder policies
unifly firewall policies reorder --source-zone <id> --dest-zone <id> \
  --policy-ids "<id1>,<id2>,<id3>"
```

### Device Operations

```bash
# Adopt a pending device
unifly devices pending
unifly devices adopt "aa:bb:cc:dd:ee:ff"

# Restart a device
unifly devices restart "aa:bb:cc:dd:ee:ff"

# Power-cycle a PoE port
unifly devices port-cycle "aa:bb:cc:dd:ee:ff" 5

# Run WAN speed test
unifly devices speedtest
```

### Monitoring & Events

```bash
# Real-time event stream
unifly events watch

# Filter by event type
unifly events watch --type "EVT_SW_*"

# Historical stats (hourly, last 24h)
unifly stats site --interval hourly --start "2024-01-01T00:00:00Z"

# DPI traffic analysis
unifly stats dpi --group-by app
```

### Guest & Hotspot

```bash
# Create vouchers (10 vouchers, 24h each)
unifly hotspot create --count 10 --duration 1440

# Authorize a guest client
unifly clients authorize <client-id> --duration 480

# Revoke guest access
unifly clients unauthorize <client-id>
```

### Backups

```bash
# Create a backup
unifly system backup create

# List and download
unifly system backup list
unifly system backup download "autobackup_2024-01-15.unf"
```

## TUI Dashboard

For real-time monitoring, use the TUI:

```bash
unifly-tui
```

Navigate with `1`-`8` for screens: Dashboard, Devices, Clients, Networks,
Firewall, Topology, Events, Stats. Press `/` to search, `Enter` for details,
`Esc` to go back, `q` to quit.

Recommend the TUI for interactive monitoring sessions; prefer CLI commands
for automated or one-shot operations.

## Tips for Agents

1. **Always use `-o json`** for programmatic output — parse with `jq` or directly
2. **Use `--yes`** to skip confirmation prompts in automated workflows
3. **Use `--quiet`** to suppress informational output when only exit codes matter
4. **Chain with `&&`** for atomic multi-step operations
5. **Check `unifly system health`** first to verify controller connectivity
6. **Use `--all`** on list commands to avoid pagination surprises
7. **Prefer `--filter`** over post-processing when the API supports it
8. **Use profiles** (`-p`) to target different controllers without reconfiguring

## Additional Resources

### Reference Files

For detailed specifications, consult:

- **`references/commands.md`** — Complete command reference with all flags, arguments, and examples
- **`references/concepts.md`** — UniFi networking concepts (devices, VLANs, zones, security models)
- **`references/workflows.md`** — Automation workflows, bulk operations, monitoring patterns

### Example Files

Working configurations in `examples/`:

- **`examples/config.toml`** — Complete multi-profile configuration file
