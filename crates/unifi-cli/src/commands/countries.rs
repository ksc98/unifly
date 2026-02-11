//! Country code command handler.

use unifi_core::Controller;

use crate::cli::GlobalOpts;
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    util::not_yet_implemented("country code listing")
}
