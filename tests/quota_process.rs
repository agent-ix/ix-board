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
        "reserve" | "settle" => {
            let settle =
                std::env::var("IX_BOARD_QUOTA_PROCESS_TEST_MODE").expect("child mode") == "settle";
            for _ in 0..20 {
                match gate.reserve(if settle { 10_000 } else { 10 }, None) {
                    Ok(reservation) => {
                        if settle {
                            gate.observe(
                                reservation,
                                200,
                                &headers(&[("x-complexity", "100")]),
                                None,
                            )
                            .expect("actual completion");
                        }
                    }
                    Err(Error::LinearLocalBudgetExhausted { .. }) => {}
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
        .observe_headers(429, &headers(&[("retry-after", "2.1")]), None)
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
            .observe_headers(
                200,
                &headers(&[
                    ("x-ratelimit-requests-limit", requests),
                    ("x-ratelimit-complexity-limit", complexity),
                ]),
                None,
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
        .observe_headers(
            400,
            &headers(&[
                ("x-ratelimit-requests-remaining", "0"),
                ("x-ratelimit-requests-reset", &reset),
            ]),
            Some(&graphql),
        )
        .expect("structured rate limit");
    shared
        .observe_headers(429, &headers(&[("retry-after", "2")]), None)
        .expect("shorter observation");
    assert_eq!(
        shared.status().expect("status").rate_limit_until,
        Some(NOW + 120_000)
    );
    let other = gate(&directory.path().join("fallback.sqlite"), NOW);
    other
        .observe_headers(429, &HeaderMap::new(), None)
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
    let reservation = shared.reserve(10, None).expect("first attempt");
    shared
        .observe(
            reservation,
            500,
            &headers(&[
                ("x-complexity", "20"),
                ("x-ratelimit-requests-limit", "100"),
                ("x-ratelimit-requests-remaining", "5"),
            ]),
            None,
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
        .observe_headers(
            200,
            &headers(&[("x-ratelimit-endpoint-requests-remaining", "0")]),
            None,
        )
        .expect("endpoint exhaustion observed on success");
    assert_eq!(
        endpoint
            .reserve(10, None)
            .expect_err("endpoint is exhausted")
            .to_string(),
        "Linear local budget exhausted; retry in 60s"
    );
    gate(&directory.path().join("endpoint.sqlite"), NOW + 60_000)
        .reserve(10, None)
        .expect("exhausted endpoint fallback remaining expires with its sixty-second cooldown");
}

#[trace("FR-031-AC-4", "FR-031-AC-7")]
#[test]
fn positive_endpoint_remaining_is_atomically_reserved_across_processes_and_expires() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    let reset = (NOW + 1000).to_string();
    shared
        .observe_headers(
            200,
            &headers(&[
                ("x-ratelimit-endpoint-requests-limit", "100"),
                ("x-ratelimit-endpoint-requests-remaining", "1"),
                ("x-ratelimit-endpoint-requests-reset", &reset),
            ]),
            None,
        )
        .expect("positive endpoint headroom");
    let first = child(&path, "reserve").spawn().expect("first competitor");
    let second = child(&path, "reserve").spawn().expect("second competitor");
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
    assert_eq!(
        shared.status().expect("status").requests_used,
        1,
        "one remaining endpoint request cannot admit two processes"
    );
    assert_eq!(
        shared
            .reserve(10, None)
            .expect_err("endpoint reservation consumed")
            .to_string(),
        "Linear local budget exhausted; retry in 1s"
    );
    gate(&path, NOW + 1000)
        .reserve(10, None)
        .expect("endpoint reset permits another attempt");
    assert_eq!(shared.status().expect("status").requests_used, 2);
}

#[trace("FR-031-AC-4", "FR-031-AC-7")]
#[test]
fn actual_endpoint_limit_applies_share_even_without_remaining_header() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    shared
        .observe_headers(
            200,
            &headers(&[("x-ratelimit-endpoint-requests-limit", "2")]),
            None,
        )
        .expect("actual endpoint capacity");
    shared
        .reserve(10, None)
        .expect("one allocated endpoint request");
    assert!(matches!(
        shared.reserve(10, None),
        Err(Error::LinearLocalBudgetExhausted { .. })
    ));
    assert_eq!(shared.status().expect("status").requests_used, 1);
}

#[trace("FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn concurrent_processes_reconcile_only_their_own_reservations() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    let first = child(&path, "settle")
        .spawn()
        .expect("first settling process");
    let second = child(&path, "settle")
        .spawn()
        .expect("second settling process");
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
    let state = shared.status().expect("settled status");
    assert_eq!(state.requests_used, 40);
    assert_eq!(
        state.complexity_used, 4000,
        "each reservation replaces only its own estimate"
    );
}

#[trace("FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn out_of_order_completion_never_restores_external_server_headroom() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    shared
        .observe_headers(
            200,
            &headers(&[("x-ratelimit-complexity-remaining", "1000000")]),
            None,
        )
        .expect("server headroom");
    let first = shared.reserve(10_000, None).expect("first pending request");
    let second = shared
        .reserve(10_000, None)
        .expect("second pending request");
    shared
        .observe(
            first,
            200,
            &headers(&[
                ("x-complexity", "100"),
                ("x-ratelimit-complexity-remaining", "899800"),
            ]),
            None,
        )
        .expect("completion after external usage");
    shared
        .observe(
            second,
            200,
            &headers(&[
                ("x-complexity", "100"),
                ("x-ratelimit-complexity-remaining", "999800"),
            ]),
            None,
        )
        .expect("older response arrives last");
    let remaining: u64 = rusqlite::Connection::open(&path)
        .expect("database")
        .query_row(
            "SELECT complexity_remaining FROM linear_shared_quota WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .expect("authoritative remaining");
    assert_eq!(
        remaining, 899_800,
        "settlement must not refund authoritative external usage"
    );
    assert_eq!(shared.status().expect("status").complexity_used, 200);
}

#[trace("FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn old_completion_cannot_refund_a_new_local_or_server_window() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let old = gate(&path, NOW);
    let reservation = old.reserve(10_000, None).expect("old generation attempt");
    let current = gate(&path, NOW + 3_600_000);
    let new = current
        .reserve(10_000, None)
        .expect("new generation attempt");
    let state = current.status().expect("new window");
    assert_eq!(state.local_generation, 1);
    current
        .observe(reservation, 500, &headers(&[("x-complexity", "100")]), None)
        .expect("late old completion remains valid");
    assert_eq!(
        current
            .status()
            .expect("unaffected current charge")
            .complexity_used,
        10_000
    );
    current
        .observe(new, 500, &headers(&[("x-complexity", "250")]), None)
        .expect("failed request actual complexity");
    assert_eq!(
        current
            .status()
            .expect("actual current charge")
            .complexity_used,
        250
    );
    let debt: u64 = rusqlite::Connection::open(&path)
        .expect("database")
        .query_row(
            "SELECT server_complexity_debt FROM linear_shared_quota WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .expect("server debt");
    assert_eq!(
        debt, 250,
        "old server generation completion cannot debit or credit new server headroom"
    );
}

#[trace("FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn missing_actual_cost_and_unobserved_process_exit_keep_conservative_charges() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    let reservation = shared.reserve(10_000, None).expect("unknown cost attempt");
    shared
        .observe(reservation, 500, &HeaderMap::new(), None)
        .expect("failure without complexity");
    assert_eq!(
        shared.status().expect("missing cost").complexity_used,
        10_000
    );
    let process = child(&path, "reserve")
        .spawn()
        .expect("unobserved child attempts");
    assert!(
        process
            .wait_with_output()
            .expect("child exit")
            .status
            .success()
    );
    assert_eq!(
        shared
            .status()
            .expect("crash-conservative accounting")
            .complexity_used,
        10_200
    );
    assert_eq!(shared.status().expect("attempt count").requests_used, 21);
}

#[trace("FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn three_hundred_small_actual_costs_do_not_exhaust_estimated_budget_or_move_local_reset() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let shared = gate(&path, NOW);
    for page in 0..300 {
        let reservation = shared
            .reserve(10_000, None)
            .expect("small actual pages must remain admissible");
        let remaining = (3_000_000 - (page + 1) * 100).to_string();
        let moving_reset = (NOW + 3_600_000 + page * 1000).to_string();
        shared
            .observe(
                reservation,
                200,
                &headers(&[
                    ("x-complexity", "100"),
                    ("x-ratelimit-complexity-remaining", &remaining),
                    ("x-ratelimit-complexity-reset", &moving_reset),
                ]),
                None,
            )
            .expect("actual page complexity");
    }
    let state = shared.status().expect("reconciled usage");
    assert_eq!(state.requests_used, 300);
    assert_eq!(state.complexity_used, 30_000);
    assert_eq!(
        state.local_window_reset_at,
        NOW + 3_600_000,
        "rolling server headers must not extend local accounting"
    );
    let next = gate(&path, NOW + 3_600_000);
    let state = next.status().expect("fixed window expires");
    assert_eq!(state.requests_used, 0);
    assert_eq!(state.complexity_used, 0);
    assert_eq!(state.local_generation, 1);
}

#[trace("FR-031-AC-1", "FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn migration_retains_legacy_charges_and_cooldown_without_fabricating_refundable_reservations() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("quota.sqlite");
    let connection = rusqlite::Connection::open(&path).expect("legacy database");
    connection.execute_batch("CREATE TABLE linear_shared_quota (
        singleton INTEGER PRIMARY KEY,cooldown INTEGER NOT NULL DEFAULT 0,
        requests INTEGER NOT NULL DEFAULT 0,complexity INTEGER NOT NULL DEFAULT 0,
        request_reset INTEGER NOT NULL DEFAULT 0,complexity_reset INTEGER NOT NULL DEFAULT 0,
        request_limit INTEGER,complexity_limit INTEGER,request_remaining INTEGER,complexity_remaining INTEGER,
        endpoint_until INTEGER NOT NULL DEFAULT 0,request_budget INTEGER,complexity_budget INTEGER,
        endpoint_limit INTEGER,endpoint_remaining INTEGER,endpoint_reset INTEGER NOT NULL DEFAULT 0,
        endpoint_used INTEGER NOT NULL DEFAULT 0);").expect("legacy schema");
    connection.execute("INSERT INTO linear_shared_quota(singleton,cooldown,requests,complexity,request_reset,complexity_reset) VALUES(1,?1,30,300000,?2,?3)",
        rusqlite::params![NOW+60_000,NOW+3_600_000,NOW+7_200_000]).expect("legacy accounting");
    drop(connection);
    let migrated = gate(&path, NOW);
    let state = migrated.status().expect("migrated state");
    assert_eq!(state.requests_used, 30);
    assert_eq!(state.complexity_used, 300_000);
    assert_eq!(state.rate_limit_until, Some(NOW + 60_000));
    assert_eq!(state.local_window_reset_at, NOW + 7_200_000);
    migrated
        .observe_headers(200, &headers(&[("x-complexity", "1")]), None)
        .expect("metadata cannot refund unattributed legacy costs");
    assert_eq!(
        migrated
            .status()
            .expect("legacy cost remains")
            .complexity_used,
        300_000
    );
    let reopened = gate(&path, NOW + 60_000);
    let reservation = reopened
        .reserve(10_000, None)
        .expect("new attributed reservation after cooldown");
    reopened
        .observe(reservation, 200, &headers(&[("x-complexity", "100")]), None)
        .expect("only new reservation reconciles");
    let state = reopened.status().expect("legacy plus actual new cost");
    assert_eq!(state.requests_used, 31);
    assert_eq!(state.complexity_used, 300_100);
}

#[trace("FR-031-AC-4", "FR-031-AC-5")]
#[test]
fn wrong_database_cannot_settle_another_databases_reservation() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let original = gate(&directory.path().join("original.sqlite"), NOW);
    let different = gate(&directory.path().join("different.sqlite"), NOW);
    let reservation = original
        .reserve(10_000, None)
        .expect("original reservation");
    assert!(matches!(
        different.observe(reservation, 200, &headers(&[("x-complexity", "1")]), None),
        Err(Error::InvalidQuotaReservation { .. })
    ));
    assert_eq!(
        original
            .status()
            .expect("original charge retained")
            .complexity_used,
        10_000
    );
    assert_eq!(
        different
            .status()
            .expect("different database untouched")
            .complexity_used,
        0
    );
}
