// ── Device domain types ──

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use super::common::{Bandwidth, DataSource, EntityOrigin};
use super::entity_id::{EntityId, MacAddress};

/// Canonical device type -- normalized from both API surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeviceType {
    Gateway,
    Switch,
    AccessPoint,
    Other,
}

/// Device operational state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DeviceState {
    Online,
    Offline,
    PendingAdoption,
    Updating,
    GettingReady,
    Adopting,
    Deleting,
    ConnectionInterrupted,
    Isolated,
    Unknown,
}

impl DeviceState {
    pub fn is_online(&self) -> bool {
        matches!(self, Self::Online)
    }

    pub fn is_transitional(&self) -> bool {
        matches!(
            self,
            Self::Updating | Self::GettingReady | Self::Adopting | Self::PendingAdoption
        )
    }
}

/// Port on a switch or gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub index: u32,
    pub name: Option<String>,
    pub state: PortState,
    pub speed_mbps: Option<u32>,
    pub max_speed_mbps: Option<u32>,
    pub connector: Option<PortConnector>,
    pub poe: Option<PoeInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortState {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortConnector {
    Rj45,
    Sfp,
    SfpPlus,
    Sfp28,
    Qsfp28,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoeInfo {
    pub standard: Option<String>,
    pub enabled: bool,
    pub state: PortState,
}

/// Radio on an access point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Radio {
    pub frequency_ghz: f32,
    pub channel: Option<u32>,
    pub channel_width_mhz: Option<u32>,
    pub wlan_standard: Option<String>,
    pub tx_retries_pct: Option<f64>,
}

/// Real-time device statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceStats {
    pub uptime_secs: Option<u64>,
    pub cpu_utilization_pct: Option<f64>,
    pub memory_utilization_pct: Option<f64>,
    pub load_average_1m: Option<f64>,
    pub load_average_5m: Option<f64>,
    pub load_average_15m: Option<f64>,
    pub uplink_bandwidth: Option<Bandwidth>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub next_heartbeat: Option<DateTime<Utc>>,
}

/// The canonical Device type. Merges data from Integration + Legacy APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Device {
    pub id: EntityId,
    pub mac: MacAddress,
    pub ip: Option<IpAddr>,
    pub wan_ipv6: Option<String>,
    pub name: Option<String>,
    pub model: Option<String>,
    pub device_type: DeviceType,
    pub state: DeviceState,

    // Firmware
    pub firmware_version: Option<String>,
    pub firmware_updatable: bool,

    // Lifecycle
    pub adopted_at: Option<DateTime<Utc>>,
    pub provisioned_at: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,

    // Hardware
    pub serial: Option<String>,
    pub supported: bool,

    // Interfaces
    pub ports: Vec<Port>,
    pub radios: Vec<Radio>,

    // Uplink
    pub uplink_device_id: Option<EntityId>,
    pub uplink_device_mac: Option<MacAddress>,

    // Features (from Integration API)
    pub has_switching: bool,
    pub has_access_point: bool,

    // Real-time stats (populated from statistics endpoint or WebSocket)
    pub stats: DeviceStats,

    // Client count (if known)
    pub client_count: Option<u32>,

    // Metadata
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) updated_at: DateTime<Utc>,
}
