// ── Command API ──
//
// All write operations flow through a unified `Command` enum.
// The controller routes each variant to the appropriate API backend
// (Integration API preferred, Legacy API for legacy-only operations).

use crate::error::CoreError;
use crate::model::*;

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
    AdoptDevice { mac: MacAddress },
    RemoveDevice { id: EntityId },
    RestartDevice { id: EntityId },
    LocateDevice { mac: MacAddress, enable: bool },
    UpgradeDevice { mac: MacAddress, firmware_url: Option<String> },
    ProvisionDevice { mac: MacAddress },
    SpeedtestDevice,
    PowerCyclePort { device_id: EntityId, port_idx: u32 },

    // ── Client operations ────────────────────────────────────────────
    BlockClient { mac: MacAddress },
    UnblockClient { mac: MacAddress },
    KickClient { mac: MacAddress },
    ForgetClient { mac: MacAddress },
    AuthorizeGuest {
        client_id: EntityId,
        time_limit_minutes: Option<u32>,
        data_limit_mb: Option<u64>,
        rx_rate_kbps: Option<u64>,
        tx_rate_kbps: Option<u64>,
    },
    UnauthorizeGuest { client_id: EntityId },

    // ── Network CRUD ─────────────────────────────────────────────────
    CreateNetwork { data: serde_json::Value },
    UpdateNetwork { id: EntityId, data: serde_json::Value },
    DeleteNetwork { id: EntityId },

    // ── WiFi CRUD ────────────────────────────────────────────────────
    CreateWifi { data: serde_json::Value },
    UpdateWifi { id: EntityId, data: serde_json::Value },
    DeleteWifi { id: EntityId },

    // ── Firewall ─────────────────────────────────────────────────────
    CreateFirewallPolicy { data: serde_json::Value },
    UpdateFirewallPolicy { id: EntityId, data: serde_json::Value },
    DeleteFirewallPolicy { id: EntityId },
    ReorderFirewallPolicies {
        zone_pair: (EntityId, EntityId),
        ordered_ids: Vec<EntityId>,
    },
    ReorderAclRules { ordered_ids: Vec<EntityId> },

    // ── DNS ──────────────────────────────────────────────────────────
    CreateDnsPolicy { data: serde_json::Value },
    UpdateDnsPolicy { id: EntityId, data: serde_json::Value },
    DeleteDnsPolicy { id: EntityId },

    // ── Hotspot / Vouchers ───────────────────────────────────────────
    CreateVoucher { data: serde_json::Value },
    DeleteVoucher { id: EntityId },
    PurgeVouchers { filter: String },

    // ── System (Legacy) ──────────────────────────────────────────────
    ArchiveAlarm { id: EntityId },
    ArchiveAllAlarms,
    CreateSite { name: String, description: String },
    DeleteSite { name: String },
    CreateBackup,
    DeleteBackup { filename: String },
    RebootController,
    PoweroffController,
    InviteAdmin { name: String, email: String, role: String },
    RevokeAdmin { id: EntityId },
}

/// Result of a command execution.
#[derive(Debug)]
pub enum CommandResult {
    Ok,
    Device(Device),
    Network(Network),
    WifiBroadcast(WifiBroadcast),
    FirewallPolicy(FirewallPolicy),
    DnsPolicy(DnsPolicy),
    Vouchers(Vec<Voucher>),
}
