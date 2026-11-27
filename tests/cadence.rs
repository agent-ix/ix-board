// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Acceptance tests for cadence scheduling and ledger behavior (FR-049).

use ix_board::fleet::FleetConfig;
use ix_board::fleet::ledger::Ledger;
use ix_trace_rs::trace;
use serde_json::json;
use std::path::Path;

fn now_ms() -> i64 {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    i64::try_from(millis).unwrap()
}

fn manifest(path: &Path, interval_secs: u64) {
    let value = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "retro",
            "interval_secs": interval_secs,
            "kind": "retro_snapshot",
            "command": ["retro", "snapshot", "--config", "/tmp/retro.json"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
#[trace("FR-049-AC-10")]
fn cadence_accepts_check_jobs_and_rejects_cleanup() {
    use ix_board::cadence::{Job, JobKind, Manifest, validate_manifest};
    let mut manifest = Manifest {
        alert_recipient: None,
        jobs: vec![Job {
            program: "QSL".into(),
            team: "qsl".into(),
            id: "check".into(),
            interval_secs: 300,
            kind: JobKind::Hygiene,
            command: vec!["hygiene".into(), "check".into()],
            after_success: None,
            owner_role: "minder".into(),
        }],
    };
    validate_manifest(&manifest).unwrap();
    for command in [
        vec!["hygiene", "clean"],
        vec!["hygiene", "clean", "--apply"],
        vec!["hygiene", "check", "--apply"],
    ] {
        manifest.jobs[0].command = command.into_iter().map(str::to_owned).collect();
        assert!(validate_manifest(&manifest).is_err());
    }
    manifest.jobs[0].kind = JobKind::HostCheck;
    manifest.jobs[0].command = vec!["host".into(), "check".into()];
    validate_manifest(&manifest).unwrap();
    manifest.jobs[0].command.push("--help".into());
    assert!(validate_manifest(&manifest).is_err());
}

#[test]
#[trace("FR-049-AC-10", "FR-048-AC-1")]
fn cadence_hygiene_check_resolves_local_host_and_records_the_check() {
    let directory = tempfile::tempdir().unwrap();
    let manifest_path = directory.path().join("cadence.json");
    let ledger_path = directory.path().join("ledger.sqlite3");
    ix_board::cadence::generate_default_manifest(&manifest_path, "QSL", "qsl").unwrap();
    let root = directory.path().join("worktrees");
    std::fs::create_dir(&root).unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[hygiene]\nworktree_root = {:?}\ndefault_free_floor_gb = 1\n",
            root.to_str().unwrap()
        ),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "hygiene-check", "--manifest"])
        .arg(&manifest_path)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .env("IX_BOARD_CONFIG_PATH", &config)
        .env("IX_BOARD_HOST", "fixture-host")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fixture-host"), "{stdout}");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("unrecognized"));
    let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
    let result = ledger
        .cadence_last_run("QSL", "qsl", "hygiene-check")
        .unwrap()
        .unwrap();
    assert!(result.finished_at_ms.is_some());
    assert!(matches!(result.outcome.as_str(), "succeeded" | "failed"));
    assert!(root.exists());
}

#[test]
#[trace("FR-049-AC-11")]
fn alert_dry_run_names_one_recipient_without_starting_transport() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cadence.json");
    let ledger = directory.path().join("ledger.sqlite3");
    std::fs::write(
        &path,
        br#"{"alert_recipient":"draco:tooling:minder","jobs":[]}"#,
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args([
            "cadence",
            "due",
            "--program",
            "QSL",
            "--team",
            "qsl",
            "--alert",
            "--dry-run",
            "--manifest",
        ])
        .arg(&path)
        .env("IX_BOARD_LEDGER_PATH", &ledger)
        .env("PATH", directory.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "dry-run: cadence alert to draco:tooling:minder; no notification sent\n"
    );
    std::fs::write(&path, br#"{"alert_recipient":"all","jobs":[]}"#).unwrap();
    assert_eq!(due_command(&path, &ledger).status.code(), Some(3));
}

fn due_command(manifest: &Path, ledger: &Path) -> std::process::Output {
    assert_cmd::Command::cargo_bin("ix-board")
        .unwrap()
        .args([
            "cadence",
            "due",
            "--program",
            "QSL",
            "--team",
            "qsl",
            "--manifest",
        ])
        .arg(manifest)
        .env("IX_BOARD_LEDGER_PATH", ledger)
        .output()
        .unwrap()
}

#[test]
#[trace("FR-049-AC-8", "FR-049-AC-10")]
fn default_retro_manifest_generation_is_idempotent_and_never_clobbers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/cadence.json");
    let generated = ix_board::cadence::generate_default_manifest(&path, "SWM", "qsl").unwrap();
    assert_eq!(generated.jobs.len(), 3);
    assert_eq!(generated.jobs[0].interval_secs, 1800);
    assert_eq!(
        generated.jobs[0].command,
        ["retro", "snapshot", "--team", "qsl"]
    );
    assert_eq!(
        generated.jobs[0].after_success.as_deref().unwrap(),
        ["retro", "delta", "--team", "qsl", "--notify"]
    );
    assert_eq!(generated.jobs[1].id, "cadence-checker");
    assert_eq!(generated.jobs[1].interval_secs, 300);
    assert_eq!(generated.jobs[1].command, ["host", "check"]);
    assert_eq!(generated.jobs[2].id, "hygiene-check");
    assert_eq!(generated.jobs[2].interval_secs, 300);
    assert_eq!(generated.jobs[2].command, ["hygiene", "check"]);
    assert!(generated.jobs[1].after_success.is_none());
    assert!(generated.jobs[2].after_success.is_none());
    let bytes = std::fs::read(&path).unwrap();
    let rerun = ix_board::cadence::generate_default_manifest(&path, "OTHER", "other").unwrap();
    assert_eq!(rerun, generated);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    let existing = directory.path().join("existing.json");
    std::fs::write(&existing, br#"{"jobs":[]}"#).unwrap();
    let original = std::fs::read(&existing).unwrap();
    let existing_manifest =
        ix_board::cadence::generate_default_manifest(&existing, "SWM", "qsl").unwrap();
    assert!(existing_manifest.jobs.is_empty());
    assert_eq!(std::fs::read(&existing).unwrap(), original);
}

#[test]
#[trace("FR-049-AC-8")]
fn cadence_due_creates_the_default_manifest_on_first_run() {
    let directory = tempfile::tempdir().unwrap();
    let config_home = directory.path().join("config");
    let ledger = directory.path().join("ledger.sqlite3");
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
            .args(["cadence", "due", "--program", "SWM", "--team", "qsl"])
            .env("XDG_CONFIG_HOME", &config_home)
            .env("IX_BOARD_LEDGER_PATH", &ledger)
            .env_remove("IX_BOARD_CADENCE_MANIFEST")
            .output()
            .unwrap()
    };
    let first = run();
    assert_eq!(first.status.code(), Some(1));
    let path = config_home.join("ix-board/cadence.json");
    let bytes = std::fs::read(&path).unwrap();
    assert!(String::from_utf8_lossy(&first.stdout).contains("retro-qsl due"));
    let second = run();
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
#[trace("FR-049-AC-3", "FR-049-AC-6")]
fn cadence_run_releases_ledger_for_fleet_tick_and_concurrent_due() {
    let directory = tempfile::tempdir().unwrap();
    let manifest_path = directory.path().join("cadence.json");
    let ledger_path = directory.path().join("ledger.sqlite3");
    let value = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "tick",
            "interval_secs": 3600,
            "kind": "fleet_tick",
            "command": ["fleet", "tick"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(&manifest_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let config_path = directory.path().join("config.toml");
    std::fs::write(&config_path, "[fleet]\nhost_timeout_secs = 1\n").unwrap();

    let run = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "tick", "--manifest"])
        .arg(&manifest_path)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .env("IX_BOARD_CONFIG_PATH", &config_path)
        .env("IX_RELAY_URL", "redis://127.0.0.1:1")
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let due = due_command(&manifest_path, &ledger_path);
    assert!(
        String::from_utf8_lossy(&due.stderr).is_empty(),
        "concurrent due could not open the ledger: {}",
        String::from_utf8_lossy(&due.stderr)
    );
    assert!(
        String::from_utf8_lossy(&due.stdout).contains("tick"),
        "concurrent due did not report the scheduled job: {}",
        String::from_utf8_lossy(&due.stdout)
    );
    let run = run.wait_with_output().unwrap();
    assert!(
        !String::from_utf8_lossy(&run.stderr).contains("another writer holds the ledger lock"),
        "fleet tick child was blocked by the cadence writer lock: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
#[trace("FR-049-AC-3")]
fn cadence_hygiene_job_runs_the_focus_hygiene_argv() {
    let directory = tempfile::tempdir().unwrap();
    let manifest_path = directory.path().join("cadence.json");
    let ledger_path = directory.path().join("ledger.sqlite3");
    let value = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "hygiene",
            "interval_secs": 3600,
            "kind": "hygiene",
            "command": ["focus", "hygiene", "--help"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(&manifest_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "hygiene", "--manifest"])
        .arg(&manifest_path)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cadence did not run focus hygiene: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: ix-board focus hygiene"));
    let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
    let result = ledger
        .cadence_last_run("QSL", "qsl", "hygiene")
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, "succeeded");
}

#[test]
#[trace("FR-049-AC-7")]
fn cadence_runs_stored_delta_after_snapshot_without_sending_when_no_defect_exists() {
    let directory = tempfile::tempdir().unwrap();
    let manifest_path = directory.path().join("cadence.json");
    let ledger_path = directory.path().join("ledger.sqlite3");
    let retro_home = directory.path().join("team-retro");
    let retro_config = directory.path().join("retro.json");
    std::fs::write(
        &retro_config,
        serde_json::to_vec(&json!({
            "team": "QSL",
            "projects": [],
            "gate_tickets": [],
            "repos": []
        }))
        .unwrap(),
    )
    .unwrap();
    let config_path = retro_config.to_string_lossy().into_owned();
    let unrelated_manifest = directory.path().join("unrelated-cadence.json");
    std::fs::write(&unrelated_manifest, "not the selected cadence manifest").unwrap();
    let manifest_value = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "retro",
            "interval_secs": 1800,
            "kind": "retro_snapshot",
            "command": ["retro", "snapshot", "--help"],
            "after_success": ["retro", "delta", "--config", config_path, "--notify"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest_value).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "retro", "--manifest"])
        .arg(&manifest_path)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .env("TEAM_RETRO_HOME", &retro_home)
        .env("IX_BOARD_CADENCE_MANIFEST", &unrelated_manifest)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: ix-board retro snapshot"));
    assert!(stdout.contains("no delta:"));
    let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
    assert_eq!(
        ledger
            .cadence_last_run("QSL", "qsl", "retro")
            .unwrap()
            .unwrap()
            .outcome,
        "succeeded"
    );
}

#[test]
#[trace("FR-049-AC-7")]
fn cadence_skips_delta_after_snapshot_failure_and_records_follow_up_failure() {
    let directory = tempfile::tempdir().unwrap();
    let ledger_path = directory.path().join("ledger.sqlite3");
    let retro_home = directory.path().join("team-retro");
    let retro_config = directory.path().join("retro.json");
    std::fs::write(
        &retro_config,
        serde_json::to_vec(&json!({
            "team": "QSL",
            "projects": [],
            "gate_tickets": [],
            "repos": [],
            "gate_log": null
        }))
        .unwrap(),
    )
    .unwrap();
    let config_path = retro_config.to_string_lossy().into_owned();
    let invalid_snapshot_config = directory.path().join("invalid-snapshot.json");
    std::fs::write(&invalid_snapshot_config, "{").unwrap();
    let failed_snapshot_manifest = directory.path().join("failed-snapshot.json");
    let failed_snapshot = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "failed-snapshot",
            "interval_secs": 1800,
            "kind": "retro_snapshot",
            "command": ["retro", "snapshot", "--config", invalid_snapshot_config],
            "after_success": ["retro", "delta", "--config", config_path, "--notify"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(
        &failed_snapshot_manifest,
        serde_json::to_vec(&failed_snapshot).unwrap(),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "failed-snapshot", "--manifest"])
        .arg(&failed_snapshot_manifest)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .env("TEAM_RETRO_HOME", &retro_home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("no delta:"),
        "follow-up delta must be skipped after snapshot failure"
    );
    let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
    assert_eq!(
        ledger
            .cadence_last_run("QSL", "qsl", "failed-snapshot")
            .unwrap()
            .unwrap()
            .outcome,
        "failed"
    );
    drop(ledger);

    let failed_delta_manifest = directory.path().join("failed-delta.json");
    let failed_delta = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "failed-delta",
            "interval_secs": 1800,
            "kind": "retro_snapshot",
            "command": ["retro", "snapshot", "--help"],
            "after_success": ["retro", "delta", "--config", invalid_snapshot_config, "--notify"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(
        &failed_delta_manifest,
        serde_json::to_vec(&failed_delta).unwrap(),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "failed-delta", "--manifest"])
        .arg(&failed_delta_manifest)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .env("TEAM_RETRO_HOME", &retro_home)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
    assert_eq!(
        ledger
            .cadence_last_run("QSL", "qsl", "failed-delta")
            .unwrap()
            .unwrap()
            .outcome,
        "failed"
    );
}

#[test]
#[trace("FR-049-AC-8", "FR-024-AC-24")]
fn generated_cadence_manifest_uses_the_auto_detected_team_sources() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("qsl-cadence.json");
    let manifest = ix_board::cadence::generate_default_manifest(&path, "QSL", "QSL").unwrap();
    assert_eq!(manifest.jobs.len(), 3);
    assert_eq!(manifest.jobs[0].interval_secs, 1800);
    assert_eq!(manifest.jobs[0].owner_role, "minder");
    assert_eq!(
        manifest.jobs[0].command,
        ["retro", "snapshot", "--team", "QSL"]
    );
    assert_eq!(
        manifest.jobs[0].after_success.as_deref().unwrap(),
        ["retro", "delta", "--team", "QSL", "--notify"]
    );
    let retro_config = ix_board::retro::RetroConfig::parse(
        Path::new("examples/qsl/retro.json"),
        include_str!("../examples/qsl/retro.json"),
    )
    .unwrap();
    assert_eq!(retro_config.team, "QSL");
    assert_eq!(retro_config.herdr_teams, ["QSL"]);
    assert!(
        retro_config
            .gate_log
            .as_ref()
            .unwrap()
            .ends_with("QSL/gate.jsonl")
    );
}

#[test]
#[trace("FR-049-AC-3")]
fn cadence_run_records_a_child_failure() {
    let directory = tempfile::tempdir().unwrap();
    let manifest_path = directory.path().join("cadence.json");
    let ledger_path = directory.path().join("ledger.sqlite3");
    let value = json!({
        "jobs": [{
            "program": "QSL",
            "team": "qsl",
            "id": "hygiene-fail",
            "interval_secs": 3600,
            "kind": "hygiene",
            "command": ["focus", "hygiene", "--not-a-real-option"],
            "owner_role": "minder"
        }]
    });
    std::fs::write(&manifest_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["cadence", "run", "hygiene-fail", "--manifest"])
        .arg(&manifest_path)
        .env("IX_BOARD_LEDGER_PATH", &ledger_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
    let result = ledger
        .cadence_last_run("QSL", "qsl", "hygiene-fail")
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, "failed");
    assert!(result.finished_at_ms.is_some());
}

#[test]
#[trace("FR-049-AC-1")]
fn cadence_due_cli_uses_exit_codes_zero_one_and_two() {
    let clear_dir = tempfile::tempdir().unwrap();
    let clear_manifest = clear_dir.path().join("cadence.json");
    let clear_ledger = clear_dir.path().join("ledger.sqlite3");
    manifest(&clear_manifest, 3600);
    let now = now_ms();
    let mut ledger = Ledger::open(&clear_ledger, &FleetConfig::default()).unwrap();
    ledger.cadence_start("QSL", "qsl", "retro", now).unwrap();
    ledger
        .cadence_finish("QSL", "qsl", "retro", now + 1, true, None)
        .unwrap();
    drop(ledger);
    assert_eq!(
        due_command(&clear_manifest, &clear_ledger).status.code(),
        Some(0)
    );

    let due_dir = tempfile::tempdir().unwrap();
    let due_manifest = due_dir.path().join("cadence.json");
    let due_ledger = due_dir.path().join("ledger.sqlite3");
    manifest(&due_manifest, 1800);
    assert_eq!(
        due_command(&due_manifest, &due_ledger).status.code(),
        Some(1)
    );

    let overdue_dir = tempfile::tempdir().unwrap();
    let overdue_manifest = overdue_dir.path().join("cadence.json");
    let overdue_ledger = overdue_dir.path().join("ledger.sqlite3");
    manifest(&overdue_manifest, 1);
    let mut ledger = Ledger::open(&overdue_ledger, &FleetConfig::default()).unwrap();
    ledger.cadence_start("QSL", "qsl", "retro", 0).unwrap();
    ledger
        .cadence_finish("QSL", "qsl", "retro", 1, true, None)
        .unwrap();
    drop(ledger);
    assert_eq!(
        due_command(&overdue_manifest, &overdue_ledger)
            .status
            .code(),
        Some(2)
    );
}
