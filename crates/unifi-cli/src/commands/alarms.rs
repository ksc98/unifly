//! Alarm command handlers.

use unifi_core::{Command as CoreCommand, Controller, EntityId};

use crate::cli::{AlarmsArgs, AlarmsCommand, GlobalOpts};
use crate::error::CliError;

use super::util;

pub async fn handle(
    controller: &Controller,
    args: AlarmsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        AlarmsCommand::List { .. } => util::not_yet_implemented("alarm listing"),

        AlarmsCommand::Archive { id } => {
            let eid = EntityId::from(id);
            controller
                .execute(CoreCommand::ArchiveAlarm { id: eid })
                .await?;
            if !global.quiet {
                eprintln!("Alarm archived");
            }
            Ok(())
        }

        AlarmsCommand::ArchiveAll => {
            if !util::confirm("Archive all alarms?", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::ArchiveAllAlarms)
                .await?;
            if !global.quiet {
                eprintln!("All alarms archived");
            }
            Ok(())
        }
    }
}
