//! Traffic matching list command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_core::{Command as CoreCommand, Controller, EntityId, TrafficMatchingList};

use crate::cli::{GlobalOpts, TrafficListsArgs, TrafficListsCommand};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct TrafficListRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    list_type: String,
    #[tabled(rename = "Items")]
    item_count: usize,
}

impl From<&Arc<TrafficMatchingList>> for TrafficListRow {
    fn from(t: &Arc<TrafficMatchingList>) -> Self {
        Self {
            id: t.id.to_string(),
            name: t.name.clone(),
            list_type: t.list_type.clone(),
            item_count: t.items.len(),
        }
    }
}

fn detail(t: &Arc<TrafficMatchingList>) -> String {
    let mut lines = vec![
        format!("ID:    {}", t.id),
        format!("Name:  {}", t.name),
        format!("Type:  {}", t.list_type),
        format!("Items: {}", t.items.len()),
    ];
    if let Some(ref origin) = t.origin {
        lines.push(format!("Origin: {origin:?}"));
    }
    if !t.items.is_empty() {
        lines.push(String::new());
        lines.push("Entries:".into());
        for item in &t.items {
            lines.push(format!("  - {item}"));
        }
    }
    lines.join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: TrafficListsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        TrafficListsCommand::List(_) => {
            let snap = controller.traffic_matching_lists_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |t| TrafficListRow::from(t),
                |t| t.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        TrafficListsCommand::Get { id } => {
            let snap = controller.traffic_matching_lists_snapshot();
            let found = snap.iter().find(|t| t.id.to_string() == id);
            match found {
                Some(t) => {
                    let out =
                        output::render_single(&global.output, t, detail, |t| t.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "traffic matching list".into(),
                        identifier: id,
                        list_command: "traffic-lists list".into(),
                    });
                }
            }
            Ok(())
        }

        TrafficListsCommand::Create {
            from_file,
            name,
            list_type: _,
            items,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                unifly_core::command::CreateTrafficMatchingListRequest {
                    name: name.unwrap_or_default(),
                    entries: items.unwrap_or_default(),
                    description: None,
                }
            };
            controller
                .execute(CoreCommand::CreateTrafficMatchingList(req))
                .await?;
            if !global.quiet {
                eprintln!("Traffic matching list created");
            }
            Ok(())
        }

        TrafficListsCommand::Update { id, from_file } => {
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                unifly_core::command::UpdateTrafficMatchingListRequest::default()
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateTrafficMatchingList { id: eid, update })
                .await?;
            if !global.quiet {
                eprintln!("Traffic matching list updated");
            }
            Ok(())
        }

        TrafficListsCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete traffic matching list {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteTrafficMatchingList { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Traffic matching list deleted");
            }
            Ok(())
        }
    }
}
