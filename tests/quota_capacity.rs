// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Production-volume quota sequences with an independent chronological frontier oracle.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use ix_board::quota::{PlanTier, QuotaClock, QuotaConfig, QuotaGate, QuotaState};
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;

const NOW: i64 = 1_800_000_000_000;
const WINDOW: i64 = 3_600_000;
struct Clock(AtomicI64);
impl QuotaClock for Clock {
    fn now_epoch_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[trace("FR-032-AC-5", "FR-032-AC-6", "FR-032-AC-10")]
#[test]
fn full_business_allocation_across_three_windows_keeps_non_dominated_evidence_available() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.sqlite");
    let clock = Arc::new(Clock(AtomicI64::new(NOW)));
    let config = QuotaConfig {
        tier: PlanTier::Business,
        ..QuotaConfig::default()
    };
    let mut gate = QuotaGate::open_with_clock(&path, &config, clock.clone()).unwrap();
    let mut frontier = VecDeque::new();
    let mut sent = 0_u64;
    let mut max_rows = 0_i64;
    let started = Instant::now();
    let mut max_operation = std::time::Duration::ZERO;
    for window in 0..3_i64 {
        for request in 0..2250_i64 {
            let now = if window == 0 {
                NOW + WINDOW - 2250 + request
            } else {
                NOW + window * WINDOW + request
            };
            clock.0.store(now, Ordering::SeqCst);
            while frontier.front().is_some_and(|(expiry, _)| *expiry <= now) {
                frontier.pop_front();
            }
            // Every response reports the same remote capacities/headroom. Since
            // earlier observations own more subsequent charges and expire sooner,
            // the chronological oldest is the independent restrictive frontier.
            if let Some((_, after_request)) = frontier.front() {
                let outstanding = sent - after_request;
                assert!(
                    5000 - outstanding >= 251,
                    "window={window},request={request},sent={sent}"
                );
                assert!(6_000_000 - outstanding * 125 >= 300_000 + 10_000);
            }
            let operation = Instant::now();
            let token = gate
                .reserve_evaluated(10_000, None)
                .unwrap_or_else(|error| {
                    panic!("window={window},request={request},sent={sent}: {error}")
                });
            sent += 1;
            let expiry = now + WINDOW;
            let mut headers = HeaderMap::new();
            for (dimension, limit, left) in [
                ("requests", 5000, 5000),
                ("complexity", 6_000_000, 6_000_000),
                ("endpoint-requests", 5000, 5000),
            ] {
                headers.insert(
                    format!("x-ratelimit-{dimension}-limit")
                        .parse::<reqwest::header::HeaderName>()
                        .unwrap(),
                    limit.to_string().parse().unwrap(),
                );
                headers.insert(
                    format!("x-ratelimit-{dimension}-remaining")
                        .parse::<reqwest::header::HeaderName>()
                        .unwrap(),
                    left.to_string().parse().unwrap(),
                );
                headers.insert(
                    format!("x-ratelimit-{dimension}-reset")
                        .parse::<reqwest::header::HeaderName>()
                        .unwrap(),
                    expiry.to_string().parse().unwrap(),
                );
            }
            headers.insert("x-complexity", "125".parse().unwrap());
            gate.observe(token, 200, &headers, None).unwrap();
            frontier.push_back((expiry, sent));
            max_operation = max_operation.max(operation.elapsed());
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        let rows: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM linear_quota_observations",
                [],
                |row| row.get(0),
            )
            .unwrap();
        max_rows = max_rows.max(rows);
        assert_eq!(
            usize::try_from(rows).unwrap(),
            frontier.len() * 3,
            "every valid non-dominated observation remains owned"
        );
        let state = gate.status().unwrap();
        assert_eq!(state.requests_used, 2250);
        assert_eq!(state.complexity_used, 2250 * 125);
        let decision = gate.decision(10_000).unwrap();
        assert_eq!(
            decision.state,
            QuotaState::LocalWindowHeld,
            "only the consumed local allocation prevents the next request"
        );
        assert!(
            serde_json::to_vec(&decision).unwrap().len() < 8192,
            "normal diagnostics must not serialize the entire retained frontier"
        );
        gate = QuotaGate::open_with_clock(&path, &config, clock.clone()).unwrap();
    }
    assert!(
        max_rows > 512,
        "fixture exceeds the failed production bound"
    );
    assert_eq!(sent, 6750);
    eprintln!(
        "quota capacity business: calls={sent}, peak_rows={max_rows}, db_bytes={}, elapsed_ms={}, max_request_ms={}",
        std::fs::metadata(&path).unwrap().len(),
        started.elapsed().as_millis(),
        max_operation.as_millis()
    );
}

#[trace("FR-032-AC-5", "FR-032-AC-6", "FR-032-AC-10")]
#[test]
fn upgraded_capacity_preserves_previous_lost_evidence_hold_until_its_absolute_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.sqlite");
    let clock = Arc::new(Clock(AtomicI64::new(NOW)));
    let config = QuotaConfig {
        tier: PlanTier::Business,
        ..QuotaConfig::default()
    };
    let gate = QuotaGate::open_with_clock(&path, &config, clock.clone()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; UPDATE linear_shared_quota SET paired_writer=1")
        .unwrap();
    db.execute("UPDATE linear_shared_quota SET request_limit=5000,complexity_limit=6000000,requests=555,complexity=194396,evidence_error_until=?1",[NOW+5000]).unwrap();
    db.execute_batch("UPDATE linear_shared_quota SET paired_writer=0; COMMIT")
        .unwrap();
    // Additional safely retained metadata cannot recover constraints discarded by
    // the previous binary. Preserve its persisted unknown-evidence deadline.
    for request in 0..600 {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ratelimit-requests-remaining",
            (4441 + request).to_string().parse().unwrap(),
        );
        headers.insert(
            "x-ratelimit-requests-reset",
            (NOW + 6000 + request).to_string().parse().unwrap(),
        );
        gate.observe_headers(200, &headers, None).unwrap();
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM linear_quota_observations",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        600,
        "new capacity retains observations beyond the former cap"
    );
    for time in [NOW, NOW + 4999] {
        clock.0.store(time, Ordering::SeqCst);
        let reopened = QuotaGate::open_with_clock(&path, &config, clock.clone()).unwrap();
        let error = reopened.reserve_evaluated(10_000, None).unwrap_err();
        assert_eq!(
            error.quota_decision().unwrap().state,
            QuotaState::EvidenceUnavailable
        );
        assert_eq!(
            error.quota_decision().unwrap().next_reevaluation_at,
            Some(NOW + 5000)
        );
        let status = reopened.status().unwrap();
        assert_eq!(
            (status.requests_used, status.complexity_used),
            (555, 194396)
        );
        assert_eq!(status.rate_limit_until, None);
    }
    clock.0.store(NOW + 5000, Ordering::SeqCst);
    let reopened = QuotaGate::open_with_clock(&path, &config, clock).unwrap();
    assert_eq!(
        reopened.decision(10_000).unwrap().state,
        QuotaState::Available
    );
    reopened.reserve_evaluated(10_000, None).unwrap();
    assert_eq!(reopened.status().unwrap().requests_used, 556);
    assert_eq!(
        db.query_row(
            "SELECT evidence_error_until FROM linear_shared_quota",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        NOW + 5000,
        "upgrade did not erase or rewrite lost evidence"
    );
}
