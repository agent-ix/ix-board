// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Offline process boundaries for refresh ownership and transport admission.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use ix_board::quota::{QuotaConfig, QuotaGate};
use ix_board::{
    Clock, Error, LinearGraphQlSource, ReadService, WorkspaceSnapshot, WorkspaceSource,
};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;

struct FixedClock(i64);

impl Clock for FixedClock {
    fn now_epoch_seconds(&self) -> i64 {
        self.0
    }
}

struct FileSource {
    directory: PathBuf,
    block: bool,
}

impl WorkspaceSource for FileSource {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        let mut attempts = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join("attempts"))
            .expect("attempt counter");
        attempts
            .write_all(b"attempt\n")
            .expect("record attempted refresh");
        if self.block {
            fs::write(self.directory.join("fetching"), b"ready").expect("fetch signal");
            wait_for(&self.directory.join("release"));
        }
        Ok(WorkspaceSnapshot::default())
    }
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "missing fixture signal: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn attempts(directory: &Path) -> usize {
    fs::read_to_string(directory.join("attempts"))
        .unwrap_or_default()
        .lines()
        .count()
}

fn child(directory: &Path, mode: &str, name: &str) -> Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "cache_coordination_child", "--nocapture"])
        .env("IX_BOARD_COORDINATION_DIRECTORY", directory)
        .env("IX_BOARD_COORDINATION_MODE", mode)
        .env("IX_BOARD_COORDINATION_NAME", name)
        .env_remove("IX_BOARD_CACHE_TTL_SECONDS")
        .env_remove("IX_BOARD_CONFIG_PATH")
        .spawn()
        .expect("launch separate cache process")
}

fn assert_child(child: Child) {
    assert!(
        child
            .wait_with_output()
            .expect("child outcome")
            .status
            .success()
    );
}

fn seed(directory: &Path) {
    let source = FileSource {
        directory: directory.to_owned(),
        block: false,
    };
    let mut service =
        ReadService::with_clock(source, &directory.join("cache.sqlite"), FixedClock(1000))
            .expect("seed service");
    assert_eq!(
        service
            .workspace_snapshot()
            .expect("seed snapshot")
            .fetched_at,
        1000
    );
}

fn lock_refresh(directory: &Path) -> File {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("cache.sqlite.refresh.lock"))
        .expect("adjacent lock");
    file.lock().expect("hold refresh lock");
    file
}

/// Trace: FR-004-AC-3, FR-031-AC-2
#[test]
fn cache_coordination_child() {
    let Some(directory) = std::env::var_os("IX_BOARD_COORDINATION_DIRECTORY") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let mode = std::env::var("IX_BOARD_COORDINATION_MODE").expect("child mode");
    if mode == "transport-cooldown" {
        transport_cooldown_child(&directory);
        return;
    }
    let name = std::env::var("IX_BOARD_COORDINATION_NAME").expect("child name");
    let source = FileSource {
        directory: directory.clone(),
        block: mode == "block",
    };
    let mut service =
        ReadService::with_clock(source, &directory.join("cache.sqlite"), FixedClock(1400))
            .expect("child service");
    fs::write(directory.join(format!("ready-{name}")), b"ready").expect("ready signal");
    let started = Instant::now();
    let result = service.workspace_snapshot();
    match mode.as_str() {
        "stale-lock" => {
            assert_eq!(result.expect("last complete snapshot").fetched_at, 1000);
            let freshness = service.cache_freshness().expect("fallback provenance");
            assert!(freshness.stale);
            assert_eq!(freshness.age_seconds, Some(400));
            assert!(
                freshness
                    .failure_reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("lock"))
            );
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "five-second wait plus scheduler tolerance"
            );
        }
        "empty-lock" => {
            assert!(matches!(result, Err(Error::RefreshLockTimeout)));
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "five-second wait plus scheduler tolerance"
            );
        }
        "block" | "refresh" => assert_eq!(result.expect("complete refresh").fetched_at, 1400),
        mode => panic!("unknown child mode: {mode}"),
    }
}

#[trace("FR-004-AC-1", "FR-004-AC-2", "FR-004-AC-20")]
#[test]
fn separate_process_waiter_reuses_one_completed_refresh_of_expired_snapshot() {
    let directory = tempfile::tempdir().expect("temporary directory");
    seed(directory.path());
    let owner = child(directory.path(), "block", "owner");
    wait_for(&directory.path().join("fetching"));
    let waiter = child(directory.path(), "refresh", "waiter");
    wait_for(&directory.path().join("ready-waiter"));
    assert_eq!(
        attempts(directory.path()),
        2,
        "seed plus one in-flight refresh"
    );
    fs::write(directory.path().join("release"), b"release").expect("release refresh");
    assert_child(owner);
    assert_child(waiter);
    assert_eq!(
        attempts(directory.path()),
        2,
        "waiter must consume published snapshot"
    );
}

#[trace("FR-004-AC-2", "FR-004-AC-20")]
#[test]
fn terminating_lock_owner_releases_refresh_ownership_for_another_process() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut owner = child(directory.path(), "block", "owner");
    wait_for(&directory.path().join("fetching"));
    owner.kill().expect("terminate fixture lock owner");
    assert!(!owner.wait().expect("terminated child").success());
    assert_child(child(directory.path(), "refresh", "recovery"));
    assert_eq!(
        attempts(directory.path()),
        2,
        "abandoned fetch and one recovery fetch"
    );
}

#[trace("FR-004-AC-3", "FR-004-AC-20")]
#[test]
fn held_os_lock_returns_stale_snapshot_or_typed_empty_cache_failure_within_five_seconds() {
    for mode in ["stale-lock", "empty-lock"] {
        let directory = tempfile::tempdir().expect("temporary directory");
        if mode == "stale-lock" {
            seed(directory.path());
        }
        let lock = lock_refresh(directory.path());
        assert_child(child(directory.path(), mode, "waiter"));
        assert_eq!(
            attempts(directory.path()),
            usize::from(mode == "stale-lock")
        );
        drop(lock);
    }
}

fn transport_cooldown_child(directory: &Path) {
    let endpoint = fs::read_to_string(directory.join("endpoint")).expect("fixture endpoint");
    let mut source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("other-key")),
        || None,
        &endpoint,
    )
    .expect("fixture credentials");
    source
        .configure_cache(&directory.join("cache.sqlite"))
        .expect("shared quota path");
    assert!(matches!(
        source.fetch_workspace(),
        Err(Error::LinearRateLimited { .. })
    ));
    for result in [
        source
            .fetch_state_changes("00000000-0000-0000-0000-000000000001")
            .map(|_| ()),
        source.fetch_issue_updates(0.0).map(|_| ()),
        source.fetch_marker_lines("x", "y").map(|_| ()),
    ] {
        let reason = result.expect_err("cooldown must reject transport");
        assert!(
            reason.contains("Linear rate limited; retry in "),
            "wrong classification: {reason}"
        );
    }
    let mut service =
        ReadService::open(source, &directory.join("cache.sqlite")).expect("empty cache service");
    assert!(matches!(
        service.force_refresh_workspace(),
        Err(Error::LinearRateLimited { .. })
    ));
}

#[trace("FR-031-AC-1", "FR-031-AC-2")]
#[test]
fn persisted_cooldown_blocks_all_actual_transport_entry_points_in_another_process() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
    let responder = listener.try_clone().expect("response listener");
    let response = std::thread::spawn(move || {
        let (mut stream, _) = responder.accept().expect("first transport request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("fixture timeout");
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let read = stream.read(&mut buffer).expect("request bytes");
            assert_ne!(read, 0, "complete HTTP request");
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("request length"))
                    })
                    .expect("content length");
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Type: text/plain\r\nContent-Length: 4\r\nRetry-After: 60\r\nConnection: close\r\n\r\nbusy").expect("non-JSON rate limit response");
    });
    fs::write(
        directory.path().join("endpoint"),
        format!(
            "http://{}/graphql",
            listener.local_addr().expect("listener address")
        ),
    )
    .expect("endpoint file");
    let gate = QuotaGate::open(
        &directory.path().join("cache.sqlite"),
        &QuotaConfig::default(),
    )
    .expect("shared quota");
    let mut source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("first-key")),
        || None,
        &fs::read_to_string(directory.path().join("endpoint")).expect("endpoint"),
    )
    .expect("source");
    source
        .configure_cache(&directory.path().join("cache.sqlite"))
        .expect("shared cache");
    assert!(
        matches!(
            source.fetch_workspace(),
            Err(Error::LinearRateLimited { .. })
        ),
        "non-JSON HTTP429 retains rate-limit classification"
    );
    response.join().expect("fixture response");
    listener
        .set_nonblocking(true)
        .expect("nonblocking observation");
    assert_child(child(directory.path(), "transport-cooldown", "transport"));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock),
        "all entry points must make zero connections"
    );
    assert_eq!(gate.status().expect("quota status").requests_used, 1);
}
