// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Real process boundaries exercise shared admission without contacting Linear.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use ix_board::Error;
use ix_board::quota::{AuthMode, PlanTier, QuotaClock, QuotaConfig, QuotaGate};
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;

const NOW: i64 = 1_800_000_000_000;

struct FixedClock(i64);

impl QuotaClock for FixedClock {
    fn now_epoch_millis(&self) -> i64 {
        self.0
    }
}

fn config() -> QuotaConfig {
    QuotaConfig {
        reserve_fraction: 0.0,
        ..QuotaConfig::default()
    }
}

fn gate(path: &Path, now: i64) -> QuotaGate {
    QuotaGate::open_with_clock(path, &config(), Arc::new(FixedClock(now)))
        .expect("open shared fixture quota")
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    pairs
        .iter()
        .map(|(name, value)| {
            (
                name.parse::<reqwest::header::HeaderName>()
                    .expect("header name"),
                value.parse().expect("header value"),
            )
        })
        .collect()
}

fn child(path: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", "quota_child_process", "--nocapture"])
        .env("IX_BOARD_QUOTA_PROCESS_TEST_PATH", path)
        .env("IX_BOARD_QUOTA_PROCESS_TEST_MODE", mode);
    command
}

/// Trace: FR-031-AC-2, FR-031-AC-4
#[test]
fn quota_child_process() {
    let Some(path) = std::env::var_os("IX_BOARD_QUOTA_PROCESS_TEST_PATH") else {
        return;
    };
    let gate = gate(Path::new(&path), NOW);
    match std::env::var("IX_BOARD_QUOTA_PROCESS_TEST_MODE")
        .expect("child mode")
        .as_str()
    {
        "cooldown" => {
            for _ in 0..5 {
                assert!(matches!(
                    gate.reserve(10, None),
                    Err(Error::LinearRateLimited { .. })
                ));
            }
            assert_eq!(gate.status().expect("status").requests_used, 0);
        }
        "reserve" => {
            for _ in 0..20 {
                match gate.reserve(10, None) {
                    Ok(()) | Err(Error::LinearLocalBudgetExhausted { .. }) => {}
                    Err(error) => panic!("unexpected admission failure: {error}"),
                }
            }
        }
        mode => panic!("unknown child mode: {mode}"),
    }
}

#[trace("FR-031-AC-1", "FR-031-AC-2", "FR-031-AC-3")]
#[test]
fn cooldown_survives_process_boundary_and_expires_without_sleeping() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let initial = gate(&path, NOW);
    initial
        .observe(429, &headers(&[("retry-after", "2.1")]), None, 10)
        .expect("persist cooldown");
    let error = initial
        .reserve(10, None)
        .expect_err("cooldown denies network admission");
    assert_eq!(error.to_string(), "Linear rate limited; retry in 3s");
    let output = child(&path, "cooldown")
        .output()
        .expect("launch separate process");
    assert!(
        output.status.success(),
        "child: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(initial.status().expect("status").requests_used, 0);
    gate(&path, NOW + 2100)
        .reserve(10, None)
        .expect("expired cooldown permits one attempt");
    assert_eq!(initial.status().expect("status").requests_used, 1);
}

#[trace("FR-031-AC-4", "FR-031-AC-7")]
#[test]
fn competing_processes_never_multiply_request_or_complexity_allowance() {
    for (requests, complexity) in [("10", "20000"), ("2000", "100")] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("quota.sqlite");
        let shared = gate(&path, NOW);
        shared
            .observe(
                200,
                &headers(&[
                    ("x-ratelimit-requests-limit", requests),
                    ("x-ratelimit-complexity-limit", complexity),
                ]),
                None,
                0,
            )
            .expect("actual limits");
        let first = child(&path, "reserve").spawn().expect("first child");
        let second = child(&path, "reserve").spawn().expect("second child");
        assert!(
            first
                .wait_with_output()
                .expect("first outcome")
                .status
                .success()
        );
        assert!(
            second
                .wait_with_output()
                .expect("second outcome")
                .status
                .success()
        );
        let state = shared.status().expect("status");
        assert_eq!(state.requests_used, 5);
        assert_eq!(state.complexity_used, 50);
        assert!(state.requests_used <= state.request_budget);
        assert!(state.complexity_used <= state.complexity_budget);
        assert_eq!(
            shared
                .reserve(10, None)
                .expect_err("allocation consumed")
                .to_string(),
            "Linear local budget exhausted; retry in 3600s"
        );
    }
}

#[trace("FR-031-AC-1")]
#[test]
fn observations_keep_latest_reset_and_missing_metadata_defaults_to_sixty_seconds() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    let graphql =
        serde_json::json!({"errors":[{"extensions":{"code":"RATELIMITED","retryAfter":90}}]});
    let reset = (NOW + 120_000).to_string();
    shared
        .observe(
            400,
            &headers(&[
                ("x-ratelimit-requests-remaining", "0"),
                ("x-ratelimit-requests-reset", &reset),
            ]),
            Some(&graphql),
            10,
        )
        .expect("structured rate limit");
    shared
        .observe(429, &headers(&[("retry-after", "2")]), None, 10)
        .expect("shorter observation");
    assert_eq!(
        shared.status().expect("status").rate_limit_until,
        Some(NOW + 120_000)
    );
    let other = gate(&directory.path().join("fallback.sqlite"), NOW);
    other
        .observe(429, &HeaderMap::new(), None, 10)
        .expect("non-JSON classification needs no body");
    assert_eq!(
        other.status().expect("status").rate_limit_until,
        Some(NOW + 60_000)
    );
}

#[trace("FR-031-AC-4", "FR-031-AC-7")]
#[test]
fn fallback_tiers_strict_shares_unknown_cost_and_single_query_ceiling() {
    for (tier, multiplier) in [
        (PlanTier::Free, 1),
        (PlanTier::Basic, 1),
        (PlanTier::Business, 2),
        (PlanTier::Enterprise, 5),
    ] {
        for (auth_mode, requests, complexity) in [
            (AuthMode::ApiKey, 2500, 3_000_000),
            (AuthMode::OAuth, 5000, 2_000_000),
        ] {
            let config = QuotaConfig {
                tier,
                auth_mode,
                ..config()
            };
            assert_eq!(
                config.fallback_limits(),
                (requests * multiplier, complexity * multiplier)
            );
        }
    }
    for share in [0.0, -0.1, 0.50001, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            QuotaConfig { share, ..config() }.validate(),
            Err(Error::InvalidQuotaConfig { .. })
        ));
    }
    for share in [0.1, 0.5] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let quota = QuotaConfig { share, ..config() };
        let shared = QuotaGate::open_with_clock(
            &directory.path().join("quota.sqlite"),
            &quota,
            Arc::new(FixedClock(NOW)),
        )
        .expect("valid quota");
        let state = shared.status().expect("status");
        assert_eq!(state.request_budget, if share == 0.1 { 250 } else { 1250 });
        assert_eq!(
            state.complexity_budget,
            if share == 0.1 { 300_000 } else { 1_500_000 }
        );
        shared
            .reserve(0, None)
            .expect("unknown complexity charged conservatively");
        assert_eq!(shared.status().expect("status").complexity_used, 10_000);
        assert!(matches!(
            shared.reserve(10_001, None),
            Err(Error::LinearQueryComplexityExceeded {
                cost: 10_001,
                limit: 10_000
            })
        ));
        assert_eq!(shared.status().expect("status").requests_used, 1);
    }
}

#[trace("FR-031-AC-4", "FR-031-AC-5", "FR-031-AC-7")]
#[test]
fn actual_cost_and_remaining_headers_preserve_reserve_and_failed_attempt_charges() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let config = QuotaConfig::default();
    let shared =
        QuotaGate::open_with_clock(&path, &config, Arc::new(FixedClock(NOW))).expect("open quota");
    shared.reserve(10, None).expect("first attempt");
    shared
        .observe(
            500,
            &headers(&[
                ("x-complexity", "20"),
                ("x-ratelimit-requests-limit", "100"),
                ("x-ratelimit-requests-remaining", "5"),
            ]),
            None,
            10,
        )
        .expect("failed response still consumes actual cost");
    let state = shared.status().expect("status");
    assert_eq!(state.requests_used, 1);
    assert_eq!(state.complexity_used, 20);
    assert_eq!(state.request_budget, 45);
    assert!(matches!(
        shared.reserve(10, None),
        Err(Error::LinearLocalBudgetExhausted { .. })
    ));
    assert_eq!(shared.status().expect("status").requests_used, 1);

    let endpoint = gate(&directory.path().join("endpoint.sqlite"), NOW);
    endpoint
        .observe(
            200,
            &headers(&[("x-ratelimit-endpoint-requests-remaining", "0")]),
            None,
            10,
        )
        .expect("endpoint exhaustion observed on success");
    assert_eq!(
        endpoint
            .reserve(10, None)
            .expect_err("endpoint is exhausted")
            .to_string(),
        "Linear local budget exhausted; retry in 60s"
    );
}
