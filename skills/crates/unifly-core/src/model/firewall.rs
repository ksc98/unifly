// ── Firewall domain types ──

use serde::{Deserialize, Serialize};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FirewallAction {
    Allow,
    Block,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpVersion {
    Ipv4,
    Ipv6,
    Both,
}

/// Firewall Zone -- container for networks, policies operate between zones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallZone {
    pub id: EntityId,
    pub name: String,
    pub network_ids: Vec<EntityId>,
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}

/// Firewall Policy -- a rule between two zones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallPolicy {
    pub id: EntityId,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub index: Option<i32>,

    pub action: FirewallAction,
    pub ip_version: IpVersion,

    pub source_zone_id: Option<EntityId>,
    pub destination_zone_id: Option<EntityId>,

    // Simplified traffic filter summary (the full filter tree is in unifly-api)
    pub source_summary: Option<String>,
    pub destination_summary: Option<String>,

    // Protocol and schedule display fields
    pub protocol_summary: Option<String>,
    pub schedule: Option<String>,
    pub ipsec_mode: Option<String>,

    pub connection_states: Vec<String>,
    pub logging_enabled: bool,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}

/// ACL Rule action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclAction {
    Allow,
    Block,
}

/// ACL Rule type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AclRuleType {
    Ipv4,
    Mac,
}

/// ACL Rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclRule {
    pub id: EntityId,
    pub name: String,
    pub enabled: bool,
    pub rule_type: AclRuleType,
    pub action: AclAction,
    pub source_summary: Option<String>,
    pub destination_summary: Option<String>,
    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) source: DataSource,
}
