//! Integration tests for the `unifi` CLI binary.
//!
//! These tests validate argument parsing, help output, shell completions,
//! and error handling — all without requiring a live UniFi controller.
#![allow(clippy::unwrap_used)]

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

// ── Helpers ─────────────────────────────────────────────────────────

/// Build a [`Command`] for the `unifi` binary with env isolation.
///
/// Clears all `UNIFI_*` env vars and points config directories at a
/// nonexistent path so tests never touch the user's real configuration.
fn unifi_cmd() -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("unifi");
    cmd.env("HOME", "/tmp/unifi-cli-test-nonexistent")
        .env("XDG_CONFIG_HOME", "/tmp/unifi-cli-test-nonexistent")
        .env_remove("UNIFI_PROFILE")
        .env_remove("UNIFI_CONTROLLER")
        .env_remove("UNIFI_SITE")
        .env_remove("UNIFI_API_KEY")
        .env_remove("UNIFI_OUTPUT")
        .env_remove("UNIFI_INSECURE")
        .env_remove("UNIFI_TIMEOUT")
        .env_remove("UNIFI_USERNAME")
        .env_remove("UNIFI_PASSWORD");
    cmd
}

/// Concatenate stdout + stderr from a command output for flexible matching.
fn combined_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}")
}

// ── Basic invocation ────────────────────────────────────────────────

#[test]
fn test_no_args_shows_help() {
    let output = unifi_cmd().output().unwrap();
    assert_eq!(output.status.code(), Some(2), "Expected exit code 2");
    let text = combined_output(&output);
    assert!(
        text.contains("Usage"),
        "Expected 'Usage' in output:\n{text}"
    );
}

#[test]
fn test_help_flag() {
    unifi_cmd().arg("--help").assert().success().stdout(
        predicate::str::contains("UniFi network")
            .and(predicate::str::contains("devices"))
            .and(predicate::str::contains("clients"))
            .and(predicate::str::contains("networks")),
    );
}

#[test]
fn test_version_flag() {
    unifi_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("unifi"));
}

// ── Shell completions ───────────────────────────────────────────────

#[test]
fn test_completions_bash() {
    unifi_cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn test_completions_zsh() {
    unifi_cmd()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef"));
}

#[test]
fn test_completions_fish() {
    unifi_cmd()
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn test_invalid_subcommand() {
    let output = unifi_cmd().arg("foobar").output().unwrap();
    assert!(
        !output.status.success(),
        "Expected failure for invalid subcommand"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("invalid") || text.contains("unrecognized") || text.contains("foobar"),
        "Expected error mentioning invalid subcommand:\n{text}"
    );
}

#[test]
fn test_devices_list_no_controller() {
    unifi_cmd()
        .args(["devices", "list"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

#[test]
fn test_config_show_no_config() {
    // `config show` uses load_config_or_default() so it succeeds even
    // when no config file exists — it just renders the default config.
    unifi_cmd().args(["config", "show"]).assert().success();
}

#[test]
fn test_invalid_output_format() {
    let output = unifi_cmd()
        .args(["--output", "invalid", "devices", "list"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "Expected failure for invalid output format"
    );
    let text = combined_output(&output);
    assert!(
        text.contains("invalid")
            || text.contains("possible values")
            || text.contains("valid value"),
        "Expected error about valid output formats:\n{text}"
    );
}

#[test]
fn test_global_flags_parsing() {
    // All flags should parse correctly — the failure should be about
    // missing controller config, not about argument parsing.
    unifi_cmd()
        .args([
            "--output",
            "json",
            "--verbose",
            "--insecure",
            "--timeout",
            "60",
            "devices",
            "list",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("config")
                .or(predicate::str::contains("Configuration"))
                .or(predicate::str::contains("controller"))
                .or(predicate::str::contains("profile")),
        );
}

// ── Subcommand help discovery ───────────────────────────────────────

#[test]
fn test_devices_subcommands_exist() {
    unifi_cmd()
        .args(["devices", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("adopt"))
                .and(predicate::str::contains("remove"))
                .and(predicate::str::contains("restart")),
        );
}

#[test]
fn test_clients_subcommands_exist() {
    unifi_cmd()
        .args(["clients", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("get"))
                .and(predicate::str::contains("block"))
                .and(predicate::str::contains("unblock")),
        );
}

#[test]
fn test_firewall_subcommands_exist() {
    unifi_cmd()
        .args(["firewall", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("policies").and(predicate::str::contains("zones")));
}

#[test]
fn test_config_subcommands_exist() {
    unifi_cmd()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("init")
                .and(predicate::str::contains("show"))
                .and(predicate::str::contains("profiles")),
        );
}
