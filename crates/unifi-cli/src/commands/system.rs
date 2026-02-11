//! System command handlers.

use tabled::Tabled;
use unifi_core::{Command as CoreCommand, Controller, HealthSummary, SysInfo, SystemInfo};

use crate::cli::{BackupCommand, GlobalOpts, SystemArgs, SystemCommand};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table rows ──────────────────────────────────────────────────────

#[derive(Tabled)]
struct HealthRow {
    #[tabled(rename = "Subsystem")]
    subsystem: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Devices")]
    devices: String,
    #[tabled(rename = "Clients")]
    clients: String,
}

impl From<&HealthSummary> for HealthRow {
    fn from(h: &HealthSummary) -> Self {
        Self {
            subsystem: h.subsystem.clone(),
            status: h.status.clone(),
            devices: h.num_adopted.map(|n| n.to_string()).unwrap_or_default(),
            clients: h.num_sta.map(|n| n.to_string()).unwrap_or_default(),
        }
    }
}

// ── Detail views ────────────────────────────────────────────────────

fn system_info_detail(info: &SystemInfo) -> String {
    let mut lines = vec![
        format!("Version:  {}", info.version),
    ];
    if let Some(ref name) = info.controller_name {
        lines.insert(0, format!("Name:     {name}"));
    }
    if let Some(ref build) = info.build {
        lines.push(format!("Build:    {build}"));
    }
    if let Some(ref hostname) = info.hostname {
        lines.push(format!("Hostname: {hostname}"));
    }
    if let Some(ip) = info.ip {
        lines.push(format!("IP:       {ip}"));
    }
    if let Some(uptime) = info.uptime_secs {
        lines.push(format!("Uptime:   {}s", uptime));
    }
    if let Some(update) = info.update_available {
        lines.push(format!("Update:   {}", if update { "available" } else { "up to date" }));
    }
    lines.join("\n")
}

fn sysinfo_detail(info: &SysInfo) -> String {
    let mut lines = Vec::new();
    if let Some(ref hostname) = info.hostname {
        lines.push(format!("Hostname:   {hostname}"));
    }
    if let Some(ref tz) = info.timezone {
        lines.push(format!("Timezone:   {tz}"));
    }
    if !info.ip_addrs.is_empty() {
        lines.push(format!("IPs:        {}", info.ip_addrs.join(", ")));
    }
    if let Some(autobackup) = info.autobackup {
        lines.push(format!("Autobackup: {}", if autobackup { "yes" } else { "no" }));
    }
    if let Some(retention) = info.data_retention_days {
        lines.push(format!("Retention:  {} days", retention));
    }
    if lines.is_empty() {
        lines.push("(no data)".into());
    }
    lines.join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

pub async fn handle(
    controller: &Controller,
    args: SystemArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        SystemCommand::Info => {
            let info = controller.get_system_info().await?;
            let out = output::render_single(
                &global.output,
                &info,
                system_info_detail,
                |i| i.version.clone(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        SystemCommand::Health => {
            let health = controller.get_site_health().await?;
            let out = output::render_list(
                &global.output,
                &health,
                |h| HealthRow::from(h),
                |h| h.subsystem.clone(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        SystemCommand::Sysinfo => {
            let info = controller.get_sysinfo().await?;
            let out = output::render_single(
                &global.output,
                &info,
                sysinfo_detail,
                |_| "sysinfo".into(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        SystemCommand::Backup(backup_args) => {
            handle_backup(controller, backup_args.command, global).await
        }

        SystemCommand::Reboot => {
            if !util::confirm("Reboot controller hardware?", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::RebootController)
                .await?;
            if !global.quiet {
                eprintln!("Controller reboot initiated");
            }
            Ok(())
        }

        SystemCommand::Poweroff => {
            if !util::confirm("Power off controller hardware? This cannot be undone remotely.", global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::PoweroffController)
                .await?;
            if !global.quiet {
                eprintln!("Controller power-off initiated");
            }
            Ok(())
        }
    }
}

async fn handle_backup(
    controller: &Controller,
    cmd: BackupCommand,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match cmd {
        BackupCommand::Create => {
            controller
                .execute(CoreCommand::CreateBackup)
                .await?;
            if !global.quiet {
                eprintln!("Backup created");
            }
            Ok(())
        }

        BackupCommand::List => util::not_yet_implemented("backup listing"),

        BackupCommand::Download { .. } => util::not_yet_implemented("backup download"),

        BackupCommand::Delete { filename } => {
            if !util::confirm(&format!("Delete backup '{filename}'?"), global.yes)? {
                return Ok(());
            }
            controller
                .execute(CoreCommand::DeleteBackup { filename })
                .await?;
            if !global.quiet {
                eprintln!("Backup deleted");
            }
            Ok(())
        }
    }
}
