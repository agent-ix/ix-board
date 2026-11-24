// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board fleet status` through the real binary.

use assert_cmd::Command;
use ix_trace_rs::trace;

fn fleet_status(json: bool) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    let mut command = Command::cargo_bin("ix-board").unwrap();
    command
        .env("IX_BOARD_CONFIG_PATH", &config)
        .env("IX_RELAY_URL", "redis://127.0.0.1:1")
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
