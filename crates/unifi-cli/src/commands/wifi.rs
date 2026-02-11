//! WiFi broadcast command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::model::WifiBroadcast;
use unifi_core::{Command as CoreCommand, Controller, EntityId};

use crate::cli::{GlobalOpts, WifiArgs, WifiCommand};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct WifiRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "SSID")]
    name: String,
    #[tabled(rename = "Type")]
    btype: String,
    #[tabled(rename = "Security")]
    security: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Bands")]
    bands: String,
}

impl From<&Arc<WifiBroadcast>> for WifiRow {
    fn from(w: &Arc<WifiBroadcast>) -> Self {
        Self {
            id: w.id.to_string(),
            name: w.name.clone(),
            btype: format!("{:?}", w.broadcast_type),
            security: format!("{:?}", w.security),
            enabled: if w.enabled { "yes" } else { "no" }.into(),
            bands: w
                .frequencies_ghz
                .iter()
                .map(|f| format!("{f}GHz"))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

fn detail(w: &Arc<WifiBroadcast>) -> String {
    vec![
        format!("ID:         {}", w.id),
        format!("SSID:       {}", w.name),
        format!("Enabled:    {}", w.enabled),
        format!("Type:       {:?}", w.broadcast_type),
        format!("Security:   {:?}", w.security),
        format!("Hidden:     {}", w.hidden),
        format!("Fast Roam:  {}", w.fast_roaming),
        format!("Band Steer: {}", w.band_steering),
        format!("MLO:        {}", w.mlo_enabled),
        format!("Hotspot:    {}", w.hotspot_enabled),
        format!("Network:    {}", w.network_id.as_ref().map(ToString::to_string).unwrap_or_else(|| "-".into())),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: WifiArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        WifiCommand::List(_list) => {
            let snap = controller.wifi_broadcasts_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |w| WifiRow::from(w),
                |w| w.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        WifiCommand::Get { id } => {
            let snap = controller.wifi_broadcasts_snapshot();
            let found = snap.iter().find(|w| w.id.to_string() == id);
            match found {
                Some(w) => {
                    let out = output::render_single(&global.output, w, detail, |w| w.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "wifi".into(),
                        identifier: id,
                        list_command: "wifi list".into(),
                    })
                }
            }
            Ok(())
        }

        WifiCommand::Create {
            from_file,
            name,
            broadcast_type,
            network,
            security,
            passphrase,
            frequencies,
            hidden,
            band_steering,
            fast_roaming,
        } => {
            let data = if let Some(ref path) = from_file {
                util::read_json_file(path)?
            } else {
                let mut map = serde_json::Map::new();
                if let Some(name) = name {
                    map.insert("name".into(), serde_json::json!(name));
                }
                map.insert("broadcast_type".into(), serde_json::json!(format!("{broadcast_type:?}")));
                if let Some(network) = network {
                    map.insert("network_id".into(), serde_json::json!(network));
                }
                map.insert("security".into(), serde_json::json!(format!("{security:?}")));
                if let Some(passphrase) = passphrase {
                    map.insert("passphrase".into(), serde_json::json!(passphrase));
                }
                if let Some(freqs) = frequencies {
                    map.insert("frequencies".into(), serde_json::json!(freqs));
                }
                if hidden {
                    map.insert("hidden".into(), serde_json::json!(true));
                }
                if band_steering {
                    map.insert("band_steering".into(), serde_json::json!(true));
                }
                if fast_roaming {
                    map.insert("fast_roaming".into(), serde_json::json!(true));
                }
                serde_json::Value::Object(map)
            };

            controller
                .execute(CoreCommand::CreateWifi { data })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast created");
            }
            Ok(())
        }

        WifiCommand::Update {
            id,
            from_file,
            name,
            passphrase,
            enabled,
        } => {
            let data = if let Some(ref path) = from_file {
                util::read_json_file(path)?
            } else {
                let mut map = serde_json::Map::new();
                if let Some(name) = name {
                    map.insert("name".into(), serde_json::json!(name));
                }
                if let Some(passphrase) = passphrase {
                    map.insert("passphrase".into(), serde_json::json!(passphrase));
                }
                if let Some(enabled) = enabled {
                    map.insert("enabled".into(), serde_json::json!(enabled));
                }
                serde_json::Value::Object(map)
            };

            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateWifi { id: eid, data })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast updated");
            }
            Ok(())
        }

        WifiCommand::Delete { id, force: _ } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete WiFi broadcast {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteWifi { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast deleted");
            }
            Ok(())
        }
    }
}
