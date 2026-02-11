//! CLI error types with miette diagnostics.
//!
//! Maps `CoreError` variants into user-facing errors with actionable help text.

use miette::Diagnostic;
use thiserror::Error;

use unifi_core::CoreError;

/// Exit codes per the CLI spec.
pub mod exit_code {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const AUTH: i32 = 3;
    pub const NOT_FOUND: i32 = 4;
    pub const PERMISSION: i32 = 5;
    pub const CONFLICT: i32 = 6;
    pub const CONNECTION: i32 = 7;
    pub const TIMEOUT: i32 = 8;
}

#[derive(Debug, Error, Diagnostic)]
#[allow(dead_code, unused_assignments)]
pub enum CliError {
    // ── Connection ───────────────────────────────────────────────────

    #[error("Could not connect to controller at {url}")]
    #[diagnostic(
        code(unifi::connection_failed),
        help(
            "Check that the controller is running and accessible.\n\
             URL: {url}\n\
             Try: unifi system info --insecure"
        )
    )]
    ConnectionFailed {
        url: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("TLS certificate verification failed for {url}")]
    #[diagnostic(
        code(unifi::tls_error),
        help(
            "The controller is using a self-signed certificate.\n\
             Use --insecure (-k) to accept it, or configure ca_cert in your profile."
        )
    )]
    TlsError { url: String },

    // ── Authentication ───────────────────────────────────────────────

    #[error("Authentication failed")]
    #[diagnostic(
        code(unifi::auth_failed),
        help(
            "Verify your API key or credentials.\n\
             For Integration API: Check Settings > Integrations on your controller.\n\
             Run: unifi config set-password --profile {profile}"
        )
    )]
    AuthFailed { profile: String },

    #[error("No credentials configured for profile '{profile}'")]
    #[diagnostic(
        code(unifi::no_credentials),
        help(
            "Configure credentials with: unifi config init\n\
             Or set UNIFI_API_KEY environment variable."
        )
    )]
    NoCredentials { profile: String },

    // ── Resources ────────────────────────────────────────────────────

    #[error("{resource_type} '{identifier}' not found")]
    #[diagnostic(
        code(unifi::not_found),
        help("Run: unifi {list_command} to see available {resource_type}s")
    )]
    NotFound {
        resource_type: String,
        identifier: String,
        list_command: String,
    },

    #[error("{resource_type} '{identifier}' already exists")]
    #[diagnostic(code(unifi::conflict))]
    Conflict {
        resource_type: String,
        identifier: String,
    },

    // ── API ──────────────────────────────────────────────────────────

    #[error("API error ({code}): {message}")]
    #[diagnostic(code(unifi::api_error))]
    ApiError {
        code: String,
        message: String,
        request_id: Option<String>,
    },

    // ── Unsupported ──────────────────────────────────────────────────

    #[error("Operation '{operation}' is not supported with the current auth mode")]
    #[diagnostic(
        code(unifi::unsupported),
        help(
            "This command requires {required}.\n\
             Configure the appropriate credentials with: unifi config init"
        )
    )]
    Unsupported { operation: String, required: String },

    #[error("'{feature}' is not yet implemented")]
    #[diagnostic(
        code(unifi::not_implemented),
        help("This feature requires direct Legacy API access, planned for a future release.")
    )]
    NotYetImplemented { feature: String },

    // ── Validation ───────────────────────────────────────────────────

    #[error("Invalid value for {field}: {reason}")]
    #[diagnostic(code(unifi::validation))]
    Validation { field: String, reason: String },

    // ── Configuration ────────────────────────────────────────────────

    #[error("Profile '{name}' not found in configuration")]
    #[diagnostic(
        code(unifi::profile_not_found),
        help(
            "Available profiles: {available}\n\
             Create one with: unifi config init"
        )
    )]
    ProfileNotFound { name: String, available: String },

    #[error("Configuration file not found")]
    #[diagnostic(
        code(unifi::no_config),
        help(
            "Create one with: unifi config init\n\
             Expected at: {path}"
        )
    )]
    NoConfig { path: String },

    #[error(transparent)]
    #[diagnostic(code(unifi::config))]
    Config(Box<figment::Error>),

    // ── Interactive ──────────────────────────────────────────────────

    #[error("Destructive operation '{action}' requires confirmation")]
    #[diagnostic(
        code(unifi::confirmation_required),
        help("Use --yes (-y) to skip confirmation in non-interactive contexts.")
    )]
    NonInteractiveRequiresYes { action: String },

    // ── Timeout ──────────────────────────────────────────────────────

    #[error("Request timed out after {seconds}s")]
    #[diagnostic(
        code(unifi::timeout),
        help("Increase timeout with --timeout or check controller responsiveness.")
    )]
    Timeout { seconds: u64 },

    // ── IO / Serialization ────────────────────────────────────────────

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Invalid JSON payload: {0}")]
    #[diagnostic(code(unifi::json), help("Check the JSON file contents and try again."))]
    Json(#[from] serde_json::Error),
}

impl From<figment::Error> for CliError {
    fn from(err: figment::Error) -> Self {
        Self::Config(Box::new(err))
    }
}

impl CliError {
    /// Map this error to an exit code for process termination.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ConnectionFailed { .. } | Self::TlsError { .. } => exit_code::CONNECTION,
            Self::AuthFailed { .. } | Self::NoCredentials { .. } => exit_code::AUTH,
            Self::NotFound { .. } => exit_code::NOT_FOUND,
            Self::Conflict { .. } => exit_code::CONFLICT,
            Self::Timeout { .. } => exit_code::TIMEOUT,
            Self::Validation { .. } | Self::NonInteractiveRequiresYes { .. } => exit_code::USAGE,
            Self::Unsupported { .. } | Self::NotYetImplemented { .. } => exit_code::PERMISSION,
            _ => exit_code::GENERAL,
        }
    }
}

// ── CoreError → CliError mapping ─────────────────────────────────────

impl From<CoreError> for CliError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::ConnectionFailed { url, reason } => CliError::ConnectionFailed {
                url,
                source: reason.into(),
            },

            CoreError::AuthenticationFailed { message: _ } => CliError::AuthFailed {
                profile: "current".into(),
            },

            CoreError::ControllerDisconnected => CliError::ConnectionFailed {
                url: "(disconnected)".into(),
                source: "Controller connection was lost".into(),
            },

            CoreError::Timeout { timeout_secs } => CliError::Timeout {
                seconds: timeout_secs,
            },

            CoreError::DeviceNotFound { identifier } => CliError::NotFound {
                resource_type: "device".into(),
                identifier,
                list_command: "devices list".into(),
            },

            CoreError::ClientNotFound { identifier } => CliError::NotFound {
                resource_type: "client".into(),
                identifier,
                list_command: "clients list".into(),
            },

            CoreError::SiteNotFound { name } => CliError::NotFound {
                resource_type: "site".into(),
                identifier: name,
                list_command: "sites list".into(),
            },

            CoreError::NetworkNotFound { identifier } => CliError::NotFound {
                resource_type: "network".into(),
                identifier,
                list_command: "networks list".into(),
            },

            CoreError::NotFound {
                entity_type,
                identifier,
            } => CliError::NotFound {
                list_command: format!("{entity_type}s list"),
                resource_type: entity_type,
                identifier,
            },

            CoreError::Unsupported {
                operation,
                required,
            } => CliError::Unsupported {
                operation,
                required,
            },

            CoreError::ValidationFailed { message } => CliError::Validation {
                field: "input".into(),
                reason: message,
            },

            CoreError::Rejected { message } => CliError::ApiError {
                code: "rejected".into(),
                message,
                request_id: None,
            },

            CoreError::OperationFailed { message } => CliError::ApiError {
                code: "operation_failed".into(),
                message,
                request_id: None,
            },

            CoreError::Api {
                message,
                code,
                status: _,
            } => CliError::ApiError {
                code: code.unwrap_or_default(),
                message,
                request_id: None,
            },

            CoreError::Config { message } => {
                if message.contains("profile") {
                    CliError::ProfileNotFound {
                        name: message,
                        available: String::new(),
                    }
                } else {
                    CliError::NoConfig {
                        path: String::new(),
                    }
                }
            }

            CoreError::Internal(message) => CliError::ApiError {
                code: "internal".into(),
                message,
                request_id: None,
            },
        }
    }
}
