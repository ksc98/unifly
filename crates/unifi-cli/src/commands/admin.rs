//! Admin command handlers.

use tabled::Tabled;
use unifi_core::{Admin, Command as CoreCommand, Controller, EntityId};

use crate::cli::{AdminArgs, AdminCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct AdminRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Email")]
    email: String,
    #[tabled(rename = "Role")]
    role: String,
    #[tabled(rename = "Super")]
    is_super: String,
}

impl From<&Admin> for AdminRow {
    fn from(a: &Admin) -> Self {
        Self {
            id: a.id.to_string(),
            name: a.name.clone(),
            email: a.email.clone().unwrap_or_default(),
            role: a.role.clone(),
            is_super: if a.is_super { "yes" } else { "no" }.into(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: AdminArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        AdminCommand::List => {
            let admins = controller.list_admins().await?;
            let out = output::render_list(
                &global.output,
                &admins,
                |a| AdminRow::from(a),
                |a| a.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        AdminCommand::Invite { name, email, role } => {
            controller
                .execute(CoreCommand::InviteAdmin { name, email, role })
                .await?;
            if !global.quiet {
                eprintln!("Admin invitation sent");
            }
            Ok(())
        }

        AdminCommand::Revoke { admin } => {
            let id = EntityId::from(admin.clone());
            if !util::confirm(&format!("Revoke admin access for {admin}?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::RevokeAdmin { id })
                .await?;
            if !global.quiet {
                eprintln!("Admin access revoked");
            }
            Ok(())
        }

        AdminCommand::Update { admin, role } => {
            let id = EntityId::from(admin);
            controller
                .execute(CoreCommand::UpdateAdmin {
                    id,
                    role: Some(role),
                })
                .await?;
            if !global.quiet {
                eprintln!("Admin role updated");
            }
            Ok(())
        }
    }
}
