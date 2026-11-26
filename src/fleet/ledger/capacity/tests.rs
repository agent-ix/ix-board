// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use super::*;
use crate::fleet::capacity::{
    Availability, CapacityError, LaneOutputMeasurement as SourceLaneOutput, Observation,
    WindowRequest,
};
use crate::fleet::ledger::query as query_ledger;
use ix_trace_rs::trace;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const START: i64 = 1_000_000;

fn path() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleet.sqlite3");
    (dir, path)
}

#[trace("FR-041-AC-16", "FR-041-AC-17")]
#[test]
fn schema_two_ledger_supports_read_only_queries_without_capacity_tables() {
    let (_dir, path) = path();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "PRAGMA user_version=2;
             CREATE TABLE run(sequence INTEGER PRIMARY KEY, relay_ms INTEGER NOT NULL);
             CREATE VIEW idle_minutes AS SELECT 1 AS at_ms, 0 AS minutes;",
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        query_ledger(&path, "idle_minutes", None)
            .unwrap()
            .rows
            .len(),
        1
    );
    assert!(status(&path).unwrap().hosts.is_empty());
    assert!(
        metrics_at(&path, START)
            .unwrap()
            .contains("# TYPE ix_fleet_capacity_recorded_level gauge")
    );
}

fn value<T>(value: T, source: &str, at_ms: i64) -> Observation<T> {
    Observation::available(value, source, at_ms)
}

fn capacity_config() -> FleetConfig {
    let mut config = FleetConfig {
        hosts: vec!["draco".to_owned()],
        ..FleetConfig::default()
    };
    config.capacity_limits.insert(
        "draco".to_owned(),
        crate::fleet::config::HostCapacityLimits {
            build_slots: Some(2),
            lane_capacity: None,
            memory_reserve_bytes: Some(1024),
            thermal_limit_celsius: Some(85.0),
        },
    );
    config
}

#[trace("FR-041-AC-17")]
#[test]
fn unrecorded_hosts_with_demand_excludes_uncovered_and_recorded_hosts() {
    let (_dir, path) = path();
    let ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
    ledger
        .connection
        .execute_batch(
            "INSERT INTO saturation_sample(
                 host,at,deficit,ready_buffer,active_lanes,supplied_lanes,
                 unsupplied_lanes,ready_share,registry_current,bootstrap_ticket_pool
             ) VALUES
               ('demanding',100,2,2,2,0,2,2,1,0),
               ('no-demand',100,0,0,2,0,2,0,1,0),
               ('recorded',100,3,2,2,0,2,2,1,0);
             INSERT INTO capacity_level(host,level,recorded_at)
               VALUES('recorded',1,50);",
        )
        .unwrap();

    let observed = ledger
        .connection
        .prepare("SELECT host,deficit FROM unrecorded_hosts_with_demand ORDER BY host")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(observed, vec![("demanding".to_owned(), 2)]);
}

fn sample(request: &WindowRequest) -> WindowMeasurements {
    let at = request.end_ms;
    let output_basis = value("tickets_prs".to_owned(), "ix-board.capacity", at);
    WindowMeasurements {
        lanes: value(request.lanes, "ledger.capacity_window", at),
        build_slots_busy: value(2, "prometheus.ix_build_slots_busy", at),
        build_slots_sustainable: value(
            2,
            "ix_board_config:fleet.capacity_limits.draco.build_slots",
            at,
        ),
        window_start_ms: request.start_ms,
        window_end_ms: request.end_ms,
        rampup_end_ms: request.rampup_end_ms,
        tickets_closed: value(3, "linear.closed_tickets", at),
        prs_merged: value(2, "github.merged_pull_requests", at),
        commits_pushed: value(1, "git.push_events", at),
        host_output_per_hour: value(30.0, "ix-board.capacity.output", at),
        output_basis,
        queue_wait_p90_s: value(4.0, "prometheus:ix_build_queue_wait_seconds", at),
        throttled_share: Observation::available(0.0, "admission_only", at),
        memory_headroom_min: value(0.3, "node_exporter:node_memory_MemAvailable_bytes", at),
        thermal_headroom_min: value(8.0, "node_thermal_temperature_celsius", at),
        slots_busy_equals_sustainable: value(true, "ix-board.capacity.slot_comparison", at),
        derived_sources: BTreeMap::from([
            (
                "throttled_share_basis".to_owned(),
                "admission_only".to_owned(),
            ),
            (
                "memory_reserve".to_owned(),
                "ix_board_config:fleet.capacity_limits.draco.memory_reserve".to_owned(),
            ),
            (
                "thermal_limit".to_owned(),
                "ix_board_config:fleet.capacity_limits.draco.thermal_limit".to_owned(),
            ),
        ]),
        lane_output: vec![SourceLaneOutput {
            lane: "draco:w:1".to_owned(),
            tickets_closed: value(2, "linear.closed_tickets", at),
            prs_merged: value(1, "github.merged_pull_requests", at),
            commits_pushed: value(1, "git.push_events", at),
        }],
    }
}

struct Source;

impl MeasurementSource for Source {
    fn measure(&self, request: &WindowRequest) -> Result<WindowMeasurements, CapacityError> {
        Ok(sample(request))
    }
}

struct BrokenSource;

impl MeasurementSource for BrokenSource {
    fn measure(&self, _: &WindowRequest) -> Result<WindowMeasurements, CapacityError> {
        Err(CapacityError::Source {
            source_name: "node_exporter".to_owned(),
            field: "node_memory_MemAvailable_bytes".to_owned(),
            reason: "scrape timeout".to_owned(),
        })
    }
}

struct UnavailableSlotsSource;

impl MeasurementSource for UnavailableSlotsSource {
    fn measure(&self, request: &WindowRequest) -> Result<WindowMeasurements, CapacityError> {
        let mut measurement = sample(request);
        measurement.build_slots_busy =
            Observation::unavailable("ix_build_slots_busy", "series_missing", None);
        measurement.build_slots_sustainable =
            Observation::unavailable("ix_build_slots_configured", "series_missing", None);
        measurement.slots_busy_equals_sustainable = Observation::unavailable(
            "ix-board.derived:build_slots_busy==build_slots_sustainable",
            "slot_values_unavailable",
            None,
        );
        Ok(measurement)
    }
}

#[test]
#[trace("FR-041-AC-11")]
fn run_window_and_level_round_trip_with_each_field_basis() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    assert_eq!(window.step_no, 1);
    assert_eq!(status(&path).unwrap().open_windows, vec![window.clone()]);

    {
        let ledger = Ledger::open(&path, &config).unwrap();
        for tick in 1..=12 {
            let at = rampup_end + i64::from(tick) * 300_000;
            ledger
                .connection
                .execute(
                    "INSERT INTO saturation_sample(host,at,deficit,source_basis,coverage,ready_buffer,active_lanes,supplied_lanes,unsupplied_lanes,ready_share,registry_current)
                     VALUES('draco',?1,0,'test',1.0,2,2,0,2,2,1)",
                    [at],
                )
                .unwrap();
        }
    }
    measure_open_step(&path, &config, run_id, rampup_end + 3_600_000, &Source).unwrap();
    assert_eq!(
        end(
            &path,
            &config,
            "draco",
            "build_slots_saturated",
            rampup_end + 3_601_000
        )
        .unwrap(),
        Some(2)
    );

    let history = status(&path).unwrap();
    assert!(history.open_windows.is_empty());
    assert_eq!(history.hosts[0].discovery_state, "ended");
    assert_eq!(history.hosts[0].recorded_level.as_ref().unwrap().level, 2);
    assert_eq!(history.steps[0].host_output_per_hour, Some(30.0));
    assert_eq!(
        history.steps[0].source_basis["build_slots_sustainable"].source,
        "ix_board_config:fleet.capacity_limits.draco.build_slots"
    );
    assert_eq!(
        history.steps[0].source_basis["throttled_share"].source,
        "admission_only"
    );
    assert_eq!(history.steps[0].lane_output[0].tickets_closed, Some(2));
    assert_eq!(
        history.steps[0].lane_output[0].source_basis["commits_pushed"].source,
        "git.push_events"
    );
    assert_eq!(history.runs[0].found_level, Some(2));
    let output_share = query_ledger(&path, "host_output_share_of_recorded_level", None).unwrap();
    assert!(output_share.rows[0]["source_basis"].is_object());
    assert!(output_share.rows[0].get("unavailable_reason").is_some());
    let exposition = metrics(&path).unwrap();
    assert!(exposition.contains("ix_fleet_capacity_recorded_level{host=\"draco\"} 2"));
    assert!(
        exposition.contains("ix_fleet_capacity_discovery_state{host=\"draco\",state=\"ended\"} 1")
    );
    record_saturation_samples(
        &path,
        &config,
        rampup_end + 3_602_000,
        &[SaturationSampleInput {
            host: "draco",
            active_lanes: 2,
            supplied_lanes: Some(0),
            unsupplied_lanes: Some(2),
            registry_current: true,
            ready_buffer: Some(2),
            ready_share: Some(2),
            queued_behind_first: Some(0),
            bootstrap_ticket_pool: Some(0),
            deficit: Some(2),
            source_basis: "{}".to_owned(),
            reason: None,
            coverage: Some(1.0),
        }],
    )
    .unwrap();
    assert!(
        metrics_at(&path, rampup_end + 3_602_000)
            .unwrap()
            .contains("ix_fleet_ready_buffer_deficit{host=\"draco\"} 2")
    );
    let json = serde_json::to_value(history).unwrap();
    assert_eq!(
        json["steps"][0]["source_basis"]["memory_headroom_min"]["availability"],
        "available"
    );
}

#[trace("FR-041-AC-12", "FR-041-AC-13")]
#[test]
fn unavailable_slots_do_not_invalidate_a_demand_covered_window_or_allow_slot_end() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    {
        let ledger = Ledger::open(&path, &config).unwrap();
        for tick in 1..=12 {
            let at = rampup_end + i64::from(tick) * 300_000;
            ledger
                .connection
                .execute(
                    "INSERT INTO saturation_sample(host,at,deficit,source_basis,coverage,ready_buffer,active_lanes,supplied_lanes,unsupplied_lanes,ready_share,registry_current)
                     VALUES('draco',?1,0,'test',1.0,2,2,0,2,2,1)",
                    [at],
                )
                .unwrap();
        }
    }
    measure_open_step(
        &path,
        &config,
        run_id,
        rampup_end + 3_600_000,
        &UnavailableSlotsSource,
    )
    .unwrap();
    let history = status(&path).unwrap();
    assert_eq!(history.steps[0].valid, Some(true));
    assert_eq!(history.steps[0].build_slots_busy, None);
    let error = end(
        &path,
        &config,
        "draco",
        "build_slots_saturated",
        rampup_end + 3_601_000,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("busy slots to equal sustainable slots"),
        "{error}"
    );
}

#[test]
#[trace("FR-041-AC-11")]
fn saturation_coverage_controls_window_validity_when_metrics_are_unavailable() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    for tick in 1..=12 {
        let at = rampup_end + i64::from(tick) * 300_000;
        record_saturation_samples(
            &path,
            &config,
            at,
            &[SaturationSampleInput {
                host: "draco",
                active_lanes: 2,
                supplied_lanes: Some(0),
                unsupplied_lanes: Some(2),
                registry_current: true,
                ready_buffer: Some(2),
                ready_share: Some(2),
                queued_behind_first: Some(0),
                bootstrap_ticket_pool: Some(0),
                deficit: Some(0),
                source_basis: "{}".to_owned(),
                reason: None,
                coverage: Some(1.0),
            }],
        )
        .unwrap();
    }
    let mut measurement = sample(&WindowRequest {
        host: window.host,
        lanes: window.lanes,
        start_ms: rampup_end,
        end_ms: rampup_end + 3_600_000,
        rampup_end_ms: rampup_end,
    });
    measurement.build_slots_busy =
        Observation::unavailable("ix_build_slots_busy", "series_missing", None);
    measurement.build_slots_sustainable =
        Observation::unavailable("config", "not_configured", None);
    measurement.host_output_per_hour = Observation::unavailable("output", "source_missing", None);
    measurement.output_basis = Observation::unavailable("output", "source_missing", None);
    measurement.queue_wait_p90_s =
        Observation::unavailable("ix_build_queue_wait_seconds_bucket", "series_missing", None);
    measurement.throttled_share =
        Observation::unavailable("admission_only", "series_missing", None);
    measurement.memory_headroom_min = Observation::unavailable("memory", "series_missing", None);
    measurement.thermal_headroom_min = Observation::unavailable("thermal", "series_missing", None);
    measurement.slots_busy_equals_sustainable =
        Observation::unavailable("slots", "source_missing", None);
    record_window(&path, &config, run_id, 1, None, &measurement).unwrap();
    let connection = Connection::open(&path).unwrap();
    let (valid, basis): (i64, String) = connection
        .query_row(
            "SELECT valid,source_basis FROM capacity_step WHERE run_id=?1 AND step_no=1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(valid, 1);
    let basis: serde_json::Value = serde_json::from_str(&basis).unwrap();
    assert_eq!(basis["knee_signal"]["availability"], "unavailable");
    assert_eq!(basis["knee_signal"]["reason"], "knee_inputs_unavailable");
}

#[test]
#[trace("FR-041-AC-11")]
fn demand_coverage_validates_positive_deficit_and_names_short_demand() {
    let run_case = |ready_buffer: u32| {
        let (_dir, path) = path();
        let config = capacity_config();
        let run_id = start(&path, &config, "draco", "first", START).unwrap();
        let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
        let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
        for tick in 1..=12 {
            record_saturation_samples(
                &path,
                &config,
                rampup_end + i64::from(tick) * 300_000,
                &[SaturationSampleInput {
                    host: "draco",
                    active_lanes: 2,
                    supplied_lanes: Some(0),
                    unsupplied_lanes: Some(2),
                    registry_current: true,
                    ready_buffer: Some(ready_buffer),
                    ready_share: Some(ready_buffer),
                    queued_behind_first: Some(0),
                    bootstrap_ticket_pool: Some(0),
                    deficit: Some(4),
                    source_basis: "{}".to_owned(),
                    reason: None,
                    coverage: Some(1.0),
                }],
            )
            .unwrap();
        }
        let request = WindowRequest {
            host: window.host,
            lanes: window.lanes,
            start_ms: rampup_end,
            end_ms: rampup_end + 3_600_000,
            rampup_end_ms: rampup_end,
        };
        record_window(&path, &config, run_id, 1, None, &sample(&request)).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .query_row(
                "SELECT valid,inconclusive_reason,deficit_coverage
                 FROM capacity_step WHERE run_id=?1 AND step_no=1",
                [run_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, f64>(2)?,
                    ))
                },
            )
            .unwrap()
    };
    assert_eq!(run_case(2), (1, None, 11.0 / 12.0));
    assert_eq!(run_case(1), (0, Some("demand_short".to_owned()), 0.0));
}

#[test]
#[trace("FR-041-AC-11")]
fn demand_coverage_does_not_depend_on_deficit_availability() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    for tick in 1..=12 {
        record_saturation_samples(
            &path,
            &config,
            rampup_end + i64::from(tick) * 300_000,
            &[SaturationSampleInput {
                host: "draco",
                active_lanes: 2,
                supplied_lanes: Some(0),
                unsupplied_lanes: Some(2),
                registry_current: true,
                ready_buffer: Some(2),
                ready_share: Some(2),
                queued_behind_first: Some(0),
                bootstrap_ticket_pool: Some(0),
                deficit: None,
                source_basis: "{}".to_owned(),
                reason: Some("deficit_unavailable"),
                coverage: Some(1.0),
            }],
        )
        .unwrap();
    }
    let request = WindowRequest {
        host: window.host,
        lanes: window.lanes,
        start_ms: rampup_end,
        end_ms: rampup_end + 3_600_000,
        rampup_end_ms: rampup_end,
    };
    record_window(&path, &config, run_id, 1, None, &sample(&request)).unwrap();
    let (valid, basis): (i64, String) = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT valid,source_basis FROM capacity_step WHERE run_id=?1 AND step_no=1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(valid, 1);
    let basis: serde_json::Value = serde_json::from_str(&basis).unwrap();
    assert_eq!(
        basis["deficit_zero_throughout"]["availability"],
        "unavailable"
    );
    assert_eq!(basis["demand_coverage"]["availability"], "available");
}

#[test]
#[trace("FR-041-AC-18")]
fn judged_without_demand_view_reports_each_invalid_transition_once() {
    let (_dir, path) = path();
    Ledger::open(&path, &capacity_config()).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "INSERT INTO capacity_run(run_id,host,trigger,started_at)
               VALUES(1,'draco','first',1000);
             INSERT INTO capacity_step(run_id,step_no,repeat_of,host,valid,knee_signal,window_end)
               VALUES(1,1,NULL,'draco',0,0,2000),
                     (1,2,NULL,'draco',0,1,3000);
             INSERT INTO capacity_level(host,level,recorded_at,run_id,reason,evidence_steps)
               VALUES('draco',2,5000,1,'step_bound','[2]');
             INSERT INTO capacity_run(run_id,host,trigger,started_at)
               VALUES(2,'draco','first',1000);
             INSERT INTO capacity_step(run_id,step_no,repeat_of,host,valid,knee_signal,window_end)
               VALUES(2,1,NULL,'draco',0,0,2000),
                     (2,1,1,'draco',1,0,3000),
                     (2,2,NULL,'draco',1,0,4000);",
        )
        .unwrap();
    let rows = query_ledger(&path, "discovery_steps_judged_without_demand", None)
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row["step_no"] == 2));
    assert!(
        rows.iter()
            .any(|row| row["violation"] == "new_step_after_invalid")
    );
    assert!(rows.iter().any(|row| row["violation"] == "knee_on_invalid"));
    assert!(
        rows.iter()
            .any(|row| row["violation"] == "level_uses_invalid_step")
    );
    assert!(!rows.iter().any(|row| row["run_id"] == 2));
}

#[test]
#[trace("FR-041-AC-11", "FR-041-AC-17")]
fn discovery_start_view_requires_demand_and_an_available_run_slot() {
    let (_dir, path) = path();
    let config = capacity_config();
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let sampled_at = now - 1_000;
    record_saturation_samples(
        &path,
        &config,
        sampled_at,
        &[SaturationSampleInput {
            host: "draco",
            active_lanes: 0,
            supplied_lanes: Some(0),
            unsupplied_lanes: Some(0),
            registry_current: true,
            ready_buffer: Some(1),
            ready_share: Some(1),
            queued_behind_first: Some(0),
            bootstrap_ticket_pool: Some(1),
            deficit: None,
            source_basis: "{}".to_owned(),
            reason: None,
            coverage: Some(1.0),
        }],
    )
    .unwrap();
    assert_eq!(
        query_ledger(&path, "unrecorded_hosts_ready_for_discovery", None)
            .unwrap()
            .rows
            .len(),
        1
    );
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO capacity_run(host,trigger,started_at)
             VALUES('draco','first',?1)",
            [now],
        )
        .unwrap();
    drop(connection);
    assert!(
        query_ledger(&path, "unrecorded_hosts_ready_for_discovery", None)
            .unwrap()
            .rows
            .is_empty()
    );
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE capacity_run SET ended_at=?1,end_reason='inconclusive_no_demand'
             WHERE ended_at IS NULL",
            [now + 1],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO capacity_run(host,trigger,started_at,ended_at,end_reason)
             VALUES('draco','first',?1,?2,'inconclusive_no_demand')",
            rusqlite::params![now, now + 1],
        )
        .unwrap();
    drop(connection);
    assert!(
        query_ledger(&path, "unrecorded_hosts_ready_for_discovery", None)
            .unwrap()
            .rows
            .is_empty(),
        "the cap is evaluated against today's runs"
    );
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE saturation_sample SET at=?1 WHERE host='draco'",
            [sampled_at - 600_001],
        )
        .unwrap();
    drop(connection);
    assert!(
        query_ledger(&path, "unrecorded_hosts_ready_for_discovery", None)
            .unwrap()
            .rows
            .is_empty(),
        "stale samples cannot start a discovery run"
    );
}

#[test]
#[trace("FR-041-AC-11")]
fn discovery_violation_view_keeps_each_sample_and_accounts_for_later_start() {
    let (_dir, path) = path();
    let config = capacity_config();
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let first = now - 25 * 60_000;
    let second = first + 5 * 60_000;
    for at in [first, second] {
        record_saturation_samples(
            &path,
            &config,
            at,
            &[SaturationSampleInput {
                host: "draco",
                active_lanes: 0,
                supplied_lanes: Some(0),
                unsupplied_lanes: Some(0),
                registry_current: true,
                ready_buffer: Some(1),
                ready_share: Some(0),
                queued_behind_first: Some(0),
                bootstrap_ticket_pool: Some(1),
                deficit: None,
                source_basis: "{}".to_owned(),
                reason: None,
                coverage: Some(1.0),
            }],
        )
        .unwrap();
    }
    let rows = query_ledger(&path, "unrecorded_hosts_without_discovery_run", None)
        .unwrap()
        .rows;
    assert_eq!(
        rows.len(),
        2,
        "both mature demand samples remain observable"
    );
    Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO capacity_run(host,trigger,started_at,ended_at,end_reason)
             VALUES('draco','first',?1,?2,'step_bound')",
            rusqlite::params![first + 60_000, first + 120_000],
        )
        .unwrap();
    let rows = query_ledger(&path, "unrecorded_hosts_without_discovery_run", None)
        .unwrap()
        .rows;
    assert_eq!(
        rows.len(),
        1,
        "the later sample still records an unmet start"
    );
    assert_eq!(rows[0]["at"], second);
}

#[test]
#[trace("FR-041-AC-11")]
fn zero_lane_bootstrap_never_covers_a_capacity_step() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    for tick in 1..=12 {
        record_saturation_samples(
            &path,
            &config,
            rampup_end + i64::from(tick) * 300_000,
            &[SaturationSampleInput {
                host: "draco",
                active_lanes: 0,
                supplied_lanes: Some(0),
                unsupplied_lanes: Some(0),
                registry_current: true,
                ready_buffer: Some(1),
                ready_share: Some(0),
                queued_behind_first: Some(0),
                bootstrap_ticket_pool: Some(1),
                deficit: None,
                source_basis: "{}".to_owned(),
                reason: None,
                coverage: Some(1.0),
            }],
        )
        .unwrap();
    }
    let request = WindowRequest {
        host: window.host,
        lanes: window.lanes,
        start_ms: rampup_end,
        end_ms: rampup_end + 3_600_000,
        rampup_end_ms: rampup_end,
    };
    record_window(&path, &config, run_id, 1, None, &sample(&request)).unwrap();
    let (valid, reason, coverage): (i64, Option<String>, f64) = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT valid,inconclusive_reason,deficit_coverage
             FROM capacity_step WHERE run_id=?1 AND step_no=1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(valid, 0);
    assert_eq!(reason.as_deref(), Some("demand_short"));
    assert_eq!(coverage, 0.0);
}

#[test]
#[trace("FR-041-AC-11")]
fn partial_host_coverage_cannot_certify_demand() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    for tick in 1..=12 {
        let partial = tick == 1;
        record_saturation_samples(
            &path,
            &config,
            rampup_end + i64::from(tick) * 300_000,
            &[SaturationSampleInput {
                host: "draco",
                active_lanes: 2,
                supplied_lanes: Some(0),
                unsupplied_lanes: Some(2),
                registry_current: true,
                ready_buffer: Some(2),
                ready_share: Some(2),
                queued_behind_first: Some(0),
                bootstrap_ticket_pool: Some(0),
                deficit: Some(0),
                source_basis: "{}".to_owned(),
                reason: partial.then_some("partial_deficit_from_hosts_with_usable_supply_and_duty"),
                coverage: Some(if partial { 0.5 } else { 1.0 }),
            }],
        )
        .unwrap();
    }
    let request = WindowRequest {
        host: window.host,
        lanes: window.lanes,
        start_ms: rampup_end,
        end_ms: rampup_end + 3_600_000,
        rampup_end_ms: rampup_end,
    };
    record_window(&path, &config, run_id, 1, None, &sample(&request)).unwrap();
    let valid: i64 = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT valid FROM capacity_step WHERE run_id=?1 AND step_no=1",
            [run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(valid, 0);
}

#[test]
#[trace("FR-041-AC-11")]
fn duplicate_and_clumped_saturation_samples_do_not_inflate_window_coverage() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let window = open_step(&path, &config, run_id, 2, None, START, rampup_end).unwrap();
    for sample_no in 0..6 {
        for _duplicate in 0..2 {
            record_saturation_samples(
                &path,
                &config,
                rampup_end + 300_000 + i64::from(sample_no) * 10_000,
                &[SaturationSampleInput {
                    host: "draco",
                    active_lanes: 2,
                    supplied_lanes: Some(0),
                    unsupplied_lanes: Some(2),
                    registry_current: true,
                    ready_buffer: Some(2),
                    ready_share: Some(2),
                    queued_behind_first: Some(0),
                    bootstrap_ticket_pool: Some(0),
                    deficit: Some(0),
                    source_basis: "{}".to_owned(),
                    reason: None,
                    coverage: Some(1.0),
                }],
            )
            .unwrap();
        }
    }
    let request = WindowRequest {
        host: window.host,
        lanes: window.lanes,
        start_ms: rampup_end,
        end_ms: rampup_end + 3_600_000,
        rampup_end_ms: rampup_end,
    };
    record_window(&path, &config, run_id, 1, None, &sample(&request)).unwrap();
    let (valid, reason, coverage): (i64, Option<String>, f64) = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT valid,inconclusive_reason,deficit_coverage
             FROM capacity_step WHERE run_id=?1 AND step_no=1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(valid, 0);
    assert_eq!(reason.as_deref(), Some("deficit_coverage"));
    assert!(coverage < 0.9);
}

#[test]
#[trace("FR-041-AC-11")]
fn version_six_view_refresh_restores_output_share_provenance_columns() {
    let (_dir, path) = path();
    let config = capacity_config();
    let mut connection = Connection::open(&path).unwrap();
    connection.execute_batch(super::super::schema::V1).unwrap();
    let tx = connection.transaction().unwrap();
    super::super::apply_v2(&tx, None).unwrap();
    super::super::apply_v3(&tx, None).unwrap();
    super::super::apply_v4(&tx).unwrap();
    super::super::apply_v5(&tx).unwrap();
    super::super::apply_v6(&tx).unwrap();
    tx.commit().unwrap();
    drop(connection);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP VIEW IF EXISTS host_output_share_of_recorded_level;
             DROP VIEW IF EXISTS unrecorded_hosts_with_demand;
             DROP VIEW IF EXISTS unrecorded_hosts_without_discovery_run;
             DROP VIEW IF EXISTS unrecorded_hosts_ready_for_discovery;
             DROP VIEW IF EXISTS drains_over_cap_without_escalation;
             DROP VIEW IF EXISTS raises_during_cool_off;
             ALTER TABLE host_output_sample DROP COLUMN sample_interval_ms;
             ALTER TABLE saturation_sample DROP COLUMN source_basis;
             ALTER TABLE saturation_sample DROP COLUMN unavailable_reason;
             ALTER TABLE saturation_sample DROP COLUMN coverage;
             ALTER TABLE saturation_sample DROP COLUMN queued_behind_first;
             ALTER TABLE saturation_sample DROP COLUMN bootstrap_ticket_pool;
             ALTER TABLE capacity_event DROP COLUMN drain_cap_ms;
             ALTER TABLE capacity_event DROP COLUMN ceiling_lanes",
        )
        .unwrap();
    drop(Ledger::open(&path, &config).unwrap());
    assert!(status(&path).unwrap().events.is_empty());
    let connection = Connection::open(&path).unwrap();
    let columns = connection
        .prepare("PRAGMA table_info(host_output_share_of_recorded_level)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(columns.iter().any(|column| column == "source_basis"));
    assert!(columns.iter().any(|column| column == "unavailable_reason"));
    assert!(columns.iter().any(|column| column == "sample_interval_ms"));
    let event_columns = connection
        .prepare("PRAGMA table_info(capacity_event)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(event_columns.iter().any(|column| column == "drain_cap_ms"));
    assert!(event_columns.iter().any(|column| column == "ceiling_lanes"));
    let saturation_columns = connection
        .prepare("PRAGMA table_info(saturation_sample)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        saturation_columns
            .iter()
            .any(|column| column == "source_basis")
    );
    assert!(
        saturation_columns
            .iter()
            .any(|column| column == "unavailable_reason")
    );
    assert!(saturation_columns.iter().any(|column| column == "coverage"));
    assert!(
        saturation_columns
            .iter()
            .any(|column| column == "queued_behind_first")
    );
    assert!(
        saturation_columns
            .iter()
            .any(|column| column == "bootstrap_ticket_pool")
    );
}

#[test]
#[trace("FR-041-AC-21")]
fn host_output_share_normalizes_multi_hour_samples() {
    let (_dir, path) = path();
    let config = capacity_config();
    drop(Ledger::open(&path, &config).unwrap());
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "INSERT INTO capacity_step(run_id,step_no,host,valid,output_basis,host_output_per_hour)
               VALUES(1,1,'draco',1,'tickets_prs',3.0),(1,2,'draco',1,'tickets_prs',4.0);
             INSERT INTO capacity_level(host,level,output_basis,recorded_at,run_id,reason,evidence_steps)
               VALUES('draco',2,'tickets_prs',1000,1,'step_bound','[1,2]');
             INSERT INTO host_output_sample(host,at,lanes,tickets_closed,prs_merged,commits_pushed,
               source_basis,sample_interval_ms)
               VALUES('draco',14401000,2,6,2,NULL,
                 '{\"tickets_closed\":{\"availability\":\"available\"},\"prs_merged\":{\"availability\":\"available\"}}',
                 14400000);",
        )
        .unwrap();
    drop(connection);
    let output_share = query_ledger(&path, "host_output_share_of_recorded_level", None).unwrap();
    assert_eq!(output_share.rows.len(), 1);
    assert_eq!(output_share.rows[0]["step_no"], 2);
    assert_eq!(output_share.rows[0]["sample_interval_ms"], 14_400_000);
    assert_eq!(output_share.rows[0]["output_per_hour"], 2.0);
    assert_eq!(output_share.rows[0]["share_percent"], 50.0);
    let exposition = metrics_at(&path, 14_401_000).unwrap();
    assert!(
        exposition.contains("ix_fleet_host_output_share_of_recorded_level{host=\"draco\"} 0.5")
    );
    assert!(exposition.contains(
        "ix_fleet_host_output_share_of_recorded_level_timestamp_seconds{host=\"draco\"} 14401"
    ));
}

#[test]
#[trace("FR-041-AC-4")]
fn unavailable_saturation_deficit_is_not_exported_as_zero() {
    let (_dir, path) = path();
    let config = capacity_config();
    record_saturation_samples(
        &path,
        &config,
        START,
        &[SaturationSampleInput {
            host: "draco",
            active_lanes: 2,
            supplied_lanes: Some(0),
            unsupplied_lanes: Some(2),
            registry_current: true,
            ready_buffer: None,
            ready_share: None,
            queued_behind_first: None,
            bootstrap_ticket_pool: None,
            deficit: None,
            source_basis: "{}".to_owned(),
            reason: Some("supply_unavailable"),
            coverage: Some(0.0),
        }],
    )
    .unwrap();
    assert!(
        !metrics(&path)
            .unwrap()
            .contains("ix_fleet_ready_buffer_deficit{host=\"draco\"}")
    );
}

#[test]
#[trace("FR-041-AC-7")]
fn ready_deficit_export_requires_a_fresh_sample_and_exposes_its_timestamp() {
    let (_dir, path) = path();
    let config = capacity_config();
    let now = 10_000_000_i64;
    record_saturation_samples(
        &path,
        &config,
        now - 601_000,
        &[SaturationSampleInput {
            host: "draco",
            active_lanes: 2,
            supplied_lanes: Some(0),
            unsupplied_lanes: Some(2),
            registry_current: true,
            ready_buffer: Some(3),
            ready_share: Some(2),
            queued_behind_first: Some(0),
            bootstrap_ticket_pool: Some(0),
            deficit: Some(3),
            source_basis: r#"{"deficit":{"source":"fleet.capacity.known_hosts_only"}}"#.to_owned(),
            reason: None,
            coverage: Some(1.0),
        }],
    )
    .unwrap();
    let persisted: (String, Option<String>, Option<f64>) = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT source_basis,unavailable_reason,coverage FROM saturation_sample
             WHERE lower(host)='draco' ORDER BY at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        persisted.0,
        r#"{"deficit":{"source":"fleet.capacity.known_hosts_only"}}"#
    );
    assert_eq!(persisted.1, None);
    assert_eq!(persisted.2, Some(1.0));
    let exposition = metrics_at(&path, now).unwrap();
    assert!(!exposition.contains("ix_fleet_ready_buffer_deficit{host=\"draco\"}"));
    assert!(exposition.contains("ix_fleet_ready_buffer_deficit_timestamp_seconds{host=\"draco\"}"));
    assert!(exposition.contains(
        "ix_fleet_ready_buffer_deficit_unavailable{host=\"draco\",reason=\"sample_stale\"} 1"
    ));
}

#[test]
#[trace("FR-041-AC-16")]
fn throttled_export_requires_twelve_covered_admission_samples() {
    let (_dir, path) = path();
    let config = capacity_config();
    let now = 10_000_000_i64;
    drop(Ledger::open(&path, &config).unwrap());
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO capacity_level(host,level,output_basis,recorded_at,reason,evidence_steps)
             VALUES('draco',2,'tickets_prs',?1,'step_bound','[]')",
            [now - 4_000_000],
        )
        .unwrap();
    for index in 0..12_i64 {
        connection
            .execute(
                "INSERT INTO capacity_throttle_sample(host,at,interval_s,throttled_share,
                   coverage,source_basis) VALUES('draco',?1,300,0.06,0.95,'admission_only')",
                [now - 3_300_000 + index * 300_000],
            )
            .unwrap();
    }
    drop(connection);

    let exposition = metrics_at(&path, now).unwrap();
    assert!(exposition.contains("ix_fleet_capacity_throttled_at_level{host=\"draco\"} 1"));
    assert!(exposition.contains("ix_fleet_capacity_throttled_without_step_down{host=\"draco\"} 1"));

    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE capacity_throttle_sample SET throttled_share=NULL WHERE at=?1",
            [now - 3_300_000],
        )
        .unwrap();
    let exposition = metrics_at(&path, now).unwrap();
    assert!(!exposition.contains("ix_fleet_capacity_throttled_at_level{host=\"draco\"}"));
}

#[trace("FR-041-AC-10")]
#[test]
fn incomplete_observation_is_null_with_its_source_and_reason() {
    let mut missing: Observation<f64> = Observation::unavailable(
        "node_exporter:node_thermal_temperature_celsius",
        "coverage_below_90_percent",
        Some(0.4),
    );
    missing.availability = Availability::Available;
    assert!(measured(&missing).is_none());
    let basis = observation_basis(&missing);
    assert_eq!(basis.availability, Availability::Unavailable);
    assert_eq!(basis.reason.as_deref(), Some("coverage_below_90_percent"));
}

#[trace("FR-039-AC-18")]
#[test]
fn a_failed_measurement_keeps_the_open_hold_for_retry() {
    let (_dir, path) = path();
    let config = capacity_config();
    let run_id = start(&path, &config, "draco", "first", START).unwrap();
    let rampup_end = START + i64::try_from(config.capacity_rampup.as_millis()).unwrap();
    let expected = open_step(&path, &config, run_id, 1, None, START, rampup_end).unwrap();
    let error = measure_open_step(
        &path,
        &config,
        run_id,
        rampup_end + 3_601_000,
        &BrokenSource,
    )
    .unwrap_err();
    assert!(error.to_string().contains("scrape timeout"));
    assert_eq!(status(&path).unwrap().open_windows, vec![expected]);
    assert!(status(&path).unwrap().steps.is_empty());
}

#[trace("FR-041-AC-20")]
#[test]
fn capacity_events_keep_verified_evidence() {
    let (_dir, path) = path();
    let config = capacity_config();
    record_event(
        &path,
        &config,
        &CapacityEventInput {
            host: "draco",
            lane: Some("draco:w:1"),
            kind: "draining_mark",
            reason: Some("thermal_abort"),
            evidence_kind: Some("thermal_abort"),
            cycle_id: Some(42),
            at_ms: START,
            ceiling_until: Some(START + 21_600_000),
        },
    )
    .unwrap();
    let event = &status(&path).unwrap().events[0];
    assert_eq!(event.kind, "draining_mark");
    assert_eq!(event.evidence_kind, None);
    assert_eq!(event.ceiling_until, Some(START + 21_600_000));
    record_event(
        &path,
        &config,
        &CapacityEventInput {
            host: "draco",
            lane: Some("draco:w:1"),
            kind: "draining_mark",
            reason: Some("thermal_abort"),
            evidence_kind: Some("pause"),
            cycle_id: None,
            at_ms: START + 1,
            ceiling_until: None,
        },
    )
    .unwrap();
    assert_eq!(status(&path).unwrap().events[0].evidence_kind, None);
}

#[test]
fn version_three_ledger_migrates_additively_to_resumable_windows() {
    let (_dir, path) = path();
    let config = capacity_config();
    drop(Ledger::open(&path, &config).unwrap());
    {
        let connection = Connection::open(&path).unwrap();
        for view in crate::fleet::ledger::schema::QUERYABLE {
            if *view == "cycle_missed" {
                continue;
            }
            connection
                .execute_batch(&format!("DROP VIEW {view}"))
                .unwrap();
        }
        connection
            .execute_batch(
                "ALTER TABLE action_send DROP COLUMN sender_role;
             DROP INDEX action_send_receipt_unique;
             ALTER TABLE assignment_history DROP COLUMN sender_role;
             ALTER TABLE assignment_history DROP COLUMN sender_addr;
             ALTER TABLE cycle_missed DROP COLUMN at_ms;
             ALTER TABLE session_turn DROP COLUMN transcript_id;
             DROP TABLE lane_lead;
             DROP TABLE failed_probe_tick;
             DROP TABLE relay_order;
             DROP TABLE relay_order_snapshot;
             DROP TABLE host_build_pause_interval;
             ALTER TABLE capacity_step DROP COLUMN build_slots_busy;
             ALTER TABLE capacity_step DROP COLUMN build_slots_sustainable;
             ALTER TABLE capacity_step DROP COLUMN measurement_values;
             ALTER TABLE action DROP COLUMN project;
             ALTER TABLE action DROP COLUMN requested_count;
             DROP TABLE capacity_throttle_sample;
             DROP TABLE session_turn_stream_state;
             DROP TABLE session_turn_session_role;
             ALTER TABLE capacity_event DROP COLUMN source_basis;
             ALTER TABLE host_output_sample DROP COLUMN source_basis;
             CREATE UNIQUE INDEX capacity_step_run_step ON capacity_step(run_id,step_no);
             CREATE UNIQUE INDEX capacity_step_lane_output_run_step_lane
               ON capacity_step_lane_output(run_id,step_no,lane);
             PRAGMA user_version=4;",
            )
            .unwrap();
    }
    drop(Ledger::open(&path, &config).unwrap());
    assert_eq!(
        user_version(&Connection::open(&path).unwrap()).unwrap(),
        VERSION
    );
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='capacity_window'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[trace("FR-041-AC-14")]
#[test]
fn marking_a_lane_draining_requires_live_watcher_evidence() {
    let (_dir, path) = path();
    let config = capacity_config();
    let error = mark_draining(
        &path,
        &config,
        "draco",
        "draco:workspace:a",
        "memory_below_reserve",
        START,
        Duration::from_secs(4 * 60 * 60),
    )
    .unwrap_err();
    assert!(error.to_string().contains("live lane-state evidence"));
    let status = status(&path).unwrap();
    assert!(status.events.is_empty());
}

struct TimestampedLaneState(f64);

impl crate::fleet::live::PromQuery for TimestampedLaneState {
    fn query(&self, _: &str) -> Result<String, crate::fleet::SourceError> {
        unreachable!("lane-state freshness uses a timestamped query")
    }

    fn query_at(&self, expression: &str, _: i64) -> Result<String, crate::fleet::SourceError> {
        assert!(expression.starts_with("max(timestamp(ix_fleet_lane_state_ratio{"));
        Ok(serde_json::json!({
            "data": {"result": [{"value": ["1700000060", self.0.to_string()]}]}
        })
        .to_string())
    }
}

#[trace("FR-041-AC-14")]
#[test]
fn drain_freshness_uses_the_sample_timestamp_not_the_query_timestamp() {
    let at_ms = 1_700_000_060_000;
    let current = TimestampedLaneState(1_700_000_000.0);
    let stale = TimestampedLaneState(1_699_999_999.0);
    assert!(live_lane_current(
        &current,
        "draco",
        "draco:workspace:a",
        at_ms,
        Duration::from_secs(30),
    ));
    assert!(!live_lane_current(
        &stale,
        "draco",
        "draco:workspace:a",
        at_ms,
        Duration::from_secs(30),
    ));
}

#[trace("FR-041-AC-14", "FR-041-AC-20")]
#[test]
fn live_drain_records_missing_evidence_idempotently_and_knee_has_no_ceiling() {
    let (_dir, path) = path();
    let now_ms = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    {
        let config = capacity_config();
        drop(Ledger::open(&path, &config).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO run(kind,relay_ms,tick_interval_s) VALUES('status',{},{});
                 INSERT INTO lane_tick(run_seq,lane,host,class,observed_ms,evidence) VALUES
                   (1,'draco:workspace:a','draco','IDLE',{},'{{}}'),
                   (1,'draco:workspace:b','draco','IDLE',{},'{{}}');",
                now_ms - 10_000,
                300,
                now_ms - 1_000,
                now_ms - 1_000
            ))
            .unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let sample_ts = now_ms / 1000;
    let server = thread::spawn(move || {
        for _ in 0..18 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let size = stream.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..size]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap();
            let body = if request.contains("timestamp") {
                serde_json::json!({"status":"success","data":{"result":[{
                    "value":[sample_ts.to_string(), sample_ts.to_string()]
                }]}})
            } else {
                serde_json::json!({"status":"success","data":{"result":[]}})
            }
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            )
            .unwrap();
        }
    });
    let mut config = capacity_config();
    config.prometheus_url = Some(format!("http://{address}"));
    config.host_timeout = Duration::from_secs(2);
    mark_draining(
        &path,
        &config,
        "draco",
        "draco:workspace:a",
        "memory_below_reserve",
        now_ms,
        Duration::from_secs(4 * 60 * 60),
    )
    .unwrap();
    mark_draining(
        &path,
        &config,
        "draco",
        "draco:workspace:a",
        "memory_below_reserve",
        now_ms + 1,
        Duration::from_secs(4 * 60 * 60),
    )
    .unwrap();
    mark_draining(
        &path,
        &config,
        "draco",
        "draco:workspace:b",
        "knee_step_back",
        now_ms + 2,
        Duration::from_secs(4 * 60 * 60),
    )
    .unwrap();
    server.join().unwrap();

    let events = status(&path).unwrap().events;
    assert_eq!(events.len(), 4);
    assert!(events.iter().all(|event| event.evidence_kind.is_none()));
    let step_down_ceilings = events
        .iter()
        .filter(|event| event.kind == "step_down")
        .map(|event| event.ceiling_lanes)
        .collect::<Vec<_>>();
    assert_eq!(step_down_ceilings, vec![Some(0), Some(1)]);
    let drain_caps = Connection::open(&path)
        .unwrap()
        .prepare("SELECT DISTINCT drain_cap_ms FROM capacity_event")
        .unwrap()
        .query_map([], |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(drain_caps, vec![14_400_000]);
    let overload_drain = events
        .iter()
        .filter(|event| event.reason.as_deref() == Some("memory_below_reserve"))
        .collect::<Vec<_>>();
    assert_eq!(overload_drain.len(), 2);
    assert!(
        overload_drain
            .iter()
            .all(|event| event.ceiling_until == Some(now_ms + 21_600_000))
    );
    let knee_drain = events
        .iter()
        .filter(|event| event.reason.as_deref() == Some("knee_step_back"))
        .collect::<Vec<_>>();
    assert_eq!(knee_drain.len(), 2);
    assert!(knee_drain.iter().all(|event| event.ceiling_until.is_none()));
    let violations =
        query_ledger(&path, "lane_reductions_without_throttle_evidence", None).unwrap();
    assert_eq!(violations.rows.len(), 4);
}

#[trace("FR-041-AC-15")]
#[test]
fn lane_removal_view_requires_a_drain_for_each_removal() {
    let (_dir, path) = path();
    Ledger::open(&path, &capacity_config()).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "INSERT INTO capacity_event(host,lane,kind,at) VALUES
               ('draco','draco:workspace:a','draining_mark',100),
               ('draco','draco:workspace:a','drain_done',150),
               ('draco','draco:workspace:a','lane_removed',200),
               ('draco','draco:workspace:b','draining_mark',100),
               ('draco','draco:workspace:b','lane_removed',150),
               ('draco','draco:workspace:c','draining_mark',100),
               ('draco','draco:workspace:c','drain_done',150),
               ('draco','draco:workspace:c','lane_removed',200),
               ('draco','draco:workspace:c','lane_added',250),
               ('draco','draco:workspace:c','lane_removed',300),
               ('draco','draco:workspace:d','lane_removed',150);",
        )
        .unwrap();
    let view = query_ledger(&path, "lane_removals_before_drain", None).unwrap();
    let lanes = view
        .rows
        .iter()
        .filter_map(|row| row.get("lane").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        lanes,
        vec![
            "draco:workspace:b",
            "draco:workspace:c",
            "draco:workspace:d"
        ]
    );
}

#[trace("FR-041-AC-15")]
#[test]
fn drain_over_cap_uses_the_recorded_drain_cap_not_the_cool_off() {
    let (_dir, path) = path();
    Ledger::open(&path, &capacity_config()).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO run(kind,relay_ms) VALUES('status',?1)",
            [START + 5_000],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO capacity_event(host,lane,kind,at,ceiling_until,drain_cap_ms)
             VALUES('draco','draco:workspace:a','draining_mark',?1,NULL,4000)",
            [START],
        )
        .unwrap();
    let view = query_ledger(&path, "drains_over_cap_without_escalation", None).unwrap();
    assert_eq!(view.rows.len(), 1);
}

#[trace("FR-041-AC-14")]
#[test]
fn cool_off_view_uses_the_captured_lane_ceiling_with_or_without_a_recorded_level() {
    let (_dir, path) = path();
    Ledger::open(&path, &capacity_config()).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "INSERT INTO run(kind,relay_ms,tick_interval_s) VALUES('status',1000000,300);
             INSERT INTO capacity_event(host,lane,kind,at,ceiling_until,ceiling_lanes)
               VALUES('draco','draco:workspace:a','step_down',50000,900000,2);
             INSERT INTO capacity_event(host,lane,kind,at)
               VALUES('draco','draco:workspace:a','draining_mark',45000);
             INSERT INTO capacity_event(host,lane,kind,at)
               VALUES('draco','draco:workspace:d','lane_added',100000);
             INSERT INTO lane_tick(run_seq,lane,host,class,observed_ms,evidence) VALUES
               (1,'draco:workspace:a','draco','IDLE',40000,'{}'),
               (1,'draco:workspace:b','draco','IDLE',40000,'{}'),
               (1,'draco:workspace:c','draco','IDLE',40000,'{}'),
               (1,'draco:workspace:b','draco','IDLE',200000,'{}'),
               (1,'draco:workspace:c','draco','IDLE',200000,'{}'),
               (1,'draco:workspace:d','draco','IDLE',200000,'{}');",
        )
        .unwrap();
    let view = query_ledger(&path, "raises_during_cool_off", None).unwrap();
    let lanes = view
        .rows
        .iter()
        .filter_map(|row| row.get("lane").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(lanes, vec!["draco:workspace:d"]);
    Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM capacity_level", [])
        .unwrap();
    assert_eq!(
        query_ledger(&path, "raises_during_cool_off", None)
            .unwrap()
            .rows
            .len(),
        1
    );
}
