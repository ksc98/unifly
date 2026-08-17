# Authentication

Unifly supports three authentication modes, each suited to different use cases.

## API Key (Recommended)

Generate a key on your controller under **Settings > Integrations**. Provides full CRUD access via the Integration API.

```bash
unifly config init                     # Select "API Key" during setup
unifly --api-key <KEY> devices list    # Or pass directly
```

**Pros:** Simplest setup, no session management, works with all Integration API endpoints.

**Limitation:** No access to Legacy API features (events, statistics, device commands).

## Username / Password

Legacy session-based auth with cookie and CSRF token handling. Required for events, statistics, and device commands not yet in the Integration API.

```bash
unifly config init                     # Select "Username/Password" during setup
```

**Pros:** Full access to Legacy API features.

**Limitation:** Session tokens expire, requires periodic re-authentication.

## Hybrid Mode

Best of both worlds — API key for Integration API CRUD, username/password for Legacy API features. The setup wizard offers this when both are available.

```bash
unifly config init                     # Select "Hybrid" during setup
```

**Pros:** Complete access to all API features.

**How it works:** Unifly uses the API key for standard CRUD operations and transparently falls back to session auth for Legacy-only endpoints.

## Credential Storage

All credentials are stored in your OS keyring:

| OS | Backend |
|---|---|
| macOS | Keychain |
| Linux | Secret Service (GNOME Keyring, KWallet) |
| Windows | Windows Credential Manager |

Nothing is ever written to disk in plaintext. The `config.toml` file only stores non-sensitive settings like controller URLs and site names.

## Environment Variables

For CI/CD and scripting, pass credentials via environment:

```bash
export UNIFI_API_KEY="your-api-key-here"
export UNIFI_URL="https://192.168.1.1"
unifly devices list
```

::: tip
Environment variables take precedence over profile config but are overridden by CLI flags.
:::
