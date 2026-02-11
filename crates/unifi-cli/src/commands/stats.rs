//! Statistics command handlers.

use unifi_core::Controller;

use crate::cli::{GlobalOpts, StatsArgs, StatsCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: StatsArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        StatsCommand::Site(_) => util::not_yet_implemented("site statistics"),
        StatsCommand::Device(_) => util::not_yet_implemented("device statistics"),
        StatsCommand::Client(_) => util::not_yet_implemented("client statistics"),
        StatsCommand::Gateway(_) => util::not_yet_implemented("gateway statistics"),
        StatsCommand::Dpi { .. } => util::not_yet_implemented("DPI statistics"),
    }
}
