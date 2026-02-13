//! DNS policy command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::model::{DnsPolicy, DnsPolicyType};
use unifi_core::{
    Command as CoreCommand, Controller, CreateDnsPolicyRequest, EntityId, UpdateDnsPolicyRequest,
};

use crate::cli::{DnsArgs, DnsCommand, DnsRecordType, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

fn map_dns_type(rt: DnsRecordType) -> DnsPolicyType {
    match rt {
        DnsRecordType::A => DnsPolicyType::ARecord,
        DnsRecordType::Aaaa => DnsPolicyType::AaaaRecord,
        DnsRecordType::Cname => DnsPolicyType::CnameRecord,
        DnsRecordType::Mx => DnsPolicyType::MxRecord,
        DnsRecordType::Txt => DnsPolicyType::TxtRecord,
        DnsRecordType::Srv => DnsPolicyType::SrvRecord,
        DnsRecordType::Forward => DnsPolicyType::ForwardDomain,
    }
}

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
    [
        format!("ID:     {}", d.id),
        format!("Type:   {:?}", d.policy_type),
        format!("Domain: {}", d.domain),
        format!("Value:  {}", d.value),
        format!(
            "TTL:    {}",
            d.ttl_seconds
                .map_or_else(|| "-".into(), |t: u32| t.to_string())
        ),
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
                    let out =
                        output::render_single(&global.output, d, detail, |d| d.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "DNS policy".into(),
                        identifier: id,
                        list_command: "dns list".into(),
                    });
                }
            }
            Ok(())
        }

        DnsCommand::Create {
            from_file,
            record_type,
            domain,
            value: _,
            ttl: _,
            priority: _,
        } => {
            let req = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                CreateDnsPolicyRequest {
                    name: domain.clone().unwrap_or_default(),
                    policy_type: record_type
                        .map_or(DnsPolicyType::ARecord, map_dns_type),
                    enabled: true,
                    domains: domain.map(|d| vec![d]),
                    upstream: None,
                }
            };

            controller
                .execute(CoreCommand::CreateDnsPolicy(req))
                .await?;
            if !global.quiet {
                eprintln!("DNS policy created");
            }
            Ok(())
        }

        DnsCommand::Update { id, from_file } => {
            let update = if let Some(ref path) = from_file {
                serde_json::from_value(util::read_json_file(path)?)?
            } else {
                UpdateDnsPolicyRequest::default()
            };
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::UpdateDnsPolicy { id: eid, update })
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
