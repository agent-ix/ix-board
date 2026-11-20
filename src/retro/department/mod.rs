// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Shared department evidence, assessment and persistence contract (FR-029).

use super::{Direction, Status};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Definition release used in measurement identities.
pub const CATALOG_VERSION: &str = "department-1";
/// Canonical class identifiers, in report order.
pub const CLASSES: [&str; 10] = [
    "flow_wip",
    "serial_dependency",
    "pin_version_churn",
    "capacity_resources",
    "liveness",
    "authority_policy",
    "signal_integrity",
    "review_rework",
    "decision_latency",
    "comms_overhead",
];

// Each field documents the shared wire contract as well as generating strict decoders.
macro_rules! wire {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])* #[derive(Clone, Debug, serde::Deserialize, PartialEq, serde::Serialize)]
        #[serde(deny_unknown_fields)] pub struct $name { $($(#[$field_meta])* pub $field: $ty),* }
    };
}
mod assess;
mod collect;
mod evidence;
mod github;
mod native;
mod operations;
mod pull_requests;
mod source_logs;
pub use assess::*;
pub use collect::{LoopFinding, LoopReport, collect};
pub use evidence::*;
pub use github::collect_pins;
pub use native::{enrich_claim_history, enrich_merged_attachments, enrich_reviewer_rounds};
pub use operations::*;
pub use pull_requests::{
    DIFF_CLASSIFIER, PR_DETAILS, PR_FILES, PR_REVIEWS, PR_TIMELINE, RAW_CONTENT,
    collect_pull_request,
};
pub use source_logs::{
    Coverage, LogEvent, LogInput, LogKind, MovementKind, NativeRecord, SourceLog, read_source_logs,
};
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;

wire! {
    /// A native selector paired with its display label.
    Selector {
        /// Source-native identity.
        id: String,
        /// Display label; never used for joins.
        label: String
    }
}
wire! {
    /// Explicit selected populations.
    #[derive(Default)]
    Scope {
        /// Native team IDs or source-native team keys.
        #[serde(default)] teams: Vec<Selector>,
        /// Native project UUIDs.
        #[serde(default)] projects: Vec<Selector>,
        /// Native repository names.
        #[serde(default)] repositories: Vec<Selector>,
        /// Canonical lane identities.
        #[serde(default)] lanes: Vec<Selector>,
        /// Native host identities.
        #[serde(default)] hosts: Vec<Selector>
    }
}
wire! {
    /// Configured metric parameters, never inferred from host process counts.
    #[derive(Default)]
    Parameters {
        /// Explicit read-only authoritative history/classification exports.
        #[serde(default)] event_logs: Vec<LogInput>,
        /// Native dependency identities selected for tag/repin event populations.
        #[serde(default)] dependencies: BTreeSet<String>,
        /// Work capacity per lane, distinct from host build capacity.
        #[serde(default)] lane_capacity: BTreeMap<String, u64>,
        /// Issue UUID to owning lane.
        #[serde(default)] issue_lanes: BTreeMap<String, String>,
        /// Required named completion obligations.
        #[serde(default)] completion_obligations: Option<BTreeSet<String>>,
        /// Source-native state IDs selected for residence durations.
        #[serde(default)] states: BTreeSet<String>,
        /// Gate issue UUIDs.
        #[serde(default)] gates: BTreeSet<String>,
        /// Claim staleness interval.
        #[serde(default)] stale_seconds: Option<u64>,
        /// Maximum tag to repin interval.
        #[serde(default)] repin_seconds: Option<u64>,
        /// Required diff classifier release.
        #[serde(default)] diff_classifier: Option<String>,
        /// Authorized review round limit.
        #[serde(default)] review_round_limit: Option<u64>,
        /// Native submitted-review states qualifying for reviewer-age exclusion.
        #[serde(default)] qualifying_review_states: BTreeSet<String>,
        /// Loop lookback interval.
        #[serde(default)] loop_lookback_seconds: Option<u64>,
        /// Minimum distinct repeats.
        #[serde(default)] loop_repeat_count: Option<u64>
    }
}
wire! {
    /// Versioned department configuration.
    Config {
        /// Configuration schema, currently one.
        version: u32,
        /// Assessment release identity.
        policy_id: String,
        /// Explicit measurement scope.
        scope: Scope,
        /// Maximum permitted source age by source name.
        freshness_seconds: BTreeMap<String, u64>,
        /// Permitted future source timestamp skew.
        clock_tolerance_seconds: u64,
        /// Counting parameters.
        #[serde(default)] parameters: Parameters,
        /// Explicit credential-free operational source exports and resource policy.
        #[serde(default, skip_serializing_if = "Option::is_none")] operations: Option<OperationalConfig>,
        /// Metric policies, identified by metric ID.
        #[serde(default)] policies: Vec<Policy>,
        /// Class required/optional membership and routes.
        #[serde(default)] classes: Vec<ClassConfig>
    }
}
wire! {
    /// Metric membership and ownership for one canonical class.
    ClassConfig {
        /// Canonical class ID.
        id: String,
        /// Metrics affecting severity and completeness.
        required: BTreeSet<String>,
        /// Display-only metrics.
        #[serde(default)] optional: BTreeSet<String>,
        /// Explicit owner and action, if configured.
        #[serde(default)] route: Option<OwnerAction>
    }
}
wire! {
    /// Configured accountable owner and normal action.
    OwnerAction {
        /// Owner native identity or configured address.
        owner: String,
        /// Concrete configured action.
        action: String
    }
}
wire! {
    /// Numeric threshold policy. Keyed measurements must explicitly select keys.
    Policy {
        /// Stable metric ID; unique in the configuration.
        metric_id: String,
        /// Selected scalar or distribution component.
        statistic: Statistic,
        /// Component unit.
        unit: String,
        /// Threshold comparison.
        comparator: Comparator,
        /// Amber boundary.
        amber: f64,
        /// Red boundary.
        red: f64,
        /// Current value or compatible numeric difference.
        basis: Basis,
        /// Required measurement window.
        window_seconds: u64,
        /// Maximum source age allowed by this policy.
        freshness_seconds: u64,
        /// Explicit selected native keys for keyed values.
        #[serde(default)] expected_keys: BTreeSet<String>,
        /// Required for keyed values.
        #[serde(default)] key_reduction: Option<KeyReduction>
    }
}
/// Numeric component selected for assessment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Statistic {
    /// Scalar value.
    Value,
    /// Distribution population.
    Count,
    /// Distribution median.
    Median,
    /// Distribution nearest rank p90.
    P90,
}
/// Threshold operator; equality is explicit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparator {
    /// Strictly greater.
    Gt,
    /// Greater or equal.
    Gte,
    /// Strictly less.
    Lt,
    /// Less or equal.
    Lte,
}
/// Reference used by a threshold policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Current observation.
    Current,
    /// Latest minus previous.
    SincePrevious,
    /// Latest minus baseline.
    SinceBaseline,
}
/// Explicit keyed aggregation rule.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyReduction {
    /// Red, amber, unknown, then green.
    WorstSeverity,
}
/// Numeric or categorical measurement representation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    /// One scalar.
    Scalar(f64),
    /// Scalar rows by native key.
    Keyed(BTreeMap<String, f64>),
    /// Duration rows by native key.
    Durations(BTreeMap<String, Distribution>),
    /// Original categorical source observation.
    SourceObservation(BTreeMap<String, SourceObservation>),
}
wire! {
    /// A duration population; empty populations never construct this value.
    Distribution {
        /// Population size, not a duration.
        count: u64,
        /// Median in the metric unit.
        median: f64,
        /// Nearest rank p90 in the metric unit.
        p90: f64
    }
}
impl Distribution {
    /// Summarizes finite nonnegative samples; empty populations are unavailable.
    pub fn from_samples(samples: &[f64]) -> Option<Self> {
        if samples.is_empty() || samples.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return None;
        }
        let mut values = samples.to_vec();
        values.sort_by(f64::total_cmp);
        let n = values.len();
        let median = if n.is_multiple_of(2) {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        } else {
            values[n / 2]
        };
        let index = n - n / 10 - 1;
        Some(Self {
            count: u64::try_from(n).ok()?,
            median,
            p90: values[index],
        })
    }
}
wire! {
    /// Categorical evidence, never a numeric state code.
    SourceObservation {
        /// Source category without inferred ordering.
        category: String,
        /// Original source schema release.
        source_schema_version: String,
        /// Reference to the persisted source observation.
        observation_ref: String
    }
}
wire! {
    /// One measurement with independently retained evidence and availability.
    Measurement {
        /// Canonical catalog ID.
        id: String,
        /// Formula release.
        catalog_version: String,
        /// Complete successful read or unavailable.
        status: Status,
        /// Typed value, null when unavailable.
        value: Option<Value>,
        /// Unavailable reason.
        reason: Option<String>,
        /// Component unit.
        unit: String,
        /// Preferred numeric direction.
        direction: Direction,
        /// Native selected populations.
        scope: Scope,
        /// Normalized measurement and assessment identity.
        query_identity: String,
        /// Start of canonical (start,end] UTC window.
        window_start: String,
        /// End of UTC window, gauges evaluated here.
        window_end: String,
        /// Window duration, part of comparison identity.
        window_seconds: u64,
        /// Age of oldest required observation in seconds.
        source_age_seconds: Option<f64>,
        /// Read population, when known.
        population: Option<u64>,
        /// Ratio denominator, when applicable.
        denominator: Option<u64>,
        /// Native-key denominators for keyed ratios; counts retain their population separately.
        denominators: BTreeMap<String, u64>,
        /// Explicit configured counting parameters, persisted alongside their identity.
        query_parameters: Parameters,
        /// Resolving evidence/source references.
        evidence_refs: Vec<String>,
        /// Classified-subset denominator and complete send-population coverage.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classification_coverage: Option<ClassificationCoverage>
        ,
        /// Operational source selectors and configured resource policy used by this measurement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operational_parameters: Option<OperationalConfig>
    }
}
wire! {
    /// Shared version-one report used by collectors and consumers.
    Report {
        /// Envelope schema version.
        version: u32,
        /// Definition release.
        catalog_version: String,
        /// Assessment release.
        policy_id: String,
        /// Normalized configuration digest.
        config_identity: String,
        /// Explicit selected populations.
        scope: Scope,
        /// Persisted normalized evidence, including categorical source rows.
        observations: Vec<Observation>,
        /// All catalog measurements, including unavailable later collectors.
        measurements: BTreeMap<String, Measurement>,
        /// Stable detector outputs.
        loops: BTreeMap<String, LoopReport>,
        /// All ten canonical classes.
        classes: BTreeMap<String, ClassAssessment>,
        /// Department severity and independent completeness.
        rollup: Summary
    }
}

/// A local catalog definition, independent of legacy formulas.
#[derive(Clone, Copy, Debug)]
pub struct Definition {
    /// Stable metric ID.
    pub id: &'static str,
    /// Canonical class.
    pub class: &'static str,
    /// Component unit.
    pub unit: &'static str,
    /// Measurement shape used for validation.
    pub shape: Shape,
}
/// Numeric shape of a definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    /// Scalar count or gauge.
    Scalar,
    /// Keyed scalar count or gauge.
    Keyed,
    /// Keyed duration distribution.
    Durations,
    /// Source categorical evidence.
    Categorical,
}
macro_rules! catalog {
    ($($class:literal => [$($id:literal : $unit:literal / $shape:ident),* $(,)?]),* $(,)?) => {
        /// Complete canonical department metric catalog.
        pub const CATALOG: &[Definition] = &[$($(Definition { id: $id, class: $class, unit: $unit, shape: Shape::$shape }),*),*];
    };
}
catalog! {
 "flow_wip" => ["wip_active":"issues"/Scalar, "wip_capacity_ratio":"ratio"/Keyed, "in_progress_stale":"issues"/Scalar, "claims_without_movement":"issues"/Scalar, "merged_open":"issues"/Scalar, "idle_with_ready":"lanes"/Scalar, "state_cycle_hours":"hours"/Durations, "pr_without_reviewer_age_hours":"hours"/Durations],
 "serial_dependency" => ["open_chain_length":"edges"/Scalar, "oldest_gated_hours":"hours"/Scalar, "lane_fanin":"issues"/Keyed],
 "pin_version_churn" => ["crate_revision_count":"revisions"/Keyed, "pins_off_main":"pins"/Scalar, "repin_only_prs":"pull_requests"/Scalar, "tag_repin_events":"events"/Scalar],
 "capacity_resources" => ["host_slots_used":"units"/Keyed, "host_slots_free":"units"/Keyed, "host_overcommit_units":"units"/Keyed, "queue_wait_minutes":"minutes"/Durations, "lock_hold_minutes":"minutes"/Durations, "host_load_per_slot":"ratio"/Keyed, "disk_headroom_bytes":"bytes"/Keyed, "disk_available_bytes":"bytes"/Keyed],
 "liveness" => ["lane_execution_state":"category"/Categorical, "login_blocked_lanes":"lanes"/Scalar, "order_unacked_age_minutes":"minutes"/Durations, "lane_no_movement_hours":"hours"/Keyed],
 "authority_policy" => ["refusals_by_lane":"events"/Keyed, "frozen_denial_hours":"hours"/Keyed, "repeated_orders":"events"/Scalar],
 "signal_integrity" => ["reconciliation_agreement_ratio":"ratio"/Scalar, "reconciliation_unpaired":"entities"/Scalar, "source_age_hours":"hours"/Keyed, "instruction_version_mismatches":"claims"/Scalar, "relay_queued":"events"/Scalar, "relay_delivered":"events"/Scalar, "relay_acknowledged":"events"/Scalar, "prompt_timeout_unconfirmed":"events"/Scalar],
 "review_rework" => ["fix_rounds_max":"rounds"/Scalar, "fix_rounds_total":"rounds"/Scalar, "full_gates_max":"gates"/Scalar, "main_red_incidents":"incidents"/Scalar, "main_red_minutes":"minutes"/Scalar, "review_round_policy_excess":"pull_requests"/Scalar, "focused_check_runs":"runs"/Scalar, "workspace_check_runs":"runs"/Scalar, "gates_per_pr":"gates"/Keyed],
 "decision_latency" => ["decisions_open":"issues"/Scalar, "decision_oldest_hours":"hours"/Scalar, "ruling_waiting_issues":"issues"/Scalar, "spec_merge_waiting_issues":"issues"/Scalar],
 "comms_overhead" => ["messages_per_sender":"messages"/Keyed, "correction_rate":"ratio"/Scalar, "status_only_messages":"messages"/Scalar, "planner_ad_hoc_assignments":"assignments"/Scalar, "planner_scope_violations":"assignments"/Scalar, "planner_unbatched_sends":"messages"/Scalar]
}

/// Returns a definition without aliasing or changing old formulas.
pub fn definition(id: &str) -> Option<&'static Definition> {
    CATALOG.iter().find(|d| d.id == id)
}

impl Config {
    /// Rejects malformed policies before reading sources or writing stores.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("department.version: unsupported {}", self.version));
        }
        if self.policy_id.trim().is_empty() {
            return Err("department.policy_id: empty".into());
        }
        for (field, selectors) in [
            ("teams", &self.scope.teams),
            ("projects", &self.scope.projects),
            ("repositories", &self.scope.repositories),
            ("lanes", &self.scope.lanes),
            ("hosts", &self.scope.hosts),
        ] {
            let mut seen = BTreeSet::new();
            if selectors
                .iter()
                .any(|s| s.id.is_empty() || !seen.insert(&s.id))
            {
                return Err(format!(
                    "department.scope.{field}: empty or duplicate native id"
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for policy in &self.policies {
            let field = format!("department.policies.{}", policy.metric_id);
            if !seen.insert(&policy.metric_id) {
                return Err(format!("{field}: duplicate metric_id"));
            }
            let d = definition(&policy.metric_id)
                .ok_or_else(|| format!("{field}.metric_id: unknown catalog ID"))?;
            if d.shape == Shape::Categorical {
                return Err(format!("{field}: categorical evidence only"));
            }
            let valid_stat = match d.shape {
                Shape::Scalar | Shape::Keyed => policy.statistic == Statistic::Value,
                Shape::Durations => policy.statistic != Statistic::Value,
                Shape::Categorical => false,
            };
            if !valid_stat {
                return Err(format!(
                    "{field}.statistic: invalid for measurement variant"
                ));
            }
            let unit = if policy.statistic == Statistic::Count {
                "count"
            } else {
                d.unit
            };
            if policy.unit != unit {
                return Err(format!("{field}.unit: expected {unit}"));
            }
            let keyed = matches!(d.shape, Shape::Keyed | Shape::Durations);
            if keyed && (policy.expected_keys.is_empty() || policy.key_reduction.is_none()) {
                return Err(format!("{field}.expected_keys/key_reduction: required"));
            }
            if !keyed && (!policy.expected_keys.is_empty() || policy.key_reduction.is_some()) {
                return Err(format!("{field}.key_reduction: scalar policy"));
            }
            if !policy.amber.is_finite() || !policy.red.is_finite() {
                return Err(format!("{field}.thresholds: nonfinite"));
            }
            let ordered = match policy.comparator {
                Comparator::Gt | Comparator::Gte => policy.red >= policy.amber,
                Comparator::Lt | Comparator::Lte => policy.red <= policy.amber,
            };
            if !ordered {
                return Err(format!("{field}.thresholds: reversed amber/red"));
            }
            if policy.window_seconds == 0 {
                return Err(format!("{field}.window_seconds: must be positive"));
            }
        }
        let mut seen = BTreeSet::new();
        for class in &self.classes {
            if !CLASSES.contains(&class.id.as_str()) || !seen.insert(&class.id) {
                return Err(format!(
                    "department.classes.{}: unknown or duplicate class ID",
                    class.id
                ));
            }
            for id in class.required.union(&class.optional) {
                if definition(id).is_none() {
                    return Err(format!(
                        "department.classes.{}.metrics: unknown {id}",
                        class.id
                    ));
                }
            }
            if class
                .route
                .as_ref()
                .is_some_and(|r| r.owner.is_empty() || r.action.is_empty())
            {
                return Err(format!(
                    "department.classes.{}.route: owner/action required",
                    class.id
                ));
            }
        }
        if self.parameters.loop_repeat_count == Some(0) {
            return Err("department.parameters.loop_repeat_count: must be positive".into());
        }
        if let Some(operations) = &self.operations {
            operations.validate(&self.scope)?;
        }
        let mut sources = BTreeSet::new();
        let mut capabilities = BTreeSet::new();
        for input in &self.parameters.event_logs {
            if input.source.is_empty()
                || input.path.as_os_str().is_empty()
                || input.kinds.is_empty()
                || !sources.insert(&input.source)
            {
                return Err(
                    "department.parameters.event_logs: empty or duplicate source/path/kinds".into(),
                );
            }
            for kind in &input.kinds {
                if !capabilities.insert(kind) {
                    return Err(
                        "department.parameters.event_logs.kinds: duplicate capability owner".into(),
                    );
                }
            }
        }
        Ok(())
    }
    /// Canonical serialized identity; selector and policy ordering is immaterial.
    pub fn identity(&self) -> String {
        let mut normalized = self.clone();
        for selectors in [
            &mut normalized.scope.teams,
            &mut normalized.scope.projects,
            &mut normalized.scope.repositories,
            &mut normalized.scope.lanes,
            &mut normalized.scope.hosts,
        ] {
            selectors.sort_by(|a, b| a.id.cmp(&b.id));
            for selector in selectors {
                selector.label.clear();
            }
        }
        normalized
            .policies
            .sort_by(|a, b| a.metric_id.cmp(&b.metric_id));
        normalized.classes.sort_by(|a, b| a.id.cmp(&b.id));
        normalized
            .parameters
            .event_logs
            .sort_by(|a, b| a.source.cmp(&b.source));
        if let Some(operations) = &mut normalized.operations {
            operations
                .evidence_files
                .sort_by(|a, b| a.source.cmp(&b.source));
        }
        digest(&normalized)
    }
    /// Identity for one metric, excluding unrelated assessment membership and routes.
    pub fn measurement_identity(&self, id: &str, window: u64) -> String {
        let mut selected = self.clone();
        selected.classes.clear();
        selected.policies.retain(|p| p.metric_id == id);
        digest(&(CATALOG_VERSION, id, window, selected.identity()))
    }
}
fn digest(value: &impl Serialize) -> String {
    // All callers are finite, strict typed wire values.
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}
