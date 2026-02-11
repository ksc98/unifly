//! WAN interface command handlers.

use unifi_core::Controller;

use crate::cli::{GlobalOpts, WansArgs, WansCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: WansArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        WansCommand::List(_) => util::not_yet_implemented("WAN interface listing"),
    }
}
