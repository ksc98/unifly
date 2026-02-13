//! Client command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::{Client, Command as CoreCommand, Controller, EntityId, MacAddress};

use crate::cli::{ClientsArgs, ClientsCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct ClientRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "MAC")]
    mac: String,
    #[tabled(rename = "Type")]
    ctype: String,
    #[tabled(rename = "Uplink")]
    uplink: String,
}

impl From<&Arc<Client>> for ClientRow {
    fn from(c: &Arc<Client>) -> Self {
        Self {
            id: c.id.to_string(),
            name: c
                .name
                .clone()
                .or_else(|| c.hostname.clone())
                .unwrap_or_default(),
            ip: c.ip.map(|ip| ip.to_string()).unwrap_or_default(),
            mac: c.mac.to_string(),
            ctype: format!("{:?}", c.client_type),
            uplink: c
                .uplink_device_mac
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        }
    }
}

fn detail(c: &Arc<Client>) -> String {
    let mut lines = vec![
        format!("ID:        {}", c.id),
        format!("Name:      {}", c.name.as_deref().unwrap_or("-")),
        format!("Hostname:  {}", c.hostname.as_deref().unwrap_or("-")),
        format!("MAC:       {}", c.mac),
        format!(
            "IP:        {}",
            c.ip.map_or_else(|| "-".into(), |ip| ip.to_string())
        ),
        format!("Type:      {:?}", c.client_type),
        format!("Guest:     {}", c.is_guest),
        format!("Blocked:   {}", c.blocked),
    ];
    if let Some(ref w) = c.wireless {
        lines.push(format!("SSID:      {}", w.ssid.as_deref().unwrap_or("-")));
        if let Some(sig) = w.signal_dbm {
            lines.push(format!("Signal:    {sig} dBm"));
        }
    }
    if let Some(os) = &c.os_name {
        lines.push(format!("OS:        {os}"));
    }
    lines.join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: ClientsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        ClientsCommand::List(_list) => {
            let snap = controller.clients_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |c| ClientRow::from(c),
                |c| c.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ClientsCommand::Get { client } => {
            let snap = controller.clients_snapshot();
            let found = snap
                .iter()
                .find(|c| c.id.to_string() == client || c.mac.to_string() == client);
            match found {
                Some(c) => {
                    let out =
                        output::render_single(&global.output, c, detail, |c| c.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "client".into(),
                        identifier: client,
                        list_command: "clients list".into(),
                    });
                }
            }
            Ok(())
        }

        ClientsCommand::Authorize {
            client,
            minutes,
            data_limit_mb,
            rx_limit_kbps,
            tx_limit_kbps,
        } => {
            let client_id = EntityId::from(client);
            controller
                .execute(CoreCommand::AuthorizeGuest {
                    client_id,
                    time_limit_minutes: Some(minutes),
                    data_limit_mb,
                    rx_rate_kbps: rx_limit_kbps,
                    tx_rate_kbps: tx_limit_kbps,
                })
                .await?;
            if !global.quiet {
                eprintln!("Guest authorized for {minutes} minutes");
            }
            Ok(())
        }

        ClientsCommand::Unauthorize { client } => {
            let client_id = EntityId::from(client);
            controller
                .execute(CoreCommand::UnauthorizeGuest { client_id })
                .await?;
            if !global.quiet {
                eprintln!("Guest authorization revoked");
            }
            Ok(())
        }

        ClientsCommand::Block { mac } => {
            let mac = MacAddress::new(&mac);
            controller.execute(CoreCommand::BlockClient { mac }).await?;
            if !global.quiet {
                eprintln!("Client blocked");
            }
            Ok(())
        }

        ClientsCommand::Unblock { mac } => {
            let mac = MacAddress::new(&mac);
            controller
                .execute(CoreCommand::UnblockClient { mac })
                .await?;
            if !global.quiet {
                eprintln!("Client unblocked");
            }
            Ok(())
        }

        ClientsCommand::Kick { mac } => {
            let mac = MacAddress::new(&mac);
            controller.execute(CoreCommand::KickClient { mac }).await?;
            if !global.quiet {
                eprintln!("Client disconnected");
            }
            Ok(())
        }

        ClientsCommand::Forget { mac } => {
            let mac_addr = MacAddress::new(&mac);
            if !util::confirm(
                &format!("Forget client {mac}? This cannot be undone."),
                global.yes,
            )? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::ForgetClient { mac: mac_addr })
                .await?;
            if !global.quiet {
                eprintln!("Client forgotten");
            }
            Ok(())
        }
    }
}
