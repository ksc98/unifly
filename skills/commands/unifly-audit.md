---
name: unifly-audit
description: Security audit of UniFi configuration — checks for open WiFi, permissive firewall rules, and misconfigurations
arguments:
  - name: profile
    description: "Config profile to use (e.g., home, office)"
    required: false
---

# UniFi Security Audit

Perform a security-focused audit of the UniFi configuration.

## Execution

Run each check and compile findings into a security report.

### 1. WiFi Security Check

Identify SSIDs with weak or no security:

```bash
unifly wifi list -p {{profile}} -o json | jq '[
  .[] | {
    name,
    security,
    enabled,
    alert: (if .security == "open" then "CRITICAL: Open network"
            elif .security == "wpa2-personal" then "MODERATE: WPA2 only"
            else "OK" end)
  } | select(.alert != "OK")
]'
```

### 2. Firewall Policy Review

Check for overly permissive rules:

```bash
# All allow rules
unifly firewall policies list -p {{profile}} -o json | jq '[
  .[] | select(.action == "ALLOW") | {id, description, source_zone, dest_zone}
]'

# Rules without logging
unifly firewall policies list -p {{profile}} -o json | jq '[
  .[] | select(.logging == false) | {id, description, action}
]'
```

### 3. Device Firmware Check

Identify devices with outdated firmware:

```bash
unifly devices list --all -p {{profile}} -o json | jq '[
  .[] | select(.upgrade_available == true) | {name, mac, model, current_version}
]'
```

### 4. Network Isolation Check

Verify IoT and guest networks are properly isolated:

```bash
unifly networks list --all -p {{profile}} -o json | jq '[
  .[] | {name, vlan_id, isolation}
]'
```

### 5. Unused Resources

Find networks with no cross-references:

```bash
unifly networks list --all -p {{profile}} -o json
```

For each network, check `unifly networks refs <id>` for cross-references.

When `-p {{profile}}` is provided, prepend `-p -p {{profile}}` to all commands.

## Result Reporting

Compile findings into a report with severity levels:

- **CRITICAL** — Open WiFi networks, no firewall between zones
- **HIGH** — WPA2-only networks, allow-all firewall rules, no logging
- **MODERATE** — Outdated firmware, unused networks
- **INFO** — Network isolation status, device counts

Recommend specific remediation steps for each finding.
