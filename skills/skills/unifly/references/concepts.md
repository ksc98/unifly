# UniFi Networking Concepts

Reference for UniFi networking concepts relevant to unifly operations.

## Architecture

### Controller

The UniFi controller (UniFi OS Console) is the central management platform for
all UniFi network devices. It runs on dedicated hardware (UDM, UDM Pro, UCG)
or as a self-hosted application. unifly communicates with the controller via
its REST APIs.

### Sites

A controller can manage multiple **sites** — logical groupings of devices and
configuration. Each site has its own networks, firewall rules, WiFi SSIDs, and
client database. Most unifly commands operate within a single site context
(set via `--site` or the config profile).

### Device Types

| Prefix | Type             | Examples                      |
| ------ | ---------------- | ----------------------------- |
| UDM    | Dream Machine    | UDM, UDM Pro, UDM SE          |
| UCG    | Cloud Gateway    | UCG Ultra, UCG Max            |
| USG    | Security Gateway | USG, USG Pro                  |
| USW    | Switch           | USW Lite 8, USW Pro 24 PoE    |
| UAP    | Access Point     | U6 Pro, U6 Enterprise, U7 Pro |
| UNVR   | NVR              | UNVR, UNVR Pro                |
| UXBG   | Building Bridge  | UXBG Pro                      |

### Device States

- **ONLINE** — Device is connected and operating normally
- **OFFLINE** — Device is unreachable
- **PENDING** — Device discovered but not yet adopted
- **ADOPTING** — Adoption in progress
- **UPGRADING** — Firmware upgrade in progress
- **PROVISIONING** — Configuration being applied

## Networking

### VLANs

Virtual LANs segment the network at Layer 2. Each UniFi network can have a
VLAN ID (1-4094). The default network typically uses VLAN 1 or is untagged.

Common VLAN design:

| VLAN | Name    | Subnet         | Purpose         |
| ---- | ------- | -------------- | --------------- |
| 1    | Default | 192.168.1.0/24 | Management      |
| 10   | Trusted | 10.0.10.0/24   | Trusted devices |
| 20   | Guest   | 10.0.20.0/24   | Guest access    |
| 30   | IoT     | 10.0.30.0/24   | IoT devices     |
| 40   | Cameras | 10.0.40.0/24   | Surveillance    |

### Network Management Types

- **Gateway** — Routed network with DHCP, NAT, firewall (most common)
- **Switch** — Layer 2 only, no routing
- **Unmanaged** — No UniFi management, pass-through

### DHCP

UniFi networks support three DHCP modes:

- **Server** — Controller acts as DHCP server (most common)
- **Relay** — Forward DHCP requests to external server
- **None** — No DHCP, static IPs only

Configuration includes IP range (start/end), lease time, DNS servers,
and optional DHCP options.

### IPv6

UniFi supports dual-stack networking with:

- **SLAAC** — Stateless Address Autoconfiguration
- **DHCPv6** — Stateful IPv6 address assignment
- **Prefix delegation** — Automatic prefix assignment from upstream

## Security

### Firewall Zones

UniFi uses a **zone-based firewall** model. Zones group networks, and
policies control traffic between zone pairs.

Built-in zones:

- **Internal** — Default zone for LAN networks
- **External** — WAN/Internet traffic
- **DMZ** — Demilitarized zone for public-facing services
- **VPN** — VPN traffic zone

Custom zones can be created and networks attached to them.

### Firewall Policies

Policies define traffic rules between source and destination zones:

- **Action** — `allow`, `block`, or `reject`
- **Direction** — Implied by source/destination zone pair
- **Logging** — Enable traffic logging for the rule
- **Order** — Policies evaluate in order; first match wins

Policy evaluation order matters. Use `unifly firewall policies reorder` to
control precedence.

### Access Control Lists (ACLs)

ACLs provide device-level access control independent of firewall zones:

- **IPv4 ACLs** — Filter by IP address or subnet
- **MAC ACLs** — Filter by MAC address

### Traffic Matching Lists

Reusable lists of ports, IPs, or subnets that can be referenced by
firewall policies. Avoids duplicating the same set of addresses across
multiple rules.

## WiFi

### Security Modes

| Mode               | Protocol | Auth       | Use Case          |
| ------------------ | -------- | ---------- | ----------------- |
| Open               | None     | None       | Captive portals   |
| WPA2 Personal      | WPA2-PSK | Passphrase | Home/small office |
| WPA3 Personal      | SAE      | Passphrase | Modern devices    |
| WPA2/WPA3 Personal | Mixed    | Passphrase | Compatibility     |
| WPA2 Enterprise    | 802.1X   | RADIUS     | Corporate         |
| WPA3 Enterprise    | 802.1X   | RADIUS     | High security     |

### Band Steering

Encourages dual-band clients to prefer 5 GHz over 2.4 GHz for better
performance. Enable on networks where most clients support 5 GHz.

### Fast Roaming (802.11r)

Reduces roaming time between access points for latency-sensitive
applications (VoIP, video). Some older clients may have compatibility
issues.

### SSID Types

- **Standard** — Normal WiFi network
- **IoT** — Optimized for IoT devices (2.4 GHz only, lower power)

## Guest Access

### Hotspot Portal

UniFi supports captive portal guest access with:

- **Vouchers** — Pre-generated access codes with time/data limits
- **Password** — Simple shared password
- **External portal** — Redirect to custom authentication

### Voucher Parameters

- **Duration** — How long the voucher grants access (minutes)
- **Guest limit** — Concurrent devices per voucher
- **Data limit** — Total transfer cap (MB)
- **Rate limits** — Upload/download bandwidth caps (Kbps)

### Client Authorization

Individual clients can be authorized/unauthorized for guest access
without vouchers, with optional duration and rate limits.

## Monitoring

### Events

UniFi generates events for network activity:

- Device state changes (online, offline, restart)
- Client connections/disconnections
- Security events (IDS/IPS alerts)
- Configuration changes
- Firmware updates

Events can be viewed historically or streamed in real-time via WebSocket.

### Alarms

Alarms are persistent alerts that require acknowledgment:

- Connectivity issues
- Security threats
- Hardware warnings
- Configuration conflicts

Archive alarms after addressing them.

### Statistics

Historical statistics are available at multiple intervals:

- **5 minute** — High-resolution, short retention
- **Hourly** — Medium resolution
- **Daily** — Long-term trends
- **Monthly** — Capacity planning

Available metrics: bytes transferred, client counts, CPU/memory usage,
WAN latency, packet loss.

### DPI (Deep Packet Inspection)

Traffic analysis by application or category:

- **By app** — Individual application traffic (Netflix, YouTube, etc.)
- **By category** — Grouped traffic (Streaming, Social Media, etc.)

DPI data can be filtered by client MAC for per-device analysis.

## Dual API Architecture

unifly uses two APIs for maximum coverage:

### Integration API

- **Auth:** API key via header
- **Endpoints:** Full CRUD for networks, WiFi, firewall, clients, devices
- **Format:** Modern JSON with UUIDs
- **Best for:** Configuration management, automation

### Legacy API

- **Auth:** Cookie + CSRF token
- **Endpoints:** Events, statistics, device commands, site management
- **Format:** Envelope-wrapped JSON with string IDs
- **Best for:** Monitoring, historical data, device operations

### Hybrid Mode

Combines both APIs for complete coverage. API key handles CRUD operations
while credentials enable events, stats, and legacy device commands.
This is the recommended mode for full functionality.
