//! Site command handlers.

use unifi_core::{Command as CoreCommand, Controller};

use crate::cli::{GlobalOpts, SitesArgs, SitesCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    controller: &Controller,
    args: SitesArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        SitesCommand::List(_) => util::legacy_stub("Site listing"),

        SitesCommand::Create { name, description } => {
            controller
                .execute(CoreCommand::CreateSite { name, description })
                .await?;
            if !global.quiet {
                eprintln!("Site created");
            }
            Ok(())
        }

        SitesCommand::Delete { name } => {
            if !util::confirm(&format!("Delete site '{name}'? This is destructive."), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteSite { name })
                .await?;
            if !global.quiet {
                eprintln!("Site deleted");
            }
            Ok(())
        }
    }
}
