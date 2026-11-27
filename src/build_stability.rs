// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Build terminations and queue pressure from retained locked-build status (FR-051).
//!
//! Cumulative Prometheus counters cannot establish a rolling window on their own. The
//! status ledger supplies request timestamps and outcomes; no counter is treated as a rate.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};

use crate::agents::Host;

/// Hosts with stable additive retro metric identities.
pub const HOSTS: [&str; 4] = ["draco", "luna", "titan", "terra"];

/// Recognizes only the additive stability catalog ids for the four owned hosts.
#[implements("FR-051-AC-4")]
pub fn metric_host(id: &str) -> Option<(&str, &str)> {
    let (kind, host) = id.rsplit_once('_')?;
    (HOSTS.contains(&host)
        && matches!(
            kind,
            "build_kill_rate"
                | "build_oom_count"
                | "build_supervision_loss_count"
                | "build_queue_wait_p95_seconds"
        ))
    .then_some((kind, host))
}

impl Window {
    /// Computes a stability catalog value from this window; empty rates stay unavailable.
    #[implements("FR-051-AC-4")]
    pub fn metric_value(&self, kind: &str) -> Option<f64> {
        match kind {
            "build_kill_rate" => self.kill_rate,
            "build_oom_count" => Some(
                self.terminations_by_code
                    .get("LB_OOM_KILLED")
                    .copied()
                    .unwrap_or(0) as f64,
            ),
            "build_supervision_loss_count" => Some(
                self.terminations_by_code
                    .get("LB_SUPERVISION_LOST")
                    .copied()
                    .unwrap_or(0) as f64,
            ),
            "build_queue_wait_p95_seconds" => self
                .queue
                .get("all")
                .and_then(|queue| queue.wait_p95_seconds),
            _ => None,
        }
    }
}

/// Windowed stability and queue evidence, keyed by canonical host.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StabilityReport {
    /// Each expected host, including unavailable sources.
    pub hosts: BTreeMap<String, HostStability>,
}

/// One host's two rolling windows.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostStability {
    /// Safe unavailable reason; missing evidence never becomes a zero.
    pub reason: Option<String>,
    /// Last hour, or null when the retained source cannot establish it.
    pub last_1h: Option<Window>,
    /// Last day, or null when the retained source cannot establish it.
    pub last_24h: Option<Window>,
}

/// Counts and queue analysis over `(start_ms, end_ms]`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// Beginning of the window, Unix milliseconds.
    pub start_ms: u64,
    /// End of the window, Unix milliseconds.
    pub end_ms: u64,
    /// Requests enqueued in the window.
    pub requests: u64,
    /// Requests that reached a terminal state in the window.
    pub completed: u64,
    /// Successful terminal requests divided by completed; null for no completions.
    pub success_rate: Option<f64>,
    /// Abnormal terminal/refused attempts by code, including command failures.
    pub terminations_by_code: BTreeMap<String, u64>,
    /// Kills (excluding command failures/refusals) divided by completed; null if empty.
    pub kill_rate: Option<f64>,
    /// OOM kills plus supervision loss, counted once per request.
    pub oom_supervision_loss_count: u64,
    /// Most frequent violated limit, with available measured and limit evidence.
    pub top_limit_violated: Option<LimitViolation>,
    /// Queue analysis for all requests, and separately for each priority class.
    pub queue: BTreeMap<String, QueueAnalysis>,
}

/// Most frequent violated bound; ties use lexical limit-key order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LimitViolation {
    /// Stable termination code.
    pub code: String,
    /// Number of violations within the window.
    pub count: u64,
    /// Producer's configuration key or derived bound.
    pub limit_key: Option<String>,
    /// Producer's measurement name and units.
    pub measured_what: Option<String>,
    /// Latest available configured limit, in the producer's units.
    pub limit: Option<locked_build::wire::Value>,
    /// Latest available measured value, in the producer's units.
    pub measured: Option<locked_build::wire::Value>,
}

/// Queue and memory evidence for a host or class.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueueAnalysis {
    /// Nearest-rank p50 of completed queue waits, seconds; null for an empty population.
    pub wait_p50_seconds: Option<f64>,
    /// Nearest-rank p95 of completed queue waits, seconds; null for an empty population.
    pub wait_p95_seconds: Option<f64>,
    /// Step series reconstructed from enqueue/start/terminal timestamps, including baseline.
    pub depth_over_time: Vec<DepthPoint>,
    /// Clipped blocked seconds by reason, null if timing evidence is incomplete.
    pub blocked_seconds: Option<BTreeMap<String, f64>>,
    /// Current or final legacy blocker labels; durations remain unknown without history.
    pub observed_blockers: BTreeSet<String>,
    /// Per-request requested/granted memory; absent values remain null on legacy sources.
    pub memory: Vec<MemoryGrant>,
}

/// A queue-depth step, including explicit producer enqueue depth and position when present.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepthPoint {
    /// Unix milliseconds.
    pub at_ms: u64,
    /// Reconstructed number of queued requests.
    pub depth: u64,
    /// Every enqueue at this timestamp, ordered by request id.
    pub enqueues: Vec<EnqueueObservation>,
}

/// Producer queue observations for one enqueue.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnqueueObservation {
    /// Stable request identity.
    pub request_id: String,
    /// Producer total depth at enqueue; absent on legacy sources.
    pub queue_depth_at_enqueue: Option<u64>,
    /// Producer queue rank, when present.
    pub position: Option<u64>,
}

/// Requested and granted memory for an individual request.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryGrant {
    /// Stable producer request id.
    pub request_id: String,
    /// Requested hard-cap bytes, if recorded; never an admission reservation.
    pub requested_memory_bytes: Option<u64>,
    /// Granted hard-cap bytes, if recorded; never an admission reservation.
    pub granted_memory_bytes: Option<u64>,
    /// Requested admission reservation, distinct from the hard cap.
    #[serde(default)]
    pub requested_memory_reservation_bytes: Option<u64>,
    /// Granted admission reservation, distinct from the hard cap.
    #[serde(default)]
    pub granted_memory_reservation_bytes: Option<u64>,
    /// Admission reservation currently needed, if supplied by status.
    #[serde(default)]
    pub memory_needed_bytes: Option<u64>,
    /// Currently unreserved admission budget, if supplied by status.
    #[serde(default)]
    pub memory_free_budget_bytes: Option<u64>,
}

// The producer owns the writer and reader types. Only local derived report types live here.
use locked_build::wire::{
    BlockObservation, Code, RequestProjection as RequestInput, StatusProjection as StatusInput,
};

trait RequestTimes {
    fn phase(&self) -> &str;
    fn queued_at(&self) -> Option<u64>;
    fn admitted_at(&self) -> Option<u64>;
    fn finished_at(&self) -> Option<u64>;
    fn queued_seconds(&self) -> Option<f64>;
    fn blockers(&self) -> Option<&[BlockObservation]>;
}

impl RequestTimes for RequestInput {
    fn phase(&self) -> &str {
        if self.trace.lifecycle_state.is_empty() {
            &self.state
        } else {
            &self.trace.lifecycle_state
        }
    }
    fn queued_at(&self) -> Option<u64> {
        self.trace.queued_at_ms.or(self.enqueued_at_ms)
    }
    fn admitted_at(&self) -> Option<u64> {
        self.trace.admitted_at_ms.or(self.started_at_ms)
    }
    fn finished_at(&self) -> Option<u64> {
        self.trace.finished_at_ms.or(self.ended_at_ms)
    }
    fn queued_seconds(&self) -> Option<f64> {
        (!self.trace.lifecycle_state.is_empty()).then_some(self.trace.queued_seconds)
    }
    fn blockers(&self) -> Option<&[BlockObservation]> {
        (!self.trace.lifecycle_state.is_empty()).then_some(&self.trace.blocked_reasons)
    }
}

fn observed_ms(status: &StatusInput) -> u64 {
    status.observed_at_ms.unwrap_or_default()
}

impl HostStability {
    /// Represents a failed, stale, incompatible or incomplete source explicitly.
    pub fn unavailable(reason: &str) -> Self {
        Self {
            reason: Some(reason.to_owned()),
            last_1h: None,
            last_24h: None,
        }
    }
}

/// Decodes a redacted status document without exposing raw producer content on errors.
#[implements("FR-051-AC-1", "FR-051-AC-2", "FR-051-AC-3", "FR-051-AC-5")]
pub fn from_status(text: &str, expected_host: &str, now_ms: u64) -> HostStability {
    from_status_at(text, expected_host, now_ms, now_ms)
}

#[implements("FR-051-AC-1", "FR-051-AC-2", "FR-051-AC-3", "FR-051-AC-5")]
fn from_status_at(
    text: &str,
    expected_host: &str,
    now_ms: u64,
    freshness_ms: u64,
) -> HostStability {
    if text.len() > crate::agents::MAX_PROTOCOL_BYTES {
        return HostStability::unavailable("status_too_large");
    }
    let Ok(mut status) = serde_json::from_str::<StatusInput>(text) else {
        return HostStability::unavailable("invalid_status");
    };
    if status.schema_version != Some(1) || status.host.to_lowercase() != expected_host {
        return HostStability::unavailable("incompatible_status_or_host");
    }
    if observed_ms(&status) > freshness_ms.saturating_add(5_000)
        || freshness_ms.saturating_sub(observed_ms(&status)) > 120_000
    {
        return HostStability::unavailable("stale_status");
    }
    if status.pruned_events.unwrap_or(0) > 0 {
        return HostStability::unavailable("pruned_history");
    }
    // Retained requests are authoritative. Duplicate identities would inflate every metric.
    let mut ids = BTreeSet::new();
    if status.requests.iter().any(|request| {
        !ids.insert(&request.request_id) || !valid_request(request, observed_ms(&status))
    }) {
        return HostStability::unavailable("invalid_request_history");
    }
    // Events may add timing detail but must never count a retained request twice.
    if !enrich_events(&mut status) {
        return HostStability::unavailable("invalid_event_history");
    }
    HostStability {
        reason: None,
        last_1h: Some(summarize(&status, now_ms.saturating_sub(3_600_000), now_ms)),
        last_24h: Some(summarize(
            &status,
            now_ms.saturating_sub(86_400_000),
            now_ms,
        )),
    }
}

#[implements("FR-051-AC-5")]
fn valid_request(request: &RequestInput, observed_at_ms: u64) -> bool {
    let phase = request.phase();
    let queued = request.queued_at();
    let admitted = request.admitted_at();
    let finished = request.finished_at();
    matches!(
        request.class.as_str(),
        "point" | "block" | "must-fix" | "periodic" | "arch-test" | "kani"
    ) && matches!(
        phase,
        "queued"
            | "admitted"
            | "running"
            | "succeeded"
            | "failed"
            | "timed_out"
            | "cancelled"
            | "refused"
    ) && (queued.is_some() || phase == "refused")
        && queued.is_none_or(|at| at <= observed_at_ms)
        && admitted
            .is_none_or(|at| queued.is_some_and(|queued| at >= queued) && at <= observed_at_ms)
        && request.trace.running_at_ms.is_none_or(|at| {
            admitted.is_some_and(|admitted| at >= admitted) && at <= observed_at_ms
        })
        && finished.is_none_or(|at| {
            at >= request
                .trace
                .running_at_ms
                .or(admitted)
                .or(queued)
                .unwrap_or(0)
                && at <= observed_at_ms
        })
        && (matches!(phase, "queued" | "admitted" | "running") == finished.is_none())
        && [
            request.queued_seconds(),
            Some(request.trace.admitted_seconds),
            Some(request.trace.running_seconds),
        ]
        .into_iter()
        .flatten()
        .all(|seconds| seconds.is_finite() && seconds >= 0.0)
        && request.blockers().is_none_or(valid_blockers)
}

#[implements("FR-051-AC-3", "FR-051-AC-5")]
fn valid_blockers(reasons: &[BlockObservation]) -> bool {
    let mut seen = BTreeSet::new();
    reasons.iter().all(|reason| {
        seen.insert(&reason.reason)
            && BLOCK_REASONS.contains(&reason.reason.as_str())
            && reason.value.is_finite()
            && reason.limit.is_finite()
            && reason.seconds.is_finite()
            && reason.seconds >= 0.0
    })
}

const BLOCK_REASONS: [&str; 7] = [
    "memory_budget",
    "units",
    "load",
    "disk",
    "priority_reservation",
    "memory_reserve",
    "telemetry_unavailable",
];

#[implements("FR-051-AC-2")]
fn termination_code(request: &RequestInput) -> Option<Code> {
    if let Some(code) = request.trace.diagnostic.code {
        return Some(code);
    }
    match request.reason.as_deref() {
        Some("rss_cap") => Some(Code::RssCap),
        Some("oom_killed" | "cgroup_oom_killed") => Some(Code::OomKilled),
        Some("hold_timeout") => Some(Code::HoldTimeout),
        Some("supervisor_lost" | "supervisor_lost_group_exit_unverified" | "host_rebooted") => {
            Some(Code::SupervisionLost)
        }
        Some("memory_cap_unavailable") => Some(Code::RefusedCapUnavailable),
        _ if request.phase() == "timed_out" => Some(Code::HoldTimeout),
        _ if request.phase() == "cancelled" => Some(Code::Cancelled),
        _ if request.signal.is_some_and(|signal| signal > 0) => Some(Code::ExternalSignal),
        _ if request.exit_code.is_some_and(|code| code != 0) => Some(Code::CommandExit),
        _ => None,
    }
}

fn is_kill(code: &str) -> bool {
    matches!(
        code,
        "LB_RSS_CAP"
            | "LB_OOM_KILLED"
            | "LB_HOLD_TIMEOUT"
            | "LB_SUPERVISION_LOST"
            | "LB_CANCELLED"
            | "LB_EXTERNAL_SIGNAL"
    )
}

#[implements("FR-051-AC-1", "FR-051-AC-2")]
fn summarize(status: &StatusInput, start_ms: u64, end_ms: u64) -> Window {
    let requests = &status.requests;
    let contains = |at| at > start_ms && at <= end_ms;
    let completed: Vec<_> = requests
        .iter()
        .filter(|r| r.finished_at().is_some_and(contains))
        .collect();
    let mut terminations_by_code = BTreeMap::new();
    let mut limits: BTreeMap<String, LimitViolation> = BTreeMap::new();
    let mut ordered = completed.clone();
    ordered.sort_by_key(|request| (request.finished_at(), &request.request_id));
    for request in ordered {
        if let Some(code) = termination_code(request) {
            let code = code.as_str();
            *terminations_by_code.entry(code.to_owned()).or_insert(0) += 1;
            if matches!(
                code,
                "LB_RSS_CAP"
                    | "LB_OOM_KILLED"
                    | "LB_HOLD_TIMEOUT"
                    | "LB_REFUSED_MEMORY_BUDGET"
                    | "LB_REFUSED_CLASS_CAP"
                    | "LB_REFUSED_QUEUE"
                    | "LB_REFUSED_CAP_UNAVAILABLE"
            ) {
                let key = request
                    .trace
                    .diagnostic
                    .limit_key
                    .as_deref()
                    .unwrap_or(code)
                    .to_owned();
                let entry = limits.entry(key).or_insert(LimitViolation {
                    code: code.to_owned(),
                    count: 0,
                    limit_key: request.trace.diagnostic.limit_key.clone(),
                    measured_what: None,
                    limit: None,
                    measured: None,
                });
                entry.count += 1;
                entry.code = code.to_owned();
                entry.measured_what = request.trace.diagnostic.measured_what.clone();
                entry.limit = request.trace.diagnostic.limit_value.clone();
                entry.measured = request.trace.diagnostic.measured_value.clone();
            }
        }
    }
    let count = u64::try_from(completed.len()).unwrap_or(u64::MAX);
    let ratio = |value: u64| (count > 0).then(|| value as f64 / count as f64);
    let successes = completed
        .iter()
        .filter(|r| r.phase() == "succeeded" && r.exit_code.is_none_or(|code| code == 0))
        .count();
    let oom_supervision_loss_count = ["LB_OOM_KILLED", "LB_SUPERVISION_LOST"]
        .iter()
        .map(|code| terminations_by_code.get(*code).copied().unwrap_or(0))
        .sum();
    let kills = completed
        .iter()
        .filter(|request| request.phase() != "succeeded")
        .filter_map(|request| termination_code(request))
        .filter(|code| is_kill(code.as_str()))
        .count() as u64;
    let top_limit_violated = limits.into_values().max_by(|left, right| {
        left.count
            .cmp(&right.count)
            .then_with(|| right.limit_key.cmp(&left.limit_key))
    });
    let mut queue = BTreeMap::from([(
        "all".to_owned(),
        queue_analysis(status, None, start_ms, end_ms),
    )]);
    for class in requests
        .iter()
        .map(|request| request.class.as_str())
        .collect::<BTreeSet<_>>()
    {
        queue.insert(
            class.to_owned(),
            queue_analysis(status, Some(class), start_ms, end_ms),
        );
    }
    Window {
        start_ms,
        end_ms,
        requests: requests
            .iter()
            .filter(|r| {
                r.queued_at().is_some_and(contains)
                    || (r.phase() == "refused" && r.finished_at().is_some_and(contains))
            })
            .count() as u64,
        completed: count,
        success_rate: ratio(successes as u64),
        terminations_by_code,
        kill_rate: ratio(kills),
        oom_supervision_loss_count,
        top_limit_violated,
        queue,
    }
}

#[implements("FR-051-AC-3")]
fn queue_analysis(
    status: &StatusInput,
    class: Option<&str>,
    start_ms: u64,
    end_ms: u64,
) -> QueueAnalysis {
    let requests: Vec<_> = status
        .requests
        .iter()
        .filter(|r| class.is_none_or(|class| r.class == class))
        .collect();
    let mut waits: Vec<_> = requests
        .iter()
        .filter(|r| {
            r.admitted_at()
                .is_some_and(|at| at > start_ms && at <= end_ms)
        })
        .filter_map(|r| {
            r.queued_seconds()
                .or_else(|| r.wait_ms.map(|ms| ms as f64 / 1000.0))
                .or_else(|| {
                    r.admitted_at()
                        .zip(r.queued_at())
                        .map(|(admitted, queued)| admitted.saturating_sub(queued) as f64 / 1000.0)
                })
        })
        .collect();
    waits.sort_by(f64::total_cmp);
    let percentile = |percent: usize| {
        let rank = waits.len().saturating_mul(percent).div_ceil(100);
        rank.checked_sub(1)
            .and_then(|index| waits.get(index))
            .copied()
    };
    let mut transitions: BTreeMap<u64, i64> = BTreeMap::from([(start_ms, 0), (end_ms, 0)]);
    let mut enqueues = BTreeMap::new();
    let mut memory = Vec::new();
    let mut observed_blockers = BTreeSet::new();
    let mut blocked = BLOCK_REASONS
        .into_iter()
        .map(|reason| (reason.to_owned(), 0.0))
        .collect::<BTreeMap<_, _>>();
    let mut blocked_complete = true;
    for request in requests {
        if (request.queued_at().is_some_and(|at| at <= end_ms)
            || (request.phase() == "refused"
                && request.finished_at().is_some_and(|at| at <= end_ms)))
            && request.finished_at().unwrap_or(end_ms) > start_ms
        {
            memory.push(MemoryGrant {
                request_id: request.request_id.clone(),
                requested_memory_bytes: request.trace.requested_memory_bytes,
                requested_memory_reservation_bytes: request
                    .trace
                    .requested_memory_reservation_bytes,
                granted_memory_reservation_bytes: request
                    .admitted_at()
                    .filter(|at| *at <= end_ms)
                    .and(request.trace.granted_memory_reservation_bytes),
                memory_needed_bytes: request.memory_needed_bytes,
                memory_free_budget_bytes: request.memory_free_budget_bytes,
                granted_memory_bytes: request
                    .admitted_at()
                    .filter(|at| *at <= end_ms)
                    .and(request.trace.granted_memory_bytes),
            });
        }
        let Some(queued_at) = request.queued_at() else {
            continue;
        };
        let left = queued_at.max(start_ms);
        let right = request
            .admitted_at()
            .or(request.finished_at())
            .unwrap_or(end_ms)
            .min(end_ms);
        if queued_at <= end_ms
            && (right > left
                || (queued_at == end_ms
                    && request.admitted_at().or(request.finished_at()).is_none()))
        {
            *transitions.entry(left).or_default() += 1;
            if request
                .admitted_at()
                .or(request.finished_at())
                .is_some_and(|at| at <= end_ms)
            {
                *transitions.entry(right).or_default() -= 1;
            }
            if queued_at >= start_ms {
                enqueues
                    .entry(left)
                    .or_insert_with(Vec::new)
                    .push(EnqueueObservation {
                        request_id: request.request_id.clone(),
                        queue_depth_at_enqueue: request
                            .queued_seconds()
                            .map(|_| u64::from(request.trace.queue_depth_at_enqueue)),
                        position: request.trace.position.map(u64::from),
                    });
            }
            if let Some(reason) = request.reason.as_deref().and_then(legacy_blocker) {
                observed_blockers.insert(reason.to_owned());
            }
            match blocked_in_window(status, request, start_ms, end_ms) {
                Some(seconds) => {
                    for (reason, seconds) in seconds {
                        *blocked.entry(reason).or_default() += seconds;
                    }
                }
                None => blocked_complete = false,
            }
        }
    }
    memory.sort_by(|left, right| left.request_id.cmp(&right.request_id));
    let mut depth = 0_i64;
    let depth_over_time = transitions
        .into_iter()
        .map(|(at_ms, change)| {
            depth += change;
            let mut observations = enqueues.remove(&at_ms).unwrap_or_default();
            observations.sort_by(|left, right| left.request_id.cmp(&right.request_id));
            DepthPoint {
                at_ms,
                depth: u64::try_from(depth).unwrap_or(0),
                enqueues: observations,
            }
        })
        .collect();
    QueueAnalysis {
        wait_p50_seconds: percentile(50),
        wait_p95_seconds: percentile(95),
        depth_over_time,
        blocked_seconds: blocked_complete.then_some(blocked),
        observed_blockers,
        memory,
    }
}

#[implements("FR-051-AC-3")]
fn legacy_blocker(reason: &str) -> Option<&str> {
    match reason {
        "memory_budget"
        | "units"
        | "load"
        | "disk"
        | "priority_reservation"
        | "memory_reserve"
        | "telemetry_unavailable" => Some(reason),
        "capacity" | "capacity_occupied" | "capacity_shrunk" => Some("units"),
        "high_load" | "host_load" => Some("load"),
        "fifo_or_priority_reservation" => Some("priority_reservation"),
        "host_sample_unavailable" | "host_sample_invalid_or_stale" => Some("telemetry_unavailable"),
        "disk_reserve" => Some("disk"),
        _ => None,
    }
}

#[implements("FR-051-AC-3")]
fn blocked_in_window(
    status: &StatusInput,
    request: &RequestInput,
    start_ms: u64,
    end_ms: u64,
) -> Option<BTreeMap<String, f64>> {
    let observations = request.blockers()?;
    let endpoint = request
        .admitted_at()
        .or(request.finished_at())
        .unwrap_or(observed_ms(status));
    if endpoint > end_ms {
        return None;
    }
    let baseline = if request.queued_at()? >= start_ms {
        BTreeMap::new()
    } else {
        let event = status.events.iter().find(|event| {
            event.request.request_id == request.request_id && event.observed_at_ms == start_ms
        })?;
        let baseline = event.request.blockers()?;
        if !valid_blockers(baseline) {
            return None;
        }
        baseline
            .iter()
            .map(|observation| (observation.reason.clone(), observation.seconds))
            .collect()
    };
    if baseline.keys().any(|reason| {
        !observations
            .iter()
            .any(|observation| &observation.reason == reason)
    }) {
        return None;
    }
    let mut seconds = BTreeMap::new();
    for observation in observations {
        let previous = baseline.get(&observation.reason).copied().unwrap_or(0.0);
        if observation.seconds < previous {
            return None;
        }
        seconds.insert(observation.reason.clone(), observation.seconds - previous);
    }
    Some(seconds)
}

#[implements("FR-051-AC-1", "FR-051-AC-2")]
fn enrich_events(status: &mut StatusInput) -> bool {
    // Refusals can exist only in events. Latest snapshot per attempt wins (producer
    // event order breaks timestamp ties); corrections, transitions,
    // blocker snapshots and command-start events never increment counts independently.
    let mut missing = BTreeMap::new();
    let retained: BTreeSet<_> = status
        .requests
        .iter()
        .map(|request| request.request_id.as_str())
        .collect();
    for event in &status.events {
        let id = event.request.request_id.as_str();
        if retained.contains(id) {
            continue;
        }
        if missing
            .get(id)
            .is_none_or(|(previous, _)| event.observed_at_ms >= *previous)
        {
            missing.insert(id, (event.observed_at_ms, &event.request));
        }
    }
    for (_, (_, request)) in missing {
        if !valid_request(request, observed_ms(status)) {
            return false;
        }
        status.requests.push(request.clone());
    }
    true
}

/// Reads each reachable machine once in parallel and retains explicit missing hosts.
#[implements("FR-051-AC-5")]
pub fn collect(hosts: &[Arc<dyn Host>], now_ms: u64) -> StabilityReport {
    collect_at(hosts, now_ms, now_ms)
}

#[implements("FR-051-AC-5")]
fn collect_at(hosts: &[Arc<dyn Host>], window_end_ms: u64, freshness_ms: u64) -> StabilityReport {
    let collection_started = Instant::now();
    let mut report = StabilityReport {
        hosts: HOSTS
            .into_iter()
            .map(|host| {
                (
                    host.to_owned(),
                    HostStability::unavailable("host_unavailable"),
                )
            })
            .collect(),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    for host in hosts {
        let host = Arc::clone(host);
        let sender = sender.clone();
        std::thread::spawn(move || {
            let result = host.build_status();
            let _ = sender.send((host.label().to_lowercase(), result));
        });
    }
    drop(sender);
    let deadline = collection_started + Duration::from_secs(45);
    let mut seen = BTreeSet::new();
    while let Ok((label, result)) =
        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        let Ok(text) = result else {
            continue;
        };
        let host = if label == "local" {
            serde_json::from_str::<StatusInput>(&text)
                .ok()
                .map(|status| status.host.to_lowercase())
        } else {
            Some(label)
        };
        if let Some(host) = host.filter(|host| HOSTS.contains(&host.as_str())) {
            let row = if seen.insert(host.clone()) {
                from_status_at(
                    &text,
                    &host,
                    window_end_ms,
                    freshness_ms.saturating_add(
                        u64::try_from(collection_started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    ),
                )
            } else {
                HostStability::unavailable("duplicate_host_source")
            };
            report.hosts.insert(host, row);
        }
    }
    report
}

/// Collects the configured Herdr host inventory; discovery failure still reports all hosts.
#[implements("FR-051-AC-5")]
pub fn live(now_ms: u64) -> StabilityReport {
    let hosts = crate::agents::system_hosts()
        .unwrap_or_else(|_| vec![Arc::new(crate::agents::SystemHost::local())]);
    // Freshness is observed after discovery and preceding retro source reads. The window
    // retains the caller's as-of time, even if gathering evidence takes several seconds.
    let freshness_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0);
    collect_at(&hosts, now_ms, freshness_ms)
}

/// Renders rolling host stability and queue evidence without raw producer strings.
#[implements("FR-051-AC-1", "FR-051-AC-3")]
pub fn render_text(report: &StabilityReport) -> String {
    let mut lines = Vec::new();
    for (host, row) in &report.hosts {
        if let Some(reason) = &row.reason {
            lines.push(format!("stability {host} unavailable={reason}"));
        }
        for (label, window) in [("1h", row.last_1h.as_ref()), ("24h", row.last_24h.as_ref())] {
            let Some(window) = window else {
                continue;
            };
            lines.push(format!("stability {host} {label} requests={} completed={} success_rate={:?} terminations={:?} top_limit={:?}", window.requests, window.completed, window.success_rate, window.terminations_by_code, window.top_limit_violated));
            for (class, queue) in &window.queue {
                lines.push(format!("queue {host} {label} class={class} wait_p50_seconds={:?} wait_p95_seconds={:?} depth={:?} blocked_seconds={:?} observed_blockers={:?} memory={:?}", queue.wait_p50_seconds, queue.wait_p95_seconds, queue.depth_over_time, queue.blocked_seconds, queue.observed_blockers, queue.memory));
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;
    use serde_json::json;

    const NOW: u64 = 100_000_000;

    fn status(requests: serde_json::Value, events: serde_json::Value) -> serde_json::Value {
        json!({"schema_version":1,"host":"draco","observed_at_ms":NOW,"requests":requests,"events":events})
    }

    #[test]
    #[trace("FR-051-AC-1", "FR-051-AC-2")]
    fn stability_windows_count_attempts_not_event_snapshots_and_keep_limit_evidence() {
        let source = status(
            json!([
                {"request_id":"ok","state":"succeeded","class":"point","enqueued_at_ms":99_000_000,"started_at_ms":99_001_000,"ended_at_ms":99_002_000,"exit_code":0},
                {"request_id":"rss","state":"failed","class":"block","enqueued_at_ms":99_000_000,"started_at_ms":99_002_000,"ended_at_ms":99_003_000,"code":"LB_RSS_CAP","limit_key":"policy.memory_per_job_bytes*parallelism","limit_value":100,"measured_value":125,"measured_what":"rss bytes"},
                {"request_id":"oom","state":"failed","class":"block","enqueued_at_ms":99_000_000,"started_at_ms":99_003_000,"ended_at_ms":99_004_000,"code":"LB_OOM_KILLED","reason":"rss_cap","signal":15,"limit_key":"policy.scope"},
                {"request_id":"old","state":"failed","class":"block","enqueued_at_ms":90_000_000,"started_at_ms":90_001_000,"ended_at_ms":96_400_000,"reason":"supervisor_lost"},
                {"request_id":"signal","state":"failed","class":"point","enqueued_at_ms":99_000_000,"started_at_ms":99_001_000,"ended_at_ms":100_000_000,"signal":9},
                {"request_id":"exit","state":"failed","class":"point","enqueued_at_ms":99_000_000,"started_at_ms":99_001_000,"ended_at_ms":100_000_000,"exit_code":101}
            ]),
            json!([
                {"request_id":"rss","observed_at_ms":99_003_000,"kind":"transition"},
                {"request_id":"rss","observed_at_ms":99_003_000,"kind":"blocked"},
                {"request_id":"refused","observed_at_ms":99_999_000,"lifecycle_state":"refused","class":"point","finished_at_ms":99_999_000,"code":"LB_REFUSED_QUEUE","limit_key":"policy.queue"},
                {"request_id":"refused","observed_at_ms":99_999_001,"lifecycle_state":"refused","class":"point","finished_at_ms":99_999_000,"code":"LB_REFUSED_QUEUE","limit_key":"policy.queue"}
            ]),
        );
        let report = from_status(&source.to_string(), "draco", NOW);
        assert_eq!(report.reason, None);
        let hour = report.last_1h.unwrap();
        assert_eq!(hour.requests, 6);
        assert_eq!(hour.completed, 6);
        assert_eq!(hour.success_rate, Some(1.0 / 6.0));
        assert_eq!(hour.kill_rate, Some(0.5));
        assert_eq!(
            hour.terminations_by_code,
            BTreeMap::from([
                ("LB_RSS_CAP".into(), 1),
                ("LB_OOM_KILLED".into(), 1),
                ("LB_EXTERNAL_SIGNAL".into(), 1),
                ("LB_COMMAND_EXIT".into(), 1),
                ("LB_REFUSED_QUEUE".into(), 1)
            ])
        );
        assert_eq!(hour.oom_supervision_loss_count, 1);
        let limit = hour.top_limit_violated.unwrap();
        // A named configured resource bound wins the lexical tie with an unnamed refusal.
        assert_eq!(limit.code, "LB_RSS_CAP");
        assert_eq!(limit.limit, Some(locked_build::wire::Value::Integer(100)));
        assert_eq!(
            limit.measured,
            Some(locked_build::wire::Value::Integer(125))
        );
        let day = report.last_24h.unwrap();
        assert_eq!(day.completed, 7);
        assert_eq!(day.oom_supervision_loss_count, 2);
        assert_eq!(day.kill_rate, Some(4.0 / 7.0));
    }

    #[test]
    #[trace("FR-051-AC-2")]
    fn stability_legacy_outcomes_do_not_infer_oom_from_sigkill_or_exit_137() {
        for (state, reason, signal, exit_code, expected) in [
            ("failed", Some("rss_cap"), None, None, Some("LB_RSS_CAP")),
            ("timed_out", None, None, None, Some("LB_HOLD_TIMEOUT")),
            (
                "failed",
                Some("supervisor_lost_group_exit_unverified"),
                None,
                None,
                Some("LB_SUPERVISION_LOST"),
            ),
            (
                "failed",
                Some("memory_cap_unavailable"),
                None,
                None,
                Some("LB_REFUSED_CAP_UNAVAILABLE"),
            ),
            ("cancelled", None, None, None, Some("LB_CANCELLED")),
            ("failed", None, Some(9), None, Some("LB_EXTERNAL_SIGNAL")),
            ("failed", None, None, Some(137), Some("LB_COMMAND_EXIT")),
            ("succeeded", None, None, Some(0), None),
        ] {
            let source = status(
                json!([{"request_id":"one","state":state,"class":"block","enqueued_at_ms":99_000_000,"ended_at_ms":99_010_000,"reason":reason,"signal":signal,"exit_code":exit_code}]),
                json!([]),
            );
            let report = from_status(&source.to_string(), "draco", NOW)
                .last_1h
                .unwrap();
            assert_eq!(
                report
                    .terminations_by_code
                    .keys()
                    .next()
                    .map(String::as_str),
                expected,
                "{reason:?} {signal:?} {exit_code:?}"
            );
        }
    }

    #[test]
    #[trace("FR-051-AC-3")]
    fn stability_queue_percentiles_depth_blockers_and_memory_use_frozen_contract() {
        let source = status(
            json!([
                {"request_id":"a","state":"succeeded","lifecycle_state":"succeeded","class":"block","queued_at_ms":99_000_000,"admitted_at_ms":99_010_000,"running_at_ms":99_012_000,"finished_at_ms":99_030_000,"queued_seconds":10.0,"admitted_seconds":2.0,"running_seconds":18.0,"queue_depth_at_enqueue":1,"position":null,"requested_memory_bytes":500,"granted_memory_bytes":400,"blocked_reasons":[{"reason":"memory_budget","value":600,"limit":400,"seconds":7.5},{"reason":"load","value":20,"limit":16,"seconds":5.0}]},
                {"request_id":"b","state":"running","lifecycle_state":"running","class":"block","queued_at_ms":99_005_000,"admitted_at_ms":99_025_000,"running_at_ms":99_029_000,"queued_seconds":20.0,"queue_depth_at_enqueue":2,"blocked_reasons":[{"reason":"units","value":8,"limit":4,"seconds":12.0}]},
                {"request_id":"c","state":"queued","lifecycle_state":"queued","class":"point","queued_at_ms":99_020_000,"queued_seconds":980.0,"queue_depth_at_enqueue":2,"position":1,"blocked_reasons":[{"reason":"disk","value":5,"limit":100,"seconds":980.0}]}
            ]),
            json!([]),
        );
        let hour = from_status(&source.to_string(), "draco", NOW)
            .last_1h
            .unwrap();
        let all = &hour.queue["all"];
        assert_eq!(all.wait_p50_seconds, Some(10.0));
        assert_eq!(all.wait_p95_seconds, Some(20.0));
        assert_eq!(hour.queue["block"].wait_p50_seconds, Some(10.0));
        assert_eq!(hour.queue["point"].wait_p95_seconds, None);
        assert_eq!(all.blocked_seconds.as_ref().unwrap()["memory_budget"], 7.5);
        assert_eq!(all.blocked_seconds.as_ref().unwrap()["units"], 12.0);
        assert_eq!(all.blocked_seconds.as_ref().unwrap()["load"], 5.0);
        assert_eq!(all.blocked_seconds.as_ref().unwrap()["disk"], 980.0);
        assert_eq!(all.memory[0].requested_memory_bytes, Some(500));
        assert_eq!(all.memory[0].granted_memory_bytes, Some(400));
        assert_eq!(all.memory[1].requested_memory_bytes, None);
        assert_eq!(
            all.depth_over_time
                .iter()
                .map(|p| (p.at_ms, p.depth))
                .collect::<Vec<_>>(),
            vec![
                (96_400_000, 0),
                (99_000_000, 1),
                (99_005_000, 2),
                (99_010_000, 1),
                (99_020_000, 2),
                (99_025_000, 1),
                (100_000_000, 1)
            ]
        );
        assert_eq!(
            all.depth_over_time[2].enqueues[0].queue_depth_at_enqueue,
            Some(2)
        );
        assert_eq!(all.depth_over_time[4].enqueues[0].position, Some(1));
    }

    #[test]
    #[trace("FR-051-AC-3")]
    fn stability_queue_window_baseline_cancellation_and_unknown_legacy_duration() {
        let source = status(
            json!([
                {"request_id":"old","state":"cancelled","class":"point","enqueued_at_ms":96_000_000,"ended_at_ms":96_410_000,"wait_ms":410_000,"reason":"units"},
                {"request_id":"fresh","state":"running","class":"block","enqueued_at_ms":99_000_000,"started_at_ms":99_003_000,"wait_ms":4000}
            ]),
            json!([]),
        );
        let hour = from_status(&source.to_string(), "draco", NOW)
            .last_1h
            .unwrap();
        let all = &hour.queue["all"];
        assert_eq!(all.wait_p95_seconds, Some(4.0));
        assert_eq!(all.depth_over_time[0].depth, 1);
        assert_eq!(all.depth_over_time[1].at_ms, 96_410_000);
        assert_eq!(all.depth_over_time[1].depth, 0);
        assert_eq!(all.blocked_seconds, None);
        assert!(all.observed_blockers.contains("units"));
        assert_eq!(all.memory[0].requested_memory_bytes, None);
    }

    #[test]
    #[trace("FR-051-AC-5")]
    fn stability_unavailable_sources_never_become_zeros_or_echo_payloads() {
        let source = status(json!([]), json!([]));
        for (field, value, reason) in [
            ("schema_version", json!(2), "incompatible_status_or_host"),
            ("host", json!("terra"), "incompatible_status_or_host"),
            ("observed_at_ms", json!(99_879_999), "stale_status"),
            ("observed_at_ms", json!(100_005_001), "stale_status"),
            ("pruned_events", json!(1), "pruned_history"),
            (
                "requests",
                json!([{"request_id":"bad","state":"running","class":"point","enqueued_at_ms":100_000_001}]),
                "invalid_request_history",
            ),
        ] {
            let mut input = source.clone();
            input[field] = value;
            let report = from_status(&input.to_string(), "draco", NOW);
            assert_eq!(report.reason.as_deref(), Some(reason));
            assert_eq!(report.last_1h, None);
            assert_eq!(report.last_24h, None);
        }
        let truncated = from_status(
            r#"{"schema_version":1,"host":"draco","observed_at_ms":100000000,"events":[]}"#,
            "draco",
            NOW,
        );
        assert_eq!(truncated.reason.as_deref(), Some("invalid_status"));
        let bad = from_status("secret credential command", "draco", NOW);
        assert_eq!(
            serde_json::to_string(&bad).unwrap(),
            r#"{"reason":"invalid_status","last_1h":null,"last_24h":null}"#
        );
        let empty = from_status(&source.to_string(), "draco", NOW)
            .last_1h
            .unwrap();
        assert_eq!(empty.requests, 0);
        assert_eq!(empty.kill_rate, None);
        assert_eq!(empty.success_rate, None);
    }

    #[test]
    #[trace("FR-051-AC-5")]
    fn stability_missing_inventory_still_names_every_host() {
        let report = collect(&[], NOW);
        assert_eq!(report.hosts.len(), 4);
        for host in HOSTS {
            assert_eq!(
                report.hosts[host].reason.as_deref(),
                Some("host_unavailable")
            );
        }
        assert!(render_text(&report).contains("stability terra unavailable=host_unavailable"));
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;
    use crate::agents::{AgentsError, Probe, ProbeRequest};
    use ix_trace_rs::trace;

    struct FixtureHost {
        label: String,
        status: String,
    }
    impl Host for FixtureHost {
        fn label(&self) -> &str {
            &self.label
        }
        fn build_status(&self) -> Result<String, AgentsError> {
            Ok(self.status.clone())
        }
        fn herdr(&self, _args: &[&str]) -> Result<String, AgentsError> {
            Err(AgentsError::ProbeFormat("unused".into()))
        }
        fn probe(&self, _request: &ProbeRequest) -> Result<Probe, AgentsError> {
            Err(AgentsError::ProbeFormat("unused".into()))
        }
    }

    #[test]
    #[trace("FR-051-AC-5")]
    fn stability_collects_every_injected_host_and_refuses_mismatched_identity() {
        let hosts=HOSTS.iter().map(|host| Arc::new(FixtureHost { label: if *host=="draco" {"local".into()} else {(*host).into()}, status: format!(r#"{{"schema_version":1,"host":"{host}","observed_at_ms":100000000,"requests":[],"events":[]}}"#) }) as Arc<dyn Host>).collect::<Vec<_>>();
        let report = collect(&hosts, 100_000_000);
        assert_eq!(report.hosts.len(), 4);
        assert!(report.hosts.values().all(|row| row.reason.is_none()));
        let mismatch: Arc<dyn Host>=Arc::new(FixtureHost {label:"terra".into(),status:r#"{"schema_version":1,"host":"luna","observed_at_ms":100000000,"requests":[],"events":[]}"#.into()});
        let report = collect(&[mismatch], 100_000_000);
        assert_eq!(
            report.hosts["terra"].reason.as_deref(),
            Some("incompatible_status_or_host")
        );
        assert!(report.hosts["luna"].last_1h.is_none());
    }

    #[test]
    #[trace("FR-051-AC-3")]
    fn stability_blocked_seconds_subtract_exact_window_boundary_observation() {
        let input = r#"{"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[{"request_id":"old","class":"point","lifecycle_state":"running","queued_at_ms":96000000,"admitted_at_ms":96410000,"queued_seconds":410,"blocked_reasons":[{"reason":"memory_budget","value":100,"limit":90,"seconds":15.0},{"reason":"load","value":20,"limit":16,"seconds":10.0}]}],"events":[{"event_id":"baseline","observed_at_ms":96400000,"request_id":"old","lifecycle_state":"queued","blocked_reasons":[{"reason":"memory_budget","value":100,"limit":90,"seconds":10.0},{"reason":"load","value":20,"limit":16,"seconds":8.0}]}]}"#;
        let report = from_status(input, "draco", 100_000_000);
        let hour = report.last_1h.unwrap();
        assert_eq!(
            hour.queue["all"].blocked_seconds.as_ref().unwrap()["memory_budget"],
            5.0
        );
        assert_eq!(
            hour.queue["all"].blocked_seconds.as_ref().unwrap()["load"],
            2.0
        );
        assert_eq!(hour.queue["all"].depth_over_time[0].depth, 1);
    }
}

#[cfg(test)]
mod review_regressions {
    use super::*;
    use ix_trace_rs::trace;
    use serde_json::json;

    #[test]
    #[trace("FR-051-AC-1", "FR-051-AC-5")]
    fn stability_freshness_after_slow_reads_preserves_requested_window_end() {
        let input = r#"{"schema_version":1,"host":"draco","observed_at_ms":100010000,"requests":[{"request_id":"before","class":"point","state":"succeeded","enqueued_at_ms":99990000,"ended_at_ms":99999000},{"request_id":"after","class":"point","state":"succeeded","enqueued_at_ms":100001000,"ended_at_ms":100009000}],"events":[]}"#;
        let report = from_status_at(input, "draco", 100_000_000, 100_010_000);
        assert_eq!(report.reason, None);
        let hour = report.last_1h.unwrap();
        assert_eq!(hour.end_ms, 100_000_000);
        assert_eq!(hour.completed, 1);
        assert_eq!(hour.requests, 1);
        assert_eq!(
            from_status_at(input, "draco", 100_000_000, 100_200_000)
                .reason
                .as_deref(),
            Some("stale_status")
        );
    }

    #[test]
    #[trace("FR-051-AC-3", "FR-051-AC-5")]
    fn stability_invalid_event_blocker_baselines_cannot_inflate_seconds() {
        let source = json!({"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[{"request_id":"old","class":"point","lifecycle_state":"running","queued_at_ms":96000000,"admitted_at_ms":96410000,"queued_seconds":410,"blocked_reasons":[{"reason":"memory_budget","value":100,"limit":90,"seconds":15.0}]}],"events":[{"event_id":"baseline","observed_at_ms":96400000,"request_id":"old","lifecycle_state":"queued","blocked_reasons":[]}]});
        for baseline in [
            json!([{"reason":"memory_budget","value":100,"limit":90,"seconds":-10.0}]),
            json!([{"reason":"unknown","value":100,"limit":90,"seconds":10.0}]),
            json!([{"reason":"units","value":8,"limit":4,"seconds":10.0}]),
            json!([{"reason":"memory_budget","value":100,"limit":90,"seconds":10.0},{"reason":"memory_budget","value":100,"limit":90,"seconds":11.0}]),
            json!([{"reason":"memory_budget","value":100,"limit":90,"seconds":16.0}]),
        ] {
            let mut input = source.clone();
            input["events"][0]["blocked_reasons"] = baseline;
            let report = from_status(&input.to_string(), "draco", 100_000_000);
            assert_eq!(report.last_1h.unwrap().queue["all"].blocked_seconds, None);
        }
        let mut input = source;
        input["events"][0]["blocked_reasons"] =
            json!([{"reason":"memory_budget","value":100,"limit":90,"seconds":10.0}]);
        let report = from_status(&input.to_string(), "draco", 100_000_000);
        assert_eq!(
            report.last_1h.unwrap().queue["all"]
                .blocked_seconds
                .as_ref()
                .unwrap()["memory_budget"],
            5.0
        );
    }

    #[test]
    #[trace("FR-051-AC-1", "FR-051-AC-3")]
    fn stability_refused_attempt_retains_requested_memory_without_queue_entry() {
        let input = r#"{"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[],"events":[{"event_id":"refused","observed_at_ms":99999000,"request_id":"memory-refusal","class":"block","lifecycle_state":"refused","queued_at_ms":null,"finished_at_ms":99999000,"requested_memory_bytes":500,"granted_memory_bytes":null,"code":"LB_REFUSED_MEMORY_BUDGET"}]}"#;
        let report = from_status(input, "draco", 100_000_000).last_1h.unwrap();
        assert_eq!(report.requests, 1);
        for class in ["all", "block"] {
            assert_eq!(
                report.queue[class].memory,
                vec![MemoryGrant {
                    request_id: "memory-refusal".into(),
                    requested_memory_bytes: Some(500),
                    requested_memory_reservation_bytes: None,
                    granted_memory_reservation_bytes: None,
                    granted_memory_bytes: None,
                    memory_needed_bytes: None,
                    memory_free_budget_bytes: None
                }]
            );
            assert!(
                report.queue[class]
                    .depth_over_time
                    .iter()
                    .all(|point| point.depth == 0)
            );
            assert_eq!(report.queue[class].wait_p95_seconds, None);
        }
    }
}

#[cfg(test)]
mod contract_clarification_tests {
    use super::*;
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-051-AC-1", "FR-051-AC-2", "FR-051-AC-4")]
    fn stability_successful_degraded_supervision_is_success_with_a_diagnostic() {
        let input = r#"{"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[{"request_id":"degraded","class":"point","state":"succeeded","lifecycle_state":"succeeded","queued_at_ms":99990000,"admitted_at_ms":99991000,"running_at_ms":99992000,"finished_at_ms":99999000,"exit_code":0,"code":"LB_SUPERVISION_LOST","message":"degraded supervision"}],"events":[]}"#;
        let hour = from_status(input, "draco", 100_000_000).last_1h.unwrap();
        assert_eq!(hour.completed, 1);
        assert_eq!(hour.success_rate, Some(1.0));
        assert_eq!(hour.kill_rate, Some(0.0));
        assert_eq!(hour.terminations_by_code["LB_SUPERVISION_LOST"], 1);
        assert_eq!(hour.metric_value("build_supervision_loss_count"), Some(1.0));
    }
}

#[cfg(test)]
mod status_memory_tests {
    use super::*;
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-051-AC-3", "FR-051-AC-5")]
    fn stability_status_memory_needed_and_free_budget_are_distinct_and_optional() {
        let input = r#"{"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[{"request_id":"waiting","class":"block","state":"queued","enqueued_at_ms":99990000,"requested_memory_bytes":500,"granted_memory_bytes":null,"memory_needed_bytes":600,"memory_free_budget_bytes":300}],"events":[]}"#;
        let report = from_status(input, "draco", 100_000_000);
        let hour = report.last_1h.as_ref().unwrap();
        for class in ["all", "block"] {
            assert_eq!(
                hour.queue[class].memory,
                vec![MemoryGrant {
                    request_id: "waiting".into(),
                    requested_memory_bytes: Some(500),
                    requested_memory_reservation_bytes: None,
                    granted_memory_reservation_bytes: None,
                    granted_memory_bytes: None,
                    memory_needed_bytes: Some(600),
                    memory_free_budget_bytes: Some(300),
                }]
            );
        }
        let text = render_text(&StabilityReport {
            hosts: [("draco".to_owned(), report)].into(),
        });
        assert!(text.contains("memory_needed_bytes: Some(600)"));
        assert!(text.contains("memory_free_budget_bytes: Some(300)"));
        let legacy = serde_json::from_str::<MemoryGrant>(
            r#"{"request_id":"old","requested_memory_bytes":500,"granted_memory_bytes":400}"#,
        )
        .unwrap();
        assert_eq!(legacy.requested_memory_bytes, Some(500));
        assert_eq!(legacy.granted_memory_bytes, Some(400));
        assert_eq!(legacy.requested_memory_reservation_bytes, None);
        assert_eq!(legacy.granted_memory_reservation_bytes, None);
        assert_eq!(legacy.memory_needed_bytes, None);
        assert_eq!(legacy.memory_free_budget_bytes, None);
    }
}

#[cfg(test)]
mod reservation_contract_tests {
    use super::*;

    #[test]
    fn stability_admission_reservation_is_distinct_from_hard_cap() {
        let input = r#"{"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[{"request_id":"waiting","class":"block","state":"queued","lifecycle_state":"queued","queued_at_ms":99990000,"requested_memory_bytes":4096,"requested_memory_reservation_bytes":512,"memory_needed_bytes":512,"memory_free_budget_bytes":256},{"request_id":"admitted","class":"block","state":"running","lifecycle_state":"running","queued_at_ms":99990000,"admitted_at_ms":99991000,"requested_memory_bytes":8192,"granted_memory_bytes":8192,"requested_memory_reservation_bytes":1024,"granted_memory_reservation_bytes":1024,"memory_needed_bytes":1024,"memory_free_budget_bytes":256}],"events":[]}"#;
        let report = from_status(input, "draco", 100_000_000);
        let hour = report.last_1h.as_ref().unwrap();
        for class in ["all", "block"] {
            let memory = &hour.queue[class].memory;
            assert_eq!(memory.len(), 2);
            assert_eq!(memory[0].request_id, "admitted");
            assert_eq!(memory[0].requested_memory_bytes, Some(8192));
            assert_eq!(memory[0].granted_memory_bytes, Some(8192));
            assert_eq!(memory[0].requested_memory_reservation_bytes, Some(1024));
            assert_eq!(memory[0].granted_memory_reservation_bytes, Some(1024));
            assert_eq!(memory[1].requested_memory_bytes, Some(4096));
            assert_eq!(memory[1].granted_memory_bytes, None);
            assert_eq!(memory[1].requested_memory_reservation_bytes, Some(512));
            assert_eq!(memory[1].granted_memory_reservation_bytes, None);
            assert_eq!(memory[1].memory_needed_bytes, Some(512));
            assert_eq!(memory[1].memory_free_budget_bytes, Some(256));
        }
        let text = render_text(&StabilityReport {
            hosts: [("draco".into(), report)].into(),
        });
        assert!(text.contains("requested_memory_reservation_bytes: Some(1024)"));
        assert!(text.contains("granted_memory_bytes: Some(8192)"));
    }
}

#[cfg(test)]
mod correction_contract_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stability_correction_of_a_historical_outcome_counts_one_attempt() {
        let mut source = json!({"schema_version":1,"host":"draco","observed_at_ms":100000000,"requests":[],"events":[
            {"event_id":"original","observed_at_ms":99995000,"request_id":"one","class":"point","state":"failed","lifecycle_state":"failed","queued_at_ms":99990000,"admitted_at_ms":99991000,"finished_at_ms":99995000,"code":"LB_COMMAND_EXIT","exit_code":101},
            {"event_id":"repair","correction":true,"observed_at_ms":99999000,"request_id":"one","class":"point","state":"succeeded","lifecycle_state":"succeeded","queued_at_ms":99990000,"admitted_at_ms":99991000,"finished_at_ms":99999000,"code":"LB_SUPERVISION_LOST","exit_code":0}
        ]});
        let decoded = serde_json::from_value::<StatusInput>(source.clone()).unwrap();
        assert!(!decoded.events[0].correction);
        assert!(decoded.events[1].correction);
        let event_only = from_status(&source.to_string(), "draco", 100_000_000)
            .last_1h
            .unwrap();
        assert_eq!(event_only.requests, 1);
        assert_eq!(event_only.completed, 1);
        assert_eq!(event_only.success_rate, Some(1.0));
        assert_eq!(event_only.kill_rate, Some(0.0));
        assert_eq!(
            event_only.terminations_by_code,
            BTreeMap::from([("LB_SUPERVISION_LOST".into(), 1)])
        );
        let mut tied = source.clone();
        tied["events"][1]["observed_at_ms"] = json!(99995000);
        tied["events"][1]["finished_at_ms"] = json!(99995000);
        let tied_report = from_status(&tied.to_string(), "draco", 100_000_000)
            .last_1h
            .unwrap();
        assert_eq!(tied_report, event_only);
        source["requests"] = json!([source["events"][1].clone()]);
        let retained = from_status(&source.to_string(), "draco", 100_000_000)
            .last_1h
            .unwrap();
        assert_eq!(retained, event_only);
    }
}
