#![cfg(not(any(target_os = "windows", target_os = "macos")))]

use std::{path::PathBuf, process::Command};

use ix_board::{Error, ReadService, WorkspaceSnapshot, WorkspaceSource};
use tempfile::TempDir;

const PROBE_EXPECTED_PATH: &str = "IX_BOARD_CACHE_PATH_TEST_EXPECTED";

struct NoFetchSource;

impl WorkspaceSource for NoFetchSource {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        Err(Error::MissingWorkspaceData)
    }
}

/// Tracing: TC-004-07
/// ACs: FR-004-AC-5, FR-004-AC-6
#[test]
fn tc_004_07_public_cache_api_uses_override_and_cli_default_policy() {
    let temp = TempDir::new().expect("temporary cache root");

    let override_path = temp.path().join("explicit.sqlite3");
    run_public_api_probe(&override_path, Some(&override_path), None);
    assert!(override_path.is_file());

    let xdg_home = temp.path().join("xdg-cache");
    let xdg_path = xdg_home.join("ix-board").join("linear.sqlite3");
    run_public_api_probe(&xdg_path, None, Some(&xdg_home));
    assert!(xdg_path.is_file());
}

fn run_public_api_probe(
    expected_path: &std::path::Path,
    override_path: Option<&std::path::Path>,
    xdg_cache_home: Option<&std::path::Path>,
) {
    let mut command = Command::new(std::env::current_exe().expect("integration test executable"));
    command
        .args(["--ignored", "--exact", "cache_path_probe", "--nocapture"])
        .env(PROBE_EXPECTED_PATH, expected_path);
    if let Some(path) = override_path {
        command.env("IX_BOARD_CACHE_PATH", path);
    } else {
        command
            .env_remove("IX_BOARD_CACHE_PATH")
            .env_remove("HOME")
            .env_remove("USERPROFILE")
            .env(
                "XDG_CACHE_HOME",
                xdg_cache_home.expect("XDG path for default probe"),
            );
    }
    let output = command.output().expect("run public API probe");
    assert!(
        output.status.success(),
        "probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `run_linear` in the CLI uses this same public constructor, so this probe
/// exercises the cache selection path shared by both consumers.
#[test]
#[ignore = "subprocess-only helper for tc_004_07 public cache API coverage"]
fn cache_path_probe() {
    let expected = PathBuf::from(
        std::env::var_os(PROBE_EXPECTED_PATH).expect("probe's expected path environment variable"),
    );
    let service = ReadService::open_default(NoFetchSource).expect("open default cache service");
    drop(service);
    assert_eq!(
        ix_board::default_cache_path().expect("resolve default cache path"),
        expected
    );
    assert!(
        expected.is_file(),
        "the public API opened the expected cache"
    );
}
