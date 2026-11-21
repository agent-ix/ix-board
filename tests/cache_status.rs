// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Offline CLI status and configuration precedence.

use assert_cmd::Command;
use serde_json::Value;

fn cli(directory: &tempfile::TempDir) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("ix-board"));
    command
        .env(
            "IX_BOARD_CACHE_PATH",
            directory.path().join("cache.sqlite3"),
        )
        .env("HOME", directory.path())
        .env_remove("IX_BOARD_CONFIG_PATH")
        .env_remove("IX_BOARD_CACHE_TTL_SECONDS")
        .env_remove("IX_BOARD_LINEAR_PLAN_TIER")
        .env_remove("IX_BOARD_LINEAR_QUOTA_SHARE")
        .env_remove("IX_BOARD_LINEAR_QUOTA_RESERVE")
        .env_remove("IX_BOARD_LINEAR_AUTH_MODE")
        .env("LINEAR_API_KEY", "");
    command
}

/// Trace: FR-031-AC-6, FR-004-AC-21
#[test]
fn offline_status_accepts_empty_and_legacy_databases_without_credentials() {
    let directory = tempfile::tempdir().expect("cache");
    let output = cli(&directory)
        .args(["cache", "status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: Value = serde_json::from_slice(&output).expect("JSON status");
    assert_eq!(status["datasets"], serde_json::json!([]));
    assert!(status["latest_refresh"].is_null());
    assert!(status["quota"]["rate_limit_until"].is_null());
    let connection =
        rusqlite::Connection::open(directory.path().join("cache.sqlite3")).expect("cache");
    connection.execute_batch("CREATE TABLE workspace_snapshot(cache_key TEXT PRIMARY KEY,fetched_at INTEGER NOT NULL,payload BLOB NOT NULL);
        INSERT INTO workspace_snapshot VALUES('legacy',1,X'7b7d');").expect("legacy schema");
    let output = cli(&directory)
        .args(["cache", "status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: Value = serde_json::from_slice(&output).expect("legacy JSON status");
    assert_eq!(status["datasets"][0]["coverage"], "detail");
    assert!(status["datasets"][0]["age_seconds"].is_null());
    assert_eq!(status["datasets"][0]["stale"], true);
    assert!(status["datasets"][0]["last_full_reconciliation"].is_null());
    let output = cli(&directory)
        .args(["cache", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).expect("text");
    assert!(text.contains("coverage: detail | stale: true | age_seconds: unknown"));
}

/// Trace: FR-004-AC-11, FR-031-AC-7
#[test]
fn environment_overrides_configuration_and_invalid_explicit_settings_fail_offline() {
    let directory = tempfile::tempdir().expect("cache");
    let config = directory.path().join("config.toml");
    std::fs::write(&config,"[cache]\nttl_seconds = 0\n[linear]\nplan_tier = 'business'\nquota_share = 0.1\nquota_reserve = 0.0\n").expect("configuration");
    let output = cli(&directory)
        .env("IX_BOARD_CONFIG_PATH", &config)
        .env("IX_BOARD_CACHE_TTL_SECONDS", "600")
        .args(["cache", "status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: Value = serde_json::from_slice(&output).expect("JSON status");
    assert_eq!(status["quota"]["request_budget"], 500);
    assert_eq!(status["quota"]["complexity_budget"], 600_000);
    for invalid in ["0", "-1", "1.5", "oops"] {
        cli(&directory)
            .env("IX_BOARD_CACHE_TTL_SECONDS", invalid)
            .args(["cache", "status"])
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "cache TTL must be a positive integer",
            ));
    }
    for invalid in ["0", "-0.1", "0.5001", "NaN", "inf", "oops"] {
        cli(&directory)
            .env("IX_BOARD_LINEAR_QUOTA_SHARE", invalid)
            .args(["cache", "status"])
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "invalid Linear quota configuration",
            ));
    }
}
