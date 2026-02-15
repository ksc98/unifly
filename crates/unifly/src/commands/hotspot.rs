//! Hotspot voucher command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifly_core::model::Voucher;
use unifly_core::{Command as CoreCommand, Controller, CreateVouchersRequest, EntityId};

use crate::cli::{GlobalOpts, HotspotArgs, HotspotCommand};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct VoucherRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Code")]
    code: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Minutes")]
    minutes: String,
    #[tabled(rename = "Expired")]
    expired: String,
}

impl From<&Arc<Voucher>> for VoucherRow {
    fn from(v: &Arc<Voucher>) -> Self {
        Self {
            id: v.id.to_string(),
            code: v.code.clone(),
            name: v.name.clone().unwrap_or_default(),
            minutes: v
                .time_limit_minutes
                .map(|m| m.to_string())
                .unwrap_or_default(),
            expired: if v.expired { "yes" } else { "no" }.into(),
        }
    }
}

fn detail(v: &Arc<Voucher>) -> String {
    [
        format!("ID:         {}", v.id),
        format!("Code:       {}", v.code),
        format!("Name:       {}", v.name.as_deref().unwrap_or("-")),
        format!("Expired:    {}", v.expired),
        format!(
            "Minutes:    {}",
            v.time_limit_minutes
                .map_or_else(|| "-".into(), |m: u32| m.to_string())
        ),
        format!(
            "Data Limit: {} MB",
            v.data_usage_limit_mb
                .map_or_else(|| "-".into(), |m: u64| m.to_string())
        ),
        format!(
            "Guests:     {}/{}",
            v.authorized_guest_count.unwrap_or(0),
            v.authorized_guest_limit
                .map_or_else(|| "unlimited".into(), |l: u32| l.to_string())
        ),
    ]
    .join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: HotspotArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        HotspotCommand::List { .. } => {
            let snap = controller.vouchers_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |v| VoucherRow::from(v),
                |v| v.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        HotspotCommand::Get { id } => {
            let snap = controller.vouchers_snapshot();
            let found = snap.iter().find(|v| v.id.to_string() == id);
            match found {
                Some(v) => {
                    let out =
                        output::render_single(&global.output, v, detail, |v| v.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "voucher".into(),
                        identifier: id,
                        list_command: "hotspot list".into(),
                    });
                }
            }
            Ok(())
        }

        HotspotCommand::Create {
            name,
            count,
            minutes,
            guest_limit,
            data_limit_mb,
            rx_limit_kbps,
            tx_limit_kbps,
        } => {
            let req = CreateVouchersRequest {
                count,
                name: Some(name),
                time_limit_minutes: Some(minutes),
                data_usage_limit_mb: data_limit_mb,
                rx_rate_limit_kbps: rx_limit_kbps,
                tx_rate_limit_kbps: tx_limit_kbps,
                authorized_guest_limit: guest_limit,
            };

            controller.execute(CoreCommand::CreateVouchers(req)).await?;
            if !global.quiet {
                eprintln!("{count} voucher(s) created");
            }
            Ok(())
        }

        HotspotCommand::Delete { id } => {
            let eid = EntityId::from(id.clone());
            if !util::confirm(&format!("Delete voucher {id}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteVoucher { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Voucher deleted");
            }
            Ok(())
        }

        HotspotCommand::Purge { filter } => {
            if !util::confirm("Purge vouchers matching filter?", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::PurgeVouchers { filter })
                .await?;
            if !global.quiet {
                eprintln!("Vouchers purged");
            }
            Ok(())
        }
    }
}
