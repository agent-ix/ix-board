// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! The opt-in bounded workspace read (FR-004-AC-11 through FR-004-AC-19).
//!
//! Credentials come only from an injected fake store and every HTTP request goes to a
//! loopback server; nothing here reads the process environment or a real credential.

use ix_board::{
    CatalogEvidence, Clock, Deadline, Error, Issue, IssueId, LinearGraphQlSource, NamedFocusRequest,
    NamedMember, NamedSelection, Project, ProjectId, ProjectStatusCategory, ReadPolicy,
    ReadService, TeamKey, WorkflowStateType, WorkspaceSnapshot, WorkspaceSource,
};
use ix_cli_kit::secrets::{
    AppScope, CredentialBackend, DeleteStatus, Presence, SecretError, SecretKey, SecretStore,
    SecretValue,
};
use ix_trace_rs::trace;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SECOND: Duration = Duration::from_secs(1);

/// A clock whose epoch seconds and monotonic time only move when a test says so.
#[derive(Clone)]
struct ManualClock {
    epoch: Arc<AtomicI64>,
    base: Instant,
    elapsed: Arc<Mutex<Duration>>,
    /// Added to the monotonic time after every reading of it, so work that checks the
    /// deadline sees time pass without any real waiting. Zero unless a test sets it.
    step: Arc<Mutex<Duration>>,
}

impl ManualClock {
    fn new(epoch: i64) -> Self {
        Self {
            epoch: Arc::new(AtomicI64::new(epoch)),
            base: Instant::now(),
            elapsed: Arc::new(Mutex::new(Duration::ZERO)),
            step: Arc::new(Mutex::new(Duration::ZERO)),
        }
    }

    fn set_step(&self, step: Duration) {
        *self.step.lock().expect("clock step lock") = step;
    }

    fn set_epoch(&self, epoch: i64) {
        self.epoch.store(epoch, Ordering::SeqCst);
    }

    fn advance(&self, by: Duration) {
        *self.elapsed.lock().expect("clock lock") += by;
    }

    fn elapsed(&self) -> Duration {
        *self.elapsed.lock().expect("clock lock")
    }
}

impl Clock for ManualClock {
    fn now_epoch_seconds(&self) -> i64 {
        self.epoch.load(Ordering::SeqCst)
    }

    fn monotonic_now(&self) -> Instant {
        let step = *self.step.lock().expect("clock step lock");
        let mut elapsed = self.elapsed.lock().expect("clock lock");
        let now = self.base + *elapsed;
        *elapsed += step;
        now
    }
}

#[derive(Clone, Copy)]
enum Behavior {
    Succeed,
    NeverComplete,
    /// Each page costs this long; the read fails once a request's timeout is reached.
    Pages {
        pages: usize,
        cost: Duration,
    },
    CompleteAfterExpiry,
}

/// A source that honors, or refuses, a bounded read and records what it was asked.
#[derive(Clone)]
struct FakeSource {
    snapshot: WorkspaceSnapshot,
    clock: ManualClock,
    bounded: bool,
    behavior: Arc<Mutex<Behavior>>,
    reads: Arc<AtomicUsize>,
    timeouts: Arc<Mutex<Vec<Duration>>>,
    /// How long describing the catalog evidence takes on the controlled clock.
    evidence_cost: Arc<Mutex<Duration>>,
}

impl FakeSource {
    fn new(clock: &ManualClock, bounded: bool) -> Self {
        Self {
            snapshot: WorkspaceSnapshot::default(),
            clock: clock.clone(),
            bounded,
            behavior: Arc::new(Mutex::new(Behavior::Succeed)),
            reads: Arc::new(AtomicUsize::new(0)),
            timeouts: Arc::new(Mutex::new(Vec::new())),
            evidence_cost: Arc::new(Mutex::new(Duration::ZERO)),
        }
    }

    fn evidence_takes(&self, cost: Duration) {
        *self.evidence_cost.lock().expect("evidence cost lock") = cost;
    }

    fn behave(&self, behavior: Behavior) {
        *self.behavior.lock().expect("behavior lock") = behavior;
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn timeouts(&self) -> Vec<Duration> {
        self.timeouts.lock().expect("timeouts lock").clone()
    }

    fn request(&self, deadline: &Deadline<'_>) -> Result<Duration, Error> {
        let timeout = deadline.request_timeout(Duration::from_secs(30))?;
        self.timeouts.lock().expect("timeouts lock").push(timeout);
        Ok(timeout)
    }
}

impl WorkspaceSource for FakeSource {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.snapshot.clone())
    }

    fn supports_bounded_read(&self) -> bool {
        self.bounded
    }

    fn catalog_evidence(&self) -> CatalogEvidence {
        let cost = *self.evidence_cost.lock().expect("evidence cost lock");
        self.clock.advance(cost);
        CatalogEvidence::default()
    }

    fn fetch_workspace_bounded(&self, deadline: &Deadline<'_>) -> Result<WorkspaceSnapshot, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let behavior = *self.behavior.lock().expect("behavior lock");
        match behavior {
            Behavior::Succeed => Ok(self.snapshot.clone()),
            Behavior::NeverComplete => {
                let timeout = self.request(deadline)?;
                self.clock.advance(timeout);
                Err(Error::ReadTimeout {
                    deadline: deadline.limit(),
                })
            }
            Behavior::Pages { pages, cost } => {
                for _ in 0..pages {
                    let timeout = self.request(deadline)?;
                    if cost >= timeout {
                        self.clock.advance(timeout);
                        return Err(Error::ReadTimeout {
                            deadline: deadline.limit(),
                        });
                    }
                    self.clock.advance(cost);
                }
                Ok(self.snapshot.clone())
            }
            Behavior::CompleteAfterExpiry => {
                self.clock.advance(deadline.limit() + SECOND);
                Ok(self.snapshot.clone())
            }
        }
    }
}

fn policy(max_age_seconds: u64, deadline: Duration) -> ReadPolicy {
    ReadPolicy {
        max_age: Duration::from_secs(max_age_seconds),
        deadline,
    }
}

fn open(
    source: &FakeSource,
    clock: &ManualClock,
    path: &Path,
) -> ReadService<FakeSource, ManualClock> {
    ReadService::with_clock(source.clone(), path, clock.clone()).expect("service opens")
}

fn cache_path(directory: &tempfile::TempDir) -> PathBuf {
    directory.path().join("cache.sqlite3")
}

fn rows(path: &Path) -> i64 {
    Connection::open(path)
        .expect("open cache")
        .query_row("SELECT count(*) FROM workspace_snapshot", [], |row| {
            row.get(0)
        })
        .expect("count rows")
}

/// Writes a bounded-read row. Evidence is complete when `started` is present: the entry
/// then carries the digest of its own payload, as a bounded read writes it.
fn write_row(path: &Path, fetched_at: i64, started: Option<i64>, started_for: Option<i64>) {
    write_row_with_digest(path, fetched_at, started, started_for, true);
}

fn write_row_with_digest(
    path: &Path,
    fetched_at: i64,
    started: Option<i64>,
    started_for: Option<i64>,
    digest: bool,
) {
    let snapshot = WorkspaceSnapshot {
        fetched_at,
        ..WorkspaceSnapshot::default()
    };
    let payload = serde_json::to_vec(&snapshot).expect("encode");
    let digest = (digest && started.is_some()).then(|| format!("{:x}", Sha256::digest(&payload)));
    Connection::open(path)
        .expect("open cache")
        .execute(
            "INSERT OR REPLACE INTO workspace_snapshot(
                 cache_key, fetched_at, payload, read_started_at, read_started_for,
                 read_started_digest)
             VALUES ('fixture-default', ?1, ?2, ?3, ?4, ?5)",
            params![fetched_at, payload, started, started_for, digest],
        )
        .expect("write row");
}

#[trace("FR-004-AC-11")]
#[test]
fn legacy_reads_keep_the_five_minute_lifetime_and_need_no_policy() {
    let clock = ManualClock::new(10_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    service.workspace_snapshot().expect("first read");
    clock.set_epoch(10_299);
    service.workspace_snapshot().expect("cached at 299 seconds");
    assert_eq!(source.reads(), 1);
    clock.set_epoch(10_300);
    service
        .workspace_snapshot()
        .expect("refresh at 300 seconds");
    assert_eq!(source.reads(), 2);
}

#[trace("FR-004-AC-12")]
#[test]
fn cached_data_is_accepted_by_the_age_of_its_source_read_and_over_age_is_refreshed() {
    let clock = ManualClock::new(1_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    let limits = policy(240, Duration::from_secs(5));
    let first = service
        .workspace_snapshot_bounded(&limits)
        .expect("cold read");
    assert_eq!((source.reads(), first.fetched_at), (1, 1_000));
    clock.set_epoch(1_200);
    let cached = service
        .workspace_snapshot_bounded(&limits)
        .expect("age 200");
    assert_eq!(source.reads(), 1, "age 200 <= 240 needs no source read");
    assert_eq!(cached.fetched_at, 1_000, "the actual source fetch time");
    clock.set_epoch(1_250);
    let refreshed = service
        .workspace_snapshot_bounded(&limits)
        .expect("age 250");
    assert_eq!((source.reads(), refreshed.fetched_at), (2, 1_250));
    for invalid in [
        policy(301, Duration::from_secs(5)),
        policy(u64::MAX, Duration::from_secs(5)),
        policy(240, Duration::ZERO),
    ] {
        assert!(matches!(
            service.workspace_snapshot_bounded(&invalid),
            Err(Error::InvalidReadPolicy { .. })
        ));
    }
    assert_eq!(source.reads(), 2, "an invalid policy reads nothing");
    assert!(
        service
            .workspace_snapshot_bounded(&policy(300, Duration::from_secs(5)))
            .is_ok()
    );
}

#[trace("FR-004-AC-13")]
#[test]
fn age_is_measured_from_the_source_read_not_the_cache_write() {
    let clock = ManualClock::new(2_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let path = cache_path(&directory);
    let mut service = open(&source, &clock, &path);
    let limits = policy(240, Duration::from_secs(5));
    // Source read began 250 s ago, but the entry was written 10 s ago.
    write_row(&path, 1_990, Some(1_750), Some(1_990));
    service
        .workspace_snapshot_bounded(&limits)
        .expect("refresh");
    assert_eq!(source.reads(), 1, "age by source read start refreshes");
    // An entry with no evidence of its source read start is refreshed, never accepted.
    write_row(&path, 1_990, None, None);
    service
        .workspace_snapshot_bounded(&limits)
        .expect("refresh");
    assert_eq!(source.reads(), 2);
    // Evidence that belongs to a different payload is not trusted.
    write_row(&path, 1_990, Some(1_900), Some(1_980));
    service
        .workspace_snapshot_bounded(&limits)
        .expect("refresh");
    assert_eq!(source.reads(), 3);
    // Valid evidence within the age is returned with its own fetch time.
    write_row(&path, 1_800, Some(1_800), Some(1_800));
    let cached = service.workspace_snapshot_bounded(&limits).expect("cached");
    assert_eq!((source.reads(), cached.fetched_at), (3, 1_800));
    // Timestamps that agree are not enough: evidence without the digest of its payload
    // cannot show that it describes this payload, so it is refreshed.
    write_row_with_digest(&path, 1_800, Some(1_800), Some(1_800), false);
    service
        .workspace_snapshot_bounded(&limits)
        .expect("refresh");
    assert_eq!(source.reads(), 4);
}

#[trace("FR-004-AC-11", "FR-004-AC-13")]
#[test]
fn old_and_new_cache_entries_coexist_with_old_readers_and_writers() {
    let clock = ManualClock::new(3_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let path = cache_path(&directory);
    let mut service = open(&source, &clock, &path);
    let limits = policy(240, Duration::from_secs(5));
    // A legacy entry has no read-start evidence, so a bounded read refreshes it.
    service.workspace_snapshot().expect("legacy read");
    service
        .workspace_snapshot_bounded(&limits)
        .expect("bounded read");
    assert_eq!(source.reads(), 2);
    // A bounded entry still serves a legacy read without another source read.
    service
        .workspace_snapshot()
        .expect("legacy read of bounded entry");
    assert_eq!(source.reads(), 2);
    // An older strict reader sees the original columns and an undecorated payload.
    let connection = Connection::open(&path).expect("open cache");
    let (fetched_at, payload): (i64, Vec<u8>) = connection
        .query_row(
            "SELECT fetched_at, payload FROM workspace_snapshot WHERE cache_key = ?1",
            params!["fixture-default"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("old reader query");
    let strict: WorkspaceSnapshot = serde_json::from_slice(&payload).expect("strict decode");
    assert_eq!(strict.fetched_at, fetched_at);
    // An older writer replaces the payload without the evidence columns; the bounded
    // read must not trust the evidence that no longer describes the payload.
    connection
        .execute(
            "INSERT INTO workspace_snapshot(cache_key, fetched_at, payload) VALUES (?1, ?2, ?3)
             ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at,
               payload = excluded.payload",
            params!["fixture-default", 3_005_i64, payload],
        )
        .expect("old writer");
    clock.set_epoch(3_006);
    service
        .workspace_snapshot_bounded(&limits)
        .expect("bounded read");
    assert_eq!(source.reads(), 3);
}

#[trace("FR-004-AC-11")]
#[test]
fn a_cache_created_before_the_evidence_columns_still_opens_and_serves_legacy_reads() {
    let directory = tempfile::tempdir().expect("dir");
    let path = cache_path(&directory);
    let snapshot = WorkspaceSnapshot {
        fetched_at: 4_990,
        ..WorkspaceSnapshot::default()
    };
    Connection::open(&path)
        .expect("create old cache")
        .execute_batch(
            "CREATE TABLE workspace_snapshot (
                 cache_key TEXT PRIMARY KEY, fetched_at INTEGER NOT NULL, payload BLOB NOT NULL);",
        )
        .expect("old schema");
    Connection::open(&path)
        .expect("open old cache")
        .execute(
            "INSERT INTO workspace_snapshot VALUES ('fixture-default', 4990, ?1)",
            params![serde_json::to_vec(&snapshot).expect("encode")],
        )
        .expect("old row");
    let clock = ManualClock::new(5_000);
    let source = FakeSource::new(&clock, true);
    let mut service = open(&source, &clock, &path);
    service
        .workspace_snapshot()
        .expect("legacy read of an old entry");
    assert_eq!(
        source.reads(),
        0,
        "the old entry is still fresh for legacy reads"
    );
    service
        .workspace_snapshot_bounded(&policy(240, Duration::from_secs(5)))
        .expect("bounded read refreshes it");
    assert_eq!(source.reads(), 1);
}

#[trace("FR-004-AC-14")]
#[test]
fn requests_never_start_after_the_deadline_and_each_is_clamped_to_the_time_left() {
    let clock = ManualClock::new(6_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    let limits = policy(240, Duration::from_secs(5));
    source.behave(Behavior::NeverComplete);
    assert!(matches!(
        service.workspace_snapshot_bounded(&limits),
        Err(Error::ReadTimeout { deadline }) if deadline == Duration::from_secs(5)
    ));
    assert_eq!(
        clock.elapsed(),
        Duration::from_secs(5),
        "exactly the deadline"
    );
    assert_eq!(source.timeouts(), [Duration::from_secs(5)]);
    // Pages that each cost 2 s: the requests carry 5 s, 3 s and 1 s, and a fourth
    // never starts.
    let clock = ManualClock::new(6_000);
    let source = FakeSource::new(&clock, true);
    // The directory must outlive the service, or the cache file vanishes mid-read.
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    source.behave(Behavior::Pages {
        pages: 10,
        cost: Duration::from_secs(2),
    });
    assert!(matches!(
        service.workspace_snapshot_bounded(&limits),
        Err(Error::ReadTimeout { .. })
    ));
    assert_eq!(
        source.timeouts(),
        [Duration::from_secs(5), Duration::from_secs(3), SECOND]
    );
    assert_eq!(clock.elapsed(), Duration::from_secs(5));
    // With plenty of time the legacy 30-second request timeout still caps each request.
    let clock = ManualClock::new(6_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    source.behave(Behavior::Pages {
        pages: 1,
        cost: SECOND,
    });
    service
        .workspace_snapshot_bounded(&policy(240, Duration::from_secs(600)))
        .expect("read");
    assert_eq!(source.timeouts(), [Duration::from_secs(30)]);
}

#[trace("FR-004-AC-16")]
#[test]
fn a_late_completion_changes_neither_the_cache_nor_a_later_result() {
    let clock = ManualClock::new(7_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let path = cache_path(&directory);
    let mut service = open(&source, &clock, &path);
    let limits = policy(240, Duration::from_secs(5));
    source.behave(Behavior::CompleteAfterExpiry);
    assert!(matches!(
        service.workspace_snapshot_bounded(&limits),
        Err(Error::ReadTimeout { .. })
    ));
    assert_eq!(rows(&path), 0, "nothing is stored as fresh after expiry");
    source.behave(Behavior::Succeed);
    service
        .workspace_snapshot_bounded(&limits)
        .expect("later read");
    assert_eq!(
        source.reads(),
        2,
        "the late result was not served from the cache"
    );
    assert_eq!(rows(&path), 1);
}

#[trace("FR-004-AC-17")]
#[test]
fn a_source_that_cannot_honor_a_bound_fails_before_any_read_and_legacy_reads_still_work() {
    let clock = ManualClock::new(8_000);
    let source = FakeSource::new(&clock, false);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    let limits = policy(240, Duration::from_secs(5));
    assert!(matches!(
        service.workspace_snapshot_bounded(&limits),
        Err(Error::BoundUnsupported)
    ));
    assert_eq!(source.reads(), 0);
    // Even with a fresh entry in the cache the unsupported bound is refused, not ignored.
    service
        .workspace_snapshot()
        .expect("legacy read still works");
    assert_eq!(source.reads(), 1);
    assert!(matches!(
        service.workspace_snapshot_bounded(&limits),
        Err(Error::BoundUnsupported)
    ));
    assert_eq!(source.reads(), 1);
}

#[trace("FR-004-AC-19")]
#[test]
fn a_locked_cache_times_out_within_the_deadline_without_reading_the_source() {
    let clock = ManualClock::new(9_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let path = cache_path(&directory);
    // The default clock is real, so the lock wait is genuinely bounded by the deadline.
    let mut service = ReadService::open(source.clone(), &path).expect("service opens");
    let blocker = Connection::open(&path).expect("second connection");
    blocker
        .execute_batch("BEGIN EXCLUSIVE")
        .expect("hold the lock");
    let deadline = Duration::from_millis(400);
    let started = Instant::now();
    let result = service.workspace_snapshot_bounded(&policy(240, deadline));
    let elapsed = started.elapsed();
    assert!(
        matches!(result, Err(Error::ReadTimeout { .. })),
        "{result:?}"
    );
    assert!(
        elapsed <= deadline + Duration::from_millis(250),
        "returned after {elapsed:?}"
    );
    assert_eq!(source.reads(), 0);
    blocker.execute_batch("ROLLBACK").expect("release");
    service
        .workspace_snapshot_bounded(&policy(240, Duration::from_secs(5)))
        .expect("read once the lock is free");
    assert_eq!(rows(&path), 1);
}

/// What a loopback server should do with one request.
enum Reply {
    Json(Value),
    Status(u16, Vec<(&'static str, String)>, Value),
    Hang,
}

struct Seen {
    query: String,
    authorization: String,
}

struct Server {
    endpoint: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    fn start(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking accept");
        let endpoint = format!("http://{}/graphql", listener.local_addr().expect("address"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let (seen_for_thread, stop_for_thread) = (Arc::clone(&seen), Arc::clone(&stop));
        let accept = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop_for_thread.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (handler, seen, stop) = (
                            Arc::clone(&handler),
                            Arc::clone(&seen_for_thread),
                            Arc::clone(&stop_for_thread),
                        );
                        workers.push(std::thread::spawn(move || {
                            serve(stream, &*handler, &seen, &stop);
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
        Self {
            endpoint,
            seen,
            stop,
            accept: Some(accept),
        }
    }

    fn requests(&self) -> usize {
        self.seen.lock().expect("seen lock").len()
    }

    fn queries(&self) -> Vec<String> {
        self.seen
            .lock()
            .expect("seen lock")
            .iter()
            .map(|seen| seen.query.clone())
            .collect()
    }

    fn authorizations(&self) -> Vec<String> {
        self.seen
            .lock()
            .expect("seen lock")
            .iter()
            .map(|seen| seen.authorization.clone())
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

fn serve(
    mut stream: TcpStream,
    handler: &(impl Fn(&str) -> Reply + ?Sized),
    seen: &Mutex<Vec<Seen>>,
    stop: &AtomicBool,
) {
    stream.set_nonblocking(false).expect("blocking stream");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    let (header_end, length, authorization) = loop {
        let size = stream.read(&mut buffer).unwrap_or(0);
        if size == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..size]);
        let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..end]).into_owned();
        let header = |name: &str| {
            headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case(name)
                        .then(|| value.trim().to_owned())
                })
                .unwrap_or_default()
        };
        break (
            end + 4,
            header("content-length").parse().unwrap_or(0),
            header("authorization"),
        );
    };
    while request.len() < header_end + length {
        let size = stream.read(&mut buffer).unwrap_or(0);
        if size == 0 {
            return;
        }
        request.extend_from_slice(&buffer[..size]);
    }
    let body: Value = serde_json::from_slice(&request[header_end..header_end + length])
        .expect("a GraphQL request body");
    let query = body["query"].as_str().unwrap_or_default().to_owned();
    let reply = handler(&query);
    seen.lock().expect("seen lock").push(Seen {
        query,
        authorization,
    });
    let (status, headers, body) = match reply {
        Reply::Json(body) => (200, Vec::new(), body),
        Reply::Status(status, headers, body) => (status, headers, body),
        Reply::Hang => {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            return;
        }
    };
    let body = body.to_string();
    let extra = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn page(nodes: Value, next: Option<&str>) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next}})
}

fn projects(nodes: Value, next: Option<&str>) -> Reply {
    Reply::Json(json!({"data": {"projects": page(nodes, next)}}))
}

fn issues(nodes: Value, next: Option<&str>) -> Reply {
    Reply::Json(json!({"data": {"issues": page(nodes, next)}}))
}

struct FakeCredentials {
    value: Result<&'static str, SecretError>,
    delay: Duration,
    reads: Arc<AtomicUsize>,
}

impl CredentialBackend for FakeCredentials {
    fn get(&self, scope: &AppScope, key: &SecretKey) -> Result<Option<SecretValue>, SecretError> {
        assert_eq!(scope.as_str(), "ix-board");
        assert_eq!(key.as_str(), "linear-api-key");
        std::thread::sleep(self.delay);
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.value.map(|value| Some(SecretValue::new(value)))
    }

    fn set(&self, _: &AppScope, _: &SecretKey, _: &SecretValue) -> Result<(), SecretError> {
        panic!("a read must not write credentials")
    }

    fn delete(&self, _: &AppScope, _: &SecretKey) -> Result<DeleteStatus, SecretError> {
        panic!("a read must not delete credentials")
    }

    fn status(&self, _: &AppScope, _: &SecretKey) -> Result<Presence, SecretError> {
        panic!("a read must not probe credential status")
    }
}

fn no_fallback() -> Option<String> {
    panic!("the CLI fallback must not run for a saved key or a credential failure")
}

fn linear_source(
    endpoint: &str,
    value: Result<&'static str, SecretError>,
    delay: Duration,
) -> (Result<LinearGraphQlSource, Error>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let store = SecretStore::new(FakeCredentials {
        value,
        delay,
        reads: Arc::clone(&reads),
    });
    // `None` is the environment snapshot: the process environment is never consulted.
    (
        LinearGraphQlSource::from_credential_store(&store, None, no_fallback, endpoint),
        reads,
    )
}

fn linear_service(
    server: &Server,
    directory: &tempfile::TempDir,
) -> ReadService<LinearGraphQlSource> {
    let (source, _) = linear_source(&server.endpoint, Ok("fixture-saved-key"), Duration::ZERO);
    ReadService::open(
        source.expect("fake credentials resolve"),
        &cache_path(directory),
    )
    .expect("service opens")
}

const SLACK: Duration = Duration::from_millis(250);

#[trace("FR-004-AC-14", "FR-004-AC-16")]
#[test]
fn a_page_that_never_completes_times_out_within_the_deadline_with_no_work_left_behind() {
    let pages = AtomicUsize::new(0);
    let server = Server::start(move |query| {
        if query.contains("query WorkspaceProjects") && pages.fetch_add(1, Ordering::SeqCst) == 0 {
            projects(json!([]), Some("cursor-1"))
        } else {
            Reply::Hang
        }
    });
    let directory = tempfile::tempdir().expect("dir");
    let mut service = linear_service(&server, &directory);
    let deadline = Duration::from_millis(700);
    let started = Instant::now();
    let result = service.workspace_snapshot_bounded(&policy(240, deadline));
    let elapsed = started.elapsed();
    assert!(
        matches!(result, Err(Error::ReadTimeout { .. })),
        "{result:?}"
    );
    assert!(
        elapsed <= deadline + SLACK,
        "{elapsed:?} must not add a further request timeout to {deadline:?}"
    );
    assert_eq!(
        server.requests(),
        2,
        "the first page, then the page that hangs"
    );
    assert_eq!(rows(&cache_path(&directory)), 0, "nothing stored as fresh");
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        server.requests(),
        2,
        "no retry or request after the call returned"
    );
}

#[trace("FR-004-AC-14")]
#[test]
fn a_nested_relation_read_is_inside_the_same_deadline() {
    let server = Server::start(|query| {
        if query.contains("query WorkspaceProjects") {
            projects(json!([]), None)
        } else if query.contains("query WorkspaceIssues") {
            let issue = json!({
                "id": "i1", "identifier": "SWM-1", "title": "t", "description": null,
                "priority": 2.0, "sortOrder": 1.0,
                "state": {"name": "Todo", "type": "unstarted"}, "team": {"key": "SWM"},
                "project": null,
                "relations": page(json!([]), Some("relations-1")),
                "inverseRelations": page(json!([]), None)
            });
            issues(json!([issue]), None)
        } else {
            Reply::Hang
        }
    });
    let directory = tempfile::tempdir().expect("dir");
    let mut service = linear_service(&server, &directory);
    let deadline = Duration::from_millis(700);
    let started = Instant::now();
    let result = service.workspace_snapshot_bounded(&policy(240, deadline));
    assert!(
        matches!(result, Err(Error::ReadTimeout { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() <= deadline + SLACK);
    assert_eq!(
        server.requests(),
        3,
        "projects, issues, then the nested relations page"
    );
}

#[trace("FR-004-AC-18")]
#[test]
fn a_rate_limit_is_the_typed_error_with_its_cooldown_and_is_never_retried() {
    let server = Server::start(|_| {
        Reply::Status(
            400,
            vec![
                ("X-RateLimit-Requests-Remaining", "0".into()),
                ("X-RateLimit-Requests-Reset", "1".into()),
            ],
            json!({"errors": [{"message": "quota exceeded",
                "extensions": {"code": "RATELIMITED", "type": "extra server metadata"}}]}),
        )
    });
    let directory = tempfile::tempdir().expect("dir");
    let mut service = linear_service(&server, &directory);
    let started = Instant::now();
    let result = service.workspace_snapshot_bounded(&policy(240, Duration::from_secs(10)));
    let Err(Error::LinearRateLimited { retry_after }) = &result else {
        panic!("a rate limit must be the typed error, got {result:?}");
    };
    assert_eq!(*retry_after, Duration::from_secs(60));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "no sleep or retry"
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(server.requests(), 1);
}

#[trace("FR-004-AC-15")]
#[test]
fn the_budget_starts_after_credentials_are_ready_and_credentials_are_never_resolved_again() {
    // The conflicting environment is supplied to a child process so this process is not
    // mutated; the injected provider must still be the only credential source.
    const CHILD: &str = "IX_BOARD_BOUNDED_READ_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "the_budget_starts_after_credentials_are_ready_and_credentials_are_never_resolved_again",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("LINEAR_API_KEY", "fixture-global-env-must-not-be-read")
            .output()
            .expect("spawn child");
        assert!(
            output.status.success(),
            "child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let server = Server::start(|query| {
        if query.contains("query WorkspaceProjects") {
            projects(json!([]), None)
        } else {
            issues(json!([]), None)
        }
    });
    let credential_wait = Duration::from_millis(600);
    let deadline = Duration::from_millis(300);
    let (source, reads) = linear_source(&server.endpoint, Ok("fixture-saved-key"), credential_wait);
    let source = source.expect("the fake provider resolves the key");
    let directory = tempfile::tempdir().expect("dir");
    let mut service = ReadService::open(source, &cache_path(&directory)).expect("service opens");
    let snapshot = service
        .workspace_snapshot_bounded(&policy(240, deadline))
        .expect("the full budget is available after the credential wait");
    assert!(snapshot.fetched_at > 0);
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "credentials resolved once, before the read"
    );
    assert_eq!(
        server.authorizations(),
        ["fixture-saved-key", "fixture-saved-key"],
        "the injected key, not the environment, authenticates both requests"
    );
    assert_eq!(server.queries().len(), 2);
}

#[trace("FR-004-AC-15")]
#[test]
fn a_credential_failure_is_the_existing_error_and_nothing_is_read_or_retried() {
    let server = Server::start(|_| Reply::Hang);
    for failure in [SecretError::Locked, SecretError::Unavailable] {
        let (source, reads) = linear_source(&server.endpoint, Err(failure), Duration::ZERO);
        assert!(matches!(
            source,
            Err(Error::Credential(actual)) if actual == failure
        ));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
    assert_eq!(server.requests(), 0);
}

/// One project of `issues` open leaves, enough work that evaluating it checks the budget
/// many times.
fn workload(issues: usize) -> WorkspaceSnapshot {
    let project = Project {
        id: ProjectId("p1".into()),
        identifier: Some("PRJ-p1".into()),
        name: "Project p1".into(),
        updated_at: None,
        description: String::new(),
        content: None,
        status: "Started".into(),
        status_name: None,
        status_category: ProjectStatusCategory::Started,
        priority: 2,
        sort_order: 0.0,
        target_date: None,
        team_keys: vec![TeamKey("SWM".into())],
        milestones: vec![],
        initiatives: vec![],
    };
    let issues = (0..issues)
        .map(|index| Issue {
            id: IssueId(format!("SWM-{index}")),
            identifier: format!("SWM-{index}"),
            title: format!("Issue {index}"),
            description: None,
            state: "Specced".into(),
            state_name: "Specced".into(),
            state_type: WorkflowStateType::Unstarted,
            parent_id: None,
            team_key: TeamKey("SWM".into()),
            assignee_id: None,
            assignee_name: None,
            project_id: Some(ProjectId("p1".into())),
            project_milestone_id: None,
            priority: 2.0,
            sort_order: 0.0,
            blocked_by: vec![],
            created_at: None,
            updated_at: None,
            attachment_urls: vec![],
            attachments_complete: true,
        })
        .collect();
    WorkspaceSnapshot {
        projects: vec![project],
        issues,
        fetched_at: 0,
    }
}

fn project_batch() -> Vec<NamedFocusRequest> {
    vec![NamedFocusRequest {
        key: "focus".into(),
        selection: NamedSelection::new(
            TeamKey("SWM".into()),
            vec![NamedMember::Project { id: "p1".into() }],
            vec![],
        ),
    }]
}

fn is_timeout(result: &Result<impl std::fmt::Debug, Error>, deadline: Duration) -> bool {
    matches!(result, Err(Error::ReadTimeout { deadline: actual }) if *actual == deadline)
}

#[trace("FR-004-AC-14", "FR-004-AC-16")]
#[test]
fn a_cache_hit_that_outlives_the_deadline_returns_the_timeout_not_the_snapshot() {
    let clock = ManualClock::new(11_000);
    let mut source = FakeSource::new(&clock, true);
    source.snapshot = workload(400);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    let limits = policy(240, Duration::from_secs(1));
    service
        .workspace_snapshot_bounded(&limits)
        .expect("a cold read fills the cache");
    assert_eq!(source.reads(), 1);
    // From here every look at the clock costs 600 ms, so the one second passes while the
    // cached entry is looked up, copied and decoded.
    clock.set_step(Duration::from_millis(600));
    let result = service.workspace_snapshot_bounded(&limits);
    assert!(
        is_timeout(&result, Duration::from_secs(1)),
        "an expired cache hit is the typed timeout, never the snapshot: {:?}",
        result.map(|snapshot| snapshot.issues.len())
    );
    assert_eq!(
        source.reads(),
        1,
        "an expired cache hit starts no source read"
    );
    // The entry is untouched and still serves a read that has time to spare.
    clock.set_step(Duration::ZERO);
    let served = service
        .workspace_snapshot_bounded(&limits)
        .expect("served from the cache");
    assert_eq!((source.reads(), served.issues.len()), (1, 400));
}

#[trace("FR-004-AC-14", "FR-012-AC-19")]
#[test]
fn the_composed_batch_keeps_one_budget_through_evidence_and_evaluation() {
    let limit = Duration::from_secs(5);
    // The source's catalog evidence consumes the rest of the budget after the read.
    let clock = ManualClock::new(12_000);
    let mut source = FakeSource::new(&clock, true);
    source.snapshot = workload(3);
    source.evidence_takes(Duration::from_secs(6));
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    let result = service.named_batch_bounded(&policy(240, limit), &project_batch());
    assert!(
        is_timeout(&result, limit),
        "no publishable batch after expiry: {result:?}"
    );
    assert_eq!(source.reads(), 1);

    // A cold read followed by an evaluation that keeps checking the same budget: each
    // look at the clock costs 100 ms of the one second, so the evaluation of a large
    // snapshot runs the budget out and the batch is refused rather than returned late.
    let clock = ManualClock::new(12_000);
    let mut source = FakeSource::new(&clock, true);
    source.snapshot = workload(60);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    let limits = policy(240, Duration::from_secs(1));
    clock.set_step(Duration::from_millis(100));
    let result = service.named_batch_bounded(&limits, &project_batch());
    assert!(
        is_timeout(&result, Duration::from_secs(1)),
        "a refresh near the deadline cannot evaluate past it: {result:?}"
    );

    // The same evaluation after a cache hit is under the same single budget.
    let clock = ManualClock::new(12_000);
    let mut source = FakeSource::new(&clock, true);
    source.snapshot = workload(60);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    service
        .workspace_snapshot_bounded(&limits)
        .expect("a cold read fills the cache");
    clock.set_step(Duration::from_millis(100));
    let result = service.named_batch_bounded(&limits, &project_batch());
    assert!(
        is_timeout(&result, Duration::from_secs(1)),
        "a cache hit near the deadline cannot evaluate past it: {result:?}"
    );
    assert_eq!(source.reads(), 1, "the cache hit read nothing");

    // With time to spare the same call returns the complete batch for that one snapshot.
    clock.set_step(Duration::ZERO);
    let batch = service
        .named_batch_bounded(&limits, &project_batch())
        .expect("the batch with a budget to spare");
    assert_eq!(batch.selections[0].counts.total, 60);
    assert_eq!(source.reads(), 1);
}

#[trace("FR-004-AC-13")]
#[test]
fn an_old_writer_replacing_the_payload_in_the_same_second_voids_the_evidence() {
    let clock = ManualClock::new(13_000);
    let source = FakeSource::new(&clock, true);
    let directory = tempfile::tempdir().expect("dir");
    let path = cache_path(&directory);
    let mut service = open(&source, &clock, &path);
    let limits = policy(240, Duration::from_secs(5));
    service
        .workspace_snapshot_bounded(&limits)
        .expect("a cold read writes evidence");
    assert_eq!(source.reads(), 1);
    // An older writer stores a different payload at the very same `fetched_at` and
    // leaves the evidence columns as they were. The clock has not moved, so nothing but
    // the payload itself can show that the evidence no longer describes it.
    let foreign = WorkspaceSnapshot {
        fetched_at: 13_000,
        ..workload(1)
    };
    Connection::open(&path)
        .expect("open cache")
        .execute(
            "INSERT INTO workspace_snapshot(cache_key, fetched_at, payload) VALUES (?1, ?2, ?3)
             ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at,
               payload = excluded.payload",
            params![
                "fixture-default",
                13_000_i64,
                serde_json::to_vec(&foreign).expect("encode")
            ],
        )
        .expect("old writer");
    let refreshed = service
        .workspace_snapshot_bounded(&limits)
        .expect("bounded read");
    assert_eq!(
        source.reads(),
        2,
        "evidence of another payload is refreshed"
    );
    assert!(
        refreshed.issues.is_empty(),
        "the old writer's payload is not served as bounded data"
    );
    // The refresh wrote new evidence, so the next read is a cache hit again.
    service
        .workspace_snapshot_bounded(&limits)
        .expect("cached read");
    assert_eq!(source.reads(), 2);
}
