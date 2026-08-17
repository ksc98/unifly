// ── Legacy-only model types ──
//
// These types support Legacy API resources that have no Integration API
// equivalent. Consumed by CLI stats, system, admin, and DPI commands.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use super::entity_id::{EntityId, MacAddress};

/// Statistical report (from Legacy API `stat/report/*`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatReport {
    pub interval: String,
    pub entries: Vec<StatEntry>,
}

/// A single stats entry within a report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatEntry {
    pub timestamp: DateTime<Utc>,
    pub wan_tx_bytes: Option<u64>,
    pub wan_rx_bytes: Option<u64>,
    pub num_sta: Option<u32>,
    pub lan_num_sta: Option<u32>,
    pub wlan_num_sta: Option<u32>,
    pub latency: Option<f64>,
    /// Catch-all for stat-specific fields.
    pub extra: serde_json::Value,
}

/// Controller system info (from `GET /v1/info` or Legacy `/api/s/{site}/stat/sysinfo`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub controller_name: Option<String>,
    pub version: String,
    pub build: Option<String>,
    pub hostname: Option<String>,
    pub ip: Option<IpAddr>,
    pub uptime_secs: Option<u64>,
    pub update_available: Option<bool>,
}

/// Health summary for a subsystem (from Legacy `stat/health`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSummary {
    /// Subsystem: "www", "wlan", "wan", "lan", "vpn"
    pub subsystem: String,
    /// Status: "ok", "warn", "error"
    pub status: String,
    pub num_adopted: Option<u32>,
    pub num_sta: Option<u32>,
    pub tx_bytes_r: Option<u64>,
    pub rx_bytes_r: Option<u64>,
    pub latency: Option<f64>,
    pub wan_ip: Option<String>,
    pub gateways: Option<Vec<String>>,
    pub extra: serde_json::Value,
}

/// Low-level controller system info (from Legacy `stat/sysinfo`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysInfo {
    pub timezone: Option<String>,
    pub autobackup: Option<bool>,
    pub hostname: Option<String>,
    pub ip_addrs: Vec<String>,
    pub live_chat: Option<String>,
    pub data_retention_days: Option<u32>,
    pub extra: serde_json::Value,
}

/// Backup entry (from Legacy `cmd/backup`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backup {
    pub filename: String,
    pub size_bytes: u64,
    pub created_at: Option<DateTime<Utc>>,
    pub version: Option<String>,
}

/// Admin user (from Legacy `cmd/sitemgr`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Admin {
    pub id: EntityId,
    pub name: String,
    pub email: Option<String>,
    /// Role: "admin", "readonly", "custom"
    pub role: String,
    pub is_super: bool,
    pub last_login: Option<DateTime<Utc>>,
}

/// Country entry (from Legacy `stat/ccode`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub code: String,
    pub name: String,
}

/// DPI application entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpiApplication {
    pub id: u32,
    pub name: String,
    pub category_id: u32,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

/// DPI category entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpiCategory {
    pub id: u32,
    pub name: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub apps: Vec<DpiApplication>,
}

/// Interval for historical stats queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatsInterval {
    FiveMinutes,
    Hourly,
    Daily,
}

/// A single site-level stats sample (from `stat/report/hourly.site` or `daily.site`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteStatsSample {
    pub timestamp: DateTime<Utc>,
    pub wan_tx_bytes: Option<u64>,
    pub wan_rx_bytes: Option<u64>,
    pub num_sta: Option<u32>,
    pub lan_num_sta: Option<u32>,
    pub wlan_num_sta: Option<u32>,
    pub latency: Option<f64>,
}

/// A single device-level stats sample (from `stat/report/hourly.ap` or `hourly.gw`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatsSample {
    pub timestamp: DateTime<Utc>,
    pub mac: MacAddress,
    pub tx_bytes: Option<u64>,
    pub rx_bytes: Option<u64>,
    pub num_sta: Option<u32>,
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
}
