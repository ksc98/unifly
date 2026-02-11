//! Event command handlers.

use unifi_core::Controller;

use crate::cli::{EventsArgs, EventsCommand, GlobalOpts};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: EventsArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        EventsCommand::List { .. } => util::legacy_stub("Event listing"),
        EventsCommand::Watch { .. } => util::legacy_stub("Event streaming"),
    }
}
