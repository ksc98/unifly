mod cli;
mod commands;
mod config;
mod error;
mod output;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use unifi_core::Controller;

use crate::cli::{Cli, Command};
use crate::error::CliError;

#[tokio::main]
async fn main() {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Setup tracing based on verbosity
    init_tracing(cli.global.verbose);

    // Dispatch and handle errors with proper exit codes
    if let Err(err) = run(cli).await {
        let code = err.exit_code();
        eprintln!("{:?}", miette::Report::new(err));
        std::process::exit(code);
    }
}

fn init_tracing(verbosity: u8) {
    let filter = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        // Config commands don't need a controller connection
        Command::Config(args) => commands::config_cmd::handle(args, &cli.global).await,

        // Shell completions generation
        Command::Completions(args) => {
            use clap::CommandFactory;
            use clap_complete::generate;

            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "unifi", &mut std::io::stdout());
            Ok(())
        }

        // All other commands require a controller connection
        cmd => {
            let controller_config = build_controller_config(&cli.global)?;
            let controller = Controller::new(controller_config);

            tracing::debug!(command = ?cmd, "dispatching command");
            commands::dispatch(cmd, &controller, &cli.global).await
        }
    }
}

/// Build a `ControllerConfig` from the config file, profile, and CLI overrides.
fn build_controller_config(
    global: &cli::GlobalOpts,
) -> Result<unifi_core::ControllerConfig, CliError> {
    let cfg = config::load_config_or_default();
    let profile_name = config::active_profile_name(global, &cfg);

    // If a profile exists, use it with CLI flag overrides
    if let Some(profile) = cfg.profiles.get(&profile_name) {
        return config::resolve_profile(profile, &profile_name, global);
    }

    // No profile found -- try to build from CLI flags / env vars alone
    let url_str = global.controller.as_deref().ok_or_else(|| CliError::NoConfig {
        path: config::config_path().display().to_string(),
    })?;

    let url: url::Url = url_str.parse().map_err(|_| CliError::Validation {
        field: "controller".into(),
        reason: format!("invalid URL: {url_str}"),
    })?;

    let auth = if let Some(ref key) = global.api_key {
        unifi_core::AuthCredentials::ApiKey(secrecy::SecretString::from(key.clone()))
    } else {
        return Err(CliError::NoCredentials {
            profile: profile_name,
        });
    };

    let tls = if global.insecure {
        unifi_core::TlsVerification::DangerAcceptInvalid
    } else {
        unifi_core::TlsVerification::SystemDefaults
    };

    Ok(unifi_core::ControllerConfig {
        url,
        auth,
        site: global.site.clone().unwrap_or_else(|| "default".into()),
        tls,
        timeout: std::time::Duration::from_secs(global.timeout),
        refresh_interval_secs: 0,
        websocket_enabled: false,
        polling_interval_secs: 30,
    })
}
