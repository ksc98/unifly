//! Event command handlers.

use std::sync::Arc;

use chrono::Utc;
use tabled::Tabled;
use unifi_core::{Controller, Event};

use crate::cli::{EventsArgs, EventsCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct EventRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Time")]
    time: String,
    #[tabled(rename = "Category")]
    category: String,
    #[tabled(rename = "Message")]
    message: String,
}

impl From<&Arc<Event>> for EventRow {
    fn from(e: &Arc<Event>) -> Self {
        Self {
            id: e.id.as_ref().map(|id| id.to_string()).unwrap_or_default(),
            time: e.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            category: format!("{:?}", e.category),
            message: e.message.clone(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: EventsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        EventsCommand::List { limit, within } => {
            let snap = controller.events_snapshot();
            let cutoff = Utc::now() - chrono::TimeDelta::hours(within as i64);
            let filtered: Vec<_> = snap.iter()
                .filter(|e| e.timestamp >= cutoff)
                .take(limit as usize)
                .cloned()
                .collect();
            let out = output::render_list(
                &global.output,
                &filtered,
                |e| EventRow::from(e),
                |e| e.id.as_ref().map(|id| id.to_string()).unwrap_or_default(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        EventsCommand::Watch { .. } => util::not_yet_implemented("event streaming"),
    }
}
