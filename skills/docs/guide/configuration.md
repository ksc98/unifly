# Configuration

## Config File

Configuration lives at `~/.config/unifly/config.toml`:

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
auth_mode = "hybrid"
# API key + password stored in OS keyring

[profiles.office]
controller = "https://10.0.0.1"
site = "default"
auth_mode = "legacy"
username = "admin"
insecure = true
```

## Profile Management

```bash
unifly config init             # Interactive setup wizard
unifly config use office       # Switch active profile
unifly config profiles         # List profiles (* marks active)
unifly --profile home devices  # One-off override
```

## Environment Variables

All settings can be overridden via environment variables:

| Variable | Description | Example |
|---|---|---|
| `UNIFI_API_KEY` | Integration API key | `abc123...` |
| `UNIFI_URL` | Controller URL | `https://192.168.1.1` |
| `UNIFI_PROFILE` | Profile name | `home` |
| `UNIFI_SITE` | Site name or UUID | `default` |
| `UNIFI_OUTPUT` | Default output format | `json` |
| `UNIFI_INSECURE` | Accept self-signed certs | `true` |
| `UNIFI_TIMEOUT` | Request timeout (seconds) | `60` |

## Precedence

Settings are resolved in this order (highest priority first):

1. CLI flags (`--controller`, `--site`, etc.)
2. Environment variables (`UNIFI_URL`, etc.)
3. Profile-specific config in `config.toml`
4. Default values from `[defaults]` section

## Global Flags

These flags work with every command:

```
-p, --profile <NAME>     Controller profile to use
-c, --controller <URL>   Controller URL (overrides profile)
-s, --site <SITE>        Site name or UUID
-o, --output <FORMAT>    Output: table, json, json-compact, yaml, plain
-k, --insecure           Accept self-signed TLS certificates
-v, --verbose            Increase verbosity (-v, -vv, -vvv)
-q, --quiet              Suppress non-error output
-y, --yes                Skip confirmation prompts
    --timeout <SECS>     Request timeout (default: 30)
    --color <MODE>       Color: auto, always, never
```

## TLS Certificates

UniFi controllers use self-signed certificates by default. To accept them:

```bash
# Per-command
unifly -k devices list

# Per-profile
# Set insecure = true in the profile config

# Via environment
export UNIFI_INSECURE=true
```

::: warning
Only use `--insecure` with controllers you trust. It disables TLS certificate verification.
:::
