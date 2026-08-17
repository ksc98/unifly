//! Shared helpers for command handlers.

use std::path::Path;

use unifly_core::{Controller, EntityId, MacAddress};

use crate::cli::ListArgs;
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
#[allow(clippy::unnecessary_wraps)]
pub fn resolve_device_mac(
    controller: &Controller,
    identifier: &str,
) -> Result<MacAddress, CliError> {
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
#[allow(dead_code)]
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
        .map_err(|e| CliError::Io(std::io::Error::other(e)))?;
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

/// Apply list flags (`--limit`, `--offset`, `--all`, `--filter`) to an iterator.
pub fn apply_list_args<T>(
    items: impl IntoIterator<Item = T>,
    list: &ListArgs,
    matches_filter: impl Fn(&T, &str) -> bool,
) -> Vec<T> {
    let offset = usize::try_from(list.offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(list.limit).unwrap_or(usize::MAX);
    let filter = list
        .filter
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty());

    let filtered = items.into_iter().filter(|item| match filter {
        Some(expr) => matches_filter(item, expr),
        None => true,
    });

    if list.all {
        filtered.skip(offset).collect()
    } else {
        filtered.skip(offset).take(limit).collect()
    }
}

/// Fallback filter matcher for list items: case-insensitive JSON text contains.
pub fn matches_json_filter<T: serde::Serialize>(item: &T, filter: &str) -> bool {
    let needle = filter.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return true;
    }
    serde_json::to_string(item)
        .map(|haystack| haystack.to_ascii_lowercase().contains(&needle))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{apply_list_args, matches_json_filter};
    use crate::cli::ListArgs;

    #[test]
    fn apply_list_args_respects_offset_limit() {
        let args = ListArgs {
            limit: 2,
            offset: 1,
            all: false,
            filter: None,
        };
        let rows = vec![1, 2, 3, 4];
        let sliced = apply_list_args(rows, &args, |_, _| true);
        assert_eq!(sliced, vec![2, 3]);
    }

    #[test]
    fn apply_list_args_respects_filter_case_insensitive() {
        let args = ListArgs {
            limit: 25,
            offset: 0,
            all: false,
            filter: Some("BETA".into()),
        };
        let rows = vec![
            serde_json::json!({"name":"alpha"}),
            serde_json::json!({"name":"beta"}),
        ];
        let filtered = apply_list_args(rows, &args, |item, filter| {
            matches_json_filter(item, filter)
        });
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["name"], "beta");
    }
}
