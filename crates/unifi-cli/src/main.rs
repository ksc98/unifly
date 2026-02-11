mod cli;
mod config;
mod error;
mod output;

use clap::Parser;
use tracing_subscriber::EnvFilter;

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

async fn run(cli: Cli) -> std::result::Result<(), CliError> {
    match cli.command {
        // Config commands don't need a controller connection
        Command::Config(args) => {
            tracing::debug!("config subcommand: {:?}", args.command);
            eprintln!("Config commands not yet implemented");
            Ok(())
        }

        // Shell completions generation
        Command::Completions(args) => {
            use clap::CommandFactory;
            use clap_complete::generate;

            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "unifi", &mut std::io::stdout());
            Ok(())
        }

        // All other commands require a controller connection (stubbed for now)
        cmd => {
            let _config_data = config::load_config_or_default();
            let _profile_name = config::active_profile_name(&cli.global, &_config_data);

            tracing::debug!(command = ?cmd, "dispatching command");
            eprintln!("Command not yet implemented: {cmd:?}");
            Ok(())
        }
    }
}
