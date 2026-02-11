//! DPI reference data command handlers.

use unifi_core::Controller;

use crate::cli::{DpiArgs, DpiCommand, GlobalOpts};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: DpiArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        DpiCommand::Apps(_) => util::not_yet_implemented("DPI applications"),
        DpiCommand::Categories(_) => util::not_yet_implemented("DPI categories"),
    }
}
