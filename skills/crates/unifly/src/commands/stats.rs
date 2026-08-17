//! Statistics command handlers.

use chrono::{DateTime, Utc};
use unifly_core::Controller;

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

fn parse_time(value: &str, field: &str) -> Result<i64, CliError> {
    if let Ok(ts) = value.parse::<i64>() {
        return Ok(ts);
    }
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .map_err(|_| CliError::Validation {
            field: field.into(),
            reason: format!("invalid timestamp '{value}' (use Unix seconds or RFC3339)"),
        })
}

fn parse_time_range(
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(Option<i64>, Option<i64>), CliError> {
    let start_ts = start.map(|s| parse_time(s, "start")).transpose()?;
    let end_ts = end.map(|s| parse_time(s, "end")).transpose()?;
    if let (Some(s), Some(e)) = (start_ts, end_ts) {
        if s > e {
            return Err(CliError::Validation {
                field: "start".into(),
                reason: "start must be <= end".into(),
            });
        }
    }
    Ok((start_ts, end_ts))
}

pub async fn handle(
    controller: &Controller,
    args: StatsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let data = match args.command {
        StatsCommand::Site(query) => {
            let interval = interval_str(&query.interval);
            let (start, end) = parse_time_range(query.start.as_deref(), query.end.as_deref())?;
            controller
                .get_site_stats(interval, start, end, query.attrs.as_deref())
                .await?
        }
        StatsCommand::Device(query) => {
            let interval = interval_str(&query.interval);
            controller
                .get_device_stats(interval, query.macs.as_deref(), query.attrs.as_deref())
                .await?
        }
        StatsCommand::Client(query) => {
            let interval = interval_str(&query.interval);
            controller
                .get_client_stats(interval, query.macs.as_deref(), query.attrs.as_deref())
                .await?
        }
        StatsCommand::Gateway(query) => {
            let interval = interval_str(&query.interval);
            let (start, end) = parse_time_range(query.start.as_deref(), query.end.as_deref())?;
            controller
                .get_gateway_stats(interval, start, end, query.attrs.as_deref())
                .await?
        }
        StatsCommand::Dpi { group_by, macs } => {
            let gb = match group_by {
                DpiGroupBy::ByApp => "by_app",
                DpiGroupBy::ByCat => "by_cat",
            };
            controller.get_dpi_stats(gb, macs.as_deref()).await?
        }
    };

    let out = render_stats(&data, &global.output);
    output::print_output(&out, global.quiet);
    Ok(())
}
