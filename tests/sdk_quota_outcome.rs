// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! External SDK consumers retain old literals and receive attempt-scoped quota evidence.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicUsize, Ordering};
use std::time::Duration;

use ix_board::quota::{QuotaClock, QuotaConfig, QuotaDimension, QuotaGate, QuotaState};
use ix_board::{
    CacheFreshness, Clock, Deadline, Error, ReadPolicy, ReadService, RefreshOutcome, RefreshRead,
    SummaryRead, WorkspaceSnapshot, WorkspaceSource,
};
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;
use serde_json::json;

const NOW: i64 = 1_800_000_000_000;

#[derive(Clone)]
struct SourceClock(Arc<AtomicI64>);

impl Clock for SourceClock {
    fn now_epoch_seconds(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct GateClock(Arc<AtomicI64>);

impl QuotaClock for GateClock {
    fn now_epoch_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct FixtureSource {
    gate: Arc<QuotaGate>,
    mode: Arc<AtomicU8>,
    successful_fetches: Arc<AtomicUsize>,
}

impl WorkspaceSource for FixtureSource {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        match self.mode.load(Ordering::SeqCst) {
            0 => {
                self.successful_fetches.fetch_add(1, Ordering::SeqCst);
                Ok(WorkspaceSnapshot::default())
            }
            1 => {
                let reservation = self.gate.reserve_evaluated(10, None)?;
                self.successful_fetches.fetch_add(1, Ordering::SeqCst);
                self.gate
                    .observe(reservation, 200, &HeaderMap::new(), None)?;
                Ok(WorkspaceSnapshot::default())
            }
            // Deliberately resembles a quota message. Classification uses the error
            // variant, so source wording cannot turn this into a quota refusal.
            2 => Err(Error::GraphQl {
                messages: "Linear rate limited; retry in 99s".to_owned(),
            }),
            3 => Err(Error::RefreshLockTimeout),
            _ => Err(Error::GraphQl {
                messages: "an unrelated replacement diagnostic".to_owned(),
            }),
        }
    }

    fn supports_bounded_read(&self) -> bool {
        true
    }

    fn fetch_workspace_bounded(&self, deadline: &Deadline<'_>) -> Result<WorkspaceSnapshot, Error> {
        deadline.remaining()?;
        self.fetch_workspace()
    }
}

fn held_gate(path: &Path, now: Arc<AtomicI64>) -> Arc<QuotaGate> {
    let gate = QuotaGate::open_with_clock(
        path,
        &QuotaConfig {
            reserve_fraction: 0.4,
            ..QuotaConfig::default()
        },
        Arc::new(GateClock(now)),
    )
    .expect("fixture gate");
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-requests-remaining",
        "250".parse().expect("remaining header"),
    );
    headers.insert(
        "x-ratelimit-requests-reset",
        (NOW + 5000).to_string().parse().expect("reset header"),
    );
    gate.observe_headers(200, &headers, None)
        .expect("paired restrictive observation");
    Arc::new(gate)
}

#[trace("FR-032-AC-1", "FR-032-AC-8", "FR-032-AC-9")]
#[test]
fn stale_sdk_read_retains_atomic_quota_reason_and_successful_age() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("cache.sqlite");
    let quota_now = Arc::new(AtomicI64::new(NOW));
    let gate = held_gate(&path, Arc::clone(&quota_now));
    let mode = Arc::new(AtomicU8::new(0));
    let fetches = Arc::new(AtomicUsize::new(0));
    let wall = Arc::new(AtomicI64::new(1000));
    let mut service = ReadService::with_clock(
        FixtureSource {
            gate: Arc::clone(&gate),
            mode: Arc::clone(&mode),
            successful_fetches: Arc::clone(&fetches),
        },
        &path,
        SourceClock(Arc::clone(&wall)),
    )
    .expect("legacy service constructor");

    let first = service.workspace_snapshot().expect("legacy read call");
    assert_eq!(first.fetched_at, 1000);
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::Refreshed);
    service.workspace_snapshot().expect("warm compatible reuse");
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::CacheReused);
    assert_eq!(fetches.load(Ordering::SeqCst), 1);

    mode.store(1, Ordering::SeqCst);
    wall.store(2000, Ordering::SeqCst);
    let stale = service
        .workspace_snapshot()
        .expect("complete stale fallback");
    assert_eq!(stale.fetched_at, first.fetched_at);
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "quota gate prevents fetch"
    );
    let captured = match service.refresh_outcome() {
        RefreshOutcome::QuotaRefused { decision } => decision.clone(),
        other => panic!("expected typed quota refusal, got {other:?}"),
    };
    assert_eq!(captured.state, QuotaState::ServerReserveHeld);
    assert_eq!(captured.next_reevaluation_at, Some(NOW + 5000));
    assert_eq!(captured.blockers.len(), 1);
    assert_eq!(
        captured.blockers[0].dimension,
        Some(QuotaDimension::Requests)
    );
    let freshness = service.cache_freshness().expect("fallback provenance");
    assert!(freshness.stale);
    assert_eq!(freshness.last_successful_sync, Some(1000));
    assert_eq!(freshness.last_full_reconciliation, Some(1000));
    assert_eq!(freshness.age_seconds, Some(1000));
    assert_eq!(
        freshness.rate_limit_until, None,
        "reserve is not server cooldown"
    );

    // A separate live status read can retire evidence but cannot rewrite the
    // attempt that produced the stale snapshot's provenance.
    quota_now.store(NOW + 5000, Ordering::SeqCst);
    assert_eq!(
        gate.decision(10).expect("later status").state,
        QuotaState::Available
    );
    assert_eq!(
        service.refresh_outcome(),
        &RefreshOutcome::QuotaRefused { decision: captured }
    );
}

#[trace("FR-032-AC-8", "FR-032-AC-9")]
#[test]
fn source_and_lock_outcomes_do_not_classify_failure_message_text() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("cache.sqlite");
    let mode = Arc::new(AtomicU8::new(0));
    let mut service = ReadService::with_clock(
        FixtureSource {
            gate: held_gate(&path, Arc::new(AtomicI64::new(NOW))),
            mode: Arc::clone(&mode),
            successful_fetches: Arc::new(AtomicUsize::new(0)),
        },
        &path,
        SourceClock(Arc::new(AtomicI64::new(1000))),
    )
    .expect("fixture service");
    service
        .workspace_snapshot()
        .expect("seed complete snapshot");
    for source_mode in [2, 4] {
        mode.store(source_mode, Ordering::SeqCst);
        let fallback = service.force_refresh_workspace().expect("source fallback");
        assert_eq!(fallback.fetched_at, 1000);
        assert_eq!(service.refresh_outcome(), &RefreshOutcome::SourceFailed);
        let freshness = service.cache_freshness().expect("failure provenance");
        assert!(freshness.stale);
        assert_eq!(freshness.rate_limit_until, None);
    }
    mode.store(3, Ordering::SeqCst);
    service.force_refresh_workspace().expect("lock fallback");
    assert_eq!(
        service.refresh_outcome(),
        &RefreshOutcome::RefreshLockRefused
    );
    assert_eq!(
        service
            .cache_freshness()
            .expect("lock provenance")
            .last_successful_sync,
        Some(1000)
    );
}

#[trace("FR-032-AC-8", "FR-032-AC-9")]
#[test]
fn bounded_sdk_read_returns_typed_refusal_without_stale_snapshot() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("cache.sqlite");
    let mode = Arc::new(AtomicU8::new(0));
    let wall = Arc::new(AtomicI64::new(1000));
    let gate = held_gate(&path, Arc::new(AtomicI64::new(NOW)));
    let mut service = ReadService::with_clock(
        FixtureSource {
            gate,
            mode: Arc::clone(&mode),
            successful_fetches: Arc::new(AtomicUsize::new(0)),
        },
        &path,
        SourceClock(Arc::clone(&wall)),
    )
    .expect("fixture service");
    service
        .workspace_snapshot()
        .expect("seed complete snapshot");
    mode.store(1, Ordering::SeqCst);
    wall.store(2000, Ordering::SeqCst);
    let error = service
        .workspace_snapshot_bounded(&ReadPolicy {
            max_age: Duration::ZERO,
            deadline: Duration::from_secs(5),
        })
        .expect_err("bounded contract never returns stale data");
    let decision = error
        .quota_decision()
        .expect("typed refusal in returned error");
    assert_eq!(decision.state, QuotaState::ServerReserveHeld);
    assert_eq!(decision.next_reevaluation_at, Some(NOW + 5000));
    assert_eq!(
        service
            .force_refresh_workspace()
            .expect("ordinary fallback remains complete")
            .fetched_at,
        1000
    );
    assert_eq!(
        service
            .cache_freshness()
            .expect("old successful sync")
            .last_successful_sync,
        Some(1000)
    );
}

#[trace("FR-032-AC-9")]
#[test]
fn legacy_public_literals_compile_and_absent_outcome_is_unknown() {
    // This integration test compiles as an external crate. These are the complete
    // pre-outcome literals, with no constructors or newly required fields.
    let freshness = CacheFreshness {
        last_successful_sync: Some(1000),
        last_full_reconciliation: Some(900),
        stale: true,
        age_seconds: Some(50),
        rate_limit_until: None,
        failure_reason: Some("Linear rate limited; retry in 99s".to_owned()),
    };
    let summary = SummaryRead {
        data: ix_board::linear_summary::SummarySnapshot::default(),
        freshness: freshness.clone(),
        last_successful_sync: 1000,
        last_full_reconciliation: 900,
    };
    let quota = ix_board::quota::QuotaStatus {
        rate_limit_until: None,
        request_budget: 100,
        complexity_budget: 1000,
        requests_used: 0,
        complexity_used: 0,
        request_reset_at: 2000,
        complexity_reset_at: 2000,
        local_generation: 0,
        local_window_reset_at: 2000,
    };
    let status = ix_board::cache_status::CacheStatus {
        cache_path: "fixture.sqlite".to_owned(),
        datasets: Vec::new(),
        quota,
        rate_limit_until: None,
        latest_refresh: None,
    };
    assert_eq!(summary.freshness.age_seconds, Some(50));
    assert_eq!(status.quota.requests_used, 0);
    let old_json = json!({"read": serde_json::to_value(&freshness).expect("legacy metadata")});
    let decoded: RefreshRead<serde_json::Value> =
        serde_json::from_value(old_json).expect("legacy envelope without outcome");
    assert_eq!(decoded.outcome, RefreshOutcome::Unknown);
    assert_eq!(
        decoded.read["failure_reason"],
        "Linear rate limited; retry in 99s"
    );
    let renamed: RefreshRead<serde_json::Value> = serde_json::from_value(json!({
        "read": {"failure_reason": "replacement wording"}
    }))
    .expect("legacy replacement wording");
    assert_eq!(renamed.outcome, RefreshOutcome::Unknown);
    let wrapped = RefreshRead {
        read: summary,
        outcome: RefreshOutcome::CacheReused,
    };
    let output = serde_json::to_value(wrapped).expect("additive consumer envelope");
    assert_eq!(output["outcome"]["kind"], "cache_reused");
    assert_eq!(output["read"]["last_successful_sync"], 1000);
}

#[trace("FR-032-AC-1", "FR-032-AC-8", "FR-032-AC-9")]
#[test]
fn native_summary_envelope_keeps_legacy_api_and_captured_quota_outcome() {
    use std::ffi::OsString;
    use std::net::TcpListener;

    use ix_board::LinearGraphQlSource;
    use ix_cli_kit::secrets::SecretStore;
    use rusqlite::{Connection, params};

    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("summary.sqlite");
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback-only endpoint");
    let endpoint = format!(
        "http://{}/graphql",
        listener.local_addr().expect("fixture address")
    );
    let mut source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("fictional-sdk-fixture-key")),
        || None,
        &endpoint,
    )
    .expect("fixture credential override");
    source
        .configure_cache(&path)
        .expect("isolated shared cache");
    // A send bug would fail immediately against this closed loopback endpoint,
    // rather than contacting any real service or leaving a fixture worker waiting.
    drop(listener);
    let connection = Connection::open(&path).expect("summary database");
    connection.execute_batch("CREATE TABLE summary_snapshot (
        cache_key TEXT PRIMARY KEY,payload BLOB NOT NULL,synced_at INTEGER NOT NULL,full_at INTEGER NOT NULL
    )").expect("compatible summary schema");
    let data = ix_board::linear_summary::SummarySnapshot::default();
    connection.execute(
        "INSERT INTO summary_snapshot(cache_key,payload,synced_at,full_at) VALUES(?1,?2,1000,900)",
        params![format!("{}:summary:v1",source.cache_namespace()),serde_json::to_vec(&data).expect("complete dataset")],
    ).expect("seed compatible summary partition");
    let clock = SourceClock(Arc::new(AtomicI64::new(1000)));
    let legacy: SummaryRead = source
        .summary_snapshot_cached_with_clock(&clock)
        .expect("unchanged summary signature");
    assert_eq!(legacy.last_successful_sync, 1000);
    let warm = source
        .summary_snapshot_cached_with_outcome_and_clock(&clock)
        .expect("typed warm read");
    assert_eq!(warm.outcome, RefreshOutcome::CacheReused);

    let gate = QuotaGate::open(&path, &QuotaConfig::default()).expect("transport quota database");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Unix time");
    let until = i64::try_from(now.as_millis()).expect("millisecond range") + 60_000;
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ratelimit-requests-remaining",
        "0".parse().expect("remaining header"),
    );
    headers.insert(
        "x-ratelimit-requests-reset",
        until.to_string().parse().expect("owning deadline"),
    );
    gate.observe_headers(200, &headers, None)
        .expect("reserve hold without actual cooldown");
    clock.0.store(2000, Ordering::SeqCst);
    let fallback = source
        .summary_snapshot_cached_with_outcome_and_clock(&clock)
        .expect("native summary fallback");
    let RefreshOutcome::QuotaRefused { decision } = fallback.outcome else {
        panic!("native summary must expose typed quota refusal")
    };
    assert_eq!(decision.state, QuotaState::ServerReserveHeld);
    assert_eq!(decision.next_reevaluation_at, Some(until));
    assert_eq!(fallback.read.data, legacy.data);
    assert_eq!(fallback.read.last_successful_sync, 1000);
    assert_eq!(fallback.read.last_full_reconciliation, 900);
    assert_eq!(fallback.read.freshness.age_seconds, Some(1000));
    assert_eq!(fallback.read.freshness.rate_limit_until, None);
    assert!(fallback.read.freshness.stale);
    assert_eq!(gate.status().expect("no send charge").requests_used, 0);
    let stored: (i64, i64) = connection
        .query_row(
            "SELECT synced_at,full_at FROM summary_snapshot",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("successful markers");
    assert_eq!(stored, (1000, 900));
}
