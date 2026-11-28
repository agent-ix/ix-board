// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Operational source adapters and canonical department measurements (FR-029).

use super::{
    Attribution, CATALOG_VERSION, Config, Distribution, Evidence, Fact, LoopFinding, LoopReport,
    Measurement, Observation, Scope, Value, definition,
};
use crate::retro::{Direction, Frame, Status};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;

wire! {
    /// Read-only operational inputs. No source is activated or repaired by monitoring.
    #[derive(Default)]
    OperationalConfig {
        /// Normalized evidence exports, including relay and attributed audit records.
        #[serde(default)] evidence_files: Vec<OperationalInput>,
        /// Redacted ix-build status exports, keyed by native host identity.
        #[serde(default)] scheduler_files: BTreeMap<String, PathBuf>,
        /// Filesystem reserve by native host identity.
        #[serde(default)] disk_reserve_bytes: BTreeMap<String, u64>,
        /// Filesystem identity by native host identity.
        #[serde(default)] filesystems: BTreeMap<String, String>
    }
}

wire! {
    /// Configured producer and the operational capabilities it is authorized to supply.
    OperationalInput {
        /// Read-only export path.
        path: PathBuf,
        /// Exact producer identity used by freshness policy.
        source: String,
        /// Explicit independent fact families supplied by this source.
        families: BTreeSet<String>
    }
}
wire! {
    /// Versioned operational export with explicit native population watermarks.
    OperationalExport {
        /// Export schema version, currently one.
        version: u32,
        /// Native facts and independently supplied source timestamps.
        evidence: Evidence,
        /// Coverage asserted by the authoritative producer, not inferred from rows.
        coverage: Vec<OperationalCoverage>
    }
}
wire! {
    /// Complete native subjects and time interval for one operational capability.
    OperationalCoverage {
        /// Canonical operational fact family.
        family: String,
        /// Hosts, lanes or teams covered, using source-native identities.
        entities: BTreeSet<String>,
        /// First covered UTC instant.
        from: String,
        /// Complete-through UTC watermark.
        until: String,
        /// Producer confirms no missing pages or events.
        complete: bool,
        /// History from subject origin, required for unresolved intervals/order chains.
        full_history: bool
    }
}

impl OperationalConfig {
    pub(super) fn validate(&self, scope: &Scope) -> Result<(), String> {
        let mut owners = BTreeSet::new();
        let mut sources = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for input in &self.evidence_files {
            if input.path.as_os_str().is_empty()
                || input.source.is_empty()
                || input.families.is_empty()
                || !sources.insert(&input.source)
                || !paths.insert(&input.path)
            {
                return Err("department.operations.evidence_files: empty/duplicate path, source or families".into());
            }
            for family in &input.families {
                if !OPERATIONAL_FAMILIES.contains(&family.as_str())
                    || !owners.insert(family.as_str())
                {
                    return Err(format!(
                        "department.operations.evidence_files.families: unknown or duplicate owner {family}"
                    ));
                }
            }
        }
        for (host, path) in &self.scheduler_files {
            if path.as_os_str().is_empty() || !scope.hosts.iter().any(|s| &s.id == host) {
                return Err(
                    "department.operations.scheduler_files: empty path or unselected host".into(),
                );
            }
            if owners.contains("host")
                || owners.contains("scheduler_request")
                || owners.contains("scheduler_event")
            {
                return Err("department.operations: scheduler and normalized export have duplicate capability ownership".into());
            }
        }
        Ok(())
    }
}

wire! {
    /// Explicit classification coverage for rates over an attributable message subset.
    ClassificationCoverage {
        /// Messages with a usable attributed classification.
        classified: u64,
        /// All distinct sends in the complete source population and window.
        total: u64,
        /// Whether the rate describes a labeled subset or the complete population.
        basis: CoverageBasis
    }
}
/// The population described by a classification-dependent rate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageBasis {
    /// Only the explicitly attributed classified sends are in the denominator.
    LabeledSubset,
    /// Every send in the complete read has a usable attributed classification.
    CompletePopulation,
}

/// Operational facts preserve explicit source classifications and native identities.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OperationFact {
    /// Verified host occupancy and resources at the source timestamp.
    Host(HostFact),
    /// Scheduler-native request lifecycle and measured elapsed durations.
    Request(RequestFact),
    /// Exact redacted scheduler transition with stable native identity and sequence.
    SchedulerEvent(SchedulerTransition),
    /// Verified pane authentication and movement observations; no lifecycle classifier.
    LaneSignal(LaneSignal),
    /// Explicit relay transition, deduplicated by message and transition kind.
    Relay(RelayFact),
    /// Attributed denial interval and explicit resolution.
    Denial(DenialFact),
    /// Time-aligned tool and ground-truth values for one native entity.
    Reconciliation(ReconciliationFact),
    /// Claimed and resolved instruction releases.
    Instruction(InstructionFact),
    /// Structured decision/spec blockers from a complete dependency pass.
    Decision(DecisionFact),
    /// Attributed message classification, never derived from message text.
    Message(MessageFact),
    /// Recorded planner pre-send and end-tick audit inputs.
    Assignment(AssignmentFact),
    /// Explicit order identity, action direction, supersession and evidence revision.
    Order(OrderFact),
}

wire! {
    /// Host occupancy and independently optional resource observations.
    HostFact {
        /// Native host identity.
        host: String,
        /// Current effective capacity; unrelated to lane work capacity.
        capacity_units: Option<u64>,
        /// Occupied scheduler resource units, including suspect leases.
        used_units: Option<u64>,
        /// One-minute observed host load.
        load_one: Option<f64>,
        /// Actual available filesystem bytes.
        disk_available_bytes: Option<u64>,
        /// Native filesystem identity for the sample.
        filesystem: Option<String>,
        /// Resource collection failure retained independently from scheduler occupancy.
        source_error: Option<String>,
        /// Actual resource observation timestamp, independent from lease-state observation.
        sample_observed_at: Option<String>
    }
}
wire! {
    /// One scheduler request with source-measured monotonic elapsed durations.
    RequestFact {
        /// Native host identity.
        host: String,
        /// Canonical owner lane.
        lane: String,
        /// Source-native ticket owner.
        ticket: String,
        /// Requested resource units, never inferred from wrapper processes.
        units: u64,
        /// Effective host capacity in this snapshot.
        capacity_units: u64,
        /// Scheduler-assigned base priority.
        priority: u32,
        /// Scheduler reports incomplete ownership/heartbeat evidence.
        suspect: bool,
        /// Native wait or termination reason.
        reason: Option<String>,
        /// Observed direct-child exit code.
        exit_code: Option<i32>,
        /// Observed direct-child termination signal.
        signal: Option<i32>,
        /// Current phase.
        phase: RequestPhase,
        /// Native enqueue UTC timestamp.
        enqueued_at: String,
        /// Native start UTC timestamp.
        started_at: Option<String>,
        /// Native terminal UTC timestamp.
        ended_at: Option<String>,
        /// Scheduler-monotonic queue duration at the source observation.
        wait_seconds: f64,
        /// Scheduler-monotonic hold duration at the source observation.
        hold_seconds: Option<f64>
    }
}
wire! {
    /// Published scheduler transition, without commands, environment or private state.
    SchedulerTransition {
        /// Native host identity.
        host: String,
        /// Canonical owning lane.
        lane: String,
        /// Native request identity.
        request_id: String,
        /// Native ticket owner.
        ticket: String,
        /// Resulting scheduler phase.
        phase: RequestPhase,
        /// Requested resource units.
        units: u64,
        /// Effective capacity at this transition.
        capacity_units: u64,
        /// Free units after transition.
        free_units: u64,
        /// Base priority.
        priority: u32,
        /// Native monotonic measured wait.
        wait_seconds: f64,
        /// Native monotonic measured hold.
        hold_seconds: Option<f64>,
        /// Native termination or failure reason.
        reason: Option<String>
    }
}
/// Closed scheduler phase vocabulary; unknown phases are refused at the wire boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPhase {
    /// Waiting for admission.
    Queued,
    /// Holds a lease.
    Running,
    /// Successful completion.
    Succeeded,
    /// Failed completion.
    Failed,
    /// Hold deadline exceeded.
    TimedOut,
    /// Explicit cancellation.
    Cancelled,
}
wire! {
    /// Existing lifecycle source supplements, without reclassifying its verdicts.
    LaneSignal {
        /// Canonical verified lane identity.
        lane: String,
        /// Verified login/authentication-screen observation.
        login_blocked: Option<bool>,
        /// Whether PR-head, commit, worktree and token movement inputs are complete.
        movement_complete: bool,
        /// Latest verified movement timestamp from each supported movement source.
        movement_at: Vec<String>,
        /// Missing input reasons, retained with the source row.
        reasons: BTreeMap<String, String>
    }
}
/// Relay transition kinds remain independent; enqueue never implies delivery.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayKind {
    /// Successfully enqueued.
    Queued,
    /// Delivery receipt.
    Delivered,
    /// Explicit acknowledgement.
    Acknowledged,
    /// Timed-out attempt without assumed delivery outcome.
    PromptTimeout,
}
wire! {
    /// One relay event chain transition, without message bodies.
    RelayFact {
        /// Stable original message identity, preserved on redelivery.
        message_id: String,
        /// Canonical destination lane identity.
        lane: String,
        /// Relay transition.
        transition: RelayKind,
        /// True only for an explicitly identified actionable order.
        order: bool
    }
}
wire! {
    /// Explicit denial-to-resolution interval.
    DenialFact {
        /// Canonical affected lane.
        lane: String,
        /// Stable denied order identity.
        order_id: String,
        /// Denial UTC timestamp.
        denied_at: String,
        /// Explicit resolution; absence extends the interval to the window end.
        resolved_at: Option<String>
    }
}
wire! {
    /// Independently observed values joined only on a source-native entity ID.
    ReconciliationFact {
        /// Tool-side observation.
        tool_value: Option<String>,
        /// Ground-truth observation.
        truth_value: Option<String>,
        /// Tool observation UTC timestamp.
        tool_at: Option<String>,
        /// Ground-truth observation UTC timestamp.
        truth_at: Option<String>
    }
}
wire! {
    /// Active instruction claim compared to the actually resolved release.
    InstructionFact {
        /// Canonical lane identity.
        lane: String,
        /// Instruction/tool identity being compared.
        component: String,
        /// Claimed active release.
        claimed: Option<String>,
        /// Resolved executable or installed plugin release.
        actual: Option<String>
    }
}
wire! {
    /// Structural decision/spec blocker evidence, without label-name heuristics.
    DecisionFact {
        /// Whether the issue is open at window end.
        open: bool,
        /// Explicit decision/ruling classification.
        decision: Option<bool>,
        /// Latest decision-open timestamp.
        opened_at: Option<String>,
        /// Distinct open issue IDs structurally blocked by this ruling.
        ruling_waiters: BTreeSet<String>,
        /// Distinct open issue IDs structurally blocked by its unmerged owned spec.
        spec_waiters: BTreeSet<String>
    }
}
wire! {
    /// Message semantic labels with attribution carried by the enclosing observation.
    MessageFact {
        /// Stable original message identity; redelivery preserves it.
        message_id: String,
        /// Original send UTC time, preserved by every redelivery or classification export.
        sent_at: String,
        /// Canonical sender address.
        sender: String,
        /// Explicit correction classification; absence is unclassified.
        correction: Option<bool>,
        /// Explicit status-only classification; absence is unclassified.
        status_only: Option<bool>
    }
}
wire! {
    /// Explicit planner audit inputs; missing inputs invalidate dependent metrics.
    AssignmentFact {
        /// Stable actionable message identity.
        message_id: String,
        /// Canonical target lane.
        lane: String,
        /// Source-native planner tick identity.
        tick_id: String,
        /// Published plan revision inspected before send.
        plan_revision: Option<String>,
        /// Assignment belongs to that plan revision.
        in_plan: Option<bool>,
        /// Assignment belongs to authorized lane scope.
        in_scope: Option<bool>,
        /// Explicit evidence freshness/identity pre-send audit.
        evidence_checked: Option<bool>,
        /// Explicit prior-order and changed-evidence pre-send audit.
        prior_orders_checked: Option<bool>,
        /// End-tick audit reference.
        end_tick_audit_ref: Option<String>,
        /// New blocking evidence permits another actionable send this tick.
        new_blocking_evidence: Option<bool>
    }
}
wire! {
    /// Attributed order relation; no semantic reversal is inferred from prose.
    OrderFact {
        /// Native send identity; redelivery preserves it, a resend gets a new identity.
        send_id: String,
        /// Stable order identity.
        order_id: String,
        /// Canonical target lane.
        lane: String,
        /// Stable action key.
        action_key: Option<String>,
        /// Explicit direction token; opposing token supplied by the source.
        direction: Option<String>,
        /// Explicitly reversed predecessor identity.
        reverses_order_id: Option<String>,
        /// Explicit superseded predecessor identity.
        supersedes_order_id: Option<String>,
        /// Immutable evidence/authority revision inspected for this order.
        evidence_revision: Option<String>
    }
}

impl OperationFact {
    /// Independent completeness capability required for this fact family.
    pub fn family(&self) -> &'static str {
        match self {
            Self::Host(_) => "host",
            Self::Request(_) => "scheduler_request",
            Self::SchedulerEvent(_) => "scheduler_event",
            Self::LaneSignal(_) => "lane_signal",
            Self::Relay(_) => "relay_event",
            Self::Denial(_) => "denial",
            Self::Reconciliation(_) => "reconciliation",
            Self::Instruction(_) => "instruction",
            Self::Decision(_) => "decision",
            Self::Message(_) => "message",
            Self::Assignment(_) => "assignment",
            Self::Order(_) => "order",
        }
    }
    pub(super) fn selected(&self, scope: &Scope, row: &Observation) -> bool {
        let host = |host: &str| scope.hosts.iter().any(|s| s.id == host);
        let lane = |lane: &str| scope.lanes.iter().any(|s| s.id.eq_ignore_ascii_case(lane));
        match self {
            Self::Host(f) => host(&f.host),
            Self::Request(f) => host(&f.host),
            Self::SchedulerEvent(f) => host(&f.host),
            Self::LaneSignal(f) => lane(&f.lane),
            Self::Relay(f) => lane(&f.lane),
            Self::Denial(f) => lane(&f.lane),
            Self::Instruction(f) => lane(&f.lane),
            Self::Assignment(f) => lane(&f.lane),
            Self::Order(f) => lane(&f.lane),
            Self::Message(f) => lane(&f.sender),
            Self::Reconciliation(_) | Self::Decision(_) => {
                row.scope
                    .teams
                    .iter()
                    .any(|s| scope.teams.iter().any(|c| c.id == s.id))
                    || row.scope.lanes.iter().any(|s| lane(&s.id))
            }
        }
    }
}

fn epoch(at: &str) -> Result<f64, String> {
    crate::drift::timestamp_epoch(at).map_err(|error| error.to_string())
}
fn event_time(row: &Observation) -> Result<f64, String> {
    epoch(row.event_at.as_deref().ok_or("missing event timestamp")?)
}
fn attributed(row: &Observation) -> Result<&Attribution, String> {
    let a = row
        .attribution
        .as_ref()
        .ok_or("missing semantic classification attribution")?;
    if a.actor.is_empty() || a.classifier_version.is_empty() || a.offset_end < a.offset_start {
        return Err("incomplete semantic classification attribution".into());
    }
    Ok(a)
}
fn canonical_sender(sender: &str) -> Result<String, String> {
    let parts: Vec<_> = sender.split(':').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err("sender is not a verified server:workspace:tab address".into());
    }
    Ok(sender.to_lowercase())
}

struct Calculator<'a> {
    config: &'a Config,
    operations: &'a OperationalConfig,
    frame: Frame,
    batches: &'a [Evidence],
}

struct Population<'a> {
    rows: Vec<(&'a Observation, &'a OperationFact)>,
    age: f64,
}

struct Outcome {
    value: Value,
    age: f64,
    population: u64,
    denominator: Option<u64>,
    refs: Vec<String>,
}

struct MessageSend {
    sender: String,
    sent_at: f64,
    correction: Result<Option<bool>, String>,
    status_only: Result<Option<bool>, String>,
}

fn merge_message_label(label: &mut Result<Option<bool>, String>, incoming: Option<bool>) {
    let Some(incoming) = incoming else {
        return;
    };
    match label {
        Ok(Some(previous)) if *previous != incoming => {
            *label = Err("conflicting attributed message classification".into())
        }
        Ok(value) => *value = Some(incoming),
        Err(_) => {}
    }
}

impl<'a> Calculator<'a> {
    fn contains(&self, at: f64) -> bool {
        at > self.frame.now - self.frame.window_secs as f64 && at <= self.frame.now
    }
    fn population(&self, family: &str) -> Result<Population<'a>, String> {
        let mut found = false;
        let mut rows = BTreeMap::new();
        let mut age: f64 = 0.0;
        for batch in self.batches {
            let has_family_rows = batch.observations.iter().any(|r| {
                matches!(&r.fact,
                Fact::Operation(f) if f.family() == family)
            });
            let relevant = batch.observations.iter().any(|r| {
                matches!(&r.fact,
                Fact::Operation(f) if f.family() == family && f.selected(&self.config.scope, r))
            });
            if has_family_rows && !relevant {
                continue;
            }
            if let Some(reason) = batch.reasons.get(family) {
                return Err(format!("{}: {reason}", batch.source));
            }
            if !batch.complete.contains(family) {
                if relevant {
                    return Err(format!("{}: incomplete {family} source", batch.source));
                }
                continue;
            }
            found = true;
            let limit = self
                .config
                .freshness_seconds
                .get(&batch.source)
                .ok_or_else(|| format!("missing freshness_seconds.{}", batch.source))?;
            let observed = epoch(
                batch
                    .observed_at
                    .as_deref()
                    .ok_or("missing source timestamp")?,
            )?;
            let source_age = self.frame.now - observed;
            if source_age < -(self.config.clock_tolerance_seconds as f64) {
                return Err(format!("{} excessive future clock skew", batch.source));
            }
            if source_age > *limit as f64 {
                return Err(format!("{} stale source", batch.source));
            }
            age = age.max(source_age.max(0.0));
            for row in &batch.observations {
                let Fact::Operation(fact) = &row.fact else {
                    continue;
                };
                if fact.family() != family || !fact.selected(&self.config.scope, row) {
                    continue;
                }
                if !row.complete {
                    return Err(format!("partial source row {}", row.event_id));
                }
                let observed = epoch(
                    row.observed_at
                        .as_deref()
                        .ok_or("missing row source timestamp")?,
                )?;
                let row_age = self.frame.now - observed;
                if row_age < -(self.config.clock_tolerance_seconds as f64)
                    || row_age > *limit as f64
                {
                    return Err(format!("stale or clock-skewed source row {}", row.event_id));
                }
                age = age.max(row_age.max(0.0));
                if event_time(row)? > self.frame.now + self.config.clock_tolerance_seconds as f64 {
                    return Err(format!("future event {}", row.event_id));
                }
                if let Some((old, _)) = rows.insert(row.event_id.as_str(), (row, fact))
                    && old != row
                {
                    return Err(format!("conflicting duplicate event {}", row.event_id));
                }
            }
        }
        if !found {
            return Err(format!("{family} source unavailable or not configured"));
        }
        Ok(Population {
            rows: rows.into_values().collect(),
            age,
        })
    }
    fn latest(
        &self,
        p: &Population<'a>,
    ) -> Result<Vec<(&'a Observation, &'a OperationFact)>, String> {
        let mut entities: BTreeMap<&str, (&Observation, &OperationFact)> = BTreeMap::new();
        for &(row, fact) in &p.rows {
            if event_time(row)? > self.frame.now {
                continue;
            }
            if let Some((previous, _)) = entities.get(row.entity_id.as_str()) {
                match event_time(row)?.total_cmp(&event_time(previous)?) {
                    std::cmp::Ordering::Less => continue,
                    std::cmp::Ordering::Equal => {
                        let (a, b) = previous
                            .sequence
                            .zip(row.sequence)
                            .ok_or("missing sequence for equal-time entity observations")?;
                        if b == a {
                            return Err("ambiguous equal-time entity observations".into());
                        }
                        if b < a {
                            continue;
                        }
                    }
                    std::cmp::Ordering::Greater => {}
                }
            }
            entities.insert(&row.entity_id, (row, fact));
        }
        Ok(entities.into_values().collect())
    }
    fn finish(
        &self,
        p: &Population<'a>,
        value: Value,
        denominator: Option<u64>,
    ) -> Result<Outcome, String> {
        Ok(Outcome {
            value,
            age: p.age,
            population: u64::try_from(p.rows.len()).map_err(|_| "population overflow")?,
            denominator,
            refs: p.rows.iter().map(|(r, _)| r.event_id.clone()).collect(),
        })
    }
    fn freshness_limit(&self, row: &Observation) -> Result<u64, String> {
        self.batches
            .iter()
            .filter(|b| b.observations.iter().any(|r| r.event_id == row.event_id))
            .filter_map(|b| self.config.freshness_seconds.get(&b.source))
            .copied()
            .min()
            .ok_or_else(|| "missing source freshness policy".into())
    }
    fn scalar(&self, p: &Population<'a>, value: f64) -> Result<Outcome, String> {
        self.finish(p, Value::Scalar(value), None)
    }
    fn durations(
        &self,
        p: &Population<'a>,
        samples: BTreeMap<String, Vec<f64>>,
    ) -> Result<Outcome, String> {
        let mut values = BTreeMap::new();
        for (key, samples) in samples {
            let d = Distribution::from_samples(&samples)
                .ok_or("empty or invalid duration population")?;
            values.insert(key, d);
        }
        if values.is_empty() {
            return Err("empty duration population".into());
        }
        self.finish(p, Value::Durations(values), None)
    }
    fn calculate(&self, id: &str) -> Result<Outcome, String> {
        match id {
            "host_slots_used"
            | "host_slots_free"
            | "host_overcommit_units"
            | "host_load_per_slot"
            | "disk_headroom_bytes"
            | "disk_available_bytes" => self.hosts(id),
            "queue_wait_minutes" | "lock_hold_minutes" => self.requests(id),
            "login_blocked_lanes" | "lane_no_movement_hours" => self.lanes(id),
            "relay_queued"
            | "relay_delivered"
            | "relay_acknowledged"
            | "prompt_timeout_unconfirmed"
            | "order_unacked_age_minutes" => self.relay(id),
            "refusals_by_lane" | "frozen_denial_hours" => self.denials(id),
            "reconciliation_agreement_ratio" | "reconciliation_unpaired" => self.reconcile(id),
            "source_age_hours" => self.source_ages(),
            "instruction_version_mismatches" => self.instructions(),
            "decisions_open"
            | "decision_oldest_hours"
            | "ruling_waiting_issues"
            | "spec_merge_waiting_issues" => self.decisions(id),
            "messages_per_sender" | "correction_rate" | "status_only_messages" => self.messages(id),
            "planner_ad_hoc_assignments"
            | "planner_scope_violations"
            | "planner_unbatched_sends" => self.assignments(id),
            "repeated_orders" => self.repeated_orders(),
            _ => Err("not an operational metric".into()),
        }
    }
    fn hosts(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("host")?;
        if self.config.scope.hosts.is_empty() {
            return Err("missing explicit host selectors".into());
        }
        let latest = self.latest(&p)?;
        let mut values = BTreeMap::new();
        let mut required_age = p.age;
        for selector in &self.config.scope.hosts {
            let (row, f) = latest
                .iter()
                .find_map(|(row, f)| match f {
                    OperationFact::Host(f) if f.host == selector.id => Some((*row, f)),
                    _ => None,
                })
                .ok_or_else(|| format!("host {} telemetry unavailable", selector.id))?;
            let capacity = || f.capacity_units.ok_or("host capacity unavailable");
            let used = || f.used_units.ok_or("host occupied units unavailable");
            if matches!(
                id,
                "host_load_per_slot" | "disk_headroom_bytes" | "disk_available_bytes"
            ) {
                let at = epoch(
                    f.sample_observed_at
                        .as_deref()
                        .ok_or("missing host resource sample timestamp")?,
                )?;
                let age = self.frame.now - at;
                let limit = self
                    .batches
                    .iter()
                    .filter(|b| b.observations.iter().any(|r| r.event_id == row.event_id))
                    .filter_map(|b| self.config.freshness_seconds.get(&b.source))
                    .min()
                    .ok_or("missing host resource sample freshness policy")?;
                if age > *limit as f64 || age < -(self.config.clock_tolerance_seconds as f64) {
                    return Err("stale or clock-skewed host resource sample".into());
                }
                required_age = required_age.max(age.max(0.0));
            }
            let disk = || -> Result<u64, String> {
                if let Some(reason) = &f.source_error {
                    return Err(reason.clone());
                }
                if f.filesystem.as_ref().is_none_or(String::is_empty) {
                    return Err("filesystem identity unavailable".into());
                }
                f.disk_available_bytes
                    .ok_or_else(|| "disk telemetry unavailable".into())
            };
            let value = match id {
                "host_slots_used" => used()? as f64,
                "host_slots_free" => capacity()?.saturating_sub(used()?) as f64,
                "host_overcommit_units" => used()?.saturating_sub(capacity()?) as f64,
                "host_load_per_slot" => {
                    if let Some(reason) = &f.source_error {
                        return Err(reason.clone());
                    }
                    let capacity = capacity()?;
                    if capacity == 0 {
                        return Err("zero host capacity denominator".into());
                    }
                    let load = f
                        .load_one
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or("host load unavailable or invalid")?;
                    load / capacity as f64
                }
                "disk_headroom_bytes" => {
                    let reserve = self
                        .operations
                        .disk_reserve_bytes
                        .get(&f.host)
                        .ok_or("missing configured disk reserve")?;
                    disk()? as f64 - *reserve as f64
                }
                "disk_available_bytes" => disk()? as f64,
                _ => return Err("unknown host metric".into()),
            };
            values.insert(f.host.clone(), value);
        }
        let mut outcome = self.finish(&p, Value::Keyed(values), None)?;
        outcome.age = required_age;
        Ok(outcome)
    }
    fn requests(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("scheduler_request")?;
        let mut samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (row, fact) in self.latest(&p)? {
            let OperationFact::Request(f) = fact else {
                continue;
            };
            let enqueued = epoch(&f.enqueued_at)?;
            let started = f.started_at.as_deref().map(epoch).transpose()?;
            let ended = f.ended_at.as_deref().map(epoch).transpose()?;
            if started.is_some_and(|s| s < enqueued)
                || ended.zip(started).is_some_and(|(e, s)| e < s)
            {
                return Err("inverted scheduler timestamps".into());
            }
            if enqueued > self.frame.now + self.config.clock_tolerance_seconds as f64
                || started.is_some_and(|s| {
                    s > self.frame.now + self.config.clock_tolerance_seconds as f64
                })
                || ended.is_some_and(|e| {
                    e > self.frame.now + self.config.clock_tolerance_seconds as f64
                })
            {
                return Err("future scheduler request timestamp".into());
            }
            let valid_phase = match f.phase {
                RequestPhase::Queued => started.is_none() && ended.is_none(),
                RequestPhase::Running => started.is_some() && ended.is_none(),
                RequestPhase::Cancelled => ended.is_some(),
                RequestPhase::Succeeded | RequestPhase::Failed | RequestPhase::TimedOut => {
                    started.is_some() && ended.is_some()
                }
            };
            if !valid_phase
                || !f.wait_seconds.is_finite()
                || f.wait_seconds < 0.0
                || f.hold_seconds.is_some_and(|s| !s.is_finite() || s < 0.0)
            {
                return Err("inconsistent scheduler phase/timing evidence".into());
            }
            let observation = epoch(
                row.observed_at
                    .as_deref()
                    .ok_or("missing request observation")?,
            )?;
            let (label, seconds) = if id == "queue_wait_minutes" {
                match f.phase {
                    RequestPhase::Queued => (
                        "queued_age",
                        f.wait_seconds + (self.frame.now - observation).max(0.0),
                    ),
                    RequestPhase::Running
                    | RequestPhase::Succeeded
                    | RequestPhase::Failed
                    | RequestPhase::TimedOut
                    | RequestPhase::Cancelled => {
                        let Some(started) = started else {
                            continue;
                        };
                        if !self.contains(started) {
                            continue;
                        }
                        ("started_wait", f.wait_seconds)
                    }
                }
            } else {
                match f.phase {
                    RequestPhase::Queued => continue,
                    RequestPhase::Running => (
                        "running_age",
                        f.hold_seconds.ok_or("missing running hold duration")?
                            + (self.frame.now - observation).max(0.0),
                    ),
                    RequestPhase::Succeeded
                    | RequestPhase::Failed
                    | RequestPhase::TimedOut
                    | RequestPhase::Cancelled => {
                        if started.is_none() {
                            continue;
                        }
                        if !self.contains(ended.ok_or("missing terminal request timestamp")?) {
                            continue;
                        }
                        (
                            "finished_hold",
                            f.hold_seconds.ok_or("missing final hold duration")?,
                        )
                    }
                }
            };
            if !seconds.is_finite() || seconds < 0.0 {
                return Err("invalid scheduler duration".into());
            }
            samples
                .entry(format!("{}/{label}", f.host))
                .or_default()
                .push(seconds / 60.0);
        }
        self.durations(&p, samples)
    }
    fn lanes(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("lane_signal")?;
        if self.config.scope.lanes.is_empty() {
            return Err("missing explicit lane selectors".into());
        }
        let latest = self.latest(&p)?;
        let mut values = BTreeMap::new();
        let mut login = 0_u64;
        for selector in &self.config.scope.lanes {
            let f = latest
                .iter()
                .find_map(|(_, f)| match f {
                    OperationFact::LaneSignal(f) if f.lane == selector.id => Some(f),
                    _ => None,
                })
                .ok_or_else(|| format!("lane {} remote/local source unavailable", selector.id))?;
            if id == "login_blocked_lanes" {
                login += u64::from(
                    f.login_blocked
                        .ok_or("verified login-screen evidence unavailable")?,
                );
            } else {
                if !f.movement_complete {
                    return Err(format!("incomplete lane movement inputs: {:?}", f.reasons));
                }
                let mut latest: Option<f64> = None;
                for at in &f.movement_at {
                    let at = epoch(at)?;
                    if at > self.frame.now + self.config.clock_tolerance_seconds as f64 {
                        return Err("future lane movement timestamp".into());
                    }
                    latest = Some(latest.map_or(at, |previous| previous.max(at)));
                }
                let at =
                    latest.ok_or("no verified token/PR-head/commit/worktree movement timestamp")?;
                values.insert(f.lane.clone(), (self.frame.now - at).max(0.0) / 3600.0);
            }
        }
        if id == "login_blocked_lanes" {
            self.scalar(&p, login as f64)
        } else {
            self.finish(&p, Value::Keyed(values), None)
        }
    }
    fn relay(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("relay_event")?;
        let mut chains: BTreeMap<&str, BTreeMap<RelayKind, &Observation>> = BTreeMap::new();
        for &(row, fact) in &p.rows {
            let OperationFact::Relay(f) = fact else {
                continue;
            };
            if event_time(row)? > self.frame.now {
                continue;
            }
            let chain = chains.entry(&f.message_id).or_default();
            if let Some(previous) = chain.get(&f.transition)
                && event_time(previous)? <= event_time(row)?
            {
                continue;
            }
            chain.insert(f.transition, row);
        }
        let mut count = 0_u64;
        let mut samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for chain in chains.values() {
            let transition = match id {
                "relay_queued" => Some(RelayKind::Queued),
                "relay_delivered" => Some(RelayKind::Delivered),
                "relay_acknowledged" => Some(RelayKind::Acknowledged),
                _ => None,
            };
            if let Some(transition) = transition {
                if let Some(row) = chain.get(&transition) {
                    count += u64::from(self.contains(event_time(row)?));
                }
            } else if id == "prompt_timeout_unconfirmed" {
                if let Some(row) = chain.get(&RelayKind::PromptTimeout)
                    && self.contains(event_time(row)?)
                    && !chain.contains_key(&RelayKind::Delivered)
                    && !chain.contains_key(&RelayKind::Acknowledged)
                {
                    count += 1;
                }
            } else if !chain.contains_key(&RelayKind::Acknowledged) {
                let (row, label) = if let Some(row) = chain.get(&RelayKind::Delivered) {
                    (*row, "since_delivery")
                } else if let Some(row) = chain.get(&RelayKind::Queued) {
                    (*row, "since_enqueue_delivery_unknown")
                } else {
                    continue;
                };
                let Fact::Operation(OperationFact::Relay(f)) = &row.fact else {
                    continue;
                };
                if f.order {
                    samples
                        .entry(format!("{}/{label}", f.lane))
                        .or_default()
                        .push((self.frame.now - event_time(row)?) / 60.0);
                }
            }
        }
        if id == "order_unacked_age_minutes" {
            self.durations(&p, samples)
        } else {
            self.scalar(&p, count as f64)
        }
    }
    fn denials(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("denial")?;
        let mut values: BTreeMap<String, f64> = self
            .config
            .scope
            .lanes
            .iter()
            .map(|s| (s.id.clone(), 0.0))
            .collect();
        let mut intervals: BTreeMap<&str, Vec<(f64, f64)>> = BTreeMap::new();
        for (row, fact) in self.latest(&p)? {
            let OperationFact::Denial(f) = fact else {
                continue;
            };
            attributed(row)?;
            let start = epoch(&f.denied_at)?;
            let end = f
                .resolved_at
                .as_deref()
                .map(epoch)
                .transpose()?
                .unwrap_or(self.frame.now);
            if start > self.frame.now + self.config.clock_tolerance_seconds as f64
                || end > self.frame.now + self.config.clock_tolerance_seconds as f64
            {
                return Err("future denial/resolution timestamp".into());
            }
            if end < start {
                return Err("denial resolution precedes denial".into());
            }
            if id == "refusals_by_lane" {
                if self.contains(start) {
                    *values.entry(f.lane.clone()).or_default() += 1.0;
                }
            } else {
                let start = start.max(self.frame.now - self.frame.window_secs as f64);
                let end = end.min(self.frame.now);
                if end > start {
                    intervals.entry(&f.lane).or_default().push((start, end));
                }
            }
        }
        if id == "frozen_denial_hours" {
            for (lane, mut ranges) in intervals {
                ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut union: Option<(f64, f64)> = None;
                let mut total = 0.0;
                for (start, end) in ranges {
                    match union {
                        Some((left, right)) if start <= right => {
                            union = Some((left, right.max(end)))
                        }
                        Some((left, right)) => {
                            total += right - left;
                            union = Some((start, end));
                        }
                        None => union = Some((start, end)),
                    }
                }
                if let Some((left, right)) = union {
                    total += right - left;
                }
                values.insert(lane.into(), total / 3600.0);
            }
        }
        self.finish(&p, Value::Keyed(values), None)
    }
    fn reconcile(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("reconciliation")?;
        let mut paired = 0_u64;
        let mut matching = 0_u64;
        let mut unpaired = 0_u64;
        let mut pair_age = p.age;
        for (row, fact) in self.latest(&p)? {
            let OperationFact::Reconciliation(f) = fact else {
                continue;
            };
            match (&f.tool_value, &f.truth_value) {
                (Some(tool), Some(truth)) => {
                    let a = epoch(
                        f.tool_at
                            .as_deref()
                            .ok_or("missing tool reconciliation timestamp")?,
                    )?;
                    let b = epoch(
                        f.truth_at
                            .as_deref()
                            .ok_or("missing truth reconciliation timestamp")?,
                    )?;
                    if (a - b).abs() > self.config.clock_tolerance_seconds as f64 {
                        return Err("reconciliation timestamp skew".into());
                    }
                    if a.max(b) > self.frame.now + self.config.clock_tolerance_seconds as f64 {
                        return Err("future reconciliation timestamp".into());
                    }
                    let age = (self.frame.now - a.min(b)).max(0.0);
                    if age > self.freshness_limit(row)? as f64 {
                        return Err("stale reconciliation observation".into());
                    }
                    pair_age = pair_age.max(age);
                    paired += 1;
                    matching += u64::from(tool == truth);
                }
                _ => unpaired += 1,
            }
        }
        if id == "reconciliation_unpaired" {
            return self.scalar(&p, unpaired as f64);
        }
        if paired == 0 {
            return Err("no complete paired reconciliation denominator".into());
        }
        let mut outcome = self.finish(
            &p,
            Value::Scalar(matching as f64 / paired as f64),
            Some(paired),
        )?;
        outcome.age = pair_age;
        Ok(outcome)
    }
    fn instructions(&self) -> Result<Outcome, String> {
        let p = self.population("instruction")?;
        let mut count = 0_u64;
        let mut lanes = BTreeSet::new();
        for (_, fact) in self.latest(&p)? {
            let OperationFact::Instruction(f) = fact else {
                continue;
            };
            if f.component.is_empty() {
                return Err("missing native instruction component identity".into());
            }
            lanes.insert(f.lane.as_str());
            let claimed = f
                .claimed
                .as_ref()
                .filter(|s| !s.is_empty())
                .ok_or("instruction claim unavailable")?;
            let actual = f
                .actual
                .as_ref()
                .filter(|s| !s.is_empty())
                .ok_or("resolved installed instruction version unavailable")?;
            count += u64::from(claimed != actual);
        }
        for lane in &self.config.scope.lanes {
            if !lanes.contains(lane.id.as_str()) {
                return Err(format!(
                    "lane {} installed instruction evidence unavailable",
                    lane.id
                ));
            }
        }
        self.scalar(&p, count as f64)
    }
    fn source_ages(&self) -> Result<Outcome, String> {
        let mut values = BTreeMap::new();
        let mut refs = BTreeSet::new();
        let mut age: f64 = 0.0;
        for source in self.config.freshness_seconds.keys() {
            let batches: Vec<_> = self
                .batches
                .iter()
                .filter(|b| b.source == *source)
                .collect();
            if batches.is_empty() {
                return Err(format!("source {source} unavailable"));
            }
            let mut oldest: f64 = 0.0;
            for batch in batches {
                let observed = epoch(
                    batch
                        .observed_at
                        .as_deref()
                        .ok_or("missing source observation timestamp")?,
                )?;
                let seconds = self.frame.now - observed;
                if seconds < -(self.config.clock_tolerance_seconds as f64) {
                    return Err("future source observation timestamp".into());
                }
                oldest = oldest.max(seconds.max(0.0));
                refs.extend(
                    batch
                        .observations
                        .iter()
                        .filter(|r| {
                            matches!(&r.fact,
                    Fact::Operation(f) if f.selected(&self.config.scope,r))
                        })
                        .map(|r| r.event_id.clone()),
                );
            }
            values.insert(source.clone(), oldest / 3600.0);
            age = age.max(oldest);
        }
        if values.is_empty() {
            return Err("missing configured source population".into());
        }
        Ok(Outcome {
            population: u64::try_from(values.len()).map_err(|_| "source population overflow")?,
            value: Value::Keyed(values),
            age,
            denominator: None,
            refs: refs.into_iter().collect(),
        })
    }
    fn decisions(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("decision")?;
        let mut count = 0_u64;
        let mut oldest: Option<f64> = None;
        let mut waiters = BTreeSet::new();
        for (row, fact) in self.latest(&p)? {
            let OperationFact::Decision(f) = fact else {
                continue;
            };
            if !f.open {
                continue;
            }
            attributed(row)?;
            let decision = f
                .decision
                .ok_or("decision/ruling classification unavailable")?;
            if decision {
                count += 1;
                if id == "decision_oldest_hours" {
                    let opened = epoch(
                        f.opened_at
                            .as_deref()
                            .ok_or("missing decision open timestamp")?,
                    )?;
                    if opened > self.frame.now {
                        return Err("future decision open timestamp".into());
                    }
                    oldest = Some(oldest.map_or(opened, |v| v.min(opened)));
                }
                if id == "ruling_waiting_issues" {
                    waiters.extend(f.ruling_waiters.iter());
                }
            }
            if id == "spec_merge_waiting_issues" {
                waiters.extend(f.spec_waiters.iter());
            }
        }
        match id {
            "decisions_open" => self.scalar(&p, count as f64),
            "decision_oldest_hours" => self.scalar(
                &p,
                (self.frame.now - oldest.ok_or("empty decision age population")?) / 3600.0,
            ),
            _ => self.scalar(&p, waiters.len() as f64),
        }
    }
    fn messages(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("message")?;
        let mut senders: BTreeMap<String, f64> = BTreeMap::new();
        let mut total = 0_u64;
        let mut corrections = 0_u64;
        let mut classified = 0_u64;
        let mut statuses = 0_u64;
        for message in self.message_sends(&p)?.into_values() {
            if !self.contains(message.sent_at) {
                continue;
            }
            total += 1;
            *senders.entry(message.sender).or_default() += 1.0;
            if id == "correction_rate"
                && let Some(correction) = message.correction?
            {
                classified += 1;
                corrections += u64::from(correction);
            }
            if id == "status_only_messages" {
                statuses += u64::from(
                    message
                        .status_only?
                        .ok_or("incomplete status-only classification")?,
                );
            }
        }
        match id {
            "messages_per_sender" => self.finish(&p, Value::Keyed(senders), None),
            "status_only_messages" => self.scalar(&p, statuses as f64),
            _ if classified == 0 => Err("zero attributable classified message denominator".into()),
            _ => {
                let mut outcome = self.finish(
                    &p,
                    Value::Scalar(corrections as f64 / classified as f64),
                    Some(classified),
                )?;
                outcome.population = total;
                Ok(outcome)
            }
        }
    }
    fn correction_coverage(&self) -> Result<ClassificationCoverage, String> {
        let p = self.population("message")?;
        let mut total = 0_u64;
        let mut classified = 0_u64;
        for message in self.message_sends(&p)?.into_values() {
            if !self.contains(message.sent_at) {
                continue;
            }
            total += 1;
            if message.correction?.is_some() {
                classified += 1;
            }
        }
        Ok(ClassificationCoverage {
            classified,
            total,
            basis: if classified == total {
                CoverageBasis::CompletePopulation
            } else {
                CoverageBasis::LabeledSubset
            },
        })
    }
    fn message_sends(&self, p: &Population<'a>) -> Result<BTreeMap<&'a str, MessageSend>, String> {
        let mut sends: BTreeMap<&str, MessageSend> = BTreeMap::new();
        for &(row, fact) in &p.rows {
            let OperationFact::Message(f) = fact else {
                continue;
            };
            let sent_at = epoch(&f.sent_at)?;
            if sent_at > self.frame.now + self.config.clock_tolerance_seconds as f64 {
                return Err("future original message send timestamp".into());
            }
            let sender = canonical_sender(&f.sender)?;
            let send = sends.entry(&f.message_id).or_insert_with(|| MessageSend {
                sender: sender.clone(),
                sent_at,
                correction: Ok(None),
                status_only: Ok(None),
            });
            if send.sender != sender || send.sent_at != sent_at {
                return Err("conflicting original message sender/time provenance".into());
            }
            if attributed(row).is_ok() {
                merge_message_label(&mut send.correction, f.correction);
                merge_message_label(&mut send.status_only, f.status_only);
            }
        }
        Ok(sends)
    }
    fn assignments(&self, id: &str) -> Result<Outcome, String> {
        let p = self.population("assignment")?;
        let mut seen = BTreeSet::new();
        let mut ticks: BTreeMap<(&str, &str), Vec<(&Observation, &AssignmentFact)>> =
            BTreeMap::new();
        let mut count = 0_u64;
        for &(row, fact) in &p.rows {
            let OperationFact::Assignment(f) = fact else {
                continue;
            };
            if !self.contains(event_time(row)?) || !seen.insert(&f.message_id) {
                continue;
            }
            attributed(row)?;
            if f.tick_id.is_empty()
                || f.plan_revision.as_ref().is_none_or(String::is_empty)
                || f.evidence_checked.is_none()
                || f.prior_orders_checked.is_none()
                || f.end_tick_audit_ref.as_ref().is_none_or(String::is_empty)
            {
                return Err("missing planner pre-send/end-tick audit inputs".into());
            }
            match id {
                "planner_ad_hoc_assignments" => {
                    count += u64::from(!f.in_plan.ok_or("missing published plan membership")?)
                }
                "planner_scope_violations" => {
                    count += u64::from(!f.in_scope.ok_or("missing authorized lane scope")?)
                }
                _ => {
                    ticks
                        .entry((&f.lane, &f.tick_id))
                        .or_default()
                        .push((row, f));
                }
            }
        }
        for mut sends in ticks.into_values() {
            sends.sort_by(|a, b| {
                event_time(a.0)
                    .unwrap_or(0.0)
                    .total_cmp(&event_time(b.0).unwrap_or(0.0))
                    .then(a.0.sequence.cmp(&b.0.sequence))
            });
            for pair in sends.windows(2) {
                if event_time(pair[0].0)? == event_time(pair[1].0)?
                    && pair[0]
                        .0
                        .sequence
                        .zip(pair[1].0.sequence)
                        .is_none_or(|(a, b)| a == b)
                {
                    return Err("missing planner send ordering".into());
                }
            }
            for (_, f) in sends.into_iter().skip(1) {
                count += u64::from(
                    !f.new_blocking_evidence
                        .ok_or("missing changed blocking evidence classification")?,
                );
            }
        }
        self.scalar(&p, count as f64)
    }
    fn repeated_orders(&self) -> Result<Outcome, String> {
        let p = self.population("order")?;
        let mut revisions: BTreeMap<(&str, &str), &str> = BTreeMap::new();
        let mut repeated = 0_u64;
        for (row, fact) in self.ordered_orders(&p)? {
            let OperationFact::Order(f) = fact else {
                continue;
            };
            let revision = f
                .evidence_revision
                .as_deref()
                .ok_or("missing order evidence/authority revision")?;
            if revisions.insert((&f.lane, &f.order_id), revision) == Some(revision)
                && self.contains(event_time(row)?)
            {
                repeated += 1;
            }
        }
        self.scalar(&p, repeated as f64)
    }
    fn ordered_orders(
        &self,
        p: &Population<'a>,
    ) -> Result<Vec<(&'a Observation, &'a OperationFact)>, String> {
        let mut candidates = Vec::new();
        for &(row, fact) in &p.rows {
            let at = event_time(row)?;
            if at > self.frame.now {
                continue;
            }
            candidates.push((row, fact));
        }
        candidates.sort_by(|a, b| {
            event_time(a.0)
                .unwrap_or(0.0)
                .total_cmp(&event_time(b.0).unwrap_or(0.0))
                .then(a.0.sequence.cmp(&b.0.sequence))
        });
        let mut rows = Vec::new();
        let mut sends: BTreeMap<(&str, &str), (&Observation, &OrderFact)> = BTreeMap::new();
        for (row, fact) in candidates {
            let OperationFact::Order(f) = fact else {
                continue;
            };
            if f.send_id.is_empty() {
                return Err("missing native order send identity".into());
            }
            let key = (f.lane.as_str(), f.send_id.as_str());
            if let Some((original, previous)) = sends.get(&key) {
                if *previous != f
                    || original.entity_id != row.entity_id
                    || row
                        .attribution
                        .as_ref()
                        .zip(original.attribution.as_ref())
                        .is_some_and(|(a, b)| a != b)
                {
                    return Err("conflicting redelivery order provenance".into());
                }
                if event_time(original)? == event_time(row)?
                    && original
                        .sequence
                        .zip(row.sequence)
                        .is_none_or(|(a, b)| a == b)
                {
                    return Err("missing original-send source sequence".into());
                }
                continue;
            }
            attributed(row)?;
            if f.action_key.as_ref().is_none_or(String::is_empty)
                || f.direction.as_ref().is_none_or(String::is_empty)
                || f.evidence_revision.as_ref().is_none_or(String::is_empty)
            {
                return Err("missing order action/direction/evidence classification".into());
            }
            sends.insert(key, (row, f));
            rows.push((row, fact));
        }
        for pair in rows.windows(2) {
            if event_time(pair[0].0)? == event_time(pair[1].0)?
                && pair[0]
                    .0
                    .sequence
                    .zip(pair[1].0.sequence)
                    .is_none_or(|(a, b)| a == b)
            {
                return Err("missing equal-time order source sequence".into());
            }
        }
        Ok(rows)
    }
    fn order_flips(&self) -> Result<Vec<LoopFinding>, String> {
        let p = self.population("order")?;
        let lookback = self
            .config
            .parameters
            .loop_lookback_seconds
            .ok_or("missing loop_lookback_seconds")?;
        let repeat = self
            .config
            .parameters
            .loop_repeat_count
            .filter(|n| *n > 0)
            .ok_or("missing positive loop_repeat_count")?;
        let mut previous: BTreeMap<(&str, &str), (&Observation, &OrderFact)> = BTreeMap::new();
        let mut groups: BTreeMap<(&str, &str), (u64, Vec<&Observation>)> = BTreeMap::new();
        for (row, fact) in self.ordered_orders(&p)? {
            let OperationFact::Order(f) = fact else {
                continue;
            };
            let key = (
                f.lane.as_str(),
                f.action_key.as_deref().ok_or("missing action key")?,
            );
            if let Some((old_row, old)) = previous.get(&key)
                && event_time(row)? > self.frame.now - lookback as f64
                && f.supersedes_order_id.as_deref() == Some(&old.order_id)
                && f.reverses_order_id.as_deref() == Some(&old.order_id)
                && f.direction != old.direction
                && f.evidence_revision == old.evidence_revision
            {
                let (flips, rows) = groups.entry(key).or_default();
                *flips += 1;
                if rows.last().is_none_or(|r| r.event_id != old_row.event_id) {
                    rows.push(old_row);
                }
                rows.push(row);
            }
            previous.insert(key, (row, f));
        }
        let mut findings = Vec::new();
        for ((lane, action), (flips, rows)) in groups {
            if flips < repeat {
                continue;
            }
            if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
                let event_ids: Vec<_> = rows.iter().map(|r| r.event_id.clone()).collect();
                findings.push(LoopFinding {
                    deduplication_key: super::digest(&(
                        "order_flip_loop",
                        lane,
                        action,
                        &event_ids,
                    )),
                    event_ids,
                    start: first.event_at.clone().ok_or("missing first event time")?,
                    end: last.event_at.clone().ok_or("missing last event time")?,
                    participants: [lane.into(), action.into()].into(),
                });
            }
        }
        Ok(findings)
    }
}

/// Calculates operational metrics in the shared envelope; core assessment runs afterwards.
pub fn collect_operations(
    config: &Config,
    operations: &OperationalConfig,
    frame: Frame,
    batches: &[Evidence],
) -> (BTreeMap<String, Measurement>, LoopReport) {
    let calc = Calculator {
        config,
        operations,
        frame,
        batches,
    };
    let mut identity_config = config.clone();
    identity_config.operations = Some(operations.clone());
    let mut measurements = BTreeMap::new();
    for id in OPERATIONAL_METRICS {
        let Some(d) = definition(id) else {
            continue;
        };
        let outcome = calc.calculate(id);
        let (value, reason, age, population, denominator, mut refs) = match outcome {
            Ok(o) => (
                Some(o.value),
                None,
                Some(o.age),
                Some(o.population),
                o.denominator,
                o.refs,
            ),
            Err(reason) => (
                None,
                Some(reason),
                None,
                None,
                None,
                batches
                    .iter()
                    .flat_map(|b| &b.observations)
                    .filter(|r| matches!(&r.fact,Fact::Operation(f) if f.selected(&config.scope,r)))
                    .map(|r| r.event_id.clone())
                    .collect(),
            ),
        };
        refs.sort();
        refs.dedup();
        measurements.insert(
            (*id).into(),
            Measurement {
                id: (*id).into(),
                catalog_version: CATALOG_VERSION.into(),
                status: if value.is_some() {
                    Status::Ok
                } else {
                    Status::Unavailable
                },
                value,
                reason,
                unit: d.unit.into(),
                direction: if matches!(
                    *id,
                    "host_slots_free"
                        | "disk_headroom_bytes"
                        | "disk_available_bytes"
                        | "reconciliation_agreement_ratio"
                ) {
                    Direction::Up
                } else {
                    Direction::Down
                },
                scope: config.scope.clone(),
                query_identity: identity_config.measurement_identity(id, frame.window_secs),
                window_start: crate::drift::format_timestamp(frame.now - frame.window_secs as f64),
                window_end: crate::drift::format_timestamp(frame.now),
                window_seconds: frame.window_secs,
                source_age_seconds: age,
                population,
                denominator,
                evidence_refs: refs,
                denominators: if *id == "host_load_per_slot" {
                    calc.population("host")
                        .and_then(|p| calc.latest(&p))
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|(_, f)| match f {
                            OperationFact::Host(f) => f.capacity_units.map(|n| (f.host.clone(), n)),
                            _ => None,
                        })
                        .collect()
                } else {
                    BTreeMap::new()
                },
                query_parameters: config.parameters.clone(),
                classification_coverage: if *id == "correction_rate" {
                    calc.correction_coverage().ok()
                } else {
                    None
                },
                operational_parameters: Some(operations.clone()),
            },
        );
    }
    let (status, reason, findings) = match calc.order_flips() {
        Ok(findings) => (Status::Ok, None, findings),
        Err(reason) => (Status::Unavailable, Some(reason), Vec::new()),
    };
    (
        measurements,
        LoopReport {
            detector_id: "order_flip_loop".into(),
            status,
            reason,
            findings,
        },
    )
}

const OPERATIONAL_METRICS: &[&str] = &[
    "host_slots_used",
    "host_slots_free",
    "host_overcommit_units",
    "queue_wait_minutes",
    "lock_hold_minutes",
    "host_load_per_slot",
    "disk_headroom_bytes",
    "disk_available_bytes",
    "login_blocked_lanes",
    "order_unacked_age_minutes",
    "lane_no_movement_hours",
    "refusals_by_lane",
    "frozen_denial_hours",
    "repeated_orders",
    "reconciliation_agreement_ratio",
    "reconciliation_unpaired",
    "source_age_hours",
    "instruction_version_mismatches",
    "relay_queued",
    "relay_delivered",
    "relay_acknowledged",
    "prompt_timeout_unconfirmed",
    "ruling_waiting_issues",
    "spec_merge_waiting_issues",
    "messages_per_sender",
    "correction_rate",
    "status_only_messages",
    "planner_ad_hoc_assignments",
    "planner_scope_violations",
    "planner_unbatched_sends",
];

/// Reads configured credential-free exports. Source failures remain dependent unknowns.
pub fn read_operational_files(
    operations: &OperationalConfig,
    config: &Config,
    frame: Frame,
) -> Vec<Evidence> {
    let mut batches = Vec::new();
    for input in &operations.evidence_files {
        let result = read_json::<OperationalExport>(&input.path)
            .and_then(|export| normalize_export(export, input, config, frame));
        batches.push(result.unwrap_or_else(|reason| {
            Evidence {
                source: input.source.clone(),
                reasons: input
                    .families
                    .iter()
                    .map(|f| (f.clone(), reason.clone()))
                    .collect(),
                ..Default::default()
            }
        }));
    }
    for (host, path) in &operations.scheduler_files {
        batches.push(
            read_json::<SchedulerReport>(path)
                .and_then(|report| {
                    scheduler_evidence(report, host, path, operations, &config.scope)
                })
                .unwrap_or_else(|reason| Evidence {
                    source: format!("scheduler:{host}"),
                    reasons: ["host", "scheduler_request"]
                        .map(|f| (f.into(), reason.clone()))
                        .into(),
                    ..Default::default()
                }),
        );
    }
    if !operations.scheduler_files.is_empty() {
        for host in &config.scope.hosts {
            if !operations.scheduler_files.contains_key(&host.id) {
                batches.push(Evidence {
                    source: format!("scheduler:{}", host.id),
                    reasons: ["host", "scheduler_request"]
                        .map(|f| {
                            (
                                f.into(),
                                format!("host {} telemetry export not configured", host.id),
                            )
                        })
                        .into(),
                    ..Default::default()
                });
            }
        }
    }
    batches
}
fn normalize_export(
    export: OperationalExport,
    input: &OperationalInput,
    config: &Config,
    frame: Frame,
) -> Result<Evidence, String> {
    if export.version != 1 {
        return Err(format!(
            "unsupported operational export.version {}",
            export.version
        ));
    }
    let mut evidence = export.evidence;
    if evidence.source != input.source {
        return Err("operational export source differs from configured producer".into());
    }
    let observed = epoch(
        evidence
            .observed_at
            .as_deref()
            .ok_or("missing operational source timestamp")?,
    )?;
    let mut errors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rows = Vec::new();
    for row in std::mem::take(&mut evidence.observations) {
        let family = match &row.fact {
            Fact::Operation(f) => f.family(),
            Fact::Lifecycle(_) => "lifecycle",
            _ => return Err("operational export contains an unauthorized fact family".into()),
        };
        if !input.families.contains(family) {
            return Err("operational export contains unconfigured capability".into());
        }
        let validation = (|| -> Result<(), String> {
            if row.event_id.is_empty() || row.entity_id.is_empty() || row.source_ref.is_empty() {
                return Err("operational export lacks native event/entity/reference".into());
            }
            if event_time(&row)? > observed + config.clock_tolerance_seconds as f64 {
                return Err("operational event follows source observation watermark".into());
            }
            Ok(())
        })();
        match validation {
            Ok(()) => rows.push(row),
            Err(reason) => {
                errors
                    .entry(family.into())
                    .or_default()
                    .insert(format!("{}: {reason}", row.event_id));
            }
        }
    }
    evidence.observations = rows;
    for family in &input.families {
        let selectors = match family.as_str() {
            "host" | "scheduler_request" | "scheduler_event" => &config.scope.hosts,
            "decision" | "reconciliation" => &config.scope.teams,
            _ => &config.scope.lanes,
        };
        let expected: BTreeSet<_> = selectors.iter().map(|s| s.id.as_str()).collect();
        let gauge = matches!(
            family.as_str(),
            "host" | "lane_signal" | "reconciliation" | "instruction" | "decision" | "lifecycle"
        );
        let required_until = if gauge { observed } else { frame.now };
        let from = if gauge {
            observed
        } else {
            frame.now - frame.window_secs as f64
        };
        let origin = matches!(family.as_str(), "order" | "denial" | "relay_event");
        let validation = (|| -> Result<(), String> {
            let mut covered = BTreeSet::new();
            for coverage in export.coverage.iter().filter(|c| &c.family == family) {
                let beginning = epoch(&coverage.from)?;
                let until = epoch(&coverage.until)?;
                if beginning > until || until > observed + config.clock_tolerance_seconds as f64 {
                    return Err("invalid operational coverage interval/watermark".into());
                }
                if coverage.complete
                    && (!origin || coverage.full_history)
                    && beginning <= from
                    && until >= required_until
                {
                    covered.extend(coverage.entities.iter().map(String::as_str));
                }
            }
            let missing: Vec<_> = expected.difference(&covered).copied().collect();
            if expected.is_empty() || !missing.is_empty() || !evidence.complete.contains(family) {
                return Err(format!(
                    "missing complete {family} coverage for configured native subjects {missing:?}"
                ));
            }
            Ok(())
        })();
        if let Err(reason) = validation {
            errors.entry(family.clone()).or_default().insert(reason);
        }
    }
    for (family, mut reasons) in errors {
        evidence.complete.remove(&family);
        if let Some(previous) = evidence.reasons.remove(&family) {
            reasons.insert(previous);
        }
        evidence
            .reasons
            .insert(family, reasons.into_iter().collect::<Vec<_>>().join("; "));
    }
    evidence.complete.retain(|f| input.families.contains(f));
    Ok(evidence)
}
const OPERATIONAL_FAMILIES: &[&str] = &[
    "host",
    "scheduler_request",
    "scheduler_event",
    "lifecycle",
    "lane_signal",
    "relay_event",
    "denial",
    "reconciliation",
    "instruction",
    "decision",
    "message",
    "assignment",
    "order",
];
fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, String> {
    const LIMIT: u64 = 32 * 1024 * 1024;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("source export must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if u64::try_from(bytes.len()).map_err(|_| "source size overflow")? > LIMIT {
        return Err("source export exceeds 32 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

// Exact redacted ix-build v1 export; private command/state fields are never read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchedulerReport {
    schema_version: u32,
    store_id: String,
    host: String,
    observed_at_ms: u64,
    capacity_units: u32,
    used_units: u32,
    free_units: u32,
    overcommit_units: u32,
    sample: Option<HostSample>,
    source_error: Option<String>,
    requests: Vec<SchedulerRequest>,
    events: Vec<SchedulerEvent>,
    pruned_events: u64,
    next_event_sequence: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    dead_code,
    reason = "Decode the complete redacted upstream wire contract, including display-only fields."
)]
struct HostSample {
    host: String,
    observed_at_ms: u64,
    memory_available_bytes: u64,
    disk_available_bytes: u64,
    load_one: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchedulerRequest {
    request_id: String,
    owner: String,
    ticket: String,
    state: RequestPhase,
    units: u32,
    priority: u32,
    enqueued_at_ms: u64,
    started_at_ms: Option<u64>,
    ended_at_ms: Option<u64>,
    wait_ms: u64,
    hold_ms: Option<u64>,
    suspect: bool,
    reason: Option<String>,
    exit_code: Option<i32>,
    signal: Option<i32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchedulerEvent {
    schema_version: u32,
    event_id: String,
    request_id: String,
    host: String,
    owner: String,
    ticket: String,
    observed_at_ms: u64,
    state: RequestPhase,
    units: u32,
    priority: u32,
    capacity_units: u32,
    free_units: u32,
    wait_ms: u64,
    hold_ms: Option<u64>,
    reason: Option<String>,
}
fn milliseconds(at: u64) -> String {
    crate::drift::format_timestamp(at as f64 / 1000.0)
}
fn scheduler_evidence(
    report: SchedulerReport,
    expected_host: &str,
    path: &std::path::Path,
    config: &OperationalConfig,
    scope: &Scope,
) -> Result<Evidence, String> {
    if report.schema_version != 1 {
        return Err(format!(
            "unsupported scheduler schema_version {}",
            report.schema_version
        ));
    }
    if report.host != expected_host || report.store_id.is_empty() {
        return Err("scheduler export host/store identity mismatch".into());
    }
    if report.free_units != report.capacity_units.saturating_sub(report.used_units)
        || report.overcommit_units != report.used_units.saturating_sub(report.capacity_units)
    {
        return Err("inconsistent scheduler occupancy".into());
    }
    let observed = milliseconds(report.observed_at_ms);
    let mut evidence = Evidence {
        source: format!("scheduler:{expected_host}"),
        observed_at: Some(observed.clone()),
        complete: ["host".into(), "scheduler_request".into()].into(),
        ..Default::default()
    };
    let source = format!("file:{}", path.display());
    if report
        .sample
        .as_ref()
        .is_some_and(|s| s.host != expected_host)
    {
        return Err("scheduler host sample identity mismatch".into());
    }
    evidence.observations.push(Observation {
        event_id: format!("{}:host:{}", report.store_id, report.observed_at_ms),
        entity_id: expected_host.into(),
        event_at: Some(observed.clone()),
        observed_at: Some(observed.clone()),
        sequence: None,
        scope: scope.clone(),
        complete: true,
        source_ref: source.clone(),
        attribution: None,
        fact: Fact::Operation(OperationFact::Host(HostFact {
            host: expected_host.into(),
            capacity_units: Some(u64::from(report.capacity_units)),
            used_units: Some(u64::from(report.used_units)),
            load_one: report.sample.as_ref().map(|s| s.load_one),
            disk_available_bytes: report.sample.as_ref().map(|s| s.disk_available_bytes),
            filesystem: config.filesystems.get(expected_host).cloned(),
            source_error: report.source_error,
            sample_observed_at: report
                .sample
                .as_ref()
                .map(|s| milliseconds(s.observed_at_ms)),
        })),
    });
    let mut seen = BTreeSet::new();
    for request in report.requests {
        if !seen.insert(request.request_id.clone()) {
            return Err("duplicate scheduler request identity".into());
        }
        evidence.observations.push(Observation {
            event_id: format!("{}:snapshot:{}", request.request_id, report.observed_at_ms),
            entity_id: request.request_id,
            event_at: Some(observed.clone()),
            observed_at: Some(observed.clone()),
            sequence: None,
            scope: scope.clone(),
            complete: true,
            source_ref: source.clone(),
            attribution: None,
            fact: Fact::Operation(OperationFact::Request(RequestFact {
                host: expected_host.into(),
                lane: request.owner,
                ticket: request.ticket,
                units: u64::from(request.units),
                capacity_units: u64::from(report.capacity_units),
                priority: request.priority,
                suspect: request.suspect,
                reason: request.reason,
                exit_code: request.exit_code,
                signal: request.signal,
                phase: request.state,
                enqueued_at: milliseconds(request.enqueued_at_ms),
                started_at: request.started_at_ms.map(milliseconds),
                ended_at: request.ended_at_ms.map(milliseconds),
                wait_seconds: request.wait_ms as f64 / 1000.0,
                hold_seconds: request.hold_ms.map(|n| n as f64 / 1000.0),
            })),
        });
    }
    // Request snapshots retain complete elapsed samples only while terminal requests are retained.
    // Explicit event pruning therefore invalidates the window-history capability.
    if report.pruned_events > 0 {
        evidence.complete.remove("scheduler_request");
        evidence.reasons.insert(
            "scheduler_request".into(),
            "scheduler transition history pruned".into(),
        );
    }
    if report.next_event_sequence == 0 {
        return Err("invalid scheduler event sequence".into());
    }
    let mut sequences = BTreeSet::new();
    for event in report.events {
        if event.schema_version != 1 || event.host != expected_host {
            return Err("scheduler event schema/host mismatch".into());
        }
        let prefix = format!("{}:e", report.store_id);
        let sequence: u64 = event
            .event_id
            .strip_prefix(&prefix)
            .ok_or("scheduler event lacks native store sequence")?
            .parse()
            .map_err(|_| "invalid scheduler event sequence")?;
        if sequence == 0
            || sequence >= report.next_event_sequence
            || !sequences.insert(sequence)
            || event.observed_at_ms > report.observed_at_ms
        {
            return Err("inconsistent scheduler transition sequence/watermark".into());
        }
        evidence.observations.push(Observation {
            event_id: event.event_id,
            entity_id: event.request_id.clone(),
            event_at: Some(milliseconds(event.observed_at_ms)),
            observed_at: Some(observed.clone()),
            sequence: Some(sequence),
            scope: scope.clone(),
            complete: true,
            source_ref: source.clone(),
            attribution: None,
            fact: Fact::Operation(OperationFact::SchedulerEvent(SchedulerTransition {
                host: expected_host.into(),
                lane: event.owner,
                request_id: event.request_id,
                ticket: event.ticket,
                phase: event.state,
                units: u64::from(event.units),
                capacity_units: u64::from(event.capacity_units),
                free_units: u64::from(event.free_units),
                priority: event.priority,
                wait_seconds: event.wait_ms as f64 / 1000.0,
                hold_seconds: event.hold_ms.map(|n| n as f64 / 1000.0),
                reason: event.reason,
            })),
        });
    }
    let retained =
        u64::try_from(sequences.len()).map_err(|_| "scheduler event population overflow")?;
    if retained
        .checked_add(report.pruned_events)
        .and_then(|n| n.checked_add(1))
        != Some(report.next_event_sequence)
    {
        evidence.complete.remove("scheduler_request");
        evidence.reasons.insert(
            "scheduler_request".into(),
            "incomplete scheduler transition history".into(),
        );
    }
    Ok(evidence)
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
