// ── Hotspot / voucher domain types ──

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::common::DataSource;
use super::entity_id::EntityId;

/// Voucher for guest hotspot access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voucher {
    pub id: EntityId,
    pub code: String,
    pub name: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub activated_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub expired: bool,
    pub time_limit_minutes: Option<u32>,
    pub data_usage_limit_mb: Option<u64>,
    pub authorized_guest_limit: Option<u32>,
    pub authorized_guest_count: Option<u32>,
    pub rx_rate_limit_kbps: Option<u64>,
    pub tx_rate_limit_kbps: Option<u64>,

    #[serde(skip)]
    pub(crate) source: DataSource,
}
