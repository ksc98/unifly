//! Traffic matching list command handlers.

use unifi_core::Controller;

use crate::cli::{GlobalOpts, TrafficListsArgs, TrafficListsCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: TrafficListsArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        TrafficListsCommand::List(_) => util::legacy_stub("Traffic matching lists"),
        TrafficListsCommand::Get { id: _ } => util::legacy_stub("Traffic matching list details"),
        TrafficListsCommand::Create { .. } => util::legacy_stub("Traffic matching list creation"),
        TrafficListsCommand::Update { .. } => util::legacy_stub("Traffic matching list update"),
        TrafficListsCommand::Delete { id: _ } => util::legacy_stub("Traffic matching list deletion"),
    }
}
