// ── Supporting / auxiliary domain types ──
//
// VPN, WAN, traffic matching, RADIUS, device tags, and other
// resources that don't warrant their own module.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use super::common::EntityOrigin;
use super::entity_id::EntityId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnServer {
    pub id: EntityId,
    pub name: Option<String>,
    /// Server type: OPENVPN, WIREGUARD, L2TP, PPTP, UID
    pub server_type: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnTunnel {
    pub id: EntityId,
    pub name: Option<String>,
    /// Tunnel type: OPENVPN, IPSEC, WIREGUARD
    pub tunnel_type: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WanInterface {
    pub id: EntityId,
    pub name: Option<String>,
    pub ip: Option<IpAddr>,
    pub gateway: Option<IpAddr>,
    pub dns: Vec<IpAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficMatchingList {
    pub id: EntityId,
    pub name: String,
    /// List type: PORTS, IPV4_ADDRESSES, IPV6_ADDRESSES
    pub list_type: String,
    pub items: Vec<String>,
    pub origin: Option<EntityOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadiusProfile {
    pub id: EntityId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTag {
    pub id: EntityId,
    pub name: String,
}
