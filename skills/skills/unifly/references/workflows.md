# Automation Workflows

Common automation patterns using unifly for network management at scale.

## Pre-Flight Checks

Before any automation, verify connectivity and state:

```bash
# Verify controller is reachable
unifly system health -o json -q

# Check current site
unifly system info -o json
```

Always use `-o json` for machine-parseable output and `--yes` to skip
confirmation prompts in automated scripts.

## Network Provisioning

### Create a Complete Network Segment

Full workflow: network + WiFi + firewall zone.

```bash
# 1. Create the network
NETWORK_ID=$(unifly networks create \
  --name "IoT" \
  --vlan-id 30 \
  --management-type gateway \
  --ipv4-host 10.0.30.1 \
  --ipv4-prefix 24 \
  --dhcp-mode server \
  --dhcp-start 10.0.30.100 \
  --dhcp-end 10.0.30.254 \
  -o json | jq -r '.id')

# 2. Create a firewall zone for it
ZONE_ID=$(unifly firewall zones create \
  --name "IoT Zone" \
  --network-ids "$NETWORK_ID" \
  -o json | jq -r '.id')

# 3. Create WiFi SSID on the network
unifly wifi create \
  --name "IoT-WiFi" \
  --type iot \
  --security wpa2-personal \
  --passphrase "IoTSecure2024!" \
  --network-id "$NETWORK_ID"

# 4. Block IoT zone from reaching Internal zone
unifly firewall policies create \
  --action block \
  --description "Block IoT to Internal" \
  --logging true
```

### Bulk DNS Records

Create multiple DNS records from a list:

```bash
# From a CSV: domain,type,value
while IFS=',' read -r domain type value; do
  unifly dns create --domain "$domain" --type "$type" --value "$value" --ttl 3600
done < dns_records.csv
```

## Device Management

### Fleet Firmware Upgrade

Upgrade all devices of a specific type:

```bash
# Get all online switches
unifly devices list -o json --all | \
  jq -r '.[] | select(.type == "USW" and .state == "ONLINE") | .mac' | \
while read -r mac; do
  echo "Upgrading $mac..."
  unifly devices upgrade "$mac" --yes
  sleep 5  # stagger upgrades
done
```

### Adopt All Pending Devices

```bash
unifly devices pending -o json | \
  jq -r '.[].mac' | \
while read -r mac; do
  unifly devices adopt "$mac"
done
```

### PoE Port Reset for Stuck Devices

```bash
# Restart a PoE-powered device by cycling its port
unifly devices port-cycle "switch-mac" 5
```

## Client Management

### Block Rogue Clients

Block clients not matching a known MAC prefix:

```bash
ALLOWED_PREFIX="aa:bb:cc"

unifly clients list --all -o json | \
  jq -r --arg prefix "$ALLOWED_PREFIX" \
  '.[] | select(.mac | startswith($prefix) | not) | .mac' | \
while read -r mac; do
  echo "Blocking unknown client: $mac"
  unifly clients block "$mac"
done
```

### Generate and Distribute Vouchers

```bash
# Generate 50 one-day vouchers
VOUCHERS=$(unifly hotspot create --count 50 --duration 1440 -o json)

# Extract voucher codes
echo "$VOUCHERS" | jq -r '.[].code'
```

## Monitoring & Alerting

### Health Check Script

```bash
#!/usr/bin/env bash
set -euo pipefail

HEALTH=$(unifly system health -o json)
STATUS=$(echo "$HEALTH" | jq -r '.status // "unknown"')

if [ "$STATUS" != "ok" ]; then
  echo "ALERT: Site health is $STATUS"
  # Send notification
fi

# Check for offline devices
OFFLINE=$(unifly devices list --all -o json | \
  jq '[.[] | select(.state == "OFFLINE")] | length')

if [ "$OFFLINE" -gt 0 ]; then
  echo "ALERT: $OFFLINE devices offline"
  unifly devices list --all -o json | \
    jq '.[] | select(.state == "OFFLINE") | {name, mac, last_seen}'
fi
```

### Event Monitoring

```bash
# Stream events and filter for security alerts
unifly events watch --type "EVT_IPS_*" | while read -r event; do
  echo "SECURITY: $event"
  # Forward to SIEM, Slack, etc.
done
```

### Traffic Analysis

```bash
# Top bandwidth consumers (last 24h)
unifly stats client --interval daily -o json | \
  jq 'sort_by(-.rx_bytes + -.tx_bytes) | .[0:10] | .[] | {mac, rx_bytes, tx_bytes}'

# DPI breakdown by category
unifly stats dpi --group-by category -o json | \
  jq 'sort_by(-.bytes) | .[0:10]'
```

## Backup & Recovery

### Automated Backup

```bash
#!/usr/bin/env bash
set -euo pipefail

# Create backup
unifly system backup create --yes

# Wait and download latest
sleep 30
LATEST=$(unifly system backup list -o json | jq -r '.[0].filename')
unifly system backup download "$LATEST"

echo "Backup saved: $LATEST"
```

### Backup Rotation

```bash
# Keep only the 5 most recent backups
unifly system backup list -o json | \
  jq -r '.[5:] | .[].filename' | \
while read -r backup; do
  unifly system backup delete "$backup" --yes
done
```

## Security Audit

### Firewall Policy Audit

```bash
# List all allow policies (potential security gaps)
unifly firewall policies list -o json | \
  jq '.[] | select(.action == "ALLOW") | {description, source_zone, dest_zone}'

# Find policies without logging
unifly firewall policies list -o json | \
  jq '.[] | select(.logging == false) | {id, description}'
```

### Open WiFi Check

```bash
# Find any open (no security) SSIDs
unifly wifi list -o json | \
  jq '.[] | select(.security == "open") | {name, id}'
```

### Unused Network Detection

```bash
# Networks with zero clients
unifly networks list --all -o json | jq -c '.[]' | while read -r net; do
  NET_ID=$(echo "$net" | jq -r '.id')
  NAME=$(echo "$net" | jq -r '.name')
  REFS=$(unifly networks refs "$NET_ID" -o json 2>/dev/null)
  if [ "$(echo "$REFS" | jq 'length')" -eq 0 ]; then
    echo "Unused network: $NAME ($NET_ID)"
  fi
done
```

## Incident Response

### Isolate a Compromised Client

```bash
MAC="aa:bb:cc:dd:ee:ff"

# 1. Block the client immediately
unifly clients block "$MAC"

# 2. Kick from WiFi (if wireless)
unifly clients kick "$MAC"

# 3. Document the event
unifly events list --hours 1 -o json | \
  jq --arg mac "$MAC" '.[] | select(.client // "" | contains($mac))'
```

### Emergency Network Lockdown

```bash
# Disable all guest WiFi SSIDs
unifly wifi list -o json | \
  jq -r '.[] | select(.name | test("guest|visitor"; "i")) | .id' | \
while read -r id; do
  unifly wifi update "$id" --enabled false --yes
done
```

### Device Quarantine

```bash
# Move suspicious device's port to quarantine VLAN
# First create a quarantine network if it doesn't exist
QUARANTINE_ID=$(unifly networks list -o json | \
  jq -r '.[] | select(.name == "Quarantine") | .id')

if [ -z "$QUARANTINE_ID" ]; then
  QUARANTINE_ID=$(unifly networks create \
    --name "Quarantine" \
    --vlan-id 999 \
    --management-type gateway \
    --ipv4-host 10.0.99.1 \
    --ipv4-prefix 24 \
    --dhcp-mode none \
    -o json | jq -r '.id')
fi
```

## Multi-Controller Operations

### Cross-Controller Status

```bash
for profile in home office warehouse; do
  echo "=== $profile ==="
  unifly -p "$profile" system health -o json | jq '.status'
  unifly -p "$profile" devices list --all -o json | jq 'length'
done
```

## Error Handling Patterns

### Retry with Backoff

```bash
retry_unifly() {
  local max_attempts=3
  local delay=5
  local attempt=1

  while [ $attempt -le $max_attempts ]; do
    if output=$("$@" 2>&1); then
      echo "$output"
      return 0
    fi
    echo "Attempt $attempt failed, retrying in ${delay}s..." >&2
    sleep $delay
    delay=$((delay * 2))
    attempt=$((attempt + 1))
  done

  echo "Failed after $max_attempts attempts" >&2
  return 1
}

# Usage
retry_unifly unifly devices list -o json
```

### Validate Before Modify

```bash
# Always read before write to verify entity exists
NETWORK=$(unifly networks get "$NETWORK_ID" -o json 2>/dev/null) || {
  echo "Network $NETWORK_ID not found"
  exit 1
}

# Proceed with update
unifly networks update "$NETWORK_ID" --name "Updated Name"
```

## Agent Best Practices

1. **Inspect before mutating** — Always `get` or `list` an entity before
   `create`, `update`, or `delete` to understand current state
2. **Use JSON output** — Always pass `-o json` for structured, parseable results
3. **Capture IDs** — Store entity IDs from create operations for subsequent steps
4. **Verify after changes** — Re-fetch entities after mutation to confirm success
5. **Use `--yes`** — Skip confirmation prompts for non-interactive execution
6. **Stagger bulk operations** — Add delays between bulk device operations
   to avoid overwhelming the controller
7. **Check health first** — Run `unifly system health` before complex workflows
8. **Handle errors** — Check exit codes and stderr; unifly returns non-zero on failure
9. **Use profiles** — Target different controllers via `-p profile_name`
10. **Log actions** — Use `-v` or capture output for audit trails
