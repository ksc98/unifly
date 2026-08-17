//! DPI reference data command handlers.

use tabled::Tabled;
use unifly_core::{Controller, DpiApplication, DpiCategory};

use crate::cli::{DpiArgs, DpiCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table rows ──────────────────────────────────────────────────────

#[derive(Tabled)]
struct DpiAppRow {
    #[tabled(rename = "ID")]
    id: u32,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Category")]
    category_id: u32,
    #[tabled(rename = "TX Bytes")]
    tx_bytes: u64,
    #[tabled(rename = "RX Bytes")]
    rx_bytes: u64,
}

impl From<&DpiApplication> for DpiAppRow {
    fn from(a: &DpiApplication) -> Self {
        Self {
            id: a.id,
            name: a.name.clone(),
            category_id: a.category_id,
            tx_bytes: a.tx_bytes,
            rx_bytes: a.rx_bytes,
        }
    }
}

#[derive(Tabled)]
struct DpiCategoryRow {
    #[tabled(rename = "ID")]
    id: u32,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Apps")]
    app_count: usize,
    #[tabled(rename = "TX Bytes")]
    tx_bytes: u64,
    #[tabled(rename = "RX Bytes")]
    rx_bytes: u64,
}

impl From<&DpiCategory> for DpiCategoryRow {
    fn from(c: &DpiCategory) -> Self {
        Self {
            id: c.id,
            name: c.name.clone(),
            app_count: c.apps.len(),
            tx_bytes: c.tx_bytes,
            rx_bytes: c.rx_bytes,
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: DpiArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        DpiCommand::Apps(list) => {
            let apps = util::apply_list_args(
                controller.list_dpi_applications().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &apps,
                |a| DpiAppRow::from(a),
                |a| a.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DpiCommand::Categories(list) => {
            let cats = util::apply_list_args(
                controller.list_dpi_categories().await?,
                &list,
                util::matches_json_filter,
            );
            let out = output::render_list(
                &global.output,
                &cats,
                |c| DpiCategoryRow::from(c),
                |c| c.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }
    }
}
