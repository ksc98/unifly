//! Event command handlers.

use std::sync::Arc;

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
        EventsCommand::List { limit, within: _ } => {
            let snap = controller.events_snapshot();
            let events = &snap[..snap.len().min(limit as usize)];
            let out = output::render_list(
                &global.output,
                events,
                |e| EventRow::from(e),
                |e| e.id.as_ref().map(|id| id.to_string()).unwrap_or_default(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        EventsCommand::Watch { .. } => util::not_yet_implemented("event streaming"),
    }
}
