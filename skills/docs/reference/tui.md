# TUI Dashboard

The `unifly-tui` binary provides a real-time terminal dashboard for monitoring your UniFi network.

## Launch

```bash
unifly-tui                   # Launch with default profile
unifly-tui -p office         # Use a specific profile
unifly-tui -v                # Verbose logging to /tmp/unifly-tui.log
```

## Screens

Navigate with number keys `1`-`8` or `Tab`/`Shift+Tab`:

| Key | Screen | Description |
|---|---|---|
| `1` | **Dashboard** | btop-style overview with six live panels |
| `2` | **Devices** | Adopted devices — model, firmware, IP, uptime, CPU/MEM |
| `3` | **Clients** | Connected clients — hostname, IP, MAC, VLAN, signal, traffic |
| `4` | **Networks** | VLAN topology — subnets, DHCP, IPv6 config |
| `5` | **Firewall** | Policies and zones with rule counts |
| `6` | **Topology** | Network topology tree view |
| `7` | **Events** | Live event stream with severity indicators |
| `8` | **Stats** | Historical charts — WAN bandwidth, client counts, DPI |

## Dashboard Panels

The dashboard packs six live panels into a single view:

- **WAN Traffic** — Braille line chart with live TX/RX rates and peak tracking
- **Gateway** — WAN IP, DNS, latency, uptime, ISP name, IPv6 when available
- **System Health** — Subsystem status dots, CPU/MEM utilization bars, load averages
- **Networks** — VLANs sorted by ID with IPv6 prefix delegation and mode
- **Top Clients** — Proportional traffic bars with fractional block characters
- **Recent Events** — Compact two-per-line event display

## Key Bindings

| Key | Action |
|---|---|
| `1`-`8` | Switch screens |
| `Tab` / `Shift+Tab` | Next / previous screen |
| `j` / `k` | Scroll down / up |
| `Enter` | Open detail view |
| `Esc` | Close detail / go back |
| `/` | Search / filter |
| `?` | Show help |
| `q` | Quit |

## Detail Views

Press `Enter` on any list item to open its detail view. Detail views show comprehensive information about the selected resource with sub-tabs for related data.

## Data Refresh

The TUI refreshes data automatically:

- **Devices and clients** — polled every 30 seconds
- **Health subsystems** — polled every 30 seconds
- **Events** — pushed via WebSocket in real-time
- **Bandwidth** — sampled from device stats on each refresh cycle

## Authentication Modes

The TUI works with all authentication modes:

| Mode | Dashboard | Devices | Clients | Events | Stats |
|---|---|---|---|---|---|
| API Key | Partial | Full | Full | No | No |
| Username/Password | Full | Full | Full | Full | Full |
| Hybrid | Full | Full | Full | Full | Full |

::: tip
Use **Hybrid mode** for the best TUI experience. It provides access to all features including events and statistics that require the Legacy API.
:::

## Graceful Degradation

When data is unavailable (e.g., API-key-only mode without Legacy access), panels show `\u2500` placeholders instead of crashing. The dashboard adapts to whatever data sources are available.
