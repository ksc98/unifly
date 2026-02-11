use thiserror::Error;

/// Unified error type for the core crate.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("placeholder")]
    Todo,
}
