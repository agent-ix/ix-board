// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The quota-only cutover fences pre-paired SQL writers and unknown in-flight responses.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use ix_board::quota::{QuotaClock, QuotaConfig, QuotaGate, QuotaState};
use ix_board::{Clock, Error, ReadService, WorkspaceSnapshot, WorkspaceSource};
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;
use rusqlite::{Connection, params};

const NOW: i64 = 1_800_000_000_000;
struct GateClock(AtomicI64);
impl QuotaClock for GateClock {
    fn now_epoch_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
fn gate(path: &Path, clock: &Arc<GateClock>) -> QuotaGate {
    QuotaGate::open_with_clock(path, &QuotaConfig::default(), clock.clone()).unwrap()
}

fn legacy_schema(path: &Path, pending: bool) {
    let db = Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE linear_shared_quota (
        singleton INTEGER PRIMARY KEY,cooldown INTEGER NOT NULL DEFAULT 0,
        requests INTEGER NOT NULL DEFAULT 0,complexity INTEGER NOT NULL DEFAULT 0,
        request_reset INTEGER NOT NULL DEFAULT 0,complexity_reset INTEGER NOT NULL DEFAULT 0,
        request_limit INTEGER,complexity_limit INTEGER,request_remaining INTEGER,complexity_remaining INTEGER,
        endpoint_until INTEGER NOT NULL DEFAULT 0,request_budget INTEGER,complexity_budget INTEGER,
        endpoint_limit INTEGER,endpoint_remaining INTEGER,endpoint_reset INTEGER NOT NULL DEFAULT 0,
        endpoint_used INTEGER NOT NULL DEFAULT 0,database_identity TEXT,local_generation INTEGER NOT NULL DEFAULT 0,
        local_reset INTEGER,server_complexity_generation INTEGER NOT NULL DEFAULT 0,server_complexity_debt INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE linear_quota_reservations(id INTEGER PRIMARY KEY AUTOINCREMENT,generation INTEGER NOT NULL,estimate INTEGER NOT NULL,server_complexity_generation INTEGER NOT NULL);").unwrap();
    db.execute("INSERT INTO linear_shared_quota(singleton,requests,complexity,request_limit,request_remaining,request_reset,complexity_remaining,complexity_reset,local_reset,database_identity,server_complexity_debt) VALUES(1,?1,?2,10000,250,?3,155000,?4,?5,'pre-paired-fixture',123)",params![u64::from(pending),if pending {10000}else{0},NOW+1000,NOW+2000,NOW+3_600_000]).unwrap();
    if pending {
        db.execute("INSERT INTO linear_quota_reservations(generation,estimate,server_complexity_generation) VALUES(0,10000,0)",[]).unwrap();
    }
}

fn assert_upgrade(error: &rusqlite::Error) {
    assert!(
        error
            .to_string()
            .contains("quota writer upgrade required; rebuild ix-board clients"),
        "{error}"
    );
}
fn obligations(db: &Connection) -> (i64, i64, i64, i64) {
    db.query_row("SELECT (SELECT COUNT(*) FROM linear_quota_reservations),(SELECT COALESCE(SUM(estimate),0) FROM linear_quota_reservations),server_complexity_debt,paired_writer FROM linear_shared_quota",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap()
}

#[trace("FR-032-AC-3", "FR-032-AC-4", "FR-032-AC-6")]
#[test]
fn post_migration_old_header_reservation_and_debt_writes_are_fenced_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.sqlite");
    legacy_schema(&path, false);
    let clock = Arc::new(GateClock(AtomicI64::new(NOW)));
    let current = gate(&path, &clock);
    let mut old = Connection::open(&path).unwrap();
    let before = current.decision(10).unwrap();
    assert_eq!(before.state, QuotaState::ServerReserveHeld);
    let error=old.execute("UPDATE linear_shared_quota SET request_limit=10000,request_remaining=250,request_reset=?1 WHERE singleton=1",[NOW+10000]).unwrap_err();
    assert_upgrade(&error);
    assert_eq!(
        current.decision(10).unwrap().next_reevaluation_at,
        Some(NOW + 1000),
        "rejected old update cannot slide imported evidence"
    );
    for sql in [
        "UPDATE linear_shared_quota SET database_identity=COALESCE(database_identity,lower(hex(randomblob(16)))),local_reset=COALESCE(local_reset,1) WHERE singleton=1",
        "UPDATE linear_shared_quota SET requests=requests+1,complexity=complexity+10000,request_remaining=MAX(0,request_remaining-1) WHERE singleton=1",
        "UPDATE linear_shared_quota SET server_complexity_debt=0,requests=0,complexity=0 WHERE singleton=1",
        "INSERT INTO linear_quota_reservations(generation,estimate,server_complexity_generation) VALUES(0,10000,0)",
    ] {
        assert_upgrade(&old.execute(sql, []).unwrap_err());
    }
    old.execute_batch("CREATE TABLE unrelated_attempts(value INTEGER)")
        .unwrap();
    {
        let transaction = old.transaction().unwrap();
        transaction
            .execute("INSERT INTO unrelated_attempts VALUES(1)", [])
            .unwrap();
        // The old gate's first advance aborts before commit/network even if a
        // caller had already made unrelated writes in its transaction.
        assert_upgrade(&transaction.execute("UPDATE linear_shared_quota SET request_remaining=CASE WHEN request_reset<=?1 THEN NULL ELSE request_remaining END WHERE singleton=1",[NOW]).unwrap_err());
    }
    assert_eq!(
        old.query_row("SELECT COUNT(*) FROM unrelated_attempts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(obligations(&old), (0, 0, 123, 0));
    assert_eq!(
        current.decision(10).unwrap().state,
        QuotaState::ServerReserveHeld
    );
    clock.0.store(NOW + 2000, Ordering::SeqCst);
    let reopened = gate(&path, &clock);
    assert_eq!(
        reopened.decision(10).unwrap().state,
        QuotaState::Available,
        "only imported evidence expires; rejected legacy writes create no new constraint"
    );
    assert_eq!(obligations(&old), (0, 0, 123, 0));
    reopened.reserve_evaluated(10, None).unwrap();
    assert_upgrade(
        &old.execute("DELETE FROM linear_quota_reservations", [])
            .unwrap_err(),
    );
    assert_eq!(
        reopened.decision(10).unwrap().legacy_pending_reservations,
        0,
        "new-protocol abandoned charge is not quarantined"
    );
}

#[trace("FR-032-AC-6", "FR-032-AC-7")]
#[test]
fn old_inflight_http_and_graphql_429_cannot_be_followed_by_a_new_send() {
    for graphql in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.sqlite");
        legacy_schema(&path, true);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = listener.try_clone().unwrap();
        let (accepted_tx, accepted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = fixture.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 512];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() < 4096);
            }
            accepted_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (status, body) = if graphql {
                (
                    "400 Bad Request",
                    r#"{"errors":[{"extensions":{"code":"RATELIMITED","retryAfter":90}}]}"#,
                )
            } else {
                ("429 Too Many Requests", "rate limited")
            };
            write!(stream,"HTTP/1.1 {status}\r\nRetry-After: 90\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        // This request was reserved and sent by the pre-paired process before cutover.
        let mut in_flight = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        in_flight
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        in_flight
            .write_all(b"POST /graphql HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let clock = Arc::new(GateClock(AtomicI64::new(NOW)));
        let current = gate(&path, &clock);
        let decision = current.decision(10).unwrap();
        assert_eq!(decision.state, QuotaState::EvidenceUnavailable);
        assert_eq!(decision.legacy_pending_reservations, 1);
        release_tx.send(()).unwrap();
        let mut response = String::new();
        in_flight.read_to_string(&mut response).unwrap();
        worker.join().unwrap();
        assert!(response.contains(if graphql {
            "RATELIMITED"
        } else {
            "429 Too Many Requests"
        }));
        let mut old = Connection::open(&path).unwrap();
        let before = obligations(&old);
        {
            let transaction = old.transaction().unwrap();
            // Old observe_response advances before settlement, headers and the
            // actual429 write. The rejected completion cannot reach that write.
            assert_upgrade(&transaction.execute("UPDATE linear_shared_quota SET request_remaining=CASE WHEN request_reset<=?1 THEN NULL ELSE request_remaining END WHERE singleton=1",[NOW]).unwrap_err());
        }
        for sql in [
            "DELETE FROM linear_quota_reservations WHERE id=1",
            "UPDATE linear_shared_quota SET complexity=100,server_complexity_debt=0",
        ] {
            assert_upgrade(&old.execute(sql, []).unwrap_err());
        }
        assert_eq!(
            obligations(&old),
            before,
            "rejected old completion retains its estimate, identity and debt"
        );
        assert_eq!(before, (1, 10000, 123, 0));
        assert_eq!(
            current.status().unwrap().rate_limit_until,
            None,
            "an unrecordable old response must not invent a known actual cooldown"
        );
        for time in [NOW, NOW + 90_000, NOW + 3_600_001] {
            clock.0.store(time, Ordering::SeqCst);
            let reopened = gate(&path, &clock);
            let refusal = reopened.reserve_evaluated(10, None).unwrap_err();
            assert_eq!(
                refusal.quota_decision().unwrap().state,
                QuotaState::EvidenceUnavailable
            );
            assert_eq!(
                refusal
                    .quota_decision()
                    .unwrap()
                    .legacy_pending_reservations,
                1
            );
        }
        let decision = current.decision(10).unwrap();
        assert_eq!(
            decision.next_reevaluation_at, None,
            "unknown old response has no invented recovery deadline"
        );
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the only HTTP call was admitted before cutover"
        );
        assert_eq!(obligations(&old), before);
    }
}

#[trace("FR-032-AC-6", "FR-032-AC-7")]
#[test]
fn concurrent_new_writers_keep_known_backoff_and_crash_charge_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.sqlite");
    let clock = Arc::new(GateClock(AtomicI64::new(NOW)));
    let current = gate(&path, &clock);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let current = gate(&path, &clock);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let token = current.reserve_evaluated(10, None).unwrap();
                current
                    .observe(token, 200, &HeaderMap::new(), None)
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(current.status().unwrap().requests_used, 2);
    assert_eq!(
        obligations(&Connection::open(&path).unwrap()),
        (0, 0, 20, 0)
    );
    current.reserve_evaluated(10, None).unwrap();
    clock.0.store(NOW + 3_600_000, Ordering::SeqCst);
    assert_eq!(
        gate(&path, &clock).decision(10).unwrap().state,
        QuotaState::Available
    );
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", "60".parse().unwrap());
    current.observe_headers(429, &headers, None).unwrap();
    let old = Connection::open(&path).unwrap();
    assert_upgrade(
        &old.execute("UPDATE linear_shared_quota SET cooldown=0", [])
            .unwrap_err(),
    );
    assert_eq!(
        gate(&path, &clock).decision(10).unwrap().state,
        QuotaState::ServerBackoff
    );
    assert_eq!(
        current.status().unwrap().rate_limit_until,
        Some(NOW + 3_660_000)
    );
}

struct Source {
    fetches: Arc<AtomicUsize>,
}
impl WorkspaceSource for Source {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(WorkspaceSnapshot::default())
    }
}
struct SourceClock;
impl Clock for SourceClock {
    fn now_epoch_seconds(&self) -> i64 {
        1000
    }
}

#[trace("FR-032-AC-6", "FR-032-AC-8", "FR-030-AC-6", "FR-004-AC-21")]
#[test]
fn quota_only_migration_preserves_complete_snapshot_partition_and_success_markers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    let fetches = Arc::new(AtomicUsize::new(0));
    let mut service = ReadService::with_clock(
        Source {
            fetches: fetches.clone(),
        },
        &path,
        SourceClock,
    )
    .unwrap();
    service.workspace_snapshot().unwrap();
    let freshness = service.cache_freshness().unwrap().clone();
    let db = Connection::open(&path).unwrap();
    let detail: Vec<u8> = db
        .query_row("SELECT payload FROM workspace_snapshot", [], |row| {
            row.get(0)
        })
        .unwrap();
    let key: String = db
        .query_row("SELECT cache_key FROM workspace_snapshot", [], |row| {
            row.get(0)
        })
        .unwrap();
    db.execute_batch("CREATE TABLE summary_snapshot(cache_key TEXT PRIMARY KEY,payload BLOB NOT NULL,synced_at INTEGER NOT NULL,full_at INTEGER NOT NULL); INSERT INTO summary_snapshot VALUES('existing-summary',X'7b7d',900,800)").unwrap();
    legacy_schema(&path, false);
    let clock = Arc::new(GateClock(AtomicI64::new(NOW)));
    gate(&path, &clock);
    let mut reopened = ReadService::with_clock(
        Source {
            fetches: fetches.clone(),
        },
        &path,
        SourceClock,
    )
    .unwrap();
    reopened.workspace_snapshot().unwrap();
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "migration must not cause a full source read"
    );
    let after = reopened.cache_freshness().unwrap();
    assert_eq!(
        (after.last_successful_sync, after.last_full_reconciliation),
        (
            freshness.last_successful_sync,
            freshness.last_full_reconciliation
        )
    );
    assert_eq!(
        db.query_row(
            "SELECT cache_key,payload FROM workspace_snapshot",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        )
        .unwrap(),
        (key, detail)
    );
    assert_eq!(
        db.query_row(
            "SELECT cache_key,payload,synced_at,full_at FROM summary_snapshot",
            [],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?
            ))
        )
        .unwrap(),
        ("existing-summary".to_owned(), b"{}".to_vec(), 900, 800)
    );
}
