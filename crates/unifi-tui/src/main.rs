//! `unifi-tui` — Real-time terminal dashboard for UniFi network monitoring.
//!
//! Built on [ratatui](https://ratatui.rs) with reactive data from
//! `unifi-core`'s [`EntityStream`](unifi_core::EntityStream). Screens are
//! navigable via number keys (1-8): Dashboard, Devices, Clients, Networks,
//! Firewall, Topology, Events, and Stats.
//!
//! Logs are written to a file (default `/tmp/unifi-tui.log`) to avoid
//! corrupting the terminal UI. A background data bridge task continuously
//! streams entity updates from the controller into the TUI action loop.
//!
//! Entry point: CLI argument parsing, tracing setup, panic hooks, and app launch.

mod action;
mod app;
mod component;
mod data_bridge;
mod event;
mod screen;
mod screens;
mod theme;
mod tui;
mod widgets;

use std::path::PathBuf;

use clap::Parser;
use color_eyre::eyre::Result;
use secrecy::SecretString;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use unifi_core::{AuthCredentials, Controller, ControllerConfig, TlsVerification};

use crate::app::App;

/// Terminal dashboard for monitoring and managing UniFi networks.
#[derive(Parser, Debug)]
#[command(name = "unifi-tui", version, about)]
struct Cli {
    /// UniFi Controller URL (e.g., https://192.168.1.1)
    #[arg(short = 'u', long, env = "UNIFI_URL")]
    url: Option<String>,

    /// Site name (defaults to "default")
    #[arg(short = 's', long, default_value = "default", env = "UNIFI_SITE")]
    site: String,

    /// API key for the Integration API
    #[arg(short = 'k', long, env = "UNIFI_API_KEY")]
    api_key: Option<String>,

    /// Log file path (defaults to /tmp/unifi-tui.log)
    #[arg(long, default_value = "/tmp/unifi-tui.log")]
    log_file: PathBuf,

    /// Increase log verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

/// Set up file-based tracing. We MUST NOT log to stdout/stderr — that would
/// corrupt the TUI output. Returns a guard that must be held for the
/// lifetime of the application to ensure logs are flushed.
fn setup_tracing(cli: &Cli) -> WorkerGuard {
    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("unifi_tui={log_level}")));

    let log_dir = cli
        .log_file
        .parent()
        .unwrap_or(std::path::Path::new("/tmp"));
    let log_filename = cli
        .log_file
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("unifi-tui.log"));

    let file_appender = tracing_appender::rolling::never(log_dir, log_filename);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true),
        )
        .init();

    guard
}

/// Build a [`Controller`] from CLI args, if a URL was provided.
fn build_controller(cli: &Cli) -> Option<Controller> {
    let url_str = cli.url.as_deref()?;
    let url = url_str.parse().expect("invalid controller URL");

    let auth = match &cli.api_key {
        Some(key) => AuthCredentials::ApiKey(SecretString::from(key.clone())),
        None => {
            // No credentials — can't connect
            return None;
        }
    };

    let config = ControllerConfig {
        url,
        auth,
        site: cli.site.clone(),
        tls: TlsVerification::DangerAcceptInvalid,
        timeout: std::time::Duration::from_secs(30),
        refresh_interval_secs: 30,
        websocket_enabled: true,
        polling_interval_secs: 30,
    };

    Some(Controller::new(config))
}

/// Try loading a controller from the shared config file (default profile).
fn build_controller_from_config() -> Option<Controller> {
    let cfg = unifi_config::load_config().ok()?;
    let profile_name = cfg
        .default_profile
        .as_deref()
        .unwrap_or("default");
    let profile = cfg.profiles.get(profile_name)?;
    let config = unifi_config::profile_to_controller_config(profile, profile_name).ok()?;
    Some(Controller::new(config))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Install panic/error hooks BEFORE entering the terminal
    tui::install_hooks()?;

    // Tracing to file — hold the guard so logs flush on exit
    let _log_guard = setup_tracing(&cli);

    info!(
        url = cli.url.as_deref().unwrap_or("(not set)"),
        site = %cli.site,
        "starting unifi-tui"
    );

    // Priority: CLI flags > config file > onboarding wizard
    let controller = build_controller(&cli).or_else(build_controller_from_config);
    let mut app = App::new(controller);
    app.run().await?;

    Ok(())
}
