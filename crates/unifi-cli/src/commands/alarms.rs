//! Alarm command handlers.

use tabled::Tabled;
use unifi_core::{Alarm, Command as CoreCommand, Controller, EntityId};

use crate::cli::{AlarmsArgs, AlarmsCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct AlarmRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Time")]
    time: String,
    #[tabled(rename = "Severity")]
    severity: String,
    #[tabled(rename = "Category")]
    category: String,
    #[tabled(rename = "Message")]
    message: String,
    #[tabled(rename = "Archived")]
    archived: String,
}

impl From<&Alarm> for AlarmRow {
    fn from(a: &Alarm) -> Self {
        Self {
            id: a.id.to_string(),
            time: a.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            severity: format!("{:?}", a.severity),
            category: format!("{:?}", a.category),
            message: a.message.clone(),
            archived: if a.archived { "yes" } else { "no" }.into(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: AlarmsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        AlarmsCommand::List { unarchived: _, limit: _ } => {
            let alarms = controller.list_alarms().await?;
            let out = output::render_list(
                &global.output,
                &alarms,
                |a| AlarmRow::from(a),
                |a| a.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

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
