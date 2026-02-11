// ── API-to-domain type conversions ──
//
// Bridges raw `unifi_api` response types into canonical `unifi_core::model`
// domain types. Each `From` impl normalizes field names, parses strings into
// strong types, and fills sensible defaults for missing optional data.

use std::net::IpAddr;

use chrono::{DateTime, Utc};

use unifi_api::integration_types;
use unifi_api::legacy::models::{
    LegacyAlarm, LegacyClientEntry, LegacyDevice, LegacyEvent, LegacySite,
};
use unifi_api::websocket::UnifiEvent;

use crate::model::{
    client::{Client, ClientType, GuestAuth, WirelessInfo},
    common::{DataSource, EntityOrigin},
    device::{Device, DeviceState, DeviceStats, DeviceType},
    dns::{DnsPolicy, DnsPolicyType},
    entity_id::{EntityId, MacAddress},
    event::{Alarm, Event, EventCategory, EventSeverity},
    firewall::{AclAction, AclRule, AclRuleType, FirewallAction, FirewallPolicy, FirewallZone},
    hotspot::Voucher,
    network::Network,
    site::Site,
    supporting::TrafficMatchingList,
    wifi::{WifiBroadcast, WifiBroadcastType, WifiSecurityMode},
};

// ── Helpers ────────────────────────────────────────────────────────

/// Parse an optional string to an `IpAddr`, silently dropping unparseable values.
fn parse_ip(raw: &Option<String>) -> Option<IpAddr> {
    raw.as_deref().and_then(|s| s.parse().ok())
}

/// Convert an optional epoch-seconds timestamp to `DateTime<Utc>`.
fn epoch_to_datetime(epoch: Option<i64>) -> Option<DateTime<Utc>> {
    epoch.and_then(|ts| DateTime::from_timestamp(ts, 0))
}

/// Parse an ISO-8601 datetime string (as returned by the legacy event/alarm endpoints).
fn parse_datetime(raw: &Option<String>) -> Option<DateTime<Utc>> {
    raw.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

// ── Device ─────────────────────────────────────────────────────────

/// Infer `DeviceType` from the legacy `type` field and optional `model` string.
///
/// The legacy API `type` field is typically: `"uap"`, `"usw"`, `"ugw"`, `"udm"`.
/// We also check the `model` prefix for newer hardware that may not match cleanly.
fn infer_device_type(device_type: &str, model: &Option<String>) -> DeviceType {
    match device_type {
        "uap" => DeviceType::AccessPoint,
        "usw" => DeviceType::Switch,
        "ugw" | "udm" => DeviceType::Gateway,
        _ => {
            // Fallback: check the model string prefix
            if let Some(m) = model.as_deref() {
                let upper = m.to_uppercase();
                if upper.starts_with("UAP") || upper.starts_with("U6") || upper.starts_with("U7") {
                    DeviceType::AccessPoint
                } else if upper.starts_with("USW") || upper.starts_with("USL") {
                    DeviceType::Switch
                } else if upper.starts_with("UGW")
                    || upper.starts_with("UDM")
                    || upper.starts_with("UDR")
                    || upper.starts_with("UXG")
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
        let device_type = infer_device_type(&d.device_type, &d.model);
        let state = map_device_state(d.state);

        // Build stats from sys_stats + uptime
        let stats = {
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
                    (Some(used), Some(total)) if total > 0 => {
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
            ip: parse_ip(&d.ip),
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
            uplink_device_mac: None,
            has_switching: device_type == DeviceType::Switch || device_type == DeviceType::Gateway,
            has_access_point: device_type == DeviceType::AccessPoint,
            stats,
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
        let wireless = if !is_wired {
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
        } else {
            None
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
            ip: parse_ip(&c.ip),
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
            tx_bytes: c.tx_bytes.and_then(|b| b.try_into().ok()),
            rx_bytes: c.rx_bytes.and_then(|b| b.try_into().ok()),
            bandwidth: None,
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
        32..=68 => 5.0,
        96..=177 => 5.0,
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
fn map_event_category(subsystem: &Option<String>) -> EventCategory {
    match subsystem.as_deref() {
        Some("wlan") | Some("lan") | Some("wan") => EventCategory::Network,
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
            timestamp: parse_datetime(&e.datetime).unwrap_or_else(Utc::now),
            category: map_event_category(&e.subsystem),
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
            timestamp: parse_datetime(&a.datetime).unwrap_or_else(Utc::now),
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
            timestamp: parse_datetime(&a.datetime).unwrap_or_else(Utc::now),
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
    } else if upper.contains("DISCONNECT")
        || upper.contains("LOST")
        || upper.contains("DOWN")
    {
        EventSeverity::Warning
    } else {
        EventSeverity::Info
    }
}

impl From<UnifiEvent> for Event {
    fn from(e: UnifiEvent) -> Self {
        let category = map_event_category(&Some(e.subsystem.clone()));
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
            timestamp: parse_datetime(&e.datetime).unwrap_or_else(Utc::now),
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

    if has("switching") && has("routing") {
        DeviceType::Gateway
    } else if has("accessPoint") {
        DeviceType::AccessPoint
    } else if has("switching") {
        DeviceType::Switch
    } else {
        // Fallback to model prefix
        infer_device_type("", &Some(model.to_owned()))
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

        // Extract MAC from access object if present
        let mac_str = c
            .access
            .get("macAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("");

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

impl From<integration_types::NetworkResponse> for Network {
    fn from(n: integration_types::NetworkResponse) -> Self {
        Network {
            id: EntityId::Uuid(n.id),
            name: n.name,
            enabled: n.enabled,
            management: None,
            purpose: None,
            is_default: n.default,
            vlan_id: Some(n.vlan_id as u16),
            subnet: None,
            gateway_ip: None,
            dhcp: None,
            ipv6_enabled: false,
            ipv6_mode: None,
            ipv6_prefix: None,
            dhcpv6_enabled: false,
            slaac_enabled: false,
            ntp_server: None,
            pxe_enabled: false,
            tftp_server: None,
            firewall_zone_id: None,
            isolation_enabled: false,
            internet_access_enabled: true,
            mdns_forwarding_enabled: false,
            cellular_backup_enabled: false,
            origin: map_origin(&n.management),
            source: DataSource::IntegrationApi,
        }
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
            .map(|mode| match mode {
                "OPEN" => WifiSecurityMode::Open,
                "WPA2_PERSONAL" => WifiSecurityMode::Wpa2Personal,
                "WPA3_PERSONAL" => WifiSecurityMode::Wpa3Personal,
                "WPA2_WPA3_PERSONAL" => WifiSecurityMode::Wpa2Wpa3Personal,
                "WPA2_ENTERPRISE" => WifiSecurityMode::Wpa2Enterprise,
                "WPA3_ENTERPRISE" => WifiSecurityMode::Wpa3Enterprise,
                "WPA2_WPA3_ENTERPRISE" => WifiSecurityMode::Wpa2Wpa3Enterprise,
                _ => WifiSecurityMode::Open,
            })
            .unwrap_or(WifiSecurityMode::Open);

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
        let action = p
            .action
            .get("type")
            .and_then(|v| v.as_str())
            .map(|a| match a {
                "ALLOW" => FirewallAction::Allow,
                "REJECT" => FirewallAction::Reject,
                _ => FirewallAction::Block,
            })
            .unwrap_or(FirewallAction::Block);

        let index = p
            .extra
            .get("index")
            .and_then(|v| v.as_i64())
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
            ttl_seconds: d
                .extra
                .get("ttl")
                .and_then(|v| v.as_u64())
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
        assert_eq!(infer_device_type("uap", &None), DeviceType::AccessPoint);
        assert_eq!(infer_device_type("usw", &None), DeviceType::Switch);
        assert_eq!(infer_device_type("ugw", &None), DeviceType::Gateway);
        assert_eq!(infer_device_type("udm", &None), DeviceType::Gateway);
    }

    #[test]
    fn device_type_from_model_fallback() {
        assert_eq!(
            infer_device_type("unknown", &Some("UAP-AC-Pro".into())),
            DeviceType::AccessPoint
        );
        assert_eq!(
            infer_device_type("unknown", &Some("U6-LR".into())),
            DeviceType::AccessPoint
        );
        assert_eq!(
            infer_device_type("unknown", &Some("USW-24-PoE".into())),
            DeviceType::Switch
        );
        assert_eq!(
            infer_device_type("unknown", &Some("UDM-Pro".into())),
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
            desc: Some("".into()),
            role: None,
            extra: serde_json::Map::new(),
        };
        let converted: Site = site.into();
        assert_eq!(converted.name, "branch-1");
    }

    #[test]
    fn event_category_mapping() {
        assert_eq!(
            map_event_category(&Some("wlan".into())),
            EventCategory::Network
        );
        assert_eq!(
            map_event_category(&Some("device".into())),
            EventCategory::Device
        );
        assert_eq!(
            map_event_category(&Some("admin".into())),
            EventCategory::Admin
        );
        assert_eq!(map_event_category(&None), EventCategory::Unknown);
    }

    #[test]
    fn channel_frequency_bands() {
        assert_eq!(channel_to_frequency(Some(6)), Some(2.4));
        assert_eq!(channel_to_frequency(Some(36)), Some(5.0));
        assert_eq!(channel_to_frequency(Some(149)), Some(5.0));
        assert_eq!(channel_to_frequency(None), None);
    }
}
