//! WiFi broadcast command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::model::{WifiBroadcast, WifiSecurityMode};
use unifi_core::{
    Command as CoreCommand, Controller, CreateWifiBroadcastRequest, EntityId,
    UpdateWifiBroadcastRequest,
};

use crate::cli::{GlobalOpts, WifiArgs, WifiCommand, WifiSecurity};
use crate::error::CliError;
use crate::output;

use super::util;

fn map_security(s: WifiSecurity) -> WifiSecurityMode {
    match s {
        WifiSecurity::Open => WifiSecurityMode::Open,
        WifiSecurity::Wpa2Personal => WifiSecurityMode::Wpa2Personal,
        WifiSecurity::Wpa3Personal => WifiSecurityMode::Wpa3Personal,
        WifiSecurity::Wpa2Wpa3Personal => WifiSecurityMode::Wpa2Wpa3Personal,
        WifiSecurity::Wpa2Enterprise => WifiSecurityMode::Wpa2Enterprise,
        WifiSecurity::Wpa3Enterprise => WifiSecurityMode::Wpa3Enterprise,
        WifiSecurity::Wpa2Wpa3Enterprise => WifiSecurityMode::Wpa2Wpa3Enterprise,
    }
}

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
            broadcast_type: _,
            network,
            security,
            passphrase,
            frequencies: _,
            hidden,
            band_steering: _,
            fast_roaming: _,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateWifiBroadcastRequest {
                    name: name.clone().unwrap_or_default(),
                    ssid: name.unwrap_or_default(),
                    security_mode: map_security(security),
                    passphrase,
                    enabled: true,
                    network_id: network.map(EntityId::from),
                    hide_ssid: hidden,
                }
            };

            controller
                .execute(CoreCommand::CreateWifiBroadcast(req))
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
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateWifiBroadcastRequest {
                    name,
                    ssid: None,
                    security_mode: None,
                    passphrase,
                    enabled,
                    hide_ssid: None,
                }
            };

            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateWifiBroadcast { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast updated");
            }
            Ok(())
        }

        WifiCommand::Delete { id, force } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete WiFi broadcast {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteWifiBroadcast { id: eid, force })
                .await?;
            if !global.quiet {
                eprintln!("WiFi broadcast deleted");
            }
            Ok(())
        }
    }
}
