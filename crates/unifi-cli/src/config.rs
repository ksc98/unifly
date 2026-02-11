//! CLI-owned configuration: TOML profiles, credential resolution, and
//! translation to `unifi_core::ControllerConfig`.
//!
//! Core never sees these types -- it receives a pre-built `ControllerConfig`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use directories::ProjectDirs;
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use unifi_core::{AuthCredentials, ControllerConfig, TlsVerification};

use crate::cli::GlobalOpts;
use crate::error::CliError;

// ── TOML config structs ──────────────────────────────────────────────

/// CLI-owned TOML configuration. Core never touches this type.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Default profile name (used when --profile is not specified).
    pub default_profile: Option<String>,

    /// Global defaults.
    #[serde(default)]
    pub defaults: Defaults,

    /// Named controller profiles.
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: Some("default".into()),
            defaults: Defaults::default(),
            profiles: HashMap::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Defaults {
    #[serde(default = "default_output")]
    pub output: String,

    #[serde(default = "default_color")]
    pub color: String,

    #[serde(default)]
    pub insecure: bool,

    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            output: default_output(),
            color: default_color(),
            insecure: false,
            timeout: default_timeout(),
        }
    }
}

fn default_output() -> String {
    "table".into()
}
fn default_color() -> String {
    "auto".into()
}
fn default_timeout() -> u64 {
    30
}

/// CLI-owned profile definition.
#[derive(Debug, Deserialize, Serialize)]
pub struct Profile {
    /// Controller base URL (e.g., "https://192.168.1.1").
    pub controller: String,

    /// Site name or UUID.
    #[serde(default = "default_site")]
    pub site: String,

    /// Auth mode: "integration" (API key) or "legacy" (username/password).
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,

    /// API key (plaintext -- prefer keyring or env var).
    pub api_key: Option<String>,

    /// Environment variable name containing the API key.
    pub api_key_env: Option<String>,

    /// Username for legacy auth.
    pub username: Option<String>,

    /// Password for legacy auth (plaintext -- prefer keyring).
    pub password: Option<String>,

    /// Path to custom CA certificate.
    pub ca_cert: Option<PathBuf>,

    /// Override insecure TLS setting.
    pub insecure: Option<bool>,

    /// Override timeout.
    pub timeout: Option<u64>,
}

fn default_site() -> String {
    "default".into()
}
fn default_auth_mode() -> String {
    "integration".into()
}

// ── Config file path ─────────────────────────────────────────────────

/// Resolve the config file path via XDG / platform conventions.
pub fn config_path() -> PathBuf {
    ProjectDirs::from("com", "unifi-cli", "unifi-cli")
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .unwrap_or_else(|| {
            let mut p = dirs_fallback();
            p.push("config.toml");
            p
        })
}

fn dirs_fallback() -> PathBuf {
    let mut p = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    p.push(".config");
    p.push("unifi-cli");
    p
}

// ── Config loading ───────────────────────────────────────────────────

/// Load the full Config from file + environment.
pub fn load_config() -> Result<Config, CliError> {
    let path = config_path();

    let figment = Figment::new()
        .merge(Serialized::defaults(Config::default()))
        .merge(Toml::file(&path))
        .merge(Env::prefixed("UNIFI_").split("_"));

    let config: Config = figment.extract()?;
    Ok(config)
}

/// Load config, returning a default if the file doesn't exist.
pub fn load_config_or_default() -> Config {
    load_config().unwrap_or_default()
}

// ── Profile resolution ───────────────────────────────────────────────

/// Resolve the active profile name from CLI flags and config.
pub fn active_profile_name(global: &GlobalOpts, config: &Config) -> String {
    global
        .profile
        .clone()
        .or_else(|| config.default_profile.clone())
        .unwrap_or_else(|| "default".into())
}

/// Translate a CLI `Profile` + global flags into a `ControllerConfig`.
///
/// This is the single boundary where CLI config types cross into core types.
pub fn resolve_profile(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<ControllerConfig, CliError> {
    // 1. Controller URL (flag > env > profile)
    let url_str = global.controller.as_deref().unwrap_or(&profile.controller);
    let url: url::Url = url_str.parse().map_err(|_| CliError::Validation {
        field: "controller".into(),
        reason: format!("invalid URL: {url_str}"),
    })?;

    // 2. Auth credentials
    let auth = match profile.auth_mode.as_str() {
        "integration" => {
            let secret = resolve_api_key(profile, profile_name, global)?;
            AuthCredentials::ApiKey(secret)
        }
        "legacy" => {
            let (username, password) = resolve_legacy_credentials(profile, profile_name)?;
            AuthCredentials::Credentials { username, password }
        }
        other => {
            return Err(CliError::Validation {
                field: "auth_mode".into(),
                reason: format!("expected 'integration' or 'legacy', got '{other}'"),
            })
        }
    };

    // 3. TLS verification
    let tls = if global.insecure || profile.insecure.unwrap_or(false) {
        TlsVerification::DangerAcceptInvalid
    } else if let Some(ref ca_path) = profile.ca_cert {
        TlsVerification::CustomCa(ca_path.clone())
    } else {
        TlsVerification::SystemDefaults
    };

    // 4. Site (flag > env > profile)
    let site = global
        .site
        .as_deref()
        .unwrap_or(&profile.site)
        .to_string();

    // 5. Timeout
    let timeout = Duration::from_secs(global.timeout);

    Ok(ControllerConfig {
        url,
        auth,
        site,
        tls,
        timeout,
        refresh_interval_secs: 0,
        websocket_enabled: false,
        polling_interval_secs: 30,
    })
}

// ── Credential helpers ───────────────────────────────────────────────

/// Resolve an API key from the credential chain.
fn resolve_api_key(
    profile: &Profile,
    profile_name: &str,
    global: &GlobalOpts,
) -> Result<SecretString, CliError> {
    // 1. CLI flag
    if let Some(ref key) = global.api_key {
        return Ok(SecretString::from(key.clone()));
    }

    // 2. Profile's api_key_env -> env var lookup
    if let Some(ref env_name) = profile.api_key_env {
        if let Ok(val) = std::env::var(env_name) {
            return Ok(SecretString::from(val));
        }
    }

    // 3. System keyring
    if let Ok(entry) = keyring::Entry::new("unifi-cli", &format!("{profile_name}/api-key")) {
        if let Ok(secret) = entry.get_password() {
            return Ok(SecretString::from(secret));
        }
    }

    // 4. Plaintext in config
    if let Some(ref key) = profile.api_key {
        return Ok(SecretString::from(key.clone()));
    }

    Err(CliError::NoCredentials {
        profile: profile_name.into(),
    })
}

/// Resolve legacy credentials (username + password).
fn resolve_legacy_credentials(
    profile: &Profile,
    profile_name: &str,
) -> Result<(String, SecretString), CliError> {
    let username = profile
        .username
        .clone()
        .or_else(|| std::env::var("UNIFI_USERNAME").ok())
        .ok_or_else(|| CliError::NoCredentials {
            profile: profile_name.into(),
        })?;

    // 1. Env var
    if let Ok(pw) = std::env::var("UNIFI_PASSWORD") {
        return Ok((username, SecretString::from(pw)));
    }

    // 2. Keyring
    if let Ok(entry) = keyring::Entry::new("unifi-cli", &format!("{profile_name}/password")) {
        if let Ok(pw) = entry.get_password() {
            return Ok((username, SecretString::from(pw)));
        }
    }

    // 3. Plaintext in config
    if let Some(ref pw) = profile.password {
        return Ok((username, SecretString::from(pw.clone())));
    }

    Err(CliError::NoCredentials {
        profile: profile_name.into(),
    })
}
