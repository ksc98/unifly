//! WAN interface command handlers.

use tabled::Tabled;
use unifi_core::{Controller, WanInterface};

use crate::cli::{GlobalOpts, WansArgs, WansCommand};
use crate::error::CliError;
use crate::output;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct WanRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "Gateway")]
    gateway: String,
}

impl From<&WanInterface> for WanRow {
    fn from(w: &WanInterface) -> Self {
        Self {
            id: w.id.to_string(),
            name: w.name.clone().unwrap_or_default(),
            ip: w.ip.map(|ip| ip.to_string()).unwrap_or_default(),
            gateway: w.gateway.map(|gw| gw.to_string()).unwrap_or_default(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: WansArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        WansCommand::List(_) => {
            let wans = controller.list_wans().await?;
            let out = output::render_list(
                &global.output,
                &wans,
                |w| WanRow::from(w),
                |w| w.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
