// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Fleet supply, duty, and ready-buffer saturation (FR-041).

use crate::fleet::live::PromQuery;
use ix_trace_rs::implements;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
    /// Optional operator-entered limit on lane demand.
    pub lane_capacity: Evidence<u32>,
    /// Locked-build occupied slots, if the real producer is present.
    pub build_slots_busy: Evidence<u32>,
    /// Locked-build configured slots, if the real producer is present.
    pub build_slots_configured: Evidence<u32>,
    /// Arithmetic mean of per-lane compiler duty for lanes with sufficient observations.
    pub duty: Evidence<f64>,
    /// Reachable active lane count on this host.
    pub active_lanes: u32,
    /// Reachable lanes with at least one unfinished assigned ticket.
    pub supplied_lanes: u32,
    /// Reachable lanes without an unfinished assigned ticket.
    pub unsupplied_lanes: u32,
    /// Whether the relay assignment lists were read completely.
    pub assignment_evidence_available: bool,
    /// Whether the host's registry lease is stale.
    pub stale_host: bool,
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
    /// Reachable lanes with at least one unfinished assigned ticket.
    pub supplied_lanes: Option<u32>,
    /// Reachable lanes without an unfinished assigned ticket.
    pub unsupplied_lanes: Option<u32>,
    /// Finite share of the global ready buffer reserved for this host's unsupplied lanes.
    pub ready_share: Evidence<u32>,
    /// Distinct unfinished assignment tickets queued behind reachable lane heads.
    pub queued_behind_first: Evidence<u32>,
    /// Global ready tickets plus eligible lane-queue tails available for zero-lane discovery.
    pub bootstrap_ticket_pool: Evidence<u32>,
    /// Whether this host has a current registry lease.
    pub registry_current: bool,
    /// Target ready tickets for this host.
    pub target_ready: Evidence<u32>,
    /// Its ready share, or the bootstrap ticket pool when the host has no reachable lanes.
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
    /// Distinct unfinished tickets queued after the head of reachable lane assignment lists.
    pub queued_behind_first: Option<u32>,
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

#[implements("FR-041-AC-2")]
fn fr001_ready_candidate(
    issue: &crate::Issue,
    project_started: bool,
    blockers_complete: bool,
    reachable_assignment: bool,
) -> bool {
    project_started
        && matches!(issue.state.as_str(), "Specced" | "Specify" | "Backlog")
        && !matches!(
            issue.state_type,
            crate::WorkflowStateType::Completed
                | crate::WorkflowStateType::Canceled
                | crate::WorkflowStateType::Duplicate
        )
        && issue.assignee_id.is_none()
        && blockers_complete
        && !reachable_assignment
}

#[implements("FR-041-AC-8")]
fn reachable_assignment_ticket_keys(
    report: &crate::fleet::FleetReport,
    assignments: &[(String, Vec<String>)],
) -> BTreeSet<String> {
    use crate::fleet::classify::ClassKind;

    let reachable_lanes: BTreeSet<String> = report
        .rows
        .iter()
        .filter(|row| !matches!(row.class, ClassKind::Dead | ClassKind::Unreachable))
        .map(|row| row.lane.to_lowercase())
        .collect();
    assignments
        .iter()
        .filter(|(lane, _)| reachable_lanes.contains(&lane.to_lowercase()))
        .flat_map(|(_, tickets)| tickets.iter().map(|ticket| ticket.to_lowercase()))
        .collect()
}

#[implements("FR-041-AC-11")]
fn is_unfinished_assignment(issue: &crate::Issue) -> bool {
    !matches!(
        issue.state_type,
        crate::WorkflowStateType::Completed
            | crate::WorkflowStateType::Canceled
            | crate::WorkflowStateType::Duplicate
    )
}

#[implements("FR-041-AC-6")]
fn allocate_idle_lane_shares(
    project_lanes: &mut BTreeMap<String, u32>,
    project_ready: &BTreeMap<String, u32>,
    idle_lanes: usize,
) {
    for _ in 0..idle_lanes {
        let Some(receiver) = project_lanes
            .keys()
            .min_by(|a, b| {
                project_lanes
                    .get(*a)
                    .copied()
                    .unwrap_or_default()
                    .cmp(&project_lanes.get(*b).copied().unwrap_or_default())
                    .then_with(|| project_ready.get(*a).cmp(&project_ready.get(*b)))
                    .then_with(|| a.cmp(b))
            })
            .cloned()
        else {
            break;
        };
        let count = project_lanes.get_mut(&receiver).expect("receiver exists");
        *count = count.saturating_add(1);
    }
}

#[implements("FR-041-AC-2")]
fn host_has_current_registry_lease(
    relay: &crate::fleet::report::RelaySnapshot,
    host: &str,
) -> bool {
    relay.lists.iter().any(|list| {
        list.machine.to_string().eq_ignore_ascii_case(host) && !list.expired(relay.now_ms)
    })
}

#[implements("FR-041-AC-1")]
fn slot_count(value: f64) -> Option<u32> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= f64::from(u32::MAX))
        .then_some(value as u32)
}

/// Reads watcher compiler duty from its live counters and lane-presence samples.
///
/// The denominator counts only observed non-idle lane-state samples times the configured push
/// interval, capped at the requested window for each lane. Lanes below 90% state coverage or the
/// minimum engaged time are excluded; the arithmetic mean uses the remaining lanes.
#[implements("FR-041-AC-5")]
pub fn live_duty(
    query: &dyn PromQuery,
    host: &str,
    lanes: &[String],
    window_secs: u64,
    push_interval_secs: u64,
    min_observation_secs: u64,
) -> Evidence<f64> {
    let compile_source = "prometheus.ix_fleet_lane_compile_seconds_total{job=ix-relay-watcher}/ix_fleet_lane_state_ratio{state!=idle}_engaged_time";
    if lanes.is_empty() || window_secs == 0 || push_interval_secs == 0 {
        return unavailable(compile_source, "no_lane_or_invalid_window");
    }
    let span = format!("{window_secs}s");
    let mut lane_duties = Vec::new();
    let mut coverage = 1.0_f64;
    let mut under_minimum = 0usize;
    let mut excluded = 0usize;
    for lane in lanes {
        let host_label = escape_label(host);
        let lane_label = escape_label(lane);
        let compiler_selector = format!(
            "ix_fleet_lane_compile_seconds_total{{host=\"{host_label}\",lane=\"{lane_label}\",job=\"ix-relay-watcher\"}}"
        );
        let state_selector = format!(
            "ix_fleet_lane_state_ratio{{host=\"{host_label}\",lane=\"{lane_label}\",job=\"ix-relay-watcher\"}}"
        );
        let engaged_state_selector = format!(
            "ix_fleet_lane_state_ratio{{host=\"{host_label}\",lane=\"{lane_label}\",job=\"ix-relay-watcher\",state!=\"idle\"}}"
        );
        let read = || -> Result<(f64, f64, f64, f64), String> {
            let compiler = query_sum(
                query,
                &format!("sum(increase({compiler_selector}[{span}]))"),
            )?;
            let compiler_samples = query_sum(
                query,
                &format!("sum(count_over_time({compiler_selector}[{span}]))"),
            )?;
            // Collapse state-label series so a state transition cannot double count one lane.
            let state_samples = query_sum(
                query,
                &format!(
                    "count_over_time((max by (host,lane) ({state_selector}))[{span}:{push_interval_secs}s])"
                ),
            )?
            .ok_or_else(|| "lane_presence_series_missing".to_owned())?;
            let engaged_samples = query_sum(
                query,
                &format!(
                    "count_over_time((max by (host,lane) ({engaged_state_selector}))[{span}:{push_interval_secs}s])"
                ),
            )?
            .unwrap_or(0.0);
            let expected_samples = window_secs as f64 / push_interval_secs as f64;
            let lane_coverage =
                (state_samples * push_interval_secs as f64 / window_secs as f64).min(1.0);
            if lane_coverage < 0.9 {
                return Err(format!(
                    "watcher_lane_coverage_below_90_percent:lane={lane_coverage:.3},expected_samples={expected_samples:.0}"
                ));
            }
            // Current watcher presence with no compiler series is evidence of zero build time.
            // If a compiler counter exists but is sparse, its increment does not cover this window.
            let (compiler, compiler_coverage) = match (compiler, compiler_samples) {
                (None, None) => (0.0, lane_coverage),
                (Some(compiler), Some(samples)) => {
                    let coverage =
                        (samples * push_interval_secs as f64 / window_secs as f64).min(1.0);
                    if coverage < 0.9 {
                        return Err(format!(
                            "watcher_compiler_coverage_below_90_percent:{coverage:.3}"
                        ));
                    }
                    (compiler, coverage)
                }
                (None, Some(_)) => (0.0, lane_coverage),
                (Some(_), None) => {
                    return Err("compiler_seconds_coverage_missing".to_owned());
                }
            };
            let observed = (state_samples * push_interval_secs as f64).min(window_secs as f64);
            let engaged = (engaged_samples * push_interval_secs as f64).min(window_secs as f64);
            Ok((
                compiler,
                observed,
                engaged,
                lane_coverage.min(compiler_coverage),
            ))
        };
        match read() {
            Ok((compiler, _observed, engaged, lane_coverage)) => {
                if engaged < min_observation_secs as f64 {
                    under_minimum += 1;
                    excluded += 1;
                    continue;
                }
                lane_duties.push((compiler / engaged).clamp(0.0, 1.0));
                coverage = coverage.min(lane_coverage);
            }
            Err(_) => excluded += 1,
        }
    }
    if lane_duties.is_empty() {
        let reason = if under_minimum > 0 {
            format!("no_lane_meets_minimum_observation:{min_observation_secs}s")
        } else {
            "no_lane_with_usable_watcher_coverage".to_owned()
        };
        return Evidence::unavailable(compile_source, reason, None);
    }
    let mean_duty = lane_duties.iter().sum::<f64>() / lane_duties.len() as f64;
    let mut evidence = Evidence::available(mean_duty, compile_source, coverage);
    if excluded > 0 {
        evidence.reason = Some(format!(
            "excluded_{excluded}_of_{}_lanes_for_coverage_or_minimum_observation",
            lanes.len()
        ));
    }
    evidence
}

/// Reads admission-blocked share for one complete tick interval.
#[implements("FR-041-AC-16")]
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
#[implements("FR-041-AC-15")]
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
    let freshness = query_sum(query, &format!("min(time() - timestamp({selector}))"));
    let non_working_selector = format!(
        "ix_fleet_lane_state_ratio{{host=\"{}\",lane=\"{}\",state!=\"working\"}}",
        escape_label(host),
        escape_label(lane)
    );
    let newest_non_working = format!(
        "({non_working_selector}) and on(host,lane,state) \
         (timestamp({non_working_selector}) == on(host,lane) group_left \
         max by(host,lane)(timestamp({selector})))"
    );
    let non_working = query_sum(query, &format!("sum({newest_non_working})"));
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

/// Allocates ready tickets across unsupplied lanes using largest remainders and host ID tie-breaks.
#[implements("FR-041-AC-11")]
fn allocate_ready_shares(hosts: &[HostInput], ready: u32) -> BTreeMap<String, u32> {
    let mut shares = BTreeMap::new();
    let total_unsupplied = hosts
        .iter()
        .map(|host| u64::from(host.unsupplied_lanes))
        .sum::<u64>();
    if total_unsupplied == 0 {
        return shares;
    }

    let allocatable = u64::from(ready).min(total_unsupplied);
    let mut remainders = Vec::new();
    let mut assigned = 0u64;
    for host in hosts {
        if host.unsupplied_lanes == 0 {
            continue;
        }
        let numerator = allocatable * u64::from(host.unsupplied_lanes);
        let share = numerator / total_unsupplied;
        let remainder = numerator % total_unsupplied;
        let share = u32::try_from(share).unwrap_or(u32::MAX);
        shares.insert(host.host.to_lowercase(), share);
        assigned += u64::from(share);
        remainders.push((remainder, host.host.to_lowercase(), host.unsupplied_lanes));
    }
    remainders.sort_by(
        |(left_remainder, left_host, _), (right_remainder, right_host, _)| {
            right_remainder
                .cmp(left_remainder)
                .then_with(|| left_host.cmp(right_host))
        },
    );
    let mut remaining = allocatable.saturating_sub(assigned);
    for (_, host, capacity) in remainders {
        if remaining == 0 {
            break;
        }
        let share = shares.entry(host).or_default();
        if *share < capacity {
            *share += 1;
            remaining -= 1;
        }
    }
    shares
}

#[implements("FR-041-AC-11")]
fn allocate_ready_by_targets(targets: &[(String, u32)], ready: u32) -> BTreeMap<String, u32> {
    let total = targets
        .iter()
        .map(|(_, target)| u64::from(*target))
        .sum::<u64>();
    let allocatable = u64::from(ready).min(total);
    if total == 0 || allocatable == 0 {
        return targets
            .iter()
            .map(|(host, _)| (host.to_lowercase(), 0))
            .collect();
    }
    let mut allocated = BTreeMap::new();
    let mut remainders = Vec::new();
    let mut assigned = 0_u64;
    for (host, target) in targets {
        let numerator = allocatable * u64::from(*target);
        let share = numerator / total;
        let remainder = numerator % total;
        allocated.insert(
            host.to_lowercase(),
            u32::try_from(share).unwrap_or(u32::MAX),
        );
        assigned += share;
        remainders.push((remainder, host.to_lowercase(), *target));
    }
    remainders.sort_by(|(left_rem, left_host, _), (right_rem, right_host, _)| {
        right_rem
            .cmp(left_rem)
            .then_with(|| left_host.cmp(right_host))
    });
    let mut remaining = allocatable.saturating_sub(assigned);
    for (_, host, target) in remainders {
        if remaining == 0 {
            break;
        }
        let share = allocated.entry(host).or_default();
        if *share < target {
            *share += 1;
            remaining -= 1;
        }
    }
    allocated
}

#[implements("FR-041-AC-11")]
fn bootstrap_ticket_pool(hosts: &[HostInput], ready: u32, queued_behind_first: u32) -> u32 {
    let total_unsupplied = hosts
        .iter()
        .map(|host| host.unsupplied_lanes)
        .fold(0u32, u32::saturating_add);
    ready
        .saturating_sub(total_unsupplied)
        .saturating_add(queued_behind_first)
}

/// Computes supply, mean duty, target, buffer, and planner shares with no external reads.
#[implements("FR-041-AC-1")]
#[implements("FR-041-AC-3")]
#[implements("FR-041-AC-4")]
#[implements("FR-041-AC-5")]
#[implements("FR-041-AC-6")]
#[implements("FR-041-AC-11")]
pub fn evaluate(input: SaturationInput) -> SaturationReport {
    let total_ready = input
        .project_ready
        .as_ref()
        .map(|ready| ready.values().copied().sum::<u32>());
    let assignments_available = input
        .hosts
        .iter()
        .all(|host| host.assignment_evidence_available);
    let ready_shares = total_ready
        .filter(|_| assignments_available)
        .map(|ready| allocate_ready_shares(&input.hosts, ready));
    let bootstrap_ticket_pool = total_ready
        .zip(input.queued_behind_first)
        .filter(|_| assignments_available)
        .map(|(ready, queued)| {
            Evidence::available(
                bootstrap_ticket_pool(&input.hosts, ready, queued),
                "linear.ready_queue.remaining_after_unsupplied_lanes+relay.assignment_queue_tails",
                1.0,
            )
        })
        .unwrap_or_else(|| {
            unavailable(
                "linear.ready_queue+relay.assignment_queue_tails",
                if total_ready.is_none() {
                    "project_ready_unavailable"
                } else {
                    "assignment_evidence_unavailable"
                },
            )
        });
    let queued_behind_first = input.queued_behind_first.map_or_else(
        || {
            unavailable(
                "relay.assignments.queue_tails",
                "assignment_evidence_unavailable",
            )
        },
        |count| Evidence::available(count, "relay.assignments.queue_tails", 1.0),
    );
    let mut hosts = Vec::new();
    let mut host_targets = BTreeMap::new();
    let mut complete = true;
    let mut active_hosts = 0usize;
    for source in input.hosts {
        if source.active_lanes == 0 {
            let ready_share = if !source.assignment_evidence_available {
                unavailable("relay.assignments", "assignment_evidence_unavailable")
            } else {
                Evidence::available(0, "no_unsupplied_lanes", 1.0)
            };
            hosts.push(HostSaturation {
                host: source.host,
                sustainable_slots: source.sustainable_slots,
                lane_capacity: source.lane_capacity,
                build_slots_busy: source.build_slots_busy,
                build_slots_configured: source.build_slots_configured,
                duty: source.duty,
                lanes_needed: Evidence::unavailable(
                    "relay.current_lane_state",
                    "no_current_reachable_lanes",
                    None,
                ),
                active_lanes: 0,
                supplied_lanes: source.assignment_evidence_available.then_some(0),
                unsupplied_lanes: source.assignment_evidence_available.then_some(0),
                ready_share: ready_share.clone(),
                queued_behind_first: queued_behind_first.clone(),
                bootstrap_ticket_pool: bootstrap_ticket_pool.clone(),
                registry_current: !source.stale_host,
                target_ready: Evidence::unavailable(
                    "fleet.capacity",
                    "no_current_reachable_lanes",
                    None,
                ),
                ready_buffer: bootstrap_ticket_pool.clone(),
                ready_buffer_deficit: unavailable("fleet.capacity", "host_target_unavailable"),
            });
            continue;
        }
        active_hosts += 1;
        let duty = source.duty.clone();
        let needed = match (value(&source.sustainable_slots), value(&duty)) {
            (Some(slots), Some(duty)) if duty > 0.0 => {
                let count = (f64::from(slots) / duty).ceil() as u32;
                Some(value(&source.lane_capacity).map_or(count, |capacity| count.min(capacity)))
            }
            _ => None,
        };
        let lanes_needed = if let Some(needed) = needed {
            Evidence::available(
                needed,
                "ceil(ix_board_config.fleet.capacity_limits.build_slots/mean_relay_watcher_lane_duty), bounded by operator lane capacity when present",
                1.0,
            )
        } else {
            complete = false;
            Evidence::unavailable(
                "ceil(ix_board_config.fleet.capacity_limits.build_slots/mean_relay_watcher_lane_duty)",
                if value(&source.sustainable_slots).is_none() {
                    source
                        .sustainable_slots
                        .reason
                        .clone()
                        .unwrap_or_else(|| "sustainable_slots_unavailable".to_owned())
                } else if value(&duty).is_none() {
                    duty.reason
                        .clone()
                        .unwrap_or_else(|| "duty_unavailable".to_owned())
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
        let ready_share = if !source.assignment_evidence_available {
            unavailable("relay.assignments", "assignment_evidence_unavailable")
        } else {
            ready_shares.as_ref().map_or_else(
                || unavailable("linear.ready_queue.share", "project_buffer_pending"),
                |shares| {
                    Evidence::available(
                        shares
                            .get(&source.host.to_lowercase())
                            .copied()
                            .unwrap_or_default(),
                        "linear.ready_queue.share_by_unsupplied_lanes",
                        1.0,
                    )
                },
            )
        };
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
            supplied_lanes: source
                .assignment_evidence_available
                .then_some(source.supplied_lanes),
            unsupplied_lanes: source
                .assignment_evidence_available
                .then_some(source.unsupplied_lanes),
            ready_share: ready_share.clone(),
            queued_behind_first: queued_behind_first.clone(),
            bootstrap_ticket_pool: bootstrap_ticket_pool.clone(),
            registry_current: !source.stale_host,
            target_ready,
            ready_buffer: ready_share,
            ready_buffer_deficit,
        });
    }
    complete &= active_hosts > 0;
    complete &= assignments_available;
    complete &= total_ready.is_some();
    let known_target_total = host_targets.values().copied().sum::<u32>();
    let target_total = (complete || !host_targets.is_empty()).then_some(known_target_total);
    let deficit_total = target_total
        .zip(total_ready)
        .map(|(target, ready)| target.saturating_sub(ready));
    if let Some(ready) = total_ready {
        let targets = hosts
            .iter()
            .filter_map(|host| {
                host.target_ready
                    .value
                    .map(|target| (host.host.clone(), target))
            })
            .collect::<Vec<_>>();
        let allocations = allocate_ready_by_targets(&targets, ready);
        for host in &mut hosts {
            if let Some(target) = host.target_ready.value {
                let allocated = allocations
                    .get(&host.host.to_lowercase())
                    .copied()
                    .unwrap_or_default();
                host.ready_buffer =
                    Evidence::available(allocated, "linear.ready_queue.target_share", 1.0);
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
            planner: input
                .planners
                .get(&project.to_lowercase())
                .cloned()
                .or_else(|| {
                    input
                        .planners
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(&project))
                        .map(|(_, planner)| planner.clone())
                }),
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
    let mut target_ready = target_total.map_or_else(
        || unavailable("fleet.capacity", "required_supply_or_duty_unavailable"),
        |target| {
            Evidence::available(
                target,
                if complete {
                    "fleet.capacity"
                } else {
                    "fleet.capacity.known_hosts_only"
                },
                if complete {
                    1.0
                } else {
                    host_targets.len() as f64 / active_hosts.max(1) as f64
                },
            )
        },
    );
    let mut ready_buffer_deficit = deficit_total.map_or_else(
        || unavailable("fleet.capacity", "required_supply_or_duty_unavailable"),
        |deficit| {
            Evidence::available(
                deficit,
                if complete {
                    "fleet.capacity"
                } else {
                    "fleet.capacity.known_hosts_only"
                },
                if complete {
                    1.0
                } else {
                    host_targets.len() as f64 / active_hosts.max(1) as f64
                },
            )
        },
    );
    if !complete && target_total.is_some() {
        target_ready.reason = Some("one_or_more_hosts_or_planners_unavailable".to_owned());
        ready_buffer_deficit.reason =
            Some("partial_deficit_from_hosts_with_usable_supply_and_duty".to_owned());
    }
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
#[implements("FR-041-AC-5")]
#[implements("FR-041-AC-2")]
#[implements("FR-041-AC-8")]
#[implements("FR-041-AC-9")]
#[implements("FR-041-AC-24")]
#[implements("FR-041-AC-11")]
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
    let lease_current: HashMap<String, bool> = relay_snapshot
        .as_ref()
        .ok()
        .map(|relay| {
            config
                .hosts
                .iter()
                .map(|host| {
                    let current = host_has_current_registry_lease(relay, host);
                    (host.to_lowercase(), current)
                })
                .collect()
        })
        .unwrap_or_default();
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
    let assignment_evidence_available = true;

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
    let issues_by_id: HashMap<String, _> = workspace
        .issues
        .iter()
        .map(|issue| (issue.id.0.clone(), issue))
        .collect();

    let reachable_lanes: BTreeSet<String> = report
        .rows
        .iter()
        .filter(|row| !matches!(row.class, ClassKind::Dead | ClassKind::Unreachable))
        .map(|row| row.lane.to_lowercase())
        .collect();
    let assignment_keys = assignments
        .iter()
        .map(|list| {
            (
                list.lane.to_lowercase(),
                list.tickets
                    .iter()
                    .map(|ticket| {
                        if ticket.id.trim().is_empty() {
                            ticket.reference.trim().to_lowercase()
                        } else {
                            ticket.id.trim().to_lowercase()
                        }
                    })
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let reachable_assigned = reachable_assignment_ticket_keys(&report, &assignment_keys);
    let mut supplied_lanes = BTreeSet::new();
    let mut queued_behind_first = BTreeSet::new();
    let mut lane_project: BTreeMap<String, String> = BTreeMap::new();
    for list in assignments {
        let lane = list.lane.to_lowercase();
        if !reachable_lanes.contains(&lane) {
            continue;
        }
        for (position, ticket) in list.tickets.into_iter().enumerate() {
            let key = if ticket.id.trim().is_empty() {
                ticket.reference.trim().to_lowercase()
            } else {
                ticket.id.trim().to_lowercase()
            };
            let issue = issue_map.get(&key);
            if issue.is_some_and(|issue| is_unfinished_assignment(issue)) {
                supplied_lanes.insert(lane.clone());
                if position > 0 {
                    queued_behind_first.insert(key.clone());
                }
            }
            if let Some(project) = issue
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
        let Some(project_record) = workspace.projects.iter().find(|p| p.id == *project_id) else {
            continue;
        };
        let blockers_complete = {
            let mut pending: Vec<String> = issue.blocked_by.iter().map(|id| id.0.clone()).collect();
            let mut visited = BTreeSet::new();
            let mut complete = true;
            while let Some(blocker_id) = pending.pop() {
                if !visited.insert(blocker_id.clone()) {
                    continue;
                }
                let Some(blocker) = issues_by_id.get(&blocker_id) else {
                    complete = false;
                    break;
                };
                if blocker.state_type != WorkflowStateType::Completed {
                    complete = false;
                    break;
                }
                pending.extend(blocker.blocked_by.iter().map(|id| id.0.clone()));
            }
            complete
        };
        let ready_and_unassigned = fr001_ready_candidate(
            issue,
            project_record.status == "Started",
            blockers_complete,
            reachable_assigned.contains(&issue.identifier.to_lowercase()),
        );
        if ready_and_unassigned && let Some(ready) = project_ready.get_mut(project) {
            *ready = ready.saturating_add(1);
        }
    }
    let mut planners = BTreeMap::new();
    for project in &workspace.projects {
        let Some(key) = project_keys.get(&project.id.0) else {
            continue;
        };
        let aliases = [
            key.as_str(),
            project.identifier.as_deref().unwrap_or_default(),
            project.name.as_str(),
        ];
        if let Some(planner) = aliases
            .iter()
            .filter_map(|alias| settings.planners.get(&alias.to_lowercase()))
            .next()
        {
            planners.insert(key.to_lowercase(), planner.clone());
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
    // Idle lanes with no FR-072 assignments are shared supply. Give each one to the project with
    // the smallest current lane share, then the shortest ready buffer, then stable project key.
    let mut free_lanes: Vec<_> = reachable_lanes
        .difference(&supplied_lanes)
        .filter(|lane| !lane_project.contains_key(*lane))
        .cloned()
        .collect();
    free_lanes.sort();
    allocate_idle_lane_shares(&mut project_lanes, &project_ready, free_lanes.len());

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
        let current_lease = lease_current.get(&host_key).copied().unwrap_or(false);
        let stale_host = !current_lease
            || report.defects.iter().any(|defect| {
                defect.kind == crate::fleet::DefectKind::StaleHostRegistry
                    && defect
                        .host
                        .as_deref()
                        .is_some_and(|defect_host| defect_host.eq_ignore_ascii_case(&host_key))
            });
        let sustainable_slots = if stale_host {
            Evidence::available(
                0,
                format!(
                    "ix_board_config:fleet.capacity_limits.{host_key}.build_slots;stale_host_registry_lease"
                ),
                1.0,
            )
        } else {
            config
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
                )
        };
        let lane_capacity = if stale_host {
            Evidence::available(
                0,
                "ix_board_config:fleet.capacity_limits.lane_capacity;stale_host_registry_lease",
                1.0,
            )
        } else {
            config
                .capacity_limits
                .get(&host_key)
                .and_then(|limits| limits.lane_capacity)
                .map_or_else(
                    || {
                        Evidence::unavailable(
                            format!(
                                "ix_board_config:fleet.capacity_limits.{host_key}.lane_capacity"
                            ),
                            "lane_capacity_not_configured",
                            None,
                        )
                    },
                    |lanes| {
                        Evidence::available(
                            lanes,
                            format!(
                                "ix_board_config:fleet.capacity_limits.{host_key}.lane_capacity"
                            ),
                            1.0,
                        )
                    },
                )
        };
        let duty = prometheus
            .as_ref()
            .map(|query| {
                live_duty(
                    query,
                    &host_key,
                    &lanes,
                    window,
                    config.push_interval.as_secs(),
                    config.min_duty_observation.as_secs(),
                )
            })
            .unwrap_or_else(|| {
                Evidence::unavailable(
                    "prometheus.ix_fleet_lane_compile_seconds_total",
                    "prometheus_url_unconfigured",
                    None,
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
        let supplied_count = u32::try_from(
            lanes
                .iter()
                .filter(|lane| supplied_lanes.contains(*lane))
                .count(),
        )
        .unwrap_or(u32::MAX);
        hosts.push(HostInput {
            host: host_key,
            sustainable_slots,
            lane_capacity,
            build_slots_busy,
            build_slots_configured,
            duty,
            active_lanes,
            supplied_lanes: supplied_count,
            unsupplied_lanes: active_lanes.saturating_sub(supplied_count),
            assignment_evidence_available,
            stale_host,
        });
    }
    Ok(evaluate(SaturationInput {
        observed_at_ms: now,
        window_secs: window,
        queue_depth: config.saturation_queue_depth,
        hosts,
        project_ready: Some(project_ready),
        queued_behind_first: Some(u32::try_from(queued_behind_first.len()).unwrap_or(u32::MAX)),
        planners,
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
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-041-AC-11")]
    fn configured_host_with_no_reachable_lanes_is_not_registry_current() {
        let report = evaluate(SaturationInput {
            observed_at_ms: 1_000,
            window_secs: 3_600,
            queue_depth: 1,
            hosts: vec![HostInput {
                host: "draco".to_owned(),
                sustainable_slots: Evidence::available(2, "config", 1.0),
                lane_capacity: Evidence::unavailable(
                    "config",
                    "lane_capacity_not_configured",
                    None,
                ),
                build_slots_busy: Evidence::unavailable("broker", "series_missing", None),
                build_slots_configured: Evidence::unavailable("broker", "series_missing", None),
                duty: Evidence::unavailable("watcher", "no_current_lanes", None),
                active_lanes: 0,
                supplied_lanes: 0,
                unsupplied_lanes: 0,
                assignment_evidence_available: true,
                stale_host: true,
            }],
            project_ready: Some(BTreeMap::new()),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::new(),
        });
        assert!(!report.hosts[0].registry_current);
    }

    use std::sync::Mutex;

    fn ready_issue(state: &str, state_type: crate::WorkflowStateType) -> crate::Issue {
        crate::Issue {
            id: crate::IssueId("issue-1".to_owned()),
            identifier: "QSL-1".to_owned(),
            title: "ready candidate".to_owned(),
            description: None,
            state: state.to_owned(),
            state_name: state.to_owned(),
            state_type,
            parent_id: None,
            team_key: crate::TeamKey("QSL".to_owned()),
            assignee_id: None,
            assignee_name: None,
            project_id: Some(crate::ProjectId("project-1".to_owned())),
            project_milestone_id: None,
            priority: 0.0,
            sort_order: 0.0,
            blocked_by: Vec::new(),
            created_at: None,
            updated_at: None,
            attachment_urls: Vec::new(),
            attachments_complete: true,
            started_at: None,
            completed_at: None,
            pull_requests: Vec::new(),
        }
    }

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

    struct LaneCoveragePrometheus;

    impl PromQuery for LaneCoveragePrometheus {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            let is_short_lane = expression.contains("lane=\"draco:workspace:short\"");
            let value = if expression.contains("increase(ix_fleet_lane_compile_seconds_total") {
                if is_short_lane { 0.0 } else { 324.0 }
            } else if expression.contains("count_over_time(ix_fleet_lane_compile_seconds_total")
                || expression.contains("count_over_time((max by (host,lane)")
            {
                if is_short_lane { 80.0 } else { 108.0 }
            } else {
                return Err(SourceError(format!("unexpected query: {expression}")));
            };
            Ok(format!(
                r#"{{"status":"success","data":{{"result":[{{"value":[0,"{value}"]}}]}}}}"#
            ))
        }
    }

    struct NoCompileSeries;

    impl PromQuery for NoCompileSeries {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            if expression.contains("ix_fleet_lane_compile_seconds_total") {
                return Ok(r#"{"status":"success","data":{"result":[]}}"#.to_owned());
            }
            Ok(r#"{"status":"success","data":{"result":[{"value":[0,"108"]}]}}"#.to_owned())
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

    #[trace("FR-041-AC-15")]
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

    #[trace("FR-041-AC-16")]
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

    #[trace("FR-041-AC-16")]
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

    #[trace("FR-041-AC-5")]
    #[test]
    fn live_watcher_duty_uses_real_metrics_and_deduplicates_state_series() {
        let source = DutyPrometheus {
            queries: Mutex::new(Vec::new()),
            compiler_samples: 108.0,
        };
        let duty = live_duty(
            &source,
            "draco",
            &["draco:ix-relay:builder".to_owned()],
            3_600,
            30,
            3_000,
        );
        assert_eq!(duty.value, Some(360.0 / 3_240.0));
        assert_eq!(duty.coverage, Some(0.9));
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
                && query.contains("lane=\"draco:ix-relay:builder\"")
                && query.contains("job=\"ix-relay-watcher\"")
                && query.contains("max by (host,lane)")
        }));
        assert!(
            queries
                .iter()
                .any(|query| query.contains("state!=\"idle\""))
        );
    }

    struct StarvedLanePrometheus;

    impl PromQuery for StarvedLanePrometheus {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            let value = if expression.contains("increase(ix_fleet_lane_compile_seconds_total") {
                90.0
            } else if expression.contains("count_over_time(ix_fleet_lane_compile_seconds_total") {
                108.0
            } else if expression.contains("state!=\"idle\"") {
                36.0
            } else {
                108.0
            };
            Ok(format!(
                r#"{{"status":"success","data":{{"result":[{{"value":[0,"{value}"]}}]}}}}"#
            ))
        }
    }

    #[trace("FR-041-AC-5")]
    #[test]
    fn lane_duty_uses_engaged_time_and_excludes_idle_samples() {
        let duty = live_duty(
            &StarvedLanePrometheus,
            "draco",
            &["draco:workspace:builder".to_owned()],
            3_600,
            30,
            1_000,
        );
        assert_eq!(duty.value, Some(90.0 / 1_080.0));
    }

    #[trace("FR-041-AC-5")]
    #[test]
    fn missing_watcher_coverage_never_becomes_zero_duty() {
        let source = DutyPrometheus {
            queries: Mutex::new(Vec::new()),
            compiler_samples: 0.0,
        };
        let duty = live_duty(
            &source,
            "draco",
            &["draco:ix-relay:builder".to_owned()],
            3_600,
            30,
            3_000,
        );
        assert_eq!(duty.value, None);
        assert!(duty.reason.unwrap().contains("coverage"));
    }

    #[trace("FR-041-AC-5")]
    #[test]
    fn live_duty_excludes_undercovered_lanes_and_applies_minimum_observation() {
        let lanes = vec![
            "draco:workspace:good".to_owned(),
            "draco:workspace:short".to_owned(),
        ];
        let duty = live_duty(&LaneCoveragePrometheus, "draco", &lanes, 3_600, 30, 3_000);
        assert_eq!(duty.value, Some(0.1));

        let minimum_not_met = live_duty(
            &LaneCoveragePrometheus,
            "draco",
            &lanes[..1],
            3_600,
            30,
            3_300,
        );
        assert_eq!(minimum_not_met.value, None);
        assert_eq!(
            minimum_not_met.reason.as_deref(),
            Some("no_lane_meets_minimum_observation:3300s")
        );
    }

    #[trace("FR-041-AC-5")]
    #[test]
    fn current_watcher_lane_without_compile_series_is_zero_duty() {
        let duty = live_duty(
            &NoCompileSeries,
            "draco",
            &["draco:workspace:idle".to_owned()],
            3_600,
            30,
            3_000,
        );
        assert_eq!(duty.value, Some(0.0));
        let report = evaluate(SaturationInput {
            observed_at_ms: 1,
            window_secs: 3_600,
            queue_depth: 2,
            hosts: vec![HostInput {
                host: "draco".to_owned(),
                sustainable_slots: Evidence::available(2, "config", 1.0),
                lane_capacity: Evidence::available(4, "relay", 1.0),
                build_slots_busy: Evidence::unavailable("broker", "missing", None),
                build_slots_configured: Evidence::unavailable("broker", "missing", None),
                duty,
                active_lanes: 1,
                supplied_lanes: 0,
                unsupplied_lanes: 1,
                assignment_evidence_available: true,
                stale_host: false,
            }],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 0)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::from([("QSL".to_owned(), 1)]),
        });
        assert_eq!(report.hosts[0].lanes_needed.value, None);
        assert_eq!(
            report.hosts[0].lanes_needed.reason.as_deref(),
            Some("zero_duty")
        );
        assert_eq!(report.ready_buffer_deficit.value, None);
    }

    #[trace("FR-041-AC-1")]
    #[test]
    fn absent_operator_lane_capacity_does_not_block_demand_or_deficit() {
        let report = evaluate(SaturationInput {
            observed_at_ms: 1,
            window_secs: 3_600,
            queue_depth: 2,
            hosts: vec![HostInput {
                host: "draco".to_owned(),
                sustainable_slots: Evidence::available(2, "config", 1.0),
                lane_capacity: Evidence::unavailable(
                    "ix_board_config.fleet.capacity_limits.draco.lane_capacity",
                    "lane_capacity_not_configured",
                    None,
                ),
                build_slots_busy: Evidence::unavailable("broker", "missing", None),
                build_slots_configured: Evidence::unavailable("broker", "missing", None),
                duty: Evidence::available(0.5, "watcher", 1.0),
                active_lanes: 1,
                supplied_lanes: 0,
                unsupplied_lanes: 1,
                assignment_evidence_available: true,
                stale_host: false,
            }],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 2)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::from([("QSL".to_owned(), 1)]),
        });

        assert_eq!(report.hosts[0].lanes_needed.value, Some(4));
        assert_eq!(report.hosts[0].target_ready.value, Some(8));
        assert_eq!(report.hosts[0].ready_buffer_deficit.value, Some(6));
    }

    #[trace("FR-041-AC-2")]
    #[test]
    fn zero_lane_start_uses_shared_bootstrap_pool_when_lease_is_current() {
        let make_host = |host: &str, stale_host| HostInput {
            host: host.to_owned(),
            sustainable_slots: Evidence::available(2, "config", 1.0),
            lane_capacity: Evidence::unavailable(
                "ix_board_config.lane_capacity",
                "lane_capacity_not_configured",
                None,
            ),
            build_slots_busy: Evidence::unavailable("broker", "series_missing", None),
            build_slots_configured: Evidence::unavailable("broker", "series_missing", None),
            duty: Evidence::unavailable("watcher", "no_current_reachable_lanes", None),
            active_lanes: 0,
            supplied_lanes: 0,
            unsupplied_lanes: 0,
            assignment_evidence_available: true,
            stale_host,
        };
        let mut assigned = make_host("titan", false);
        assigned.active_lanes = 4;
        assigned.unsupplied_lanes = 4;
        let report = evaluate(SaturationInput {
            observed_at_ms: 1,
            window_secs: 3_600,
            queue_depth: 2,
            hosts: vec![
                assigned,
                make_host("luna", true),
                make_host("terra", false),
                make_host("draco", false),
            ],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 1)])),
            queued_behind_first: Some(3),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::new(),
        });
        for host in &report.hosts[1..] {
            assert_eq!(host.bootstrap_ticket_pool.value, Some(3));
        }
        assert!(!report.hosts[1].registry_current);
        assert!(report.hosts[2].registry_current);
        assert!(report.hosts[3].registry_current);
    }

    #[trace("FR-041-AC-2")]
    #[test]
    fn a_never_registered_host_has_no_current_registry_lease() {
        assert!(!host_has_current_registry_lease(
            &crate::fleet::report::RelaySnapshot::default(),
            "draco",
        ));
    }

    #[test]
    #[trace("FR-041-AC-11")]
    fn any_nonterminal_assignment_state_supplies_a_lane() {
        for state in [
            crate::WorkflowStateType::Triage,
            crate::WorkflowStateType::Backlog,
            crate::WorkflowStateType::Unstarted,
            crate::WorkflowStateType::Started,
        ] {
            assert!(is_unfinished_assignment(&ready_issue("Assigned", state)));
        }
        for state in [
            crate::WorkflowStateType::Completed,
            crate::WorkflowStateType::Canceled,
            crate::WorkflowStateType::Duplicate,
        ] {
            assert!(!is_unfinished_assignment(&ready_issue("Closed", state)));
        }
    }

    #[trace("FR-041-AC-11")]
    #[test]
    fn ready_shares_use_largest_remainder_and_never_duplicate_tickets() {
        let make_host = |host: &str| HostInput {
            host: host.to_owned(),
            sustainable_slots: Evidence::available(1, "config", 1.0),
            lane_capacity: Evidence::unavailable("config", "lane_capacity_not_configured", None),
            build_slots_busy: Evidence::unavailable("broker", "series_missing", None),
            build_slots_configured: Evidence::unavailable("broker", "series_missing", None),
            duty: Evidence::available(1.0, "watcher", 1.0),
            active_lanes: 1,
            supplied_lanes: 0,
            unsupplied_lanes: 1,
            assignment_evidence_available: true,
            stale_host: false,
        };
        let report = evaluate(SaturationInput {
            observed_at_ms: 1,
            window_secs: 3_600,
            queue_depth: 1,
            hosts: vec![make_host("terra"), make_host("draco")],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 1)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::new(),
        });
        let shares: BTreeMap<_, _> = report
            .hosts
            .iter()
            .map(|host| (host.host.as_str(), host.ready_share.value.unwrap()))
            .collect();
        assert_eq!(shares.get("draco"), Some(&1));
        assert_eq!(shares.get("terra"), Some(&0));
        assert_eq!(shares.values().sum::<u32>(), 1);
    }

    #[test]
    #[trace("FR-041-AC-11")]
    fn per_host_ready_buffers_and_deficits_sum_to_the_global_values() {
        let hosts = ["draco", "terra", "titan"]
            .into_iter()
            .map(|host| HostInput {
                host: host.to_owned(),
                sustainable_slots: Evidence::available(1, "config", 1.0),
                lane_capacity: Evidence::unavailable(
                    "config",
                    "lane_capacity_not_configured",
                    None,
                ),
                build_slots_busy: Evidence::unavailable("broker", "series_missing", None),
                build_slots_configured: Evidence::unavailable("broker", "series_missing", None),
                duty: Evidence::available(0.25, "watcher", 1.0),
                active_lanes: 1,
                supplied_lanes: 0,
                unsupplied_lanes: 1,
                assignment_evidence_available: true,
                stale_host: false,
            })
            .collect();
        let report = evaluate(SaturationInput {
            observed_at_ms: 1_000,
            window_secs: 3_600,
            queue_depth: 1,
            hosts,
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 2)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::from([("QSL".to_owned(), 3)]),
        });
        assert_eq!(report.ready_buffer.value, Some(2));
        assert_eq!(report.ready_buffer_deficit.value, Some(10));
        assert_eq!(
            report
                .hosts
                .iter()
                .map(|host| host.ready_buffer.value.unwrap())
                .sum::<u32>(),
            2
        );
        assert_eq!(
            report
                .hosts
                .iter()
                .map(|host| host.ready_buffer_deficit.value.unwrap())
                .sum::<u32>(),
            10
        );
    }

    #[trace("FR-041-AC-11")]
    #[test]
    fn missing_assignment_history_keeps_demand_evidence_unavailable() {
        let report = evaluate(SaturationInput {
            observed_at_ms: 1,
            window_secs: 3_600,
            queue_depth: 1,
            hosts: vec![HostInput {
                host: "draco".to_owned(),
                sustainable_slots: Evidence::available(1, "config", 1.0),
                lane_capacity: Evidence::unavailable(
                    "config",
                    "lane_capacity_not_configured",
                    None,
                ),
                build_slots_busy: Evidence::unavailable("broker", "series_missing", None),
                build_slots_configured: Evidence::unavailable("broker", "series_missing", None),
                duty: Evidence::available(1.0, "watcher", 1.0),
                active_lanes: 1,
                supplied_lanes: 0,
                unsupplied_lanes: 1,
                assignment_evidence_available: false,
                stale_host: false,
            }],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 2)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::new(),
        });
        assert!(!report.complete);
        assert_eq!(report.hosts[0].supplied_lanes, None);
        assert_eq!(report.hosts[0].ready_share.value, None);
        assert_eq!(
            report.hosts[0].ready_share.reason.as_deref(),
            Some("assignment_evidence_unavailable")
        );
    }

    #[trace("FR-041-AC-1")]
    #[test]
    fn entered_operator_lane_capacity_bounds_demand() {
        let report = evaluate(SaturationInput {
            observed_at_ms: 1,
            window_secs: 3_600,
            queue_depth: 2,
            hosts: vec![HostInput {
                host: "draco".to_owned(),
                sustainable_slots: Evidence::available(2, "config", 1.0),
                lane_capacity: Evidence::available(3, "ix_board_config.lane_capacity", 1.0),
                build_slots_busy: Evidence::unavailable("broker", "missing", None),
                build_slots_configured: Evidence::unavailable("broker", "missing", None),
                duty: Evidence::available(0.5, "watcher", 1.0),
                active_lanes: 1,
                supplied_lanes: 0,
                unsupplied_lanes: 1,
                assignment_evidence_available: true,
                stale_host: false,
            }],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 2)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::new(),
            project_lanes: BTreeMap::from([("QSL".to_owned(), 1)]),
        });

        assert_eq!(report.hosts[0].lanes_needed.value, Some(3));
        assert_eq!(report.hosts[0].target_ready.value, Some(6));
        assert_eq!(report.hosts[0].ready_buffer_deficit.value, Some(4));
    }

    #[trace("FR-041-AC-2")]
    #[test]
    fn ready_buffer_uses_fr001_project_state_blockers_and_spec_state() {
        let issue = ready_issue("Specced", crate::WorkflowStateType::Unstarted);
        assert!(fr001_ready_candidate(&issue, true, true, false));
        let backlog_issue = ready_issue("Backlog", crate::WorkflowStateType::Backlog);
        assert!(fr001_ready_candidate(&backlog_issue, true, true, false));
        assert!(!fr001_ready_candidate(&issue, false, true, false));
        assert!(!fr001_ready_candidate(&issue, true, false, false));
        assert!(!fr001_ready_candidate(&issue, true, true, true));
        let ineligible_spec = ready_issue("In Progress", crate::WorkflowStateType::Started);
        assert!(!fr001_ready_candidate(&ineligible_spec, true, true, false));
        let closed_issue = ready_issue("Backlog", crate::WorkflowStateType::Completed);
        assert!(!fr001_ready_candidate(&closed_issue, true, true, false));
    }

    #[trace("FR-041-AC-8")]
    #[test]
    fn unreachable_lane_assignment_does_not_remove_a_ticket_from_ready_buffer() {
        let issue = ready_issue("Backlog", crate::WorkflowStateType::Unstarted);
        assert!(fr001_ready_candidate(&issue, true, true, false));
        assert!(!fr001_ready_candidate(&issue, true, true, true));
    }

    #[trace("FR-041-AC-8")]
    #[test]
    fn ready_buffer_assembly_uses_only_assignments_on_reachable_report_lanes() {
        let report = crate::fleet::FleetReport {
            schema_version: 1,
            complete: true,
            run_ms: 1_000,
            rows: vec![crate::fleet::report::LaneRow {
                lane: "draco:ws:ready".to_owned(),
                host: "draco".to_owned(),
                class: crate::fleet::classify::ClassKind::Working,
                reason: None,
                state_age_secs: None,
                last_event_ms: None,
                evidence: Vec::new(),
                compile_seconds: None,
                focus: None,
                thresholds_defaulted: false,
                schema_version: 1,
            }],
            hosts: Vec::new(),
            defects: Vec::new(),
            host_busy: BTreeMap::new(),
            relay_unavailable: false,
        };
        let assignments = vec![
            ("draco:ws:ready".to_owned(), vec!["qsl-1".to_owned()]),
            ("terra:ws:unreachable".to_owned(), vec!["qsl-2".to_owned()]),
        ];
        let assigned = reachable_assignment_ticket_keys(&report, &assignments);
        let reachable_issue = ready_issue("Backlog", crate::WorkflowStateType::Unstarted);
        let mut unreachable_issue = reachable_issue.clone();
        unreachable_issue.identifier = "QSL-2".to_owned();

        assert!(!fr001_ready_candidate(
            &reachable_issue,
            true,
            true,
            assigned.contains("qsl-1")
        ));
        assert!(fr001_ready_candidate(
            &unreachable_issue,
            true,
            true,
            assigned.contains("qsl-2")
        ));
    }

    #[trace("FR-041-AC-6")]
    #[test]
    fn idle_unassigned_lanes_contribute_to_project_shares() {
        let mut lanes = BTreeMap::from([("Alpha".to_owned(), 0), ("Beta".to_owned(), 0)]);
        let ready = BTreeMap::from([("Alpha".to_owned(), 2), ("Beta".to_owned(), 0)]);
        allocate_idle_lane_shares(&mut lanes, &ready, 3);
        assert_eq!(
            lanes,
            BTreeMap::from([("Alpha".to_owned(), 1), ("Beta".to_owned(), 2)])
        );
    }

    #[trace("FR-041-AC-1", "FR-041-AC-6")]
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
                lane_capacity: Evidence::unavailable(
                    "ix_board_config.fleet.capacity_limits.draco.lane_capacity",
                    "lane_capacity_not_configured",
                    None,
                ),
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
                duty: Evidence::available(
                    1.0,
                    "watcher.compiler_seconds/observed_lane_seconds",
                    1.0,
                ),
                active_lanes: 3,
                supplied_lanes: 0,
                unsupplied_lanes: 3,
                assignment_evidence_available: true,
                stale_host: false,
            }],
            project_ready: Some(BTreeMap::from([("p1".to_owned(), 1), ("p2".to_owned(), 2)])),
            queued_behind_first: Some(0),
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

    #[test]
    fn missing_host_duty_keeps_known_host_deficit_with_partial_basis() {
        let host = |name: &str, duty: Evidence<f64>| HostInput {
            host: name.to_owned(),
            sustainable_slots: Evidence::available(4, format!("config.{name}.build_slots"), 1.0),
            lane_capacity: Evidence::available(8, "ix_board_config.lane_capacity", 1.0),
            build_slots_busy: Evidence::unavailable("ix_build_slots_busy", "series_missing", None),
            build_slots_configured: Evidence::unavailable(
                "ix_build_slots_configured",
                "series_missing",
                None,
            ),
            duty,
            active_lanes: 2,
            supplied_lanes: 0,
            unsupplied_lanes: 2,
            assignment_evidence_available: true,
            stale_host: false,
        };
        let report = evaluate(SaturationInput {
            observed_at_ms: 1_000,
            window_secs: 3_600,
            queue_depth: 2,
            hosts: vec![
                host("draco", Evidence::available(1.0, "watcher", 1.0)),
                host(
                    "terra",
                    Evidence::unavailable("watcher", "no_lane_with_usable_watcher_coverage", None),
                ),
            ],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 1)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::from([("qsl".to_owned(), "draco:factory:minder".to_owned())]),
            project_lanes: BTreeMap::from([("QSL".to_owned(), 2)]),
        });
        assert!(!report.complete);
        assert_eq!(report.ready_buffer_deficit.value, Some(7));
        assert_eq!(
            report.ready_buffer_deficit.source,
            "fleet.capacity.known_hosts_only"
        );
        assert_eq!(
            report.ready_buffer_deficit.reason.as_deref(),
            Some("partial_deficit_from_hosts_with_usable_supply_and_duty")
        );
        assert_eq!(
            report.projects[0].planner.as_deref(),
            Some("draco:factory:minder")
        );
    }

    #[trace("FR-041-AC-9")]
    #[test]
    fn host_without_current_reachable_lanes_does_not_block_other_hosts() {
        let report = evaluate(SaturationInput {
            observed_at_ms: 1_000,
            window_secs: 3_600,
            queue_depth: 2,
            hosts: vec![
                HostInput {
                    host: "stale".to_owned(),
                    sustainable_slots: Evidence::available(0, "stale_registry_lease", 1.0),
                    lane_capacity: Evidence::available(0, "stale_registry_lease", 1.0),
                    build_slots_busy: Evidence::unavailable("broker", "missing", None),
                    build_slots_configured: Evidence::unavailable("broker", "missing", None),
                    duty: Evidence::unavailable("watcher", "no_current_reachable_lanes", None),
                    active_lanes: 0,
                    supplied_lanes: 0,
                    unsupplied_lanes: 0,
                    assignment_evidence_available: true,
                    stale_host: true,
                },
                HostInput {
                    host: "draco".to_owned(),
                    sustainable_slots: Evidence::available(2, "config.draco.build_slots", 1.0),
                    lane_capacity: Evidence::available(4, "ix_board_config.lane_capacity", 1.0),
                    build_slots_busy: Evidence::unavailable("broker", "missing", None),
                    build_slots_configured: Evidence::unavailable("broker", "missing", None),
                    duty: Evidence::available(1.0, "watcher", 1.0),
                    active_lanes: 1,
                    supplied_lanes: 0,
                    unsupplied_lanes: 1,
                    assignment_evidence_available: true,
                    stale_host: false,
                },
            ],
            project_ready: Some(BTreeMap::from([("QSL".to_owned(), 0)])),
            queued_behind_first: Some(0),
            planners: BTreeMap::from([("qsl".to_owned(), "draco:factory:minder".to_owned())]),
            project_lanes: BTreeMap::from([("QSL".to_owned(), 1)]),
        });
        assert!(report.complete);
        assert_eq!(report.ready_buffer_deficit.value, Some(4));
        assert_eq!(
            report.hosts[0].lanes_needed.reason.as_deref(),
            Some("no_current_reachable_lanes")
        );
        assert_eq!(report.hosts[0].sustainable_slots.value, Some(0));
        assert_eq!(report.hosts[0].lane_capacity.value, Some(0));
    }
}
