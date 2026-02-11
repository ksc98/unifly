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
            name: _,
            rule_type: _,
            action: _,
        } => {
            let _ = from_file;
            util::not_yet_implemented("ACL rule creation")
        }

        AclCommand::Update { id: _, from_file: _ } => {
            util::not_yet_implemented("ACL rule update")
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
                util::not_yet_implemented("ACL rule ordering query")?;
            }
            Ok(())
        }
    }
}
