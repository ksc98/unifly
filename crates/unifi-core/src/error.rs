// ── Core error types ──
//
// User-facing errors from unifi-core. These are NOT API-specific --
// consumers never see HTTP status codes or JSON parse failures directly.
// The `From<unifi_api::Error>` impl translates transport-layer errors
// into domain-appropriate variants.

use thiserror::Error;

/// Unified error type for the core crate.
#[derive(Debug, Error)]
pub enum CoreError {
    // ── Connection errors ────────────────────────────────────────────
    #[error("Cannot connect to controller at {url}: {reason}")]
    ConnectionFailed { url: String, reason: String },

    #[error("Authentication failed: {message}")]
    AuthenticationFailed { message: String },

    #[error("Controller disconnected")]
    ControllerDisconnected,

    #[error("Controller connection timed out after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    // ── Data errors ──────────────────────────────────────────────────
    #[error("Device not found: {identifier}")]
    DeviceNotFound { identifier: String },

    #[error("Client not found: {identifier}")]
    ClientNotFound { identifier: String },

    #[error("Network not found: {identifier}")]
    NetworkNotFound { identifier: String },

    #[error("Site not found: {name}")]
    SiteNotFound { name: String },

    #[error("Entity not found: {entity_type} with id {identifier}")]
    NotFound {
        entity_type: String,
        identifier: String,
    },

    // ── Operation errors ─────────────────────────────────────────────
    #[error("Operation not supported: {operation} (requires {required})")]
    Unsupported { operation: String, required: String },

    #[error("Operation rejected by controller: {message}")]
    Rejected { message: String },

    #[error("Validation failed: {message}")]
    ValidationFailed { message: String },

    #[error("Operation failed: {message}")]
    OperationFailed { message: String },

    // ── API errors (wrapped, not exposed raw) ────────────────────────
    #[error("API error: {message}")]
    Api {
        message: String,
        /// The API-specific error code (e.g., "api.authentication.missing-credentials").
        code: Option<String>,
        /// HTTP status code (if applicable).
        status: Option<u16>,
    },

    // ── Configuration errors ─────────────────────────────────────────
    #[error("Configuration error: {message}")]
    Config { message: String },

    // ── Internal errors ──────────────────────────────────────────────
    #[error("Internal error: {0}")]
    Internal(String),
}

// ── Conversion from transport-layer errors ───────────────────────────

impl From<unifi_api::Error> for CoreError {
    fn from(err: unifi_api::Error) -> Self {
        match err {
            unifi_api::Error::Authentication { message } => {
                CoreError::AuthenticationFailed { message }
            }
            unifi_api::Error::TwoFactorRequired => CoreError::AuthenticationFailed {
                message: "Two-factor authentication token required".into(),
            },
            unifi_api::Error::SessionExpired => CoreError::AuthenticationFailed {
                message: "Session expired -- re-authentication required".into(),
            },
            unifi_api::Error::InvalidApiKey => CoreError::AuthenticationFailed {
                message: "Invalid API key".into(),
            },
            unifi_api::Error::WrongAuthStrategy { expected, got } => {
                CoreError::AuthenticationFailed {
                    message: format!("Wrong auth strategy: expected {expected}, got {got}"),
                }
            }
            unifi_api::Error::Transport(ref e) => {
                if e.is_timeout() {
                    CoreError::Timeout { timeout_secs: 0 }
                } else if e.is_connect() {
                    CoreError::ConnectionFailed {
                        url: e
                            .url()
                            .map(|u| u.to_string())
                            .unwrap_or_else(|| "<unknown>".into()),
                        reason: e.to_string(),
                    }
                } else if e.status().map(|s| s.as_u16()) == Some(404) {
                    CoreError::NotFound {
                        entity_type: "resource".into(),
                        identifier: e.url().map(|u| u.path().to_string()).unwrap_or_default(),
                    }
                } else {
                    CoreError::Api {
                        message: e.to_string(),
                        code: None,
                        status: e.status().map(|s| s.as_u16()),
                    }
                }
            }
            unifi_api::Error::InvalidUrl(e) => CoreError::Config {
                message: format!("Invalid URL: {e}"),
            },
            unifi_api::Error::Timeout { timeout_secs } => CoreError::Timeout { timeout_secs },
            unifi_api::Error::Tls(msg) => CoreError::ConnectionFailed {
                url: String::new(),
                reason: format!("TLS error: {msg}"),
            },
            unifi_api::Error::RateLimited { retry_after_secs } => CoreError::Api {
                message: format!("Rate limited -- retry after {retry_after_secs}s"),
                code: Some("rate_limited".into()),
                status: Some(429),
            },
            unifi_api::Error::Integration {
                message,
                code,
                status,
            } => CoreError::Api {
                message,
                code,
                status: Some(status),
            },
            unifi_api::Error::LegacyApi { message } => CoreError::Api {
                message,
                code: None,
                status: None,
            },
            unifi_api::Error::WebSocketConnect(reason) => CoreError::ConnectionFailed {
                url: String::new(),
                reason: format!("WebSocket connection failed: {reason}"),
            },
            unifi_api::Error::WebSocketClosed { code, reason } => CoreError::ConnectionFailed {
                url: String::new(),
                reason: format!("WebSocket closed (code {code}): {reason}"),
            },
            unifi_api::Error::Deserialization { message, body: _ } => {
                CoreError::Internal(format!("Deserialization error: {message}"))
            }
            unifi_api::Error::UnsupportedOperation(op) => CoreError::Unsupported {
                operation: op.to_string(),
                required: "a newer controller firmware".into(),
            },
        }
    }
}
