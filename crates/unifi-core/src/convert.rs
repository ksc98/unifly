// ── API-to-domain type conversions ──
//
// Bridges raw `unifi_api` response types into canonical `unifi_core::model`
// domain types. Each `From` impl normalizes field names, parses strings into
// strong types, and fills sensible defaults for missing optional data.

use std::net::IpAddr;

use chrono::{DateTime, Utc};

use unifi_api::legacy::models::{
    LegacyAlarm, LegacyClientEntry, LegacyDevice, LegacyEvent, LegacySite,
};

use crate::model::{
    client::{Client, ClientType, GuestAuth, WirelessInfo},
    common::DataSource,
    device::{Device, DeviceState, DeviceStats, DeviceType},
    entity_id::{EntityId, MacAddress},
    event::{Event, EventCategory, EventSeverity},
    site::Site,
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
            let mut s = DeviceStats::default();
            s.uptime_secs = d.uptime.and_then(|u| u.try_into().ok());
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
