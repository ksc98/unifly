//! Network command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::{Command as CoreCommand, Controller, EntityId, Network};

use crate::cli::{GlobalOpts, NetworkManagement, NetworksArgs, NetworksCommand};
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
            management: n
                .management
                .map(|m| format!("{m:?}"))
                .unwrap_or_default(),
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
        format!("Management: {}", n.management.map(|m: unifi_core::model::NetworkManagement| format!("{m:?}")).unwrap_or_else(|| "-".into())),
        format!("VLAN:       {}", n.vlan_id.map(|v: u16| v.to_string()).unwrap_or_else(|| "-".into())),
        format!("Subnet:     {}", n.subnet.as_deref().unwrap_or("-")),
        format!("Gateway:    {}", n.gateway_ip.map(|ip: std::net::Ipv4Addr| ip.to_string()).unwrap_or_else(|| "-".into())),
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
                    let out = output::render_single(&global.output, n, detail, |n| n.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "network".into(),
                        identifier: id,
                        list_command: "networks list".into(),
                    })
                }
            }
            Ok(())
        }

        NetworksCommand::Create {
            from_file,
            name,
            management,
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
            let data = if let Some(ref path) = from_file {
                util::read_json_file(path)?
            } else {
                let mut map = serde_json::Map::new();
                if let Some(name) = name {
                    map.insert("name".into(), serde_json::json!(name));
                }
                if let Some(mgmt) = management {
                    let val = match mgmt {
                        NetworkManagement::Gateway => "GATEWAY",
                        NetworkManagement::Switch => "SWITCH",
                        NetworkManagement::Unmanaged => "UNMANAGED",
                    };
                    map.insert("management".into(), serde_json::json!(val));
                }
                if let Some(vlan) = vlan {
                    map.insert("vlan".into(), serde_json::json!(vlan));
                }
                map.insert("enabled".into(), serde_json::json!(enabled));
                if let Some(host) = ipv4_host {
                    map.insert("ipv4_host_address".into(), serde_json::json!(host));
                }
                if dhcp {
                    map.insert("dhcp_enabled".into(), serde_json::json!(true));
                }
                if let Some(start) = dhcp_start {
                    map.insert("dhcp_start".into(), serde_json::json!(start));
                }
                if let Some(stop) = dhcp_stop {
                    map.insert("dhcp_stop".into(), serde_json::json!(stop));
                }
                if let Some(lease) = dhcp_lease {
                    map.insert("dhcp_lease".into(), serde_json::json!(lease));
                }
                if let Some(zone) = zone {
                    map.insert("zone_id".into(), serde_json::json!(zone));
                }
                if isolated {
                    map.insert("isolation_enabled".into(), serde_json::json!(true));
                }
                map.insert("internet_access_enabled".into(), serde_json::json!(internet));
                serde_json::Value::Object(map)
            };

            controller
                .execute(CoreCommand::CreateNetwork { data })
                .await?;
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
            let data = if let Some(ref path) = from_file {
                util::read_json_file(path)?
            } else {
                let mut map = serde_json::Map::new();
                if let Some(name) = name {
                    map.insert("name".into(), serde_json::json!(name));
                }
                if let Some(enabled) = enabled {
                    map.insert("enabled".into(), serde_json::json!(enabled));
                }
                if let Some(vlan) = vlan {
                    map.insert("vlan".into(), serde_json::json!(vlan));
                }
                serde_json::Value::Object(map)
            };

            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateNetwork { id: eid, data })
                .await?;
            if !global.quiet {
                eprintln!("Network updated");
            }
            Ok(())
        }

        NetworksCommand::Delete { id, force: _ } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete network {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteNetwork { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Network deleted");
            }
            Ok(())
        }

        NetworksCommand::Refs { id: _ } => {
            util::legacy_stub("Network cross-references")
        }
    }
}
