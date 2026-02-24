// ── API-to-domain type conversions ──
//
// Bridges raw `unifly_api` response types into canonical `unifly_core::model`
// domain types. Each `From` impl normalizes field names, parses strings into
// strong types, and fills sensible defaults for missing optional data.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Utc};
use serde_json::Value;

use unifly_api::integration_types;
use unifly_api::legacy::models::{
    LegacyAlarm, LegacyClientEntry, LegacyDevice, LegacyEvent, LegacySite,
};
use unifly_api::websocket::UnifiEvent;

use crate::model::{
    client::{Client, ClientType, GuestAuth, WirelessInfo},
    common::{Bandwidth, DataSource, EntityOrigin},
    device::{Device, DeviceState, DeviceStats, DeviceType},
    dns::{DnsPolicy, DnsPolicyType},
    entity_id::{EntityId, MacAddress},
    event::{Alarm, Event, EventCategory, EventSeverity},
    firewall::{AclAction, AclRule, AclRuleType, FirewallAction, FirewallPolicy, FirewallZone},
    hotspot::Voucher,
    network::{DhcpConfig, Ipv6Mode, Network, NetworkManagement},
    site::Site,
    supporting::TrafficMatchingList,
    wifi::{WifiBroadcast, WifiBroadcastType, WifiSecurityMode},
};

// ── Helpers ────────────────────────────────────────────────────────

/// Parse an optional string to an `IpAddr`, silently dropping unparseable values.
fn parse_ip(raw: Option<&String>) -> Option<IpAddr> {
    raw.and_then(|s| s.parse().ok())
}

/// Convert an optional epoch-seconds timestamp to `DateTime<Utc>`.
fn epoch_to_datetime(epoch: Option<i64>) -> Option<DateTime<Utc>> {
    epoch.and_then(|ts| DateTime::from_timestamp(ts, 0))
}

/// Parse an ISO-8601 datetime string (as returned by the legacy event/alarm endpoints).
fn parse_datetime(raw: Option<&String>) -> Option<DateTime<Utc>> {
    raw.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn parse_ipv6_text(raw: &str) -> Option<std::net::Ipv6Addr> {
    let candidate = raw.trim().split('/').next().unwrap_or(raw).trim();
    candidate.parse::<std::net::Ipv6Addr>().ok()
}

fn pick_ipv6_from_value(value: &Value) -> Option<String> {
    let mut first_link_local: Option<String> = None;

    let iter: Box<dyn Iterator<Item = &Value> + '_> = match value {
        Value::Array(items) => Box::new(items.iter()),
        _ => Box::new(std::iter::once(value)),
    };

    for item in iter {
        if let Some(ipv6) = item.as_str().and_then(parse_ipv6_text) {
            let ip_text = ipv6.to_string();
            if !ipv6.is_unicast_link_local() {
                return Some(ip_text);
            }
            if first_link_local.is_none() {
                first_link_local = Some(ip_text);
            }
        }
    }

    first_link_local
}

fn parse_legacy_wan_ipv6(extra: &serde_json::Map<String, Value>) -> Option<String> {
    // Primary source on gateways: wan1.ipv6 = ["global", "link-local"].
    if let Some(v) = extra
        .get("wan1")
        .and_then(|wan| wan.get("ipv6"))
        .and_then(pick_ipv6_from_value)
    {
        return Some(v);
    }

    // Fallback source on some firmware: top-level ipv6 array.
    extra.get("ipv6").and_then(pick_ipv6_from_value)
}

// ── Device ─────────────────────────────────────────────────────────

/// Infer `DeviceType` from the legacy `type` field and optional `model` string.
///
/// The legacy API `type` field is typically: `"uap"`, `"usw"`, `"ugw"`, `"udm"`.
/// We also check the `model` prefix for newer hardware that may not match cleanly.
fn infer_device_type(device_type: &str, model: Option<&String>) -> DeviceType {
    match device_type {
        "uap" => DeviceType::AccessPoint,
        "usw" => DeviceType::Switch,
        "ugw" | "udm" => DeviceType::Gateway,
        _ => {
            // Fallback: check the model string prefix
            if let Some(m) = model {
                let upper = m.to_uppercase();
                if upper.starts_with("UAP") || upper.starts_with("U6") || upper.starts_with("U7") {
                    DeviceType::AccessPoint
                } else if upper.starts_with("USW") || upper.starts_with("USL") {
                    DeviceType::Switch
                } else if upper.starts_with("UGW")
                    || upper.starts_with("UDM")
                    || upper.starts_with("UDR")
                    || upper.starts_with("UXG")
                    || upper.starts_with("UCG")
                    || upper.starts_with("UCK")
                {
                    DeviceType::Gateway
                } else {
                    DeviceType::Other
                }
            } else {
                DeviceType::Other
            }
        }
    }
}

/// Map the legacy integer state code to `DeviceState`.
///
/// Known codes: 0=offline, 1=online, 2=pending adoption, 4=upgrading, 5=provisioning.
fn map_device_state(code: i32) -> DeviceState {
    match code {
        0 => DeviceState::Offline,
        1 => DeviceState::Online,
        2 => DeviceState::PendingAdoption,
        4 => DeviceState::Updating,
        5 => DeviceState::GettingReady,
        _ => DeviceState::Unknown,
    }
}

impl From<LegacyDevice> for Device {
    fn from(d: LegacyDevice) -> Self {
        let device_type = infer_device_type(&d.device_type, d.model.as_ref());
        let state = map_device_state(d.state);

        // Build device_stats from sys_stats + uptime
        let device_stats = {
            let mut s = DeviceStats {
                uptime_secs: d.uptime.and_then(|u| u.try_into().ok()),
                ..Default::default()
            };
            if let Some(ref sys) = d.sys_stats {
                s.load_average_1m = sys.load_1.as_deref().and_then(|v| v.parse().ok());
                s.load_average_5m = sys.load_5.as_deref().and_then(|v| v.parse().ok());
                s.load_average_15m = sys.load_15.as_deref().and_then(|v| v.parse().ok());
                s.cpu_utilization_pct = sys.cpu.as_deref().and_then(|v| v.parse().ok());
                // Memory utilization as a percentage
                s.memory_utilization_pct = match (sys.mem_used, sys.mem_total) {
                    (Some(used), Some(total)) if total > 0 =>
                    {
                        #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
                        Some((used as f64 / total as f64) * 100.0)
                    }
                    _ => None,
                };
            }
            s
        };

        Device {
            id: EntityId::from(d.id),
            mac: MacAddress::new(&d.mac),
            ip: parse_ip(d.ip.as_ref()),
            wan_ipv6: parse_legacy_wan_ipv6(&d.extra),
            name: d.name,
            model: d.model,
            device_type,
            state,
            firmware_version: d.version,
            firmware_updatable: d.upgradable.unwrap_or(false),
            adopted_at: None, // Legacy API doesn't provide adoption timestamp
            provisioned_at: None,
            last_seen: epoch_to_datetime(d.last_seen),
            serial: d.serial,
            supported: true, // Legacy API only returns adopted/supported devices
            ports: Vec::new(),
            radios: Vec::new(),
            uplink_device_id: None,
            uplink_device_mac: d
                .extra
                .get("uplink")
                .and_then(|u| u.get("uplink_mac"))
                .and_then(|v| v.as_str())
                .map(MacAddress::new),
            has_switching: device_type == DeviceType::Switch || device_type == DeviceType::Gateway,
            has_access_point: device_type == DeviceType::AccessPoint,
            stats: device_stats,
            client_count: d.num_sta.and_then(|n| n.try_into().ok()),
            origin: None,
            source: DataSource::LegacyApi,
            updated_at: Utc::now(),
        }
    }
}

// ── Client ─────────────────────────────────────────────────────────

impl From<LegacyClientEntry> for Client {
    fn from(c: LegacyClientEntry) -> Self {
        let is_wired = c.is_wired.unwrap_or(false);
        let client_type = if is_wired {
            ClientType::Wired
        } else {
            ClientType::Wireless
        };

        // Build wireless info for non-wired clients
        let wireless = if is_wired {
            None
        } else {
            Some(WirelessInfo {
                ssid: c.essid.clone(),
                bssid: c.bssid.as_deref().map(MacAddress::new),
                channel: c.channel.and_then(|ch| ch.try_into().ok()),
                frequency_ghz: channel_to_frequency(c.channel),
                signal_dbm: c.signal.or(c.rssi),
                noise_dbm: c.noise,
                satisfaction: c.satisfaction.and_then(|s| s.try_into().ok()),
                tx_rate_kbps: c.tx_rate.and_then(|r| r.try_into().ok()),
                rx_rate_kbps: c.rx_rate.and_then(|r| r.try_into().ok()),
            })
        };

        // Build guest auth if the client is a guest
        let is_guest = c.is_guest.unwrap_or(false);
        let guest_auth = if is_guest {
            Some(GuestAuth {
                authorized: c.authorized.unwrap_or(false),
                method: None,
                expires_at: None,
                tx_bytes: c.tx_bytes.and_then(|b| b.try_into().ok()),
                rx_bytes: c.rx_bytes.and_then(|b| b.try_into().ok()),
                elapsed_minutes: None,
            })
        } else {
            None
        };

        // Determine uplink device MAC based on connection type
        let uplink_device_mac = if is_wired {
            c.sw_mac.as_deref().map(MacAddress::new)
        } else {
            c.ap_mac.as_deref().map(MacAddress::new)
        };

        // Estimate connected_at from uptime
        let connected_at = c.uptime.and_then(|secs| {
            let duration = chrono::Duration::seconds(secs);
            Utc::now().checked_sub_signed(duration)
        });

        Client {
            id: EntityId::from(c.id),
            mac: MacAddress::new(&c.mac),
            ip: parse_ip(c.ip.as_ref()),
            name: c.name,
            hostname: c.hostname,
            client_type,
            connected_at,
            uplink_device_id: None,
            uplink_device_mac,
            network_id: c.network_id.map(EntityId::from),
            vlan: None,
            wireless,
            guest_auth,
            is_guest,
            tx_bytes: c.tx_bytes.or(c.wired_tx_bytes).and_then(|b| b.try_into().ok()),
            rx_bytes: c.rx_bytes.or(c.wired_rx_bytes).and_then(|b| b.try_into().ok()),
            bandwidth: {
                let tx = c.tx_bytes_r.or(c.wired_tx_bytes_r);
                let rx = c.rx_bytes_r.or(c.wired_rx_bytes_r);
                if tx.is_some() || rx.is_some() {
                    Some(crate::model::Bandwidth {
                        tx_bytes_per_sec: tx.unwrap_or(0.0) as u64,
                        rx_bytes_per_sec: rx.unwrap_or(0.0) as u64,
                    })
                } else {
                    None
                }
            },
            oui: c.oui,
            network_name: c.network,
            sw_port: c.sw_port.and_then(|p| p.try_into().ok()),
            os_name: None,
            device_class: None,
            blocked: c.blocked.unwrap_or(false),
            source: DataSource::LegacyApi,
            updated_at: Utc::now(),
        }
    }
}

/// Rough channel-to-frequency mapping for common Wi-Fi channels.
fn channel_to_frequency(channel: Option<i32>) -> Option<f32> {
    channel.map(|ch| match ch {
        1..=14 => 2.4,
        32..=68 | 96..=177 => 5.0,
        _ => 6.0, // Wi-Fi 6E / 7
    })
}

// ── Site ───────────────────────────────────────────────────────────

impl From<LegacySite> for Site {
    fn from(s: LegacySite) -> Self {
        // `desc` is the human-friendly label; `name` is the internal slug (e.g. "default").
        let display_name = s
            .desc
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| s.name.clone());

        Site {
            id: EntityId::from(s.id),
            internal_name: s.name,
            name: display_name,
            device_count: None,
            client_count: None,
            source: DataSource::LegacyApi,
        }
    }
}

// ── Event ──────────────────────────────────────────────────────────

/// Map legacy subsystem string to `EventCategory`.
fn map_event_category(subsystem: Option<&String>) -> EventCategory {
    match subsystem.map(String::as_str) {
        Some("wlan" | "lan" | "wan") => EventCategory::Network,
        Some("device") => EventCategory::Device,
        Some("client") => EventCategory::Client,
        Some("system") => EventCategory::System,
        Some("admin") => EventCategory::Admin,
        Some("firewall") => EventCategory::Firewall,
        Some("vpn") => EventCategory::Vpn,
        _ => EventCategory::Unknown,
    }
}

impl From<LegacyEvent> for Event {
    fn from(e: LegacyEvent) -> Self {
        Event {
            id: Some(EntityId::from(e.id)),
            timestamp: parse_datetime(e.datetime.as_ref()).unwrap_or_else(Utc::now),
            category: map_event_category(e.subsystem.as_ref()),
            severity: EventSeverity::Info,
            event_type: e.key.clone().unwrap_or_default(),
            message: e.msg.unwrap_or_default(),
            device_mac: None,
            client_mac: None,
            site_id: e.site_id.map(EntityId::from),
            raw_key: e.key,
            source: DataSource::LegacyApi,
        }
    }
}

// ── Alarm → Event ──────────────────────────────────────────────────

impl From<LegacyAlarm> for Event {
    fn from(a: LegacyAlarm) -> Self {
        Event {
            id: Some(EntityId::from(a.id)),
            timestamp: parse_datetime(a.datetime.as_ref()).unwrap_or_else(Utc::now),
            category: EventCategory::System,
            severity: EventSeverity::Warning,
            event_type: a.key.clone().unwrap_or_default(),
            message: a.msg.unwrap_or_default(),
            device_mac: None,
            client_mac: None,
            site_id: None,
            raw_key: a.key,
            source: DataSource::LegacyApi,
        }
    }
}

impl From<LegacyAlarm> for Alarm {
    fn from(a: LegacyAlarm) -> Self {
        Alarm {
            id: EntityId::from(a.id),
            timestamp: parse_datetime(a.datetime.as_ref()).unwrap_or_else(Utc::now),
            category: EventCategory::System,
            severity: EventSeverity::Warning,
            message: a.msg.unwrap_or_default(),
            archived: a.archived.unwrap_or(false),
            device_mac: None,
            site_id: None,
        }
    }
}

// ── WebSocket Event ──────────────────────────────────────────────

/// Infer severity from a WebSocket event key.
///
/// Disconnect/Lost/Down keywords → Warning, Error/Fail → Error, else Info.
fn infer_ws_severity(key: &str) -> EventSeverity {
    let upper = key.to_uppercase();
    if upper.contains("ERROR") || upper.contains("FAIL") {
        EventSeverity::Error
    } else if upper.contains("DISCONNECT") || upper.contains("LOST") || upper.contains("DOWN") {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    }
}

impl From<UnifiEvent> for Event {
    fn from(e: UnifiEvent) -> Self {
        let category = map_event_category(Some(&e.subsystem));
        let severity = infer_ws_severity(&e.key);

        // Extract device MAC from common extra fields
        let device_mac = e
            .extra
            .get("mac")
            .or_else(|| e.extra.get("sw"))
            .or_else(|| e.extra.get("ap"))
            .and_then(|v| v.as_str())
            .map(MacAddress::new);

        // Extract client MAC from common extra fields
        let client_mac = e
            .extra
            .get("user")
            .or_else(|| e.extra.get("sta"))
            .and_then(|v| v.as_str())
            .map(MacAddress::new);

        let site_id = if e.site_id.is_empty() {
            None
        } else {
            Some(EntityId::Legacy(e.site_id))
        };

        Event {
            id: None,
            timestamp: parse_datetime(e.datetime.as_ref()).unwrap_or_else(Utc::now),
            category,
            severity,
            event_type: e.key.clone(),
            message: e.message.unwrap_or_default(),
            device_mac,
            client_mac,
            site_id,
            raw_key: Some(e.key),
            source: DataSource::LegacyApi,
        }
    }
}

// ━━ Integration API conversions ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

// ── Helpers ────────────────────────────────────────────────────────

/// Parse an ISO-8601 string (Integration API format) to `DateTime<Utc>`.
fn parse_iso(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Map Integration API management string to `EntityOrigin`.
fn map_origin(management: &str) -> Option<EntityOrigin> {
    match management {
        "USER_DEFINED" => Some(EntityOrigin::UserDefined),
        "SYSTEM_DEFINED" => Some(EntityOrigin::SystemDefined),
        "ORCHESTRATED" => Some(EntityOrigin::Orchestrated),
        _ => None,
    }
}

/// Extract origin from a `metadata` JSON object.
///
/// Checks `metadata.origin` (real API) and `metadata.management` (spec)
/// since the field name varies by firmware version.
fn origin_from_metadata(metadata: &serde_json::Value) -> Option<EntityOrigin> {
    metadata
        .get("origin")
        .or_else(|| metadata.get("management"))
        .and_then(|v| v.as_str())
        .and_then(map_origin)
}

/// Map Integration API device state string to `DeviceState`.
fn map_integration_device_state(state: &str) -> DeviceState {
    match state {
        "ONLINE" => DeviceState::Online,
        "OFFLINE" => DeviceState::Offline,
        "PENDING_ADOPTION" => DeviceState::PendingAdoption,
        "UPDATING" => DeviceState::Updating,
        "GETTING_READY" => DeviceState::GettingReady,
        "ADOPTING" => DeviceState::Adopting,
        "DELETING" => DeviceState::Deleting,
        "CONNECTION_INTERRUPTED" => DeviceState::ConnectionInterrupted,
        "ISOLATED" => DeviceState::Isolated,
        _ => DeviceState::Unknown,
    }
}

/// Infer `DeviceType` from Integration API `features` list and `model` string.
fn infer_device_type_integration(features: &[String], model: &str) -> DeviceType {
    let has = |f: &str| features.iter().any(|s| s == f);

    // Check model prefix first — some gateways (UCG Max) report "switching"
    // without "routing", which would misclassify them as switches.
    let upper = model.to_uppercase();
    let is_gateway_model = upper.starts_with("UGW")
        || upper.starts_with("UDM")
        || upper.starts_with("UDR")
        || upper.starts_with("UXG")
        || upper.starts_with("UCG")
        || upper.starts_with("UCK");

    if is_gateway_model || (has("switching") && has("routing")) || has("gateway") {
        DeviceType::Gateway
    } else if has("accessPoint") {
        DeviceType::AccessPoint
    } else if has("switching") {
        DeviceType::Switch
    } else {
        // Fallback to model prefix
        let model_owned = model.to_owned();
        infer_device_type("", Some(&model_owned))
    }
}

// ── Device ────────────────────────────────────────────────────────

impl From<integration_types::DeviceResponse> for Device {
    fn from(d: integration_types::DeviceResponse) -> Self {
        let device_type = infer_device_type_integration(&d.features, &d.model);
        let state = map_integration_device_state(&d.state);

        Device {
            id: EntityId::Uuid(d.id),
            mac: MacAddress::new(&d.mac_address),
            ip: d.ip_address.as_deref().and_then(|s| s.parse().ok()),
            wan_ipv6: None,
            name: Some(d.name),
            model: Some(d.model),
            device_type,
            state,
            firmware_version: d.firmware_version,
            firmware_updatable: d.firmware_updatable,
            adopted_at: None,
            provisioned_at: None,
            last_seen: None,
            serial: None,
            supported: d.supported,
            ports: Vec::new(),
            radios: Vec::new(),
            uplink_device_id: None,
            uplink_device_mac: None,
            has_switching: d.features.iter().any(|f| f == "switching"),
            has_access_point: d.features.iter().any(|f| f == "accessPoint"),
            stats: DeviceStats::default(),
            client_count: None,
            origin: None,
            source: DataSource::IntegrationApi,
            updated_at: Utc::now(),
        }
    }
}

/// Convert Integration API device statistics into domain `DeviceStats`.
pub(crate) fn device_stats_from_integration(
    resp: &integration_types::DeviceStatisticsResponse,
) -> DeviceStats {
    DeviceStats {
        uptime_secs: resp.uptime_sec.and_then(|u| u.try_into().ok()),
        cpu_utilization_pct: resp.cpu_utilization_pct,
        memory_utilization_pct: resp.memory_utilization_pct,
        load_average_1m: resp.load_average_1_min,
        load_average_5m: resp.load_average_5_min,
        load_average_15m: resp.load_average_15_min,
        last_heartbeat: resp.last_heartbeat_at.as_deref().and_then(parse_iso),
        next_heartbeat: resp.next_heartbeat_at.as_deref().and_then(parse_iso),
        uplink_bandwidth: resp.uplink.as_ref().and_then(|u| {
            let tx = u
                .get("txRateBps")
                .or_else(|| u.get("txBytesPerSecond"))
                .or_else(|| u.get("tx_bytes-r"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let rx = u
                .get("rxRateBps")
                .or_else(|| u.get("rxBytesPerSecond"))
                .or_else(|| u.get("rx_bytes-r"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if tx == 0 && rx == 0 {
                None
            } else {
                Some(Bandwidth {
                    tx_bytes_per_sec: tx,
                    rx_bytes_per_sec: rx,
                })
            }
        }),
    }
}

// ── Client ────────────────────────────────────────────────────────

impl From<integration_types::ClientResponse> for Client {
    fn from(c: integration_types::ClientResponse) -> Self {
        let client_type = match c.client_type.as_str() {
            "WIRED" => ClientType::Wired,
            "WIRELESS" => ClientType::Wireless,
            "VPN" => ClientType::Vpn,
            "TELEPORT" => ClientType::Teleport,
            _ => ClientType::Unknown,
        };

        // Extract MAC from access object; fall back to UUID so clients
        // without a macAddress still get unique store keys.
        let mac_from_access = c
            .access
            .get("macAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let uuid_fallback = c.id.to_string();
        let mac_str = if mac_from_access.is_empty() {
            uuid_fallback.as_str()
        } else {
            mac_from_access.as_str()
        };

        Client {
            id: EntityId::Uuid(c.id),
            mac: MacAddress::new(mac_str),
            ip: c.ip_address.as_deref().and_then(|s| s.parse().ok()),
            name: Some(c.name),
            hostname: None,
            client_type,
            connected_at: c.connected_at.as_deref().and_then(parse_iso),
            uplink_device_id: None,
            uplink_device_mac: None,
            network_id: None,
            vlan: None,
            wireless: None,
            guest_auth: None,
            is_guest: false,
            tx_bytes: None,
            rx_bytes: None,
            bandwidth: None,
            oui: None,
            network_name: None,
            sw_port: None,
            os_name: None,
            device_class: None,
            blocked: false,
            source: DataSource::IntegrationApi,
            updated_at: Utc::now(),
        }
    }
}

// ── Site ──────────────────────────────────────────────────────────

impl From<integration_types::SiteResponse> for Site {
    fn from(s: integration_types::SiteResponse) -> Self {
        Site {
            id: EntityId::Uuid(s.id),
            internal_name: s.internal_reference,
            name: s.name,
            device_count: None,
            client_count: None,
            source: DataSource::IntegrationApi,
        }
    }
}

// ── Network ──────────────────────────────────────────────────────

/// Look up a field in `extra` first, then fall back to `metadata`.
fn net_field<'a>(
    extra: &'a HashMap<String, Value>,
    metadata: &'a Value,
    key: &str,
) -> Option<&'a Value> {
    extra.get(key).or_else(|| metadata.get(key))
}

/// Parse network configuration from API extra/metadata fields into a `Network`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn parse_network_fields(
    id: uuid::Uuid,
    name: String,
    enabled: bool,
    management_str: &str,
    vlan_id: i32,
    is_default: bool,
    metadata: &Value,
    extra: &HashMap<String, Value>,
) -> Network {
    // ── Feature flags ───────────────────────────────────────────
    let isolation_enabled = net_field(extra, metadata, "isolationEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let internet_access_enabled = net_field(extra, metadata, "internetAccessEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mdns_forwarding_enabled = net_field(extra, metadata, "mdnsForwardingEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let cellular_backup_enabled = net_field(extra, metadata, "cellularBackupEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // ── Firewall zone ───────────────────────────────────────────
    let firewall_zone_id = net_field(extra, metadata, "zoneId")
        .and_then(Value::as_str)
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(EntityId::Uuid);

    // ── IPv4 configuration ──────────────────────────────────────
    // Detail API uses: hostIpAddress, prefixLength, dhcpConfiguration
    // Some firmware uses: host, prefix, dhcp.server
    let ipv4 = net_field(extra, metadata, "ipv4Configuration");

    let gateway_ip: Option<Ipv4Addr> = ipv4
        .and_then(|v| v.get("hostIpAddress").or_else(|| v.get("host")))
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok());

    let subnet = ipv4.and_then(|v| {
        let host = v.get("hostIpAddress").or_else(|| v.get("host"))?.as_str()?;
        let prefix = v
            .get("prefixLength")
            .or_else(|| v.get("prefix"))?
            .as_u64()?;
        Some(format!("{host}/{prefix}"))
    });

    // ── DHCP ────────────────────────────────────────────────────
    // Detail API: dhcpConfiguration.mode/leaseTimeSeconds/ipAddressRange/dnsServerIpAddressesOverride
    // Fallback:   dhcp.server.enabled/rangeStart/rangeStop/leaseTimeSec/dnsOverride.servers
    let dhcp = ipv4.and_then(|v| {
        // Try new-style dhcpConfiguration first
        if let Some(dhcp_cfg) = v.get("dhcpConfiguration") {
            let mode = dhcp_cfg.get("mode").and_then(Value::as_str).unwrap_or("");
            let dhcp_enabled = mode == "SERVER";
            let range = dhcp_cfg.get("ipAddressRange");
            let range_start = range
                .and_then(|r| r.get("start").or_else(|| r.get("rangeStart")))
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok());
            let range_stop = range
                .and_then(|r| r.get("end").or_else(|| r.get("rangeStop")))
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok());
            let lease_time_secs = dhcp_cfg.get("leaseTimeSeconds").and_then(Value::as_u64);
            let dns_servers = dhcp_cfg
                .get("dnsServerIpAddressesOverride")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str()?.parse::<IpAddr>().ok())
                        .collect()
                })
                .unwrap_or_default();
            return Some(DhcpConfig {
                enabled: dhcp_enabled,
                range_start,
                range_stop,
                lease_time_secs,
                dns_servers,
                gateway: gateway_ip,
            });
        }

        // Fallback: old-style dhcp.server
        let server = v.get("dhcp")?.get("server")?;
        let dhcp_enabled = server
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let range_start = server
            .get("rangeStart")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok());
        let range_stop = server
            .get("rangeStop")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok());
        let lease_time_secs = server.get("leaseTimeSec").and_then(Value::as_u64);
        let dns_servers = server
            .get("dnsOverride")
            .and_then(|d| d.get("servers"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str()?.parse::<IpAddr>().ok())
                    .collect()
            })
            .unwrap_or_default();
        let gateway = server
            .get("gateway")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .or(gateway_ip);
        Some(DhcpConfig {
            enabled: dhcp_enabled,
            range_start,
            range_stop,
            lease_time_secs,
            dns_servers,
            gateway,
        })
    });

    // ── PXE / NTP / TFTP ────────────────────────────────────────
    let pxe_enabled = ipv4
        .and_then(|v| v.get("pxe"))
        .and_then(|v| v.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ntp_server = ipv4
        .and_then(|v| v.get("ntp"))
        .and_then(|v| v.get("server"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<IpAddr>().ok());
    let tftp_server = ipv4
        .and_then(|v| v.get("tftp"))
        .and_then(|v| v.get("server"))
        .and_then(Value::as_str)
        .map(String::from);

    // ── IPv6 ────────────────────────────────────────────────────
    // Detail API: interfaceType, clientAddressAssignment.slaacEnabled, additionalHostIpSubnets
    // Fallback:   type, slaac.enabled, dhcpv6.enabled, prefix
    let ipv6 = net_field(extra, metadata, "ipv6Configuration");
    let ipv6_enabled = ipv6.is_some();
    let ipv6_mode = ipv6
        .and_then(|v| v.get("interfaceType").or_else(|| v.get("type")))
        .and_then(Value::as_str)
        .and_then(|s| match s {
            "PREFIX_DELEGATION" => Some(Ipv6Mode::PrefixDelegation),
            "STATIC" => Some(Ipv6Mode::Static),
            _ => None,
        });
    let slaac_enabled = ipv6
        .and_then(|v| {
            // New: clientAddressAssignment.slaacEnabled
            v.get("clientAddressAssignment")
                .and_then(|ca| ca.get("slaacEnabled"))
                .and_then(Value::as_bool)
                // Fallback: slaac.enabled
                .or_else(|| v.get("slaac").and_then(|s| s.get("enabled")).and_then(Value::as_bool))
        })
        .unwrap_or(false);
    let dhcpv6_enabled = ipv6
        .and_then(|v| {
            v.get("clientAddressAssignment")
                .and_then(|ca| ca.get("dhcpv6Enabled"))
                .and_then(Value::as_bool)
                .or_else(|| {
                    v.get("dhcpv6")
                        .and_then(|d| d.get("enabled"))
                        .and_then(Value::as_bool)
                })
        })
        .unwrap_or(false);
    let ipv6_prefix = ipv6.and_then(|v| {
        // New: additionalHostIpSubnets[0]
        v.get("additionalHostIpSubnets")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .map(String::from)
                // Fallback: prefix
                .or_else(|| v.get("prefix").and_then(Value::as_str).map(String::from))
    });

    // ── Management type inference ───────────────────────────────
    let has_ipv4_config = ipv4.is_some();
    let has_device_id = extra.contains_key("deviceId");
    let management = if has_ipv4_config && !has_device_id {
        Some(NetworkManagement::Gateway)
    } else if has_device_id {
        Some(NetworkManagement::Switch)
    } else if has_ipv4_config {
        Some(NetworkManagement::Gateway)
    } else {
        None
    };

    Network {
        id: EntityId::Uuid(id),
        name,
        enabled,
        management,
        purpose: None,
        is_default,
        #[allow(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        vlan_id: Some(vlan_id as u16),
        subnet,
        gateway_ip,
        dhcp,
        ipv6_enabled,
        ipv6_mode,
        ipv6_prefix,
        dhcpv6_enabled,
        slaac_enabled,
        ntp_server,
        pxe_enabled,
        tftp_server,
        firewall_zone_id,
        isolation_enabled,
        internet_access_enabled,
        mdns_forwarding_enabled,
        cellular_backup_enabled,
        origin: map_origin(management_str),
        source: DataSource::IntegrationApi,
    }
}

impl From<integration_types::NetworkResponse> for Network {
    fn from(n: integration_types::NetworkResponse) -> Self {
        parse_network_fields(
            n.id,
            n.name,
            n.enabled,
            &n.management,
            n.vlan_id,
            n.default,
            &n.metadata,
            &n.extra,
        )
    }
}

impl From<integration_types::NetworkDetailsResponse> for Network {
    fn from(n: integration_types::NetworkDetailsResponse) -> Self {
        parse_network_fields(
            n.id,
            n.name,
            n.enabled,
            &n.management,
            n.vlan_id,
            n.default,
            &n.metadata,
            &n.extra,
        )
    }
}

// ── WiFi Broadcast ───────────────────────────────────────────────

impl From<integration_types::WifiBroadcastResponse> for WifiBroadcast {
    fn from(w: integration_types::WifiBroadcastResponse) -> Self {
        let broadcast_type = match w.broadcast_type.as_str() {
            "IOT_OPTIMIZED" => WifiBroadcastType::IotOptimized,
            _ => WifiBroadcastType::Standard,
        };

        let security = w
            .security_configuration
            .get("mode")
            .and_then(|v| v.as_str())
            .map_or(WifiSecurityMode::Open, |mode| match mode {
                "WPA2_PERSONAL" => WifiSecurityMode::Wpa2Personal,
                "WPA3_PERSONAL" => WifiSecurityMode::Wpa3Personal,
                "WPA2_WPA3_PERSONAL" => WifiSecurityMode::Wpa2Wpa3Personal,
                "WPA2_ENTERPRISE" => WifiSecurityMode::Wpa2Enterprise,
                "WPA3_ENTERPRISE" => WifiSecurityMode::Wpa3Enterprise,
                "WPA2_WPA3_ENTERPRISE" => WifiSecurityMode::Wpa2Wpa3Enterprise,
                _ => WifiSecurityMode::Open,
            });

        WifiBroadcast {
            id: EntityId::Uuid(w.id),
            name: w.name,
            enabled: w.enabled,
            broadcast_type,
            security,
            network_id: w
                .network
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .map(EntityId::Uuid),
            frequencies_ghz: Vec::new(),
            hidden: false,
            client_isolation: false,
            band_steering: false,
            mlo_enabled: false,
            fast_roaming: false,
            hotspot_enabled: false,
            origin: origin_from_metadata(&w.metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

// ── Firewall Policy ──────────────────────────────────────────────

impl From<integration_types::FirewallPolicyResponse> for FirewallPolicy {
    fn from(p: integration_types::FirewallPolicyResponse) -> Self {
        let action = p.action.get("type").and_then(|v| v.as_str()).map_or(
            FirewallAction::Block,
            |a| match a {
                "ALLOW" => FirewallAction::Allow,
                "REJECT" => FirewallAction::Reject,
                _ => FirewallAction::Block,
            },
        );

        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
        let index = p
            .extra
            .get("index")
            .and_then(serde_json::Value::as_i64)
            .map(|i| i as i32);

        // Zone IDs may be in flat fields (real API) or nested source/destination objects (spec)
        let source_zone_id = p
            .extra
            .get("sourceFirewallZoneId")
            .and_then(|v| v.as_str())
            .or_else(|| {
                p.extra
                    .get("source")
                    .and_then(|v| v.get("zoneId"))
                    .and_then(|v| v.as_str())
            })
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(EntityId::Uuid);

        let destination_zone_id = p
            .extra
            .get("destinationFirewallZoneId")
            .and_then(|v| v.as_str())
            .or_else(|| {
                p.extra
                    .get("destination")
                    .and_then(|v| v.get("zoneId"))
                    .and_then(|v| v.as_str())
            })
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(EntityId::Uuid);

        let ipsec_mode = p
            .extra
            .get("ipsecFilter")
            .and_then(|v| v.as_str())
            .map(String::from);

        let connection_states = p
            .extra
            .get("connectionStateFilter")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        FirewallPolicy {
            id: EntityId::Uuid(p.id),
            name: p.name,
            description: p.description,
            enabled: p.enabled,
            index,
            action,
            ip_version: crate::model::firewall::IpVersion::Both,
            source_zone_id,
            destination_zone_id,
            source_summary: None,
            destination_summary: None,
            protocol_summary: None,
            schedule: None,
            ipsec_mode,
            connection_states,
            logging_enabled: p.logging_enabled,
            origin: p.metadata.as_ref().and_then(origin_from_metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

// ── Firewall Zone ────────────────────────────────────────────────

impl From<integration_types::FirewallZoneResponse> for FirewallZone {
    fn from(z: integration_types::FirewallZoneResponse) -> Self {
        FirewallZone {
            id: EntityId::Uuid(z.id),
            name: z.name,
            network_ids: z.network_ids.into_iter().map(EntityId::Uuid).collect(),
            origin: origin_from_metadata(&z.metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

// ── ACL Rule ─────────────────────────────────────────────────────

impl From<integration_types::AclRuleResponse> for AclRule {
    fn from(r: integration_types::AclRuleResponse) -> Self {
        let rule_type = match r.rule_type.as_str() {
            "MAC" => AclRuleType::Mac,
            _ => AclRuleType::Ipv4,
        };

        let action = match r.action.as_str() {
            "ALLOW" => AclAction::Allow,
            _ => AclAction::Block,
        };

        AclRule {
            id: EntityId::Uuid(r.id),
            name: r.name,
            enabled: r.enabled,
            rule_type,
            action,
            source_summary: None,
            destination_summary: None,
            origin: origin_from_metadata(&r.metadata),
            source: DataSource::IntegrationApi,
        }
    }
}

// ── DNS Policy ───────────────────────────────────────────────────

impl From<integration_types::DnsPolicyResponse> for DnsPolicy {
    fn from(d: integration_types::DnsPolicyResponse) -> Self {
        let policy_type = match d.policy_type.as_str() {
            "A" => DnsPolicyType::ARecord,
            "AAAA" => DnsPolicyType::AaaaRecord,
            "CNAME" => DnsPolicyType::CnameRecord,
            "MX" => DnsPolicyType::MxRecord,
            "TXT" => DnsPolicyType::TxtRecord,
            "SRV" => DnsPolicyType::SrvRecord,
            _ => DnsPolicyType::ForwardDomain,
        };

        DnsPolicy {
            id: EntityId::Uuid(d.id),
            policy_type,
            domain: d.domain.unwrap_or_default(),
            value: d
                .extra
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            ttl_seconds: d
                .extra
                .get("ttl")
                .and_then(serde_json::Value::as_u64)
                .map(|t| t as u32),
            origin: None,
            source: DataSource::IntegrationApi,
        }
    }
}

// ── Traffic Matching List ────────────────────────────────────────

impl From<integration_types::TrafficMatchingListResponse> for TrafficMatchingList {
    fn from(t: integration_types::TrafficMatchingListResponse) -> Self {
        let items = t
            .extra
            .get("items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        TrafficMatchingList {
            id: EntityId::Uuid(t.id),
            name: t.name,
            list_type: t.list_type,
            items,
            origin: None,
        }
    }
}

// ── Voucher ──────────────────────────────────────────────────────

impl From<integration_types::VoucherResponse> for Voucher {
    fn from(v: integration_types::VoucherResponse) -> Self {
        #[allow(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        Voucher {
            id: EntityId::Uuid(v.id),
            code: v.code,
            name: Some(v.name),
            created_at: parse_iso(&v.created_at),
            activated_at: v.activated_at.as_deref().and_then(parse_iso),
            expires_at: v.expires_at.as_deref().and_then(parse_iso),
            expired: v.expired,
            time_limit_minutes: Some(v.time_limit_minutes as u32),
            data_usage_limit_mb: v.data_usage_limit_m_bytes.map(|b| b as u64),
            authorized_guest_limit: v.authorized_guest_limit.map(|l| l as u32),
            authorized_guest_count: Some(v.authorized_guest_count as u32),
            rx_rate_limit_kbps: v.rx_rate_limit_kbps.map(|r| r as u64),
            tx_rate_limit_kbps: v.tx_rate_limit_kbps.map(|r| r as u64),
            source: DataSource::IntegrationApi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_type_from_legacy_type_field() {
        assert_eq!(infer_device_type("uap", None), DeviceType::AccessPoint);
        assert_eq!(infer_device_type("usw", None), DeviceType::Switch);
        assert_eq!(infer_device_type("ugw", None), DeviceType::Gateway);
        assert_eq!(infer_device_type("udm", None), DeviceType::Gateway);
    }

    #[test]
    fn device_type_from_model_fallback() {
        assert_eq!(
            infer_device_type("unknown", Some(&"UAP-AC-Pro".into())),
            DeviceType::AccessPoint
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"U6-LR".into())),
            DeviceType::AccessPoint
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"USW-24-PoE".into())),
            DeviceType::Switch
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"UDM-Pro".into())),
            DeviceType::Gateway
        );
        assert_eq!(
            infer_device_type("unknown", Some(&"UCG-Max".into())),
            DeviceType::Gateway
        );
    }

    #[test]
    fn integration_device_type_gateway_by_model() {
        // UCG Max has "switching" but not "routing" — should still be Gateway
        assert_eq!(
            infer_device_type_integration(&["switching".into()], "UCG-Max"),
            DeviceType::Gateway
        );
        // UDM with both features
        assert_eq!(
            infer_device_type_integration(&["switching".into(), "routing".into()], "UDM-Pro"),
            DeviceType::Gateway
        );
    }

    #[test]
    fn device_state_mapping() {
        assert_eq!(map_device_state(0), DeviceState::Offline);
        assert_eq!(map_device_state(1), DeviceState::Online);
        assert_eq!(map_device_state(2), DeviceState::PendingAdoption);
        assert_eq!(map_device_state(4), DeviceState::Updating);
        assert_eq!(map_device_state(5), DeviceState::GettingReady);
        assert_eq!(map_device_state(99), DeviceState::Unknown);
    }

    #[test]
    fn legacy_site_uses_desc_as_display_name() {
        let site = LegacySite {
            id: "abc123".into(),
            name: "default".into(),
            desc: Some("Main Office".into()),
            role: None,
            extra: serde_json::Map::new(),
        };
        let converted: Site = site.into();
        assert_eq!(converted.internal_name, "default");
        assert_eq!(converted.name, "Main Office");
    }

    #[test]
    fn legacy_site_falls_back_to_name_when_desc_empty() {
        let site = LegacySite {
            id: "abc123".into(),
            name: "branch-1".into(),
            desc: Some(String::new()),
            role: None,
            extra: serde_json::Map::new(),
        };
        let converted: Site = site.into();
        assert_eq!(converted.name, "branch-1");
    }

    #[test]
    fn event_category_mapping() {
        assert_eq!(
            map_event_category(Some(&"wlan".into())),
            EventCategory::Network
        );
        assert_eq!(
            map_event_category(Some(&"device".into())),
            EventCategory::Device
        );
        assert_eq!(
            map_event_category(Some(&"admin".into())),
            EventCategory::Admin
        );
        assert_eq!(map_event_category(None), EventCategory::Unknown);
    }

    #[test]
    fn channel_frequency_bands() {
        assert_eq!(channel_to_frequency(Some(6)), Some(2.4));
        assert_eq!(channel_to_frequency(Some(36)), Some(5.0));
        assert_eq!(channel_to_frequency(Some(149)), Some(5.0));
        assert_eq!(channel_to_frequency(None), None);
    }
}
