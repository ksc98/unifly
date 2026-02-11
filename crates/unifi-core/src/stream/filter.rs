// ── Filter predicates for entity streams ──
//
// Used by the TUI to filter snapshots without re-querying the API.

use crate::model::*;

/// Filter predicate for device collections.
pub enum DeviceFilter {
    All,
    ByType(DeviceType),
    ByState(DeviceState),
    Online,
    Offline,
    Custom(Box<dyn Fn(&Device) -> bool + Send + Sync>),
}

impl DeviceFilter {
    pub fn matches(&self, device: &Device) -> bool {
        match self {
            Self::All => true,
            Self::ByType(dt) => device.device_type == *dt,
            Self::ByState(ds) => device.state == *ds,
            Self::Online => device.state.is_online(),
            Self::Offline => matches!(device.state, DeviceState::Offline),
            Self::Custom(f) => f(device),
        }
    }
}

/// Filter predicate for client collections.
pub enum ClientFilter {
    All,
    ByType(ClientType),
    ByNetwork(EntityId),
    ByDevice(MacAddress),
    Guests,
    Blocked,
    Custom(Box<dyn Fn(&Client) -> bool + Send + Sync>),
}

impl ClientFilter {
    pub fn matches(&self, client: &Client) -> bool {
        match self {
            Self::All => true,
            Self::ByType(ct) => client.client_type == *ct,
            Self::ByNetwork(nid) => client.network_id.as_ref() == Some(nid),
            Self::ByDevice(mac) => client.uplink_device_mac.as_ref() == Some(mac),
            Self::Guests => client.is_guest,
            Self::Blocked => client.blocked,
            Self::Custom(f) => f(client),
        }
    }
}
