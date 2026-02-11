// ── Typed request structs for Command payloads ──
//
// Every Command variant that previously took `serde_json::Value`
// now uses one of these strongly-typed request structs instead.

use serde::{Deserialize, Serialize};

use crate::model::{DnsPolicyType, EntityId, FirewallAction, NetworkPurpose, WifiSecurityMode};

// ── Network ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<NetworkPurpose>,
    pub dhcp_enabled: bool,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_range_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_range_stop: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_lease_time: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_zone_id: Option<String>,
    pub isolation_enabled: bool,
    pub internet_access_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateNetworkRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dhcp_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

// ── WiFi ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWifiBroadcastRequest {
    pub name: String,
    pub ssid: String,
    pub security_mode: WifiSecurityMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<EntityId>,
    pub hide_ssid: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateWifiBroadcastRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_mode: Option<WifiSecurityMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_ssid: Option<bool>,
}

// ── Firewall Policy ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFirewallPolicyRequest {
    pub name: String,
    pub action: FirewallAction,
    pub source_zone_id: EntityId,
    pub destination_zone_id: EntityId,
    pub enabled: bool,
    pub logging_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_port: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateFirewallPolicyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<FirewallAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_port: Option<String>,
}

// ── Firewall Zone ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFirewallZoneRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub network_ids: Vec<EntityId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateFirewallZoneRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_ids: Option<Vec<EntityId>>,
}

// ── ACL Rule ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAclRuleRequest {
    pub name: String,
    pub action: FirewallAction,
    pub source_zone_id: EntityId,
    pub destination_zone_id: EntityId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_port: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateAclRuleRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<FirewallAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

// ── DNS Policy ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDnsPolicyRequest {
    pub name: String,
    pub policy_type: DnsPolicyType,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateDnsPolicyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

// ── Traffic Matching List ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrafficMatchingListRequest {
    pub name: String,
    pub entries: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTrafficMatchingListRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Vouchers ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVouchersRequest {
    pub count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_limit_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_usage_limit_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_rate_limit_kbps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_rate_limit_kbps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorized_guest_limit: Option<u32>,
}
