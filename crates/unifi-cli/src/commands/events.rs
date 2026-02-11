//! Event command handlers.

use std::io::{self, Write};
use std::sync::Arc;

use chrono::Utc;
use tabled::Tabled;
use unifi_core::{Controller, Event};

use crate::cli::{EventsArgs, EventsCommand, GlobalOpts, OutputFormat};
use crate::error::CliError;
use crate::output;

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

        EventsCommand::Watch { types } => {
            watch_events(controller, &global.output, types.as_deref()).await
        }
    }
}

/// Stream live events from the controller's WebSocket broadcast channel.
///
/// Prints each event as it arrives; Ctrl+C terminates cleanly.
async fn watch_events(
    controller: &Controller,
    format: &OutputFormat,
    type_filter: Option<&[String]>,
) -> Result<(), CliError> {
    let mut rx = controller.events();
    let mut stdout = io::stdout().lock();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        // Apply type filter if specified
                        if let Some(types) = type_filter {
                            let cat = format!("{:?}", event.category);
                            if !types.iter().any(|t| cat.eq_ignore_ascii_case(t)) {
                                continue;
                            }
                        }

                        let line = match format {
                            OutputFormat::Json | OutputFormat::JsonCompact => {
                                serde_json::to_string(&*event)
                                    .unwrap_or_else(|_| format!("{:?}", event))
                            }
                            OutputFormat::Yaml => {
                                serde_yaml::to_string(&*event)
                                    .unwrap_or_else(|_| format!("{:?}", event))
                            }
                            _ => {
                                let time = event.timestamp.format("%H:%M:%S");
                                let cat = format!("{:?}", event.category);
                                format!("{time}  [{cat}]  {}", event.message)
                            }
                        };

                        if writeln!(stdout, "{line}").is_err() {
                            break; // Broken pipe
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("warning: skipped {n} events (too slow)");
                    }
                }
            }
        }
    }

    Ok(())
}
