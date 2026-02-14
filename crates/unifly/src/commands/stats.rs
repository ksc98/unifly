//! Statistics command handlers.

use unifi_core::Controller;

use crate::cli::{DpiGroupBy, GlobalOpts, OutputFormat, StatsArgs, StatsCommand, StatsInterval};
use crate::error::CliError;
use crate::output;

/// Convert a `StatsInterval` to the Legacy API string.
fn interval_str(interval: &StatsInterval) -> &'static str {
    match interval {
        StatsInterval::FiveMinutes => "5minutes",
        StatsInterval::Hourly => "hourly",
        StatsInterval::Daily => "daily",
        StatsInterval::Monthly => "monthly",
    }
}

/// Render `Vec<serde_json::Value>` in the chosen output format.
fn render_stats(data: &[serde_json::Value], format: &OutputFormat) -> String {
    match format {
        OutputFormat::JsonCompact => output::render_json_compact(data),
        OutputFormat::Yaml => output::render_yaml(data),
        // Dynamic fields -- fall back to pretty JSON for table/plain/json
        _ => output::render_json_pretty(data),
    }
}

pub async fn handle(
    controller: &Controller,
    args: StatsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let data = match args.command {
        StatsCommand::Site(query) => {
            let interval = interval_str(&query.interval);
            controller.get_site_stats(interval, None, None).await?
        }
        StatsCommand::Device(query) => {
            let interval = interval_str(&query.interval);
            controller
                .get_device_stats(interval, query.macs.as_deref())
                .await?
        }
        StatsCommand::Client(query) => {
            let interval = interval_str(&query.interval);
            controller
                .get_client_stats(interval, query.macs.as_deref())
                .await?
        }
        StatsCommand::Gateway(query) => {
            let interval = interval_str(&query.interval);
            controller.get_gateway_stats(interval, None, None).await?
        }
        StatsCommand::Dpi { group_by, .. } => {
            let gb = match group_by {
                DpiGroupBy::ByApp => "by-app",
                DpiGroupBy::ByCat => "by-cat",
            };
            controller.get_dpi_stats(gb).await?
        }
    };

    let out = render_stats(&data, &global.output);
    output::print_output(&out, global.quiet);
    Ok(())
}
