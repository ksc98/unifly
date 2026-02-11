// Legacy API HTTP client
//
// Wraps `reqwest::Client` with UniFi-specific URL construction, envelope
// unwrapping, and platform-aware path prefixing. All endpoint modules
// (devices, clients, etc.) are implemented as inherent methods via
// separate files to keep this module focused on transport mechanics.

use std::sync::Arc;

use reqwest::cookie::Jar;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::debug;
use url::Url;

use crate::auth::ControllerPlatform;
use crate::error::Error;
use crate::legacy::models::LegacyResponse;

/// Raw HTTP client for the UniFi controller's legacy API.
///
/// Handles the `{ data: [], meta: { rc, msg } }` envelope, site-scoped
/// URL construction, and platform-aware path prefixing. All methods return
/// unwrapped `data` payloads -- the envelope is stripped before the caller
/// sees it.
pub struct LegacyClient {
    http: reqwest::Client,
    base_url: Url,
    site: String,
    platform: ControllerPlatform,
}

impl LegacyClient {
    /// Create a new legacy client.
    ///
    /// Builds a `reqwest::Client` with cookie storage enabled. The `base_url`
    /// should be the controller root (e.g. `https://192.168.1.1` for UniFi OS
    /// or `https://controller:8443` for standalone).
    pub fn new(base_url: Url, site: String, platform: ControllerPlatform) -> Self {
        let jar = Arc::new(Jar::default());
        let http = reqwest::Client::builder()
            .cookie_provider(jar)
            .danger_accept_invalid_certs(true)
            .build()
            .expect("failed to build reqwest client");

        Self {
            http,
            base_url,
            site,
            platform,
        }
    }

    /// Create a legacy client with a pre-built `reqwest::Client`.
    ///
    /// Use this when you already have a client with a session cookie in its
    /// jar (e.g. after authenticating via a shared client).
    pub fn with_client(
        http: reqwest::Client,
        base_url: Url,
        site: String,
        platform: ControllerPlatform,
    ) -> Self {
        Self {
            http,
            base_url,
            site,
            platform,
        }
    }

    /// The current site identifier.
    pub fn site(&self) -> &str {
        &self.site
    }

    /// The underlying HTTP client (for auth flows that need direct access).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The controller base URL.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// The detected controller platform.
    pub fn platform(&self) -> ControllerPlatform {
        self.platform
    }

    // ── URL builders ─────────────────────────────────────────────────

    /// Build a full URL for a controller-level API path.
    ///
    /// Applies the platform-specific legacy prefix, then appends `/api/{path}`.
    /// For example, on UniFi OS: `https://host/proxy/network/api/{path}`
    pub(crate) fn api_url(&self, path: &str) -> Url {
        let prefix = self.platform.legacy_prefix().unwrap_or("");
        let full = format!("{}{}/api/{}", self.base_url, prefix.trim_end_matches('/'), path);
        Url::parse(&full).expect("invalid API URL")
    }

    /// Build a site-scoped URL: `{base}{prefix}/api/s/{site}/{path}`
    ///
    /// Most legacy endpoints are site-scoped: stat/device, cmd/devmgr, etc.
    pub(crate) fn site_url(&self, path: &str) -> Url {
        let prefix = self.platform.legacy_prefix().unwrap_or("");
        let full = format!(
            "{}{}/api/s/{}/{}",
            self.base_url,
            prefix.trim_end_matches('/'),
            self.site,
            path
        );
        Url::parse(&full).expect("invalid site URL")
    }

    // ── Request helpers ──────────────────────────────────────────────

    /// Send a GET request and unwrap the legacy envelope.
    pub(crate) async fn get<T: DeserializeOwned>(&self, url: Url) -> Result<Vec<T>, Error> {
        debug!("GET {}", url);

        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(Error::Transport)?;

        self.parse_envelope(resp).await
    }

    /// Send a POST request with JSON body and unwrap the legacy envelope.
    pub(crate) async fn post<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &impl Serialize,
    ) -> Result<Vec<T>, Error> {
        debug!("POST {}", url);

        let resp = self
            .http
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(Error::Transport)?;

        self.parse_envelope(resp).await
    }

    /// Parse the `{ meta, data }` envelope, returning `data` on success
    /// or an `Error::LegacyApi` if `meta.rc != "ok"`.
    async fn parse_envelope<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<Vec<T>, Error> {
        let status = resp.status();

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Authentication {
                message: "session expired or invalid credentials".into(),
            });
        }

        let body = resp.text().await.map_err(Error::Transport)?;

        let envelope: LegacyResponse<T> =
            serde_json::from_str(&body).map_err(|e| Error::Deserialization {
                message: e.to_string(),
                body: body.clone(),
            })?;

        match envelope.meta.rc.as_str() {
            "ok" => Ok(envelope.data),
            _ => Err(Error::LegacyApi {
                message: envelope
                    .meta
                    .msg
                    .unwrap_or_else(|| format!("rc={}", envelope.meta.rc)),
            }),
        }
    }
}
