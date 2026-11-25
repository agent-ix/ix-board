// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Fleet supply, duty, and ready-buffer saturation (FR-041).

use crate::fleet::live::PromQuery;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// A source value with its availability and basis, retained in the JSON contract.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Evidence<T> {
    /// Value; absent when unavailable.
    pub value: Option<T>,
    /// Stable source name or metric.
    pub source: String,
    /// Why the source value cannot be used.
    pub reason: Option<String>,
    /// Covered fraction of the requested interval.
    pub coverage: Option<f64>,
}

impl<T> Evidence<T> {
    /// Available source value.
    pub fn available(value: T, source: impl Into<String>, coverage: f64) -> Self {
        Self {
            value: Some(value),
            source: source.into(),
            reason: None,
            coverage: Some(coverage),
        }
    }

    /// Unavailable source value; it never substitutes zero.
    pub fn unavailable(
        source: impl Into<String>,
        reason: impl Into<String>,
        coverage: Option<f64>,
    ) -> Self {
        Self {
            value: None,
            source: source.into(),
            reason: Some(reason.into()),
            coverage,
        }
    }
}

/// Source observations for one live host.
#[derive(Clone, Debug, PartialEq)]
pub struct HostInput {
    /// Host name.
    pub host: String,
    /// Interim sustainable build slots from explicit ix-board host config.
    pub sustainable_slots: Evidence<u32>,
    /// Maximum lanes from relay server-capacity.
    pub lane_capacity: Evidence<u32>,
    /// Locked-build occupied slots, if the real producer is present.
    pub build_slots_busy: Evidence<u32>,
    /// Locked-build configured slots, if the real producer is present.
    pub build_slots_configured: Evidence<u32>,
    /// Compiler seconds attributed by relay-watcher over the trailing window.
    pub compiler_seconds: Evidence<f64>,
    /// Reporter-observed lane seconds over the same interval.
    pub observed_lane_seconds: Evidence<f64>,
    /// Reachable active lane count on this host.
    pub active_lanes: u32,
}

/// Saturation values for one host.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HostSaturation {
    /// Host name.
    pub host: String,
    /// Sustainable slots and their source basis.
    pub sustainable_slots: Evidence<u32>,
    /// Lane capacity and its source basis.
    pub lane_capacity: Evidence<u32>,
    /// Broker's occupied build slots.
    pub build_slots_busy: Evidence<u32>,
    /// Broker's configured build slots.
    pub build_slots_configured: Evidence<u32>,
    /// Compiler duty over observed lane time.
    pub duty: Evidence<f64>,
    /// Number of lanes needed to occupy sustainable slots.
    pub lanes_needed: Evidence<u32>,
    /// Active reachable lanes counted from the relay.
    pub active_lanes: u32,
    /// Target ready tickets for this host.
    pub target_ready: Evidence<u32>,
    /// This host's allocated ready tickets.
    pub ready_buffer: Evidence<u32>,
    /// Positive shortage for this host; unavailable inputs remain null.
    pub ready_buffer_deficit: Evidence<u32>,
}

/// Ready queue count and host lane share for one owning project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectBuffer {
    /// Project identifier or exact name.
    pub project: String,
    /// Owning planner session address, when configured.
    pub planner: Option<String>,
    /// Ready, unassigned tickets.
    pub ready: u32,
    /// Reachable lanes attributed to this project.
    pub lanes: u32,
    /// Additional tickets this planner should supply.
    pub need: u32,
}

/// JSON contract for `ix-board fleet saturation --json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SaturationReport {
    /// JSON schema version.
    pub schema_version: u32,
    /// Whether every value required for the deficit is available.
    pub complete: bool,
    /// Relay time in Unix milliseconds.
    pub observed_at_ms: i64,
    /// Requested trailing interval in seconds.
    pub window_secs: u64,
    /// Queue depth per lane.
    pub queue_depth: u32,
    /// Host-level supply and deficit values.
    pub hosts: Vec<HostSaturation>,
    /// Project-level ready counts and planner shares.
    pub projects: Vec<ProjectBuffer>,
    /// Total ready tickets.
    pub ready_buffer: Evidence<u32>,
    /// Total target buffer.
    pub target_ready: Evidence<u32>,
    /// Total shortage; null if required inputs are unavailable.
    pub ready_buffer_deficit: Evidence<u32>,
}

/// Input for deterministic saturation evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct SaturationInput {
    /// Observation time from relay.
    pub observed_at_ms: i64,
    /// Trailing-window length.
    pub window_secs: u64,
    /// Target tickets held per needed lane.
    pub queue_depth: u32,
    /// One source observation per reachable configured host.
    pub hosts: Vec<HostInput>,
    /// Ready, unassigned ticket counts, keyed by project.
    pub project_ready: Option<BTreeMap<String, u32>>,
    /// Owning planner address by project.
    pub planners: BTreeMap<String, String>,
    /// Reachable lane count by owning project.
    pub project_lanes: BTreeMap<String, u32>,
}

fn value<T: Copy>(evidence: &Evidence<T>) -> Option<T> {
    evidence
        .coverage
        .filter(|coverage| *coverage >= 0.9)
        .and(evidence.value)
}

fn unavailable<T>(source: &str, reason: &str) -> Evidence<T> {
    Evidence::unavailable(source, reason, None)
}

fn query_sum(query: &dyn PromQuery, expression: &str) -> Result<Option<f64>, String> {
    let body = query.query(expression).map_err(|error| error.to_string())?;
    let response: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    if response["status"] != "success" {
        return Err("Prometheus query did not succeed".to_owned());
    }
    let results = response["data"]["result"]
        .as_array()
        .ok_or_else(|| "Prometheus response has no result vector".to_owned())?;
    let mut total = 0.0;
    for result in results {
        let value = result["value"][1]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| "Prometheus value is not finite numeric data".to_owned())?;
        total += value;
    }
    Ok((!results.is_empty()).then_some(total))
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn slot_count(value: f64) -> Option<u32> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= f64::from(u32::MAX))
        .then_some(value as u32)
}

/// Reads watcher compiler duty from its live counters and lane-presence samples.
///
/// The denominator is the count of `ix_fleet_lane_state_ratio` samples times the configured
/// push interval, capped at the requested window for each lane. Every lane must have 90% coverage
/// in both series; otherwise both host aggregates are unavailable.
pub fn live_duty(
    query: &dyn PromQuery,
    host: &str,
    lanes: &[String],
    window_secs: u64,
    push_interval_secs: u64,
) -> (Evidence<f64>, Evidence<f64>) {
    let compile_source = "prometheus.ix_fleet_lane_compile_seconds_total{job=ix-relay-watcher}";
    let observed_source = "prometheus.ix_fleet_lane_state_ratio*push_interval_secs";
    if lanes.is_empty() || window_secs == 0 || push_interval_secs == 0 {
        return (
            unavailable(compile_source, "no_lane_or_invalid_window"),
            unavailable(observed_source, "no_lane_or_invalid_window"),
        );
    }
    let span = format!("{window_secs}s");
    let mut compiler_total = 0.0;
    let mut observed_total = 0.0;
    let mut coverage = 1.0_f64;
    for lane in lanes {
        let host_label = escape_label(host);
        let lane_label = escape_label(lane);
        let compiler_selector = format!(
            "ix_fleet_lane_compile_seconds_total{{host=\"{host_label}\",lane=\"{lane_label}\",job=\"ix-relay-watcher\"}}"
        );
        let state_selector =
            format!("ix_fleet_lane_state_ratio{{host=\"{host_label}\",lane=\"{lane_label}\"}}");
        let read = || -> Result<(f64, f64, f64), String> {
            let compiler = query_sum(
                query,
                &format!("sum(increase({compiler_selector}[{span}]))"),
            )?
            .ok_or_else(|| "compiler_seconds_series_missing".to_owned())?;
            let compiler_samples = query_sum(
                query,
                &format!("sum(count_over_time({compiler_selector}[{span}]))"),
            )?
            .ok_or_else(|| "compiler_seconds_coverage_missing".to_owned())?;
            let state_samples = query_sum(
                query,
                &format!("sum(count_over_time({state_selector}[{span}]))"),
            )?
            .ok_or_else(|| "lane_presence_series_missing".to_owned())?;
            let expected_samples = window_secs as f64 / push_interval_secs as f64;
            let lane_coverage =
                (state_samples * push_interval_secs as f64 / window_secs as f64).min(1.0);
            let compiler_coverage =
                (compiler_samples * push_interval_secs as f64 / window_secs as f64).min(1.0);
            if lane_coverage < 0.9 || compiler_coverage < 0.9 {
                return Err(format!(
                    "watcher_coverage_below_90_percent:lane={lane_coverage:.3},compiler={compiler_coverage:.3},expected_samples={expected_samples:.0}"
                ));
            }
            Ok((
                compiler,
                state_samples * push_interval_secs as f64,
                lane_coverage.min(compiler_coverage),
            ))
        };
        match read() {
            Ok((compiler, observed, lane_coverage)) => {
                compiler_total += compiler;
                observed_total += observed.min(window_secs as f64);
                coverage = coverage.min(lane_coverage);
            }
            Err(reason) => {
                return (
                    Evidence::unavailable(compile_source, reason.clone(), None),
                    Evidence::unavailable(observed_source, reason, None),
                );
            }
        }
    }
    (
        Evidence::available(compiler_total, compile_source, coverage),
        Evidence::available(observed_total, observed_source, coverage),
    )
}

/// Reads admission-blocked share for one complete tick interval.
pub fn live_admission_blocked_share(
    query: &dyn PromQuery,
    host: &str,
    interval_secs: u64,
    push_interval_secs: u64,
) -> Evidence<f64> {
    let source = "prometheus.ix_build_admission_blocked_seconds_total";
    if interval_secs == 0 || push_interval_secs == 0 {
        return Evidence::unavailable(source, "invalid_interval", None);
    }
    let selector = format!(
        "ix_build_admission_blocked_seconds_total{{host=\"{}\"}}",
        escape_label(host)
    );
    let span = format!("{interval_secs}s");
    let blocked = query_sum(query, &format!("sum(increase({selector}[{span}]))"));
    let samples = query_sum(query, &format!("sum(count_over_time({selector}[{span}]))"));
    let coverage = samples
        .as_ref()
        .ok()
        .and_then(|samples| *samples)
        .map(|samples| (samples * push_interval_secs as f64 / interval_secs as f64).min(1.0));
    match (blocked, samples) {
        (Ok(Some(blocked)), Ok(Some(_))) if coverage.is_some_and(|value| value >= 0.9) => {
            Evidence::available(
                (blocked / interval_secs as f64).clamp(0.0, 1.0),
                source,
                coverage.unwrap_or_default(),
            )
        }
        (Ok(Some(_)), Ok(Some(_))) => {
            Evidence::unavailable(source, "coverage_below_90_percent", coverage)
        }
        (Err(error), _) | (_, Err(error)) => Evidence::unavailable(source, error, coverage),
        _ => Evidence::unavailable(source, "series_missing", coverage),
    }
}

/// Determines whether a drain is complete from fresh watcher state and lane compiler activity.
pub fn live_drain_done(
    query: &dyn PromQuery,
    host: &str,
    lane: &str,
    push_interval_secs: u64,
    window_secs: u64,
) -> Evidence<bool> {
    let source = "relay_watcher.ix_fleet_lane_state_ratio+ix_fleet_lane_compile_seconds_total";
    if push_interval_secs == 0 || window_secs == 0 {
        return Evidence::unavailable(source, "invalid_interval", None);
    }
    let selector = format!(
        "ix_fleet_lane_state_ratio{{host=\"{}\",lane=\"{}\"}}",
        escape_label(host),
        escape_label(lane)
    );
    let freshness = query_sum(query, &format!("max(time() - timestamp({selector}))"));
    let non_working_selector = format!(
        "ix_fleet_lane_state_ratio{{host=\"{}\",lane=\"{}\",state!=\"working\"}}",
        escape_label(host),
        escape_label(lane)
    );
    let non_working = query_sum(query, &format!("sum({non_working_selector})"));
    let compile_selector = format!(
        "ix_fleet_lane_compile_seconds_total{{host=\"{}\",lane=\"{}\",job=\"ix-relay-watcher\"}}",
        escape_label(host),
        escape_label(lane)
    );
    let compile = query_sum(
        query,
        &format!("sum(increase({compile_selector}[{window_secs}s]))"),
    );
    match (freshness, non_working, compile) {
        (Ok(Some(age)), Ok(state), Ok(compile))
            if age >= 0.0 && age <= (2 * push_interval_secs) as f64 =>
        {
            Evidence::available(
                state.is_some_and(|value| value > 0.0) && compile.unwrap_or(0.0) <= 0.0,
                source,
                1.0,
            )
        }
        (Ok(None), _, _) => Evidence::unavailable(source, "watcher_state_missing_or_stale", None),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
            Evidence::unavailable(source, error, None)
        }
        _ => Evidence::unavailable(source, "watcher_state_stale", None),
    }
}

/// Computes supply, mean duty, target, buffer, and planner shares with no external reads.
pub fn evaluate(input: SaturationInput) -> SaturationReport {
    let mut hosts = Vec::new();
    let mut host_targets = BTreeMap::new();
    let mut complete = true;
    for source in input.hosts {
        let duty = match (
            value(&source.compiler_seconds),
            value(&source.observed_lane_seconds),
        ) {
            (Some(seconds), Some(observed)) if observed > 0.0 => Evidence::available(
                (seconds / observed).clamp(0.0, 1.0),
                "relay_watcher.compiler_seconds/observed_lane_seconds",
                source
                    .compiler_seconds
                    .coverage
                    .unwrap_or(0.0)
                    .min(source.observed_lane_seconds.coverage.unwrap_or(0.0)),
            ),
            (Some(_), Some(_)) => Evidence::unavailable(
                "relay_watcher.compiler_seconds/observed_lane_seconds",
                "zero_observed_lane_time",
                Some(0.0),
            ),
            _ => Evidence::unavailable(
                "relay_watcher.compiler_seconds",
                source
                    .compiler_seconds
                    .reason
                    .clone()
                    .or_else(|| source.observed_lane_seconds.reason.clone())
                    .unwrap_or_else(|| "duty_source_unavailable".to_owned()),
                source
                    .compiler_seconds
                    .coverage
                    .zip(source.observed_lane_seconds.coverage)
                    .map(|(a, b)| a.min(b)),
            ),
        };
        let needed = match (
            value(&source.sustainable_slots),
            value(&source.lane_capacity),
            value(&duty),
        ) {
            (Some(slots), Some(capacity), Some(duty)) if duty > 0.0 => {
                let count = (f64::from(slots) / duty).ceil() as u32;
                Some(count.min(capacity))
            }
            (Some(_), Some(_), Some(_)) => None,
            _ => None,
        };
        let lanes_needed = if let Some(needed) = needed {
            Evidence::available(
                needed,
                "ceil(ix_board_config.fleet.capacity_limits.build_slots/relay_watcher.duty)",
                1.0,
            )
        } else {
            complete = false;
            Evidence::unavailable(
                "ceil(ix_board_config.fleet.capacity_limits.build_slots/relay_watcher.duty)",
                if value(&source.sustainable_slots).is_none() {
                    source
                        .sustainable_slots
                        .reason
                        .clone()
                        .unwrap_or_else(|| "sustainable_slots_unavailable".to_owned())
                } else if value(&source.lane_capacity).is_none() {
                    source
                        .lane_capacity
                        .reason
                        .clone()
                        .unwrap_or_else(|| "lane_capacity_unavailable".to_owned())
                } else if value(&duty).is_none() {
                    "duty_unavailable".to_owned()
                } else {
                    "zero_duty".to_owned()
                },
                source
                    .sustainable_slots
                    .coverage
                    .zip(duty.coverage)
                    .map(|(a, b)| a.min(b)),
            )
        };
        let target = lanes_needed
            .value
            .map(|needed| needed.saturating_mul(input.queue_depth));
        let target_ready = target.map_or_else(
            || unavailable("fleet.capacity", "lanes_needed_unavailable"),
            |target| Evidence::available(target, "fleet.queue_depth", 1.0),
        );
        let ready_buffer = unavailable("linear.ready_queue", "project_buffer_pending");
        let ready_buffer_deficit = unavailable("fleet.capacity", "project_buffer_pending");
        if let Some(target) = target {
            host_targets.insert(source.host.clone(), target);
        }
        hosts.push(HostSaturation {
            host: source.host,
            sustainable_slots: source.sustainable_slots,
            lane_capacity: source.lane_capacity,
            build_slots_busy: source.build_slots_busy,
            build_slots_configured: source.build_slots_configured,
            duty,
            lanes_needed,
            active_lanes: source.active_lanes,
            target_ready,
            ready_buffer,
            ready_buffer_deficit,
        });
    }
    let total_ready = input
        .project_ready
        .as_ref()
        .map(|ready| ready.values().copied().sum::<u32>());
    complete &= total_ready.is_some();
    let target_total = complete.then(|| host_targets.values().copied().sum::<u32>());
    let deficit_total = target_total
        .zip(total_ready)
        .map(|(target, ready)| target.saturating_sub(ready));
    let known_target_total = host_targets.values().copied().sum::<u32>();
    if let Some(ready) = total_ready {
        for host in &mut hosts {
            if let Some(target) = host.target_ready.value {
                let allocated = ready
                    .saturating_mul(target)
                    .checked_div(known_target_total)
                    .unwrap_or_default();
                host.ready_buffer =
                    Evidence::available(allocated, "linear.ready_queue.allocated", 1.0);
                host.ready_buffer_deficit =
                    Evidence::available(target.saturating_sub(allocated), "fleet.capacity", 1.0);
            }
        }
    }
    let mut project_names: BTreeMap<String, u32> = input
        .project_lanes
        .iter()
        .map(|(name, lanes)| (name.clone(), *lanes))
        .collect();
    if let Some(ready) = &input.project_ready {
        for project in ready.keys() {
            project_names.entry(project.clone()).or_default();
        }
    }
    let mut projects: Vec<ProjectBuffer> = project_names
        .into_iter()
        .map(|(project, lanes)| ProjectBuffer {
            ready: input
                .project_ready
                .as_ref()
                .and_then(|ready| ready.get(&project).copied())
                .unwrap_or(0),
            planner: input.planners.get(&project).cloned(),
            project,
            lanes,
            need: 0,
        })
        .collect();
    if let Some(deficit) = deficit_total {
        let total_lanes = projects.iter().map(|p| p.lanes).sum::<u32>();
        let mut assigned = 0u32;
        for project in &mut projects {
            project.need = deficit
                .saturating_mul(project.lanes)
                .checked_div(total_lanes)
                .unwrap_or_default();
            assigned = assigned.saturating_add(project.need);
        }
        if total_lanes > 0 {
            if let Some(receiver) = projects
                .iter_mut()
                .filter(|project| project.lanes > 0)
                .min_by(|a, b| {
                    a.ready
                        .cmp(&b.ready)
                        .then_with(|| a.project.cmp(&b.project))
                })
            {
                receiver.need = receiver
                    .need
                    .saturating_add(deficit.saturating_sub(assigned));
            }
        } else if deficit > 0 {
            complete = false;
        }
    }
    complete &= projects
        .iter()
        .all(|project| project.need == 0 || project.planner.is_some());
    let ready_buffer = total_ready.map_or_else(
        || unavailable("linear.ready_queue", "project_ready_unavailable"),
        |ready| Evidence::available(ready, "linear.ready_queue", 1.0),
    );
    let target_ready = target_total.map_or_else(
        || unavailable("fleet.capacity", "required_supply_or_duty_unavailable"),
        |target| Evidence::available(target, "fleet.capacity", 1.0),
    );
    let ready_buffer_deficit = deficit_total.map_or_else(
        || unavailable("fleet.capacity", "required_supply_or_duty_unavailable"),
        |deficit| Evidence::available(deficit, "fleet.capacity", 1.0),
    );
    SaturationReport {
        schema_version: 1,
        complete,
        observed_at_ms: input.observed_at_ms,
        window_secs: input.window_secs,
        queue_depth: input.queue_depth,
        hosts,
        projects,
        ready_buffer,
        target_ready,
        ready_buffer_deficit,
    }
}

/// Reads production relay, Linear and Prometheus sources and evaluates live fleet saturation.
///
/// Source failures stay attached to the affected fields as unavailable evidence.
pub fn read_live(
    config: &crate::fleet::FleetConfig,
    settings: &crate::fleet::tick::TickSettings,
) -> Result<SaturationReport, crate::fleet::FleetError> {
    use crate::fleet::classify::ClassKind;
    use crate::fleet::live::{self, PromHttp};
    use crate::fleet::{Coverage, SeriesSource, Sources, build_report};
    use crate::{LinearGraphQlSource, ReadService, WorkflowStateType};
    use relay_core::Relay;
    use relay_core::config::{CoreFlags, load_core};
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Arc;

    if config.saturation_projects.is_empty() {
        return Err(crate::fleet::FleetError::Config(
            "fleet.saturation_projects must list the Linear projects that supply the ready buffer"
                .to_owned(),
        ));
    }
    let prometheus = config
        .prometheus_url
        .as_deref()
        .map(|url| PromHttp::new(url, config.host_timeout))
        .transpose()
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let series: Option<Arc<dyn SeriesSource>> = prometheus.as_ref().map(|query| {
        Arc::new(live::PrometheusSeries {
            query: query.clone(),
        }) as Arc<dyn SeriesSource>
    });
    let (relay_snapshot, receipts) = live::read_relay(config.host_timeout);
    let sources = Sources {
        relay: relay_snapshot,
        series,
        receipts: &receipts,
        weak: &live::NoWeakSignals,
    };
    let report = build_report(config, &sources, now_ms());
    if report.relay_unavailable || report.coverage() == Coverage::Failed {
        return Err(crate::fleet::FleetError::Tick(
            "relay lane registry unavailable for saturation".to_owned(),
        ));
    }

    let relay_settings = load_core(CoreFlags::default())
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let mut relay = Relay::connect(&relay_settings.url.value, config.host_timeout)
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?
        .with_prefix(relay_settings.prefix.value.clone());
    let assignments = relay
        .assignments()
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let capacities = relay
        .server_capacities()
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let capacity_by_host: HashMap<String, _> = capacities
        .into_iter()
        .map(|capacity| (capacity.machine.to_string().to_lowercase(), capacity))
        .collect();

    let source = LinearGraphQlSource::from_env()
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let mut service = ReadService::open_default(source)
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let workspace = service
        .workspace_snapshot()
        .map_err(|error| crate::fleet::FleetError::Tick(error.to_string()))?;
    let selected: BTreeSet<String> = config
        .saturation_projects
        .iter()
        .map(|project| project.to_lowercase())
        .collect();
    let mut project_keys = HashMap::new();
    let mut matched_projects = BTreeSet::new();
    for project in &workspace.projects {
        let key = project
            .identifier
            .as_deref()
            .unwrap_or(&project.name)
            .to_owned();
        let key_lower = key.to_lowercase();
        let name_lower = project.name.to_lowercase();
        if selected.contains(&key_lower) || selected.contains(&name_lower) {
            project_keys.insert(project.id.0.clone(), key);
            if selected.contains(&key_lower) {
                matched_projects.insert(key_lower);
            }
            if selected.contains(&name_lower) {
                matched_projects.insert(name_lower);
            }
        }
    }
    let missing_projects: Vec<_> = selected.difference(&matched_projects).cloned().collect();
    if !missing_projects.is_empty() {
        return Err(crate::fleet::FleetError::Config(format!(
            "fleet.saturation_projects not found in the Linear workspace: {}",
            missing_projects.join(", ")
        )));
    }
    let issue_map: HashMap<String, _> = workspace
        .issues
        .iter()
        .map(|issue| (issue.identifier.to_lowercase(), issue))
        .collect();

    let reachable_lanes: BTreeSet<String> = report
        .rows
        .iter()
        .filter(|row| !matches!(row.class, ClassKind::Dead | ClassKind::Unreachable))
        .map(|row| row.lane.to_lowercase())
        .collect();
    let mut reachable_assigned = BTreeSet::new();
    let mut lane_project: BTreeMap<String, String> = BTreeMap::new();
    for list in assignments {
        let lane = list.lane.to_lowercase();
        if !reachable_lanes.contains(&lane) {
            continue;
        }
        for ticket in list.tickets {
            let key = ticket
                .reference
                .rsplit('/')
                .next()
                .unwrap_or(&ticket.reference)
                .to_lowercase();
            reachable_assigned.insert(key.clone());
            if let Some(project) = issue_map
                .get(&key)
                .and_then(|issue| issue.project_id.as_ref())
                .and_then(|project| project_keys.get(&project.0))
            {
                lane_project
                    .entry(lane.clone())
                    .or_insert_with(|| project.clone());
            }
        }
    }

    let mut project_ready: BTreeMap<String, u32> = project_keys
        .values()
        .cloned()
        .map(|project| (project, 0))
        .collect();
    for issue in &workspace.issues {
        let Some(project_id) = issue.project_id.as_ref() else {
            continue;
        };
        let Some(project) = project_keys.get(&project_id.0) else {
            continue;
        };
        let ready_and_unassigned = issue.state_type == WorkflowStateType::Unstarted
            && issue.assignee_id.is_none()
            && issue.started_at.is_none()
            && !reachable_assigned.contains(&issue.identifier.to_lowercase());
        if ready_and_unassigned && let Some(ready) = project_ready.get_mut(project) {
            *ready = ready.saturating_add(1);
        }
    }
    let mut project_lanes = BTreeMap::<String, u32>::new();
    for project in project_keys.values() {
        project_lanes.entry(project.clone()).or_default();
    }
    for project in lane_project.values() {
        let count = project_lanes.entry(project.clone()).or_default();
        *count = count.saturating_add(1);
    }

    let now = report.run_ms;
    let window = config.saturation_window.as_secs();
    let mut hosts = Vec::new();
    for host in &config.hosts {
        let host_key = host.to_lowercase();
        let lanes: Vec<String> = reachable_lanes
            .iter()
            .filter(|lane| {
                lane.split(':')
                    .next()
                    .is_some_and(|name| name == host_key.as_str())
            })
            .cloned()
            .collect();
        let capacity = capacity_by_host.get(&host_key);
        let sustainable_slots = config
            .capacity_limits
            .get(&host_key)
            .and_then(|limits| limits.build_slots)
            .map_or_else(
                || {
                    Evidence::unavailable(
                        format!("ix_board_config:fleet.capacity_limits.{host_key}.build_slots"),
                        "build_slots_not_configured",
                        None,
                    )
                },
                |slots| {
                    Evidence::available(
                        slots,
                        format!("ix_board_config:fleet.capacity_limits.{host_key}.build_slots"),
                        1.0,
                    )
                },
            );
        let lane_capacity = capacity
            .and_then(|capacity| capacity.max_agents)
            .map_or_else(
                || {
                    Evidence::unavailable(
                        "relay.server_capacity.max_agents",
                        "lane_capacity_not_configured",
                        None,
                    )
                },
                |lanes| Evidence::available(lanes, "relay.server_capacity.max_agents", 1.0),
            );
        let (compiler_seconds, observed_lane_seconds) = prometheus
            .as_ref()
            .map(|query| {
                live_duty(
                    query,
                    &host_key,
                    &lanes,
                    window,
                    config.push_interval.as_secs(),
                )
            })
            .unwrap_or_else(|| {
                (
                    Evidence::unavailable(
                        "prometheus.ix_fleet_lane_compile_seconds_total",
                        "prometheus_url_unconfigured",
                        None,
                    ),
                    Evidence::unavailable(
                        "prometheus.ix_fleet_lane_state_ratio",
                        "prometheus_url_unconfigured",
                        None,
                    ),
                )
            });
        let build_slots_busy = prometheus
            .as_ref()
            .and_then(|query| {
                query_sum(
                    query,
                    &format!(
                        "sum(ix_build_slots_busy{{host=\"{}\"}})",
                        escape_label(&host_key)
                    ),
                )
                .ok()
                .flatten()
            })
            .and_then(slot_count)
            .map_or_else(
                || Evidence::unavailable("prometheus.ix_build_slots_busy", "series_missing", None),
                |value| Evidence::available(value, "prometheus.ix_build_slots_busy", 1.0),
            );
        let build_slots_configured = prometheus
            .as_ref()
            .and_then(|query| {
                query_sum(
                    query,
                    &format!(
                        "sum(ix_build_slots_configured{{host=\"{}\"}})",
                        escape_label(&host_key)
                    ),
                )
                .ok()
                .flatten()
            })
            .and_then(slot_count)
            .map_or_else(
                || {
                    Evidence::unavailable(
                        "prometheus.ix_build_slots_configured",
                        "series_missing",
                        None,
                    )
                },
                |value| Evidence::available(value, "prometheus.ix_build_slots_configured", 1.0),
            );
        let active_lanes = u32::try_from(lanes.len()).unwrap_or(u32::MAX);
        hosts.push(HostInput {
            host: host_key,
            sustainable_slots,
            lane_capacity,
            build_slots_busy,
            build_slots_configured,
            compiler_seconds,
            observed_lane_seconds,
            active_lanes,
        });
    }
    Ok(evaluate(SaturationInput {
        observed_at_ms: now,
        window_secs: window,
        queue_depth: config.saturation_queue_depth,
        hosts,
        project_ready: Some(project_ready),
        planners: settings.planners.clone(),
        project_lanes,
    }))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_i64, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::SourceError;
    use std::sync::Mutex;

    struct DutyPrometheus {
        queries: Mutex<Vec<String>>,
        compiler_samples: f64,
    }

    impl PromQuery for DutyPrometheus {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            self.queries.lock().unwrap().push(expression.to_owned());
            let value = if expression.contains("increase(ix_fleet_lane_compile_seconds_total") {
                360.0
            } else if expression.contains("count_over_time(ix_fleet_lane_compile_seconds_total") {
                self.compiler_samples
            } else {
                108.0
            };
            if value == 0.0 {
                return Ok(r#"{"status":"success","data":{"result":[]}}"#.to_owned());
            }
            Ok(format!(
                r#"{{"status":"success","data":{{"result":[{{"value":[0,"{value}"]}}]}}}}"#
            ))
        }
    }

    struct AdmissionPrometheus {
        blocked: Option<f64>,
        samples: Option<f64>,
        queries: Mutex<Vec<String>>,
    }

    impl PromQuery for AdmissionPrometheus {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            self.queries.lock().unwrap().push(expression.to_owned());
            let value = if expression.contains("increase(ix_build_admission_blocked_seconds_total")
            {
                self.blocked
            } else {
                self.samples
            };
            Ok(match value {
                Some(value) => format!(
                    r#"{{"status":"success","data":{{"result":[{{"value":[0,"{value}"]}}]}}}}"#
                ),
                None => r#"{"status":"success","data":{"result":[]}}"#.to_owned(),
            })
        }
    }

    struct DrainPrometheus {
        fresh_age: Option<f64>,
        non_working: Option<f64>,
        compile: Option<f64>,
    }

    impl PromQuery for DrainPrometheus {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            let value = if expression.contains("timestamp(ix_fleet_lane_state_ratio") {
                self.fresh_age
            } else if expression.contains("state!=\"working\"") {
                self.non_working
            } else {
                self.compile
            };
            Ok(match value {
                Some(value) => format!(
                    r#"{{"status":"success","data":{{"result":[{{"value":[0,"{value}"]}}]}}}}"#
                ),
                None => r#"{"status":"success","data":{"result":[]}}"#.to_owned(),
            })
        }
    }

    #[test]
    fn drain_done_requires_fresh_non_working_state_and_no_compile_increase() {
        let source = DrainPrometheus {
            fresh_age: Some(10.0),
            non_working: Some(1.0),
            compile: None,
        };
        assert_eq!(
            live_drain_done(&source, "draco", "draco:workspace:build", 30, 300).value,
            Some(true)
        );
        let building = DrainPrometheus {
            compile: Some(1.0),
            ..source
        };
        assert_eq!(
            live_drain_done(&building, "draco", "draco:workspace:build", 30, 300).value,
            Some(false)
        );
        let stale = DrainPrometheus {
            fresh_age: Some(90.0),
            non_working: Some(1.0),
            compile: None,
        };
        assert_eq!(
            live_drain_done(&stale, "draco", "draco:workspace:build", 30, 300).value,
            None
        );
    }

    #[test]
    fn live_throttle_share_uses_admission_blocked_seconds_and_coverage() {
        let source = AdmissionPrometheus {
            blocked: Some(30.0),
            samples: Some(9.0),
            queries: Mutex::new(Vec::new()),
        };
        let evidence = live_admission_blocked_share(&source, "draco", 300, 30);
        assert_eq!(evidence.value, Some(0.1));
        assert_eq!(evidence.coverage, Some(0.9));
        let queries = source.queries.lock().unwrap();
        assert!(
            queries
                .iter()
                .any(|q| q.contains("increase(ix_build_admission_blocked_seconds_total"))
        );
        assert!(
            queries
                .iter()
                .any(|q| q.contains("count_over_time(ix_build_admission_blocked_seconds_total"))
        );
    }

    #[test]
    fn live_throttle_share_is_unavailable_below_ninety_percent_coverage() {
        let source = AdmissionPrometheus {
            blocked: Some(30.0),
            samples: Some(8.0),
            queries: Mutex::new(Vec::new()),
        };
        let evidence = live_admission_blocked_share(&source, "draco", 300, 30);
        assert_eq!(evidence.value, None);
        assert_eq!(
            evidence.reason.as_deref(),
            Some("coverage_below_90_percent")
        );
    }

    #[test]
    fn live_watcher_duty_uses_compiler_counter_and_lane_presence_seconds() {
        let source = DutyPrometheus {
            queries: Mutex::new(Vec::new()),
            compiler_samples: 108.0,
        };
        let (compiler, observed) = live_duty(
            &source,
            "draco",
            &["draco:ix-relay:builder".to_owned()],
            3_600,
            30,
        );
        assert_eq!(compiler.value, Some(360.0));
        assert_eq!(observed.value, Some(3_240.0));
        assert_eq!(compiler.coverage, Some(0.9));
        assert_eq!(observed.coverage, Some(0.9));
        let queries = source.queries.lock().unwrap();
        assert!(queries.iter().any(|query| {
            query.contains("ix_fleet_lane_compile_seconds_total")
                && query.contains("job=\"ix-relay-watcher\"")
                && query.contains("increase(")
        }));
        assert!(queries.iter().any(|query| {
            query.contains("ix_fleet_lane_state_ratio")
                && query.contains("count_over_time(")
                && query.contains("host=\"draco\"")
        }));
    }

    #[test]
    fn missing_watcher_coverage_never_becomes_zero_duty() {
        let source = DutyPrometheus {
            queries: Mutex::new(Vec::new()),
            compiler_samples: 0.0,
        };
        let (compiler, observed) = live_duty(
            &source,
            "draco",
            &["draco:ix-relay:builder".to_owned()],
            3_600,
            30,
        );
        assert_eq!(compiler.value, None);
        assert_eq!(observed.value, None);
        assert!(compiler.reason.unwrap().contains("coverage"));
    }

    #[test]
    fn positive_deficit_is_split_across_projects_with_exact_total() {
        let input = SaturationInput {
            observed_at_ms: 1_000,
            window_secs: 86_400,
            queue_depth: 2,
            hosts: vec![HostInput {
                host: "draco".to_owned(),
                sustainable_slots: Evidence::available(
                    4,
                    "fleet.capacity_limits.draco.build_slots",
                    1.0,
                ),
                lane_capacity: Evidence::available(10, "relay.server_capacity.max_agents", 1.0),
                build_slots_busy: Evidence::unavailable(
                    "ix_build_slots_busy",
                    "series_missing",
                    None,
                ),
                build_slots_configured: Evidence::unavailable(
                    "ix_build_slots_configured",
                    "series_missing",
                    None,
                ),
                compiler_seconds: Evidence::available(4.0, "watcher.compiler_seconds", 1.0),
                observed_lane_seconds: Evidence::available(4.0, "watcher.lane_seconds", 1.0),
                active_lanes: 3,
            }],
            project_ready: Some(BTreeMap::from([("p1".to_owned(), 1), ("p2".to_owned(), 2)])),
            planners: BTreeMap::from([
                ("p1".to_owned(), "draco:w:p1".to_owned()),
                ("p2".to_owned(), "draco:w:p2".to_owned()),
            ]),
            project_lanes: BTreeMap::from([("p1".to_owned(), 2), ("p2".to_owned(), 1)]),
        };
        let report = evaluate(input);
        assert_eq!(report.ready_buffer_deficit.value, Some(5));
        assert_eq!(
            report
                .projects
                .iter()
                .map(|project| project.need)
                .sum::<u32>(),
            5
        );
        assert!(report.complete);
    }
}
