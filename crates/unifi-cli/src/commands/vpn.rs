//! VPN command handlers.

use unifi_core::Controller;

use crate::cli::{GlobalOpts, VpnArgs, VpnCommand};
use crate::error::CliError;

use super::util;

pub async fn handle(
    _controller: &Controller,
    args: VpnArgs,
    _global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        VpnCommand::Servers(_) => util::legacy_stub("VPN servers"),
        VpnCommand::Tunnels(_) => util::legacy_stub("VPN tunnels"),
    }
}
