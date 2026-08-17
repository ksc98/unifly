# Quick Start

## Interactive Setup

Run the setup wizard to configure your first controller profile:

```bash
unifly config init
```

The wizard walks you through:

1. **Controller URL** — your UniFi controller's address (e.g., `https://192.168.1.1`)
2. **Authentication** — API key, username/password, or hybrid mode
3. **Site selection** — choose which site to manage
4. **TLS settings** — accept self-signed certificates if needed

Credentials are stored in your OS keyring — never written to disk in plaintext.

## First Commands

Once configured, explore your network:

```bash
# List all adopted devices
unifly devices list

# See connected clients
unifly clients list

# View networks and VLANs
unifly networks list

# Stream live events
unifly events stream
```

Example output:

```
 ID                                   | Name            | Model           | Status
--------------------------------------+-----------------+-----------------+--------
 a1b2c3d4-e5f6-7890-abcd-ef1234567890 | Office Gateway  | UDM-Pro         | ONLINE
 b2c3d4e5-f6a7-8901-bcde-f12345678901 | Living Room AP  | U6-LR           | ONLINE
 c3d4e5f6-a7b8-9012-cdef-123456789012 | Garage Switch   | USW-Lite-8-PoE  | ONLINE
```

## Output Formats

Every command supports multiple output formats:

```bash
unifly devices list                  # Default table
unifly devices list -o json          # Full JSON
unifly devices list -o json-compact  # Minified JSON (pipe-friendly)
unifly devices list -o yaml          # YAML
unifly devices list -o plain         # Plain text
```

## Launch the TUI

For real-time monitoring, launch the terminal dashboard:

```bash
unifly-tui                   # Default profile
unifly-tui -p office         # Specific profile
unifly-tui -v                # Verbose logging to /tmp/unifly-tui.log
```

Navigate screens with number keys `1`-`8` or `Tab`/`Shift+Tab`. Press `q` to quit.

## Multiple Controllers

Add more profiles for different controllers:

```bash
unifly config init                    # Add another profile
unifly config profiles                # List all profiles
unifly config use office              # Switch default profile
unifly -p home devices list           # One-off override
```

## Next Steps

- [Configuration](/guide/configuration) — all config options and environment variables
- [Authentication](/guide/authentication) — API key vs password vs hybrid
- [CLI Commands](/reference/cli) — full command reference
- [TUI Dashboard](/reference/tui) — screen-by-screen guide
