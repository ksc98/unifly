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
        VpnCommand::Servers(_) => util::not_yet_implemented("VPN server listing"),
        VpnCommand::Tunnels(_) => util::not_yet_implemented("VPN tunnel listing"),
    }
}
