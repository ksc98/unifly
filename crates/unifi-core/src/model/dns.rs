// ── DNS domain types ──

use serde::{Deserialize, Serialize};

use super::common::{DataSource, EntityOrigin};
use super::entity_id::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DnsPolicyType {
    ARecord,
    AaaaRecord,
    CnameRecord,
    MxRecord,
    TxtRecord,
    SrvRecord,
    ForwardDomain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsPolicy {
    pub id: EntityId,
    pub policy_type: DnsPolicyType,
    pub domain: String,
    pub value: String,
    pub ttl_seconds: Option<u32>,

    pub origin: Option<EntityOrigin>,

    #[serde(skip)]
    pub(crate) source: DataSource,
}
