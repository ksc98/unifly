//! Firewall command handlers (policies + zones).

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::model::{FirewallAction as ModelFirewallAction, FirewallPolicy, FirewallZone};
use unifi_core::{
    Command as CoreCommand, Controller, CreateFirewallPolicyRequest, CreateFirewallZoneRequest,
    EntityId, UpdateFirewallPolicyRequest, UpdateFirewallZoneRequest,
};

use crate::cli::{
    FirewallAction, FirewallArgs, FirewallCommand, FirewallPoliciesCommand, FirewallZonesCommand,
    GlobalOpts,
};
use crate::error::CliError;
use crate::output;

use super::util;

fn map_fw_action(a: FirewallAction) -> ModelFirewallAction {
    match a {
        FirewallAction::Allow => ModelFirewallAction::Allow,
        FirewallAction::Block => ModelFirewallAction::Block,
        FirewallAction::Reject => ModelFirewallAction::Reject,
    }
}

// ── Policy table row ────────────────────────────────────────────────

#[derive(Tabled)]
struct PolicyRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
    #[tabled(rename = "Src Zone")]
    src_zone: String,
    #[tabled(rename = "Dst Zone")]
    dst_zone: String,
}

impl From<&Arc<FirewallPolicy>> for PolicyRow {
    fn from(p: &Arc<FirewallPolicy>) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name.clone(),
            action: format!("{:?}", p.action),
            enabled: if p.enabled { "yes" } else { "no" }.into(),
            src_zone: p
                .source_zone_id
                .as_ref()
                .map(|z| z.to_string())
                .unwrap_or_default(),
            dst_zone: p
                .destination_zone_id
                .as_ref()
                .map(|z| z.to_string())
                .unwrap_or_default(),
        }
    }
}

fn policy_detail(p: &Arc<FirewallPolicy>) -> String {
    [
        format!("ID:          {}", p.id),
        format!("Name:        {}", p.name),
        format!("Description: {}", p.description.as_deref().unwrap_or("-")),
        format!("Enabled:     {}", p.enabled),
        format!("Action:      {:?}", p.action),
        format!("IP Version:  {:?}", p.ip_version),
        format!(
            "Src Zone:    {}",
            p.source_zone_id
                .as_ref()
                .map(|z| z.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        format!(
            "Dst Zone:    {}",
            p.destination_zone_id
                .as_ref()
                .map(|z| z.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        format!("Logging:     {}", p.logging_enabled),
    ]
    .join("\n")
}

// ── Zone table row ──────────────────────────────────────────────────

#[derive(Tabled)]
struct ZoneRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Networks")]
    network_count: usize,
}

impl From<&Arc<FirewallZone>> for ZoneRow {
    fn from(z: &Arc<FirewallZone>) -> Self {
        Self {
            id: z.id.to_string(),
            name: z.name.clone(),
            network_count: z.network_ids.len(),
        }
    }
}

fn zone_detail(z: &Arc<FirewallZone>) -> String {
    let nets = z
        .network_ids
        .iter()
        .map(|id| format!("  - {id}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "ID:       {}\nName:     {}\nNetworks:\n{}",
        z.id, z.name, nets
    )
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: FirewallArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        FirewallCommand::Policies(pargs) => {
            handle_policies(controller, pargs.command, global).await
        }
        FirewallCommand::Zones(zargs) => handle_zones(controller, zargs.command, global).await,
    }
}

async fn handle_policies(
    controller: &Controller,
    cmd: FirewallPoliciesCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match cmd {
        FirewallPoliciesCommand::List(_list) => {
            let snap = controller.firewall_policies_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |p| PolicyRow::from(p),
                |p| p.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        FirewallPoliciesCommand::Get { id } => {
            let snap = controller.firewall_policies_snapshot();
            let found = snap.iter().find(|p| p.id.to_string() == id);
            match found {
                Some(p) => {
                    let out = output::render_single(&global.output, p, policy_detail, |p| {
                        p.id.to_string()
                    });
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "firewall policy".into(),
                        identifier: id,
                        list_command: "firewall policies list".into(),
                    });
                }
            }
            Ok(())
        }

        FirewallPoliciesCommand::Create {
            from_file,
            name,
            action,
            enabled,
            description,
            logging,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateFirewallPolicyRequest {
                    name: name.unwrap_or_default(),
                    action: action
                        .map(map_fw_action)
                        .unwrap_or(ModelFirewallAction::Block),
                    source_zone_id: EntityId::from(""),
                    destination_zone_id: EntityId::from(""),
                    enabled,
                    logging_enabled: logging,
                    description,
                    protocol: None,
                    source_address: None,
                    destination_address: None,
                    destination_port: None,
                }
            };

            controller
                .execute(CoreCommand::CreateFirewallPolicy(req))
                .await?;
            if !global.quiet {
                eprintln!("Firewall policy created");
            }
            Ok(())
        }

        FirewallPoliciesCommand::Update { id, from_file } => {
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateFirewallPolicyRequest::default()
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateFirewallPolicy { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("Firewall policy updated");
            }
            Ok(())
        }

        FirewallPoliciesCommand::Patch { id, enabled } => {
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::PatchFirewallPolicy { id: eid, enabled })
                .await?;
            if !global.quiet {
                let state = if enabled { "enabled" } else { "disabled" };
                eprintln!("Firewall policy {state}");
            }
            Ok(())
        }

        FirewallPoliciesCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete firewall policy {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteFirewallPolicy { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Firewall policy deleted");
            }
            Ok(())
        }

        FirewallPoliciesCommand::Reorder {
            source_zone,
            dest_zone,
            get,
            set,
        } => {
            if let Some(ids) = set {
                let zone_pair = (EntityId::from(source_zone), EntityId::from(dest_zone));
                let ordered_ids: Vec<EntityId> = ids.into_iter().map(EntityId::from).collect();
                controller
                    .execute(CoreCommand::ReorderFirewallPolicies {
                        zone_pair,
                        ordered_ids,
                    })
                    .await?;
                if !global.quiet {
                    eprintln!("Firewall policy order updated");
                }
            } else {
                // Default to --get behavior
                let _ = get;
                util::not_yet_implemented("firewall policy ordering query")?;
            }
            Ok(())
        }
    }
}

async fn handle_zones(
    controller: &Controller,
    cmd: FirewallZonesCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match cmd {
        FirewallZonesCommand::List(_list) => {
            let snap = controller.firewall_zones_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |z| ZoneRow::from(z),
                |z| z.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        FirewallZonesCommand::Get { id } => {
            let snap = controller.firewall_zones_snapshot();
            let found = snap.iter().find(|z| z.id.to_string() == id);
            match found {
                Some(z) => {
                    let out =
                        output::render_single(&global.output, z, zone_detail, |z| z.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "firewall zone".into(),
                        identifier: id,
                        list_command: "firewall zones list".into(),
                    });
                }
            }
            Ok(())
        }

        FirewallZonesCommand::Create { name, networks } => {
            let network_ids = networks
                .unwrap_or_default()
                .into_iter()
                .map(EntityId::from)
                .collect();
            let req = CreateFirewallZoneRequest {
                name,
                description: None,
                network_ids,
            };
            controller
                .execute(CoreCommand::CreateFirewallZone(req))
                .await?;
            if !global.quiet {
                eprintln!("Firewall zone created");
            }
            Ok(())
        }

        FirewallZonesCommand::Update { id, name, networks } => {
            let eid = EntityId::from(id);
            let update = UpdateFirewallZoneRequest {
                name,
                description: None,
                network_ids: networks.map(|ns| ns.into_iter().map(EntityId::from).collect()),
            };
            controller
                .execute(CoreCommand::UpdateFirewallZone { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("Firewall zone updated");
            }
            Ok(())
        }

        FirewallZonesCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete firewall zone {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteFirewallZone { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Firewall zone deleted");
            }
            Ok(())
        }
    }
}
