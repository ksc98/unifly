// ── Command API ──
//
// All write operations flow through a unified `Command` enum.
// The controller routes each variant to the appropriate API backend
// (Integration API preferred, Legacy API for legacy-only operations).

pub mod requests;

use crate::error::CoreError;
use crate::model::{
    AclRule, Client, Device, DnsPolicy, EntityId, FirewallPolicy, FirewallZone,
    MacAddress, Network, TrafficMatchingList, Voucher, WifiBroadcast,
};

pub use requests::{
    CreateAclRuleRequest, CreateDnsPolicyRequest, CreateFirewallPolicyRequest,
    CreateFirewallZoneRequest, CreateNetworkRequest, CreateTrafficMatchingListRequest,
    CreateVouchersRequest, CreateWifiBroadcastRequest, UpdateAclRuleRequest,
    UpdateDnsPolicyRequest, UpdateFirewallPolicyRequest, UpdateFirewallZoneRequest,
    UpdateNetworkRequest, UpdateTrafficMatchingListRequest, UpdateWifiBroadcastRequest,
};

/// A command envelope sent through the command channel.
/// Contains the command and a oneshot response channel.
pub(crate) struct CommandEnvelope {
    pub command: Command,
    pub response_tx: tokio::sync::oneshot::Sender<Result<CommandResult, CoreError>>,
}

/// All possible write operations against a UniFi controller.
#[derive(Debug, Clone)]
pub enum Command {
    // ── Device operations ────────────────────────────────────────────
    AdoptDevice {
        mac: MacAddress,
    },
    RemoveDevice {
        id: EntityId,
    },
    RestartDevice {
        id: EntityId,
    },
    LocateDevice {
        mac: MacAddress,
        enable: bool,
    },
    UpgradeDevice {
        mac: MacAddress,
        firmware_url: Option<String>,
    },
    ProvisionDevice {
        mac: MacAddress,
    },
    SpeedtestDevice,
    PowerCyclePort {
        device_id: EntityId,
        port_idx: u32,
    },

    // ── Client operations ────────────────────────────────────────────
    BlockClient {
        mac: MacAddress,
    },
    UnblockClient {
        mac: MacAddress,
    },
    KickClient {
        mac: MacAddress,
    },
    ForgetClient {
        mac: MacAddress,
    },
    AuthorizeGuest {
        client_id: EntityId,
        time_limit_minutes: Option<u32>,
        data_limit_mb: Option<u64>,
        rx_rate_kbps: Option<u64>,
        tx_rate_kbps: Option<u64>,
    },
    UnauthorizeGuest {
        client_id: EntityId,
    },

    // ── Network CRUD ─────────────────────────────────────────────────
    CreateNetwork(CreateNetworkRequest),
    UpdateNetwork {
        id: EntityId,
        update: UpdateNetworkRequest,
    },
    DeleteNetwork {
        id: EntityId,
        force: bool,
    },

    // ── WiFi CRUD ────────────────────────────────────────────────────
    CreateWifiBroadcast(CreateWifiBroadcastRequest),
    UpdateWifiBroadcast {
        id: EntityId,
        update: UpdateWifiBroadcastRequest,
    },
    DeleteWifiBroadcast {
        id: EntityId,
        force: bool,
    },

    // ── Firewall ─────────────────────────────────────────────────────
    CreateFirewallPolicy(CreateFirewallPolicyRequest),
    UpdateFirewallPolicy {
        id: EntityId,
        update: UpdateFirewallPolicyRequest,
    },
    DeleteFirewallPolicy {
        id: EntityId,
    },
    PatchFirewallPolicy {
        id: EntityId,
        enabled: bool,
    },
    ReorderFirewallPolicies {
        zone_pair: (EntityId, EntityId),
        ordered_ids: Vec<EntityId>,
    },
    CreateFirewallZone(CreateFirewallZoneRequest),
    UpdateFirewallZone {
        id: EntityId,
        update: UpdateFirewallZoneRequest,
    },
    DeleteFirewallZone {
        id: EntityId,
    },

    // ── ACL ──────────────────────────────────────────────────────────
    CreateAclRule(CreateAclRuleRequest),
    UpdateAclRule {
        id: EntityId,
        update: UpdateAclRuleRequest,
    },
    DeleteAclRule {
        id: EntityId,
    },
    ReorderAclRules {
        ordered_ids: Vec<EntityId>,
    },

    // ── DNS ──────────────────────────────────────────────────────────
    CreateDnsPolicy(CreateDnsPolicyRequest),
    UpdateDnsPolicy {
        id: EntityId,
        update: UpdateDnsPolicyRequest,
    },
    DeleteDnsPolicy {
        id: EntityId,
    },

    // ── Traffic matching lists ───────────────────────────────────────
    CreateTrafficMatchingList(CreateTrafficMatchingListRequest),
    UpdateTrafficMatchingList {
        id: EntityId,
        update: UpdateTrafficMatchingListRequest,
    },
    DeleteTrafficMatchingList {
        id: EntityId,
    },

    // ── Hotspot / Vouchers ───────────────────────────────────────────
    CreateVouchers(CreateVouchersRequest),
    DeleteVoucher {
        id: EntityId,
    },
    PurgeVouchers {
        filter: String,
    },

    // ── System (Legacy) ──────────────────────────────────────────────
    ArchiveAlarm {
        id: EntityId,
    },
    ArchiveAllAlarms,
    CreateSite {
        name: String,
        description: String,
    },
    DeleteSite {
        name: String,
    },
    CreateBackup,
    DeleteBackup {
        filename: String,
    },
    RebootController,
    PoweroffController,
    InviteAdmin {
        name: String,
        email: String,
        role: String,
    },
    RevokeAdmin {
        id: EntityId,
    },
    UpdateAdmin {
        id: EntityId,
        role: Option<String>,
    },
}

/// Result of a command execution.
#[derive(Debug)]
pub enum CommandResult {
    Ok,
    Device(Device),
    Client(Client),
    Network(Network),
    WifiBroadcast(WifiBroadcast),
    FirewallPolicy(FirewallPolicy),
    FirewallZone(FirewallZone),
    AclRule(AclRule),
    DnsPolicy(DnsPolicy),
    Vouchers(Vec<Voucher>),
    TrafficMatchingList(TrafficMatchingList),
}
