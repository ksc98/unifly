//! Country code command handler.

use tabled::Tabled;
use unifly_core::{Controller, Country};

use crate::cli::GlobalOpts;
use crate::error::CliError;
use crate::output;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct CountryRow {
    #[tabled(rename = "Code")]
    code: String,
    #[tabled(rename = "Name")]
    name: String,
}

impl From<&Country> for CountryRow {
    fn from(c: &Country) -> Self {
        Self {
            code: c.code.clone(),
            name: c.name.clone(),
        }
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(controller: &Controller, global: &GlobalOpts) -> Result<(), CliError> {
    let countries = controller.list_countries().await?;
    let out = output::render_list(
        &global.output,
        &countries,
        |c| CountryRow::from(c),
        |c| c.code.clone(),
    );
    output::print_output(&out, global.quiet);
    Ok(())
}
