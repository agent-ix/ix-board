// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic persisted admission and independent sequence oracle for FR-032.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use ix_board::quota::{
    QuotaClock, QuotaConfig, QuotaDimension, QuotaGate, QuotaState, Reservation,
};
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;

const NOW: i64 = 1_800_000_000_000;

struct Clock(AtomicI64);
impl QuotaClock for Clock {
    fn now_epoch_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
fn headers(pairs: &[(&str, String)]) -> HeaderMap {
    pairs
        .iter()
        .map(|(name, value)| {
            (
                name.parse::<reqwest::header::HeaderName>().unwrap(),
                value.parse().unwrap(),
            )
        })
        .collect()
}
fn open(path: &Path, clock: &Arc<Clock>, config: &QuotaConfig) -> QuotaGate {
    QuotaGate::open_with_clock(path, config, clock.clone()).unwrap()
}
fn request_observation(gate: &QuotaGate, remaining: u64, reset: i64) {
    gate.observe_headers(
        200,
        &headers(&[
            ("x-ratelimit-requests-limit", "10000".to_owned()),
            ("x-ratelimit-requests-remaining", remaining.to_string()),
            ("x-ratelimit-requests-reset", reset.to_string()),
        ]),
        None,
    )
    .unwrap();
}

#[trace("FR-032-AC-1", "FR-032-AC-2", "FR-032-AC-3")]
#[test]
fn paired_constraints_expire_independently_in_both_arrival_orders() {
    for newer_remaining in [1000, 400] {
        for reverse in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let clock = Arc::new(Clock(AtomicI64::new(NOW)));
            let gate = open(
                &dir.path().join("quota.sqlite"),
                &clock,
                &QuotaConfig::default(),
            );
            let observations = if reverse {
                [(newer_remaining, NOW + 2000), (250, NOW + 1000)]
            } else {
                [(250, NOW + 1000), (newer_remaining, NOW + 2000)]
            };
            for (left, reset) in observations {
                request_observation(&gate, left, reset);
            }
            let decision = gate.decision(10).unwrap();
            assert_eq!(decision.state, QuotaState::ServerReserveHeld);
            assert_eq!(decision.next_reevaluation_at, Some(NOW + 1000));
            assert_eq!(gate.status().unwrap().requests_used, 0);
            assert_eq!(gate.status().unwrap().rate_limit_until, None);
            assert!(
                decision
                    .blockers
                    .iter()
                    .any(|b| b.dimension == Some(QuotaDimension::Requests)
                        && b.deadline == Some(NOW + 1000)
                        && b.reserve == Some(500))
            );
            assert!(gate.reserve_evaluated(10, None).is_err());
            clock.0.store(NOW + 1000, Ordering::SeqCst);
            let decision = gate.decision(10).unwrap();
            assert_eq!(
                decision.state,
                if newer_remaining == 1000 {
                    QuotaState::Available
                } else {
                    QuotaState::ServerReserveHeld
                },
                "remaining={newer_remaining}, reverse={reverse}"
            );
            assert!(
                decision
                    .evidence
                    .iter()
                    .all(|b| b.effective_remaining != Some(250))
            );
            if newer_remaining == 400 {
                assert_eq!(decision.next_reevaluation_at, Some(NOW + 2000));
            }
            clock.0.store(NOW + 2000, Ordering::SeqCst);
            let decision = gate.decision(10).unwrap();
            assert_eq!(decision.state, QuotaState::Available);
            assert!(
                decision.evidence.is_empty(),
                "expiry produces unknown remote headroom"
            );
            request_observation(&gate, 250, NOW + 1000);
            assert_eq!(
                gate.decision(10).unwrap().state,
                QuotaState::Available,
                "late expired positive response cannot rearm a fallback hour"
            );
        }
    }
}

#[trace("FR-032-AC-4", "FR-032-AC-5", "FR-032-AC-6")]
#[test]
fn partial_headers_never_slide_owned_deadlines_and_backward_clock_never_resurrects() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(Clock(AtomicI64::new(NOW)));
    let config = QuotaConfig {
        window_seconds: 1,
        ..QuotaConfig::default()
    };
    let path = dir.path().join("quota.sqlite");
    let gate = open(&path, &clock, &config);
    request_observation(&gate, 250, NOW + 500);
    for pairs in [
        vec![("x-ratelimit-requests-reset", (NOW + 99_000).to_string())],
        vec![("x-ratelimit-requests-limit", "10000".to_owned())],
        vec![
            ("x-ratelimit-requests-remaining", "garbage".to_owned()),
            ("x-ratelimit-requests-reset", (NOW + 99_000).to_string()),
        ],
        vec![],
    ] {
        gate.observe_headers(200, &headers(&pairs), None).unwrap();
        assert_eq!(
            gate.decision(10).unwrap().next_reevaluation_at,
            Some(NOW + 500)
        );
    }
    clock.0.store(NOW + 500, Ordering::SeqCst);
    assert!(gate.decision(10).unwrap().evidence.is_empty());
    clock.0.store(NOW + 100, Ordering::SeqCst);
    request_observation(&gate, 250, NOW + 400);
    assert!(
        gate.decision(10).unwrap().evidence.is_empty(),
        "past pair cannot resurrect after backward wall clock"
    );
    clock.0.store(NOW + 600, Ordering::SeqCst);
    gate.observe_headers(
        200,
        &headers(&[("x-ratelimit-requests-remaining", "250".to_owned())]),
        None,
    )
    .unwrap();
    for elapsed in [700, 900, 1100, 1500] {
        clock.0.store(NOW + elapsed, Ordering::SeqCst);
        gate.observe_headers(
            200,
            &headers(&[(
                "x-ratelimit-requests-reset",
                (NOW + 99_000 + elapsed).to_string(),
            )]),
            None,
        )
        .unwrap();
        assert_eq!(
            gate.decision(10).unwrap().next_reevaluation_at,
            Some(NOW + 1600)
        );
    }
    clock.0.store(NOW + 1600, Ordering::SeqCst);
    let reopened = open(&path, &clock, &config);
    assert!(reopened.decision(10).unwrap().evidence.is_empty());
    assert_eq!(
        reopened.status().unwrap().local_window_reset_at,
        NOW + 2000,
        "fixed local boundaries do not slide to operation time"
    );
}

#[trace("FR-032-AC-2", "FR-032-AC-6")]
#[test]
fn precedence_keeps_every_blocker_and_unknown_transition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.sqlite");
    let clock = Arc::new(Clock(AtomicI64::new(NOW)));
    let gate = open(&path, &clock, &QuotaConfig::default());
    request_observation(&gate, 250, NOW + 2000);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE linear_shared_quota SET requests=4500,complexity=1350000,evidence_error_until=-1",
        [],
    )
    .unwrap();
    gate.observe_headers(
        429,
        &headers(&[
            ("retry-after", "3".to_owned()),
            ("x-ratelimit-complexity-remaining", "100".to_owned()),
            ("x-ratelimit-complexity-reset", (NOW + 4000).to_string()),
            ("x-ratelimit-endpoint-requests-remaining", "0".to_owned()),
            (
                "x-ratelimit-endpoint-requests-reset",
                (NOW + 1000).to_string(),
            ),
        ]),
        None,
    )
    .unwrap();
    let decision = gate.decision(10001).unwrap();
    assert_eq!(decision.state, QuotaState::ServerBackoff);
    for state in [
        QuotaState::EvidenceUnavailable,
        QuotaState::RequestRejected,
        QuotaState::ServerReserveHeld,
        QuotaState::LocalWindowHeld,
        QuotaState::ServerBackoff,
    ] {
        assert!(
            decision.blockers.iter().any(|b| b.state == state),
            "missing {state:?}"
        );
    }
    for dimension in [
        QuotaDimension::Requests,
        QuotaDimension::Complexity,
        QuotaDimension::EndpointRequests,
    ] {
        assert!(
            decision
                .blockers
                .iter()
                .any(|b| b.dimension == Some(dimension)),
            "missing {dimension:?}"
        );
    }
    assert_eq!(decision.next_reevaluation_at, Some(NOW + 1000));
    assert_eq!(decision.to_string(), "Linear rate limited; retry in 3s");
    assert!(gate.reserve_evaluated(10, None).is_err());
    clock.0.store(NOW + 5000, Ordering::SeqCst);
    assert_eq!(
        gate.decision(10001).unwrap().state,
        QuotaState::EvidenceUnavailable
    );
    db.execute("UPDATE linear_shared_quota SET evidence_error_until=0", [])
        .unwrap();
    assert_eq!(
        gate.decision(10001).unwrap().state,
        QuotaState::RequestRejected
    );
    db.execute("UPDATE linear_shared_quota SET requests=0,complexity=0", [])
        .unwrap();
    assert_eq!(gate.decision(10).unwrap().state, QuotaState::Available);
}

#[derive(Clone, Debug)]
struct Observation {
    complexity: bool,
    left: i128,
    charged: i128,
    expiry: i64,
}
#[derive(Debug)]
struct Oracle {
    now: i64,
    reset: i64,
    requests: u64,
    complexity: u64,
    constraints: Vec<Observation>,
    pending: Vec<u64>,
}
impl Oracle {
    fn advance(&mut self, time: i64) {
        self.now = self.now.max(time);
        while self.reset <= self.now {
            self.reset += 1000;
            self.requests = 0;
            self.complexity = 0;
        }
        self.constraints
            .retain(|observation| observation.expiry > self.now);
    }
    fn available(&self, cost: u64) -> bool {
        self.requests < 4500
            && self.complexity + cost <= 45000
            && self.constraints.iter().all(|o| {
                o.left - o.charged >= i128::from(if o.complexity { 5000 + cost } else { 501 })
            })
    }
    fn observe(&mut self, complexity: bool, left: u64, expiry: i64) {
        if expiry <= self.now {
            return;
        }
        let charged = if complexity {
            self.pending.iter().sum::<u64>()
        } else {
            u64::try_from(self.pending.len()).unwrap()
        };
        self.constraints.push(Observation {
            complexity,
            left: i128::from(left),
            charged: i128::from(charged),
            expiry,
        });
    }
}

#[trace(
    "FR-032-AC-3",
    "FR-032-AC-4",
    "FR-032-AC-5",
    "FR-032-AC-6",
    "FR-032-AC-10"
)]
#[test]
fn generated_interleavings_match_independent_uncompacted_accounting_oracle() {
    // The oracle stores every observation and no production state/decision helpers.
    // Seeds and the complete event prefix are attached to every admission assertion.
    for seed in [1_u64, 7, 19, 42, 337, 0xdead_beef] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.sqlite");
        let clock = Arc::new(Clock(AtomicI64::new(NOW)));
        let config = QuotaConfig {
            window_seconds: 1,
            ..QuotaConfig::default()
        };
        let mut gate = open(&path, &clock, &config);
        gate.observe_headers(
            200,
            &headers(&[
                ("x-ratelimit-requests-limit", "10000".to_owned()),
                ("x-ratelimit-complexity-limit", "100000".to_owned()),
            ]),
            None,
        )
        .unwrap();
        let mut model = Oracle {
            now: NOW,
            reset: NOW + 1000,
            requests: 0,
            complexity: 0,
            constraints: vec![],
            pending: vec![],
        };
        let mut tokens: Vec<(Reservation, i64)> = vec![];
        let mut rng = seed;
        let mut sequence = Vec::new();
        let mut windows = 0;
        for step in 0..240 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let event = (rng >> 32) % 7;
            match event {
                0 => {
                    let cost = 10 + (rng % 4) * 100;
                    sequence.push(format!("reserve({cost})"));
                    let allowed = model.available(cost);
                    let result = gate.reserve_evaluated(cost, None);
                    assert_eq!(
                        result.is_ok(),
                        allowed,
                        "seed={seed}, step={step}, events={sequence:?}, model={model:?}, result={result:?}"
                    );
                    if let Ok(token) = result {
                        model.requests += 1;
                        model.complexity += cost;
                        for o in &mut model.constraints {
                            o.charged += i128::from(if o.complexity { cost } else { 1 });
                        }
                        model.pending.push(cost);
                        tokens.push((token, model.reset));
                    }
                }
                1 | 2 => {
                    let complexity = event == 2;
                    let left = if complexity {
                        if rng & 1 == 0 { 4900 } else { 20000 }
                    } else if rng & 1 == 0 {
                        250
                    } else {
                        1000
                    };
                    let expiry = model.now + i64::try_from((rng % 5) * 100).unwrap();
                    sequence.push(format!("observe({complexity},{left},{expiry})"));
                    let prefix = if complexity {
                        "x-ratelimit-complexity"
                    } else {
                        "x-ratelimit-requests"
                    };
                    let name_remaining = format!("{prefix}-remaining");
                    let name_reset = format!("{prefix}-reset");
                    gate.observe_headers(
                        200,
                        &headers(&[
                            (&name_remaining, left.to_string()),
                            (&name_reset, expiry.to_string()),
                        ]),
                        None,
                    )
                    .unwrap();
                    model.observe(complexity, left, expiry);
                }
                3 if !tokens.is_empty() => {
                    let index =
                        usize::try_from(rng % u64::try_from(tokens.len()).unwrap()).unwrap();
                    let (token, reset) = tokens.remove(index);
                    let estimate = model.pending.remove(index);
                    let actual = if rng & 1 == 0 { 5 } else { estimate + 70 };
                    sequence.push(format!("settle({index},{actual})"));
                    gate.observe(
                        token,
                        200,
                        &headers(&[("x-complexity", actual.to_string())]),
                        None,
                    )
                    .unwrap();
                    if reset == model.reset {
                        model.complexity = model.complexity - estimate + actual;
                    }
                    for o in &mut model.constraints {
                        if o.complexity {
                            o.charged += i128::from(actual) - i128::from(estimate);
                        }
                    }
                }
                4 => {
                    sequence.push("reopen".to_owned());
                    gate = open(&path, &clock, &config);
                }
                5 => {
                    let time = model.now + 175;
                    sequence.push(format!("clock({time})"));
                    clock.0.store(time, Ordering::SeqCst);
                    model.advance(time);
                    windows += 1;
                }
                _ => {
                    let time = model.now - 50;
                    sequence.push(format!("backward({time})"));
                    clock.0.store(time, Ordering::SeqCst);
                    model.advance(time);
                }
            }
            let decision = gate.decision(10).unwrap();
            assert_eq!(
                decision.state == QuotaState::Available,
                model.available(10),
                "seed={seed}, step={step}, events={sequence:?}, model={model:?}, decision={decision:?}"
            );
            let status = gate.status().unwrap();
            assert_eq!(
                (
                    status.requests_used,
                    status.complexity_used,
                    status.local_window_reset_at
                ),
                (model.requests, model.complexity, model.reset),
                "seed={seed}, step={step}, events={sequence:?}"
            );
        }
        assert!(
            windows >= 20,
            "sequence spans at least three local windows and many evidence expiries"
        );
    }
}
