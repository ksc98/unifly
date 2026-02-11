//! DNS policy command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::model::DnsPolicy;
use unifi_core::{Command as CoreCommand, Controller, EntityId};

use crate::cli::{DnsArgs, DnsCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct DnsRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Type")]
    record_type: String,
    #[tabled(rename = "Domain")]
    domain: String,
    #[tabled(rename = "Value")]
    value: String,
    #[tabled(rename = "TTL")]
    ttl: String,
}

impl From<&Arc<DnsPolicy>> for DnsRow {
    fn from(d: &Arc<DnsPolicy>) -> Self {
        Self {
            id: d.id.to_string(),
            record_type: format!("{:?}", d.policy_type),
            domain: d.domain.clone(),
            value: d.value.clone(),
            ttl: d.ttl_seconds.map(|t| t.to_string()).unwrap_or_default(),
        }
    }
}

fn detail(d: &Arc<DnsPolicy>) -> String {
    vec![
        format!("ID:     {}", d.id),
        format!("Type:   {:?}", d.policy_type),
        format!("Domain: {}", d.domain),
        format!("Value:  {}", d.value),
        format!("TTL:    {}", d.ttl_seconds.map(|t: u32| t.to_string()).unwrap_or_else(|| "-".into())),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: DnsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        DnsCommand::List(_list) => {
            let snap = controller.dns_policies_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |d| DnsRow::from(d),
                |d| d.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DnsCommand::Get { id } => {
            let snap = controller.dns_policies_snapshot();
            let found = snap.iter().find(|d| d.id.to_string() == id);
            match found {
                Some(d) => {
                    let out = output::render_single(&global.output, d, detail, |d| d.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "DNS policy".into(),
                        identifier: id,
                        list_command: "dns list".into(),
                    })
                }
            }
            Ok(())
        }

        DnsCommand::Create {
            from_file,
            record_type,
            domain,
            value,
            ttl,
            priority,
        } => {
            let data = if let Some(ref path) = from_file {
                util::read_json_file(path)?
            } else {
                let mut map = serde_json::Map::new();
                if let Some(rt) = record_type {
                    map.insert("record_type".into(), serde_json::json!(format!("{rt:?}")));
                }
                if let Some(domain) = domain {
                    map.insert("domain".into(), serde_json::json!(domain));
                }
                if let Some(value) = value {
                    map.insert("value".into(), serde_json::json!(value));
                }
                map.insert("ttl".into(), serde_json::json!(ttl));
                if let Some(priority) = priority {
                    map.insert("priority".into(), serde_json::json!(priority));
                }
                serde_json::Value::Object(map)
            };

            controller
                .execute(CoreCommand::CreateDnsPolicy { data })
                .await?;
            if !global.quiet {
                eprintln!("DNS policy created");
            }
            Ok(())
        }

        DnsCommand::Update { id, from_file } => {
            let data = if let Some(ref path) = from_file {
                util::read_json_file(path)?
            } else {
                serde_json::json!({})
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateDnsPolicy { id: eid, data })
                .await?;
            if !global.quiet {
                eprintln!("DNS policy updated");
            }
            Ok(())
        }

        DnsCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete DNS policy {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteDnsPolicy { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("DNS policy deleted");
            }
            Ok(())
        }
    }
}
