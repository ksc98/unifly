// ── Central reactive data store ──
//
// Thread-safe, lock-free storage for all UniFi domain entities.
// Mutations are broadcast to subscribers via `watch` channels.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::watch;

use super::collection::EntityCollection;
use crate::model::{Device, Client, Network, WifiBroadcast, FirewallPolicy, FirewallZone, AclRule, DnsPolicy, Voucher, Site, Event, TrafficMatchingList, MacAddress, EntityId};
use crate::stream::EntityStream;

/// Central reactive store for all UniFi domain entities.
///
/// Thread-safe and lock-free: all reads are wait-free, writes use
/// fine-grained per-shard locks within `DashMap`. Mutations are
/// broadcast to subscribers via `watch` channels.
pub struct DataStore {
    pub(crate) devices: EntityCollection<Device>,
    pub(crate) clients: EntityCollection<Client>,
    pub(crate) networks: EntityCollection<Network>,
    pub(crate) wifi_broadcasts: EntityCollection<WifiBroadcast>,
    pub(crate) firewall_policies: EntityCollection<FirewallPolicy>,
    pub(crate) firewall_zones: EntityCollection<FirewallZone>,
    pub(crate) acl_rules: EntityCollection<AclRule>,
    pub(crate) dns_policies: EntityCollection<DnsPolicy>,
    pub(crate) vouchers: EntityCollection<Voucher>,
    pub(crate) sites: EntityCollection<Site>,
    pub(crate) events: EntityCollection<Event>,
    pub(crate) traffic_matching_lists: EntityCollection<TrafficMatchingList>,
    pub(crate) last_full_refresh: watch::Sender<Option<DateTime<Utc>>>,
    pub(crate) last_ws_event: watch::Sender<Option<DateTime<Utc>>>,
}

impl DataStore {
    pub fn new() -> Self {
        let (last_full_refresh, _) = watch::channel(None);
        let (last_ws_event, _) = watch::channel(None);

        Self {
            devices: EntityCollection::new(),
            clients: EntityCollection::new(),
            networks: EntityCollection::new(),
            wifi_broadcasts: EntityCollection::new(),
            firewall_policies: EntityCollection::new(),
            firewall_zones: EntityCollection::new(),
            acl_rules: EntityCollection::new(),
            dns_policies: EntityCollection::new(),
            vouchers: EntityCollection::new(),
            sites: EntityCollection::new(),
            events: EntityCollection::new(),
            traffic_matching_lists: EntityCollection::new(),
            last_full_refresh,
            last_ws_event,
        }
    }

    // ── Snapshot accessors ───────────────────────────────────────────

    pub fn devices_snapshot(&self) -> Arc<Vec<Arc<Device>>> {
        self.devices.snapshot()
    }

    pub fn clients_snapshot(&self) -> Arc<Vec<Arc<Client>>> {
        self.clients.snapshot()
    }

    pub fn networks_snapshot(&self) -> Arc<Vec<Arc<Network>>> {
        self.networks.snapshot()
    }

    pub fn wifi_broadcasts_snapshot(&self) -> Arc<Vec<Arc<WifiBroadcast>>> {
        self.wifi_broadcasts.snapshot()
    }

    pub fn firewall_policies_snapshot(&self) -> Arc<Vec<Arc<FirewallPolicy>>> {
        self.firewall_policies.snapshot()
    }

    pub fn firewall_zones_snapshot(&self) -> Arc<Vec<Arc<FirewallZone>>> {
        self.firewall_zones.snapshot()
    }

    pub fn acl_rules_snapshot(&self) -> Arc<Vec<Arc<AclRule>>> {
        self.acl_rules.snapshot()
    }

    pub fn dns_policies_snapshot(&self) -> Arc<Vec<Arc<DnsPolicy>>> {
        self.dns_policies.snapshot()
    }

    pub fn vouchers_snapshot(&self) -> Arc<Vec<Arc<Voucher>>> {
        self.vouchers.snapshot()
    }

    pub fn sites_snapshot(&self) -> Arc<Vec<Arc<Site>>> {
        self.sites.snapshot()
    }

    pub fn events_snapshot(&self) -> Arc<Vec<Arc<Event>>> {
        self.events.snapshot()
    }

    pub fn traffic_matching_lists_snapshot(&self) -> Arc<Vec<Arc<TrafficMatchingList>>> {
        self.traffic_matching_lists.snapshot()
    }

    // ── Single-entity lookups ────────────────────────────────────────

    pub fn device_by_mac(&self, mac: &MacAddress) -> Option<Arc<Device>> {
        self.devices.get_by_key(mac.as_str())
    }

    pub fn device_by_id(&self, id: &EntityId) -> Option<Arc<Device>> {
        self.devices.get_by_id(id)
    }

    pub fn client_by_mac(&self, mac: &MacAddress) -> Option<Arc<Client>> {
        self.clients.get_by_key(mac.as_str())
    }

    pub fn client_by_id(&self, id: &EntityId) -> Option<Arc<Client>> {
        self.clients.get_by_id(id)
    }

    pub fn network_by_id(&self, id: &EntityId) -> Option<Arc<Network>> {
        self.networks.get_by_id(id)
    }

    // ── Count accessors ──────────────────────────────────────────────

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn network_count(&self) -> usize {
        self.networks.len()
    }

    // ── Subscriptions ────────────────────────────────────────────────

    pub fn subscribe_devices(&self) -> EntityStream<Device> {
        EntityStream::new(self.devices.subscribe())
    }

    pub fn subscribe_clients(&self) -> EntityStream<Client> {
        EntityStream::new(self.clients.subscribe())
    }

    pub fn subscribe_networks(&self) -> EntityStream<Network> {
        EntityStream::new(self.networks.subscribe())
    }

    pub fn subscribe_wifi_broadcasts(&self) -> EntityStream<WifiBroadcast> {
        EntityStream::new(self.wifi_broadcasts.subscribe())
    }

    pub fn subscribe_firewall_policies(&self) -> EntityStream<FirewallPolicy> {
        EntityStream::new(self.firewall_policies.subscribe())
    }

    pub fn subscribe_firewall_zones(&self) -> EntityStream<FirewallZone> {
        EntityStream::new(self.firewall_zones.subscribe())
    }

    pub fn subscribe_acl_rules(&self) -> EntityStream<AclRule> {
        EntityStream::new(self.acl_rules.subscribe())
    }

    pub fn subscribe_dns_policies(&self) -> EntityStream<DnsPolicy> {
        EntityStream::new(self.dns_policies.subscribe())
    }

    pub fn subscribe_vouchers(&self) -> EntityStream<Voucher> {
        EntityStream::new(self.vouchers.subscribe())
    }

    pub fn subscribe_sites(&self) -> EntityStream<Site> {
        EntityStream::new(self.sites.subscribe())
    }

    pub fn subscribe_events(&self) -> EntityStream<Event> {
        EntityStream::new(self.events.subscribe())
    }

    pub fn subscribe_traffic_matching_lists(&self) -> EntityStream<TrafficMatchingList> {
        EntityStream::new(self.traffic_matching_lists.subscribe())
    }

    // ── Metadata ─────────────────────────────────────────────────────

    pub fn last_full_refresh(&self) -> Option<DateTime<Utc>> {
        *self.last_full_refresh.borrow()
    }

    pub fn last_ws_event(&self) -> Option<DateTime<Utc>> {
        *self.last_ws_event.borrow()
    }

    /// How long ago the last full refresh occurred, or `None` if never refreshed.
    pub fn data_age(&self) -> Option<chrono::Duration> {
        self.last_full_refresh().map(|t| Utc::now() - t)
    }
}

impl Default for DataStore {
    fn default() -> Self {
        Self::new()
    }
}
