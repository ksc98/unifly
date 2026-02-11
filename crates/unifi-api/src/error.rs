use thiserror::Error;

/// Top-level error type for the `unifi-api` crate.
///
/// Covers every failure mode across all API surfaces:
/// authentication, transport, Integration API, Legacy API, WebSocket, and cloud.
/// `unifi-core` maps these into user-facing diagnostics.
#[derive(Debug, Error)]
pub enum Error {
    // ── Authentication ──────────────────────────────────────────────
    /// Login failed (wrong credentials, account locked, etc.)
    #[error("Authentication failed: {message}")]
    Authentication { message: String },

    /// 2FA token required but not provided.
    #[error("Two-factor authentication token required")]
    TwoFactorRequired,

    /// Session has expired (cookie expired or revoked).
    #[error("Session expired -- re-authentication required")]
    SessionExpired,

    /// Invalid API key (rejected by controller).
    #[error("Invalid API key")]
    InvalidApiKey,

    /// Wrong credential type for the requested operation.
    #[error("Wrong auth strategy: expected {expected}, got {got}")]
    WrongAuthStrategy { expected: String, got: String },

    // ── Transport ───────────────────────────────────────────────────
    /// HTTP transport error (connection refused, DNS failure, etc.)
    #[error("HTTP transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// URL parsing error.
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    /// Request timed out.
    #[error("Request timed out after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    /// TLS handshake or certificate error.
    #[error("TLS error: {0}")]
    Tls(String),

    // ── Cloud ───────────────────────────────────────────────────────
    /// Rate limited by the cloud API. Includes retry-after in seconds.
    #[error("Rate limited -- retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    // ── Integration API ─────────────────────────────────────────────
    /// Structured error from the Integration API.
    #[error("Integration API error (HTTP {status}): {message}")]
    Integration {
        message: String,
        code: Option<String>,
        status: u16,
    },

    // ── Legacy API ──────────────────────────────────────────────────
    /// Error from the legacy API (parsed from the `{meta: {rc, msg}}` envelope).
    #[error("Legacy API error: {message}")]
    LegacyApi { message: String },

    // ── WebSocket ───────────────────────────────────────────────────
    /// WebSocket connection failed.
    #[error("WebSocket connection failed: {0}")]
    WebSocketConnect(String),

    /// WebSocket closed unexpectedly.
    #[error("WebSocket closed (code {code}): {reason}")]
    WebSocketClosed { code: u16, reason: String },

    // ── Data ────────────────────────────────────────────────────────
    /// JSON deserialization failed, with the raw body for debugging.
    #[error("Deserialization error: {message}")]
    Deserialization { message: String, body: String },

    // ── Platform ────────────────────────────────────────────────────
    /// Operation not supported on this controller platform.
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(&'static str),
}

impl Error {
    /// Returns `true` if this error indicates auth has expired
    /// and re-authentication might resolve it.
    pub fn is_auth_expired(&self) -> bool {
        matches!(self, Self::Authentication { .. } | Self::SessionExpired)
    }

    /// Returns `true` if this is a transient error worth retrying.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Transport(e) => e.is_timeout() || e.is_connect(),
            Self::Timeout { .. } => true,
            Self::RateLimited { .. } => true,
            Self::WebSocketConnect(_) => true,
            _ => false,
        }
    }

    /// Returns `true` if this is a "not found" error.
    pub fn is_not_found(&self) -> bool {
        match self {
            Self::Transport(e) => e.status() == Some(reqwest::StatusCode::NOT_FOUND),
            Self::Integration { status: 404, .. } => true,
            _ => false,
        }
    }

    /// Extract the API error code, if available.
    pub fn api_error_code(&self) -> Option<&str> {
        match self {
            Self::Integration { code, .. } => code.as_deref(),
            _ => None,
        }
    }
}
