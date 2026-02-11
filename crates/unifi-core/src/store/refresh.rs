// ── Full refresh application logic ──
//
// Applies bulk data snapshots from the Integration and Legacy APIs
// into the DataStore. Integration data is primary; Legacy fills gaps.

use chrono::Utc;

use super::DataStore;
use crate::model::*;

impl DataStore {
    /// Apply a full Integration API data refresh.
    ///
    /// Clears all collections and repopulates from the provided data.
    /// Devices and clients are keyed by MAC address; other entities use
    /// synthetic `"prefix:{id}"` keys.
    pub(crate) fn apply_integration_snapshot(
        &self,
        devices: Vec<Device>,
        clients: Vec<Client>,
        networks: Vec<Network>,
        wifi: Vec<WifiBroadcast>,
        policies: Vec<FirewallPolicy>,
        zones: Vec<FirewallZone>,
        acls: Vec<AclRule>,
        dns: Vec<DnsPolicy>,
        vouchers: Vec<Voucher>,
    ) {
        self.devices.clear();
        self.clients.clear();
        self.networks.clear();
        self.wifi_broadcasts.clear();
        self.firewall_policies.clear();
        self.firewall_zones.clear();
        self.acl_rules.clear();
        self.dns_policies.clear();
        self.vouchers.clear();

        for device in devices {
            let key = device.mac.as_str().to_owned();
            let id = device.id.clone();
            self.devices.upsert(key, id, device);
        }

        for client in clients {
            let key = client.mac.as_str().to_owned();
            let id = client.id.clone();
            self.clients.upsert(key, id, client);
        }

        for network in networks {
            let key = format!("net:{}", network.id);
            let id = network.id.clone();
            self.networks.upsert(key, id, network);
        }

        for wb in wifi {
            let key = format!("wifi:{}", wb.id);
            let id = wb.id.clone();
            self.wifi_broadcasts.upsert(key, id, wb);
        }

        for policy in policies {
            let key = format!("fwp:{}", policy.id);
            let id = policy.id.clone();
            self.firewall_policies.upsert(key, id, policy);
        }

        for zone in zones {
            let key = format!("fwz:{}", zone.id);
            let id = zone.id.clone();
            self.firewall_zones.upsert(key, id, zone);
        }

        for acl in acls {
            let key = format!("acl:{}", acl.id);
            let id = acl.id.clone();
            self.acl_rules.upsert(key, id, acl);
        }

        for dns_policy in dns {
            let key = format!("dns:{}", dns_policy.id);
            let id = dns_policy.id.clone();
            self.dns_policies.upsert(key, id, dns_policy);
        }

        for voucher in vouchers {
            let key = format!("vch:{}", voucher.id);
            let id = voucher.id.clone();
            self.vouchers.upsert(key, id, voucher);
        }

        let _ = self.last_full_refresh.send(Some(Utc::now()));
    }

    /// Apply legacy-only data as a supplement to existing Integration data.
    ///
    /// For devices and clients, only inserts entities whose MAC is not
    /// already present (Integration data wins on conflict). Events are
    /// not stored in the DataStore — the caller should broadcast them
    /// through a separate event channel.
    pub(crate) fn apply_legacy_snapshot(
        &self,
        devices: Vec<Device>,
        clients: Vec<Client>,
        _events: Vec<Event>,
    ) {
        for device in devices {
            let key = device.mac.as_str().to_owned();
            if self.devices.get_by_key(&key).is_none() {
                let id = device.id.clone();
                self.devices.upsert(key, id, device);
            }
        }

        for client in clients {
            let key = client.mac.as_str().to_owned();
            if self.clients.get_by_key(&key).is_none() {
                let id = client.id.clone();
                self.clients.upsert(key, id, client);
            }
        }
    }
}
