//! Network command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::{
    Command as CoreCommand, Controller, CreateNetworkRequest, EntityId, Network,
    UpdateNetworkRequest,
};

use crate::cli::{GlobalOpts, NetworksArgs, NetworksCommand};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct NetworkRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "VLAN")]
    vlan: String,
    #[tabled(rename = "Management")]
    management: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Subnet")]
    subnet: String,
}

impl From<&Arc<Network>> for NetworkRow {
    fn from(n: &Arc<Network>) -> Self {
        Self {
            id: n.id.to_string(),
            name: n.name.clone(),
            vlan: n.vlan_id.map(|v| v.to_string()).unwrap_or_default(),
            management: n.management.map(|m| format!("{m:?}")).unwrap_or_default(),
            enabled: if n.enabled { "yes" } else { "no" }.into(),
            subnet: n.subnet.clone().unwrap_or_default(),
        }
    }
}

fn detail(n: &Arc<Network>) -> String {
    let mut lines = vec![
        format!("ID:         {}", n.id),
        format!("Name:       {}", n.name),
        format!("Enabled:    {}", n.enabled),
        format!(
            "Management: {}",
            n.management
                .map(|m: unifi_core::model::NetworkManagement| format!("{m:?}"))
                .unwrap_or_else(|| "-".into())
        ),
        format!(
            "VLAN:       {}",
            n.vlan_id
                .map(|v: u16| v.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        format!("Subnet:     {}", n.subnet.as_deref().unwrap_or("-")),
        format!(
            "Gateway:    {}",
            n.gateway_ip
                .map(|ip: std::net::Ipv4Addr| ip.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        format!("Isolated:   {}", n.isolation_enabled),
        format!("Internet:   {}", n.internet_access_enabled),
    ];
    if let Some(ref dhcp) = n.dhcp {
        lines.push(format!("DHCP:       {}", dhcp.enabled));
        if let Some(start) = dhcp.range_start {
            lines.push(format!("DHCP Start: {start}"));
        }
        if let Some(stop) = dhcp.range_stop {
            lines.push(format!("DHCP Stop:  {stop}"));
        }
    }
    lines.join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: NetworksArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        NetworksCommand::List(_list) => {
            let snap = controller.networks_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |n| NetworkRow::from(n),
                |n| n.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        NetworksCommand::Get { id } => {
            let snap = controller.networks_snapshot();
            let found = snap.iter().find(|n| n.id.to_string() == id);
            match found {
                Some(n) => {
                    let out =
                        output::render_single(&global.output, n, detail, |n| n.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "network".into(),
                        identifier: id,
                        list_command: "networks list".into(),
                    });
                }
            }
            Ok(())
        }

        NetworksCommand::Create {
            from_file,
            name,
            management: _, // Integration API uses different type system
            vlan,
            enabled,
            ipv4_host,
            dhcp,
            dhcp_start,
            dhcp_stop,
            dhcp_lease,
            zone,
            isolated,
            internet,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateNetworkRequest {
                    name: name.unwrap_or_default(),
                    vlan_id: vlan,
                    subnet: ipv4_host,
                    purpose: None,
                    dhcp_enabled: dhcp,
                    enabled,
                    dhcp_range_start: dhcp_start,
                    dhcp_range_stop: dhcp_stop,
                    dhcp_lease_time: dhcp_lease,
                    firewall_zone_id: zone,
                    isolation_enabled: isolated,
                    internet_access_enabled: internet,
                }
            };

            controller.execute(CoreCommand::CreateNetwork(req)).await?;
            if !global.quiet {
                eprintln!("Network created");
            }
            Ok(())
        }

        NetworksCommand::Update {
            id,
            from_file,
            name,
            enabled,
            vlan,
        } => {
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateNetworkRequest {
                    name,
                    vlan_id: vlan,
                    subnet: None,
                    dhcp_enabled: None,
                    enabled,
                }
            };

            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateNetwork { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("Network updated");
            }
            Ok(())
        }

        NetworksCommand::Delete { id, force } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete network {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteNetwork { id: eid, force })
                .await?;
            if !global.quiet {
                eprintln!("Network deleted");
            }
            Ok(())
        }

        NetworksCommand::Refs { id: _ } => util::not_yet_implemented("network cross-references"),
    }
}
