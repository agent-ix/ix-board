// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Offline CLI status and configuration precedence.

use assert_cmd::Command;
use ix_trace_rs::trace;
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
#[trace("FR-031-AC-6", "FR-004-AC-21")]
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
#[trace("FR-004-AC-11", "FR-031-AC-7")]
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

#[trace("FR-032-AC-1", "FR-032-AC-2")]
#[test]
fn offline_status_names_zero_usage_server_reserve_hold_with_owned_deadline() {
    use ix_board::quota::{QuotaConfig, QuotaGate};
    use reqwest::header::HeaderMap;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.sqlite3");
    let gate = QuotaGate::open(&path, &QuotaConfig::default()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let reset = i64::try_from(now).unwrap() + 60_000;
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("x-ratelimit-requests-limit", "10000".to_owned()),
        ("x-ratelimit-requests-remaining", "250".to_owned()),
        ("x-ratelimit-requests-reset", reset.to_string()),
    ] {
        headers.insert(name, value.parse().unwrap());
    }
    gate.observe_headers(200, &headers, None).unwrap();
    let output = cli(&directory)
        .args(["cache", "status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(status["quota"]["requests_used"], 0);
    assert_eq!(status["quota"]["complexity_used"], 0);
    assert!(status["rate_limit_until"].is_null());
    assert_eq!(status["decision"]["state"], "server_reserve_held");
    assert_eq!(status["decision"]["endpoint"], "graphql");
    assert_eq!(status["decision"]["estimated_complexity"], 10000);
    assert_eq!(status["decision"]["blockers"][0]["dimension"], "requests");
    assert_eq!(status["decision"]["blockers"][0]["deadline"], reset);
    assert_eq!(status["decision"]["blockers"][0]["reserve"], 500);
    let output = cli(&directory)
        .args(["cache", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(
        text.contains("locally enforced server reserve hold"),
        "{text}"
    );
    assert!(
        text.contains(&format!("owning deadline Some({reset})")),
        "{text}"
    );
    assert!(text.contains("admission not guaranteed"));
    assert!(!text.contains("Linear rate limited"));
}
