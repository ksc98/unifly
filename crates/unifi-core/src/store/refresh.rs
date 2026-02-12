// ── Full refresh application logic ──
//
// Applies bulk data snapshots from the Integration and Legacy APIs
// into the DataStore. Integration data is primary; Legacy fills gaps.

use std::collections::HashSet;

use chrono::Utc;

use super::DataStore;
use super::collection::EntityCollection;
use crate::model::{EntityId, Device, Client, Network, WifiBroadcast, FirewallPolicy, FirewallZone, AclRule, DnsPolicy, Voucher, Site, Event, TrafficMatchingList};

/// Upsert all incoming entities, then prune any existing keys not in the
/// incoming set. This avoids the brief empty state that `clear()` causes.
fn upsert_and_prune<T: Clone + Send + Sync + 'static>(
    collection: &EntityCollection<T>,
    items: Vec<(String, EntityId, T)>,
) {
    let incoming_keys: HashSet<String> = items.iter().map(|(k, _, _)| k.clone()).collect();
    for (key, id, entity) in items {
        collection.upsert(key, id, entity);
    }
    for existing_key in collection.keys() {
        if !incoming_keys.contains(&existing_key) {
            collection.remove(&existing_key);
        }
    }
}

/// All collections fetched during a single Integration API refresh cycle.
///
/// Bundles the 12 entity vectors that `apply_integration_snapshot` needs,
/// keeping the function signature manageable.
pub(crate) struct RefreshSnapshot {
    pub devices: Vec<Device>,
    pub clients: Vec<Client>,
    pub networks: Vec<Network>,
    pub wifi: Vec<WifiBroadcast>,
    pub policies: Vec<FirewallPolicy>,
    pub zones: Vec<FirewallZone>,
    pub acls: Vec<AclRule>,
    pub dns: Vec<DnsPolicy>,
    pub vouchers: Vec<Voucher>,
    pub sites: Vec<Site>,
    pub events: Vec<Event>,
    pub traffic_matching_lists: Vec<TrafficMatchingList>,
}

impl DataStore {
    /// Apply a full Integration API data refresh.
    ///
    /// Uses upsert-then-prune: incoming entities are upserted first, then
    /// any keys not present in the incoming set are removed. This avoids the
    /// brief "empty" state that a clear-then-insert approach would cause.
    pub(crate) fn apply_integration_snapshot(&self, snap: RefreshSnapshot) {
        upsert_and_prune(
            &self.devices,
            snap.devices
                .into_iter()
                .map(|d| {
                    let key = d.mac.as_str().to_owned();
                    let id = d.id.clone();
                    (key, id, d)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.clients,
            snap.clients
                .into_iter()
                .map(|c| {
                    let key = c.mac.as_str().to_owned();
                    let id = c.id.clone();
                    (key, id, c)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.networks,
            snap.networks
                .into_iter()
                .map(|n| {
                    let key = format!("net:{}", n.id);
                    let id = n.id.clone();
                    (key, id, n)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.wifi_broadcasts,
            snap.wifi
                .into_iter()
                .map(|wb| {
                    let key = format!("wifi:{}", wb.id);
                    let id = wb.id.clone();
                    (key, id, wb)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.firewall_policies,
            snap.policies
                .into_iter()
                .map(|p| {
                    let key = format!("fwp:{}", p.id);
                    let id = p.id.clone();
                    (key, id, p)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.firewall_zones,
            snap.zones
                .into_iter()
                .map(|z| {
                    let key = format!("fwz:{}", z.id);
                    let id = z.id.clone();
                    (key, id, z)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.acl_rules,
            snap.acls
                .into_iter()
                .map(|a| {
                    let key = format!("acl:{}", a.id);
                    let id = a.id.clone();
                    (key, id, a)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.dns_policies,
            snap.dns
                .into_iter()
                .map(|d| {
                    let key = format!("dns:{}", d.id);
                    let id = d.id.clone();
                    (key, id, d)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.vouchers,
            snap.vouchers
                .into_iter()
                .map(|v| {
                    let key = format!("vch:{}", v.id);
                    let id = v.id.clone();
                    (key, id, v)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.sites,
            snap.sites
                .into_iter()
                .map(|s| {
                    let key = format!("site:{}", s.id);
                    let id = s.id.clone();
                    (key, id, s)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.events,
            snap.events
                .into_iter()
                .map(|e| {
                    let key =
                        e.id.as_ref().map_or_else(|| format!("evt:{}", e.timestamp.timestamp_millis()), std::string::ToString::to_string);
                    let id =
                        e.id.clone()
                            .unwrap_or_else(|| EntityId::Legacy(key.clone()));
                    (key, id, e)
                })
                .collect(),
        );

        upsert_and_prune(
            &self.traffic_matching_lists,
            snap.traffic_matching_lists
                .into_iter()
                .map(|t| {
                    let key = format!("tml:{}", t.id);
                    let id = t.id.clone();
                    (key, id, t)
                })
                .collect(),
        );

        let _ = self.last_full_refresh.send(Some(Utc::now()));
    }

    /// Apply legacy-only data as a supplement to existing Integration data.
    ///
    /// For devices and clients, only inserts entities whose MAC is not
    /// already present (Integration data wins on conflict). Events are
    /// not stored in the DataStore — the caller should broadcast them
    /// through a separate event channel.
    #[allow(dead_code)]
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
