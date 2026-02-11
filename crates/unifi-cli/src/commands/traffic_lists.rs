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
        TrafficListsCommand::List(_) => util::not_yet_implemented("traffic list listing"),
        TrafficListsCommand::Get { id: _ } => util::not_yet_implemented("traffic list details"),
        TrafficListsCommand::Create { .. } => util::not_yet_implemented("traffic list creation"),
        TrafficListsCommand::Update { .. } => util::not_yet_implemented("traffic list update"),
        TrafficListsCommand::Delete { id: _ } => util::not_yet_implemented("traffic list deletion"),
    }
}
