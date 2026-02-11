//! Config subcommand handlers.

use crate::cli::{ConfigArgs, ConfigCommand, GlobalOpts};
use crate::config;
use crate::error::CliError;
use crate::output;

pub async fn handle(args: ConfigArgs, global: &GlobalOpts) -> Result<(), CliError> {
    match args.command {
        ConfigCommand::Init => {
            eprintln!("Interactive config init not yet implemented");
            eprintln!("Config path: {}", config::config_path().display());
            Ok(())
        }

        ConfigCommand::Show => {
            let cfg = config::load_config_or_default();
            let out = output::render_single(
                &global.output,
                &cfg,
                |c| format!("{c:#?}"),
                |_| "config".into(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        ConfigCommand::Set { key, value } => {
            eprintln!("Setting {key} = {value}");
            eprintln!("Config mutation not yet implemented");
            Ok(())
        }

        ConfigCommand::Profiles => {
            let cfg = config::load_config_or_default();
            let default = cfg.default_profile.as_deref().unwrap_or("default");
            if cfg.profiles.is_empty() {
                eprintln!("No profiles configured. Run: unifi config init");
            } else {
                for name in cfg.profiles.keys() {
                    let marker = if name == default { " *" } else { "" };
                    println!("{name}{marker}");
                }
            }
            Ok(())
        }

        ConfigCommand::Use { name } => {
            eprintln!("Setting default profile to '{name}'");
            eprintln!("Config mutation not yet implemented");
            Ok(())
        }

        ConfigCommand::SetPassword { profile: _ } => {
            eprintln!("Keyring password storage not yet implemented");
            Ok(())
        }
    }
}
