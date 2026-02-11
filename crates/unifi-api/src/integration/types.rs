//! Integration API response types for the UniFi Network Integration API (v10.1.84).
//!
//! All types match the JSON responses from `/integration/v1/` endpoints.
//! Field names use camelCase via `#[serde(rename_all = "camelCase")]`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Pagination ───────────────────────────────────────────────────────

/// Generic pagination wrapper returned by all list endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub offset: i64,
    pub limit: i32,
    pub count: i32,
    pub total_count: i64,
    pub data: Vec<T>,
}

// ── Sites ────────────────────────────────────────────────────────────

/// Site overview — from `GET /v1/sites`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteResponse {
    pub id: Uuid,
    pub name: String,
    /// Used as the Legacy API site name (`/api/s/{internalReference}/`).
    pub internal_reference: String,
}

// ── Devices ──────────────────────────────────────────────────────────

/// Adopted device overview — from `GET /v1/sites/{siteId}/devices`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceResponse {
    pub id: Uuid,
    pub mac_address: String,
    pub ip_address: Option<String>,
    pub name: String,
    pub model: String,
    /// One of: `ONLINE`, `OFFLINE`, `PENDING_ADOPTION`, `UPDATING`,
    /// `GETTING_READY`, `ADOPTING`, `DELETING`, `CONNECTION_INTERRUPTED`, `ISOLATED`.
    pub state: String,
    pub supported: bool,
    pub firmware_version: Option<String>,
    pub firmware_updatable: bool,
    pub features: Vec<String>,
    /// Complex nested interfaces object — kept as opaque JSON.
    pub interfaces: Value,
}

/// Adopted device details — extends overview with additional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDetailsResponse {
    pub id: Uuid,
    pub mac_address: String,
    pub ip_address: Option<String>,
    pub name: String,
    pub model: String,
    pub state: String,
    pub supported: bool,
    pub firmware_version: Option<String>,
    pub firmware_updatable: bool,
    pub features: Vec<String>,
    pub interfaces: Value,
    pub serial_number: Option<String>,
    pub short_name: Option<String>,
    /// ISO 8601 date-time.
    pub startup_timestamp: Option<String>,
    /// Catch-all for additional fields not modeled above.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Latest statistics for a device — from `GET /v1/sites/{siteId}/devices/{deviceId}/statistics/latest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatisticsResponse {
    pub uptime_sec: Option<i64>,
    pub cpu_utilization_pct: Option<f64>,
    pub memory_utilization_pct: Option<f64>,
    pub load_average_1_min: Option<f64>,
    pub load_average_5_min: Option<f64>,
    pub load_average_15_min: Option<f64>,
    /// ISO 8601 date-time.
    pub last_heartbeat_at: Option<String>,
    /// ISO 8601 date-time.
    pub next_heartbeat_at: Option<String>,
    /// Complex nested interfaces statistics.
    pub interfaces: Value,
    /// Uplink information.
    pub uplink: Option<Value>,
}

// ── Clients ──────────────────────────────────────────────────────────

/// Client overview — from `GET /v1/sites/{siteId}/clients`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientResponse {
    pub id: Uuid,
    pub name: String,
    /// One of: `WIRED`, `WIRELESS`, `VPN`, `TELEPORT`.
    #[serde(rename = "type")]
    pub client_type: String,
    pub ip_address: Option<String>,
    /// ISO 8601 date-time.
    pub connected_at: Option<String>,
    /// Polymorphic access object — contains a `type` discriminator field.
    pub access: Value,
}

/// Client details — extends overview with additional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDetailsResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub client_type: String,
    pub ip_address: Option<String>,
    pub connected_at: Option<String>,
    pub access: Value,
    /// Catch-all for additional fields not modeled above.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ── Networks ─────────────────────────────────────────────────────────

/// Network overview — from `GET /v1/sites/{siteId}/networks`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResponse {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    /// One of: `USER_DEFINED`, `SYSTEM_DEFINED`, `ORCHESTRATED`.
    pub management: String,
    pub vlan_id: i32,
    #[serde(default)]
    pub default: bool,
    pub metadata: Value,
}

/// Network details — extends overview with additional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkDetailsResponse {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub management: String,
    pub vlan_id: i32,
    #[serde(default)]
    pub default: bool,
    pub metadata: Value,
    pub dhcp_guarding: Option<Value>,
}

/// Create or update a network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCreateUpdate {
    pub name: String,
    pub enabled: bool,
    pub management: String,
    pub vlan_id: i32,
    pub dhcp_guarding: Option<Value>,
}

/// References to resources using a network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkReferencesResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

// ── WiFi Broadcasts ──────────────────────────────────────────────────

/// WiFi broadcast overview — from `GET /v1/sites/{siteId}/wifi`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiBroadcastResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub broadcast_type: String,
    pub enabled: bool,
    pub security_configuration: Value,
    pub metadata: Value,
    pub network: Option<Value>,
    pub broadcasting_device_filter: Option<Value>,
}

/// WiFi broadcast details — extends overview with additional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiBroadcastDetailsResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub broadcast_type: String,
    pub enabled: bool,
    pub security_configuration: Value,
    pub metadata: Value,
    pub network: Option<Value>,
    pub broadcasting_device_filter: Option<Value>,
    /// Catch-all for additional fields not modeled above.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Create or update a WiFi broadcast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiBroadcastCreateUpdate {
    pub name: String,
    #[serde(rename = "type")]
    pub broadcast_type: String,
    pub enabled: bool,
    /// All remaining type-specific fields.
    #[serde(flatten)]
    pub body: serde_json::Map<String, Value>,
}

// ── Firewall Policies ────────────────────────────────────────────────

/// Firewall policy — from `GET /v1/sites/{siteId}/firewall/policies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
    /// Polymorphic action object with `type` discriminator.
    pub action: Value,
    pub ip_protocol_scope: Option<Value>,
    #[serde(default)]
    pub logging_enabled: bool,
    pub metadata: Option<Value>,
    /// Catch-all for additional / variable fields (index, source, destination,
    /// sourceFirewallZoneId, destinationFirewallZoneId, schedule, etc.)
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Create or update a firewall policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyCreateUpdate {
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub action: Value,
    pub source: Value,
    pub destination: Value,
    pub ip_protocol_scope: Value,
    pub logging_enabled: bool,
    pub ipsec_filter: Option<String>,
    pub schedule: Option<Value>,
    pub connection_state_filter: Option<Vec<String>>,
}

/// Patch a firewall policy (partial update).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging_enabled: Option<bool>,
}

/// Ordered firewall policy IDs — for reordering policies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallPolicyOrdering {
    pub before_system_defined: Vec<Uuid>,
    pub after_system_defined: Vec<Uuid>,
}

// ── Firewall Zones ───────────────────────────────────────────────────

/// Firewall zone — from `GET /v1/sites/{siteId}/firewall/zones`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallZoneResponse {
    pub id: Uuid,
    pub name: String,
    pub network_ids: Vec<Uuid>,
    pub metadata: Value,
}

/// Create or update a firewall zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallZoneCreateUpdate {
    pub name: String,
    pub network_ids: Vec<Uuid>,
}

// ── ACL Rules ────────────────────────────────────────────────────────

/// ACL rule — from `GET /v1/sites/{siteId}/acl/rules`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleResponse {
    pub id: Uuid,
    pub name: String,
    /// One of: `IP`, `MAC`.
    #[serde(rename = "type")]
    pub rule_type: String,
    /// One of: `ALLOW`, `BLOCK`.
    pub action: String,
    pub enabled: bool,
    pub index: i32,
    pub description: Option<String>,
    pub source_filter: Option<Value>,
    pub destination_filter: Option<Value>,
    pub enforcing_device_filter: Option<Value>,
    pub metadata: Value,
}

/// Create or update an ACL rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleCreateUpdate {
    pub name: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub action: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub source_filter: Option<Value>,
    pub destination_filter: Option<Value>,
    pub enforcing_device_filter: Option<Value>,
}

/// ACL rule ordering — for reordering rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleOrdering {
    pub ordered_acl_rule_ids: Vec<Uuid>,
}

// ── DNS Policies ─────────────────────────────────────────────────────

/// DNS policy — from `GET /v1/sites/{siteId}/dns/policies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsPolicyResponse {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub policy_type: String,
    pub enabled: bool,
    pub domain: Option<String>,
    pub metadata: Value,
    /// Type-specific fields vary by policy type.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Create or update a DNS policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsPolicyCreateUpdate {
    #[serde(rename = "type")]
    pub policy_type: String,
    pub enabled: bool,
    /// Type-specific fields vary by policy type.
    #[serde(flatten)]
    pub fields: serde_json::Map<String, Value>,
}

// ── Traffic Matching Lists ───────────────────────────────────────────

/// Traffic matching list — from `GET /v1/sites/{siteId}/traffic-matching-lists`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficMatchingListResponse {
    pub id: Uuid,
    pub name: String,
    /// One of: `IPV4`, `IPV6`, `PORT`.
    #[serde(rename = "type")]
    pub list_type: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Create or update a traffic matching list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficMatchingListCreateUpdate {
    pub name: String,
    #[serde(rename = "type")]
    pub list_type: String,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, Value>,
}

// ── Hotspot Vouchers ─────────────────────────────────────────────────

/// Hotspot voucher — from `GET /v1/sites/{siteId}/hotspot/vouchers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    /// ISO 8601 date-time.
    pub created_at: String,
    /// ISO 8601 date-time.
    pub activated_at: Option<String>,
    /// ISO 8601 date-time.
    pub expires_at: Option<String>,
    pub expired: bool,
    pub time_limit_minutes: i64,
    pub authorized_guest_count: i64,
    pub authorized_guest_limit: Option<i64>,
    pub data_usage_limit_m_bytes: Option<i64>,
    pub rx_rate_limit_kbps: Option<i64>,
    pub tx_rate_limit_kbps: Option<i64>,
}

/// Create hotspot voucher(s).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherCreateRequest {
    pub name: String,
    /// Number of vouchers to create (defaults to 1).
    pub count: Option<i32>,
    pub time_limit_minutes: i64,
    pub authorized_guest_limit: Option<i64>,
    pub data_usage_limit_m_bytes: Option<i64>,
    pub rx_rate_limit_kbps: Option<i64>,
    pub tx_rate_limit_kbps: Option<i64>,
}

/// Bulk voucher deletion results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherDeletionResults {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

// ── Device Actions ───────────────────────────────────────────────────

/// Device action request body.
///
/// Valid actions: `RESTART`, `ADOPT`, `LOCATE_ON`, `LOCATE_OFF`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceActionRequest {
    pub action: String,
}

/// Device adoption request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAdoptionRequest {
    pub mac_address: String,
    pub ignore_device_limit: bool,
}

// ── Client Actions ───────────────────────────────────────────────────

/// Client action request body.
///
/// Valid actions: `BLOCK`, `UNBLOCK`, `RECONNECT`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientActionRequest {
    pub action: String,
}

/// Client action response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientActionResponse {
    pub action: String,
    pub id: Uuid,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ── Port Actions ─────────────────────────────────────────────────────

/// Port action request body.
///
/// Valid actions: `POWER_CYCLE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortActionRequest {
    pub action: String,
}

// ── Application Info ─────────────────────────────────────────────────

/// Application info — from `GET /v1/info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInfoResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

// ── Error ────────────────────────────────────────────────────────────

/// Error response returned by the Integration API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ── Read-only / Opaque Types ─────────────────────────────────────────
//
// These endpoints return complex or under-documented structures.
// We capture them as open-ended maps until we need typed access.

/// DPI category — from `GET /v1/sites/{siteId}/dpi/categories`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DpiCategoryResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// DPI application — from `GET /v1/sites/{siteId}/dpi/applications`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DpiApplicationResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// VPN server — from `GET /v1/sites/{siteId}/vpn/servers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnServerResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// VPN tunnel — from `GET /v1/sites/{siteId}/vpn/tunnels`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnTunnelResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// WAN configuration — from `GET /v1/sites/{siteId}/wan`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WanResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// RADIUS profile — from `GET /v1/sites/{siteId}/radius/profiles`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadiusProfileResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// Country metadata — from `GET /v1/countries`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountryResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// Device tag — from `GET /v1/sites/{siteId}/devices/tags`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTagResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

/// Pending device — from `GET /v1/sites/{siteId}/devices/pending`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDeviceResponse {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}
