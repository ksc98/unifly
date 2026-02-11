//! RADIUS profile command handlers.

use tabled::Tabled;
use unifi_core::{Controller, RadiusProfile};

use crate::cli::{GlobalOpts, RadiusArgs, RadiusCommand};
use crate::error::CliError;
use crate::output;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct RadiusProfileRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
}

impl From<&RadiusProfile> for RadiusProfileRow {
    fn from(r: &RadiusProfile) -> Self {
        Self {
            id: r.id.to_string(),
            name: r.name.clone(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: RadiusArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        RadiusCommand::Profiles(_) => {
            let profiles = controller.list_radius_profiles().await?;
            let out = output::render_list(
                &global.output,
                &profiles,
                |r| RadiusProfileRow::from(r),
                |r| r.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
