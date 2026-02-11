//! System command handlers.

use unifi_core::{Command as CoreCommand, Controller};

use crate::cli::{BackupCommand, GlobalOpts, SystemArgs, SystemCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    controller: &Controller,
    args: SystemArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        SystemCommand::Info => util::not_yet_implemented("system info"),
        SystemCommand::Health => util::not_yet_implemented("site health"),
        SystemCommand::Sysinfo => util::not_yet_implemented("controller sysinfo"),

        SystemCommand::Backup(backup_args) => {
            handle_backup(controller, backup_args.command, global).await
        }

        SystemCommand::Reboot => {
            if !util::confirm("Reboot controller hardware?", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::RebootController)
                .await?;
            if !global.quiet {
                eprintln!("Controller reboot initiated");
            }
            Ok(())
        }

        SystemCommand::Poweroff => {
            if !util::confirm("Power off controller hardware? This cannot be undone remotely.", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::PoweroffController)
                .await?;
            if !global.quiet {
                eprintln!("Controller power-off initiated");
            }
            Ok(())
        }
    }
}

async fn handle_backup(
    controller: &Controller,
    cmd: BackupCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match cmd {
        BackupCommand::Create => {
            controller
                .execute(CoreCommand::CreateBackup)
                .await?;
            if !global.quiet {
                eprintln!("Backup created");
            }
            Ok(())
        }

        BackupCommand::List => util::not_yet_implemented("backup listing"),

        BackupCommand::Download { .. } => util::not_yet_implemented("backup download"),

        BackupCommand::Delete { filename } => {
            if !util::confirm(&format!("Delete backup '{filename}'?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteBackup { filename })
                .await?;
            if !global.quiet {
                eprintln!("Backup deleted");
            }
            Ok(())
        }
    }
}
