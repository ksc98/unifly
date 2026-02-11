//! RADIUS profile command handlers.

use unifi_core::Controller;

use crate::cli::{GlobalOpts, RadiusArgs, RadiusCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: RadiusArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        RadiusCommand::Profiles(_) => util::not_yet_implemented("RADIUS profile listing"),
    }
}
