# unifly Command Reference

Complete reference for all unifly CLI commands, flags, and arguments.

## Devices

### `unifly devices list`

List all adopted devices.

```bash
unifly devices list [--limit N] [--offset N] [--all] [--filter EXPR] [-o FORMAT]
```

### `unifly devices get <id|mac>`

Get detailed information about a specific device.

```bash
unifly devices get "aa:bb:cc:dd:ee:ff" -o json
unifly devices get "device-uuid" -o json
```

### `unifly devices adopt <mac>`

Adopt a device pending adoption.

```bash
unifly devices adopt "aa:bb:cc:dd:ee:ff" [--ignore-limit]
```

Flags:

- `--ignore-limit` — Adopt even if device limit is reached

### `unifly devices remove <id|mac>`

Remove (unadopt) a device from the controller.

```bash
unifly devices remove "aa:bb:cc:dd:ee:ff"
```

### `unifly devices restart <id|mac>`

Reboot a device.

```bash
unifly devices restart "aa:bb:cc:dd:ee:ff"
```

### `unifly devices locate <mac>`

Toggle the locate LED on a device (blink to physically identify it).

```bash
unifly devices locate "aa:bb:cc:dd:ee:ff"
```

### `unifly devices port-cycle <id|mac> <port_idx>`

Power-cycle a PoE port on a switch. The port index is zero-based.

```bash
unifly devices port-cycle "aa:bb:cc:dd:ee:ff" 5
```

### `unifly devices stats <id|mac>`

Get real-time statistics for a device (CPU, memory, throughput, clients).

```bash
unifly devices stats "aa:bb:cc:dd:ee:ff" -o json
```

### `unifly devices pending`

List devices awaiting adoption.

```bash
unifly devices pending -o json
```

### `unifly devices upgrade <mac>`

Upgrade device firmware. Optionally specify a custom firmware URL.

```bash
unifly devices upgrade "aa:bb:cc:dd:ee:ff"
unifly devices upgrade "aa:bb:cc:dd:ee:ff" --url "https://fw.example.com/firmware.bin"
```

### `unifly devices provision <mac>`

Force re-provision of device configuration.

```bash
unifly devices provision "aa:bb:cc:dd:ee:ff"
```

### `unifly devices speedtest`

Run a WAN speed test on the gateway.

```bash
unifly devices speedtest -o json
```

### `unifly devices tags`

List device tags.

```bash
unifly devices tags -o json
```

---

## Clients

### `unifly clients list`

List all connected clients.

```bash
unifly clients list [--limit N] [--offset N] [--all] [--filter EXPR] [-o FORMAT]
```

### `unifly clients get <id|mac>`

Get detailed information about a specific client.

```bash
unifly clients get "aa:bb:cc:dd:ee:ff" -o json
```

### `unifly clients authorize <client_id>`

Grant guest network access to a client.

```bash
unifly clients authorize <client_id> \
  [--duration MINUTES] \
  [--data-limit-mb N] \
  [--upload-limit-kbps N] \
  [--download-limit-kbps N]
```

Flags:

- `--duration` — Access duration in minutes
- `--data-limit-mb` — Data transfer limit in MB
- `--upload-limit-kbps` — Upload bandwidth limit
- `--download-limit-kbps` — Download bandwidth limit

### `unifly clients unauthorize <client_id>`

Revoke guest access for a client.

```bash
unifly clients unauthorize <client_id>
```

### `unifly clients block <mac>`

Block a client from the network (Legacy API).

```bash
unifly clients block "aa:bb:cc:dd:ee:ff"
```

### `unifly clients unblock <mac>`

Unblock a previously blocked client (Legacy API).

```bash
unifly clients unblock "aa:bb:cc:dd:ee:ff"
```

### `unifly clients kick <mac>`

Disconnect a wireless client (Legacy API). The client may reconnect.

```bash
unifly clients kick "aa:bb:cc:dd:ee:ff"
```

### `unifly clients forget <mac>`

Remove a client from the controller's history entirely (Legacy API).

```bash
unifly clients forget "aa:bb:cc:dd:ee:ff"
```

---

## Networks

### `unifly networks list`

List all configured networks.

```bash
unifly networks list [--limit N] [--all] [-o FORMAT]
```

### `unifly networks get <id>`

Get detailed network configuration including IPv4, DHCP, IPv6 settings.

```bash
unifly networks get "network-uuid" -o json
```

### `unifly networks create`

Create a new network (VLAN).

```bash
unifly networks create \
  --name "IoT" \
  --vlan-id 30 \
  --management-type gateway \
  --ipv4-host 10.0.30.1 \
  --ipv4-prefix 24 \
  --dhcp-mode server \
  --dhcp-start 10.0.30.100 \
  --dhcp-end 10.0.30.254 \
  [--zone <zone-id>] \
  [--isolation true|false]
```

Flags:

- `--name` — Network name (required)
- `--vlan-id` — VLAN ID (1-4094)
- `--management-type` — `gateway`, `switch`, or `unmanaged`
- `--ipv4-host` — Gateway IP address
- `--ipv4-prefix` — Subnet prefix length (e.g., 24 = /24)
- `--dhcp-mode` — `server`, `relay`, or `none`
- `--dhcp-start` — DHCP range start IP
- `--dhcp-end` — DHCP range end IP
- `--zone` — Firewall zone to attach to
- `--isolation` — Enable client isolation

### `unifly networks update <id>`

Update an existing network.

```bash
unifly networks update "network-uuid" \
  [--name "New Name"] \
  [--enabled true|false] \
  [--vlan-id N]
```

### `unifly networks delete <id>`

Delete a network.

```bash
unifly networks delete "network-uuid" [--force]
```

- `--force` — Delete even if resources are attached

### `unifly networks refs <id>`

Show cross-references — what entities reference this network (WiFi SSIDs,
firewall zones, port profiles, etc.).

```bash
unifly networks refs "network-uuid" -o json
```

---

## WiFi

### `unifly wifi list`

List all WiFi broadcasts (SSIDs).

```bash
unifly wifi list [-o FORMAT]
```

### `unifly wifi get <id>`

Get SSID configuration details.

```bash
unifly wifi get "wifi-uuid" -o json
```

### `unifly wifi create`

Create a new WiFi broadcast.

```bash
unifly wifi create \
  --name "Guest WiFi" \
  --type standard \
  --security wpa2-personal \
  --passphrase "SecurePass123!" \
  --network-id "network-uuid" \
  [--frequency 2.4ghz|5ghz|both] \
  [--band-steering true|false] \
  [--fast-roaming true|false]
```

Flags:

- `--name` — SSID name (required)
- `--type` — `standard` or `iot`
- `--security` — `open`, `wpa2-personal`, `wpa3-personal`, `wpa2-wpa3-personal`, `wpa2-enterprise`, `wpa3-enterprise`
- `--passphrase` — WiFi password (required for personal security)
- `--network-id` — Associated network UUID
- `--frequency` — Radio frequency band
- `--band-steering` — Enable band steering
- `--fast-roaming` — Enable 802.11r fast roaming

### `unifly wifi update <id>`

Update an existing SSID.

```bash
unifly wifi update "wifi-uuid" \
  [--name "New SSID"] \
  [--passphrase "NewPass456!"] \
  [--enabled true|false]
```

### `unifly wifi delete <id>`

Delete a WiFi broadcast.

```bash
unifly wifi delete "wifi-uuid"
```

---

## Firewall

### Policies

#### `unifly firewall policies list`

List all firewall policies.

```bash
unifly firewall policies list [-o FORMAT]
```

#### `unifly firewall policies get <id>`

Get firewall policy details.

```bash
unifly firewall policies get "policy-uuid" -o json
```

#### `unifly firewall policies create`

Create a new firewall policy.

```bash
unifly firewall policies create \
  --action allow|block|reject \
  --description "Allow IoT to DNS" \
  [--logging true|false]
```

#### `unifly firewall policies update <id>`

Update a firewall policy.

```bash
unifly firewall policies update "policy-uuid" \
  [--action allow|block|reject] \
  [--description "Updated description"] \
  [--logging true|false]
```

#### `unifly firewall policies patch <id>`

Quick enable/disable toggle for a policy.

```bash
unifly firewall policies patch "policy-uuid" --enabled true|false
```

#### `unifly firewall policies delete <id>`

Delete a firewall policy.

```bash
unifly firewall policies delete "policy-uuid"
```

#### `unifly firewall policies reorder`

Reorder firewall policies between zone pairs to control evaluation order.

```bash
unifly firewall policies reorder \
  --source-zone "zone-uuid" \
  --dest-zone "zone-uuid" \
  --policy-ids "id1,id2,id3"
```

### Zones

#### `unifly firewall zones list`

List all firewall zones.

```bash
unifly firewall zones list [-o FORMAT]
```

#### `unifly firewall zones get <id>`

Get zone details including attached networks.

```bash
unifly firewall zones get "zone-uuid" -o json
```

#### `unifly firewall zones create`

Create a custom firewall zone.

```bash
unifly firewall zones create \
  --name "IoT Zone" \
  --network-ids "net-uuid-1,net-uuid-2"
```

#### `unifly firewall zones update <id>`

Update a zone.

```bash
unifly firewall zones update "zone-uuid" \
  [--name "Renamed Zone"] \
  [--network-ids "net-uuid-1,net-uuid-2"]
```

#### `unifly firewall zones delete <id>`

Delete a zone.

```bash
unifly firewall zones delete "zone-uuid"
```

---

## ACL (Access Control Lists)

### `unifly acl list`

List ACL rules.

```bash
unifly acl list [-o FORMAT]
```

### `unifly acl get <id>`

Get ACL rule details.

```bash
unifly acl get "acl-uuid" -o json
```

### `unifly acl create`

Create an ACL rule.

```bash
unifly acl create \
  --type ipv4|mac \
  --action allow|block \
  [additional flags per type]
```

### `unifly acl update <id>`

Update an ACL rule.

```bash
unifly acl update "acl-uuid" [flags]
```

### `unifly acl delete <id>`

Delete an ACL rule.

```bash
unifly acl delete "acl-uuid"
```

### `unifly acl reorder`

Reorder ACL rules.

```bash
unifly acl reorder --rule-ids "id1,id2,id3"
```

---

## DNS

### `unifly dns list`

List local DNS policies/records.

```bash
unifly dns list [-o FORMAT]
```

### `unifly dns get <id>`

Get DNS record details.

```bash
unifly dns get "dns-uuid" -o json
```

### `unifly dns create`

Create a DNS record.

```bash
unifly dns create \
  --type A|AAAA|CNAME|MX|TXT|SRV|Forward \
  --domain "app.local" \
  --value "10.0.1.50" \
  [--ttl 3600] \
  [--priority 10]
```

Supported record types:

| Type    | Description    | Value Format                  |
| ------- | -------------- | ----------------------------- |
| A       | IPv4 address   | `10.0.1.50`                   |
| AAAA    | IPv6 address   | `fd00::1`                     |
| CNAME   | Canonical name | `other.local`                 |
| MX      | Mail exchange  | `mail.example.com`            |
| TXT     | Text record    | `"v=spf1 ..."`                |
| SRV     | Service record | `target:port:weight:priority` |
| Forward | DNS forwarding | `8.8.8.8`                     |

### `unifly dns update <id>`

Update a DNS record.

```bash
unifly dns update "dns-uuid" [--value "10.0.1.51"] [--ttl 7200]
```

### `unifly dns delete <id>`

Delete a DNS record.

```bash
unifly dns delete "dns-uuid"
```

---

## Traffic Lists

### `unifly traffic-lists list`

List traffic matching lists.

```bash
unifly traffic-lists list [-o FORMAT]
```

### `unifly traffic-lists create`

Create a traffic matching list with port, IPv4, or IPv6 items.

```bash
unifly traffic-lists create \
  --name "Blocked Ports" \
  --type ports|ipv4|ipv6 \
  --items "80,443,8080"
```

### `unifly traffic-lists update <id>`

Update a traffic list.

```bash
unifly traffic-lists update "list-uuid" [--name "..."] [--items "..."]
```

### `unifly traffic-lists delete <id>`

Delete a traffic list.

```bash
unifly traffic-lists delete "list-uuid"
```

---

## Hotspot (Vouchers)

### `unifly hotspot list`

List guest vouchers.

```bash
unifly hotspot list [-o FORMAT]
```

### `unifly hotspot create`

Generate guest vouchers.

```bash
unifly hotspot create \
  --count 10 \
  --duration 1440 \
  [--guest-limit 1] \
  [--data-limit-mb 500] \
  [--upload-limit-kbps 5000] \
  [--download-limit-kbps 10000]
```

Flags:

- `--count` — Number of vouchers to generate
- `--duration` — Duration in minutes (1440 = 24 hours)
- `--guest-limit` — Max concurrent guests per voucher
- `--data-limit-mb` — Data cap in MB
- `--upload-limit-kbps` — Upload bandwidth cap
- `--download-limit-kbps` — Download bandwidth cap

### `unifly hotspot delete <id>`

Delete a single voucher.

```bash
unifly hotspot delete "voucher-uuid"
```

### `unifly hotspot purge`

Bulk delete vouchers matching a filter.

```bash
unifly hotspot purge --filter "status.eq('UNUSED')"
```

---

## VPN

### `unifly vpn servers`

List VPN server configurations.

```bash
unifly vpn servers [-o FORMAT]
```

### `unifly vpn tunnels`

List site-to-site VPN tunnels.

```bash
unifly vpn tunnels [-o FORMAT]
```

---

## Sites

### `unifly sites list`

List sites on the controller.

```bash
unifly sites list [-o FORMAT]
```

### `unifly sites create`

Create a new site (Legacy API).

```bash
unifly sites create --name "Branch Office"
```

### `unifly sites delete`

Delete a site (Legacy API).

```bash
unifly sites delete --name "Branch Office"
```

---

## Events

### `unifly events list`

List recent events.

```bash
unifly events list [--hours 24] [-o FORMAT]
```

- `--hours` — Lookback period (default: 24)

### `unifly events watch`

Stream real-time events via WebSocket.

```bash
unifly events watch [--type "EVT_SW_*"]
```

- `--type` — Filter by event type pattern (glob matching)

---

## Alarms

### `unifly alarms list`

List alarms.

```bash
unifly alarms list [--unarchived] [-o FORMAT]
```

- `--unarchived` — Show only active (unarchived) alarms

### `unifly alarms archive <id>`

Archive a single alarm.

```bash
unifly alarms archive "alarm-id"
```

### `unifly alarms archive-all`

Archive all alarms.

```bash
unifly alarms archive-all
```

---

## Statistics

### `unifly stats site`

Site-level statistics.

```bash
unifly stats site \
  [--interval 5m|hourly|daily|monthly] \
  [--start "2024-01-01T00:00:00Z"] \
  [--end "2024-01-31T23:59:59Z"] \
  [--attrs "bytes,num_sta"] \
  [-o FORMAT]
```

### `unifly stats device`

Per-device statistics.

```bash
unifly stats device \
  [--mac "aa:bb:cc:dd:ee:ff"] \
  [--interval hourly] \
  [--start "..."] [--end "..."] \
  [-o FORMAT]
```

### `unifly stats client`

Per-client statistics.

```bash
unifly stats client \
  [--mac "aa:bb:cc:dd:ee:ff"] \
  [--interval hourly] \
  [-o FORMAT]
```

### `unifly stats gateway`

Gateway statistics.

```bash
unifly stats gateway [--interval hourly] [-o FORMAT]
```

### `unifly stats dpi`

Deep packet inspection traffic analysis.

```bash
unifly stats dpi \
  [--group-by app|category] \
  [--mac "aa:bb:cc:dd:ee:ff"] \
  [-o FORMAT]
```

Flags common to all stats commands:

- `--interval` — Aggregation interval: `5m`, `hourly`, `daily`, `monthly`
- `--start` — Start of time range (ISO 8601)
- `--end` — End of time range (ISO 8601)
- `--attrs` — Comma-separated attribute names to include
- `--mac` — Filter by specific device/client MAC

---

## System

### `unifly system info`

Show application version information.

```bash
unifly system info [-o FORMAT]
```

### `unifly system health`

Show site health summary.

```bash
unifly system health [-o FORMAT]
```

### `unifly system sysinfo`

Show controller system information.

```bash
unifly system sysinfo [-o FORMAT]
```

### `unifly system backup create`

Create a controller backup.

```bash
unifly system backup create
```

### `unifly system backup list`

List available backups.

```bash
unifly system backup list [-o FORMAT]
```

### `unifly system backup download <filename>`

Download a backup file.

```bash
unifly system backup download "autobackup_2024-01-15.unf"
```

### `unifly system backup delete <filename>`

Delete a backup.

```bash
unifly system backup delete "autobackup_2024-01-15.unf"
```

### `unifly system reboot`

Reboot the controller (UDM only).

```bash
unifly system reboot
```

### `unifly system poweroff`

Power off the controller (UDM only).

```bash
unifly system poweroff
```

---

## Admin

### `unifly admin list`

List site administrators.

```bash
unifly admin list [-o FORMAT]
```

### `unifly admin invite`

Invite a new administrator.

```bash
unifly admin invite \
  --name "Jane Admin" \
  --email "jane@example.com" \
  --role admin|readonly|viewer
```

### `unifly admin revoke`

Remove administrator access.

```bash
unifly admin revoke --email "jane@example.com"
```

### `unifly admin update`

Change an administrator's role.

```bash
unifly admin update --email "jane@example.com" --role readonly
```

---

## DPI

### `unifly dpi apps`

List DPI applications.

```bash
unifly dpi apps [-o FORMAT]
```

### `unifly dpi categories`

List DPI categories.

```bash
unifly dpi categories [-o FORMAT]
```

---

## RADIUS

### `unifly radius profiles`

List RADIUS profiles.

```bash
unifly radius profiles [-o FORMAT]
```

---

## WANs

### `unifly wans list`

List WAN interfaces.

```bash
unifly wans list [-o FORMAT]
```

---

## Config

### `unifly config init`

Interactive setup wizard for first-time configuration.

```bash
unifly config init
```

### `unifly config show`

Display the resolved configuration.

```bash
unifly config show
```

### `unifly config set <key> <value>`

Set a configuration value.

```bash
unifly config set profiles.home.controller "https://192.168.1.1"
unifly config set profiles.home.auth_mode "hybrid"
unifly config set profiles.home.api_key "your-api-key"
```

### `unifly config profiles`

List configured profiles.

```bash
unifly config profiles
```

### `unifly config use <name>`

Set the default profile.

```bash
unifly config use home
```

### `unifly config set-password <profile>`

Store a password in the OS keyring.

```bash
unifly config set-password home
```

---

## Completions

Generate shell completions.

```bash
unifly completions bash > ~/.bash_completion.d/unifly
unifly completions zsh > ~/.zfunc/_unifly
unifly completions fish > ~/.config/fish/completions/unifly.fish
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.
