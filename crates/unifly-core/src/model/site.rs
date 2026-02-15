// ── Site domain type ──

use serde::{Deserialize, Serialize};

use super::common::DataSource;
use super::entity_id::EntityId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub id: EntityId,
    /// The internal reference name (e.g., "default"). Used as the site
    /// identifier in Legacy API paths (`/api/s/{name}/...`).
    pub internal_name: String,
    /// Human-friendly display name.
    pub name: String,
    /// Number of devices adopted at this site (if known).
    pub device_count: Option<u32>,
    /// Number of connected clients (if known).
    pub client_count: Option<u32>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}
