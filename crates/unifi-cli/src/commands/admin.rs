//! Admin command handlers.

use unifi_core::{Command as CoreCommand, Controller, EntityId};

use crate::cli::{AdminArgs, AdminCommand, GlobalOpts};
use crate::error::CliError;

use super::util;

pub async fn handle(
    controller: &Controller,
    args: AdminArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        AdminCommand::List => util::not_yet_implemented("admin listing"),

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
