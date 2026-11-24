// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board fleet status` through the real binary.

use assert_cmd::Command;
use ix_trace_rs::trace;

fn fleet_status(json: bool) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    fleet_status_with(json, &dir.path().join("ledger.sqlite3"))
}

fn fleet_status_with(json: bool, ledger: &std::path::Path) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    let mut command = Command::cargo_bin("ix-board").unwrap();
    command
        .env("IX_BOARD_CONFIG_PATH", &config)
        .env("IX_RELAY_URL", "redis://127.0.0.1:1")
        .env("IX_BOARD_LEDGER_PATH", ledger)
        .env_remove("IX_PROMETHEUS_URL");
    if json {
        command.arg("--json");
    }
    command.args(["fleet", "status"]).output().unwrap()
}

#[trace("FR-034-AC-15")]
#[test]
fn json_and_text_report_an_unreadable_relay_with_the_internal_error_code() {
    let json = fleet_status(true);
    assert_eq!(json.status.code(), Some(4));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["complete"], false);
    assert_eq!(value["rows"].as_array().unwrap().len(), 0);
    assert_eq!(value["defects"][0]["kind"], "unreadable_source");
    assert_eq!(value["defects"][0]["source"], "relay");
    assert!(value["defects"][0].get("observed_ms").is_some());
    let text = fleet_status(false);
    assert_eq!(text.status.code(), Some(4));
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("defect: unreadable_source source=relay"));
    assert!(text.ends_with("complete: false\n") || text.ends_with("complete: false"));
}

fn ledger_query(ledger: &std::path::Path, view: &str) -> std::process::Output {
    Command::cargo_bin("ix-board")
        .unwrap()
        .env("IX_BOARD_LEDGER_PATH", ledger)
        .env("IX_BOARD_CONFIG_PATH", ledger.with_extension("toml"))
        .args(["--json", "fleet", "ledger", "query", view])
        .output()
        .unwrap()
}

#[trace("FR-039-AC-7")]
#[test]
fn status_writes_the_ledger_and_query_reads_the_defect_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.sqlite3");
    std::fs::write(ledger.with_extension("toml"), "").unwrap();
    let first = fleet_status_with(true, &ledger);
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    // The relay is unreachable, so the run carries only host time: no relay first-seen exists.
    let seen = first["defects"][0]["observed_ms"].as_i64().unwrap();
    assert!(first["defects"][0].get("first_seen_ms").is_none());
    let second = fleet_status_with(true, &ledger);
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert!(second["defects"][0].get("first_seen_ms").is_none());
    let before = std::fs::read(&ledger).unwrap();
    let out = ledger_query(&ledger, "defect_rows");
    assert_eq!(out.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["view"], "defect_rows");
    assert_eq!(value["rows"].as_array().unwrap().len(), 1);
    assert_eq!(value["rows"][0]["source"], "relay");
    assert!(value["rows"][0]["first_seen_at"].is_null());
    assert_eq!(
        value["rows"][0]["first_seen_host_ms"].as_i64().unwrap(),
        seen
    );
    assert_eq!(std::fs::read(&ledger).unwrap(), before);
    let none = dir.path().join("none.sqlite3");
    std::fs::write(none.with_extension("toml"), "").unwrap();
    let missing = ledger_query(&none, "defect_rows");
    assert_eq!(missing.status.code(), Some(3));
}

#[trace("FR-039-AC-5")]
#[test]
fn status_with_an_unwritable_ledger_reports_ledger_unwritable() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("a-directory");
    std::fs::create_dir(&ledger).unwrap();
    let out = fleet_status_with(true, &ledger);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["complete"], false);
    let kinds: Vec<&str> = value["defects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"ledger_unwritable"), "{kinds:?}");
    assert!(kinds.contains(&"unreadable_source"), "{kinds:?}");
}
