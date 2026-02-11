use thiserror::Error;

/// Top-level error type for the unifi-api crate.
#[derive(Debug, Error)]
pub enum Error {
    #[error("placeholder")]
    Todo,
}
