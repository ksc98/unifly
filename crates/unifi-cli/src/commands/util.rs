//! Shared helpers for command handlers.

use std::path::Path;

use unifi_core::{Controller, EntityId, MacAddress};

use crate::error::CliError;

/// Resolve a device identifier (UUID or MAC) to an EntityId via snapshot lookup.
pub fn resolve_device_id(controller: &Controller, identifier: &str) -> Result<EntityId, CliError> {
    let snap = controller.devices_snapshot();
    for device in snap.iter() {
        if device.id.to_string() == identifier || device.mac.to_string() == identifier {
            return Ok(device.id.clone());
        }
    }
    Err(CliError::NotFound {
        resource_type: "device".into(),
        identifier: identifier.into(),
        list_command: "devices list".into(),
    })
}

/// Resolve a device identifier to a MacAddress via snapshot lookup.
pub fn resolve_device_mac(controller: &Controller, identifier: &str) -> Result<MacAddress, CliError> {
    let snap = controller.devices_snapshot();
    for device in snap.iter() {
        if device.id.to_string() == identifier || device.mac.to_string() == identifier {
            return Ok(device.mac.clone());
        }
    }
    // If not in snapshot, treat the identifier itself as a MAC
    Ok(MacAddress::new(identifier))
}

/// Resolve a client identifier (UUID or MAC) to an EntityId via snapshot lookup.
pub fn resolve_client_id(controller: &Controller, identifier: &str) -> Result<EntityId, CliError> {
    let snap = controller.clients_snapshot();
    for client in snap.iter() {
        if client.id.to_string() == identifier || client.mac.to_string() == identifier {
            return Ok(client.id.clone());
        }
    }
    Err(CliError::NotFound {
        resource_type: "client".into(),
        identifier: identifier.into(),
        list_command: "clients list".into(),
    })
}

/// Prompt for confirmation, auto-approving if `--yes` was passed.
pub fn confirm(message: &str, yes_flag: bool) -> Result<bool, CliError> {
    if yes_flag {
        return Ok(true);
    }
    let confirmed = dialoguer::Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .map_err(|e| CliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    Ok(confirmed)
}

/// Read and parse a JSON file for `--from-file` flags.
pub fn read_json_file(path: &Path) -> Result<serde_json::Value, CliError> {
    let contents = std::fs::read_to_string(path)?;
    serde_json::from_str(&contents).map_err(|e| CliError::Validation {
        field: "from-file".into(),
        reason: format!("invalid JSON: {e}"),
    })
}

/// Print a "legacy API not yet connected" stub message.
pub fn legacy_stub(resource: &str) -> Result<(), CliError> {
    eprintln!("{resource}: requires legacy API connection (not yet wired)");
    Ok(())
}
