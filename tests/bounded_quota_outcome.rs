// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Bounded SDK reads retain attempt metadata before preserving legacy error shapes.

use std::ffi::OsString;
use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use ix_board::quota::{QuotaClock, QuotaConfig, QuotaGate, QuotaState};
use ix_board::{
    Clock, Deadline, Error, LinearGraphQlSource, ReadPolicy, ReadService, RefreshOutcome,
    WorkspaceSnapshot, WorkspaceSource,
};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;

// Persisted time is deliberately well ahead of wall time. The quota clock
// watermark makes the sixty-second fixture cooldown independent of test duration.
const QUOTA_NOW: i64 = 4_000_000_000_000;

struct FixedQuotaClock(i64);

impl QuotaClock for FixedQuotaClock {
    fn now_epoch_millis(&self) -> i64 {
        self.0
    }
}

#[derive(Clone)]
struct ManualClock(Arc<AtomicI64>);

impl Clock for ManualClock {
    fn now_epoch_seconds(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn policy() -> ReadPolicy {
    ReadPolicy {
        max_age: Duration::from_secs(10),
        deadline: Duration::from_secs(5),
    }
}

#[trace("FR-032-AC-7", "FR-032-AC-8", "FR-032-AC-9", "FR-004-AC-18")]
#[test]
fn actual_linear_bounded_backoff_returns_legacy_error_and_captured_typed_outcome() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("quota.sqlite");
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback fixture");
    listener
        .set_nonblocking(true)
        .expect("nonblocking observation");
    let endpoint = format!(
        "http://{}/graphql",
        listener.local_addr().expect("fixture address")
    );
    let source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("fictional-bounded-outcome-test-token")),
        || None,
        &endpoint,
    )
    .expect("explicit fixture credential and endpoint");
    let gate = QuotaGate::open_with_clock(
        &path,
        &QuotaConfig::default(),
        Arc::new(FixedQuotaClock(QUOTA_NOW)),
    )
    .expect("shared fixture gate");
    let mut headers = HeaderMap::new();
    headers.insert("retry-after", "60".parse().expect("retry header"));
    gate.observe_headers(429, &headers, None)
        .expect("persist actual-server cooldown");
    let mut service =
        ReadService::with_clock(source, &path, ManualClock(Arc::new(AtomicI64::new(1000))))
            .expect("bounded service");

    let error = service
        .workspace_snapshot_bounded(&policy())
        .expect_err("bounded backoff returns no snapshot");
    assert!(
        matches!(error, Error::LinearRateLimited { retry_after } if retry_after == Duration::from_secs(60)),
        "the existing public error remains LinearRateLimited with its sixty-second retry: {error}"
    );
    assert_eq!(error.to_string(), "Linear rate limited; retry in 60s");
    let captured = match service.refresh_outcome() {
        RefreshOutcome::QuotaRefused { decision } => decision.clone(),
        other => panic!("bounded attempt lost typed quota provenance: {other:?}"),
    };
    assert_eq!(captured.state, QuotaState::ServerBackoff);
    assert_eq!(captured.evaluated_at, QUOTA_NOW);
    assert_eq!(captured.next_reevaluation_at, Some(QUOTA_NOW + 60_000));
    let backoffs: Vec<_> = captured
        .blockers
        .iter()
        .filter(|blocker| blocker.state == QuotaState::ServerBackoff)
        .collect();
    assert_eq!(backoffs.len(), 1);
    assert_eq!(backoffs[0].deadline, Some(QUOTA_NOW + 60_000));
    assert_eq!(
        gate.status()
            .expect("unmodified request count")
            .requests_used,
        0
    );
    assert!(service.cache_freshness().is_none());
    let stored: u64 = rusqlite::Connection::open(&path)
        .expect("snapshot database")
        .query_row("SELECT COUNT(*) FROM workspace_snapshot", [], |row| {
            row.get(0)
        })
        .expect("no completed snapshot publication");
    assert_eq!(stored, 0, "refusal must not create a fresh cache entry");
    assert_eq!(
        listener
            .accept()
            .expect_err("cooldown must queue zero HTTP connections")
            .kind(),
        ErrorKind::WouldBlock
    );

    let after = QuotaGate::open_with_clock(
        &path,
        &QuotaConfig::default(),
        Arc::new(FixedQuotaClock(QUOTA_NOW + 60_000)),
    )
    .expect("independent status gate after expiry");
    assert_eq!(
        after
            .decision(10_000)
            .expect("expired cooldown status")
            .state,
        QuotaState::Available
    );
    assert_eq!(
        service.refresh_outcome(),
        &RefreshOutcome::QuotaRefused { decision: captured },
        "later status cannot rewrite the bounded read attempt"
    );
    assert_eq!(
        listener
            .accept()
            .expect_err("status evaluation never probes HTTP")
            .kind(),
        ErrorKind::WouldBlock
    );
}

struct FixtureSource {
    fail: Arc<AtomicBool>,
    supports_bound: Arc<AtomicBool>,
    fetches: Arc<AtomicUsize>,
}

impl WorkspaceSource for FixtureSource {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::GraphQl {
                messages: "fixture source failed".to_owned(),
            });
        }
        Ok(WorkspaceSnapshot::default())
    }

    fn supports_bounded_read(&self) -> bool {
        self.supports_bound.load(Ordering::SeqCst)
    }

    fn fetch_workspace_bounded(&self, deadline: &Deadline<'_>) -> Result<WorkspaceSnapshot, Error> {
        deadline.remaining()?;
        self.fetch_workspace()
    }
}

#[trace("FR-032-AC-8", "FR-032-AC-9", "FR-004-AC-18")]
#[test]
fn bounded_attempt_outcome_resets_for_refresh_reuse_source_failure_and_preflight_refusal() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("cache.sqlite");
    let fail = Arc::new(AtomicBool::new(false));
    let supports_bound = Arc::new(AtomicBool::new(true));
    let fetches = Arc::new(AtomicUsize::new(0));
    let now = Arc::new(AtomicI64::new(1000));
    let mut service = ReadService::with_clock(
        FixtureSource {
            fail: Arc::clone(&fail),
            supports_bound: Arc::clone(&supports_bound),
            fetches: Arc::clone(&fetches),
        },
        &path,
        ManualClock(Arc::clone(&now)),
    )
    .expect("fixture service");
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::Unknown);
    let first = service
        .workspace_snapshot_bounded(&policy())
        .expect("cold bounded success");
    assert_eq!(first.fetched_at, 1000);
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::Refreshed);
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
    fail.store(true, Ordering::SeqCst);
    let reused = service
        .workspace_snapshot_bounded(&policy())
        .expect("warm bounded cache reuse");
    assert_eq!(reused.fetched_at, 1000);
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::CacheReused);
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "warm read cannot call source"
    );

    now.store(1011, Ordering::SeqCst);
    assert!(
        matches!(
            service.workspace_snapshot_bounded(&policy()),
            Err(Error::GraphQl { .. })
        ),
        "bounded source failure never falls back to the existing snapshot"
    );
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::SourceFailed);
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    let invalid = ReadPolicy {
        deadline: Duration::ZERO,
        ..policy()
    };
    assert!(matches!(
        service.workspace_snapshot_bounded(&invalid),
        Err(Error::InvalidReadPolicy { .. })
    ));
    assert_eq!(
        service.refresh_outcome(),
        &RefreshOutcome::Unknown,
        "preflight refusal cannot inherit a previous source failure"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 2);

    fail.store(false, Ordering::SeqCst);
    let refreshed = service
        .workspace_snapshot_bounded(&policy())
        .expect("subsequent successful refresh");
    assert_eq!(refreshed.fetched_at, 1011);
    assert_eq!(service.refresh_outcome(), &RefreshOutcome::Refreshed);
    assert_eq!(fetches.load(Ordering::SeqCst), 3);
    supports_bound.store(false, Ordering::SeqCst);
    assert!(matches!(
        service.workspace_snapshot_bounded(&policy()),
        Err(Error::BoundUnsupported)
    ));
    assert_eq!(
        service.refresh_outcome(),
        &RefreshOutcome::Unknown,
        "unsupported source refusal cannot inherit a previous success"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 3);
}
