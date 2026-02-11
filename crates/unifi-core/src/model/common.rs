// ── Common types shared across the domain model ──

use serde::{Deserialize, Serialize};

/// Origin metadata -- where this entity came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityOrigin {
    UserDefined,
    SystemDefined,
    Derived,
    Orchestrated,
}

/// Which API provided this data (internal bookkeeping, not exposed in display).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    IntegrationApi,
    LegacyApi,
    WebSocket,
    Merged,
}

impl Default for DataSource {
    fn default() -> Self {
        Self::IntegrationApi
    }
}

/// Bandwidth measurement.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Bandwidth {
    pub tx_bytes_per_sec: u64,
    pub rx_bytes_per_sec: u64,
}

/// Throughput over a time period.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Throughput {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub period: std::time::Duration,
}
