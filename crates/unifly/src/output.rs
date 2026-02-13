//! Output formatting: table, JSON, YAML, plain.
//!
//! Renders data in the format selected by `--output`. Table uses `tabled`,
//! structured formats use serde, plain emits one identifier per line.

use std::io::{self, IsTerminal, Write};

use tabled::{Table, Tabled, settings::Style};

use crate::cli::{ColorMode, OutputFormat};

// ── Color helpers (SilkCircuit palette) ──────────────────────────────

/// Determine whether color output should be enabled.
#[allow(dead_code)]
pub fn should_color(mode: &ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err(),
    }
}

// ── Render dispatchers ───────────────────────────────────────────────

/// Render a list of serde-serializable + tabled items in the chosen format.
///
/// - `table`: uses the `Tabled` derive to build a pretty table
/// - `json` / `json-compact`: serializes the original data via serde
/// - `yaml`: serializes via serde_yaml
/// - `plain`: calls `id_fn` on each item to emit one identifier per line
pub fn render_list<T, R>(
    format: &OutputFormat,
    data: &[T],
    to_row: impl Fn(&T) -> R,
    id_fn: impl Fn(&T) -> String,
) -> String
where
    T: serde::Serialize,
    R: Tabled,
{
    match format {
        OutputFormat::Table => {
            let rows: Vec<R> = data.iter().map(to_row).collect();
            render_table(&rows)
        }
        OutputFormat::Json => render_json(data, false),
        OutputFormat::JsonCompact => render_json(data, true),
        OutputFormat::Yaml => render_yaml(data),
        OutputFormat::Plain => data.iter().map(&id_fn).collect::<Vec<_>>().join("\n"),
    }
}

/// Render a single serde-serializable item in the chosen format.
///
/// Table rendering uses a custom `detail_fn` that returns a pre-formatted string,
/// since single-item detail views don't use `Tabled` derive.
pub fn render_single<T>(
    format: &OutputFormat,
    data: &T,
    detail_fn: impl Fn(&T) -> String,
    id_fn: impl Fn(&T) -> String,
) -> String
where
    T: serde::Serialize,
{
    match format {
        OutputFormat::Table => detail_fn(data),
        OutputFormat::Json => render_json(data, false),
        OutputFormat::JsonCompact => render_json(data, true),
        OutputFormat::Yaml => render_yaml(data),
        OutputFormat::Plain => id_fn(data),
    }
}

/// Print the rendered output to stdout, respecting quiet mode.
pub fn print_output(output: &str, quiet: bool) {
    if quiet || output.is_empty() {
        return;
    }
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{output}");
}

// ── Format-specific renderers ────────────────────────────────────────

fn render_table<R: Tabled>(rows: &[R]) -> String {
    Table::new(rows).with(Style::rounded()).to_string()
}

/// Pretty-printed JSON.
pub(crate) fn render_json_pretty<T: serde::Serialize + ?Sized>(data: &T) -> String {
    serde_json::to_string_pretty(data).expect("serialization should not fail")
}

/// Compact single-line JSON.
pub(crate) fn render_json_compact<T: serde::Serialize + ?Sized>(data: &T) -> String {
    serde_json::to_string(data).expect("serialization should not fail")
}

fn render_json<T: serde::Serialize + ?Sized>(data: &T, compact: bool) -> String {
    if compact {
        render_json_compact(data)
    } else {
        render_json_pretty(data)
    }
}

/// YAML output.
pub(crate) fn render_yaml<T: serde::Serialize + ?Sized>(data: &T) -> String {
    serde_yaml::to_string(data).expect("serialization should not fail")
}
