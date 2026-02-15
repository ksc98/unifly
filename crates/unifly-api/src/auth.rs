use std::sync::Arc;

use reqwest::cookie::Jar;
use secrecy::SecretString;

/// Which authentication strategy to use for a particular API call.
///
/// Marker enum (no data) -- the actual credentials live in [`Credentials`].
/// Useful for branching on auth flow without carrying secret material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStrategy {
    /// Cookie-based session (legacy API, WebSocket).
    Session,
    /// Local API key header (Integration API).
    ApiKey,
    /// Cloud API key (Site Manager, Cloud Connector).
    CloudApiKey,
}

/// Credentials for authenticating with a UniFi controller.
///
/// Each variant carries the secret material needed for its auth flow.
#[derive(Debug, Clone)]
pub enum Credentials {
    /// Cookie-based session auth. The jar holds the session cookie
    /// after a successful login; pass it into the `reqwest::Client` builder.
    Session { cookie_jar: Arc<Jar> },

    /// Local API key for the Integration API.
    /// Generated at: Network > Settings > Control Plane > Integrations.
    ApiKey { key: SecretString },

    /// Cloud API key for Site Manager + Cloud Connector.
    /// Generated at: <https://unifi.ui.com> > Settings > API Keys.
    Cloud { key: SecretString, host_id: String },
}

/// The platform type of the UniFi controller.
///
/// Determines URL prefixes, login paths, and which API surfaces are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerPlatform {
    /// UniFi OS device (UDM, UCG, etc.) -- port 443, `/proxy/network/` prefix.
    UnifiOs,
    /// Standalone Network Application (Java) -- port 8443, no prefix.
    ClassicController,
    /// Cloud-hosted via Site Manager / Cloud Connector (api.ui.com).
    Cloud,
}

impl ControllerPlatform {
    /// The path prefix for legacy API endpoints.
    ///
    /// Returns `None` for [`Cloud`](Self::Cloud) because the legacy API
    /// is not available via the cloud connector.
    pub fn legacy_prefix(&self) -> Option<&'static str> {
        match self {
            Self::UnifiOs => Some("/proxy/network"),
            Self::ClassicController => Some(""),
            Self::Cloud => None,
        }
    }

    /// The path prefix for the Integration API.
    ///
    /// On UniFi OS devices: `/proxy/network/integration`
    /// On standalone / cloud: `/integration`
    pub fn integration_prefix(&self) -> &'static str {
        match self {
            Self::UnifiOs => "/proxy/network/integration",
            Self::ClassicController | Self::Cloud => "/integration",
        }
    }

    /// The login endpoint path.
    ///
    /// Returns `None` for [`Cloud`](Self::Cloud) because cloud uses
    /// API key auth -- no session login needed.
    pub fn login_path(&self) -> Option<&'static str> {
        match self {
            Self::UnifiOs => Some("/api/auth/login"),
            Self::ClassicController => Some("/api/login"),
            Self::Cloud => None,
        }
    }

    /// The logout endpoint path.
    ///
    /// Returns `None` for [`Cloud`](Self::Cloud).
    pub fn logout_path(&self) -> Option<&'static str> {
        match self {
            Self::UnifiOs => Some("/api/auth/logout"),
            Self::ClassicController => Some("/api/logout"),
            Self::Cloud => None,
        }
    }

    /// The WebSocket path template. `{site}` must be replaced by the caller.
    ///
    /// Returns `None` for [`Cloud`](Self::Cloud) because WebSocket
    /// connections are not available via the cloud connector.
    pub fn websocket_path(&self) -> Option<&'static str> {
        match self {
            Self::UnifiOs => Some("/proxy/network/wss/s/{site}/events"),
            Self::ClassicController => Some("/wss/s/{site}/events"),
            Self::Cloud => None,
        }
    }
}
