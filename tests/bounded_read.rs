// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! The opt-in bounded workspace read (FR-004-AC-11 through FR-004-AC-19).
//!
//! Credentials come only from an injected fake store and every HTTP request goes to a
//! loopback server; nothing here reads the process environment or a real credential.

use ix_board::{
    CatalogEvidence, Clock, Deadline, Error, Initiative, Issue, IssueId, LinearGraphQlSource,
    MemberOutcome, Milestone, NamedFocusRequest, NamedList, NamedMember, NamedMemberInput,
    NamedSelection, NamedSelectionInput, Project, ProjectId, ProjectStatusCategory, ReadPolicy,
    ReadService, ResolvedNamedSelection, TeamKey, WorkflowStateType, WorkspaceSnapshot,
    WorkspaceSource, canonicalize_named_selection,
};
use ix_cli_kit::secrets::{
    AppScope, CredentialBackend, DeleteStatus, Presence, SecretError, SecretKey, SecretStore,
    SecretValue,
};
use ix_trace_rs::trace;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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

/// `projects` projects of `per_project` open leaves each, all held by initiative `i1`.
fn initiative_workload(projects: usize, per_project: usize) -> WorkspaceSnapshot {
    let template = workload(0).projects.remove(0);
    let initiative = Initiative {
        id: "i1".into(),
        identifier: "I-1".into(),
        name: "Initiative one".into(),
    };
    let mut snapshot = WorkspaceSnapshot::default();
    for project in 0..projects {
        let id = ProjectId(format!("p{project}"));
        snapshot.projects.push(Project {
            id: id.clone(),
            identifier: Some(format!("PRJ-{project}")),
            initiatives: vec![initiative.clone()],
            ..template.clone()
        });
        for index in 0..per_project {
            let mut issue = workload(1).issues.remove(0);
            issue.id = IssueId(format!("SWM-{project}-{index}"));
            issue.identifier = format!("SWM-{project}-{index}");
            issue.project_id = Some(id.clone());
            snapshot.issues.push(issue);
        }
    }
    snapshot
}

fn initiative_batch() -> Vec<NamedFocusRequest> {
    vec![NamedFocusRequest {
        key: "focus".into(),
        selection: NamedSelection::new(
            TeamKey("SWM".into()),
            vec![NamedMember::Initiative { id: "i1".into() }],
            vec![],
        ),
    }]
}

/// One composed read of `snapshot` where every look at the clock costs [`WORK_STEP`].
/// Returns the leaf total or the error, and how much controlled time the call used.
fn timed_initiative_batch(
    snapshot: WorkspaceSnapshot,
    deadline: Duration,
    from_cache: bool,
) -> (Result<usize, Error>, Duration) {
    let clock = ManualClock::new(14_000);
    let mut source = FakeSource::new(&clock, true);
    source.snapshot = snapshot;
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&source, &clock, &cache_path(&directory));
    if from_cache {
        service
            .workspace_snapshot_bounded(&policy(240, Duration::from_secs(3600)))
            .expect("a cold read fills the cache");
    }
    clock.set_step(WORK_STEP);
    let before = clock.elapsed();
    let result = service
        .named_batch_bounded(&policy(240, deadline), &initiative_batch())
        .map(|batch| batch.selections[0].counts.total);
    (result, clock.elapsed() - before)
}

/// The cost of one cooperative deadline check in the placement test below.
const WORK_STEP: Duration = Duration::from_micros(100);

#[trace("FR-004-AC-14", "FR-012-AC-19")]
#[test]
fn row_count_and_placement_work_over_many_projects_stops_at_the_shared_deadline() {
    // Phase evidence without assuming how many times the engine reads the clock. Both
    // snapshots hold the same 300 leaves, so decoding, context building, selector
    // validation, closure and readiness work cost the same. They differ only in the
    // placement phase (per-project rows, counts and reasons): one project of 300 leaves
    // against 30 projects of 10, where each project's row scans every selected leaf.
    // Unchecked work consumes no controlled time, so a deadline that only the
    // surrounding checks observe would let the many-project batch return late.
    for from_cache in [false, true] {
        let generous = Duration::from_secs(3600);
        let (one, one_used) =
            timed_initiative_batch(initiative_workload(1, 300), generous, from_cache);
        let (many, many_used) =
            timed_initiative_batch(initiative_workload(30, 10), generous, from_cache);
        assert_eq!(one.expect("one project"), 300);
        assert_eq!(many.expect("many projects"), 300);
        assert!(
            many_used > one_used * 2,
            "placement must dominate the many-project batch: {many_used:?} vs {one_used:?}"
        );

        // A deadline between the two: everything before placement, and the cheap
        // placement of one project, fit; the many-project placement does not.
        let deadline = one_used + (many_used - one_used) / 2;
        let (fits, _) = timed_initiative_batch(initiative_workload(1, 300), deadline, from_cache);
        assert_eq!(fits.expect("same cache, source and context cost fits"), 300);

        let (result, used) =
            timed_initiative_batch(initiative_workload(30, 10), deadline, from_cache);
        assert!(
            is_timeout(&result, deadline),
            "placement past the one deadline is the typed timeout (cache {from_cache}): {result:?}"
        );
        assert!(
            used <= deadline + WORK_STEP * 100,
            "placement stops soon after expiry, not after the whole traversal: \
             {used:?} against {deadline:?} (full run {many_used:?})"
        );
    }
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

fn named_project(id: &str, identifier: &str, name: &str, team: &str) -> Project {
    Project {
        id: ProjectId(id.into()),
        identifier: Some(identifier.into()),
        name: name.into(),
        team_keys: vec![TeamKey(team.into())],
        initiatives: vec![Initiative {
            id: "uuid-init".into(),
            identifier: "I-10".into(),
            name: "Delivery".into(),
        }],
        ..workload(0).projects.remove(0)
    }
}

fn milestone_record(id: &str, name: &str) -> Milestone {
    Milestone {
        id: id.into(),
        name: name.into(),
        description: None,
        sort_order: 1.0,
        target_date: None,
    }
}

fn ticket(id: &str, identifier: &str, team: &str, project: &str, parent: Option<&str>) -> Issue {
    Issue {
        id: IssueId(id.into()),
        identifier: identifier.into(),
        team_key: TeamKey(team.into()),
        project_id: Some(ProjectId(project.into())),
        parent_id: parent.map(|id| IssueId(id.into())),
        ..workload(1).issues.remove(0)
    }
}

/// Two SWM projects holding same-named milestones and a project of another team.
fn naming_snapshot() -> WorkspaceSnapshot {
    let mut alpha = named_project("uuid-alpha", "PRJ-1", "Project / Alpha", "SWM");
    alpha.milestones = vec![
        milestone_record("uuid-m1", "Derived W/M operators"),
        milestone_record("uuid-m2", "Beta"),
    ];
    let mut other = named_project("uuid-other", "PRJ-2", "Other", "SWM");
    other.milestones = vec![milestone_record("uuid-m3", "Beta")];
    let mut foreign = named_project("uuid-foreign", "PRJ-3", "Foreign", "AGE");
    foreign.initiatives.clear();
    WorkspaceSnapshot {
        projects: vec![alpha, other, foreign],
        issues: vec![
            ticket("uuid-t1", "SWM-1", "SWM", "uuid-alpha", None),
            ticket("uuid-t2", "SWM-2", "SWM", "uuid-alpha", Some("uuid-t1")),
            ticket("uuid-t3", "AGE-1", "AGE", "uuid-foreign", None),
        ],
        fetched_at: 0,
    }
}

fn milestone_input(selector: &str, project: Option<&str>) -> NamedMemberInput {
    NamedMemberInput::Milestone {
        selector: selector.into(),
        project: project.map(str::to_owned),
    }
}

fn named_input(
    include: Vec<NamedMemberInput>,
    exclude: Vec<NamedMemberInput>,
) -> NamedSelectionInput {
    NamedSelectionInput {
        team: "SWM".into(),
        include,
        exclude,
    }
}

fn naming_service(
    snapshot: WorkspaceSnapshot,
    clock: &ManualClock,
    directory: &tempfile::TempDir,
) -> (ReadService<FakeSource, ManualClock>, FakeSource) {
    let mut source = FakeSource::new(clock, true);
    source.snapshot = snapshot;
    (open(&source, clock, &cache_path(directory)), source)
}

#[trace("FR-016-AC-11", "FR-004-AC-14")]
#[test]
fn composed_canonicalization_resolves_every_member_kind_from_a_cold_read_and_a_warm_cache() {
    let clock = ManualClock::new(15_000);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(naming_snapshot(), &clock, &directory);
    let limits = policy(240, Duration::from_secs(5));
    let input = named_input(
        vec![
            NamedMemberInput::Initiative("i-10".into()),
            NamedMemberInput::Project("Project / Alpha".into()),
            milestone_input("Project / Alpha/Derived W/M operators", None),
            milestone_input("PRJ-1/Beta", Some("PRJ-1")),
            NamedMemberInput::Ticket("swm-1".into()),
        ],
        vec![
            NamedMemberInput::Project("Other".into()),
            NamedMemberInput::Ticket("SWM-2".into()),
        ],
    );
    let cold = service
        .canonicalize_named_selection_bounded(&limits, &input)
        .expect("cold read then canonicalization");
    assert_eq!(source.reads(), 1);
    let milestone = |id: &str| NamedMember::Milestone {
        id: id.into(),
        project_id: "uuid-alpha".into(),
    };
    assert_eq!(
        cold.include,
        [
            NamedMember::Initiative {
                id: "uuid-init".into()
            },
            NamedMember::Project {
                id: "uuid-alpha".into()
            },
            milestone("uuid-m1"),
            milestone("uuid-m2"),
            NamedMember::Issue {
                id: "uuid-t1".into()
            },
        ]
    );
    assert_eq!(
        cold.exclude,
        [
            NamedMember::Project {
                id: "uuid-other".into()
            },
            NamedMember::Issue {
                id: "uuid-t2".into()
            },
        ]
    );
    assert_eq!(
        cold,
        canonicalize_named_selection(&naming_snapshot(), &input).expect("standalone"),
        "the bounded form returns exactly what the standalone form does"
    );
    let warm = service
        .canonicalize_named_selection_bounded(&limits, &input)
        .expect("warm cache then canonicalization");
    assert_eq!(source.reads(), 1, "the warm call read nothing");
    assert_eq!(warm, cold);

    // An explicit empty selection stays empty: it is never the whole team.
    let empty = service
        .canonicalize_named_selection_bounded(&limits, &named_input(vec![], vec![]))
        .expect("explicit empty selection");
    assert!(empty.include.is_empty() && empty.exclude.is_empty());
    assert!(empty.encode().expect("encodes").contains(r#""include":[]"#));
}

#[trace("FR-016-AC-11", "FR-004-AC-16")]
#[test]
fn composed_canonicalization_failures_return_the_typed_error_and_no_document() {
    let clock = ManualClock::new(16_000);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(naming_snapshot(), &clock, &directory);
    let limits = policy(240, Duration::from_secs(5));
    let mut refuse = |include: Vec<NamedMemberInput>| {
        service.canonicalize_named_selection_bounded(&limits, &named_input(include, vec![]))
    };
    assert!(matches!(
        refuse(vec![milestone_input("Beta", None)]),
        Err(Error::AmbiguousMilestone { .. })
    ));
    assert!(matches!(
        refuse(vec![NamedMemberInput::Project("Nothing".into())]),
        Err(Error::ScopeSelection {
            kind: "project",
            ..
        })
    ));
    assert!(matches!(
        refuse(vec![milestone_input("PRJ-1/Beta", Some("PRJ-2"))]),
        Err(Error::NamedMemberRejected {
            outcome: MemberOutcome::ProjectMismatch,
            ..
        })
    ));
    for other_team in [
        NamedMemberInput::Ticket("AGE-1".into()),
        NamedMemberInput::Project("Foreign".into()),
    ] {
        assert!(matches!(
            refuse(vec![other_team]),
            Err(Error::NamedMemberRejected {
                outcome: MemberOutcome::WrongTeam,
                ..
            })
        ));
    }
    // A good member beside a bad one still yields nothing.
    assert!(
        refuse(vec![
            NamedMemberInput::Project("Other".into()),
            NamedMemberInput::Ticket("SWM-404".into()),
        ])
        .is_err()
    );
    let unknown_team = NamedSelectionInput {
        team: "NOPE".into(),
        ..named_input(vec![], vec![])
    };
    assert!(matches!(
        service.canonicalize_named_selection_bounded(&limits, &unknown_team),
        Err(Error::ScopeSelection { kind: "team", .. })
    ));
    assert_eq!(source.reads(), 1, "one snapshot served every refusal");

    // An invalid policy and a source that cannot honor a bound fail before any read.
    let input = named_input(vec![], vec![]);
    assert!(matches!(
        service.canonicalize_named_selection_bounded(&policy(301, Duration::from_secs(5)), &input),
        Err(Error::InvalidReadPolicy { .. })
    ));
    let unbounded = FakeSource::new(&clock, false);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&unbounded, &clock, &cache_path(&directory));
    assert!(matches!(
        service.canonicalize_named_selection_bounded(&limits, &input),
        Err(Error::BoundUnsupported)
    ));
    assert_eq!((source.reads(), unbounded.reads()), (1, 0));
}

/// `projects` SWM projects, each with a milestone and five tickets, and an input naming
/// every project, a milestone qualified by its project, and a ticket of each, so that
/// resolving it scans the snapshot hundreds of times.
fn many_names(projects: usize) -> (WorkspaceSnapshot, NamedSelectionInput) {
    let mut snapshot = WorkspaceSnapshot::default();
    let mut include = vec![NamedMemberInput::Initiative("I-10".into())];
    for index in 0..projects {
        let id = format!("p{index}");
        let identifier = format!("PRJ-{index}");
        let mut project = named_project(&id, &identifier, &format!("Name {index}"), "SWM");
        project.milestones = vec![milestone_record(
            &format!("m{index}"),
            &format!("Stage {index}"),
        )];
        snapshot.projects.push(project);
        for leaf in 0..5 {
            let key = format!("SWM-{index}-{leaf}");
            snapshot.issues.push(ticket(&key, &key, "SWM", &id, None));
        }
        include.push(NamedMemberInput::Project(format!("Name {index}")));
        include.push(milestone_input(
            &format!("PRJ-{index}/Stage {index}"),
            Some(&format!("Name {index}")),
        ));
        include.push(NamedMemberInput::Ticket(format!("SWM-{index}-0")));
    }
    (snapshot, named_input(include, vec![]))
}

/// One composed canonicalization where every look at the clock costs [`WORK_STEP`] and a
/// cold source read costs `read_cost`. Returns the result and the controlled time used.
fn timed_canonicalization(
    snapshot: WorkspaceSnapshot,
    input: &NamedSelectionInput,
    deadline: Duration,
    from_cache: bool,
    read_cost: Duration,
) -> (Result<NamedSelection, Error>, Duration) {
    let clock = ManualClock::new(17_000);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(snapshot, &clock, &directory);
    if from_cache {
        service
            .workspace_snapshot_bounded(&policy(240, Duration::from_secs(3600)))
            .expect("a cold read fills the cache");
    } else if !read_cost.is_zero() {
        source.behave(Behavior::Pages {
            pages: 1,
            cost: read_cost,
        });
    }
    clock.set_step(WORK_STEP);
    let before = clock.elapsed();
    let result = service.canonicalize_named_selection_bounded(&policy(240, deadline), input);
    (result, clock.elapsed() - before)
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn the_deadline_expires_during_many_name_and_reference_scans_not_before_them() {
    // The two inputs share a snapshot, so the cache lookup, decode and team scan cost the
    // same. They differ only in resolution: no members against hundreds of scans. Work
    // that does not check the deadline consumes no controlled time, so a deadline that
    // only the surrounding checks observe would let the heavy input return late.
    let generous = Duration::from_secs(3600);
    for from_cache in [false, true] {
        let (snapshot, heavy) = many_names(30);
        let trivial = named_input(vec![], vec![]);
        let none = Duration::ZERO;
        let (light, light_used) =
            timed_canonicalization(snapshot.clone(), &trivial, generous, from_cache, none);
        let (full, full_used) =
            timed_canonicalization(snapshot.clone(), &heavy, generous, from_cache, none);
        light.expect("trivial input");
        assert_eq!(full.expect("heavy input").include.len(), 91);
        assert!(
            full_used > light_used * 2,
            "resolution must dominate the heavy input: {full_used:?} vs {light_used:?}"
        );

        // A deadline between the two: everything before resolution fits; resolution does not.
        let deadline = light_used + (full_used - light_used) / 2;
        let (fits, _) =
            timed_canonicalization(snapshot.clone(), &trivial, deadline, from_cache, none);
        fits.expect("the same read and context cost fits");
        let (result, used) = timed_canonicalization(snapshot, &heavy, deadline, from_cache, none);
        assert!(
            is_timeout(&result, deadline),
            "resolution past the one deadline is the typed timeout (cache {from_cache}): {result:?}"
        );
        assert!(
            used <= deadline + WORK_STEP * 100,
            "resolution stops soon after expiry, not after every scan: \
             {used:?} against {deadline:?} (full run {full_used:?})"
        );
    }
}

#[trace("FR-004-AC-14", "FR-004-AC-16")]
#[test]
fn time_the_read_used_is_not_given_back_to_resolution() {
    // The source read costs five controlled seconds. The deadline leaves room for the
    // read and a trivial resolution, but only half of what the heavy resolution needs.
    // A second deadline started after the read would give resolution the whole limit again
    // and the heavy input would succeed.
    let read_cost = Duration::from_secs(5);
    let generous = Duration::from_secs(3600);
    let (snapshot, heavy) = many_names(30);
    let trivial = named_input(vec![], vec![]);
    let (light, light_used) =
        timed_canonicalization(snapshot.clone(), &trivial, generous, false, read_cost);
    let (full, full_used) =
        timed_canonicalization(snapshot.clone(), &heavy, generous, false, read_cost);
    light.expect("trivial input");
    full.expect("heavy input");
    assert!(
        full_used - light_used > WORK_STEP * 100,
        "resolution must cost real controlled time: {full_used:?} vs {light_used:?}"
    );
    let deadline = light_used + (full_used - light_used) / 2;
    assert!(
        deadline > read_cost,
        "the read alone fits inside the deadline"
    );
    let (fits, _) = timed_canonicalization(snapshot.clone(), &trivial, deadline, false, read_cost);
    fits.expect("the read and a trivial resolution fit");
    let (result, _) = timed_canonicalization(snapshot, &heavy, deadline, false, read_cost);
    assert!(
        is_timeout(&result, deadline),
        "the read's time counts against resolution: {result:?}"
    );
}

/// How many records one project holds in the nested-phase tests below. The outer project
/// count stays at one, so only the nested traversal can account for the difference.
const NESTED: u32 = 2_000;

/// The last nested index.
const LAST: u32 = NESTED - 1;

/// A single project whose nested lists hold [`NESTED`] initiatives, milestones and team
/// keys. Initiative `init-N`, milestone `m-N` (named `Stage N`) and team `T-N` all start at
/// 0 (the team list is led by `SWM`), so a selector for the first record ends a scan at once and one for the last record
/// ends it only after the whole list, in the very same payload.
fn nested_snapshot() -> WorkspaceSnapshot {
    let mut project = named_project("p0", "PRJ-0", "Solo", "SWM");
    project.initiatives = (0..NESTED)
        .map(|index| Initiative {
            id: format!("init-{index}"),
            identifier: format!("I-{index}"),
            name: format!("Initiative {index}"),
        })
        .collect();
    project.milestones = (0..NESTED)
        .map(|index| milestone_record(&format!("m-{index}"), &format!("Stage {index}")))
        .collect();
    // `SWM` leads, so every other test finds its team at the first key.
    project.team_keys = std::iter::once(TeamKey("SWM".into()))
        .chain((0..NESTED).map(|index| TeamKey(format!("T-{index}"))))
        .collect();
    WorkspaceSnapshot {
        projects: vec![project],
        ..WorkspaceSnapshot::default()
    }
}

/// [`nested_snapshot`] plus two issues of the first and last team, so that the team is
/// known from the issues and only the project's own team list is left to search.
fn nested_snapshot_with_team_issues() -> WorkspaceSnapshot {
    let mut snapshot = nested_snapshot();
    snapshot.issues = vec![
        ticket("t-first", "T-0-1", "T-0", "p0", None),
        ticket("t-last", "T-1-1", &format!("T-{LAST}"), "p0", None),
    ];
    snapshot
}

fn team_input(team: &str, include: Vec<NamedMemberInput>) -> NamedSelectionInput {
    NamedSelectionInput {
        team: team.into(),
        include,
        exclude: vec![],
    }
}

/// The adverse oracle for one nested phase. `light` and `heavy` run against the same
/// snapshot, so the cache lookup, decode, team scan and outer project count cost the same;
/// the inputs differ only in how much of the phase under test the heavy one must search.
/// Every look at the clock costs [`WORK_STEP`], so a traversal that does not check the
/// deadline uses no controlled time and the heavy input would not cost more.
fn assert_nested_phase_stops_at_the_deadline(
    phase: &str,
    snapshot: &WorkspaceSnapshot,
    light: &NamedSelectionInput,
    heavy: &NamedSelectionInput,
) {
    let generous = Duration::from_secs(3600);
    let none = Duration::ZERO;
    let expected = canonicalize_named_selection(snapshot, heavy).expect("standalone heavy");
    for from_cache in [false, true] {
        let (fast, light_used) =
            timed_canonicalization(snapshot.clone(), light, generous, from_cache, none);
        let (slow, heavy_used) =
            timed_canonicalization(snapshot.clone(), heavy, generous, from_cache, none);
        fast.expect("light input");
        assert_eq!(
            slow.expect("heavy input"),
            expected,
            "{phase}: the bounded form returns what the standalone form does"
        );
        assert!(
            heavy_used >= light_used + WORK_STEP * (NESTED / 2),
            "{phase} (cache {from_cache}): the nested traversal must cost controlled time, so \
             it checks the deadline inside the list: {heavy_used:?} against {light_used:?}"
        );

        // A deadline between the two: the read, decode and the light input's own work fit;
        // the heavy traversal does not.
        let deadline = light_used + (heavy_used - light_used) / 2;
        let (fits, _) = timed_canonicalization(snapshot.clone(), light, deadline, from_cache, none);
        fits.expect("the same read and context cost fits");
        let (result, used) =
            timed_canonicalization(snapshot.clone(), heavy, deadline, from_cache, none);
        assert!(
            is_timeout(&result, deadline),
            "{phase} (cache {from_cache}): the typed timeout and no document: {result:?}"
        );
        assert!(
            used <= deadline + WORK_STEP * 10,
            "{phase} (cache {from_cache}): stops within a few checks of expiry, not after the \
             whole list: {used:?} against {deadline:?} (full run {heavy_used:?})"
        );
    }
}

fn initiative_member(index: u32) -> Vec<NamedMemberInput> {
    vec![NamedMemberInput::Initiative(format!("init-{index}"))]
}

fn milestone_member(selector: &str) -> Vec<NamedMemberInput> {
    vec![milestone_input(selector, None)]
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_initiatives_stops_the_initiative_lookup_at_the_deadline() {
    // Light names nothing; heavy names the first initiative, whose membership check ends
    // at once. The one project's initiative list is searched for the selector only.
    assert_nested_phase_stops_at_the_deadline(
        "initiative lookup",
        &nested_snapshot(),
        &team_input("SWM", vec![]),
        &team_input("SWM", initiative_member(0)),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_initiatives_stops_the_initiative_membership_scan_at_the_deadline() {
    // Both look up their initiative through the whole list. The first ends its membership
    // scan at once, the last only after every initiative of the project.
    assert_nested_phase_stops_at_the_deadline(
        "initiative membership",
        &nested_snapshot(),
        &team_input("SWM", initiative_member(0)),
        &team_input("SWM", initiative_member(LAST)),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_milestones_stops_the_exact_id_pass_at_the_deadline() {
    // Heavy names the first milestone by UUID: the exact pass searches the whole list.
    assert_nested_phase_stops_at_the_deadline(
        "milestone exact id",
        &nested_snapshot(),
        &team_input("SWM", vec![]),
        &team_input("SWM", milestone_member("m-0")),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_milestones_stops_the_whole_name_pass_at_the_deadline() {
    // Both pass the exact pass over the whole list; only a name goes on to the name pass.
    assert_nested_phase_stops_at_the_deadline(
        "milestone whole name",
        &nested_snapshot(),
        &team_input("SWM", milestone_member("m-0")),
        &team_input("SWM", milestone_member("Stage 0")),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_milestones_stops_the_qualified_name_pass_at_the_deadline() {
    // Both pass the exact and whole-name passes; only the qualified selector then searches
    // the named project's milestones again at its slash.
    assert_nested_phase_stops_at_the_deadline(
        "milestone qualified name",
        &nested_snapshot(),
        &team_input("SWM", milestone_member("Stage 0")),
        &team_input("SWM", milestone_member("PRJ-0/Stage 0")),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_milestones_stops_the_owner_scan_at_the_deadline() {
    // Both pass the exact pass over the whole list. The first milestone ends the owner
    // scan at once, the last only after every milestone of the project.
    assert_nested_phase_stops_at_the_deadline(
        "milestone owner",
        &nested_snapshot(),
        &team_input("SWM", milestone_member("m-0")),
        &team_input("SWM", milestone_member(&format!("m-{LAST}"))),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_team_keys_stops_the_team_known_scan_at_the_deadline() {
    // No issues, so a team is known only from the project's team list: the first key ends
    // the scan at once, the last only after every key.
    assert_nested_phase_stops_at_the_deadline(
        "team known",
        &nested_snapshot(),
        &team_input("T-0", vec![]),
        &team_input(&format!("T-{LAST}"), vec![]),
    );
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn one_project_with_many_team_keys_stops_the_holds_team_work_scan_at_the_deadline() {
    // Each team is known from an issue at once. Whether the project holds that team's
    // work is then found in its team list: at the first key, or after every key.
    let project = || vec![NamedMemberInput::Project("p0".into())];
    assert_nested_phase_stops_at_the_deadline(
        "project holds team work",
        &nested_snapshot_with_team_issues(),
        &team_input("T-0", project()),
        &team_input(&format!("T-{LAST}"), project()),
    );
}

fn want_initiative(id: &str) -> NamedMember {
    NamedMember::Initiative { id: id.into() }
}

fn want_project(id: &str) -> NamedMember {
    NamedMember::Project { id: id.into() }
}

fn want_issue(id: &str) -> NamedMember {
    NamedMember::Issue { id: id.into() }
}

fn want_milestone(id: &str, project_id: &str) -> NamedMember {
    NamedMember::Milestone {
        id: id.into(),
        project_id: project_id.into(),
    }
}

/// Every kind, out of alphabetical order, with several spellings of one project, one
/// initiative and one ticket, an identical input repeated, two milestones of the same
/// name owned by different projects, and members that sit in both lists.
fn aliased_input() -> NamedSelectionInput {
    named_input(
        vec![
            NamedMemberInput::Ticket("SWM-2".into()),
            NamedMemberInput::Project("Project / Alpha".into()),
            NamedMemberInput::Initiative("Delivery".into()),
            NamedMemberInput::Project("PRJ-1".into()),
            NamedMemberInput::Project("uuid-alpha".into()),
            milestone_input("PRJ-1/Beta", Some("PRJ-1")),
            milestone_input("PRJ-2/Beta", Some("Other")),
            NamedMemberInput::Ticket("SWM-2".into()),
            NamedMemberInput::Ticket("uuid-t2".into()),
            NamedMemberInput::Initiative("I-10".into()),
        ],
        vec![
            NamedMemberInput::Ticket("swm-2".into()),
            NamedMemberInput::Project("Other".into()),
            NamedMemberInput::Project("PRJ-1".into()),
            NamedMemberInput::Ticket("SWM-2".into()),
        ],
    )
}

/// What each occurrence of [`aliased_input`] must resolve to, by hand, in input order.
fn aliased_include_members() -> Vec<NamedMember> {
    vec![
        want_issue("uuid-t2"),
        want_project("uuid-alpha"),
        want_initiative("uuid-init"),
        want_project("uuid-alpha"),
        want_project("uuid-alpha"),
        want_milestone("uuid-m2", "uuid-alpha"),
        want_milestone("uuid-m3", "uuid-other"),
        want_issue("uuid-t2"),
        want_issue("uuid-t2"),
        want_initiative("uuid-init"),
    ]
}

fn aliased_exclude_members() -> Vec<NamedMember> {
    vec![
        want_issue("uuid-t2"),
        want_project("uuid-other"),
        want_project("uuid-alpha"),
        want_issue("uuid-t2"),
    ]
}

/// Labels a caller owns, one per input occurrence. They exist only in this test: the
/// library has no label field.
const INCLUDE_LABELS: [&str; 10] = [
    "ticket",
    "alpha by name",
    "delivery by name",
    "alpha by key",
    "alpha by id",
    "beta of alpha",
    "beta of other",
    "ticket again",
    "ticket by id",
    "delivery by key",
];
const EXCLUDE_LABELS: [&str; 4] = ["skip ticket", "skip other", "skip alpha", "skip ticket again"];

#[trace("FR-016-AC-11", "FR-016-AC-6")]
#[test]
fn every_input_occurrence_keeps_its_list_index_and_exact_member() {
    let clock = ManualClock::new(18_000);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(naming_snapshot(), &clock, &directory);
    let limits = policy(240, Duration::from_secs(5));
    let input = aliased_input();

    let resolved = service
        .resolve_named_selection_bounded(&limits, &input)
        .expect("resolves");
    assert_eq!(source.reads(), 1, "one snapshot read served the whole call");

    // The canonical selection is sorted and has no duplicates, per list.
    assert_eq!(
        resolved.selection.include,
        [
            want_initiative("uuid-init"),
            want_project("uuid-alpha"),
            want_milestone("uuid-m2", "uuid-alpha"),
            want_milestone("uuid-m3", "uuid-other"),
            want_issue("uuid-t2"),
        ]
    );
    assert_eq!(
        resolved.selection.exclude,
        [
            want_project("uuid-alpha"),
            want_project("uuid-other"),
            want_issue("uuid-t2"),
        ]
    );

    // Every occurrence appears exactly once: include in supplied order, then exclude.
    let expected = aliased_include_members()
        .into_iter()
        .zip(&input.include)
        .enumerate()
        .map(|(index, (member, supplied))| (NamedList::Include, index, supplied, member))
        .chain(
            aliased_exclude_members()
                .into_iter()
                .zip(&input.exclude)
                .enumerate()
                .map(|(index, (member, supplied))| (NamedList::Exclude, index, supplied, member)),
        )
        .collect::<Vec<_>>();
    assert_eq!(resolved.associations.len(), expected.len());
    for (entry, (list, index, supplied, member)) in resolved.associations.iter().zip(expected) {
        assert_eq!((entry.list, entry.index), (list, index));
        assert_eq!(&entry.input, supplied, "{list:?}[{index}] keeps its input");
        assert_eq!(entry.member, member, "{list:?}[{index}] resolved member");
        assert_eq!(resolved.association(list, index), Some(entry));
    }
    assert_eq!(resolved.association(NamedList::Include, 10), None);
    assert_eq!(resolved.association(NamedList::Exclude, 4), None);

    // The associations and the selection describe the same members, per list.
    for (list, canonical) in [
        (NamedList::Include, &resolved.selection.include),
        (NamedList::Exclude, &resolved.selection.exclude),
    ] {
        let associated = resolved
            .associations
            .iter()
            .filter(|entry| entry.list == list)
            .map(|entry| &entry.member)
            .collect::<BTreeSet<_>>();
        assert_eq!(associated, canonical.iter().collect::<BTreeSet<_>>());
    }
    assert_eq!(source.reads(), 1, "nothing resolved a second time");
}

#[trace("FR-016-AC-11")]
#[test]
fn a_caller_attaches_its_own_labels_to_aliases_and_repeats_without_resolving_again() {
    let clock = ManualClock::new(18_100);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(naming_snapshot(), &clock, &directory);
    let input = aliased_input();
    let resolved = service
        .resolve_named_selection_bounded(&policy(240, Duration::from_secs(5)), &input)
        .expect("resolves");

    let labels: BTreeMap<(NamedList, usize), &str> = INCLUDE_LABELS
        .iter()
        .enumerate()
        .map(|(index, label)| ((NamedList::Include, index), *label))
        .chain(
            EXCLUDE_LABELS
                .iter()
                .enumerate()
                .map(|(index, label)| ((NamedList::Exclude, index), *label)),
        )
        .collect();
    let mut by_member: BTreeMap<(NamedList, NamedMember), Vec<&str>> = BTreeMap::new();
    for entry in &resolved.associations {
        by_member
            .entry((entry.list, entry.member.clone()))
            .or_default()
            .push(labels[&(entry.list, entry.index)]);
    }
    // Three spellings of one project keep three labels; the SDK chose none of them.
    assert_eq!(
        by_member[&(NamedList::Include, want_project("uuid-alpha"))],
        ["alpha by name", "alpha by key", "alpha by id"]
    );
    // An identical input repeated keeps its separate, distinct metadata.
    assert_eq!(input.include[0], input.include[7]);
    assert_eq!(
        by_member[&(NamedList::Include, want_issue("uuid-t2"))],
        ["ticket", "ticket again", "ticket by id"]
    );
    // One member in both lists keeps one label per list.
    assert_eq!(
        by_member[&(NamedList::Exclude, want_issue("uuid-t2"))],
        ["skip ticket", "skip ticket again"]
    );
    // Same-named milestones of different projects stay distinct by owner.
    assert_eq!(
        by_member[&(NamedList::Include, want_milestone("uuid-m3", "uuid-other"))],
        ["beta of other"]
    );
    assert_eq!(
        by_member[&(NamedList::Include, want_milestone("uuid-m2", "uuid-alpha"))],
        ["beta of alpha"]
    );
    assert_eq!(source.reads(), 1, "labels were attached with no second read");
}

#[trace("FR-016-AC-11")]
#[test]
fn the_same_member_in_both_lists_is_one_occurrence_per_list() {
    let clock = ManualClock::new(18_200);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, _) = naming_service(naming_snapshot(), &clock, &directory);
    let input = named_input(
        vec![NamedMemberInput::Ticket("SWM-1".into())],
        vec![NamedMemberInput::Ticket("uuid-t1".into())],
    );
    let resolved = service
        .resolve_named_selection_bounded(&policy(240, Duration::from_secs(5)), &input)
        .expect("resolves");
    let list_and_index = resolved
        .associations
        .iter()
        .map(|entry| (entry.list, entry.index, entry.member.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        list_and_index,
        [
            (NamedList::Include, 0, want_issue("uuid-t1")),
            (NamedList::Exclude, 0, want_issue("uuid-t1")),
        ]
    );
    assert_eq!(resolved.selection.include, [want_issue("uuid-t1")]);
    assert_eq!(resolved.selection.exclude, [want_issue("uuid-t1")]);
}

#[trace("FR-016-AC-11", "FR-016-AC-6")]
#[test]
fn the_selection_and_its_wire_bytes_equal_the_canonical_forms_and_carry_no_association() {
    let clock = ManualClock::new(18_300);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, _) = naming_service(naming_snapshot(), &clock, &directory);
    let limits = policy(240, Duration::from_secs(5));
    let input = aliased_input();

    let resolved = service
        .resolve_named_selection_bounded(&limits, &input)
        .expect("resolves");
    let bounded = service
        .canonicalize_named_selection_bounded(&limits, &input)
        .expect("bounded canonicalizer");
    let standalone = canonicalize_named_selection(&naming_snapshot(), &input).expect("standalone");
    assert_eq!(resolved.selection, bounded);
    assert_eq!(resolved.selection, standalone);
    assert_eq!(
        resolved.selection.encode().expect("encodes"),
        standalone.encode().expect("encodes")
    );

    let wire = serde_json::to_value(&resolved.selection).expect("serializes");
    let keys = wire
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        BTreeSet::from(["schema_version", "team", "include", "exclude"])
    );
    assert!(
        !resolved
            .selection
            .encode()
            .expect("encodes")
            .contains("label")
    );

    // An empty input is the explicit empty selection with no associations.
    let empty = service
        .resolve_named_selection_bounded(&limits, &named_input(vec![], vec![]))
        .expect("empty");
    assert!(empty.associations.is_empty());
    assert!(empty.selection.include.is_empty() && empty.selection.exclude.is_empty());
}

#[trace("FR-016-AC-11", "FR-004-AC-14")]
#[test]
fn cold_warm_and_changed_sources_resolve_from_one_snapshot_each() {
    let clock = ManualClock::new(18_400);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(naming_snapshot(), &clock, &directory);
    let limits = policy(240, Duration::from_secs(5));
    let input = aliased_input();

    let cold = service
        .resolve_named_selection_bounded(&limits, &input)
        .expect("cold");
    assert_eq!(source.reads(), 1);
    let legacy = service
        .canonicalize_named_selection_bounded(&limits, &input)
        .expect("legacy form over the warm cache");
    assert_eq!(source.reads(), 1);
    assert_eq!(cold.selection, legacy);
    let warm = service
        .resolve_named_selection_bounded(&limits, &input)
        .expect("warm");
    assert_eq!(warm, cold);
    assert_eq!(source.reads(), 1, "the warm call read nothing");

    // A source that would now never answer cannot change a result served from the cache.
    source.behave(Behavior::NeverComplete);
    let still_warm = service
        .resolve_named_selection_bounded(&limits, &input)
        .expect("still warm");
    assert_eq!(still_warm, cold);
    assert_eq!(source.reads(), 1);

    // Past `max_age` the refresh runs under the call's own deadline and times out: no
    // selection and no associations, and nothing stale is returned.
    clock.set_epoch(18_400 + 241);
    let expired = service.resolve_named_selection_bounded(&limits, &input);
    assert!(is_timeout(&expired, limits.deadline), "{expired:?}");
    assert_eq!(source.reads(), 2);

    // The source answers again: the next call reads one new snapshot and resolves from it.
    source.behave(Behavior::Succeed);
    let refreshed = service
        .resolve_named_selection_bounded(&limits, &input)
        .expect("refreshed");
    assert_eq!(source.reads(), 3);
    assert_eq!(refreshed, cold);
}

#[trace("FR-016-AC-11", "FR-004-AC-16")]
#[test]
fn a_good_member_then_a_bad_one_returns_no_selection_and_no_associations() {
    let clock = ManualClock::new(18_500);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(naming_snapshot(), &clock, &directory);
    let limits = policy(240, Duration::from_secs(5));
    let good = || {
        vec![
            NamedMemberInput::Project("Project / Alpha".into()),
            NamedMemberInput::Ticket("SWM-1".into()),
        ]
    };
    let mut refuse = |include: Vec<NamedMemberInput>, exclude: Vec<NamedMemberInput>| {
        service.resolve_named_selection_bounded(&limits, &named_input(include, exclude))
    };

    // The bad member comes after good ones, in either list.
    let mut after_good = good();
    after_good.push(milestone_input("Beta", None));
    assert!(matches!(
        refuse(after_good, vec![]),
        Err(Error::AmbiguousMilestone { .. })
    ));
    assert!(matches!(
        refuse(
            good(),
            vec![NamedMemberInput::Project("Nothing".into())]
        ),
        Err(Error::ScopeSelection {
            kind: "project",
            ..
        })
    ));
    assert!(matches!(
        refuse(
            good(),
            vec![milestone_input("PRJ-1/Beta", Some("PRJ-2"))]
        ),
        Err(Error::NamedMemberRejected {
            outcome: MemberOutcome::ProjectMismatch,
            ..
        })
    ));
    for other_team in [
        NamedMemberInput::Ticket("AGE-1".into()),
        NamedMemberInput::Project("Foreign".into()),
    ] {
        let mut include = good();
        include.push(other_team);
        assert!(matches!(
            refuse(include, vec![]),
            Err(Error::NamedMemberRejected {
                outcome: MemberOutcome::WrongTeam,
                ..
            })
        ));
    }
    let unknown_team = NamedSelectionInput {
        team: "NOPE".into(),
        ..named_input(good(), vec![])
    };
    assert!(matches!(
        service.resolve_named_selection_bounded(&limits, &unknown_team),
        Err(Error::ScopeSelection { kind: "team", .. })
    ));
    assert_eq!(source.reads(), 1, "one snapshot served every refusal");

    // An invalid policy and a source that cannot honor a bound fail before any read.
    let input = named_input(good(), vec![]);
    assert!(matches!(
        service.resolve_named_selection_bounded(&policy(301, Duration::from_secs(5)), &input),
        Err(Error::InvalidReadPolicy { .. })
    ));
    let unbounded = FakeSource::new(&clock, false);
    let directory = tempfile::tempdir().expect("dir");
    let mut service = open(&unbounded, &clock, &cache_path(&directory));
    assert!(matches!(
        service.resolve_named_selection_bounded(&limits, &input),
        Err(Error::BoundUnsupported)
    ));
    assert_eq!((source.reads(), unbounded.reads()), (1, 0));
}

/// [`timed_canonicalization`] for [`ReadService::resolve_named_selection_bounded`].
fn timed_resolution(
    snapshot: WorkspaceSnapshot,
    input: &NamedSelectionInput,
    deadline: Duration,
    from_cache: bool,
    read_cost: Duration,
) -> (Result<ResolvedNamedSelection, Error>, Duration) {
    let clock = ManualClock::new(17_000);
    let directory = tempfile::tempdir().expect("dir");
    let (mut service, source) = naming_service(snapshot, &clock, &directory);
    if from_cache {
        service
            .workspace_snapshot_bounded(&policy(240, Duration::from_secs(3600)))
            .expect("a cold read fills the cache");
    } else if !read_cost.is_zero() {
        source.behave(Behavior::Pages {
            pages: 1,
            cost: read_cost,
        });
    }
    clock.set_step(WORK_STEP);
    let before = clock.elapsed();
    let result = service.resolve_named_selection_bounded(&policy(240, deadline), input);
    (result, clock.elapsed() - before)
}

#[trace("FR-004-AC-14", "FR-016-AC-11")]
#[test]
fn capturing_associations_spends_the_same_one_budget_as_resolution_and_ordering() {
    let generous = Duration::from_secs(3600);
    let (snapshot, heavy) = many_names(30);
    for (from_cache, read_cost) in [
        (false, Duration::ZERO),
        (false, Duration::from_secs(5)),
        (true, Duration::ZERO),
    ] {
        let (legacy, legacy_used) =
            timed_canonicalization(snapshot.clone(), &heavy, generous, from_cache, read_cost);
        let (resolved, resolved_used) =
            timed_resolution(snapshot.clone(), &heavy, generous, from_cache, read_cost);
        let legacy = legacy.expect("legacy form");
        let resolved = resolved.expect("association form");
        assert_eq!(resolved.selection, legacy);
        assert_eq!(resolved.associations.len(), heavy.include.len());
        assert!(
            resolved_used > legacy_used + WORK_STEP * 50,
            "capturing every occurrence costs controlled time: {resolved_used:?} vs {legacy_used:?}"
        );

        // A deadline that the legacy form meets but the association form cannot: the
        // capture is inside the one deadline, not after it and not under a second one.
        let deadline = legacy_used + (resolved_used - legacy_used) / 2;
        let (fits, _) =
            timed_canonicalization(snapshot.clone(), &heavy, deadline, from_cache, read_cost);
        fits.expect("the legacy form fits");
        let (late, used) =
            timed_resolution(snapshot.clone(), &heavy, deadline, from_cache, read_cost);
        assert!(
            is_timeout(&late, deadline),
            "past the one deadline is the typed timeout, with no result (cache {from_cache}, \
             read {read_cost:?}): {late:?}"
        );
        assert!(
            used <= deadline + WORK_STEP * 100,
            "capture stops soon after expiry: {used:?} against {deadline:?}"
        );
    }
}
