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
        StatsCommand::Site(_) => util::legacy_stub("Site statistics"),
        StatsCommand::Device(_) => util::legacy_stub("Device statistics"),
        StatsCommand::Client(_) => util::legacy_stub("Client statistics"),
        StatsCommand::Gateway(_) => util::legacy_stub("Gateway statistics"),
        StatsCommand::Dpi { .. } => util::legacy_stub("DPI statistics"),
    }
}
