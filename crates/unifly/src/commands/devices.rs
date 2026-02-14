//! Device command handlers.

use std::sync::Arc;

use tabled::Tabled;
use unifi_core::{Command as CoreCommand, Controller, Device, MacAddress};

use crate::cli::{DevicesArgs, DevicesCommand, GlobalOpts};
use crate::error::CliError;
use crate::output;

use super::util;

// ── Table row ───────────────────────────────────────────────────────

#[derive(Tabled)]
struct DeviceRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Model")]
    model: String,
    #[tabled(rename = "Type")]
    dtype: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "IP")]
    ip: String,
    #[tabled(rename = "MAC")]
    mac: String,
}

impl From<&Arc<Device>> for DeviceRow {
    fn from(d: &Arc<Device>) -> Self {
        Self {
            id: d.id.to_string(),
            name: d.name.clone().unwrap_or_default(),
            model: d.model.clone().unwrap_or_default(),
            dtype: format!("{:?}", d.device_type),
            state: format!("{:?}", d.state),
            ip: d.ip.map(|ip| ip.to_string()).unwrap_or_default(),
            mac: d.mac.to_string(),
        }
    }
}

fn detail(d: &Arc<Device>) -> String {
    let mut lines = vec![
        format!("ID:       {}", d.id),
        format!("Name:     {}", d.name.as_deref().unwrap_or("-")),
        format!("MAC:      {}", d.mac),
        format!(
            "IP:       {}",
            d.ip.map_or_else(|| "-".into(), |ip| ip.to_string())
        ),
        format!("Model:    {}", d.model.as_deref().unwrap_or("-")),
        format!("Type:     {:?}", d.device_type),
        format!("State:    {:?}", d.state),
        format!("Firmware: {}", d.firmware_version.as_deref().unwrap_or("-")),
    ];
    if let Some(up) = d.stats.uptime_secs {
        lines.push(format!("Uptime:   {up}s"));
    }
    if let Some(cpu) = d.stats.cpu_utilization_pct {
        lines.push(format!("CPU:      {cpu:.1}%"));
    }
    if let Some(mem) = d.stats.memory_utilization_pct {
        lines.push(format!("Memory:   {mem:.1}%"));
    }
    lines.join("\n")
}

// ── Handler ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub async fn handle(
    controller: &Controller,
    args: DevicesArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    match args.command {
        DevicesCommand::List(_list) => {
            let snap = controller.devices_snapshot();
            let out = output::render_list(
                &global.output,
                &snap,
                |d| DeviceRow::from(d),
                |d| d.id.to_string(),
            );
            output::print_output(&out, global.quiet);
            Ok(())
        }

        DevicesCommand::Get { device } => {
            let snap = controller.devices_snapshot();
            let found = snap
                .iter()
                .find(|d| d.id.to_string() == device || d.mac.to_string() == device);
            match found {
                Some(d) => {
                    let out =
                        output::render_single(&global.output, d, detail, |d| d.id.to_string());
                    output::print_output(&out, global.quiet);
                }
                None => {
                    return Err(CliError::NotFound {
                        resource_type: "device".into(),
                        identifier: device,
                        list_command: "devices list".into(),
                    });
                }
            }
            Ok(())
        }

        DevicesCommand::Adopt {
            mac,
            ignore_limit: _,
        } => {
            let mac = MacAddress::new(&mac);
            controller.execute(CoreCommand::AdoptDevice { mac }).await?;
            if !global.quiet {
                eprintln!("Device adoption initiated");
            }
            Ok(())
        }

        DevicesCommand::Remove { device } => {
            let id = util::resolve_device_id(controller, &device)?;
            if !util::confirm(&format!("Remove device {device}?"), global.yes)? {
                return Ok(());
            }
            controller.execute(CoreCommand::RemoveDevice { id }).await?;
            if !global.quiet {
                eprintln!("Device removed");
            }
            Ok(())
        }

        DevicesCommand::Restart { device } => {
            let id = util::resolve_device_id(controller, &device)?;
            controller
                .execute(CoreCommand::RestartDevice { id })
                .await?;
            if !global.quiet {
                eprintln!("Device restart initiated");
            }
            Ok(())
        }

        DevicesCommand::Locate { device, on } => {
            let mac = util::resolve_device_mac(controller, &device)?;
            controller
                .execute(CoreCommand::LocateDevice { mac, enable: on })
                .await?;
            if !global.quiet {
                let state = if on { "enabled" } else { "disabled" };
                eprintln!("Locate LED {state}");
            }
            Ok(())
        }

        DevicesCommand::PortCycle { device, port } => {
            let device_id = util::resolve_device_id(controller, &device)?;
            controller
                .execute(CoreCommand::PowerCyclePort {
                    device_id,
                    port_idx: port,
                })
                .await?;
            if !global.quiet {
                eprintln!("Port {port} power-cycled");
            }
            Ok(())
        }

        DevicesCommand::Stats { device: _ } => util::not_yet_implemented("device real-time stats"),

        DevicesCommand::Pending(_list) => util::not_yet_implemented("pending device listing"),

        DevicesCommand::Upgrade { device, url } => {
            let mac = util::resolve_device_mac(controller, &device)?;
            controller
                .execute(CoreCommand::UpgradeDevice {
                    mac,
                    firmware_url: url,
                })
                .await?;
            if !global.quiet {
                eprintln!("Firmware upgrade initiated");
            }
            Ok(())
        }

        DevicesCommand::Provision { device } => {
            let mac = util::resolve_device_mac(controller, &device)?;
            controller
                .execute(CoreCommand::ProvisionDevice { mac })
                .await?;
            if !global.quiet {
                eprintln!("Device re-provision initiated");
            }
            Ok(())
        }

        DevicesCommand::Speedtest => {
            controller.execute(CoreCommand::SpeedtestDevice).await?;
            if !global.quiet {
                eprintln!("Speed test initiated");
            }
            Ok(())
        }

        DevicesCommand::Tags(_list) => util::not_yet_implemented("device tags"),
    }
}
