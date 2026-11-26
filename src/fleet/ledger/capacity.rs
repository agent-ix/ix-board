// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Capacity-discovery ledger reads and the Prometheus series exported from them (FR-041).

use super::{Ledger, LedgerError, VERSION, user_version};
use crate::fleet::FleetConfig;
use crate::fleet::capacity::{
    CapacityError, FieldBasis, MeasurementSource, WindowMeasurements, WindowRequest,
};
use ix_trace_rs::implements;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

type CapacityLevelEvidence = (
    i64,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

/// One complete read-only capacity history response.
#[derive(Clone, Debug, Default, Serialize)]
pub struct CapacityStatus {
    /// Host state summaries, including hosts observed by fleet ticks.
    pub hosts: Vec<CapacityHost>,
    /// Discovery runs, newest first.
    pub runs: Vec<CapacityRun>,
    /// Measured steps, newest first.
    pub steps: Vec<CapacityStep>,
    /// Recorded sustainable levels, current and superseded.
    pub levels: Vec<CapacityLevel>,
    /// Capacity events, newest first.
    pub events: Vec<CapacityEvent>,
    /// Holds whose ramp-up or measurement window is still in progress.
    pub open_windows: Vec<OpenCapacityWindow>,
}

/// Current discovery state and sustainable level of one host.
#[derive(Clone, Debug, Serialize)]
pub struct CapacityHost {
    /// Host name.
    pub host: String,
    /// `none`, `running`, or `ended`.
    pub discovery_state: String,
    /// Latest open run, when discovering.
    pub current_run: Option<i64>,
    /// Current sustainable level and its measurement basis.
    pub recorded_level: Option<CapacityLevel>,
    /// Lanes currently marked as draining.
    pub draining_lanes: Vec<String>,
}

/// One discovery run.
#[derive(Clone, Debug, Serialize)]
pub struct CapacityRun {
    /// Ledger run id.
    pub run_id: i64,
    /// Host name.
    pub host: String,
    /// Why the discovery started.
    pub trigger: Option<String>,
    /// Start time in relay milliseconds.
    pub started_at: i64,
    /// End time in relay milliseconds.
    pub ended_at: Option<i64>,
    /// Why it ended.
    pub end_reason: Option<String>,
    /// Level found by the run.
    pub found_level: Option<u32>,
}

/// One measured discovery window.
#[derive(Clone, Debug, Serialize)]
pub struct CapacityStep {
    /// Run id.
    pub run_id: i64,
    /// Step number within the run.
    pub step_no: u32,
    /// Previous step repeated after an invalid window.
    pub repeat_of: Option<u32>,
    /// Host name.
    pub host: String,
    /// Concurrent lanes.
    pub lanes: Option<u32>,
    /// Broker build slots occupied during the measurement window.
    pub build_slots_busy: Option<u32>,
    /// Host's sustainable build-slot capacity during the measurement window.
    pub build_slots_sustainable: Option<u32>,
    /// All source-backed measurement values, with unavailable values kept null.
    pub measurement_values: serde_json::Value,
    /// Window start in relay milliseconds.
    pub window_start: Option<i64>,
    /// Window end in relay milliseconds.
    pub window_end: Option<i64>,
    /// End of ramp-up in relay milliseconds.
    pub rampup_end: Option<i64>,
    /// Whether the window has enough zero-deficit coverage.
    pub valid: Option<bool>,
    /// Why the measurement was inconclusive.
    pub inconclusive_reason: Option<String>,
    /// Fraction of the window with usable saturation samples.
    pub deficit_coverage: Option<f64>,
    /// Whether every usable sample had no deficit.
    pub deficit_zero_throughout: Option<bool>,
    /// Host throughput per hour.
    pub host_output_per_hour: Option<f64>,
    /// Closed tickets in the window.
    pub tickets_closed: Option<u64>,
    /// Merged pull requests in the window.
    pub prs_merged: Option<u64>,
    /// Commits pushed in the window.
    pub commits_pushed: Option<u64>,
    /// Basis used for throughput.
    pub output_basis: Option<String>,
    /// 90th percentile build queue wait, seconds.
    pub queue_wait_p90_s: Option<f64>,
    /// Fraction of the window blocked or paused by the scheduler.
    pub throttled_share: Option<f64>,
    /// Minimum memory headroom over the window.
    pub memory_headroom_min: Option<f64>,
    /// Minimum thermal headroom over the window.
    pub thermal_headroom_min: Option<f64>,
    /// Whether busy slots equalled sustainable slots throughout.
    pub slots_busy_equals_sustainable: Option<bool>,
    /// Whether this step signalled a knee.
    pub knee_signal: Option<bool>,
    /// Why this step signalled a knee.
    pub knee_reason: Option<String>,
    /// Source basis per measurement field.
    pub source_basis: BTreeMap<String, FieldBasis>,
    /// Per-lane throughput attribution.
    pub lane_output: Vec<LaneOutputMeasurement>,
}

/// One lane's output in a discovery window.
#[derive(Clone, Debug, Serialize)]
pub struct LaneOutputMeasurement {
    /// Lane address.
    pub lane: String,
    /// Closed tickets.
    pub tickets_closed: Option<u64>,
    /// Merged pull requests.
    pub prs_merged: Option<u64>,
    /// Pushed commits.
    pub commits_pushed: Option<u64>,
    /// Source basis for the three lane output fields.
    pub source_basis: BTreeMap<String, FieldBasis>,
}

/// One current or superseded sustainable-level record.
#[derive(Clone, Debug, Serialize)]
pub struct CapacityLevel {
    /// Host name.
    pub host: String,
    /// Sustainable lane count.
    pub level: u32,
    /// Throughput basis for the evidence step.
    pub output_basis: Option<String>,
    /// Record time in relay milliseconds.
    pub recorded_at: i64,
    /// Discovery run that found the level.
    pub run_id: Option<i64>,
    /// Why the level was selected.
    pub reason: Option<String>,
    /// JSON array of supporting step numbers.
    pub evidence_steps: Option<String>,
    /// Time superseded, when historical.
    pub superseded_at: Option<i64>,
}

/// One capacity event.
#[derive(Clone, Debug, Serialize)]
pub struct CapacityEvent {
    /// Event id.
    pub event_id: i64,
    /// Host name.
    pub host: String,
    /// Lane, when the event is lane-specific.
    pub lane: Option<String>,
    /// Event kind.
    pub kind: String,
    /// Operator or system reason.
    pub reason: Option<String>,
    /// Evidence checked by ix-board.
    pub evidence_kind: Option<String>,
    /// JSON source basis for the event decision.
    pub source_basis: Option<String>,
    /// Orchestrator cycle id.
    pub cycle_id: Option<i64>,
    /// Event time in relay milliseconds.
    pub at: i64,
    /// End of a cool-off ceiling.
    pub ceiling_until: Option<i64>,
    /// Lane count captured at a step-down, if a host lane sample existed.
    pub ceiling_lanes: Option<i64>,
}

/// The currently open hold window for one capacity-discovery step.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OpenCapacityWindow {
    /// Discovery run id.
    pub run_id: i64,
    /// Step number, preserved when repeating an invalid hold.
    pub step_no: u32,
    /// Step being repeated after an invalid window.
    pub repeat_of: Option<u32>,
    /// Host name.
    pub host: String,
    /// Lanes held during the step.
    pub lanes: u32,
    /// When the lane was added, relay milliseconds.
    pub lane_added_at_ms: i64,
    /// Measurement starts after ramp-up, relay milliseconds.
    pub rampup_end_ms: i64,
}

/// Complete set of fields for one observed capacity event.
pub struct CapacityEventInput<'a> {
    /// Host where the event occurred.
    pub host: &'a str,
    /// Lane affected by the event.
    pub lane: Option<&'a str>,
    /// Event kind from the capacity-event contract.
    pub kind: &'a str,
    /// Operator or system reason.
    pub reason: Option<&'a str>,
    /// ix-board verified evidence kind.
    pub evidence_kind: Option<&'a str>,
    /// Orchestrator cycle, when available.
    pub cycle_id: Option<i64>,
    /// Relay-clock event time.
    pub at_ms: i64,
    /// Cool-off ceiling end.
    pub ceiling_until: Option<i64>,
}

/// One host's ready-buffer deficit sample, with `None` retained when supply or duty is missing.
#[derive(Clone, Debug, PartialEq)]
pub struct SaturationSampleInput<'a> {
    /// Host name.
    pub host: &'a str,
    /// Reachable lane count on this host at sample time.
    pub active_lanes: u32,
    /// Reachable lanes with unfinished assigned tickets at sample time.
    pub supplied_lanes: Option<u32>,
    /// Reachable lanes without unfinished assigned tickets at sample time.
    pub unsupplied_lanes: Option<u32>,
    /// Whether the host's registry lease is current at sample time.
    pub registry_current: bool,
    /// Global ready, unassigned ticket count at sample time.
    pub ready_buffer: Option<u32>,
    /// Host share of ready tickets allocated to its unsupplied lanes.
    pub ready_share: Option<u32>,
    /// Distinct unfinished assigned tickets queued after reachable lane heads.
    pub queued_behind_first: Option<u32>,
    /// Ready buffer remaining after unsupplied lanes plus unfinished assigned queue tails.
    pub bootstrap_ticket_pool: Option<u32>,
    /// Positive shortage; zero is present only when all required sources were measured.
    pub deficit: Option<u32>,
    /// JSON source, availability, reason and coverage basis for each measured field.
    pub source_basis: String,
    /// Reason the planner deficit is unavailable or partial.
    pub reason: Option<&'a str>,
    /// Coverage of ready-buffer and assignment evidence, independent of duty and deficit.
    pub coverage: Option<f64>,
}

/// Opens a host discovery run and returns its ledger id.
///
/// # Errors
///
/// A blank host, an unknown trigger, an existing open run or a ledger failure.
#[implements("FR-041-AC-14")]
pub fn start(
    path: &Path,
    config: &FleetConfig,
    host: &str,
    trigger: &str,
    at_ms: i64,
) -> Result<i64, LedgerError> {
    let host = host.trim().to_lowercase();
    if host.is_empty() || !matches!(trigger, "first" | "re_discovery" | "step_down") {
        return Err(LedgerError::Capacity(
            "host must be non-empty and trigger must be first, re_discovery or step_down".into(),
        ));
    }
    if !config
        .hosts
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&host))
        && !config.capacity_limits.contains_key(&host)
    {
        return Err(LedgerError::Capacity(format!(
            "host {host} is not configured"
        )));
    }
    let mut ledger = Ledger::open(path, config)?;
    let tx = ledger.connection.transaction()?;
    let open: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capacity_run WHERE host=?1 AND ended_at IS NULL)",
        [&host],
        |row| row.get(0),
    )?;
    if open {
        return Err(LedgerError::Capacity(format!(
            "host {host} already has an open run"
        )));
    }
    let day_start = at_ms.div_euclid(86_400_000) * 86_400_000;
    let today_runs: i64 = tx.query_row(
        "SELECT COUNT(*) FROM capacity_run WHERE lower(host)=?1 AND started_at>=?2 AND started_at<?3",
        params![host, day_start, day_start.saturating_add(86_400_000)],
        |row| row.get(0),
    )?;
    if today_runs >= 2 {
        return Err(LedgerError::Capacity(format!(
            "host {host} has already started two capacity runs today"
        )));
    }
    let cooling: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capacity_event WHERE lower(host)=?1 AND kind='step_down'
                      AND ceiling_until>?2)",
        params![host, at_ms],
        |row| row.get(0),
    )?;
    if cooling {
        return Err(LedgerError::Capacity(format!(
            "host {host} is in step-down cool-off"
        )));
    }
    let run_id = tx.query_row(
        "INSERT INTO capacity_run(host,trigger,started_at) VALUES(?1,?2,?3)
         RETURNING run_id",
        params![host, trigger, at_ms],
        |row| row.get(0),
    )?;
    tx.commit()?;
    Ok(run_id)
}

/// Appends a capacity event with ix-board's verified evidence basis.
///
/// # Errors
///
/// The host or event fields are invalid or the ledger cannot write the event.
#[implements("FR-041-AC-20")]
pub fn record_event(
    path: &Path,
    config: &FleetConfig,
    event: &CapacityEventInput<'_>,
) -> Result<(), LedgerError> {
    let host = event.host.trim().to_lowercase();
    if host.is_empty()
        || !matches!(
            event.kind,
            "lane_added"
                | "draining_mark"
                | "drain_done"
                | "lane_removed"
                | "lane_lost"
                | "step_down"
                | "overload_event"
        )
    {
        return Err(LedgerError::Capacity(
            "invalid capacity event host or kind".into(),
        ));
    }
    if !config
        .hosts
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&host))
        && !config.capacity_limits.contains_key(&host)
    {
        return Err(LedgerError::Capacity(format!(
            "host {host} is not configured"
        )));
    }
    let prometheus = config
        .prometheus_url
        .as_deref()
        .and_then(|url| crate::fleet::live::PromHttp::new(url, config.host_timeout).ok());
    let live = crate::fleet::capacity::LiveMeasurementSource {
        config,
        prometheus: prometheus
            .as_ref()
            .map(|query| query as &dyn crate::fleet::live::PromQuery),
    };
    let evidence_kind = live.derived_evidence_kind(&host).or_else(|| {
        status(path).ok().and_then(|status| {
            status
                .steps
                .iter()
                .find(|step| step.host.eq_ignore_ascii_case(&host) && step.valid == Some(true))
                .filter(|step| step.knee_signal == Some(true))
                .map(|_| "knee")
        })
    });
    let ledger = Ledger::open(path, config)?;
    ledger.connection.execute(
        "INSERT INTO capacity_event(host,lane,kind,reason,evidence_kind,cycle_id,at,ceiling_until,
           source_basis) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            host,
            event.lane,
            event.kind,
            event.reason,
            evidence_kind,
            event.cycle_id,
            event.at_ms,
            event.ceiling_until,
            serde_json::json!({"source":"fleet.capacity.command","evidence_kind":evidence_kind})
                .to_string()
        ],
    )?;
    Ok(())
}

/// Records a draining mark and its six-hour step-down ceiling in one ledger transaction.
#[implements("FR-041-AC-14")]
#[implements("FR-041-AC-20")]
pub fn mark_draining(
    path: &Path,
    config: &FleetConfig,
    host: &str,
    lane: &str,
    reason: &str,
    at_ms: i64,
    drain_cap: Duration,
) -> Result<(), LedgerError> {
    let host = host.trim().to_lowercase();
    if host.is_empty()
        || lane.trim().is_empty()
        || !lane.trim().to_lowercase().starts_with(&format!("{host}:"))
        || !matches!(
            reason,
            "memory_below_reserve" | "thermal_abort" | "throttle" | "knee_step_back"
        )
        || !config
            .hosts
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&host))
    {
        return Err(LedgerError::Capacity(
            "drain requires a configured host, non-empty lane and supported reason".into(),
        ));
    }
    let prometheus = config
        .prometheus_url
        .as_deref()
        .and_then(|url| crate::fleet::live::PromHttp::new(url, config.host_timeout).ok());
    let query = prometheus
        .as_ref()
        .ok_or_else(|| LedgerError::Capacity("drain needs live lane-state evidence".to_owned()))?;
    if !live_lane_current(query, &host, lane, at_ms, config.push_interval) {
        return Err(LedgerError::Capacity(
            "drain lane is not present in current watcher state".to_owned(),
        ));
    }
    let live = crate::fleet::capacity::LiveMeasurementSource {
        config,
        prometheus: prometheus
            .as_ref()
            .map(|query| query as &dyn crate::fleet::live::PromQuery),
    };
    let evidence = live.derived_evidence_kind(&host).or_else(|| {
        if reason == "knee_step_back" {
            status(path).ok().and_then(|status| {
                status
                    .steps
                    .iter()
                    .any(|step| {
                        step.host.eq_ignore_ascii_case(&host)
                            && step.valid == Some(true)
                            && step.knee_signal == Some(true)
                    })
                    .then_some("knee")
            })
        } else {
            None
        }
    });
    let ceiling_until =
        (reason != "knee_step_back").then_some(at_ms.saturating_add(6 * 60 * 60 * 1000));
    let mut ledger = Ledger::open(path, config)?;
    let tx = ledger.connection.transaction()?;
    let active_mark: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capacity_event d WHERE lower(d.host)=lower(?1)
          AND lower(d.lane)=lower(?2) AND d.kind='draining_mark' AND NOT EXISTS (
            SELECT 1 FROM capacity_event f WHERE lower(f.lane)=lower(d.lane)
              AND f.kind IN ('drain_done','lane_removed','lane_lost') AND f.at>=d.at))",
        params![host, lane],
        |row| row.get(0),
    )?;
    if active_mark {
        return Ok(());
    }
    let ceiling_lanes: Option<i64> = tx.query_row(
        "SELECT CASE WHEN EXISTS (
           SELECT 1 FROM lane_tick_rows t WHERE lower(t.host)=lower(?1) AND t.observed_ms=(
             SELECT MAX(x.observed_ms) FROM lane_tick_rows x
             WHERE lower(x.host)=lower(?1) AND x.observed_ms<=?3))
         THEN (SELECT COUNT(DISTINCT t.lane) FROM lane_tick_rows t
           WHERE lower(t.host)=lower(?1) AND t.observed_ms=(
             SELECT MAX(x.observed_ms) FROM lane_tick_rows x
             WHERE lower(x.host)=lower(?1) AND x.observed_ms<=?3)
             AND lower(t.lane)<>lower(?2)
             AND NOT EXISTS (SELECT 1 FROM capacity_event draining
               WHERE lower(draining.lane)=lower(t.lane) AND draining.kind='draining_mark'
                 AND draining.at<=?3 AND NOT EXISTS (
                   SELECT 1 FROM capacity_event finished WHERE lower(finished.lane)=lower(t.lane)
                     AND finished.kind IN ('drain_done','lane_removed','lane_lost')
                     AND finished.at>=draining.at AND finished.at<=?3)))
         ELSE NULL END",
        params![host, lane, at_ms],
        |row| row.get(0),
    )?;
    for kind in ["draining_mark", "step_down"] {
        tx.execute(
            "INSERT INTO capacity_event(host,lane,kind,reason,evidence_kind,at,ceiling_until,
               source_basis,drain_cap_ms,ceiling_lanes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                host,
                lane,
                kind,
                reason,
                evidence,
                at_ms,
                ceiling_until,
                serde_json::json!({
                    "source":"fleet.capacity.drain",
                    "evidence_kind":evidence,
                    "drain_cap_ms": i64::try_from(drain_cap.as_millis()).unwrap_or(i64::MAX)
                })
                .to_string(),
                i64::try_from(drain_cap.as_millis()).unwrap_or(i64::MAX),
                (kind == "step_down").then_some(ceiling_lanes).flatten()
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[implements("FR-041-AC-14")]
fn live_lane_current(
    query: &dyn crate::fleet::live::PromQuery,
    host: &str,
    lane: &str,
    at_ms: i64,
    push_interval: Duration,
) -> bool {
    let expression = format!(
        "max(timestamp(ix_fleet_lane_state_ratio{{host=\"{}\",lane=\"{}\",job=\"ix-relay-watcher\"}}))",
        host.replace('"', "\\\""),
        lane.replace('"', "\\\"")
    );
    let Ok(body) = query.query_at(&expression, at_ms) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
        return false;
    };
    let Some(samples) = value["data"]["result"].as_array() else {
        return false;
    };
    let max_age = push_interval.as_millis().saturating_mul(2) as f64 / 1000.0;
    samples.iter().any(|sample| {
        sample["value"][1]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|timestamp| {
                let age = at_ms as f64 / 1000.0 - timestamp;
                (0.0..=max_age).contains(&age)
            })
    })
}

/// Persists one saturation run's per-host deficits.
///
/// # Errors
///
/// A host is not configured or the ledger could not write the sample.
#[implements("FR-041-AC-7")]
pub fn record_saturation_samples(
    path: &Path,
    config: &FleetConfig,
    at_ms: i64,
    samples: &[SaturationSampleInput<'_>],
) -> Result<(), LedgerError> {
    let mut ledger = Ledger::open(path, config)?;
    let tx = ledger.connection.transaction()?;
    for sample in samples {
        let host = sample.host.trim().to_lowercase();
        if !config
            .hosts
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&host))
            && !config.capacity_limits.contains_key(&host)
        {
            return Err(LedgerError::Capacity(format!(
                "host {host} is not configured"
            )));
        }
        tx.execute(
            "INSERT INTO saturation_sample(host,at,deficit,source_basis,unavailable_reason,coverage,ready_buffer,active_lanes,supplied_lanes,unsupplied_lanes,ready_share,queued_behind_first,bootstrap_ticket_pool,registry_current)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                host,
                at_ms,
                sample.deficit.map(i64::from),
                sample.source_basis,
                sample.reason,
                sample.coverage,
                sample.ready_buffer.map(i64::from),
                i64::from(sample.active_lanes),
                sample.supplied_lanes.map(i64::from),
                sample.unsupplied_lanes.map(i64::from),
                sample.ready_share.map(i64::from),
                sample.queued_behind_first.map(i64::from),
                sample.bootstrap_ticket_pool.map(i64::from),
                i64::from(sample.registry_current)
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Opens the next held lane-count window for an active run.
///
/// # Errors
///
/// The run is closed or has an active window, the values are invalid, or the ledger fails.
#[implements("FR-041-AC-11")]
pub fn open_step(
    path: &Path,
    config: &FleetConfig,
    run_id: i64,
    lanes: u32,
    repeat_of: Option<u32>,
    lane_added_at_ms: i64,
    rampup_end_ms: i64,
) -> Result<OpenCapacityWindow, LedgerError> {
    let expected_ramp_end = lane_added_at_ms
        .saturating_add(i64::try_from(config.capacity_rampup.as_millis()).unwrap_or(i64::MAX));
    if lanes == 0 || rampup_end_ms != expected_ramp_end {
        return Err(LedgerError::Capacity(
            "lanes must be positive and ramp-up end must match configured capacity_rampup_secs"
                .into(),
        ));
    }
    let mut ledger = Ledger::open(path, config)?;
    let tx = ledger.connection.transaction()?;
    let host: Option<String> = tx
        .query_row(
            "SELECT host FROM capacity_run WHERE run_id=?1 AND ended_at IS NULL",
            [run_id],
            |row| row.get(0),
        )
        .optional()?;
    let host = host.ok_or_else(|| LedgerError::Capacity(format!("run {run_id} is not open")))?;
    let current: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capacity_window WHERE run_id=?1)",
        [run_id],
        |row| row.get(0),
    )?;
    if current {
        return Err(LedgerError::Capacity(format!(
            "run {run_id} already has an open window"
        )));
    }
    let last: Option<i64> = tx.query_row(
        "SELECT MAX(step_no) FROM capacity_step WHERE run_id=?1",
        [run_id],
        |row| row.get(0),
    )?;
    let step_no = if let Some(repeated_step) = repeat_of {
        let previous_valid: Option<i64> = tx
            .query_row(
                "SELECT valid FROM capacity_step WHERE run_id=?1 AND step_no=?2
                 ORDER BY window_end DESC LIMIT 1",
                params![run_id, i64::from(repeated_step)],
                |row| row.get(0),
            )
            .optional()?;
        if last != Some(i64::from(repeated_step)) || previous_valid != Some(0) {
            return Err(LedgerError::Capacity(
                "repeat_of must name the latest invalid step and keeps its step number".into(),
            ));
        }
        i64::from(repeated_step)
    } else {
        if let Some(last) = last {
            let previous_valid: Option<i64> = tx
                .query_row(
                    "SELECT valid FROM capacity_step WHERE run_id=?1 AND step_no=?2
                     ORDER BY window_end DESC LIMIT 1",
                    params![run_id, last],
                    |row| row.get(0),
                )
                .optional()?;
            if previous_valid != Some(1) {
                return Err(LedgerError::Capacity(
                    "the previous step is invalid; repeat its step number before advancing".into(),
                ));
            }
        }
        last.unwrap_or(0).saturating_add(1)
    };
    if step_no > 8 {
        return Err(LedgerError::Capacity(
            "a discovery run is limited to 8 steps".into(),
        ));
    }
    let window = OpenCapacityWindow {
        run_id,
        step_no: u32::try_from(step_no)
            .map_err(|_| LedgerError::Capacity("step number overflow".into()))?,
        repeat_of,
        host: host.clone(),
        lanes,
        lane_added_at_ms,
        rampup_end_ms,
    };
    tx.execute(
        "INSERT INTO capacity_window(run_id,step_no,repeat_of,host,lanes,lane_added_at_ms,
                                     rampup_end_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            run_id,
            step_no,
            repeat_of.map(i64::from),
            host,
            i64::from(lanes),
            lane_added_at_ms,
            rampup_end_ms,
        ],
    )?;
    tx.commit()?;
    Ok(window)
}

/// Measures and commits the active window through an injected source implementation.
///
/// # Errors
///
/// There is no active window, the provider rejects the request, the measurement is malformed,
/// or the ledger cannot commit it.
#[implements("FR-041-AC-11")]
pub fn measure_open_step(
    path: &Path,
    config: &FleetConfig,
    run_id: i64,
    window_end_ms: i64,
    source: &dyn MeasurementSource,
) -> Result<WindowMeasurements, LedgerError> {
    if window_end_ms < 0 {
        return Err(LedgerError::Capacity(
            "window end must be a positive relay timestamp".into(),
        ));
    }
    let window = {
        let Some(connection) = read_connection(path)? else {
            return Err(LedgerError::Capacity(
                "capacity ledger does not exist".into(),
            ));
        };
        connection
            .query_row(
                "SELECT run_id, step_no, repeat_of, host, lanes, lane_added_at_ms, rampup_end_ms
                 FROM capacity_window WHERE run_id=?1",
                [run_id],
                |row| {
                    Ok(OpenCapacityWindow {
                        run_id: row.get(0)?,
                        step_no: u32::try_from(row.get::<_, i64>(1)?).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                1,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })?,
                        repeat_of: row
                            .get::<_, Option<i64>>(2)?
                            .and_then(|n| u32::try_from(n).ok()),
                        host: row.get(3)?,
                        lanes: u32::try_from(row.get::<_, i64>(4)?).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                4,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })?,
                        lane_added_at_ms: row.get(5)?,
                        rampup_end_ms: row.get(6)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| LedgerError::Capacity(format!("run {run_id} has no open window")))?
    };
    let request = WindowRequest {
        host: window.host.clone(),
        lanes: window.lanes,
        start_ms: window.rampup_end_ms,
        end_ms: window_end_ms,
        rampup_end_ms: window.rampup_end_ms,
    };
    let minimum_window_ms =
        i64::try_from(config.capacity_window_min.as_millis()).unwrap_or(i64::MAX);
    if window_end_ms.saturating_sub(request.start_ms) < minimum_window_ms {
        return Err(LedgerError::Capacity(format!(
            "post-ramp-up window must be at least {} seconds",
            config.capacity_window_min.as_secs()
        )));
    }
    let measurement = source
        .measure(&request)
        .map_err(|error: CapacityError| LedgerError::Capacity(error.to_string()))?;
    if measurement.window_start_ms != request.start_ms
        || measurement.window_end_ms != request.end_ms
        || measurement.rampup_end_ms != request.rampup_end_ms
        || measured(&measurement.lanes) != Some(window.lanes)
    {
        return Err(LedgerError::Capacity(
            "measurement window does not match the open hold request".into(),
        ));
    }
    record_window_inner(
        path,
        config,
        run_id,
        window.step_no,
        window.repeat_of,
        &measurement,
        true,
    )?;
    Ok(measurement)
}

/// Ends an active run and records a new sustainable level when the reason has valid evidence.
///
/// # Errors
///
/// The run is closed, its hold is still open, its reason is unknown, or the ledger fails.
#[implements("FR-041-AC-13")]
pub fn end(
    path: &Path,
    config: &FleetConfig,
    host: &str,
    reason: &str,
    at_ms: i64,
) -> Result<Option<u32>, LedgerError> {
    let host = host.trim().to_lowercase();
    if !matches!(
        reason,
        "knee" | "build_slots_saturated" | "step_bound" | "inconclusive_no_demand" | "throttled"
    ) {
        return Err(LedgerError::Capacity(format!(
            "unknown discovery end reason {reason}"
        )));
    }
    let mut ledger = Ledger::open(path, config)?;
    let tx = ledger.connection.transaction()?;
    let run_id: Option<i64> = tx
        .query_row(
            "SELECT run_id FROM capacity_run WHERE host=?1 AND ended_at IS NULL",
            [&host],
            |row| row.get(0),
        )
        .optional()?;
    let run_id =
        run_id.ok_or_else(|| LedgerError::Capacity(format!("host {host} has no open run")))?;
    let has_window: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM capacity_window WHERE run_id=?1)",
        [run_id],
        |row| row.get(0),
    )?;
    if has_window {
        return Err(LedgerError::Capacity(format!(
            "run {run_id} still has an open window"
        )));
    }
    let started_at: i64 = tx.query_row(
        "SELECT started_at FROM capacity_run WHERE run_id=?1",
        [run_id],
        |row| row.get(0),
    )?;
    if at_ms < started_at {
        return Err(LedgerError::Capacity("end time precedes run start".into()));
    }
    let steps: i64 = tx.query_row(
        "SELECT COUNT(DISTINCT step_no) FROM capacity_step WHERE run_id=?1",
        [run_id],
        |row| row.get(0),
    )?;
    if reason == "step_bound" && steps < 8 {
        return Err(LedgerError::Capacity(
            "step_bound requires 8 measured steps".into(),
        ));
    }
    if reason == "inconclusive_no_demand" {
        let inconclusive: i64 = tx.query_row(
            "SELECT COUNT(*) FROM (SELECT valid FROM capacity_step WHERE run_id=?1
             ORDER BY step_no DESC LIMIT 2) WHERE valid=0",
            [run_id],
            |row| row.get(0),
        )?;
        if inconclusive < 2 {
            return Err(LedgerError::Capacity(
                "inconclusive_no_demand requires two consecutive inconclusive steps".into(),
            ));
        }
    }
    let level_row: Option<i64> = if matches!(
        reason,
        "knee" | "build_slots_saturated" | "step_bound"
    ) {
        if reason == "knee" {
            let knee_step: Option<i64> = tx.query_row(
                "SELECT MAX(step_no) FROM capacity_step WHERE run_id=?1 AND valid=1 AND knee_signal=1",
                [run_id],
                |row| row.get(0),
            )?;
            match knee_step {
                Some(step) => tx
                    .query_row(
                        "SELECT lanes FROM capacity_step WHERE run_id=?1 AND valid=1 AND step_no<?2
                     ORDER BY step_no DESC LIMIT 1",
                        params![run_id, step],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .optional()?
                    .flatten(),
                None => None,
            }
        } else {
            tx.query_row(
                "SELECT lanes FROM capacity_step WHERE run_id=?1 AND valid=1
                 ORDER BY step_no DESC LIMIT 1",
                [run_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
        }
    } else {
        None
    };
    let level = level_row.and_then(|lanes| u32::try_from(lanes).ok());
    if matches!(reason, "knee" | "build_slots_saturated") && level.is_none() {
        return Err(LedgerError::Capacity(format!(
            "{reason} needs at least one valid measured step"
        )));
    }
    if reason == "knee" {
        let knee: bool = tx.query_row(
            "SELECT knee_signal FROM capacity_step WHERE run_id=?1 AND valid=1
             ORDER BY step_no DESC LIMIT 1",
            [run_id],
            |row| row.get(0),
        )?;
        if !knee {
            return Err(LedgerError::Capacity(
                "knee end requires a knee signal on the latest valid step".into(),
            ));
        }
    }
    if reason == "build_slots_saturated" {
        let slots_saturated: bool = tx.query_row(
            "SELECT slots_busy_equals_sustainable FROM capacity_step
             WHERE run_id=?1 AND valid=1 ORDER BY step_no DESC LIMIT 1",
            [run_id],
            |row| row.get::<_, Option<i64>>(0).map(|value| value == Some(1)),
        )?;
        if !slots_saturated {
            return Err(LedgerError::Capacity(
                "build_slots_saturated end requires busy slots to equal sustainable slots".into(),
            ));
        }
    }
    tx.execute(
        "UPDATE capacity_run SET ended_at=?1,end_reason=?2,found_level=?3 WHERE run_id=?4",
        params![at_ms, reason, level.map(i64::from), run_id],
    )?;
    if let Some(level) = level {
        let evidence_step: Option<CapacityLevelEvidence> = tx
            .query_row(
                "SELECT step_no,output_basis,lanes,tickets_closed,prs_merged,commits_pushed
                 FROM capacity_step WHERE run_id=?1 AND valid=1 AND lanes=?2
                 ORDER BY step_no DESC,window_end DESC LIMIT 1",
                params![run_id, i64::from(level)],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((step_no, output_basis, lanes, tickets_closed, prs_merged, commits_pushed)) =
            evidence_step
        else {
            return Err(LedgerError::Capacity(
                "recorded level has no valid supporting step".into(),
            ));
        };
        let evidence_steps = serde_json::to_string(&vec![step_no])?;
        tx.execute(
            "UPDATE capacity_level SET superseded_at=?1 WHERE host=?2 AND superseded_at IS NULL",
            params![at_ms, host],
        )?;
        tx.execute(
            "INSERT INTO capacity_level(host,level,output_basis,recorded_at,run_id,reason,evidence_steps)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![host, i64::from(level), output_basis, at_ms, run_id, reason, evidence_steps],
        )?;
        tx.execute(
            "INSERT INTO host_output_sample(host,at,lanes,tickets_closed,prs_merged,commits_pushed,source_basis,sample_interval_ms)
             SELECT ?1,window_end,?2,?3,?4,?5,source_basis,window_end-window_start FROM capacity_step
             WHERE run_id=?6 AND step_no=?7 AND valid=1 ORDER BY window_end DESC LIMIT 1",
            params![
                host,
                lanes,
                tickets_closed,
                prs_merged,
                commits_pushed,
                run_id,
                step_no
            ],
        )?;
    }
    tx.commit()?;
    Ok(level)
}

fn read_connection(path: &Path) -> Result<Option<Connection>, LedgerError> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let found = user_version(&connection)?;
    if found > VERSION {
        return Err(LedgerError::UnsupportedVersion {
            path: path.to_owned(),
            found,
        });
    }
    if found < VERSION {
        let capacity_schema: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='capacity_run')",
            [],
            |row| row.get(0),
        )?;
        if !capacity_schema {
            return Ok(None);
        }
        return Err(LedgerError::UnsupportedVersion {
            path: path.to_owned(),
            found,
        });
    }
    Ok(Some(connection))
}

/// Reads host discovery state and capacity history without locking or mutating the ledger.
///
/// # Errors
///
/// A newer, foreign or unreadable ledger.
#[implements("FR-041-AC-14")]
pub fn status(path: &Path) -> Result<CapacityStatus, LedgerError> {
    let Some(connection) = read_connection(path)? else {
        return Ok(CapacityStatus::default());
    };
    let mut runs = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT run_id, host, trigger, started_at, ended_at, end_reason, found_level
             FROM capacity_run ORDER BY started_at DESC, run_id DESC",
        )?;
        for row in statement.query_map([], |r| {
            Ok(CapacityRun {
                run_id: r.get(0)?,
                host: r.get(1)?,
                trigger: r.get(2)?,
                started_at: r.get(3)?,
                ended_at: r.get(4)?,
                end_reason: r.get(5)?,
                found_level: r
                    .get::<_, Option<i64>>(6)?
                    .and_then(|n| u32::try_from(n).ok()),
            })
        })? {
            runs.push(row?);
        }
    }
    let mut steps = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT run_id, step_no, repeat_of, host, lanes, window_start, window_end, rampup_end,
                    valid, inconclusive_reason, deficit_coverage, deficit_zero_throughout,
                    host_output_per_hour, tickets_closed, prs_merged, commits_pushed, output_basis,
                    queue_wait_p90_s, throttled_share, memory_headroom_min, thermal_headroom_min,
                    slots_busy_equals_sustainable, knee_signal, knee_reason, source_basis,
                    build_slots_busy, build_slots_sustainable, measurement_values
             FROM capacity_step ORDER BY window_end DESC, run_id DESC, step_no DESC",
        )?;
        for row in statement.query_map([], |r| {
            let source_json: String = r.get(24)?;
            let source_basis = serde_json::from_str(&source_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    24,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(CapacityStep {
                run_id: r.get(0)?,
                step_no: r.get::<_, i64>(1).and_then(|n| {
                    u32::try_from(n).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Integer,
                            Box::new(error),
                        )
                    })
                })?,
                repeat_of: r
                    .get::<_, Option<i64>>(2)?
                    .and_then(|n| u32::try_from(n).ok()),
                host: r.get(3)?,
                lanes: r
                    .get::<_, Option<i64>>(4)?
                    .and_then(|n| u32::try_from(n).ok()),
                build_slots_busy: r
                    .get::<_, Option<i64>>(25)?
                    .and_then(|n| u32::try_from(n).ok()),
                build_slots_sustainable: r
                    .get::<_, Option<i64>>(26)?
                    .and_then(|n| u32::try_from(n).ok()),
                measurement_values: serde_json::from_str(&r.get::<_, String>(27)?).map_err(
                    |error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            27,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    },
                )?,
                window_start: r.get(5)?,
                window_end: r.get(6)?,
                rampup_end: r.get(7)?,
                valid: r.get::<_, Option<i64>>(8)?.map(|n| n != 0),
                inconclusive_reason: r.get(9)?,
                deficit_coverage: r.get(10)?,
                deficit_zero_throughout: r.get::<_, Option<i64>>(11)?.map(|n| n != 0),
                host_output_per_hour: r.get(12)?,
                tickets_closed: r
                    .get::<_, Option<i64>>(13)?
                    .and_then(|n| u64::try_from(n).ok()),
                prs_merged: r
                    .get::<_, Option<i64>>(14)?
                    .and_then(|n| u64::try_from(n).ok()),
                commits_pushed: r
                    .get::<_, Option<i64>>(15)?
                    .and_then(|n| u64::try_from(n).ok()),
                output_basis: r.get(16)?,
                queue_wait_p90_s: r.get(17)?,
                throttled_share: r.get(18)?,
                memory_headroom_min: r.get(19)?,
                thermal_headroom_min: r.get(20)?,
                slots_busy_equals_sustainable: r.get::<_, Option<i64>>(21)?.map(|n| n != 0),
                knee_signal: r.get::<_, Option<i64>>(22)?.map(|n| n != 0),
                knee_reason: r.get(23)?,
                source_basis,
                lane_output: Vec::new(),
            })
        })? {
            steps.push(row?);
        }
    }
    let mut levels = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT host, level, output_basis, recorded_at, run_id, reason, evidence_steps,
                    superseded_at
             FROM capacity_level ORDER BY recorded_at DESC, host",
        )?;
        for row in statement.query_map([], |r| {
            Ok(CapacityLevel {
                host: r.get(0)?,
                level: r.get::<_, i64>(1).and_then(|n| {
                    u32::try_from(n).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Integer,
                            Box::new(error),
                        )
                    })
                })?,
                output_basis: r.get(2)?,
                recorded_at: r.get(3)?,
                run_id: r.get(4)?,
                reason: r.get(5)?,
                evidence_steps: r.get(6)?,
                superseded_at: r.get(7)?,
            })
        })? {
            levels.push(row?);
        }
    }
    let mut events = Vec::new();
    {
        let has_ceiling_lanes: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('capacity_event')
             WHERE name='ceiling_lanes')",
            [],
            |row| row.get(0),
        )?;
        let query = if has_ceiling_lanes {
            "SELECT event_id, host, lane, kind, reason, evidence_kind, cycle_id, at,
                    ceiling_until, source_basis,ceiling_lanes
             FROM capacity_event ORDER BY at DESC, event_id DESC"
        } else {
            "SELECT event_id, host, lane, kind, reason, evidence_kind, cycle_id, at,
                    ceiling_until, source_basis,NULL AS ceiling_lanes
             FROM capacity_event ORDER BY at DESC, event_id DESC"
        };
        let mut statement = connection.prepare(query)?;
        for row in statement.query_map([], |r| {
            Ok(CapacityEvent {
                event_id: r.get(0)?,
                host: r.get(1)?,
                lane: r.get(2)?,
                kind: r.get(3)?,
                reason: r.get(4)?,
                evidence_kind: r.get(5)?,
                cycle_id: r.get(6)?,
                at: r.get(7)?,
                ceiling_until: r.get(8)?,
                source_basis: r.get(9)?,
                ceiling_lanes: r.get(10)?,
            })
        })? {
            events.push(row?);
        }
    }
    let mut open_windows = Vec::new();
    {
        let mut statement = connection.prepare(
            "SELECT run_id,step_no,repeat_of,host,lanes,lane_added_at_ms,rampup_end_ms
             FROM capacity_window ORDER BY run_id",
        )?;
        for row in statement.query_map([], |row| {
            Ok(OpenCapacityWindow {
                run_id: row.get(0)?,
                step_no: u32::try_from(row.get::<_, i64>(1)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Integer,
                        Box::new(error),
                    )
                })?,
                repeat_of: row
                    .get::<_, Option<i64>>(2)?
                    .and_then(|n| u32::try_from(n).ok()),
                host: row.get(3)?,
                lanes: u32::try_from(row.get::<_, i64>(4)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Integer,
                        Box::new(error),
                    )
                })?,
                lane_added_at_ms: row.get(5)?,
                rampup_end_ms: row.get(6)?,
            })
        })? {
            open_windows.push(row?);
        }
    }
    let mut hosts = BTreeSet::new();
    for run in &runs {
        hosts.insert(run.host.clone());
    }
    for level in &levels {
        hosts.insert(level.host.clone());
    }
    for event in &events {
        hosts.insert(event.host.clone());
    }
    {
        let mut statement = connection.prepare("SELECT DISTINCT host FROM host_tick")?;
        for row in statement.query_map([], |r| r.get::<_, String>(0))? {
            hosts.insert(row?);
        }
    }
    let mut host_rows = Vec::new();
    for host in hosts {
        let current_run = runs
            .iter()
            .find(|run| run.host.eq_ignore_ascii_case(&host) && run.ended_at.is_none());
        let latest_run = runs.iter().find(|run| run.host.eq_ignore_ascii_case(&host));
        let recorded_level = levels
            .iter()
            .find(|level| level.host.eq_ignore_ascii_case(&host) && level.superseded_at.is_none())
            .cloned();
        let draining_lanes = events
            .iter()
            .filter(|event| {
                event.host.eq_ignore_ascii_case(&host)
                    && event.kind == "draining_mark"
                    && !events.iter().any(|later| {
                        later.lane == event.lane
                            && matches!(
                                later.kind.as_str(),
                                "drain_done" | "lane_removed" | "lane_lost"
                            )
                            && later.at >= event.at
                    })
            })
            .filter_map(|event| event.lane.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        host_rows.push(CapacityHost {
            host,
            discovery_state: if current_run.is_some() {
                "running"
            } else if latest_run.is_some() {
                "ended"
            } else {
                "none"
            }
            .to_owned(),
            current_run: current_run.map(|run| run.run_id),
            recorded_level,
            draining_lanes,
        });
    }
    for step in &mut steps {
        let mut statement = connection.prepare(
            "SELECT lane, tickets_closed, prs_merged, commits_pushed, source_basis
             FROM capacity_step_lane_output WHERE run_id=?1 AND step_no=?2
               AND (window_end=?3 OR (window_end IS NULL AND
                 (SELECT COUNT(*) FROM capacity_step s WHERE s.run_id=?1 AND s.step_no=?2)=1))
             ORDER BY lane",
        )?;
        step.lane_output = statement
            .query_map(
                params![step.run_id, i64::from(step.step_no), step.window_end],
                |row| {
                    let source_json: String = row.get(4)?;
                    let source_basis = serde_json::from_str(&source_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(LaneOutputMeasurement {
                        lane: row.get(0)?,
                        tickets_closed: row
                            .get::<_, Option<i64>>(1)?
                            .and_then(|n| u64::try_from(n).ok()),
                        prs_merged: row
                            .get::<_, Option<i64>>(2)?
                            .and_then(|n| u64::try_from(n).ok()),
                        commits_pushed: row
                            .get::<_, Option<i64>>(3)?
                            .and_then(|n| u64::try_from(n).ok()),
                        source_basis,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok(CapacityStatus {
        hosts: host_rows,
        runs,
        steps,
        levels,
        events,
        open_windows,
    })
}

fn label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn observation_basis<T>(observation: &crate::fleet::capacity::Observation<T>) -> FieldBasis {
    let covered = observation.coverage.is_some_and(|coverage| coverage >= 0.9);
    let available = observation.availability == crate::fleet::capacity::Availability::Available
        && observation.value.is_some()
        && covered;
    FieldBasis {
        availability: if available {
            crate::fleet::capacity::Availability::Available
        } else {
            crate::fleet::capacity::Availability::Unavailable
        },
        source: observation.source.clone(),
        reason: if available {
            None
        } else {
            observation.reason.clone().or_else(|| {
                Some(if !covered {
                    "coverage_below_90_percent".to_owned()
                } else {
                    "source_value_unavailable".to_owned()
                })
            })
        },
        coverage: observation.coverage,
        observed_at_ms: observation.observed_at_ms,
    }
}

fn measured<T: Clone>(observation: &crate::fleet::capacity::Observation<T>) -> Option<T> {
    (observation_basis(observation).availability == crate::fleet::capacity::Availability::Available)
        .then(|| observation.value.clone())
        .flatten()
}

fn slots_equal(measurement: &WindowMeasurements) -> crate::fleet::capacity::Observation<bool> {
    match (
        measured(&measurement.build_slots_busy),
        measured(&measurement.build_slots_sustainable),
    ) {
        (Some(busy), Some(sustainable)) => crate::fleet::capacity::Observation {
            value: Some(busy == sustainable),
            availability: crate::fleet::capacity::Availability::Available,
            source: "ix-board.derived:build_slots_busy==build_slots_sustainable".to_owned(),
            reason: None,
            coverage: Some(
                measurement
                    .build_slots_busy
                    .coverage
                    .unwrap_or(0.0)
                    .min(measurement.build_slots_sustainable.coverage.unwrap_or(0.0)),
            ),
            observed_at_ms: measurement
                .build_slots_busy
                .observed_at_ms
                .zip(measurement.build_slots_sustainable.observed_at_ms)
                .map(|(busy, sustainable)| busy.min(sustainable)),
        },
        _ => crate::fleet::capacity::Observation::unavailable(
            "ix-board.derived:build_slots_busy==build_slots_sustainable",
            "slot_values_unavailable",
            None,
        ),
    }
}

/// Renders the capacity-level and discovery-state Prometheus series from the ledger.
///
/// # Errors
///
/// A newer, foreign or unreadable ledger.
#[implements("FR-041-AC-16")]
pub fn metrics(path: &Path) -> Result<String, LedgerError> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_i64, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        });
    metrics_at(path, now_ms)
}

#[implements("FR-041-AC-16")]
fn metrics_at(path: &Path, now_ms: i64) -> Result<String, LedgerError> {
    let status = status(path)?;
    let mut text = String::new();
    text.push_str("# TYPE ix_fleet_capacity_recorded_level gauge\n");
    text.push_str("# TYPE ix_fleet_capacity_recorded_level_timestamp_seconds gauge\n");
    text.push_str("# TYPE ix_fleet_capacity_discovery_state gauge\n");
    text.push_str("# TYPE ix_fleet_capacity_draining_lanes gauge\n");
    text.push_str("# TYPE ix_fleet_capacity_throttled_at_level gauge\n");
    text.push_str("# TYPE ix_fleet_capacity_throttled_without_step_down gauge\n");
    text.push_str("# TYPE ix_fleet_capacity_overload_events_total counter\n");
    text.push_str("# TYPE ix_fleet_ready_buffer_deficit gauge\n");
    text.push_str("# TYPE ix_fleet_ready_buffer_deficit_timestamp_seconds gauge\n");
    text.push_str("# TYPE ix_fleet_ready_buffer_deficit_unavailable gauge\n");
    text.push_str("# TYPE ix_fleet_host_output_share_of_recorded_level gauge\n");
    text.push_str("# TYPE ix_fleet_host_output_share_of_recorded_level_timestamp_seconds gauge\n");
    let trailing_hour = now_ms.saturating_sub(3_600_000);
    let connection = if path.exists() {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let found = user_version(&connection)?;
        let capacity_schema: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='capacity_run')",
            [],
            |row| row.get(0),
        )?;
        if found > VERSION {
            return Err(LedgerError::UnsupportedVersion {
                path: path.to_owned(),
                found,
            });
        }
        if !capacity_schema {
            None
        } else if found == VERSION {
            Some(connection)
        } else {
            return Err(LedgerError::UnsupportedVersion {
                path: path.to_owned(),
                found,
            });
        }
    } else {
        None
    };
    for host in &status.hosts {
        let escaped = label(&host.host);
        if let Some(level) = &host.recorded_level {
            text.push_str(&format!(
                "ix_fleet_capacity_recorded_level{{host=\"{escaped}\"}} {}\nix_fleet_capacity_recorded_level_timestamp_seconds{{host=\"{escaped}\"}} {}\n",
                level.level,
                level.recorded_at / 1000
            ));
        }
        for state in ["none", "running", "ended"] {
            text.push_str(&format!(
                "ix_fleet_capacity_discovery_state{{host=\"{escaped}\",state=\"{state}\"}} {}\n",
                u8::from(host.discovery_state == state)
            ));
        }
        if !host.draining_lanes.is_empty()
            || status.events.iter().any(|event| {
                event.host.eq_ignore_ascii_case(&host.host) && event.kind == "draining_mark"
            })
        {
            text.push_str(&format!(
                "ix_fleet_capacity_draining_lanes{{host=\"{escaped}\"}} {}\n",
                host.draining_lanes.len()
            ));
        }
        if host.recorded_level.is_some()
            && let Some(connection) = connection.as_ref()
        {
            let mut statement = connection.prepare(
                "SELECT at, interval_s, throttled_share, coverage FROM capacity_throttle_sample
                 WHERE lower(host)=lower(?1) AND at>?2 ORDER BY at DESC LIMIT 12",
            )?;
            let mut samples = statement
                .query_map(params![host.host, trailing_hour], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<f64>>(2)?,
                        row.get::<_, Option<f64>>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            samples.reverse();
            let complete = samples.len() == 12
                && samples.iter().all(|(_, interval, share, coverage)| {
                    *interval == 300 && share.is_some() && coverage.is_some_and(|v| v >= 0.9)
                })
                && samples.windows(2).all(|pair| {
                    let gap = pair[1].0.saturating_sub(pair[0].0);
                    (270_000..=330_000).contains(&gap)
                })
                && samples
                    .first()
                    .is_some_and(|(at, ..)| *at <= trailing_hour.saturating_add(300_000))
                && samples
                    .last()
                    .is_some_and(|(at, ..)| *at >= now_ms.saturating_sub(300_000));
            if complete {
                let throttled = u8::from(
                    samples
                        .iter()
                        .all(|(_, _, share, _)| share.is_some_and(|value| value > 0.05)),
                );
                let stepped_down = status.events.iter().any(|event| {
                    event.host.eq_ignore_ascii_case(&host.host)
                        && event.kind == "step_down"
                        && event.at >= trailing_hour
                });
                text.push_str(&format!(
                    "ix_fleet_capacity_throttled_at_level{{host=\"{escaped}\"}} {throttled}\nix_fleet_capacity_throttled_without_step_down{{host=\"{escaped}\"}} {}\n",
                    u8::from(throttled == 1 && !stepped_down)
                ));
            }
        }
        for cause in [
            "memory_below_reserve",
            "thermal_limit_breach",
            "thermal_abort",
        ] {
            let count = status
                .events
                .iter()
                .filter(|event| {
                    event.host.eq_ignore_ascii_case(&host.host)
                        && event.kind == "overload_event"
                        && event.reason.as_deref() == Some(cause)
                })
                .count();
            if count > 0 {
                text.push_str(&format!(
                    "ix_fleet_capacity_overload_events_total{{host=\"{escaped}\",cause=\"{cause}\"}} {count}\n"
                ));
            }
        }
    }
    if let Some(connection) = connection.as_ref() {
        let mut latest_deficit = BTreeMap::new();
        let mut statement = connection.prepare(
            "WITH ranked AS (
               SELECT host,at,deficit,unavailable_reason,coverage,
                 ROW_NUMBER() OVER(PARTITION BY lower(host) ORDER BY at DESC,id DESC) AS rn
               FROM saturation_sample)
             SELECT host,at,deficit,unavailable_reason,coverage FROM ranked WHERE rn=1",
        )?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<f64>>(4)?,
            ))
        })? {
            let (host, at, deficit, reason, coverage) = row?;
            latest_deficit.insert(host.to_lowercase(), (host, at, deficit, reason, coverage));
        }
        let hosts = status
            .hosts
            .iter()
            .map(|host| host.host.to_lowercase())
            .chain(latest_deficit.keys().cloned())
            .collect::<BTreeSet<_>>();
        for host_key in hosts {
            let Some((host, at, deficit, reason, coverage)) = latest_deficit.get(&host_key) else {
                text.push_str(&format!(
                    "ix_fleet_ready_buffer_deficit_unavailable{{host=\"{}\",reason=\"sample_missing\"}} 1\n",
                    label(&host_key)
                ));
                continue;
            };
            let escaped = label(host);
            text.push_str(&format!(
                "ix_fleet_ready_buffer_deficit_timestamp_seconds{{host=\"{escaped}\"}} {}\n",
                prometheus_seconds(*at)
            ));
            if *at <= now_ms
                && now_ms.saturating_sub(*at) <= 600_000
                && let Some(deficit) = deficit
            {
                text.push_str(&format!(
                    "ix_fleet_ready_buffer_deficit{{host=\"{escaped}\"}} {deficit}\n"
                ));
                continue;
            }
            let unavailable = if *at > now_ms {
                "sample_future"
            } else if now_ms.saturating_sub(*at) > 600_000 {
                "sample_stale"
            } else {
                bounded_deficit_reason(reason.as_deref(), *coverage)
            };
            text.push_str(&format!(
                "ix_fleet_ready_buffer_deficit_unavailable{{host=\"{escaped}\",reason=\"{unavailable}\"}} 1\n"
            ));
        }
    }
    if let Some(connection) = connection.as_ref() {
        let mut statement = connection.prepare(
            "SELECT host,share_percent,at_ms FROM host_output_share_of_recorded_level
             ORDER BY lower(host)",
        )?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })? {
            let (host, share_percent, at_ms) = row?;
            let escaped = label(&host);
            if let Some(at_ms) = at_ms {
                text.push_str(&format!(
                    "ix_fleet_host_output_share_of_recorded_level_timestamp_seconds{{host=\"{escaped}\"}} {}\n",
                    prometheus_seconds(at_ms)
                ));
            }
            if let Some(share_percent) = share_percent {
                text.push_str(&format!(
                    "ix_fleet_host_output_share_of_recorded_level{{host=\"{escaped}\"}} {}\n",
                    share_percent / 100.0
                ));
            }
        }
    }
    Ok(text)
}

#[implements("FR-041-AC-16")]
fn bounded_deficit_reason(reason: Option<&str>, coverage: Option<f64>) -> &'static str {
    match reason {
        Some("partial_deficit_from_hosts_with_usable_supply_and_duty") => "partial_deficit",
        Some(value) if value.contains("coverage") || coverage.is_some_and(|value| value < 0.9) => {
            "coverage_incomplete"
        }
        Some(value) if value.contains("duty") => "duty_unavailable",
        Some(value) if value.contains("supply") || value.contains("slot") => "supply_unavailable",
        Some(value) if value.contains("linear") || value.contains("ticket") => {
            "ready_work_unavailable"
        }
        Some(_) => "source_unavailable",
        None => "source_unavailable",
    }
}

#[implements("FR-041-AC-16")]
fn prometheus_seconds(at_ms: i64) -> String {
    format!("{}.{:03}", at_ms.div_euclid(1000), at_ms.rem_euclid(1000))
}

/// Persists one source-backed hold window and deterministically judges its demand coverage and
/// knee signal. The caller's source plugin supplies every observed field; the ledger supplies
/// saturation coverage and the derived validity/knee fields.
///
/// # Errors
///
/// A malformed window, a closed or missing run, source-basis serialization, or a database error.
#[implements("FR-041-AC-11")]
pub fn record_window(
    path: &Path,
    config: &FleetConfig,
    run_id: i64,
    step_no: u32,
    repeat_of: Option<u32>,
    measurement: &WindowMeasurements,
) -> Result<(), LedgerError> {
    record_window_inner(path, config, run_id, step_no, repeat_of, measurement, false)
}

#[implements("FR-041-AC-11")]
#[implements("FR-041-AC-12")]
fn record_window_inner(
    path: &Path,
    config: &FleetConfig,
    run_id: i64,
    step_no: u32,
    repeat_of: Option<u32>,
    measurement: &WindowMeasurements,
    close_window: bool,
) -> Result<(), LedgerError> {
    if measurement.window_end_ms <= measurement.window_start_ms
        || measurement.rampup_end_ms > measurement.window_start_ms
    {
        return Err(LedgerError::Capacity(
            "window end must follow its start, and ramp-up must finish before measurement".into(),
        ));
    }
    let step_no = i64::from(step_no);
    let repeat_of = repeat_of.map(i64::from);
    let mut ledger = Ledger::open(path, config)?;
    let tx = ledger.connection.transaction()?;
    let host: Option<String> = tx
        .query_row(
            "SELECT host FROM capacity_run WHERE run_id=?1 AND ended_at IS NULL",
            [run_id],
            |row| row.get(0),
        )
        .optional()?;
    let host = host.ok_or_else(|| LedgerError::Capacity(format!("run {run_id} is not open")))?;

    let interval_ms = ledger.tick_interval_s.saturating_mul(1000).max(1);
    let sample_rows = {
        let mut statement = tx.prepare(
            "SELECT at,coverage,ready_buffer,ready_share,supplied_lanes,
                    unsupplied_lanes,registry_current,active_lanes,deficit
             FROM saturation_sample WHERE host=?1 AND at>=?2 AND at<=?3 ORDER BY at",
        )?;
        statement
            .query_map(
                params![host, measurement.window_start_ms, measurement.window_end_ms],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<f64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<i64>>(8)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?
    };
    let samples = i64::try_from(sample_rows.len()).unwrap_or(i64::MAX);
    let mut demand_unavailable = 0i64;
    let mut deficit_unavailable = 0i64;
    let mut positive = 0i64;
    let mut demand_covered = 0i64;
    let mut intervals = Vec::with_capacity(sample_rows.len());
    for (
        at,
        coverage,
        ready_buffer,
        ready_share,
        supplied,
        unsupplied,
        registry,
        active,
        deficit,
    ) in sample_rows
    {
        if coverage.is_none_or(|coverage| coverage < 0.9)
            || ready_buffer.is_none()
            || ready_share.is_none()
            || supplied.is_none()
            || unsupplied.is_none()
            || registry.is_none()
            || active.is_none()
            || (active.is_some_and(|active| active > 0) && measurement.lanes.value.is_none())
        {
            demand_unavailable += 1;
        }
        if deficit.is_none() || coverage.is_none_or(|coverage| coverage < 0.9) {
            deficit_unavailable += 1;
        }
        if deficit.is_some_and(|deficit| deficit > 0) {
            positive += 1;
        }
        let covers = coverage.is_some_and(|coverage| coverage >= 0.9)
            && ready_buffer.is_some()
            && ready_share.is_some()
            && supplied.is_some()
            && unsupplied.is_some()
            && registry == Some(1)
            && match (active, ready_share, supplied, measurement.lanes.value) {
                (Some(active), Some(share), Some(supplied), Some(step_lanes)) if active > 0 => {
                    supplied.saturating_add(share) >= i64::from(step_lanes)
                }
                _ => false,
            };
        if covers {
            demand_covered += 1;
            let interval_start = at.max(measurement.window_start_ms);
            let interval_end = at
                .saturating_add(interval_ms)
                .min(measurement.window_end_ms);
            if interval_end > interval_start {
                intervals.push((interval_start, interval_end));
            }
        }
    }
    intervals.sort_unstable();
    let mut covered_ms = 0i64;
    let mut union_end = measurement.window_start_ms;
    for (start, end) in intervals {
        let start = start.max(union_end);
        if end > start {
            covered_ms = covered_ms.saturating_add(end - start);
            union_end = end;
        }
    }
    let span_ms = measurement
        .window_end_ms
        .saturating_sub(measurement.window_start_ms)
        .max(1);
    let coverage = (covered_ms as f64 / span_ms as f64).min(1.0);
    let zero_throughout = samples > 0 && deficit_unavailable == 0 && positive == 0;
    let demand_short = demand_unavailable == 0 && samples > 0 && demand_covered < samples;
    let valid = coverage >= 0.9 && demand_unavailable == 0 && !demand_short;
    let inconclusive_reason = if demand_unavailable > 0 || samples == 0 {
        Some("deficit_coverage")
    } else if demand_short {
        Some("demand_short")
    } else if coverage < 0.9 {
        Some("deficit_coverage")
    } else {
        None
    };
    let mut sources = BTreeMap::from([
        ("lanes".to_owned(), observation_basis(&measurement.lanes)),
        (
            "build_slots_busy".to_owned(),
            observation_basis(&measurement.build_slots_busy),
        ),
        (
            "build_slots_sustainable".to_owned(),
            observation_basis(&measurement.build_slots_sustainable),
        ),
        (
            "tickets_closed".to_owned(),
            observation_basis(&measurement.tickets_closed),
        ),
        (
            "prs_merged".to_owned(),
            observation_basis(&measurement.prs_merged),
        ),
        (
            "commits_pushed".to_owned(),
            observation_basis(&measurement.commits_pushed),
        ),
        (
            "host_output_per_hour".to_owned(),
            observation_basis(&measurement.host_output_per_hour),
        ),
        (
            "output_basis".to_owned(),
            observation_basis(&measurement.output_basis),
        ),
        (
            "queue_wait_p90_s".to_owned(),
            observation_basis(&measurement.queue_wait_p90_s),
        ),
        (
            "throttled_share".to_owned(),
            observation_basis(&measurement.throttled_share),
        ),
        (
            "memory_headroom_min".to_owned(),
            observation_basis(&measurement.memory_headroom_min),
        ),
        (
            "thermal_headroom_min".to_owned(),
            observation_basis(&measurement.thermal_headroom_min),
        ),
        (
            "slots_busy_equals_sustainable".to_owned(),
            observation_basis(&slots_equal(measurement)),
        ),
        (
            "deficit_coverage".to_owned(),
            FieldBasis {
                availability: if samples > 0 && coverage >= 0.9 {
                    crate::fleet::capacity::Availability::Available
                } else {
                    crate::fleet::capacity::Availability::Unavailable
                },
                source: "ledger.saturation_sample".to_owned(),
                reason: (samples == 0 || coverage < 0.9)
                    .then(|| "coverage_below_90_percent".to_owned()),
                coverage: Some(coverage),
                observed_at_ms: Some(measurement.window_end_ms),
            },
        ),
        (
            "deficit_zero_throughout".to_owned(),
            FieldBasis {
                availability: if samples > 0 && deficit_unavailable == 0 {
                    crate::fleet::capacity::Availability::Available
                } else {
                    crate::fleet::capacity::Availability::Unavailable
                },
                source: "ledger.saturation_sample".to_owned(),
                reason: (samples == 0 || deficit_unavailable > 0)
                    .then(|| "deficit_sample_unavailable".to_owned()),
                coverage: Some(coverage),
                observed_at_ms: Some(measurement.window_end_ms),
            },
        ),
        (
            "demand_coverage".to_owned(),
            FieldBasis {
                availability: if coverage >= 0.9 && !demand_short && demand_unavailable == 0 {
                    crate::fleet::capacity::Availability::Available
                } else {
                    crate::fleet::capacity::Availability::Unavailable
                },
                source: "ledger.saturation_sample.ready_buffer_vs_step_lanes".to_owned(),
                reason: demand_short.then(|| "demand_short".to_owned()).or_else(|| {
                    (coverage < 0.9 || demand_unavailable > 0)
                        .then(|| "demand_coverage_below_90_percent".to_owned())
                }),
                coverage: Some(coverage),
                observed_at_ms: Some(measurement.window_end_ms),
            },
        ),
        (
            "knee_signal".to_owned(),
            FieldBasis {
                availability: if measured(&measurement.throttled_share).is_some()
                    || measured(&measurement.host_output_per_hour).is_some()
                {
                    crate::fleet::capacity::Availability::Available
                } else {
                    crate::fleet::capacity::Availability::Unavailable
                },
                source: "ix-board.FR-041.knee_rule".to_owned(),
                reason: (measured(&measurement.throttled_share).is_none()
                    && measured(&measurement.host_output_per_hour).is_none())
                .then(|| "knee_inputs_unavailable".to_owned()),
                coverage: Some(
                    if measured(&measurement.throttled_share).is_some()
                        || measured(&measurement.host_output_per_hour).is_some()
                    {
                        1.0
                    } else {
                        0.0
                    },
                ),
                observed_at_ms: Some(measurement.window_end_ms),
            },
        ),
    ]);
    for (field, source) in &measurement.derived_sources {
        sources.entry(field.clone()).or_insert_with(|| FieldBasis {
            availability: crate::fleet::capacity::Availability::Available,
            source: source.clone(),
            reason: None,
            coverage: Some(1.0),
            observed_at_ms: Some(measurement.window_end_ms),
        });
    }
    let source_basis = serde_json::to_string(&sources)?;

    let mut knee_reason = None;
    if valid {
        if measured(&measurement.throttled_share).is_some_and(|share| share > 0.05) {
            knee_reason = Some("throttle");
        } else if let Some((previous_lanes, previous_output, previous_basis)) = tx
            .query_row(
                "SELECT lanes, host_output_per_hour, output_basis FROM capacity_step
                 WHERE run_id=?1 AND valid=1 AND host_output_per_hour IS NOT NULL
                   AND step_no < ?2 ORDER BY step_no DESC LIMIT 1",
                params![run_id, step_no],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?
            .and_then(|(lanes, output, basis)| {
                lanes
                    .and_then(|n| u32::try_from(n).ok())
                    .zip(basis)
                    .map(|(n, basis)| (n, output, basis))
            })
        {
            let current_basis = measured(&measurement.output_basis);
            let previous_per_lane = previous_output / f64::from(previous_lanes.max(1));
            if current_basis.as_deref() == Some(previous_basis.as_str())
                && measured(&measurement.host_output_per_hour)
                    .is_some_and(|output| output - previous_output < previous_per_lane * 0.5)
            {
                knee_reason = Some("output_gain");
            }
        }
    }
    let knee_signal = knee_reason.is_some();
    let lanes = measured(&measurement.lanes).map(i64::from);
    let tickets_closed = measured(&measurement.tickets_closed)
        .map(i64::try_from)
        .transpose()
        .map_err(|_| LedgerError::Capacity("tickets_closed does not fit SQLite INTEGER".into()))?;
    let prs_merged = measured(&measurement.prs_merged)
        .map(i64::try_from)
        .transpose()
        .map_err(|_| LedgerError::Capacity("prs_merged does not fit SQLite INTEGER".into()))?;
    let commits_pushed = measured(&measurement.commits_pushed)
        .map(i64::try_from)
        .transpose()
        .map_err(|_| LedgerError::Capacity("commits_pushed does not fit SQLite INTEGER".into()))?;
    let slots_busy_equals_sustainable = measured(&slots_equal(measurement)).map(i64::from);
    let build_slots_busy = measured(&measurement.build_slots_busy).map(i64::from);
    let build_slots_sustainable = measured(&measurement.build_slots_sustainable).map(i64::from);
    let measurement_values = serde_json::to_string(measurement)?;
    tx.execute(
        "INSERT INTO capacity_step(
           run_id,step_no,repeat_of,host,lanes,window_start,window_end,rampup_end,valid,
           inconclusive_reason,deficit_coverage,deficit_zero_throughout,host_output_per_hour,
           tickets_closed,prs_merged,commits_pushed,output_basis,queue_wait_p90_s,throttled_share,
           memory_headroom_min,thermal_headroom_min,slots_busy_equals_sustainable,knee_signal,
           knee_reason,source_basis,build_slots_busy,build_slots_sustainable,measurement_values,
           window_min_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,
                ?22,?23,?24,?25,?26,?27,?28,?29)",
        params![
            run_id,
            step_no,
            repeat_of,
            host,
            lanes,
            measurement.window_start_ms,
            measurement.window_end_ms,
            measurement.rampup_end_ms,
            valid,
            inconclusive_reason,
            coverage,
            zero_throughout,
            measured(&measurement.host_output_per_hour),
            tickets_closed,
            prs_merged,
            commits_pushed,
            measured(&measurement.output_basis),
            measured(&measurement.queue_wait_p90_s),
            measured(&measurement.throttled_share),
            measured(&measurement.memory_headroom_min),
            measured(&measurement.thermal_headroom_min),
            slots_busy_equals_sustainable,
            knee_signal,
            knee_reason,
            source_basis,
            build_slots_busy,
            build_slots_sustainable,
            measurement_values,
            i64::try_from(config.capacity_window_min.as_millis()).unwrap_or(i64::MAX),
        ],
    )?;
    for lane in &measurement.lane_output {
        let lane_sources = serde_json::to_string(&BTreeMap::from([
            (
                "tickets_closed".to_owned(),
                observation_basis(&lane.tickets_closed),
            ),
            ("prs_merged".to_owned(), observation_basis(&lane.prs_merged)),
            (
                "commits_pushed".to_owned(),
                observation_basis(&lane.commits_pushed),
            ),
        ]))?;
        let tickets = measured(&lane.tickets_closed)
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                LedgerError::Capacity("lane tickets_closed does not fit SQLite INTEGER".into())
            })?;
        let prs = measured(&lane.prs_merged)
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                LedgerError::Capacity("lane prs_merged does not fit SQLite INTEGER".into())
            })?;
        let commits = measured(&lane.commits_pushed)
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                LedgerError::Capacity("lane commits_pushed does not fit SQLite INTEGER".into())
            })?;
        tx.execute(
            "INSERT INTO capacity_step_lane_output(
               run_id,step_no,lane,tickets_closed,prs_merged,commits_pushed,source_basis,window_end)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                run_id,
                step_no,
                lane.lane,
                tickets,
                prs,
                commits,
                lane_sources,
                measurement.window_end_ms
            ],
        )?;
    }
    if close_window {
        tx.execute(
            "DELETE FROM capacity_window WHERE run_id=?1 AND step_no=?2",
            params![run_id, step_no],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests;
