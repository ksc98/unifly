---
name: unifly-status
description: Quick health check of UniFi infrastructure — shows controller health, device status, and alerts
arguments:
  - name: profile
    description: "Config profile to use (e.g., home, office)"
    required: false
---

# UniFi Status Check

Run a comprehensive health check of the UniFi infrastructure.

## Execution

1. Verify controller connectivity via `unifly system health`
2. Count online vs offline devices via `unifly devices list --all -o json`
3. Check for active alarms via `unifly alarms list --unarchived -o json`
4. Report findings in a clear summary

### Basic Status

```bash
unifly system health -p {{profile}} -o json
```

### Full Status Report

```bash
# Controller health
unifly system health -p {{profile}} -o json

# Device summary
unifly devices list --all -p {{profile}} -o json | jq '{
  total: length,
  online: [.[] | select(.state == "ONLINE")] | length,
  offline: [.[] | select(.state == "OFFLINE")] | length,
  pending: [.[] | select(.state == "PENDING")] | length
}'

# Active alarms
unifly alarms list --unarchived -p {{profile}} -o json | jq 'length'

# Client count
unifly clients list --all -p {{profile}} -o json | jq 'length'
```

When `-p {{profile}}` is provided, prepend `-p -p {{profile}}` to all commands.

## Result Reporting

Summarize the health check:

- Controller status (healthy/degraded/unreachable)
- Device counts by state
- Number of active alarms (highlight if > 0)
- Connected client count
- Any offline devices (list names and MACs)
