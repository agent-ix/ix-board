// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Public constructor and offline status share automatic cache recovery.

use ix_board::{Error, ReadService, WorkspaceSnapshot, WorkspaceSource};
use ix_trace_rs::trace;
use std::path::PathBuf;
use std::process::Command;

struct NoFetch;

impl WorkspaceSource for NoFetch {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        panic!("path recovery must not fetch Linear")
    }
}

#[test]
#[trace("FR-004-AC-22")]
fn public_default_cache_recovers_automatically_and_status_uses_same_path() {
    let directory = tempfile::tempdir().unwrap();
    let blocked_home = directory.path().join("blocked-home");
    std::fs::write(&blocked_home, b"not a directory").unwrap();
    let configuration = directory.path().join("empty.toml");
    std::fs::write(&configuration, b"").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "cache_recovery_probe",
            "--nocapture",
        ])
        .env("IX_BOARD_RECOVERY_PROBE", directory.path())
        .env("HOME", &blocked_home)
        .env("USERPROFILE", &blocked_home)
        .env("XDG_CACHE_HOME", &blocked_home)
        .env("LOCALAPPDATA", &blocked_home)
        .env("TMPDIR", directory.path())
        .env("TMP", directory.path())
        .env("TEMP", directory.path())
        .env("IX_BOARD_CONFIG_PATH", &configuration)
        .env_remove("IX_BOARD_CACHE_PATH")
        .env_remove("IX_BOARD_CACHE_TTL_SECONDS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "subprocess helper for automatic cache recovery"]
#[trace("FR-004-AC-22")]
fn cache_recovery_probe() {
    let root = PathBuf::from(std::env::var_os("IX_BOARD_RECOVERY_PROBE").unwrap());
    let path = ix_board::default_cache_path().unwrap();
    assert!(
        path.starts_with(root.canonicalize().unwrap()) || path.starts_with(&root),
        "{}",
        path.display()
    );
    assert_eq!(path.file_name().unwrap(), "linear.sqlite3");
    drop(ReadService::open_default(NoFetch).unwrap());
    let status =
        ix_board::cache_status::read_status(&ix_board::default_cache_path().unwrap()).unwrap();
    assert_eq!(status.cache_path, path.display().to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
