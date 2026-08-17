// Legacy API response types
//
// Models for the UniFi controller's legacy JSON API. All responses are wrapped
// in the `LegacyResponse<T>` envelope. Fields use `#[serde(default)]` liberally
// because the API is inconsistent about field presence across firmware versions.

use serde::{Deserialize, Serialize};

// ── Response Envelope ────────────────────────────────────────────────

/// Standard UniFi legacy API response envelope.
///
/// Every legacy endpoint wraps its payload:
/// ```json
/// { "meta": { "rc": "ok", "msg": "optional" }, "data": [...] }
/// ```
#[derive(Debug, Deserialize)]
pub struct LegacyResponse<T> {
    pub meta: Meta,
    pub data: Vec<T>,
}

/// Metadata from the legacy envelope. `rc` == `"ok"` means success.
#[derive(Debug, Deserialize)]
pub struct Meta {
    pub rc: String,
    #[serde(default)]
    pub msg: Option<String>,
}

// ── Device ───────────────────────────────────────────────────────────

/// Full device object from `stat/device`.
///
/// The legacy API can return 100+ fields per device. We model the most
/// commonly needed ones explicitly; everything else lands in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyDevice {
    #[serde(rename = "_id")]
    pub id: String,
    pub mac: String,
    #[serde(rename = "type")]
    pub device_type: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub adopted: bool,
    /// 0=offline, 1=online, 2=pending, 4=upgrading, 5=provisioning
    #[serde(default)]
    pub state: i32,
    #[serde(default)]
    pub sys_stats: Option<SysStats>,
    #[serde(default)]
    pub uptime: Option<i64>,
    #[serde(default)]
    pub num_sta: Option<i32>,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub site_id: Option<String>,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub upgradable: Option<bool>,
    #[serde(default, rename = "user-num_sta")]
    pub user_num_sta: Option<i32>,
    #[serde(default, rename = "guest-num_sta")]
    pub guest_num_sta: Option<i32>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// System statistics nested inside `LegacyDevice`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysStats {
    #[serde(default, rename = "loadavg_1")]
    pub load_1: Option<String>,
    #[serde(default, rename = "loadavg_5")]
    pub load_5: Option<String>,
    #[serde(default, rename = "loadavg_15")]
    pub load_15: Option<String>,
    #[serde(default)]
    pub mem_total: Option<i64>,
    #[serde(default)]
    pub mem_used: Option<i64>,
    #[serde(default)]
    pub cpu: Option<String>,
}

// ── Client (Station) ─────────────────────────────────────────────────

/// Connected client from `stat/sta`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyClientEntry {
    #[serde(rename = "_id")]
    pub id: String,
    pub mac: String,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub oui: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_guest: Option<bool>,
    #[serde(default)]
    pub is_wired: Option<bool>,
    #[serde(default)]
    pub authorized: Option<bool>,
    #[serde(default)]
    pub blocked: Option<bool>,
    #[serde(default)]
    pub signal: Option<i32>,
    #[serde(default)]
    pub tx_bytes: Option<i64>,
    #[serde(default)]
    pub rx_bytes: Option<i64>,
    #[serde(default)]
    pub tx_rate: Option<i64>,
    #[serde(default)]
    pub rx_rate: Option<i64>,
    #[serde(default)]
    pub uptime: Option<i64>,
    #[serde(default)]
    pub first_seen: Option<i64>,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub site_id: Option<String>,
    #[serde(default)]
    pub essid: Option<String>,
    #[serde(default)]
    pub bssid: Option<String>,
    #[serde(default)]
    pub channel: Option<i32>,
    #[serde(default)]
    pub radio: Option<String>,
    #[serde(default)]
    pub rssi: Option<i32>,
    #[serde(default)]
    pub noise: Option<i32>,
    #[serde(default)]
    pub satisfaction: Option<i32>,
    #[serde(default)]
    pub ap_mac: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub network_id: Option<String>,
    #[serde(default)]
    pub sw_mac: Option<String>,
    #[serde(default)]
    pub sw_port: Option<i32>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Site ─────────────────────────────────────────────────────────────

/// Site object from `/api/self/sites`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacySite {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Event ────────────────────────────────────────────────────────────

/// Event object from `stat/event`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyEvent {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
    #[serde(default)]
    pub subsystem: Option<String>,
    #[serde(default)]
    pub site_id: Option<String>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Alarm ────────────────────────────────────────────────────────────

/// Alarm object from `stat/alarm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyAlarm {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    /// Catch-all for undocumented fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
