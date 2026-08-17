//! VPN command handlers.

use tabled::Tabled;
use unifly_core::{Controller, VpnServer, VpnTunnel};

use crate::cli::{GlobalOpts, VpnArgs, VpnCommand};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table rows ──────────────────────────────────────────────────────

#[derive(Tabled)]
struct VpnServerRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    server_type: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
}

impl From<&VpnServer> for VpnServerRow {
    fn from(s: &VpnServer) -> Self {
        Self {
            id: s.id.to_string(),
            name: s.name.clone().unwrap_or_default(),
            server_type: s.server_type.clone(),
            enabled: s
                .enabled
                .map_or("-", |e| if e { "yes" } else { "no" })
                .into(),
        }
    }
}

#[derive(Tabled)]
struct VpnTunnelRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Type")]
    tunnel_type: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
}

impl From<&VpnTunnel> for VpnTunnelRow {
    fn from(t: &VpnTunnel) -> Self {
        Self {
            id: t.id.to_string(),
            name: t.name.clone().unwrap_or_default(),
            tunnel_type: t.tunnel_type.clone(),
            enabled: t
                .enabled
                .map_or("-", |e| if e { "yes" } else { "no" })
                .into(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: VpnArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        VpnCommand::Servers(list) => {
            let servers = util::apply_list_args(
                controller.list_vpn_servers().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &servers,
                |s| VpnServerRow::from(s),
                |s| s.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        VpnCommand::Tunnels(list) => {
            let tunnels = util::apply_list_args(
                controller.list_vpn_tunnels().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &tunnels,
                |t| VpnTunnelRow::from(t),
                |t| t.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
