//! Config subcommand handlers.

use std::collections::HashMap;

use dialoguer::{Input, Select};

use crate::cli::{ConfigArgs, ConfigCommand, GlobalOpts};
use crate::config::{self, Config, Profile};
use crate::error::CliError;
use crate::output;

// ── Helpers ─────────────────────────────────────────────────────────

/// Serialize config to TOML and write to the canonical config path.
fn save_config(cfg: &Config) -> Result<(), CliError> {
    let path = config::config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(cfg).map_err(|e| CliError::Validation {
        field: "config".into(),
        reason: format!("failed to serialize config: {e}"),
    })?;
    std::fs::write(&path, toml_str)?;
    Ok(())
}

/// Map a dialoguer / interactive I/O failure into CliError.
fn prompt_err(e: impl std::fmt::Display) -> CliError {
    CliError::Validation {
        field: "interactive".into(),
        reason: format!("prompt failed: {e}"),
    }
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(args: ConfigArgs, global: &GlobalOpts) -> Result<(), CliError> {
    match args.command {
        // ── Init: interactive wizard ────────────────────────────────
        ConfigCommand::Init => {
            let config_path = config::config_path();
            eprintln!("✨ UniFi CLI — configuration wizard");
            eprintln!("   Config path: {}\n", config_path.display());

            // 1. Profile name
            let profile_name: String = Input::new()
                .with_prompt("Profile name")
                .default("default".into())
                .interact_text()
                .map_err(prompt_err)?;

            // 2. Controller URL
            let controller: String = Input::new()
                .with_prompt("Controller URL")
                .default("https://192.168.1.1".into())
                .interact_text()
                .map_err(prompt_err)?;

            // 3. Auth mode
            let auth_choices = &["API Key (recommended)", "Username/Password"];
            let auth_selection = Select::new()
                .with_prompt("Authentication method")
                .items(auth_choices)
                .default(0)
                .interact()
                .map_err(prompt_err)?;

            let (auth_mode, api_key, username, password) = if auth_selection == 0 {
                // --- API Key flow ---
                let key = rpassword::prompt_password("API key: ")
                    .map_err(prompt_err)?;

                if key.is_empty() {
                    return Err(CliError::Validation {
                        field: "api_key".into(),
                        reason: "API key cannot be empty".into(),
                    });
                }

                // Offer keyring storage
                let store_choices = &["Store in system keyring (recommended)", "Save to config file (plaintext)"];
                let store_selection = Select::new()
                    .with_prompt("Where to store the API key?")
                    .items(store_choices)
                    .default(0)
                    .interact()
                    .map_err(prompt_err)?;

                let api_key_field = if store_selection == 0 {
                    // Store in keyring
                    let entry = keyring::Entry::new("unifi-cli", &format!("{profile_name}/api-key"))
                        .map_err(|e| CliError::Validation {
                            field: "keyring".into(),
                            reason: format!("failed to access keyring: {e}"),
                        })?;
                    entry.set_password(&key).map_err(|e| CliError::Validation {
                        field: "keyring".into(),
                        reason: format!("failed to store API key in keyring: {e}"),
                    })?;
                    eprintln!("   ✓ API key stored in system keyring");
                    None // Don't write to config file
                } else {
                    Some(key) // Save plaintext in config
                };

                ("integration".to_string(), api_key_field, None, None)
            } else {
                // --- Username/Password flow ---
                let user: String = Input::new()
                    .with_prompt("Username")
                    .interact_text()
                    .map_err(prompt_err)?;

                let pass = rpassword::prompt_password("Password: ")
                    .map_err(prompt_err)?;

                if user.is_empty() || pass.is_empty() {
                    return Err(CliError::Validation {
                        field: "credentials".into(),
                        reason: "username and password cannot be empty".into(),
                    });
                }

                // Offer keyring storage for password
                let store_choices = &["Store password in system keyring (recommended)", "Save to config file (plaintext)"];
                let store_selection = Select::new()
                    .with_prompt("Where to store the password?")
                    .items(store_choices)
                    .default(0)
                    .interact()
                    .map_err(prompt_err)?;

                let password_field = if store_selection == 0 {
                    let entry = keyring::Entry::new("unifi-cli", &format!("{profile_name}/password"))
                        .map_err(|e| CliError::Validation {
                            field: "keyring".into(),
                            reason: format!("failed to access keyring: {e}"),
                        })?;
                    entry.set_password(&pass).map_err(|e| CliError::Validation {
                        field: "keyring".into(),
                        reason: format!("failed to store password in keyring: {e}"),
                    })?;
                    eprintln!("   ✓ Password stored in system keyring");
                    None
                } else {
                    Some(pass)
                };

                ("legacy".to_string(), None, Some(user), password_field)
            };

            // 4. Site name
            let site: String = Input::new()
                .with_prompt("Site name")
                .default("default".into())
                .interact_text()
                .map_err(prompt_err)?;

            // 5. Build profile and config
            let profile = Profile {
                controller,
                site,
                auth_mode,
                api_key,
                api_key_env: None,
                username,
                password,
                ca_cert: None,
                insecure: None,
                timeout: None,
            };

            let mut profiles = HashMap::new();
            profiles.insert(profile_name.clone(), profile);

            let cfg = Config {
                default_profile: Some(profile_name.clone()),
                defaults: Default::default(),
                profiles,
            };

            // 6. Write config
            save_config(&cfg)?;

            eprintln!("\n✓ Configuration written to {}", config_path.display());
            eprintln!("  Active profile: {profile_name}");
            eprintln!("\n  Test it: unifi system info --insecure");

            Ok(())
        }

        // ── Show ────────────────────────────────────────────────────
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

        // ── Set <key> <value> ───────────────────────────────────────
        ConfigCommand::Set { key, value } => {
            let mut cfg = config::load_config_or_default();
            let profile_name = config::active_profile_name(global, &cfg);

            let profile = cfg
                .profiles
                .entry(profile_name.clone())
                .or_insert_with(|| Profile {
                    controller: String::new(),
                    site: "default".into(),
                    auth_mode: "integration".into(),
                    api_key: None,
                    api_key_env: None,
                    username: None,
                    password: None,
                    ca_cert: None,
                    insecure: None,
                    timeout: None,
                });

            match key.as_str() {
                "controller" => profile.controller = value,
                "site" => profile.site = value,
                "auth_mode" | "auth-mode" => {
                    if value != "integration" && value != "legacy" {
                        return Err(CliError::Validation {
                            field: "auth_mode".into(),
                            reason: "must be 'integration' or 'legacy'".into(),
                        });
                    }
                    profile.auth_mode = value;
                }
                "api_key" | "api-key" => profile.api_key = Some(value),
                "api_key_env" | "api-key-env" => profile.api_key_env = Some(value),
                "username" => profile.username = Some(value),
                "insecure" => {
                    profile.insecure = Some(value.parse().map_err(|_| CliError::Validation {
                        field: "insecure".into(),
                        reason: "must be 'true' or 'false'".into(),
                    })?);
                }
                "timeout" => {
                    profile.timeout = Some(value.parse().map_err(|_| CliError::Validation {
                        field: "timeout".into(),
                        reason: "must be a number (seconds)".into(),
                    })?);
                }
                "ca_cert" | "ca-cert" => profile.ca_cert = Some(value.into()),
                other => {
                    return Err(CliError::Validation {
                        field: other.into(),
                        reason: format!(
                            "unknown config key '{other}'. Valid keys: controller, site, \
                             auth_mode, api_key, api_key_env, username, insecure, timeout, ca_cert"
                        ),
                    });
                }
            }

            save_config(&cfg)?;
            eprintln!("✓ Set {key} on profile '{profile_name}'");
            Ok(())
        }

        // ── Profiles ────────────────────────────────────────────────
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

        // ── Use <name> ─────────────────────────────────────────────
        ConfigCommand::Use { name } => {
            let mut cfg = config::load_config_or_default();

            if !cfg.profiles.contains_key(&name) {
                let available: Vec<_> = cfg.profiles.keys().cloned().collect();
                return Err(CliError::ProfileNotFound {
                    name,
                    available: if available.is_empty() {
                        "(none)".into()
                    } else {
                        available.join(", ")
                    },
                });
            }

            cfg.default_profile = Some(name.clone());
            save_config(&cfg)?;
            eprintln!("✓ Default profile set to '{name}'");
            Ok(())
        }

        // ── SetPassword ─────────────────────────────────────────────
        ConfigCommand::SetPassword { profile } => {
            let cfg = config::load_config_or_default();
            let profile_name = profile
                .unwrap_or_else(|| config::active_profile_name(global, &cfg));

            let prof = cfg.profiles.get(&profile_name).ok_or_else(|| {
                let available: Vec<_> = cfg.profiles.keys().cloned().collect();
                CliError::ProfileNotFound {
                    name: profile_name.clone(),
                    available: if available.is_empty() {
                        "(none)".into()
                    } else {
                        available.join(", ")
                    },
                }
            })?;

            let keyring_key = match prof.auth_mode.as_str() {
                "integration" => format!("{profile_name}/api-key"),
                _ => format!("{profile_name}/password"),
            };

            let prompt_label = match prof.auth_mode.as_str() {
                "integration" => "API key: ",
                _ => "Password: ",
            };

            let secret = rpassword::prompt_password(prompt_label)
                .map_err(prompt_err)?;

            if secret.is_empty() {
                return Err(CliError::Validation {
                    field: "secret".into(),
                    reason: "value cannot be empty".into(),
                });
            }

            let entry = keyring::Entry::new("unifi-cli", &keyring_key)
                .map_err(|e| CliError::Validation {
                    field: "keyring".into(),
                    reason: format!("failed to access keyring: {e}"),
                })?;
            entry.set_password(&secret).map_err(|e| CliError::Validation {
                field: "keyring".into(),
                reason: format!("failed to store secret in keyring: {e}"),
            })?;

            eprintln!("✓ Secret stored in system keyring for profile '{profile_name}'");
            Ok(())
        }
    }
}
