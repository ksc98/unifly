//! ACL rule command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::model::AclRule;
use unifi_core::{Command as CoreCommand, Controller, EntityId};

use crate::cli::{AclArgs, AclCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct AclRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    rule_type: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
}

impl From<&Arc<AclRule>> for AclRow {
    fn from(r: &Arc<AclRule>) -> Self {
        Self {
            id: r.id.to_string(),
            name: r.name.clone(),
            rule_type: format!("{:?}", r.rule_type),
            action: format!("{:?}", r.action),
            enabled: if r.enabled { "yes" } else { "no" }.into(),
        }
    }
}

fn detail(r: &Arc<AclRule>) -> String {
    [
        format!("ID:      {}", r.id),
        format!("Name:    {}", r.name),
        format!("Enabled: {}", r.enabled),
        format!("Type:    {:?}", r.rule_type),
        format!("Action:  {:?}", r.action),
        format!("Source:  {}", r.source_summary.as_deref().unwrap_or("-")),
        format!("Dest:    {}", r.destination_summary.as_deref().unwrap_or("-")),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: AclArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        AclCommand::List(_list) => {
            let snap = controller.acl_rules_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |r| AclRow::from(r),
                |r| r.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        AclCommand::Get { id } => {
            let snap = controller.acl_rules_snapshot();
            let found = snap.iter().find(|r| r.id.to_string() == id);
            match found {
                Some(r) => {
                    let out = output::render_single(&global.output, r, detail, |r| r.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "ACL rule".into(),
                        identifier: id,
                        list_command: "acl list".into(),
                    })
                }
            }
            Ok(())
        }

        AclCommand::Create {
            from_file,
            name,
            rule_type: _,
            action,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                unifi_core::command::CreateAclRuleRequest {
                    name: name.unwrap_or_default(),
                    action: match action {
                        Some(crate::cli::AclAction::Allow) => unifi_core::model::FirewallAction::Allow,
                        Some(crate::cli::AclAction::Block) | None => unifi_core::model::FirewallAction::Block,
                    },
                    source_zone_id: EntityId::from(""),
                    destination_zone_id: EntityId::from(""),
                    protocol: None,
                    source_port: None,
                    destination_port: None,
                    enabled: true,
                }
            };
            controller.execute(CoreCommand::CreateAclRule(req)).await?;
            if !global.quiet {
                eprintln!("ACL rule created");
            }
            Ok(())
        }

        AclCommand::Update { id, from_file } => {
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                unifi_core::command::UpdateAclRuleRequest::default()
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateAclRule { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("ACL rule updated");
            }
            Ok(())
        }

        AclCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete ACL rule {id}?"), global.yes)? {
                return Ok(());
            }
            controller.execute(CoreCommand::DeleteAclRule { id: eid }).await?;
            if !global.quiet {
                eprintln!("ACL rule deleted");
            }
            Ok(())
        }

        AclCommand::Reorder { get, set } => {
            if let Some(ids) = set {
                let ordered_ids: Vec<EntityId> = ids.into_iter().map(EntityId::from).collect();
                controller
                    .execute(CoreCommand::ReorderAclRules { ordered_ids })
                    .await?;
                if !global.quiet {
                    eprintln!("ACL rule order updated");
                }
            } else {
                let _ = get;
                let snap = controller.acl_rules_snapshot();
                let ids: Vec<String> = snap.iter().map(|r| r.id.to_string()).collect();
                let out = match &global.output {
                    crate::cli::OutputFormat::Json | crate::cli::OutputFormat::JsonCompact => {
                        serde_json::to_string_pretty(&ids).unwrap_or_default()
                    }
                    _ => ids.join("\n"),
                };
                output::print_output(&out, global.quiet);
            }
            Ok(())
        }
    }
}
