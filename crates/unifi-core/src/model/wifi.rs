// ── WiFi broadcast domain types ──

use serde::{Deserialize, Serialize};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiBroadcastType {
    Standard,
    IotOptimized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WifiSecurityMode {
    Open,
    Wpa2Personal,
    Wpa3Personal,
    Wpa2Wpa3Personal,
    Wpa2Enterprise,
    Wpa3Enterprise,
    Wpa2Wpa3Enterprise,
}

/// The canonical WiFi Broadcast (SSID/WLAN) type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiBroadcast {
    pub id: EntityId,
    pub name: String,
    pub enabled: bool,
    pub broadcast_type: WifiBroadcastType,
    pub security: WifiSecurityMode,

    // Network association
    pub network_id: Option<EntityId>,

    // Frequencies
    pub frequencies_ghz: Vec<f32>,

    // Features
    pub hidden: bool,
    pub client_isolation: bool,
    pub band_steering: bool,
    pub mlo_enabled: bool,
    pub fast_roaming: bool,

    // Hotspot
    pub hotspot_enabled: bool,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}
