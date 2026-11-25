// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use super::*;
use crate::fleet::capacity::{
    Availability, CapacityError, LaneOutputMeasurement as SourceLaneOutput, Observation,
    WindowRequest,
};
use rusqlite::Connection;
use std::collections::BTreeMap;

const START: i64 = 1_000_000;

fn path() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleet.sqlite3");
    (dir, path)
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
            memory_reserve_bytes: Some(1024),
            thermal_limit_celsius: Some(85.0),
        },
    );
    config
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

#[test]
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
                    "INSERT INTO saturation_sample(host,at,deficit) VALUES('draco',?1,0)",
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
            deficit: Some(2),
        }],
    )
    .unwrap();
    assert!(
        metrics(&path)
            .unwrap()
            .contains("ix_fleet_ready_buffer_deficit{host=\"draco\"} 2")
    );
    let json = serde_json::to_value(history).unwrap();
    assert_eq!(
        json["steps"][0]["source_basis"]["memory_headroom_min"]["availability"],
        "available"
    );
}

#[test]
fn unavailable_saturation_deficit_is_not_exported_as_zero() {
    let (_dir, path) = path();
    let config = capacity_config();
    record_saturation_samples(
        &path,
        &config,
        START,
        &[SaturationSampleInput {
            host: "draco",
            deficit: None,
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
        for statement in crate::fleet::ledger::schema::CAPACITY_VIEWS {
            let view = statement.split_whitespace().nth(2).unwrap();
            connection
                .execute_batch(&format!("DROP VIEW {view}"))
                .unwrap();
        }
        connection
            .execute_batch(
                "ALTER TABLE capacity_step DROP COLUMN build_slots_busy;
             ALTER TABLE capacity_step DROP COLUMN build_slots_sustainable;
             ALTER TABLE capacity_step DROP COLUMN measurement_values;
             ALTER TABLE action DROP COLUMN project;
             ALTER TABLE action DROP COLUMN requested_count;
             DROP TABLE capacity_throttle_sample;
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

#[test]
fn marking_a_lane_draining_writes_step_down_ceiling_atomically() {
    let (_dir, path) = path();
    let config = capacity_config();
    mark_draining(
        &path,
        &config,
        "draco",
        "draco:workspace:a",
        "memory_below_reserve",
        START,
    )
    .unwrap();
    let status = status(&path).unwrap();
    assert_eq!(status.events.len(), 2);
    assert_eq!(status.events[0].kind, "step_down");
    assert_eq!(status.events[1].kind, "draining_mark");
    assert_eq!(status.events[0].ceiling_until, Some(START + 21_600_000));
    assert_eq!(
        status.events[1].reason.as_deref(),
        Some("memory_below_reserve")
    );
}
