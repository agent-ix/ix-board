// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Maximum documented fallback envelope, with a bulk-seeded chronological oracle.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use ix_board::quota::{
    AuthMode, PlanTier, QuotaClock, QuotaConfig, QuotaDimension, QuotaGate, QuotaState,
};
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;
use rusqlite::{Connection, params};

const NOW: i64 = 1_800_000_000_000;
const RESPONSES: i64 = 25_000;
const DIMENSIONS: i64 = 3;
const ACTUAL_COMPLEXITY: u64 = 125;

struct Clock(AtomicI64);

impl QuotaClock for Clock {
    fn now_epoch_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn maximum_config() -> QuotaConfig {
    QuotaConfig {
        tier: PlanTier::Enterprise,
        auth_mode: AuthMode::OAuth,
        reserve_fraction: 0.0,
        ..QuotaConfig::default()
    }
}

fn response_headers(expiry: i64) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (dimension, capacity) in [
        ("requests", 25_000),
        ("complexity", 10_000_000),
        ("endpoint-requests", 25_000),
    ] {
        for (suffix, value) in [
            ("limit", capacity),
            ("remaining", capacity),
            ("reset", expiry),
        ] {
            headers.insert(
                format!("x-ratelimit-{dimension}-{suffix}")
                    .parse::<reqwest::header::HeaderName>()
                    .expect("dimension header name"),
                value.to_string().parse().expect("dimension header value"),
            );
        }
    }
    headers.insert("x-complexity", "125".parse().expect("actual cost header"));
    headers
}

fn rows(db: &Connection) -> i64 {
    db.query_row(
        "SELECT COUNT(*) FROM linear_quota_observations",
        [],
        |row| row.get(0),
    )
    .expect("retained observation count")
}

#[trace("FR-032-AC-5", "FR-032-AC-10")]
#[test]
fn documented_fallback_allocations_fit_the_maximum_enterprise_oauth_envelope() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let mut largest_allocation = 0;
    for (tier, multiplier, label) in [
        (PlanTier::Free, 1, "free"),
        (PlanTier::Basic, 1, "basic"),
        (PlanTier::Business, 2, "business"),
        (PlanTier::Enterprise, 5, "enterprise"),
    ] {
        for (auth_mode, requests, complexity, auth_label) in [
            (AuthMode::ApiKey, 2500, 3_000_000, "api-key"),
            (AuthMode::OAuth, 5000, 2_000_000, "oauth"),
        ] {
            let config = QuotaConfig {
                tier,
                auth_mode,
                reserve_fraction: 0.0,
                ..QuotaConfig::default()
            };
            let expected_requests = requests * multiplier;
            let expected_complexity = complexity * multiplier;
            assert_eq!(
                config.fallback_limits(),
                (expected_requests, expected_complexity)
            );
            let gate = QuotaGate::open_with_clock(
                &directory
                    .path()
                    .join(format!("{label}-{auth_label}.sqlite")),
                &config,
                Arc::new(Clock(AtomicI64::new(NOW))),
            )
            .expect("supported fallback configuration");
            let state = gate.status().expect("public fallback allocation");
            assert_eq!(state.request_budget, expected_requests / 2);
            assert_eq!(state.complexity_budget, expected_complexity / 2);
            largest_allocation = largest_allocation.max(state.request_budget);
            assert!(state.request_budget * 2 * 3 <= 75_000);
            assert!(state.request_budget * 3 * 3 <= 112_500);
        }
    }
    assert_eq!(largest_allocation, 12_500);
    assert_eq!(
        largest_allocation * 2 * 3,
        u64::try_from(RESPONSES * DIMENSIONS).unwrap()
    );
    assert_eq!(largest_allocation * 3 * 3, 112_500);
}

#[trace("FR-032-AC-5", "FR-032-AC-6", "FR-032-AC-10")]
#[test]
fn seventy_five_thousand_frontier_rows_admit_settle_expire_and_reopen_without_evidence_loss() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("quota.sqlite");
    let clock = Arc::new(Clock(AtomicI64::new(NOW)));
    let config = maximum_config();
    let gate = QuotaGate::open_with_clock(&path, &config, clock.clone()).expect("fixture gate");
    let db = Connection::open(&path).expect("fixture database");
    let seed_started = Instant::now();
    // Independent chronological model: response i owns all later completed
    // requests. Its request headroom is i+1; each successive expiry removes
    // exactly that response's three dimensions. These constraints are pairwise
    // non-dominating: less headroom always has the earlier expiry.
    db.execute(
        "WITH RECURSIVE responses(i) AS (
            VALUES(0) UNION ALL SELECT i+1 FROM responses WHERE i+1<?1
         ), dimensions(kind,remaining,unit) AS (
            VALUES('requests',25000,1),('complexity',10000000,125),('endpoint_requests',25000,1)
         )
         INSERT INTO linear_quota_observations(
            dimension,remaining,charge,observed_at,expires_at,provenance,paired_reset,capacity
         ) SELECT kind,remaining,(?1-1-i)*unit,?2,?2+i+1,'headers',1,remaining
           FROM responses CROSS JOIN dimensions",
        params![RESPONSES, NOW],
    )
    .expect("bulk-seed complete three-dimensional response frontier");
    assert_eq!(rows(&db), RESPONSES * DIMENSIONS);
    let seeded_bytes = std::fs::metadata(&path)
        .expect("fixture database size")
        .len();
    let seed_elapsed = seed_started.elapsed();

    let operation = Instant::now();
    let available = gate
        .decision(10_000)
        .expect("full-frontier public evaluation");
    let evaluation_elapsed = operation.elapsed();
    assert_eq!(available.state, QuotaState::Available);
    assert!(available.blockers.is_empty());
    assert_eq!(available.evidence.len(), 3);
    for dimension in [QuotaDimension::Requests, QuotaDimension::EndpointRequests] {
        let evidence = available
            .evidence
            .iter()
            .find(|entry| entry.dimension == Some(dimension))
            .expect("request frontier evidence");
        assert_eq!(evidence.effective_remaining, Some(1));
        assert_eq!(evidence.deadline, Some(NOW + 1));
    }
    let complexity = available
        .evidence
        .iter()
        .find(|entry| entry.dimension == Some(QuotaDimension::Complexity))
        .expect("complexity frontier evidence");
    assert_eq!(
        complexity.effective_remaining,
        Some(10_000_000 - 24_999 * ACTUAL_COMPLEXITY)
    );
    assert!(
        serde_json::to_vec(&available)
            .expect("available diagnostics")
            .len()
            < 8192
    );

    let operation = Instant::now();
    let first = gate
        .reserve_evaluated(10_000, None)
        .expect("oldest frontier permits exactly one request");
    // Real response ingestion must retain additional non-dominated constraints
    // beyond the bulk-seeded population. A small production cap fails here even
    // though direct fixture insertion itself does not invoke the cap.
    gate.observe(first, 200, &response_headers(NOW + RESPONSES + 1), None)
        .expect("settle actual cost and ingest another complete response");
    let admission_elapsed = operation.elapsed();
    assert_eq!(rows(&db), (RESPONSES + 1) * DIMENSIONS);
    let state = gate.status().expect("settled production accounting");
    assert_eq!(
        (state.requests_used, state.complexity_used),
        (1, ACTUAL_COMPLEXITY)
    );
    let blocked = gate
        .reserve_evaluated(10_000, None)
        .expect_err("the oldest request constraints now have zero headroom");
    let blocked = blocked
        .quota_decision()
        .expect("captured dimensional refusal");
    assert_eq!(blocked.state, QuotaState::ServerReserveHeld);
    assert_eq!(blocked.next_reevaluation_at, Some(NOW + 1));
    assert_eq!(blocked.blockers.len(), 2);
    assert!(
        blocked
            .blockers
            .iter()
            .all(|entry| entry.effective_remaining == Some(0))
    );
    assert!(
        serde_json::to_vec(blocked)
            .expect("blocked diagnostics")
            .len()
            < 8192
    );
    assert_eq!(
        rows(&db),
        (RESPONSES + 1) * DIMENSIONS,
        "refusal does not drop valid evidence"
    );

    clock.0.store(NOW + 1, Ordering::SeqCst);
    let reopened = QuotaGate::open_with_clock(&path, &config, clock.clone())
        .expect("reopen at first owning expiry");
    let next = reopened
        .decision(10_000)
        .expect("retire only expired response");
    assert_eq!(next.state, QuotaState::Available);
    assert_eq!(
        rows(&db),
        RESPONSES * DIMENSIONS,
        "exactly the first response's three constraints retire"
    );
    let next_request = next
        .evidence
        .iter()
        .find(|entry| entry.dimension == Some(QuotaDimension::Requests))
        .expect("next request constraint");
    assert_eq!(next_request.effective_remaining, Some(1));
    assert_eq!(next_request.deadline, Some(NOW + 2));
    let second = reopened
        .reserve_evaluated(10_000, None)
        .expect("ordinary protected admission at the owning expiry");
    let mut cost = HeaderMap::new();
    cost.insert("x-complexity", "125".parse().expect("actual cost"));
    reopened
        .observe(second, 200, &cost, None)
        .expect("settle second actual cost");
    let settled = reopened.status().expect("reopened actual accounting");
    assert_eq!(
        (settled.requests_used, settled.complexity_used),
        (2, 2 * ACTUAL_COMPLEXITY)
    );
    assert_eq!(settled.local_generation, 0);
    assert_eq!(settled.local_window_reset_at, NOW + 3_600_000);
    let still_held = reopened.decision(10_000).expect("newest owning hold");
    assert_eq!(still_held.state, QuotaState::ServerReserveHeld);
    assert_eq!(still_held.next_reevaluation_at, Some(NOW + 2));
    assert!(
        serde_json::to_vec(&still_held)
            .expect("reopened diagnostics")
            .len()
            < 8192
    );

    clock.0.store(NOW + RESPONSES + 1, Ordering::SeqCst);
    let expired = QuotaGate::open_with_clock(&path, &config, clock)
        .expect("reopen after all owning expiries");
    let unknown = expired
        .decision(10_000)
        .expect("expired headroom becomes unknown");
    assert_eq!(unknown.state, QuotaState::Available);
    assert!(unknown.evidence.is_empty());
    assert_eq!(rows(&db), 0);
    let preserved = expired
        .status()
        .expect("evidence expiry is not local replenishment");
    assert_eq!(
        (preserved.requests_used, preserved.complexity_used),
        (2, 2 * ACTUAL_COMPLEXITY)
    );
    assert_eq!(preserved.local_generation, 0);
    assert_eq!(preserved.local_window_reset_at, NOW + 3_600_000);
    let pending: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM linear_quota_reservations",
            [],
            |row| row.get(0),
        )
        .expect("settled reservation ledger");
    assert_eq!(pending, 0);
    eprintln!(
        "quota maximum envelope: seeded_rows={}, peak_rows={}, db_bytes={}, seed_ms={}, evaluation_ms={}, admission_and_settlement_ms={}",
        RESPONSES * DIMENSIONS,
        (RESPONSES + 1) * DIMENSIONS,
        seeded_bytes,
        seed_elapsed.as_millis(),
        evaluation_elapsed.as_millis(),
        admission_elapsed.as_millis()
    );
}
