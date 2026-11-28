// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Real stores with isolated process environments and real Git indexes.

use std::path::Path;
use std::process::Command;

use ix_board::fleet::FleetConfig;
use ix_board::fleet::ledger::{Ledger, LedgerError};
use ix_board::retro::catalog::{Record, RecordKind};
use ix_board::retro::store::{Store, StoreError};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn run_case(relative: &str, allowed: bool, tracked: bool, project: bool, xdg: Option<&str>) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().canonicalize().unwrap();
    git(&home, &["init", "-q"]);
    let root = home.join(relative);
    let ledger = root.join("ix-board/fleet-ledger.sqlite3");
    let retro = root.join("team-retro/SWM/snapshots.jsonl");
    if project {
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
    }
    if tracked {
        for path in [&ledger, &retro] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        git(&home, &["add", "--", relative]);
        // Missing working files are still tracked by the index.
        std::fs::remove_file(&ledger).unwrap();
        std::fs::remove_file(&retro).unwrap();
    }
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "guard_child", "--test-threads=4", "--nocapture"])
        .env("HOME", &home)
        .env("SWM434_ROOT", &root)
        .env("SWM434_ALLOWED", if allowed { "yes" } else { "no" })
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_DATA_HOME");
    if let Some(variable) = xdg {
        child.env(variable, &root);
    }
    let output = child.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(ledger.exists(), allowed);
    assert_eq!(retro.exists(), allowed);
    if !allowed && !tracked && !project {
        assert!(!root.exists(), "refusal must create nothing");
    }
}

#[test]
fn home_rooted_repo_allows_default_state_and_data() {
    run_case(".local/state", true, false, false, None);
    run_case(".local/share", true, false, false, None);
}

#[test]
fn home_rooted_repo_allows_custom_xdg_state_and_data() {
    run_case("user-state", true, false, false, Some("XDG_STATE_HOME"));
    run_case("user-data", true, false, false, Some("XDG_DATA_HOME"));
}

#[test]
fn project_repo_is_refused_even_under_xdg_state() {
    run_case(
        ".local/state/project",
        false,
        false,
        true,
        Some("XDG_STATE_HOME"),
    );
}

#[test]
fn tracked_state_files_are_refused_even_when_missing() {
    run_case(".local/state", false, true, false, None);
}

#[test]
fn home_repo_outside_state_and_data_is_refused() {
    run_case("documents", false, false, false, None);
}

// Run in a subprocess so HOME/XDG never change in the parent test process.
#[test]
fn guard_child() {
    let Some(root) = std::env::var_os("SWM434_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let ledger_path = root.join("ix-board/fleet-ledger.sqlite3");
    let store = Store::resolve("SWM", Some(root.join("team-retro").into()), None).unwrap();
    let record = Record {
        schema_version: 1,
        kind: RecordKind::Snapshot,
        taken_at: "2026-11-27T00:00:00Z".into(),
        team: "SWM".into(),
        window: "1d".into(),
        metrics: Default::default(),
        department: None,
    };
    if std::env::var("SWM434_ALLOWED").unwrap() == "yes" {
        let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
        drop(ledger);
        store.check_writable().unwrap();
        store.append(&record).unwrap();
        assert_eq!(store.read_all().unwrap(), vec![record]);
        assert!(ledger_path.is_file());
    } else {
        assert!(matches!(
            Ledger::open(&ledger_path, &FleetConfig::default()),
            Err(LedgerError::InsideGit { .. })
        ));
        assert!(matches!(
            store.check_writable(),
            Err(StoreError::InsideGit { .. })
        ));
        assert!(matches!(
            store.append(&record),
            Err(StoreError::InsideGit { .. })
        ));
    }
}
