// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic role-rule evaluation primitives (FR-047).

use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "eval/github.rs"]
mod github;
#[path = "eval/github_rules.rs"]
pub mod github_rules;
#[path = "eval/ledger.rs"]
mod ledger;
#[path = "eval/linear.rs"]
mod linear;
#[path = "eval/relay.rs"]
pub mod relay;
#[path = "eval/transcript.rs"]
pub mod transcript;

static PROMETHEUS_REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static EVAL_DEADLINE_CLOCK: crate::SystemClock = crate::SystemClock;

/// Maximum Linear GraphQL requests admitted by one default `retro eval` invocation.
pub const DEFAULT_EVAL_LINEAR_REQUEST_LIMIT: u64 = crate::linear::RETRO_EVAL_LINEAR_REQUEST_LIMIT;

/// Shared deadline and Linear request scope for one `retro eval` invocation.
///
/// Keep this guard alive for the full operation. New `LinearGraphQlSource` instances created on
/// the calling thread automatically inherit its shared request budget, and cloned sources keep
/// that same budget when used by worker threads.
pub struct EvalRunScope {
    deadline: crate::Deadline<'static>,
    linear_budget: std::sync::Arc<crate::linear::LinearRequestBudget>,
    _linear_scope: crate::linear::LinearRequestScope,
}

impl EvalRunScope {
    /// Starts a bounded evaluation and installs the shared Linear request cap.
    #[implements("FR-047-AC-9")]
    pub fn start(
        deadline_limit: Duration,
        linear_request_limit: u64,
    ) -> Result<Self, crate::Error> {
        let deadline = crate::Deadline::start(&EVAL_DEADLINE_CLOCK, deadline_limit);
        let linear_budget =
            crate::linear::LinearRequestBudget::within(&deadline, linear_request_limit)?;
        let linear_scope =
            crate::linear::LinearRequestScope::enter(std::sync::Arc::clone(&linear_budget));
        Ok(Self {
            deadline,
            linear_budget,
            _linear_scope: linear_scope,
        })
    }

    /// Returns the operation deadline for source reads and stage-level checks.
    #[implements("FR-047-AC-9")]
    pub fn deadline(&self) -> &crate::Deadline<'static> {
        &self.deadline
    }

    /// Returns time remaining or the typed timeout error after expiry.
    #[implements("FR-047-AC-9")]
    pub fn remaining(&self) -> Result<Duration, crate::Error> {
        self.deadline.remaining()
    }

    /// Human-readable wall-clock expiry reason for unavailable partial results.
    #[implements("FR-047-AC-9")]
    pub fn deadline_reason(&self) -> Option<String> {
        self.linear_budget.deadline_reason()
    }

    /// Number of Linear requests still available in this invocation.
    #[implements("FR-047-AC-9")]
    pub fn remaining_linear_requests(&self) -> u64 {
        self.linear_budget.remaining_requests()
    }

    /// Human-readable Linear request-cap reason for unavailable partial results.
    #[implements("FR-047-AC-9")]
    pub fn request_budget_reason(&self) -> Option<String> {
        self.linear_budget.request_budget_reason()
    }
}

/// Request totals reported by one `retro eval` invocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SourceRequestCounts {
    /// Linear GraphQL HTTP requests attempted.
    pub linear_http_requests: u64,
    /// `gh api` commands spawned; the CLI may retry an HTTP request internally.
    pub github_cli_calls: u64,
    /// GitHub page responses observed; failed paginated commands can leave this as a lower bound.
    pub github_pages_observed: u64,
    /// Queries submitted through the shared team-metrics GitHub guard.
    pub github_guard_calls: u64,
    /// Prometheus HTTP requests attempted.
    pub prometheus_http_requests: u64,
}

/// Snapshot of process request counters used to report one evaluation's source cost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RequestCountSnapshot {
    linear_http_requests: u64,
    github_cli_calls: u64,
    github_pages_observed: u64,
    github_guard_calls: u64,
    prometheus_http_requests: u64,
}

/// Captures current source request counters before evaluation begins.
#[implements("FR-047-AC-9")]
pub fn request_count_snapshot() -> RequestCountSnapshot {
    let (github_cli_calls, github_pages_observed) = super::live::github_request_counts();
    RequestCountSnapshot {
        linear_http_requests: crate::linear::request_count(),
        github_cli_calls,
        github_pages_observed,
        github_guard_calls: github::request_count(),
        prometheus_http_requests: PROMETHEUS_REQUESTS.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Returns source request totals made since `start`.
#[implements("FR-047-AC-9")]
pub fn request_counts_since(start: RequestCountSnapshot) -> SourceRequestCounts {
    let end = request_count_snapshot();
    SourceRequestCounts {
        linear_http_requests: end
            .linear_http_requests
            .saturating_sub(start.linear_http_requests),
        github_cli_calls: end.github_cli_calls.saturating_sub(start.github_cli_calls),
        github_pages_observed: end
            .github_pages_observed
            .saturating_sub(start.github_pages_observed),
        github_guard_calls: end
            .github_guard_calls
            .saturating_sub(start.github_guard_calls),
        prometheus_http_requests: end
            .prometheus_http_requests
            .saturating_sub(start.prometheus_http_requests),
    }
}

/// A parsed deterministic or judgement-only rule from a role specification.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationRule {
    pub id: String,
    pub rule: String,
    pub evidence_source: String,
    pub trigger: String,
    pub method: String,
    pub query_or_rubric: String,
    pub comparator: String,
    pub target: f64,
    pub unit: String,
}

/// An objective metric declaration from a role specification's Metrics table.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectiveMetricSpec {
    pub id: String,
    pub query: String,
    pub unit: String,
    pub target: f64,
    pub direction: String,
}

/// A role and all rules in its `Evaluations` table.
#[derive(Clone, Debug, PartialEq)]
pub struct RoleSpec {
    pub role: String,
    pub metrics: Vec<ObjectiveMetricSpec>,
    pub rules: Vec<EvaluationRule>,
}

/// Outcome returned by a registered metric adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryValue {
    Value {
        value: f64,
        evidence_refs: Vec<String>,
    },
    NotApplicable {
        reason: String,
        evidence_refs: Vec<String>,
    },
    Unavailable {
        reason: String,
        evidence_refs: Vec<String>,
    },
}

/// Stable machine-readable status of a rule evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    Pass,
    Fail,
    NotApplicable,
    Unavailable,
    JudgementRequired,
}

/// One deterministic or judgement-only role rule result.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuleResult {
    pub id: String,
    pub query: String,
    pub status: RuleStatus,
    pub value: Option<f64>,
    pub comparator: String,
    pub target: f64,
    pub evidence_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rubric_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judgement_required: Option<bool>,
}

/// A pass-rate summary that excludes incomplete and non-applicable rows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Score {
    pub passed: usize,
    pub evaluated: usize,
    pub pass_rate: Option<f64>,
}

/// Evaluator settings stored separately from the retro snapshot configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalSettings {
    #[serde(default = "default_role_spec_dir")]
    pub role_spec_dir: PathBuf,
    #[serde(default = "default_review_age_limit_hours")]
    pub review_age_limit_hours: u64,
    #[serde(default)]
    pub gate_commands: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub ready_state: Option<String>,
    /// Shared team-metrics GitHub journal used to enforce the exporter quota.
    #[serde(default)]
    pub github_guard_state_path: Option<PathBuf>,
    /// Maximum bytes allowed for the shared GitHub journal.
    #[serde(default = "default_github_guard_max_bytes")]
    pub github_guard_max_bytes: u64,
}

#[implements("FR-047-AC-4")]
fn default_github_guard_max_bytes() -> u64 {
    64 * 1024 * 1024
}

impl Default for EvalSettings {
    fn default() -> Self {
        Self {
            role_spec_dir: default_role_spec_dir(),
            review_age_limit_hours: default_review_age_limit_hours(),
            gate_commands: BTreeMap::new(),
            ready_state: None,
            github_guard_state_path: None,
            github_guard_max_bytes: default_github_guard_max_bytes(),
        }
    }
}

/// The start and exclusive end of an event-time window, in Unix milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvalWindow {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Team-owned session addresses used to scope role-level lane-order evidence.
/// Addresses must come from the selected team's verified session configuration and
/// session-turn records; fleet lane names are not inferred from labels or prefixes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaneOrderScope {
    /// Stable id/name of the selected team; required to enable scoped reductions.
    pub team_id: Option<String>,
    /// Explicit lane/session addresses owned by this team, grouped by evaluator role.
    pub sessions_by_role: BTreeMap<String, BTreeSet<String>>,
}

/// Current and preceding window results for a role.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RoleResult {
    pub role: String,
    pub objective_metrics: Vec<ObjectiveMetricResult>,
    pub score: Score,
    pub previous_score: Score,
    pub trend: Option<f64>,
    pub trend_matched_rules: usize,
    pub rules: Vec<RuleResult>,
    pub failing_rules: Vec<String>,
    pub session_ids: Vec<String>,
}

/// Session-specific evaluation rows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionResult {
    pub id: String,
    pub role: String,
    pub objective_metrics: Vec<ObjectiveMetricResult>,
    pub score: Score,
    pub rules: Vec<RuleResult>,
    pub failing_rules: Vec<String>,
}

/// Evaluated objective metric included in a role or session row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObjectiveMetricResult {
    pub id: String,
    pub query: String,
    pub value: Option<f64>,
    pub status: RuleStatus,
    pub unit: String,
    pub evidence_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// JSON contract for `retro eval`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvalReport {
    pub schema_version: u32,
    pub team: String,
    pub window: EvalWindow,
    pub previous_window: EvalWindow,
    pub roles: Vec<RoleResult>,
    pub sessions: Vec<SessionResult>,
    /// Number of Linear, GitHub and Prometheus requests attempted during evaluation.
    pub source_requests: SourceRequestCounts,
}

/// Keeps only session rows selected for presentation.
#[implements("FR-047-AC-7")]
pub fn filter_sessions(
    mut sessions: Vec<SessionResult>,
    session_filter: Option<&str>,
) -> Vec<SessionResult> {
    if let Some(session_id) = session_filter {
        sessions.retain(|session| session.id == session_id);
    }
    sessions
}

/// Reads the `hooks.role_turns` rule from session-attributed relay prompt rows.
#[implements("FR-047-AC-5")]
pub fn session_turn_adapters(
    spec: &RoleSpec,
    rows: Result<&[serde_json::Map<String, serde_json::Value>], &str>,
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let query = "hooks.role_turns";
    if !spec
        .rules
        .iter()
        .any(|rule| rule.method == "deterministic" && rule.query_or_rubric == query)
    {
        return BTreeMap::new();
    }
    let value = match rows {
        Ok(rows) => {
            let matching = rows
                .iter()
                .filter_map(|row| {
                    let started_at = row.get("started_at").and_then(serde_json::Value::as_i64)?;
                    let (Some(address), Some(role)) = (
                        row.get("session_addr").and_then(serde_json::Value::as_str),
                        row.get("session_role").and_then(serde_json::Value::as_str),
                    ) else {
                        return None;
                    };
                    (role == spec.role
                        && started_at >= window.start_ms
                        && started_at < window.end_ms)
                        .then(|| format!("fleet-ledger:session_turn:{address}:{started_at}"))
                })
                .collect::<Vec<_>>();
            QueryValue::Value {
                value: matching.len() as f64,
                evidence_refs: matching,
            }
        }
        Err(reason) => QueryValue::Unavailable {
            reason: format!("session_turn ledger view unavailable: {reason}"),
            evidence_refs: Vec::new(),
        },
    };
    BTreeMap::from([(query.to_owned(), value)])
}

/// Builds session rows only for relay turns with explicit role attribution.
#[implements("FR-047-AC-5")]
#[implements("FR-047-AC-7")]
pub fn evaluate_sessions(
    specs: &[RoleSpec],
    role_adapters: &BTreeMap<String, QueryValue>,
    ledger_rows: &[serde_json::Map<String, serde_json::Value>],
    window: &EvalWindow,
    session_filter: Option<&str>,
) -> Vec<SessionResult> {
    let mut by_role = BTreeMap::<String, BTreeSet<String>>::new();
    for row in ledger_rows {
        let Some(started_at) = row.get("started_at").and_then(serde_json::Value::as_i64) else {
            continue;
        };
        if started_at < window.start_ms || started_at >= window.end_ms {
            continue;
        }
        let (Some(address), Some(role)) = (
            row.get("session_addr").and_then(serde_json::Value::as_str),
            row.get("session_role").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        by_role
            .entry(role.to_owned())
            .or_default()
            .insert(address.to_owned());
    }
    let mut sessions = Vec::new();
    for spec in specs {
        let role_sessions = by_role.get(&spec.role).cloned().unwrap_or_default();
        for session_id in role_sessions {
            if session_filter.is_some_and(|selected| selected != session_id) {
                continue;
            }
            let mut adapters = BTreeMap::new();
            for rule in &spec.rules {
                if rule.method != "deterministic" || adapters.contains_key(&rule.query_or_rubric) {
                    continue;
                }
                let query = &rule.query_or_rubric;
                let value = if query == "hooks.role_turns" {
                    let evidence_refs = ledger_rows
                        .iter()
                        .filter_map(|row| {
                            let started_at =
                                row.get("started_at").and_then(serde_json::Value::as_i64)?;
                            let address = row
                                .get("session_addr")
                                .and_then(serde_json::Value::as_str)?;
                            let role = row
                                .get("session_role")
                                .and_then(serde_json::Value::as_str)?;
                            (address == session_id
                                && role == spec.role
                                && started_at >= window.start_ms
                                && started_at < window.end_ms)
                                .then(|| {
                                    format!("fleet-ledger:session_turn:{address}:{started_at}")
                                })
                        })
                        .collect::<Vec<_>>();
                    QueryValue::Value {
                        value: evidence_refs.len() as f64,
                        evidence_refs,
                    }
                } else if role_sessions_count(&by_role, &spec.role) == 1
                    && (query.starts_with("retro.") || query.starts_with("prom."))
                {
                    role_adapters
                        .get(query)
                        .cloned()
                        .unwrap_or_else(|| QueryValue::Unavailable {
                            reason: format!("{query}: no team-level adapter value for this window"),
                            evidence_refs: Vec::new(),
                        })
                } else {
                    QueryValue::Unavailable {
                        reason: format!(
                            "{query}: session-specific evidence is unavailable for role-attributed relay session {session_id}"
                        ),
                        evidence_refs: Vec::new(),
                    }
                };
                adapters.insert(query.clone(), value);
            }
            let rules = evaluate_rules(&spec.rules, &adapters);
            sessions.push(SessionResult {
                id: session_id,
                role: spec.role.clone(),
                objective_metrics: Vec::new(),
                score: score(&rules),
                failing_rules: rules
                    .iter()
                    .filter(|rule| rule.status == RuleStatus::Fail)
                    .map(|rule| rule.id.clone())
                    .collect(),
                rules,
            });
        }
    }
    sessions.sort_by(|left, right| (&left.role, &left.id).cmp(&(&right.role, &right.id)));
    sessions
}

#[implements("FR-047-AC-5")]
fn role_sessions_count(roles: &BTreeMap<String, BTreeSet<String>>, role: &str) -> usize {
    roles.get(role).map_or(0, BTreeSet::len)
}

#[implements("FR-047-AC-4")]
fn default_review_age_limit_hours() -> u64 {
    1
}
impl EvalSettings {
    /// Gate command words configured for one repository, with FR-017's default.
    #[implements("FR-047-AC-4")]
    pub fn gate_commands_for(&self, repository: &str) -> Vec<String> {
        self.gate_commands
            .get(repository)
            .cloned()
            .unwrap_or_else(|| vec!["make".to_owned(), "ci".to_owned()])
    }
}

/// Parses the role's evaluation table and rejects malformed rules instead of omitting them.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-2")]
pub fn parse_role_spec(path: &Path, markdown: &str) -> Result<RoleSpec, String> {
    let role = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid role spec path {}", path.display()))?
        .to_owned();
    let mut in_table = false;
    let mut header_seen = false;
    let mut rules = Vec::new();
    let mut in_metrics = false;
    let mut metric_header_seen = false;
    let mut metrics = Vec::new();
    for (line_index, line) in markdown.lines().enumerate() {
        if line.trim() == "## Metrics" {
            in_metrics = true;
            metric_header_seen = false;
            continue;
        }
        if line.trim() == "## Evaluations" {
            in_table = true;
            in_metrics = false;
            continue;
        }
        if in_table && line.starts_with("## ") {
            break;
        }
        if in_metrics && line.starts_with("## ") {
            in_metrics = false;
        }
        if in_metrics && line.trim_start().starts_with('|') {
            let cells = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>();
            if !metric_header_seen {
                metric_header_seen = true;
                continue;
            }
            if cells
                .iter()
                .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
            {
                continue;
            }
            if cells.len() != 7 {
                return Err(format!(
                    "{}:{}: metric row has {} cells, expected 7",
                    path.display(),
                    line_index + 1,
                    cells.len()
                ));
            }
            let target = cells[5].parse::<f64>().map_err(|_| {
                format!(
                    "{}:{}: invalid metric target {:?}",
                    path.display(),
                    line_index + 1,
                    cells[5]
                )
            })?;
            if !target.is_finite() {
                return Err(format!(
                    "{}:{}: metric target must be finite",
                    path.display(),
                    line_index + 1
                ));
            }
            let direction = cells[6].to_ascii_lowercase();
            if direction != "higher" && direction != "lower" {
                return Err(format!(
                    "{}:{}: unknown metric direction {:?}",
                    path.display(),
                    line_index + 1,
                    cells[6]
                ));
            }
            metrics.push(ObjectiveMetricSpec {
                id: cells[0].to_owned(),
                query: cells[3].to_owned(),
                unit: cells[4].to_owned(),
                target,
                direction,
            });
            continue;
        }
        if !in_table || !line.trim_start().starts_with('|') {
            continue;
        }
        let cells = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        if !header_seen {
            header_seen = true;
            continue;
        }
        if cells
            .iter()
            .all(|cell| cell.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }
        if cells.len() != 11 {
            return Err(format!(
                "{}:{}: evaluation row has {} cells, expected 11",
                path.display(),
                line_index + 1,
                cells.len()
            ));
        }
        let method = cells[4].to_ascii_lowercase();
        if method != "deterministic" && method != "judgement" {
            return Err(format!(
                "{}:{}: unknown method {:?}",
                path.display(),
                line_index + 1,
                cells[4]
            ));
        }
        let target = cells[8].parse::<f64>().map_err(|_| {
            format!(
                "{}:{}: invalid target {:?}",
                path.display(),
                line_index + 1,
                cells[8]
            )
        })?;
        if !target.is_finite() {
            return Err(format!(
                "{}:{}: target must be finite",
                path.display(),
                line_index + 1
            ));
        }
        rules.push(EvaluationRule {
            id: cells[0].to_owned(),
            rule: cells[1].to_owned(),
            evidence_source: cells[2].to_owned(),
            trigger: cells[3].to_owned(),
            method,
            query_or_rubric: cells[5].to_owned(),
            comparator: cells[7].to_owned(),
            target,
            unit: cells[9].to_owned(),
        });
    }
    if !in_table || !header_seen {
        return Err(format!("{}: missing Evaluations table", path.display()));
    }
    if rules.is_empty() {
        return Err(format!(
            "{}: Evaluations table has no rules",
            path.display()
        ));
    }
    Ok(RoleSpec {
        role,
        metrics,
        rules,
    })
}

/// Loads every markdown role spec in stable path order.
#[implements("FR-047-AC-1")]
pub fn read_role_specs(directory: &Path) -> Result<Vec<RoleSpec>, String> {
    let mut paths = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "md"));
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let markdown = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            parse_role_spec(&path, &markdown)
        })
        .collect()
}

/// One bounded query result per rule, retaining unknown query references as unavailable rows.
#[implements("FR-047-AC-1")]
pub fn evaluate_rules(
    rules: &[EvaluationRule],
    adapters: &BTreeMap<String, QueryValue>,
) -> Vec<RuleResult> {
    rules
        .iter()
        .map(|rule| {
            if rule.method == "judgement" {
                return RuleResult {
                    id: rule.id.clone(),
                    query: rule.query_or_rubric.clone(),
                    status: RuleStatus::JudgementRequired,
                    value: None,
                    comparator: rule.comparator.clone(),
                    target: rule.target,
                    evidence_refs: Vec::new(),
                    reason: None,
                    rubric_id: Some(rule.query_or_rubric.clone()),
                    judgement_required: Some(true),
                };
            }
            let value = adapters.get(&rule.query_or_rubric);
            let (status, observed, evidence_refs, reason) = match value {
                Some(QueryValue::Value {
                    value,
                    evidence_refs,
                }) => (
                    compare(*value, &rule.comparator, rule.target).map_or(
                        RuleStatus::Unavailable,
                        |passed| {
                            if passed {
                                RuleStatus::Pass
                            } else {
                                RuleStatus::Fail
                            }
                        },
                    ),
                    Some(*value),
                    evidence_refs.clone(),
                    compare(*value, &rule.comparator, rule.target)
                        .is_none()
                        .then(|| format!("unsupported comparator {:?}", rule.comparator)),
                ),
                Some(QueryValue::NotApplicable {
                    reason,
                    evidence_refs,
                }) => (
                    RuleStatus::NotApplicable,
                    None,
                    evidence_refs.clone(),
                    Some(reason.clone()),
                ),
                Some(QueryValue::Unavailable {
                    reason,
                    evidence_refs,
                }) => (
                    RuleStatus::Unavailable,
                    None,
                    evidence_refs.clone(),
                    Some(reason.clone()),
                ),
                None => (
                    RuleStatus::Unavailable,
                    None,
                    Vec::new(),
                    Some(format!(
                        "query reference {:?} has no registered producer",
                        rule.query_or_rubric
                    )),
                ),
            };
            RuleResult {
                id: rule.id.clone(),
                query: rule.query_or_rubric.clone(),
                status,
                value: observed,
                comparator: rule.comparator.clone(),
                target: rule.target,
                evidence_refs,
                reason,
                rubric_id: None,
                judgement_required: Some(false),
            }
        })
        .collect()
}

/// Produces objective metric rows with an explicit value or source reason.
#[implements("FR-047-AC-2")]
pub fn evaluate_objective_metrics(
    metrics: &[ObjectiveMetricSpec],
    adapters: &BTreeMap<String, QueryValue>,
) -> Vec<ObjectiveMetricResult> {
    metrics
        .iter()
        .map(|metric| {
            let (value, status, evidence_refs, reason) = match adapters.get(&metric.query) {
                Some(QueryValue::Value {
                    value,
                    evidence_refs,
                }) if value.is_finite() => {
                    let passed = match metric.direction.as_str() {
                        "higher" => *value >= metric.target,
                        "lower" => *value <= metric.target,
                        _ => false,
                    };
                    (
                        Some(*value),
                        if passed {
                            RuleStatus::Pass
                        } else {
                            RuleStatus::Fail
                        },
                        evidence_refs.clone(),
                        None,
                    )
                }
                Some(QueryValue::Value { .. }) => (
                    None,
                    RuleStatus::Unavailable,
                    Vec::new(),
                    Some("objective metric producer returned a non-finite value".to_owned()),
                ),
                Some(QueryValue::NotApplicable {
                    reason,
                    evidence_refs,
                }) => (
                    None,
                    RuleStatus::NotApplicable,
                    evidence_refs.clone(),
                    Some(reason.clone()),
                ),
                Some(QueryValue::Unavailable {
                    reason,
                    evidence_refs,
                }) => (
                    None,
                    RuleStatus::Unavailable,
                    evidence_refs.clone(),
                    Some(reason.clone()),
                ),
                None => (
                    None,
                    RuleStatus::Unavailable,
                    Vec::new(),
                    Some(format!(
                        "{}: objective metric query has no registered source adapter",
                        metric.query
                    )),
                ),
            };
            ObjectiveMetricResult {
                id: metric.id.clone(),
                query: metric.query.clone(),
                value,
                status,
                unit: metric.unit.clone(),
                evidence_refs,
                reason,
            }
        })
        .collect()
}

/// Marks role-level objective metrics unavailable when their value cannot be attributed to one
/// session without ambiguity.
pub fn unavailable_objective_metrics(
    metrics: &[ObjectiveMetricSpec],
    reason: &str,
) -> Vec<ObjectiveMetricResult> {
    metrics
        .iter()
        .map(|metric| ObjectiveMetricResult {
            id: metric.id.clone(),
            query: metric.query.clone(),
            value: None,
            status: RuleStatus::Unavailable,
            unit: metric.unit.clone(),
            evidence_refs: Vec::new(),
            reason: Some(reason.to_owned()),
        })
        .collect()
}

/// Adds objective metric queries to a clone of the source-selection specs.
#[implements("FR-047-AC-6")]
pub fn specs_with_objective_metric_queries(specs: &[RoleSpec]) -> Vec<RoleSpec> {
    specs
        .iter()
        .map(|spec| {
            let mut output = spec.clone();
            for metric in &spec.metrics {
                if output
                    .rules
                    .iter()
                    .any(|rule| rule.query_or_rubric == metric.query)
                {
                    continue;
                }
                output.rules.push(EvaluationRule {
                    id: format!("{}_objective", metric.id),
                    rule: "Objective metric source selection".into(),
                    evidence_source: "objective metric".into(),
                    trigger: "objective metric".into(),
                    method: "deterministic".into(),
                    query_or_rubric: metric.query.clone(),
                    comparator: if metric.direction == "higher" {
                        ">=".into()
                    } else {
                        "<=".into()
                    },
                    target: metric.target,
                    unit: metric.unit.clone(),
                });
            }
            output
        })
        .collect()
}

/// Creates the quota-guarded GitHub source used by the targeted retro snapshot reader.
#[implements("FR-047-AC-1")]
pub fn guarded_retro_github_source(
    state_path: Option<PathBuf>,
    max_bytes: u64,
) -> impl super::sources::GithubSource {
    github::GuardedRetroGithub {
        state_path,
        max_bytes,
    }
}

/// Computes a pass rate from only complete deterministic pass/fail results.
#[implements("FR-047-AC-2")]
pub fn score(results: &[RuleResult]) -> Score {
    let passed = results
        .iter()
        .filter(|row| row.status == RuleStatus::Pass)
        .count();
    let evaluated = results
        .iter()
        .filter(|row| matches!(row.status, RuleStatus::Pass | RuleStatus::Fail))
        .count();
    Score {
        passed,
        evaluated,
        pass_rate: (evaluated > 0).then_some(passed as f64 / evaluated as f64),
    }
}

/// Compares only deterministic rules with pass/fail results in both windows.
#[implements("FR-047-AC-3")]
pub fn trend_delta(current: &[RuleResult], previous: &[RuleResult]) -> (Option<f64>, usize) {
    let previous = previous
        .iter()
        .filter(|rule| {
            rule.judgement_required != Some(true)
                && matches!(rule.status, RuleStatus::Pass | RuleStatus::Fail)
        })
        .map(|rule| (rule.id.as_str(), rule.status == RuleStatus::Pass))
        .collect::<BTreeMap<_, _>>();
    let mut matched = 0_usize;
    let mut current_passed = 0_usize;
    let mut previous_passed = 0_usize;
    for rule in current.iter().filter(|rule| {
        rule.judgement_required != Some(true)
            && matches!(rule.status, RuleStatus::Pass | RuleStatus::Fail)
    }) {
        let Some(previous_passed_for_rule) = previous.get(rule.id.as_str()) else {
            continue;
        };
        matched += 1;
        current_passed += usize::from(rule.status == RuleStatus::Pass);
        previous_passed += usize::from(*previous_passed_for_rule);
    }
    let trend =
        (matched > 0).then(|| (current_passed as f64 - previous_passed as f64) / matched as f64);
    (trend, matched)
}

/// Reports a score only when every deterministic rule has complete evidence.
#[implements("FR-047-AC-3")]
pub fn complete_score(results: &[RuleResult]) -> Option<Score> {
    let deterministic = results
        .iter()
        .filter(|row| row.judgement_required != Some(true))
        .collect::<Vec<_>>();
    if deterministic.is_empty()
        || deterministic
            .iter()
            .any(|row| row.status == RuleStatus::Unavailable)
    {
        return None;
    }
    Some(score(results))
}

/// Maps complete retro catalog values whose snapshot time falls inside the evaluation window.
#[implements("FR-047-AC-1")]
pub fn retro_adapters(
    specs: &[RoleSpec],
    records: Result<&[super::Record], &str>,
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let refs = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic" && rule.query_or_rubric.starts_with("retro."))
        .map(|rule| rule.query_or_rubric.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut values = BTreeMap::new();
    for query in refs {
        let metric_id = query.strip_prefix("retro.").unwrap_or_default();
        let result = match records {
            Err(reason) => QueryValue::Unavailable {
                reason: reason.to_owned(),
                evidence_refs: Vec::new(),
            },
            Ok(records) => {
                let found = records.iter().rev().find_map(|record| {
                    let at_ms = crate::drift::timestamp_epoch(&record.taken_at)
                        .ok()
                        .and_then(|seconds| {
                            (seconds * 1000.0)
                                .is_finite()
                                .then_some((seconds * 1000.0) as i64)
                        })?;
                    if at_ms < window.start_ms || at_ms >= window.end_ms {
                        return None;
                    }
                    Some(retro_record_value(record, metric_id))
                });
                found.unwrap_or_else(|| QueryValue::Unavailable {
                    reason: format!("no {metric_id} snapshot falls inside this event-time window"),
                    evidence_refs: Vec::new(),
                })
            }
        };
        values.insert(query, result);
    }
    values
}

/// Adapts an in-memory, read-only retro collection without appending it to the store.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
pub fn retro_record_adapters(
    specs: &[RoleSpec],
    record: Result<&super::Record, &str>,
) -> BTreeMap<String, QueryValue> {
    let refs = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic" && rule.query_or_rubric.starts_with("retro."))
        .map(|rule| rule.query_or_rubric.clone())
        .collect::<std::collections::BTreeSet<_>>();
    refs.into_iter()
        .map(|query| {
            let result = match record {
                Err(reason) => QueryValue::Unavailable {
                    reason: reason.to_owned(),
                    evidence_refs: Vec::new(),
                },
                Ok(record) => {
                    let metric_id = query.strip_prefix("retro.").unwrap_or_default();
                    retro_record_value(record, metric_id)
                }
            };
            (query, result)
        })
        .collect()
}

#[implements("FR-047-AC-1")]
fn retro_record_value(record: &super::Record, metric_id: &str) -> QueryValue {
    if let Some(metric) = record.metrics.get(metric_id) {
        let evidence_refs = vec![format!("retro:{}:{}", metric.id, record.taken_at)];
        return match (metric.status, metric.value) {
            (super::Status::Ok, Some(value)) if value.is_finite() => QueryValue::Value {
                value,
                evidence_refs,
            },
            (super::Status::Ok, Some(_)) => QueryValue::Unavailable {
                reason: "retro metric is non-finite".into(),
                evidence_refs,
            },
            (super::Status::Ok, None) => QueryValue::Unavailable {
                reason: "retro metric has no numeric value".into(),
                evidence_refs,
            },
            (super::Status::Unavailable, _) => QueryValue::Unavailable {
                reason: metric
                    .reason
                    .clone()
                    .unwrap_or_else(|| "retro metric is unavailable".to_owned()),
                evidence_refs,
            },
        };
    }
    if metric_id == "order_flip_loop" {
        let evidence_ref = format!("retro:department-loop:order_flip_loop:{}", record.taken_at);
        if record.department.is_none() {
            return QueryValue::Unavailable {
                reason: "installed department producer needs TEST department configuration and one complete 24-hour snapshot window".into(),
                evidence_refs: vec![evidence_ref],
            };
        }
        return match record
            .department
            .as_ref()
            .and_then(|department| department.loops.get("order_flip_loop"))
        {
            Some(loop_report) if loop_report.status == super::Status::Ok => QueryValue::Value {
                value: loop_report.findings.len() as f64,
                evidence_refs: vec![evidence_ref],
            },
            Some(loop_report) => QueryValue::Unavailable {
                reason: loop_report
                    .reason
                    .clone()
                    .unwrap_or_else(|| "department loop detector is unavailable".to_owned()),
                evidence_refs: vec![evidence_ref],
            },
            None => QueryValue::Unavailable {
                reason: "retro department report has no order_flip_loop detector".into(),
                evidence_refs: vec![format!("retro:observed:{}", record.taken_at)],
            },
        };
    }
    let (department_id, max_keyed) = metric_id
        .strip_suffix("_max")
        .map_or((metric_id, false), |id| (id, true));
    let evidence_ref = format!("retro:department:{}:{}", department_id, record.taken_at);
    let measurement = record
        .department
        .as_ref()
        .and_then(|department| department.measurements.get(department_id));
    match measurement {
        Some(measurement) if measurement.status == super::Status::Ok => {
            let value = match measurement.value.as_ref() {
                Some(super::department::Value::Scalar(value)) if !max_keyed => Some(*value),
                Some(super::department::Value::Keyed(values)) if max_keyed => {
                    values.values().copied().reduce(f64::max)
                }
                Some(super::department::Value::Durations(values)) if max_keyed => values
                    .values()
                    .map(|distribution| distribution.p90)
                    .reduce(f64::max),
                _ => None,
            };
            match value.filter(|value| value.is_finite()) {
                Some(value) => QueryValue::Value {
                    value,
                    evidence_refs: vec![evidence_ref],
                },
                None => QueryValue::Unavailable {
                    reason: format!(
                        "department metric {department_id} has no numeric value for query {metric_id}"
                    ),
                    evidence_refs: vec![evidence_ref],
                },
            }
        }
        Some(measurement) => QueryValue::Unavailable {
            reason: measurement
                .reason
                .clone()
                .unwrap_or_else(|| "department metric is unavailable".to_owned()),
            evidence_refs: vec![evidence_ref],
        },
        None if record.department.is_none() && super::department::definition(department_id).is_some() => {
            QueryValue::Unavailable {
                reason: "installed department producer needs TEST department configuration and one complete 24-hour snapshot window".into(),
                evidence_refs: vec![evidence_ref],
            }
        }
        None => QueryValue::Unavailable {
            reason: format!("retro catalog and department report have no metric {metric_id:?}"),
            evidence_refs: vec![format!("retro:observed:{}", record.taken_at)],
        },
    }
}

/// Reads the fleet ready-buffer gauge as a time-weighted mean over an explicit window.
#[implements("FR-047-AC-1")]
pub fn prometheus_adapters(
    specs: &[RoleSpec],
    prometheus_url: Option<&str>,
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let needs_ready_buffer = specs.iter().flat_map(|spec| &spec.rules).any(|rule| {
        rule.method == "deterministic" && rule.query_or_rubric == "prom.ready_buffer_deficit_mean"
    });
    if !needs_ready_buffer {
        return BTreeMap::new();
    }
    let value = prometheus_url.map_or_else(
        || Err("Prometheus URL is not configured".to_owned()),
        |url| read_ready_buffer_mean(url, window),
    );
    BTreeMap::from([(
        "prom.ready_buffer_deficit_mean".to_owned(),
        match value {
            Ok(value) => QueryValue::Value {
                value,
                evidence_refs: vec![format!(
                    "prometheus:ix_fleet_ready_buffer_deficit:{}..{}",
                    window.start_ms, window.end_ms
                )],
            },
            Err(reason) => QueryValue::Unavailable {
                reason,
                evidence_refs: Vec::new(),
            },
        },
    )])
}

/// Counts distinct PRs with a reviewer marker or a merge in the selected window.
#[implements("FR-047-AC-1")]
pub fn linear_reviewed_prs_adapters(
    specs: &[RoleSpec],
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> BTreeMap<String, QueryValue> {
    let queries = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic")
        .map(|rule| rule.query_or_rubric.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let needs_reviewed_prs = queries.contains("linear.reviewed_prs");
    let needs_methods = queries.contains("linear.pr_methods_with_multiple_runs");
    if !needs_reviewed_prs && !needs_methods {
        return BTreeMap::new();
    }
    let mut values = BTreeMap::new();
    if needs_reviewed_prs {
        let value = read_reviewed_prs(
            repositories,
            window,
            github_guard_state_path,
            github_guard_max_bytes,
        );
        values.insert(
            "linear.reviewed_prs".to_owned(),
            match value {
                Ok((value, evidence_refs)) => QueryValue::Value {
                    value: value as f64,
                    evidence_refs,
                },
                Err(reason) => QueryValue::Unavailable {
                    reason,
                    evidence_refs: Vec::new(),
                },
            },
        );
    }
    if needs_methods {
        let value = read_pr_methods_with_multiple_runs(
            repositories,
            window,
            github_guard_state_path,
            github_guard_max_bytes,
        );
        values.insert(
            "linear.pr_methods_with_multiple_runs".to_owned(),
            match value {
                Ok((value, evidence_refs)) => QueryValue::Value {
                    value: value as f64,
                    evidence_refs,
                },
                Err(reason) => QueryValue::Unavailable {
                    reason,
                    evidence_refs: Vec::new(),
                },
            },
        );
    }
    values
}

/// Computes direct reviewer-marker metrics from the source comments' event times.
#[implements("FR-047-AC-1")]
pub fn linear_marker_adapters(
    specs: &[RoleSpec],
    repositories: &[String],
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let queries = [
        "linear.review_markers",
        "linear.review_markers_missing_fields",
        "linear.disposition_rounds",
        "linear.review_findings",
        "linear.findings_without_location_or_scenario",
        "linear.review_comments_unparseable_or_incomplete",
    ];
    let needed = queries
        .into_iter()
        .filter(|query| {
            specs
                .iter()
                .flat_map(|spec| &spec.rules)
                .any(|rule| rule.method == "deterministic" && rule.query_or_rubric == *query)
        })
        .collect::<BTreeSet<_>>();
    if needed.is_empty() {
        return BTreeMap::new();
    }
    let computed = read_linear_marker_metrics(repositories, window);
    needed
        .into_iter()
        .map(|query| {
            let value = match &computed {
                Ok(metrics) => {
                    metrics
                        .get(query)
                        .cloned()
                        .unwrap_or_else(|| QueryValue::Unavailable {
                            reason: format!("Linear marker adapter did not register {query}"),
                            evidence_refs: Vec::new(),
                        })
                }
                Err(reason) => QueryValue::Unavailable {
                    reason: reason.clone(),
                    evidence_refs: Vec::new(),
                },
            };
            (query.to_owned(), value)
        })
        .collect()
}

#[implements("FR-047-AC-1")]
fn read_linear_marker_metrics(
    repositories: &[String],
    window: &EvalWindow,
) -> Result<BTreeMap<String, QueryValue>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear review evidence".to_owned());
    }
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let start_secs = window.start_ms as f64 / 1000.0;
    let mut comments = Vec::new();
    for repository in repositories {
        for (kind, prefix) in [
            ("reviewer", "<!-- reviewer "),
            ("reviewer-dispositions", "<!-- reviewer-dispositions "),
        ] {
            let mut rows = linear.fetch_marker_comments(
                prefix,
                &format!("repo={repository} "),
                (kind == "reviewer-dispositions").then_some(start_secs),
            )?;
            comments.append(&mut rows);
        }
    }
    for comment in &comments {
        let created_at = comment
            .created_at
            .as_deref()
            .ok_or_else(|| "Linear marker has no source creation time".to_owned())?;
        crate::drift::timestamp_epoch(created_at)
            .map_err(|error| format!("Linear marker creation time is invalid: {error}"))?;
    }
    linear::review_marker_metrics(&comments, window)
}

#[implements("FR-047-AC-1")]
#[cfg(test)]
fn parse_linear_marker_metrics(
    comments: &[crate::MarkerComment],
    start_secs: f64,
    end_secs: f64,
) -> BTreeMap<String, QueryValue> {
    let mut marker_count = 0usize;
    let mut missing_fields = 0usize;
    let mut unparseable = 0usize;
    let mut rounds = BTreeSet::new();
    let mut findings = BTreeSet::new();
    let mut locationless = BTreeSet::new();
    let mut evidence = BTreeMap::<&str, Vec<String>>::new();
    let mut artifact_prs = BTreeMap::<(String, String), String>::new();
    for comment in comments {
        let first_line = comment.body.lines().next().unwrap_or("");
        let Some((kind, fields)) = super::marker_fields(first_line) else {
            let at = comment
                .created_at
                .as_deref()
                .and_then(|value| crate::drift::timestamp_epoch(value).ok());
            if at.is_some_and(|at| at >= start_secs && at < end_secs) {
                marker_count += 1;
                missing_fields += 1;
                unparseable += 1;
                if let Some(id) = comment.id.as_deref() {
                    evidence
                        .entry("linear.review_markers")
                        .or_default()
                        .push(format!("linear-comment:{id}"));
                }
            }
            continue;
        };
        let at = comment
            .created_at
            .as_deref()
            .and_then(|value| crate::drift::timestamp_epoch(value).ok());
        let in_window = at.is_some_and(|at| at >= start_secs && at < end_secs);
        if kind == "reviewer"
            && let (Some(repo), Some(id), Some(pr)) =
                (fields.get("repo"), fields.get("id"), fields.get("pr"))
            && let Ok(pr_key) = normalize_pr_key(repo, pr)
        {
            artifact_prs.insert(((*repo).to_owned(), (*id).to_owned()), pr_key);
        }
        if !in_window {
            continue;
        }
        marker_count += 1;
        if let Some(id) = comment.id.as_deref() {
            evidence
                .entry("linear.review_markers")
                .or_default()
                .push(format!("linear-comment:{id}"));
        }
        let required: &[&str] = if kind == "reviewer" {
            &["repo", "id", "method", "pr", "run", "head"]
        } else {
            &["repo", "id", "round", "run", "head", "mergeable"]
        };
        if required.iter().any(|key| !fields.contains_key(key)) {
            missing_fields += 1;
            if let Some(id) = comment.id.as_deref() {
                evidence
                    .entry("linear.review_markers_missing_fields")
                    .or_default()
                    .push(format!("linear-comment:{id}"));
            }
        }
        let data = yaml_data_block(&comment.body);
        let data_valid = data
            .as_ref()
            .is_ok_and(|value| marker_data_complete(kind, value));
        if required.iter().any(|key| !fields.contains_key(key)) || !data_valid {
            unparseable += 1;
            if let Some(id) = comment.id.as_deref() {
                evidence
                    .entry("linear.review_comments_unparseable_or_incomplete")
                    .or_default()
                    .push(format!("linear-comment:{id}"));
            }
        }
        let Some(data) = data.ok().filter(|_| data_valid) else {
            continue;
        };
        let (Some(repo), Some(id)) = (fields.get("repo"), fields.get("id")) else {
            continue;
        };
        if kind == "reviewer" {
            if let Some(rows) = data.get("findings").and_then(serde_json::Value::as_array) {
                for finding in rows {
                    let Some(fnd) = finding.get("fnd").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    findings.insert(((*repo).to_owned(), (*id).to_owned(), fnd.to_owned()));
                    if let Some(comment_id) = comment.id.as_deref() {
                        evidence
                            .entry("linear.review_findings")
                            .or_default()
                            .push(format!("linear-comment:{comment_id}"));
                    }
                    let path = finding
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let lines = finding
                        .get("lines")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let text = finding
                        .get("finding")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if (path.is_empty() && lines.is_empty()) || text.is_empty() {
                        locationless.insert(((*repo).to_owned(), (*id).to_owned(), fnd.to_owned()));
                        if let Some(comment_id) = comment.id.as_deref() {
                            evidence
                                .entry("linear.findings_without_location_or_scenario")
                                .or_default()
                                .push(format!("linear-comment:{comment_id}"));
                        }
                    }
                }
            }
        } else {
            if let Some(round) = fields
                .get("round")
                .and_then(|round| round.parse::<u64>().ok())
                && let Some(pr) = artifact_prs.get(&((*repo).to_owned(), (*id).to_owned()))
            {
                rounds.insert((pr.clone(), round));
                if let Some(comment_id) = comment.id.as_deref() {
                    evidence
                        .entry("linear.disposition_rounds")
                        .or_default()
                        .push(format!("linear-comment:{comment_id}"));
                }
            }
            if let Some(rows) = data
                .get("new_findings")
                .and_then(serde_json::Value::as_array)
            {
                for finding in rows {
                    if let Some(fnd) = finding.get("fnd").and_then(serde_json::Value::as_str) {
                        findings.insert(((*repo).to_owned(), (*id).to_owned(), fnd.to_owned()));
                    }
                }
            }
        }
    }
    for refs in evidence.values_mut() {
        refs.sort();
        refs.dedup();
    }
    let value = |count: usize, refs: &[String]| QueryValue::Value {
        value: count as f64,
        evidence_refs: refs.to_vec(),
    };
    BTreeMap::from([
        (
            "linear.review_markers".into(),
            value(
                marker_count,
                evidence
                    .get("linear.review_markers")
                    .map_or(&[], Vec::as_slice),
            ),
        ),
        (
            "linear.review_markers_missing_fields".into(),
            value(
                missing_fields,
                evidence
                    .get("linear.review_markers_missing_fields")
                    .map_or(&[], Vec::as_slice),
            ),
        ),
        (
            "linear.disposition_rounds".into(),
            value(
                rounds.len(),
                evidence
                    .get("linear.disposition_rounds")
                    .map_or(&[], Vec::as_slice),
            ),
        ),
        (
            "linear.review_findings".into(),
            value(
                findings.len(),
                evidence
                    .get("linear.review_findings")
                    .map_or(&[], Vec::as_slice),
            ),
        ),
        (
            "linear.findings_without_location_or_scenario".into(),
            value(
                locationless.len(),
                evidence
                    .get("linear.findings_without_location_or_scenario")
                    .map_or(&[], Vec::as_slice),
            ),
        ),
        (
            "linear.review_comments_unparseable_or_incomplete".into(),
            value(
                unparseable,
                evidence
                    .get("linear.review_comments_unparseable_or_incomplete")
                    .map_or(&[], Vec::as_slice),
            ),
        ),
    ])
}

#[implements("FR-047-AC-1")]
#[cfg(test)]
fn marker_data_complete(kind: &str, data: &serde_json::Value) -> bool {
    let has_fields = |row: &serde_json::Value, fields: &[&str]| {
        fields.iter().all(|field| row.get(*field).is_some())
    };
    let finding_fields = [
        "fnd",
        "severity",
        "confidence",
        "method",
        "check",
        "artifact_id",
        "path",
        "lines",
        "excerpt",
        "finding",
    ];
    match kind {
        "reviewer" => {
            data.get("clean")
                .and_then(serde_json::Value::as_bool)
                .is_some()
                && data
                    .get("scope")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
                && data
                    .get("findings")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|rows| rows.iter().all(|row| has_fields(row, &finding_fields)))
        }
        "reviewer-dispositions" => {
            let dispositions_complete = data
                .get("dispositions")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|rows| {
                    rows.iter().all(|row| {
                        let Some(outcome) = row.get("outcome").and_then(serde_json::Value::as_str)
                        else {
                            return false;
                        };
                        has_fields(row, &["fnd", "outcome"])
                            && match outcome {
                                "fixed" => row.get("after_excerpt").is_some(),
                                "rejected" | "deferred" | "accepted-no-change" | "still-open" => {
                                    row.get("reason").is_some()
                                }
                                _ => false,
                            }
                    })
                });
            let new_findings_complete = data.get("new_findings").is_none_or(|rows| {
                rows.as_array()
                    .is_some_and(|rows| rows.iter().all(|row| has_fields(row, &finding_fields)))
            });
            dispositions_complete && new_findings_complete
        }
        _ => false,
    }
}

#[implements("FR-047-AC-1")]
#[cfg(test)]
fn yaml_data_block(body: &str) -> Result<serde_json::Value, String> {
    let mut active = false;
    let mut width = 0;
    let mut block = String::new();
    let mut decoded = Vec::new();
    for line in body.lines() {
        let text = line.trim_start_matches(' ');
        let indent = line.len() - text.len();
        if indent <= 3 && text.starts_with("```") {
            let ticks = text.bytes().take_while(|byte| *byte == b'`').count();
            let tag = text.get(ticks..).unwrap_or("").trim();
            if !active && matches!(tag, "yaml" | "yml") {
                active = true;
                width = ticks;
                block.clear();
                continue;
            }
            if active && ticks >= width && tag.is_empty() {
                let parsed = serde_yaml::from_str::<serde_json::Value>(&block)
                    .map_err(|error| error.to_string());
                if let Ok(parsed) = parsed {
                    decoded.push(parsed);
                }
                active = false;
                continue;
            }
        }
        if active {
            block.push_str(line);
            block.push('\n');
        }
    }
    if decoded.len() != 1 {
        return Err("review marker must contain exactly one supported YAML block".to_owned());
    }
    decoded
        .pop()
        .ok_or_else(|| "review marker has no structured data".to_owned())
}

#[implements("FR-047-AC-1")]
fn read_reviewed_prs(
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> Result<(usize, Vec<String>), String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear review evidence".to_owned());
    }
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let start_secs = window.start_ms as f64 / 1000.0;
    let mut prs = Vec::new();
    let mut comments = Vec::new();
    for repository in repositories {
        prs.extend(guarded_pull_requests(
            repository,
            github_guard_state_path,
            github_guard_max_bytes,
        )?);
        comments.extend(linear.fetch_marker_comments(
            "<!-- reviewer ",
            &format!("repo={repository} "),
            Some(start_secs),
        )?);
    }
    let (count, evidence_refs) = reviewed_prs_count(&prs, &comments, window)?;
    if comments.iter().any(|comment| comment.created_at.is_none()) {
        return Err("review marker coverage includes a comment without a source timestamp".into());
    }
    Ok((count, evidence_refs))
}

fn reviewed_prs_count(
    prs: &[super::sources::PullRequest],
    comments: &[crate::MarkerComment],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    linear::reviewed_prs_count(prs, comments, window)
}

#[implements("FR-047-AC-1")]
fn read_pr_methods_with_multiple_runs(
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> Result<(usize, Vec<String>), String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear review evidence".to_owned());
    }
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let mut prs = Vec::new();
    let mut comments = Vec::new();
    for repository in repositories {
        prs.extend(guarded_pull_requests(
            repository,
            github_guard_state_path,
            github_guard_max_bytes,
        )?);
        comments.extend(linear.fetch_marker_comments(
            "<!-- reviewer ",
            &format!("repo={repository} "),
            None,
        )?);
    }
    linear::pr_methods_with_multiple_runs(&prs, &comments, window)
}

/// Evaluates the class-A Linear role queries from bounded tracker history and the guarded PR
/// journal. Use the `_with_github` variant when the invocation already has its complete role-query
/// evidence so marker reducers can reuse changed-file and commit-ancestry coverage.
#[implements("FR-047-AC-1")]
pub fn linear_class_a_adapters(
    specs: &[RoleSpec],
    repositories: &[String],
    decision_label: Option<&str>,
    window: &EvalWindow,
    _github_guard_state_path: Option<&Path>,
    _github_guard_max_bytes: u64,
) -> BTreeMap<String, QueryValue> {
    linear_class_a_adapters_with_github(
        specs,
        repositories,
        decision_label,
        window,
        Err("guarded GitHub role evidence was not supplied"),
    )
}

/// Evaluates class-A Linear queries against an already-collected complete GitHub evidence set.
///
/// The evidence is shared with GitHub role reducers, so changed-file and commit ancestry reads are
/// not repeated. Pass the source error through `Err` to keep affected values unavailable.
#[implements("FR-047-AC-1")]
pub fn linear_class_a_adapters_with_github(
    specs: &[RoleSpec],
    repositories: &[String],
    decision_label: Option<&str>,
    window: &EvalWindow,
    github_evidence: Result<&[github_rules::PullRequestEvidence], &str>,
) -> BTreeMap<String, QueryValue> {
    let rule_queries = [
        "linear.redundant_gate_runs",
        "linear.findings_open_at_merge",
        "linear.gate_runs_before_clean_review",
        "linear.fix_round_findings_returned_still_open",
        "linear.open_decisions_without_owner",
        "linear.findings_without_outcome_after_disposition",
        "linear.mergeable_yes_with_open_finding",
        "linear.outside_diff_findings_without_failure",
    ];
    let mut needed = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic")
        .map(|rule| rule.query_or_rubric.as_str())
        .filter(|query| rule_queries.contains(query))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let metric_needed = specs
        .iter()
        .flat_map(|spec| &spec.metrics)
        .any(|metric| metric.query == "linear.review_to_clean_median_hours");
    if metric_needed {
        needed.insert("linear.review_to_clean_median_hours".to_owned());
    }
    if needed.is_empty() {
        return BTreeMap::new();
    }

    let mut results = BTreeMap::new();
    let unavailable = |reason: String| QueryValue::Unavailable {
        reason,
        evidence_refs: Vec::new(),
    };
    let linear = match crate::LinearGraphQlSource::from_env() {
        Ok(linear) => linear,
        Err(error) => {
            return needed
                .into_iter()
                .map(|query| (query, unavailable(error.to_string())))
                .collect();
        }
    };

    if needed.contains("linear.open_decisions_without_owner") {
        let value = match decision_label.filter(|label| !label.trim().is_empty()) {
            Some(label) => linear
                .fetch_open_decisions_without_assignee(label)
                .map(|issues| QueryValue::Value {
                    value: issues.len() as f64,
                    evidence_refs: issues
                        .into_iter()
                        .map(|identifier| format!("linear-issue:{identifier}"))
                        .collect(),
                })
                .unwrap_or_else(unavailable),
            None => unavailable("retro config has no decision_label".to_owned()),
        };
        results.insert("linear.open_decisions_without_owner".to_owned(), value);
    }

    let reviewer_queries = [
        "linear.findings_open_at_merge",
        "linear.fix_round_findings_returned_still_open",
        "linear.findings_without_outcome_after_disposition",
        "linear.mergeable_yes_with_open_finding",
        "linear.outside_diff_findings_without_failure",
        "linear.review_to_clean_median_hours",
    ];
    let reviewer_needed = reviewer_queries.iter().any(|query| needed.contains(*query));
    if reviewer_needed {
        let comments_result = read_linear_review_comments(&linear, repositories);
        let final_fix_result = if needed.contains("linear.findings_open_at_merge") {
            read_linear_gate_comments(&linear, repositories)
        } else {
            Ok(Vec::new())
        };
        match comments_result {
            Ok(comments) => {
                let (final_fix_records, final_fix_error) = match final_fix_result {
                    Ok(records) => (records, None),
                    Err(reason) => (Vec::new(), Some(reason)),
                };
                let rows = github_evidence.as_ref().ok().copied().unwrap_or_default();
                let prs = pull_requests_from_github_evidence(rows);
                let changed_files = rows
                    .iter()
                    .map(|row| {
                        row.changed_files.as_ref().map(|files| {
                            (format!("{}#{}", row.repository, row.number), files.clone())
                        })
                    })
                    .collect::<Option<BTreeMap<_, _>>>();
                let values = linear::reviewer_class_a_metrics(
                    &comments,
                    &prs,
                    window,
                    changed_files.as_ref(),
                    &final_fix_records,
                );
                for query in reviewer_queries
                    .into_iter()
                    .filter(|query| needed.contains(*query))
                {
                    let requires_github = matches!(
                        query,
                        "linear.findings_open_at_merge"
                            | "linear.fix_round_findings_returned_still_open"
                    );
                    let value = if requires_github && github_evidence.is_err() {
                        unavailable(
                            github_evidence
                                .as_ref()
                                .err()
                                .copied()
                                .unwrap_or("GitHub evidence unavailable")
                                .to_owned(),
                        )
                    } else if query == "linear.findings_open_at_merge"
                        && let Some(reason) = final_fix_error.as_ref()
                    {
                        unavailable(format!("final-fix Linear source is unavailable: {reason}"))
                    } else {
                        values.get(query).cloned().unwrap_or_else(|| {
                            unavailable(format!("Linear reducer did not register {query}"))
                        })
                    };
                    results.insert(query.to_owned(), value);
                }
            }
            Err(reason) => {
                for query in reviewer_queries
                    .into_iter()
                    .filter(|query| needed.contains(*query))
                {
                    results.insert(query.to_owned(), unavailable(reason.clone()));
                }
            }
        }
    }

    let gate_queries = [
        "linear.redundant_gate_runs",
        "linear.gate_runs_before_clean_review",
    ];
    if gate_queries.iter().any(|query| needed.contains(*query)) {
        let gate_result = read_linear_gate_comments(&linear, repositories);
        let review_result = if needed.contains("linear.gate_runs_before_clean_review") {
            read_linear_review_comments(&linear, repositories)
        } else {
            Ok(Vec::new())
        };
        match (gate_result, review_result) {
            (Ok(gate_comments), Ok(review_comments)) => {
                match linear::gate_records(&gate_comments) {
                    Ok(records) => {
                        if needed.contains("linear.gate_runs_before_clean_review") {
                            let waived = waiver_prs(&gate_comments, "LEAD-R20");
                            let value = linear::gate_runs_before_clean_review(
                                &records,
                                &review_comments,
                                &waived,
                                window,
                            )
                            .map(|(value, evidence_refs)| QueryValue::Value {
                                value: value as f64,
                                evidence_refs,
                            })
                            .unwrap_or_else(unavailable);
                            results
                                .insert("linear.gate_runs_before_clean_review".to_owned(), value);
                        }
                        if needed.contains("linear.redundant_gate_runs") {
                            let waived = waiver_prs(&gate_comments, "LEAD-R06");
                            let value = match github_evidence.as_ref() {
                                Err(reason) => unavailable((*reason).to_owned()),
                                Ok(rows) => {
                                    match gate_head_ancestry_pairs(&records, rows, &waived, window)
                                    {
                                        Ok(covered_pairs) => {
                                            let (value, evidence_refs) =
                                                linear::redundant_gate_runs(
                                                    &records,
                                                    &covered_pairs,
                                                    &waived,
                                                    window,
                                                );
                                            QueryValue::Value {
                                                value: value as f64,
                                                evidence_refs,
                                            }
                                        }
                                        Err(reason) => unavailable(reason),
                                    }
                                }
                            };
                            results.insert("linear.redundant_gate_runs".to_owned(), value);
                        }
                    }
                    Err(reason) => {
                        for query in gate_queries
                            .into_iter()
                            .filter(|query| needed.contains(*query))
                        {
                            results.insert(query.to_owned(), unavailable(reason.clone()));
                        }
                    }
                }
            }
            (Err(reason), _) | (_, Err(reason)) => {
                for query in gate_queries
                    .into_iter()
                    .filter(|query| needed.contains(*query))
                {
                    results.insert(query.to_owned(), unavailable(reason.clone()));
                }
            }
        }
    }

    if needed.contains("linear.outside_diff_findings_without_failure")
        && let Err(reason) = github_evidence
    {
        results.insert(
            "linear.outside_diff_findings_without_failure".to_owned(),
            unavailable(reason.to_owned()),
        );
    }
    results
}

fn read_linear_review_comments(
    linear: &crate::LinearGraphQlSource,
    repositories: &[String],
) -> Result<Vec<crate::MarkerComment>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear review evidence".to_owned());
    }
    let mut comments = Vec::new();
    for repository in repositories {
        comments.extend(linear.fetch_marker_comments(
            "<!-- reviewer ",
            &format!("repo={repository} "),
            None,
        )?);
        comments.extend(linear.fetch_marker_comments(
            "<!-- reviewer-dispositions ",
            &format!("repo={repository} "),
            None,
        )?);
    }
    Ok(comments)
}

/// Reads complete Linear marker populations and builds the Linear-owned GitHub PR joins.
///
/// The result includes reviewer heads, successful gate heads, and known waivers. The Linear source
/// cannot establish file coverage, ticket-start state, or transcript dispatch, so those fields stay
/// unknown for the GitHub reducer.
#[implements("FR-047-AC-1")]
pub fn linear_pull_request_joins(
    repositories: &[String],
) -> Result<BTreeMap<(String, u64), github_rules::PullRequestJoins>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear PR joins".to_owned());
    }
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let comments = read_linear_join_comments(&linear, repositories)?;
    linear::pull_request_joins(
        &comments,
        true,
        &[],
        0,
        &EvalWindow {
            start_ms: 0,
            end_ms: 0,
        },
    )
}

/// Extends Linear marker joins to the already-read guarded GitHub PR population.
///
/// Supplying GitHub rows lets the complete Linear marker read distinguish an empty marker set from
/// missing data, derives reviewer timing evidence, and adds first Linear `started` state times for
/// tickets when `include_ticket_started_at` is selected.
#[implements("FR-047-AC-1")]
pub fn linear_pull_request_joins_for_evidence(
    repositories: &[String],
    github_rows: &[github_rules::PullRequestEvidence],
    include_ticket_started_at: bool,
    review_age_limit_hours: u64,
    window: &EvalWindow,
) -> Result<BTreeMap<(String, u64), github_rules::PullRequestJoins>, String> {
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let comments = read_linear_join_comments(&linear, repositories)?;
    let mut joins =
        linear::pull_request_joins(&comments, true, github_rows, review_age_limit_hours, window)?;
    if !include_ticket_started_at {
        return Ok(joins);
    }
    let mut ticket_starts = BTreeMap::<String, Option<i64>>::new();
    for row in github_rows {
        let join = joins
            .entry((row.repository.clone(), row.number))
            .or_default();
        join.reviewed_heads.get_or_insert_with(BTreeSet::new);
        join.gate_covered_heads.get_or_insert_with(BTreeSet::new);
        join.waived_for_gate.get_or_insert(false);
        join.waived_for_review.get_or_insert(false);
        join.ready_merge_waived.get_or_insert(false);
        let Some(ticket) = row.ticket.as_deref() else {
            continue;
        };
        if !ticket_starts.contains_key(ticket) {
            let start = match linear.fetch_ticket_started_at(ticket) {
                Ok(start) => start,
                Err(_) => break,
            };
            let millis = start
                .map(|seconds| seconds * 1000.0)
                .map(|millis| {
                    if !millis.is_finite() || millis < i64::MIN as f64 || millis > i64::MAX as f64 {
                        Err(format!(
                            "Linear start time is out of range for ticket {ticket}"
                        ))
                    } else {
                        Ok(millis.round() as i64)
                    }
                })
                .transpose()?;
            ticket_starts.insert(ticket.to_owned(), millis);
        }
        join.ticket_started_at_ms = ticket_starts.get(ticket).copied().flatten();
    }
    Ok(joins)
}

fn read_linear_join_comments(
    linear: &crate::LinearGraphQlSource,
    repositories: &[String],
) -> Result<Vec<crate::MarkerComment>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear PR joins".to_owned());
    }
    let mut comments = Vec::new();
    for repository in repositories {
        comments.extend(linear.fetch_marker_comments(
            "<!-- reviewer ",
            &format!("repo={repository} "),
            None,
        )?);
        comments.extend(linear.fetch_marker_comments(
            "<!-- lead-record ",
            &format!("repo={repository} "),
            None,
        )?);
    }
    Ok(comments)
}

fn read_linear_gate_comments(
    linear: &crate::LinearGraphQlSource,
    repositories: &[String],
) -> Result<Vec<crate::MarkerComment>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear gate evidence".to_owned());
    }
    let mut comments = Vec::new();
    for repository in repositories {
        comments.extend(linear.fetch_marker_comments(
            "<!-- lead-record ",
            &format!("repo={repository} "),
            None,
        )?);
    }
    Ok(comments)
}

fn waiver_prs(comments: &[crate::MarkerComment], rule: &str) -> BTreeSet<String> {
    comments
        .iter()
        .filter_map(|comment| {
            let fields = lead_record_fields(comment.body.lines().next().unwrap_or_default())?.1;
            if fields.get("kind") != Some(&"waiver") {
                return None;
            }
            let waived_rule = fields.get("covers")?;
            if *waived_rule != rule && !waived_rule.starts_with(&format!("{rule}-")) {
                return None;
            }
            let repo = fields.get("repo")?;
            let pr = fields.get("pr")?;
            linear::normalize_pr_key(repo, pr).ok()
        })
        .collect()
}

fn pull_requests_from_github_evidence(
    rows: &[github_rules::PullRequestEvidence],
) -> Vec<super::sources::PullRequest> {
    rows.iter()
        .map(|row| super::sources::PullRequest {
            closed_at: row.closed_at_ms.map(|at| at as f64 / 1000.0),
            reopen_events: None,
            repo: row.repository.clone(),
            number: row.number,
            open: row.merged_at_ms.is_none() && row.closed_at_ms.is_none(),
            draft: false,
            created_at: row.created_at_ms.map_or(0.0, |at| at as f64 / 1000.0),
            merged_at: row.merged_at_ms.map(|at| at as f64 / 1000.0),
            updated_at: row
                .merged_at_ms
                .or(row.closed_at_ms)
                .map_or(0.0, |at| at as f64 / 1000.0),
            additions: 0,
            deletions: 0,
            review_decision: None,
            force_pushes: None,
        })
        .collect()
}

fn gate_head_ancestry_pairs(
    records: &[linear::GateRecord],
    github_rows: &[github_rules::PullRequestEvidence],
    waived_prs: &BTreeSet<String>,
    window: &EvalWindow,
) -> Result<BTreeSet<(String, String, String)>, String> {
    let start = window.start_ms as f64 / 1000.0;
    let end = window.end_ms as f64 / 1000.0;
    let mut covered = BTreeSet::new();
    for current in records.iter().filter(|record| {
        record.created_at >= start && record.created_at < end && !waived_prs.contains(&record.pr)
    }) {
        let (repository, number) = current
            .pr
            .rsplit_once('#')
            .and_then(|(repo, number)| number.parse::<u64>().ok().map(|number| (repo, number)))
            .ok_or_else(|| format!("gate record has invalid PR key {:?}", current.pr))?;
        let row = github_rows
            .iter()
            .find(|row| row.repository == repository && row.number == number)
            .ok_or_else(|| format!("GitHub evidence is incomplete for gate PR {}", current.pr))?;
        let ancestry = row.commit_ancestry.as_ref().ok_or_else(|| {
            format!(
                "GitHub commit ancestry is incomplete for gate PR {}",
                current.pr
            )
        })?;
        for earlier in records.iter().filter(|earlier| {
            earlier.pr == current.pr && earlier.exit == 0 && earlier.started_at < current.started_at
        }) {
            if commit_is_ancestor(&earlier.head, &current.head, ancestry) {
                covered.insert((
                    current.pr.clone(),
                    earlier.head.clone(),
                    current.head.clone(),
                ));
            }
        }
    }
    Ok(covered)
}

fn commit_is_ancestor(
    ancestor: &str,
    descendant: &str,
    ancestry: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    if ancestor == descendant {
        return true;
    }
    let mut pending = vec![descendant.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(commit) = pending.pop() {
        if !visited.insert(commit.clone()) {
            continue;
        }
        for parent in ancestry.get(&commit).into_iter().flatten() {
            if parent == ancestor {
                return true;
            }
            pending.push(parent.clone());
        }
    }
    false
}

#[cfg(test)]
fn pr_methods_count(
    comments: &[crate::MarkerComment],
    allowed_prs: &std::collections::BTreeSet<String>,
) -> Result<(usize, Vec<String>), String> {
    let mut methods = BTreeMap::<(String, String), std::collections::BTreeSet<String>>::new();
    let mut evidence = Vec::new();
    for comment in comments {
        let first_line = comment.body.lines().next().unwrap_or_default();
        let Some(("reviewer", fields)) = super::marker_fields(first_line) else {
            return Err("Linear returned an unsupported reviewer marker".into());
        };
        let repo = fields
            .get("repo")
            .ok_or_else(|| "reviewer marker has no repo".to_owned())?;
        let pr = fields
            .get("pr")
            .ok_or_else(|| "reviewer marker has no PR".to_owned())?;
        let pr = normalize_pr_key(repo, pr)?;
        if !allowed_prs.contains(&pr) {
            continue;
        }
        let method = fields
            .get("method")
            .ok_or_else(|| "reviewer marker has no method".to_owned())?;
        let run = fields
            .get("run")
            .ok_or_else(|| "reviewer marker has no run".to_owned())?;
        methods
            .entry((pr, (*method).to_owned()))
            .or_default()
            .insert((*run).to_owned());
        if let Some(id) = comment.id.as_deref() {
            evidence.push(format!("linear-comment:{id}"));
        }
    }
    let count = methods.values().filter(|runs| runs.len() > 1).count();
    evidence.sort();
    evidence.dedup();
    Ok((count, evidence))
}

#[cfg(test)]
fn normalize_pr_key(repo: &str, pr: &str) -> Result<String, String> {
    let number = if pr.starts_with(&format!("{repo}#")) {
        pr.strip_prefix(&format!("{repo}#"))
    } else {
        let short_repo = repo.rsplit('/').next().unwrap_or(repo);
        pr.strip_prefix(&format!("{short_repo}#"))
    }
    .and_then(|number| number.parse::<u64>().ok())
    .ok_or_else(|| "reviewer marker PR does not match its repo".to_owned())?;
    Ok(format!("{repo}#{number}"))
}

/// Counts merged pull requests whose native merge time falls in the selected window.
#[implements("FR-047-AC-1")]
pub fn github_merged_prs_adapters(
    specs: &[RoleSpec],
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> BTreeMap<String, QueryValue> {
    let needed = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .any(|rule| rule.method == "deterministic" && rule.query_or_rubric == "github.merged_prs");
    if !needed {
        return BTreeMap::new();
    }
    let value = read_merged_prs(
        repositories,
        window,
        github_guard_state_path,
        github_guard_max_bytes,
    );
    BTreeMap::from([(
        "github.merged_prs".to_owned(),
        match value {
            Ok((value, evidence_refs)) => QueryValue::Value {
                value: value as f64,
                evidence_refs,
            },
            Err(reason) => QueryValue::Unavailable {
                reason,
                evidence_refs: Vec::new(),
            },
        },
    )])
}

/// Counts merged pull requests for which Linear has no review marker naming the PR.
#[implements("FR-047-AC-1")]
pub fn linear_merged_prs_without_review_adapters(
    specs: &[RoleSpec],
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> BTreeMap<String, QueryValue> {
    let query = "linear.merged_prs_without_review_record";
    if !specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .any(|rule| rule.method == "deterministic" && rule.query_or_rubric == query)
    {
        return BTreeMap::new();
    }
    let value = read_merged_prs_without_review(
        repositories,
        window,
        github_guard_state_path,
        github_guard_max_bytes,
    );
    BTreeMap::from([(
        query.to_owned(),
        match value {
            Ok((value, evidence_refs)) => QueryValue::Value {
                value: value as f64,
                evidence_refs,
            },
            Err(reason) => QueryValue::Unavailable {
                reason,
                evidence_refs: Vec::new(),
            },
        },
    )])
}

/// Evaluates code-lead gate records against the append-only gate log.
#[implements("FR-047-AC-6")]
pub fn linear_gate_record_adapters(
    specs: &[RoleSpec],
    repositories: &[String],
    gate_log: Option<&Path>,
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let queries = ["linear.gate_records", "linear.gate_runs_without_record"];
    let needed = queries
        .into_iter()
        .filter(|query| {
            specs
                .iter()
                .flat_map(|spec| &spec.rules)
                .any(|rule| rule.method == "deterministic" && rule.query_or_rubric == *query)
        })
        .collect::<BTreeSet<_>>();
    if needed.is_empty() {
        return BTreeMap::new();
    }
    let results = read_linear_gate_record_metrics(repositories, gate_log, window);
    needed
        .into_iter()
        .map(|query| {
            let value = match &results {
                Ok(values) => {
                    values
                        .get(query)
                        .cloned()
                        .unwrap_or_else(|| QueryValue::Unavailable {
                            reason: format!("gate-record adapter did not register {query}"),
                            evidence_refs: Vec::new(),
                        })
                }
                Err(reason) => QueryValue::Unavailable {
                    reason: reason.clone(),
                    evidence_refs: Vec::new(),
                },
            };
            (query.to_owned(), value)
        })
        .collect()
}

#[implements("FR-047-AC-6")]
fn read_linear_gate_record_metrics(
    repositories: &[String],
    gate_log: Option<&Path>,
    window: &EvalWindow,
) -> Result<BTreeMap<String, QueryValue>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for Linear gate records".to_owned());
    }
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let mut comments = Vec::new();
    for repository in repositories {
        comments.extend(linear.fetch_marker_comments(
            "<!-- lead-record ",
            &format!("repo={repository} "),
            None,
        )?);
    }
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let mut gate_records = Vec::new();
    let mut record_evidence = Vec::new();
    let mut recorded_finished = BTreeSet::new();
    for comment in &comments {
        let Some(("lead-record", fields)) =
            lead_record_fields(comment.body.lines().next().unwrap_or(""))
        else {
            continue;
        };
        if fields.get("kind") != Some(&"gate") {
            continue;
        }
        let finished = fields
            .get("finished")
            .ok_or_else(|| "gate record has no finished time".to_owned())?;
        let finished_at = crate::drift::timestamp_epoch(finished)
            .map_err(|error| format!("gate record finished time is invalid: {error}"))?;
        let repo = fields
            .get("repo")
            .ok_or_else(|| "gate record has no repo".to_owned())?;
        let pr = fields
            .get("pr")
            .ok_or_else(|| "gate record has no PR".to_owned())?;
        let short_repo = repo.rsplit('/').next().unwrap_or(repo);
        if !pr.starts_with(&format!("{repo}#")) && !pr.starts_with(&format!("{short_repo}#")) {
            return Err(format!(
                "gate record PR {pr:?} does not match repo {repo:?}"
            ));
        }
        let pr_number = pr
            .rsplit_once('#')
            .and_then(|(_, number)| number.parse::<u64>().ok())
            .ok_or_else(|| format!("gate record PR {pr:?} is invalid"))?;
        let pr = format!("{repo}#{pr_number}");
        let head = fields
            .get("head")
            .filter(|head| !head.is_empty())
            .ok_or_else(|| "gate record has no head".to_owned())?;
        let exit = fields
            .get("exit")
            .and_then(|exit| exit.parse::<i32>().ok())
            .ok_or_else(|| "gate record has no valid exit status".to_owned())?;
        let started = fields
            .get("started")
            .ok_or_else(|| "gate record has no started time".to_owned())?;
        crate::drift::timestamp_epoch(started)
            .map_err(|error| format!("gate record started time is invalid: {error}"))?;
        let _ = (head, exit);
        recorded_finished.insert((pr, (finished_at * 1000.0).round() as i64));
        let created = comment
            .created_at
            .as_deref()
            .ok_or_else(|| "gate record has no source creation time".to_owned())?;
        let created_at =
            crate::drift::timestamp_epoch(created).map_err(|error| error.to_string())?;
        if created_at >= start_secs && created_at < end_secs {
            gate_records.push(comment);
            if let Some(id) = comment.id.as_deref() {
                record_evidence.push(format!("linear-comment:{id}"));
            }
        }
    }
    record_evidence.sort();
    record_evidence.dedup();

    let gate_log = gate_log.ok_or_else(|| "retro config has no gate_log path".to_owned())?;
    use super::GateLogSource;
    let lines = super::live::FileGateLog
        .lines(gate_log)
        .map_err(|error| format!("gate log is unavailable: {error}"))?;
    let unrecorded = lines
        .iter()
        .filter(|line| !line.repin && line.finished_at >= start_secs && line.finished_at < end_secs)
        .filter(|line| !has_gate_record(line, &recorded_finished))
        .collect::<Vec<_>>();
    let unrecorded_evidence = unrecorded
        .iter()
        .map(|line| {
            format!(
                "gate-log:{}:{}",
                line.pr,
                crate::drift::format_timestamp(line.finished_at)
            )
        })
        .collect::<Vec<_>>();
    Ok(BTreeMap::from([
        (
            "linear.gate_records".into(),
            QueryValue::Value {
                value: gate_records.len() as f64,
                evidence_refs: record_evidence,
            },
        ),
        (
            "linear.gate_runs_without_record".into(),
            QueryValue::Value {
                value: unrecorded.len() as f64,
                evidence_refs: unrecorded_evidence,
            },
        ),
    ]))
}

#[implements("FR-047-AC-6")]
fn lead_record_fields(line: &str) -> Option<(&str, BTreeMap<&str, &str>)> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .trim_end()
        .strip_suffix("-->")?;
    let mut tokens = inner.split_whitespace();
    let kind = tokens.next()?;
    let fields = tokens.filter_map(|token| token.split_once('=')).collect();
    Some((kind, fields))
}

#[implements("FR-047-AC-6")]
fn has_gate_record(line: &super::GateLine, records: &BTreeSet<(String, i64)>) -> bool {
    let finished_ms = (line.finished_at * 1000.0).round() as i64;
    records.contains(&(line.pr.clone(), finished_ms))
}

#[implements("FR-047-AC-1")]
fn read_merged_prs_without_review(
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> Result<(usize, Vec<String>), String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for GitHub merge evidence".to_owned());
    }
    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    let mut prs = Vec::new();
    let mut comments = Vec::new();
    for repository in repositories {
        prs.extend(guarded_pull_requests(
            repository,
            github_guard_state_path,
            github_guard_max_bytes,
        )?);
        comments.extend(linear.fetch_marker_comments(
            "<!-- reviewer ",
            &format!("repo={repository} "),
            None,
        )?);
    }
    merged_prs_without_review_count(&prs, &comments, window)
}

#[implements("FR-047-AC-1")]
fn merged_prs_without_review_count(
    prs: &[super::sources::PullRequest],
    comments: &[crate::MarkerComment],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    linear::merged_prs_without_review_count(prs, comments, window)
}

/// Reads the SWM-261 fleet-ledger views without opening the store for writes.
#[implements("FR-047-AC-1")]
pub fn ledger_adapters(
    specs: &[RoleSpec],
    ledger_path: Option<&Path>,
    ready_state: Option<&str>,
    projects: &[String],
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    ledger_adapters_impl(specs, ledger_path, ready_state, projects, window, None)
}

/// Reads Fleet ledger values while restricting lane-order queries to explicit session
/// addresses belonging to this team.
#[implements("FR-047-AC-1")]
pub fn ledger_adapters_scoped(
    specs: &[RoleSpec],
    ledger_path: Option<&Path>,
    ready_state: Option<&str>,
    projects: &[String],
    window: &EvalWindow,
    lane_order_scope: &LaneOrderScope,
) -> BTreeMap<String, QueryValue> {
    ledger_adapters_impl(
        specs,
        ledger_path,
        ready_state,
        projects,
        window,
        Some(lane_order_scope),
    )
}

fn ledger_adapters_impl(
    specs: &[RoleSpec],
    ledger_path: Option<&Path>,
    ready_state: Option<&str>,
    projects: &[String],
    window: &EvalWindow,
    lane_order_scope: Option<&LaneOrderScope>,
) -> BTreeMap<String, QueryValue> {
    let path = ledger_path
        .map(Path::to_owned)
        .or_else(|| crate::fleet::ledger::default_path().ok());
    let coverage = path
        .as_deref()
        .ok_or_else(|| "fleet ledger path is unavailable".to_owned())
        .and_then(|path| ledger::read_coverage(path, window));
    let capacity_coverage = path
        .as_deref()
        .ok_or_else(|| "fleet ledger path is unavailable".to_owned())
        .and_then(|path| ledger::read_capacity_coverage(path, window));
    let mut views = ledger::ViewRows::new();
    let requested = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic")
        .map(|rule| rule.query_or_rubric.as_str())
        .filter_map(|query| query.strip_prefix("ledger."))
        .collect::<BTreeSet<_>>();
    for view in requested {
        if matches!(
            view,
            "idle_lane_minutes_during_build_pause"
                | "need_ready_tickets_due"
                | "need_ready_tickets_unanswered"
        ) {
            continue;
        }
        let query = format!("ledger.{view}");
        let view_coverage = if ledger::needs_capacity_coverage(&query) {
            capacity_coverage
                .as_ref()
                .map(|_| ())
                .map_err(String::as_str)
        } else {
            coverage.as_ref().map(|_| ()).map_err(String::as_str)
        };
        let rows = match (view_coverage, ledger::readable_view(&query)) {
            (Err(reason), _) => Err(reason.to_owned()),
            (Ok(()), Some(readable)) => path
                .as_deref()
                .ok_or_else(|| "fleet ledger path is unavailable".to_owned())
                .and_then(|path| {
                    ledger::read_view_rows(
                        path,
                        readable,
                        ledger::time_column(&format!("ledger.{view}")),
                        window,
                    )
                }),
            (Ok(()), None) => Ok(Vec::new()),
        };
        let rows = if matches!(view, "lane_orders_due" | "lane_orders_not_taken_up") {
            rows.and_then(|rows| ledger::scope_lane_order_rows(specs, view, rows, lane_order_scope))
        } else {
            rows
        };
        views.insert(view.to_owned(), rows);
    }
    let mut values = ledger::adapters(
        specs,
        &views,
        window,
        coverage.as_ref().map(|_| ()).map_err(String::as_str),
        capacity_coverage
            .as_ref()
            .map(|_| ())
            .map_err(String::as_str),
    );
    if specs.iter().flat_map(|spec| &spec.rules).any(|rule| {
        rule.method == "deterministic" && rule.query_or_rubric == "ledger.need_ready_tickets_due"
    }) {
        values.insert(
            "ledger.need_ready_tickets_due".to_owned(),
            match match &coverage {
                Ok(()) => read_need_ready_tickets_due(ledger_path, projects, window),
                Err(reason) => Err(reason.clone()),
            } {
                Ok((value, evidence_refs)) => QueryValue::Value {
                    value: value as f64,
                    evidence_refs,
                },
                Err(reason) => QueryValue::Unavailable {
                    reason,
                    evidence_refs: Vec::new(),
                },
            },
        );
    }
    if requested_is_pause_metric(specs) {
        values.insert(
            "ledger.idle_lane_minutes_during_build_pause".into(),
            QueryValue::NotApplicable {
                reason: "builds are never paused".into(),
                evidence_refs: Vec::new(),
            },
        );
    }
    if specs.iter().flat_map(|spec| &spec.rules).any(|rule| {
        rule.method == "deterministic"
            && rule.query_or_rubric == "ledger.need_ready_tickets_unanswered"
    }) {
        values.insert(
            "ledger.need_ready_tickets_unanswered".to_owned(),
            match match &coverage {
                Ok(()) => {
                    read_need_ready_tickets_unanswered(ledger_path, ready_state, projects, window)
                }
                Err(reason) => Err(reason.clone()),
            } {
                Ok((value, evidence_refs)) => QueryValue::Value {
                    value: value as f64,
                    evidence_refs,
                },
                Err(reason) => QueryValue::Unavailable {
                    reason,
                    evidence_refs: Vec::new(),
                },
            },
        );
    }
    values
}

/// Reads hook-attributed turn rows from the ledger without invoking the fleet
/// writer's exact-version migration/query path.
#[implements("FR-047-AC-5")]
pub fn session_turn_rows(
    ledger_path: &Path,
    window: &EvalWindow,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, String> {
    ledger::read_view_rows(ledger_path, "session_turn", "started_at", window)
}

/// Reports whether the installed read-only ledger schema has explicit transcript IDs.
#[implements("FR-047-AC-5")]
pub fn session_turn_has_transcript_id(ledger_path: &Path) -> Result<bool, String> {
    ledger::session_turn_has_transcript_id(ledger_path)
}

fn requested_is_pause_metric(specs: &[RoleSpec]) -> bool {
    specs.iter().flat_map(|spec| &spec.rules).any(|rule| {
        rule.method == "deterministic"
            && rule.query_or_rubric == "ledger.idle_lane_minutes_during_build_pause"
    })
}

#[implements("FR-047-AC-1")]
fn read_need_ready_tickets_due(
    ledger_path: Option<&Path>,
    projects: &[String],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    if projects.is_empty() {
        return Err("retro config has no projects for need_ready_tickets actions".into());
    }
    let path = match ledger_path {
        Some(path) => path.to_owned(),
        None => crate::fleet::ledger::default_path().map_err(|error| error.to_string())?,
    };
    let due = ledger::read_view_rows(&path, "need_ready_tickets_due", "at_ms", window)?;
    let project_names = projects
        .iter()
        .map(|project| project.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut evidence = Vec::new();
    for row in due {
        let at_ms = row
            .get("at_ms")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| "need_ready_tickets_due row has no integer at_ms".to_owned())?;
        if at_ms < window.start_ms || at_ms >= window.end_ms {
            continue;
        }
        let project = row
            .get("project")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "need_ready_tickets_due row has no project".to_owned())?;
        if !project_names.contains(&project.to_ascii_lowercase()) {
            continue;
        }
        let action_id = row
            .get("action_id")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                row.get("action_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .ok_or_else(|| "need_ready_tickets_due row has no scalar action_id".to_owned())?;
        evidence.push(format!("fleet-ledger:need_ready_tickets_due:{action_id}"));
    }
    evidence.sort();
    evidence.dedup();
    Ok((evidence.len(), evidence))
}

#[implements("FR-047-AC-6")]
#[cfg(test)]
fn ledger_unavailable_reason(
    view: &str,
    rows: &[serde_json::Map<String, serde_json::Value>],
) -> Result<Option<String>, String> {
    for row in rows {
        let unavailable = row
            .get("available")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|available| available == 0)
            || row
                .get("available")
                .and_then(serde_json::Value::as_bool)
                .is_some_and(|available| !available);
        if unavailable {
            let reason = row
                .get("unavailable_reason")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| format!("fleet ledger {view} unavailable row has no reason"))?;
            return Ok(Some(reason.to_owned()));
        }
    }
    Ok(None)
}

#[implements("FR-047-AC-1")]
fn read_need_ready_tickets_unanswered(
    ledger_path: Option<&Path>,
    ready_state: Option<&str>,
    projects: &[String],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let ready_state = ready_state
        .filter(|state| !state.trim().is_empty())
        .ok_or_else(|| {
            "eval.json has no ready_state; Linear ready-state history cannot be counted".to_owned()
        })?;
    if projects.is_empty() {
        return Err("retro config has no projects for need_ready_tickets actions".into());
    }
    let project_names = projects
        .iter()
        .map(|project| project.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let path = match ledger_path {
        Some(path) => path.to_owned(),
        None => crate::fleet::ledger::default_path().map_err(|error| error.to_string())?,
    };
    let due = ledger::read_view_rows(&path, "need_ready_tickets_due", "at_ms", window)?;
    let actions = due
        .into_iter()
        .filter(|row| {
            row.get("at_ms")
                .and_then(serde_json::Value::as_i64)
                .is_some_and(|at_ms| at_ms >= window.start_ms && at_ms < window.end_ms)
                && row
                    .get("project")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|project| project_names.contains(&project.to_ascii_lowercase()))
        })
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return Ok((0, Vec::new()));
    }

    let linear = crate::LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
    use crate::WorkspaceSource;
    let workspace = linear
        .fetch_workspace()
        .map_err(|error| format!("Linear workspace read failed: {error}"))?;
    let mut project_ids = BTreeSet::new();
    for action in &actions {
        let project_name = action
            .get("project")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "need_ready_tickets action has no project".to_owned())?;
        let projects = workspace
            .projects
            .iter()
            .filter(|project| project.name.eq_ignore_ascii_case(project_name))
            .collect::<Vec<_>>();
        let [project] = projects.as_slice() else {
            return Err(format!(
                "Linear project {project_name:?} is missing or ambiguous"
            ));
        };
        project_ids.insert(project.id.0.clone());
    }
    let mut transitions = BTreeMap::<String, Vec<crate::linear::NamedStateChange>>::new();
    for issue in workspace.issues.iter().filter(|issue| {
        issue
            .project_id
            .as_ref()
            .is_some_and(|id| project_ids.contains(&id.0))
    }) {
        transitions.insert(
            issue.id.0.clone(),
            linear.fetch_named_state_changes(&issue.id.0)?,
        );
    }
    let mut unanswered = 0usize;
    let mut evidence = Vec::new();
    for action in actions {
        let action_id = action
            .get("action_id")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| "need_ready_tickets_due row has no integer action_id".to_owned())?;
        let project_name = action
            .get("project")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("need_ready_tickets action {action_id} has no project"))?;
        let requested = action
            .get("requested_count")
            .and_then(serde_json::Value::as_i64)
            .filter(|count| *count > 0)
            .ok_or_else(|| {
                format!("need_ready_tickets action {action_id} has no positive requested_count")
            })? as usize;
        let emitted_at = action
            .get("emitted_at")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("need_ready_tickets action {action_id} has no emitted_at"))?;
        let due_at = action
            .get("at_ms")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("need_ready_tickets action {action_id} has no due time"))?;
        let projects = workspace
            .projects
            .iter()
            .filter(|project| project.name.eq_ignore_ascii_case(project_name))
            .collect::<Vec<_>>();
        let [project] = projects.as_slice() else {
            return Err(format!(
                "Linear project {project_name:?} is missing or ambiguous"
            ));
        };
        let mut ready_tickets = BTreeSet::new();
        for issue in workspace.issues.iter().filter(|issue| {
            issue
                .project_id
                .as_ref()
                .is_some_and(|id| id.0 == project.id.0)
        }) {
            let issue_events = transitions
                .get(&issue.id.0)
                .ok_or_else(|| format!("Linear history is missing for {}", issue.identifier))?;
            if ready_ticket_after_action(issue_events, emitted_at, due_at, ready_state) {
                ready_tickets.insert(issue.identifier.clone());
            }
        }
        unanswered = unanswered.saturating_add(requested.saturating_sub(ready_tickets.len()));
        evidence.push(format!("fleet-ledger:need_ready_tickets_due:{action_id}"));
        evidence.extend(ready_tickets.into_iter().map(|ticket| {
            format!("linear-ticket:{ticket}:entered:{ready_state}:after-action:{action_id}")
        }));
    }
    evidence.sort();
    evidence.dedup();
    Ok((unanswered, evidence))
}

#[implements("FR-047-AC-1")]
fn ready_ticket_after_action(
    events: &[crate::linear::NamedStateChange],
    emitted_at_ms: i64,
    due_at_ms: i64,
    ready_state: &str,
) -> bool {
    events.iter().any(|event| {
        event.at * 1000.0 >= emitted_at_ms as f64
            && event.at * 1000.0 <= due_at_ms as f64
            && event
                .to
                .as_deref()
                .is_some_and(|state| state.eq_ignore_ascii_case(ready_state))
    })
}

fn read_merged_prs(
    repositories: &[String],
    window: &EvalWindow,
    github_guard_state_path: Option<&Path>,
    github_guard_max_bytes: u64,
) -> Result<(usize, Vec<String>), String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for GitHub merge evidence".to_owned());
    }
    let mut prs = Vec::new();
    for repository in repositories {
        prs.extend(guarded_pull_requests(
            repository,
            github_guard_state_path,
            github_guard_max_bytes,
        )?);
    }
    merged_prs_count(&prs, window)
}

#[implements("FR-047-AC-1")]
fn guarded_pull_requests(
    repository: &str,
    state_path: Option<&Path>,
    max_bytes: u64,
) -> Result<Vec<super::sources::PullRequest>, String> {
    let state_path = state_path.ok_or_else(|| {
        "shared GitHub guard state path is not configured in eval.json".to_owned()
    })?;
    github::read_pull_requests(state_path, max_bytes, repository).and_then(|rows| {
        rows.into_iter()
            .map(|row| {
                let created_at = crate::drift::timestamp_epoch(&row.created_at)
                    .map_err(|_| "GitHub PR has an invalid createdAt timestamp".to_owned())?;
                let updated_at = crate::drift::timestamp_epoch(&row.updated_at)
                    .map_err(|_| "GitHub PR has an invalid updatedAt timestamp".to_owned())?;
                let merged_at = row
                    .merged_at
                    .as_deref()
                    .map(crate::drift::timestamp_epoch)
                    .transpose()
                    .map_err(|_| "GitHub PR has an invalid mergedAt timestamp".to_owned())?;
                Ok(super::sources::PullRequest {
                    closed_at: None,
                    reopen_events: None,
                    repo: repository.to_owned(),
                    number: row.number,
                    open: row.merged_at.is_none(),
                    draft: row.is_draft,
                    created_at,
                    merged_at,
                    updated_at,
                    additions: 0,
                    deletions: 0,
                    review_decision: row.review_decision,
                    force_pushes: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()
    })
}

fn merged_prs_count(
    prs: &[super::sources::PullRequest],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let mut seen = std::collections::BTreeSet::new();
    let mut evidence = Vec::new();
    for pr in prs {
        if pr
            .merged_at
            .is_some_and(|at| at >= start_secs && at < end_secs)
        {
            let key = format!("{}#{}", pr.repo, pr.number);
            if seen.insert(key.clone()) {
                evidence.push(format!("github-pr:{key}:merged"));
            }
        }
    }
    evidence.sort();
    Ok((seen.len(), evidence))
}

/// Counts recorded aggregate-gate log lines completed in the selected window.
#[implements("FR-047-AC-1")]
pub fn gate_log_adapters(
    specs: &[RoleSpec],
    gate_log: Option<&Path>,
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let needed = specs.iter().flat_map(|spec| &spec.rules).any(|rule| {
        rule.method == "deterministic" && rule.query_or_rubric == "retro.gate_log_runs"
    });
    if !needed {
        return BTreeMap::new();
    }
    let value = read_gate_log_runs(gate_log, window);
    BTreeMap::from([(
        "retro.gate_log_runs".to_owned(),
        match value {
            Ok((value, evidence_refs)) => QueryValue::Value {
                value: value as f64,
                evidence_refs,
            },
            Err(reason) => QueryValue::Unavailable {
                reason,
                evidence_refs: Vec::new(),
            },
        },
    )])
}

fn read_gate_log_runs(
    gate_log: Option<&Path>,
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let path = gate_log.ok_or_else(|| "retro config has no gate_log path".to_owned())?;
    use super::GateLogSource;
    let lines = super::live::FileGateLog
        .lines(path)
        .map_err(|error| format!("gate log is unavailable: {error}"))?;
    gate_log_runs_count(&lines, window)
}

fn gate_log_runs_count(
    lines: &[super::GateLine],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let selected = lines
        .iter()
        .filter(|line| !line.repin && line.finished_at >= start_secs && line.finished_at < end_secs)
        .collect::<Vec<_>>();
    let evidence = selected
        .iter()
        .map(|line| {
            format!(
                "gate-log:{}:{}",
                line.pr,
                crate::drift::format_timestamp(line.finished_at)
            )
        })
        .collect::<Vec<_>>();
    Ok((selected.len(), evidence))
}

#[implements("FR-047-AC-9")]
fn read_ready_buffer_mean(url: &str, window: &EvalWindow) -> Result<f64, String> {
    let body = github::read_prometheus_range(url, "avg(ix_fleet_ready_buffer_deficit)", window)?;
    ready_buffer_mean_from_response(&body, window)
}

#[implements("FR-047-AC-1")]
fn ready_buffer_mean_from_response(body: &str, window: &EvalWindow) -> Result<f64, String> {
    let mut points = BTreeMap::<i64, Vec<f64>>::new();
    for (at_ms, sample) in github::prometheus_samples(body, window)? {
        points.entry(at_ms).or_default().push(sample);
    }
    let points = points
        .into_iter()
        .map(|(at, values)| {
            let sum = values.iter().sum::<f64>();
            (at, sum / values.len() as f64)
        })
        .collect::<Vec<_>>();
    let first = points
        .first()
        .ok_or_else(|| "Prometheus range query returned no samples".to_owned())?;
    let Some(last) = points.last() else {
        return Err("Prometheus range query returned no samples".to_owned());
    };
    if first.0 > window.start_ms.saturating_add(60_000)
        || window.end_ms.saturating_sub(last.0) > 60_000
    {
        return Err("Prometheus range has an uncovered window boundary".to_owned());
    }
    if points
        .windows(2)
        .any(|pair| pair[1].0.saturating_sub(pair[0].0) > 120_000)
    {
        return Err("Prometheus range has a gap longer than two samples".to_owned());
    }
    let mut weighted_sum = 0.0;
    for pair in points.windows(2) {
        let width_ms = pair[1].0.saturating_sub(pair[0].0);
        weighted_sum += pair[0].1 * width_ms as f64;
    }
    let covered_ms = last.0.saturating_sub(first.0);
    if covered_ms <= 0 {
        return Err("Prometheus range covers no time".to_owned());
    }
    Ok(weighted_sum / covered_ms as f64)
}

/// Completes the registry so every deterministic role reference has an explicit result.
///
/// Retro and live-source adapters are added by their readers. Remaining references are kept as
/// unavailable with a source-specific reason; they are never silently omitted or scored as zero.
#[implements("FR-047-AC-1")]
pub fn declared_adapters(specs: &[RoleSpec]) -> BTreeMap<String, QueryValue> {
    let mut values = BTreeMap::new();
    for rule in specs.iter().flat_map(|spec| &spec.rules) {
        if rule.method != "deterministic" || values.contains_key(&rule.query_or_rubric) {
            continue;
        }
        let query = rule.query_or_rubric.as_str();
        let (reason, applicable) = match query {
            "ledger.idle_lane_minutes_during_build_pause" => {
                ("builds are never paused".to_owned(), false)
            }
            "linear.waiver_records" => (
                "waiver records are judgement evidence, not a numeric producer".to_owned(),
                false,
            ),
            _ => (
                format!("{query}: {}", unavailable_source_reason(query)),
                true,
            ),
        };
        values.insert(
            rule.query_or_rubric.clone(),
            if applicable {
                QueryValue::Unavailable {
                    reason,
                    evidence_refs: Vec::new(),
                }
            } else {
                QueryValue::NotApplicable {
                    reason: reason.to_owned(),
                    evidence_refs: Vec::new(),
                }
            },
        );
    }
    values
}

#[implements("FR-047-AC-1")]
fn unavailable_source_reason(query: &str) -> &'static str {
    match query.split_once('.') {
        Some(("github", _)) => "GitHub evidence is unavailable for this evaluation window",
        Some(("linear", _)) => "Linear marker evidence is unavailable for this evaluation window",
        Some(("prom", _)) => "Prometheus evidence is unavailable for this evaluation window",
        Some(("ledger", _)) => "fleet ledger evidence is unavailable for this evaluation window",
        Some(("transcript", _)) => "session transcript evidence is unavailable for this session",
        Some(("hooks", _)) => "hook event evidence is unavailable for this evaluation window",
        Some(("retro", _)) => "retro evidence is unavailable for this evaluation window",
        _ => "query reference has no registered source adapter",
    }
}

#[implements("FR-047-AC-1")]
fn compare(value: f64, comparator: &str, target: f64) -> Option<bool> {
    match comparator {
        "=" => Some(value == target),
        "!=" => Some(value != target),
        ">" => Some(value > target),
        ">=" => Some(value >= target),
        "<" => Some(value < target),
        "<=" => Some(value <= target),
        _ => None,
    }
}

/// Root path of TEAM_RETRO_HOME, falling back to the user's local state directory.
#[implements("FR-047-AC-4")]
pub fn settings_root() -> PathBuf {
    std::env::var_os("TEAM_RETRO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local/state/team-retro")
        })
}

/// Loads evaluator-only settings from the selected team's `eval.json`.
#[implements("FR-047-AC-4")]
pub fn load_settings(team: &str) -> Result<EvalSettings, String> {
    let path = settings_root().join(team).join("eval.json");
    load_settings_file(&path)
}

#[implements("FR-047-AC-4")]
fn load_settings_file(path: &Path) -> Result<EvalSettings, String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EvalSettings::default());
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let settings: EvalSettings =
        serde_json::from_str(&contents).map_err(|error| format!("{}: {error}", path.display()))?;
    if settings.role_spec_dir.as_os_str().is_empty() {
        return Err(format!(
            "{}: role_spec_dir must not be empty",
            path.display()
        ));
    }
    if settings
        .github_guard_state_path
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(format!(
            "{}: github_guard_state_path must not be empty",
            path.display()
        ));
    }
    if !(1024 * 1024..=64 * 1024 * 1024).contains(&settings.github_guard_max_bytes) {
        return Err(format!(
            "{}: github_guard_max_bytes must be from 1 MiB through 64 MiB",
            path.display()
        ));
    }
    Ok(settings)
}

#[implements("FR-047-AC-4")]
fn default_role_spec_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/ix-board/role-specs")
}

/// Joins a relative role-spec directory to the evaluator settings directory.
#[implements("FR-047-AC-4")]
pub fn resolve_role_spec_dir(team: &str, settings: &EvalSettings) -> PathBuf {
    if settings.role_spec_dir.is_absolute() {
        settings.role_spec_dir.clone()
    } else {
        settings_root().join(team).join(&settings.role_spec_dir)
    }
}

/// Renders the same per-rule statuses and values used by the JSON document.
#[implements("FR-047-AC-7")]
pub fn render_text(report: &EvalReport) -> String {
    let mut lines = vec![format!(
        "team={} window={}..{} previous={}..{} schema_version={}",
        report.team,
        report.window.start_ms,
        report.window.end_ms,
        report.previous_window.start_ms,
        report.previous_window.end_ms,
        report.schema_version,
    )];
    for role in &report.roles {
        lines.push(format!(
            "role={} score={}/{} pass_rate={:?} previous={}/{} previous_pass_rate={:?} trend={:?} trend_matched_rules={} session_ids={}",
            role.role,
            role.score.passed,
            role.score.evaluated,
            role.score.pass_rate,
            role.previous_score.passed,
            role.previous_score.evaluated,
            role.previous_score.pass_rate,
            role.trend,
            role.trend_matched_rules,
            serde_json::to_string(&role.session_ids).unwrap_or_default(),
        ));
        lines.extend(role.objective_metrics.iter().map(|metric| {
            format!(
                "  objective_metric={}",
                serde_json::to_string(metric).unwrap_or_default()
            )
        }));
        lines.extend(
            role.rules
                .iter()
                .map(|rule| format!("  rule={}", serde_json::to_string(rule).unwrap_or_default())),
        );
        lines.push(format!(
            "  failing_rules={}",
            serde_json::to_string(&role.failing_rules).unwrap_or_default()
        ));
    }
    for session in &report.sessions {
        lines.push(format!(
            "session={} role={} score={}/{} pass_rate={:?}",
            session.id,
            session.role,
            session.score.passed,
            session.score.evaluated,
            session.score.pass_rate,
        ));
        lines.extend(session.objective_metrics.iter().map(|metric| {
            format!(
                "  objective_metric={}",
                serde_json::to_string(metric).unwrap_or_default()
            )
        }));
        lines.extend(
            session
                .rules
                .iter()
                .map(|rule| format!("  rule={}", serde_json::to_string(rule).unwrap_or_default())),
        );
        lines.push(format!(
            "  failing_rules={}",
            serde_json::to_string(&session.failing_rules).unwrap_or_default()
        ));
    }
    lines.push(format!(
        "source_requests={}",
        serde_json::to_string(&report.source_requests).unwrap_or_default()
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-047-AC-1")]
    fn parse_and_evaluate_every_rule_including_unknown_queries() {
        let markdown = "## Evaluations\n| ID | Rule | Evidence source | Trigger | Method | Query or rubric | Evaluator | Comparator | Value | Unit | Cadence |\n|---|---|---|---|---|---|---|---|---|---|---|\n| R-1 | r | transcript | hooks.role_turns | deterministic | known.query | evaluator | = | 0 | runs | end |\n| R-2 | r | transcript | hooks.role_turns | deterministic | unknown.query | evaluator | = | 0 | runs | end |\n| R-3 | r | transcript | hooks.role_turns | judgement | rubric@1#x | team-retro | >= | 2 | score | daily |\n";
        let spec = parse_role_spec(Path::new("coder.md"), markdown).unwrap();
        let adapters = BTreeMap::from([(
            "known.query".into(),
            QueryValue::Value {
                value: 0.0,
                evidence_refs: vec!["event:1".into()],
            },
        )]);
        let results = evaluate_rules(&spec.rules, &adapters);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].status, RuleStatus::Pass);
        assert_eq!(results[1].status, RuleStatus::Unavailable);
        assert!(
            results[1]
                .reason
                .as_deref()
                .unwrap()
                .contains("unknown.query")
        );
        assert_eq!(results[2].status, RuleStatus::JudgementRequired);
        assert_eq!(score(&results).pass_rate, Some(1.0));
    }

    #[test]
    #[trace("FR-047-AC-2")]
    fn role_metrics_keep_their_value_status_unit_and_null_reason() {
        let markdown = "## Metrics\n| ID | Name | Evidence source | Query | Unit | Target | Direction |\n|---|---|---|---|---|---|---|\n| CODE-M01 | Start time | github | github.ticket_start_to_pr_open_median_hours | hours | 4 | lower |\n| CODE-M02 | Throughput | retro | retro.net_closed_minus_new | issues/day | 1 | higher |\n## Evaluations\n| ID | Rule | Evidence source | Trigger | Method | Query or rubric | Evaluator | Comparator | Value | Unit | Cadence |\n|---|---|---|---|---|---|---|---|---|---|---|\n| CODE-R01-E1 | rule | github | github.pull_requests | deterministic | github.pull_requests | evaluator | = | 0 | PRs | 3h |\n";
        let spec = parse_role_spec(Path::new("coder.md"), markdown).unwrap();
        assert_eq!(spec.metrics.len(), 2);
        assert_eq!(spec.metrics[0].id, "CODE-M01");
        assert_eq!(spec.metrics[1].query, "retro.net_closed_minus_new");
        let adapters = BTreeMap::from([
            (
                "github.ticket_start_to_pr_open_median_hours".into(),
                QueryValue::Value {
                    value: 3.5,
                    evidence_refs: vec!["github:pr:7".into()],
                },
            ),
            (
                "retro.net_closed_minus_new".into(),
                QueryValue::Unavailable {
                    reason: "no complete stored snapshot".into(),
                    evidence_refs: Vec::new(),
                },
            ),
        ]);
        let rows = evaluate_objective_metrics(&spec.metrics, &adapters);
        assert_eq!(rows[0].value, Some(3.5));
        assert_eq!(rows[0].status, RuleStatus::Pass);
        assert_eq!(rows[0].evidence_refs, ["github:pr:7"]);
        assert_eq!(rows[1].value, None);
        assert_eq!(rows[1].status, RuleStatus::Unavailable);
        assert_eq!(
            rows[1].reason.as_deref(),
            Some("no complete stored snapshot")
        );
    }

    #[test]
    #[trace("FR-047-AC-2")]
    #[trace("FR-047-AC-3")]
    fn incomplete_and_not_applicable_rules_do_not_change_denominator() {
        let mk = |id: &str, status| RuleResult {
            id: id.into(),
            query: id.into(),
            status,
            value: None,
            comparator: "=".into(),
            target: 0.0,
            evidence_refs: Vec::new(),
            reason: None,
            rubric_id: None,
            judgement_required: (status == RuleStatus::JudgementRequired).then_some(true),
        };
        let rows = vec![
            mk("pass", RuleStatus::Pass),
            mk("fail", RuleStatus::Fail),
            mk("na", RuleStatus::NotApplicable),
            mk("missing", RuleStatus::Unavailable),
            mk("judgement", RuleStatus::JudgementRequired),
        ];
        let score = score(&rows);
        assert_eq!((score.passed, score.evaluated), (1, 2));
        assert_eq!(score.pass_rate, Some(0.5));
        assert_eq!(complete_score(&rows), None);
    }

    #[test]
    #[trace("FR-047-AC-3")]
    fn trend_uses_rules_scored_in_both_windows_despite_other_unavailable_rules() {
        let row = |id: &str, status, judgement_required| RuleResult {
            id: id.into(),
            query: format!("query.{id}"),
            status,
            value: None,
            comparator: "=".into(),
            target: 1.0,
            evidence_refs: Vec::new(),
            reason: None,
            rubric_id: None,
            judgement_required,
        };
        let current = [
            row("A", RuleStatus::Pass, Some(false)),
            row("B", RuleStatus::Unavailable, Some(false)),
            row("C", RuleStatus::Fail, Some(false)),
            row("D", RuleStatus::Pass, Some(false)),
            row("J", RuleStatus::JudgementRequired, Some(true)),
        ];
        let previous = [
            row("A", RuleStatus::Fail, Some(false)),
            row("B", RuleStatus::Pass, Some(false)),
            row("C", RuleStatus::Fail, Some(false)),
            row("E", RuleStatus::Pass, Some(false)),
        ];
        assert_eq!(trend_delta(&current, &previous), (Some(0.5), 2));
        assert_eq!(
            trend_delta(
                &[row("A", RuleStatus::Unavailable, Some(false))],
                &[row("A", RuleStatus::Pass, Some(false))]
            ),
            (None, 0)
        );
    }

    #[test]
    #[trace("FR-047-AC-4")]
    fn evaluator_settings_keep_defaults_and_scope_gate_commands_by_repository() {
        let settings: EvalSettings = serde_json::from_str(
            r#"{"role_spec_dir":"roles","gate_commands":{"agent-ix/ix-board":["cargo","test"]}}"#,
        )
        .unwrap();
        assert_eq!(settings.review_age_limit_hours, 1);
        assert_eq!(settings.ready_state, None);
        assert_eq!(
            settings.gate_commands_for("agent-ix/ix-board"),
            ["cargo", "test"]
        );
        assert_eq!(
            settings.gate_commands_for("agent-ix/dev-team"),
            ["make", "ci"]
        );
    }

    #[test]
    #[trace("FR-047-AC-4")]
    fn evaluator_settings_are_loaded_from_eval_json_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("eval.json");
        std::fs::write(
            &path,
            r#"{"role_spec_dir":"roles","ready_state":"Ready","review_age_limit_hours":2}"#,
        )
        .unwrap();
        let settings = load_settings_file(&path).unwrap();
        assert_eq!(settings.ready_state.as_deref(), Some("Ready"));
        assert_eq!(settings.review_age_limit_hours, 2);
        let config_path = directory.path().join("config.json");
        std::fs::write(&config_path, r#"{"team":"TEST"}"#).unwrap();
        assert!(load_settings_file(&config_path).is_err());
    }

    #[test]
    #[trace("FR-047-AC-4")]
    fn missing_eval_settings_use_documented_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let missing = load_settings_file(&directory.path().join("eval.json")).unwrap();
        assert_eq!(
            missing.role_spec_dir,
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share/ix-board/role-specs")
        );
        assert_eq!(missing.review_age_limit_hours, 1);
        assert!(missing.gate_commands.is_empty());
        assert_eq!(missing.ready_state, None);
        let minimal = directory.path().join("minimal.json");
        std::fs::write(&minimal, "{}").unwrap();
        let loaded = load_settings_file(&minimal).unwrap();
        assert_eq!(loaded.role_spec_dir, missing.role_spec_dir);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    #[trace("FR-047-AC-9")]
    fn retro_metric_adapter_uses_only_snapshots_inside_the_window() {
        let record: super::super::Record = serde_json::from_str(
            r#"{"schema_version":1,"kind":"snapshot","taken_at":"2023-11-14T22:13:20Z","team":"TEST","window":"24h","metrics":{"prs_oversize":{"id":"prs_oversize","status":"ok","value":2,"unit":"pull_requests","direction":"down","source":"github","query":"open pull requests","class":"flow","department_class":"throughput"}}}"#,
        ).unwrap();
        let spec = RoleSpec {
            role: "coder".into(),
            metrics: Vec::new(),
            rules: vec![EvaluationRule {
                id: "R-1".into(),
                rule: "rule".into(),
                evidence_source: "ix-board-metric".into(),
                trigger: "retro.prs_open".into(),
                method: "deterministic".into(),
                query_or_rubric: "retro.prs_oversize".into(),
                comparator: "=".into(),
                target: 0.0,
                unit: "pull_requests".into(),
            }],
        };
        let window = EvalWindow {
            start_ms: 1_700_000_000_000,
            end_ms: 1_795_694_400_000,
        };
        let adapters = retro_adapters(
            std::slice::from_ref(&spec),
            Ok(std::slice::from_ref(&record)),
            &window,
        );
        assert_eq!(
            adapters["retro.prs_oversize"],
            QueryValue::Value {
                value: 2.0,
                evidence_refs: vec!["retro:prs_oversize:2023-11-14T22:13:20Z".into()],
            }
        );
        let outside = EvalWindow {
            start_ms: 0,
            end_ms: 1,
        };
        let adapters = retro_adapters(
            std::slice::from_ref(&spec),
            Ok(std::slice::from_ref(&record)),
            &outside,
        );
        assert!(matches!(
            adapters["retro.prs_oversize"],
            QueryValue::Unavailable { .. }
        ));
        let live = retro_record_adapters(std::slice::from_ref(&spec), Ok(&record));
        assert_eq!(
            live["retro.prs_oversize"],
            QueryValue::Value {
                value: 2.0,
                evidence_refs: vec!["retro:prs_oversize:2023-11-14T22:13:20Z".into()],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-9")]
    fn retro_adapter_reads_hand_written_additive_churn_metrics() {
        let record: super::super::Record = serde_json::from_str(
            r#"{"schema_version":1,"kind":"snapshot","taken_at":"2026-11-14T22:13:20Z","team":"QSL","window":"24h","metrics":{"prs_per_ticket_max":{"id":"prs_per_ticket_max","status":"ok","value":3,"unit":"pull_requests","direction":"down","source":"linear+github","query":"attached pull requests per ticket","class":"rework"},"commits_per_pr_max":{"id":"commits_per_pr_max","status":"ok","value":7,"unit":"commits","direction":"down","source":"github","query":"commits on pull request","class":"rework"}}}"#,
        )
        .unwrap();
        let spec = RoleSpec {
            role: "coder".into(),
            metrics: Vec::new(),
            rules: ["prs_per_ticket_max", "commits_per_pr_max"]
                .into_iter()
                .map(|metric_id| EvaluationRule {
                    id: format!("R-{metric_id}"),
                    rule: "rule".into(),
                    evidence_source: "ix-board-metric".into(),
                    trigger: format!("retro.{metric_id}"),
                    method: "deterministic".into(),
                    query_or_rubric: format!("retro.{metric_id}"),
                    comparator: "=".into(),
                    target: 0.0,
                    unit: "count".into(),
                })
                .collect(),
        };
        let live = retro_record_adapters(std::slice::from_ref(&spec), Ok(&record));
        assert_eq!(
            live["retro.prs_per_ticket_max"],
            QueryValue::Value {
                value: 3.0,
                evidence_refs: vec!["retro:prs_per_ticket_max:2026-11-14T22:13:20Z".into()],
            }
        );
        assert_eq!(
            live["retro.commits_per_pr_max"],
            QueryValue::Value {
                value: 7.0,
                evidence_refs: vec!["retro:commits_per_pr_max:2026-11-14T22:13:20Z".into()],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn missing_department_configuration_has_the_accumulation_reason() {
        let record: super::super::Record = serde_json::from_str(
            r#"{"schema_version":1,"kind":"snapshot","taken_at":"2023-11-14T22:13:20Z","team":"TEST","window":"24h","metrics":{}}"#,
        )
        .unwrap();
        assert_eq!(
            retro_record_value(&record, "claims_without_movement"),
            QueryValue::Unavailable {
                reason: "installed department producer needs TEST department configuration and one complete 24-hour snapshot window".into(),
                evidence_refs: vec!["retro:department:claims_without_movement:2023-11-14T22:13:20Z".into()],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn build_pause_metric_is_not_applicable_without_a_producer() {
        let spec = RoleSpec {
            role: "orchestrator".into(),
            metrics: Vec::new(),
            rules: vec![EvaluationRule {
                id: "ORCH-R1".into(),
                rule: "rule".into(),
                evidence_source: "ledger".into(),
                trigger: "ledger.idle_lane_minutes_during_build_pause".into(),
                method: "deterministic".into(),
                query_or_rubric: "ledger.idle_lane_minutes_during_build_pause".into(),
                comparator: "=".into(),
                target: 0.0,
                unit: "minutes".into(),
            }],
        };
        let adapters = declared_adapters(std::slice::from_ref(&spec));
        let results = evaluate_rules(&spec.rules, &adapters);
        assert_eq!(results[0].status, RuleStatus::NotApplicable);
        assert_eq!(results[0].value, None);
        assert_eq!(
            results[0].reason.as_deref(),
            Some("builds are never paused")
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn every_deterministic_rule_has_an_explicit_registry_result() {
        let specs = [RoleSpec {
            role: "planner".into(),
            metrics: Vec::new(),
            rules: vec![
                EvaluationRule {
                    id: "A".into(),
                    rule: "a".into(),
                    evidence_source: "ledger".into(),
                    trigger: "ledger.view".into(),
                    method: "deterministic".into(),
                    query_or_rubric: "ledger.need_ready_tickets_unanswered".into(),
                    comparator: "=".into(),
                    target: 0.0,
                    unit: "actions".into(),
                },
                EvaluationRule {
                    id: "B".into(),
                    rule: "b".into(),
                    evidence_source: "linear".into(),
                    trigger: "linear.waiver_records".into(),
                    method: "judgement".into(),
                    query_or_rubric: "rubric#waiver".into(),
                    comparator: ">=".into(),
                    target: 2.0,
                    unit: "score".into(),
                },
                EvaluationRule {
                    id: "C".into(),
                    rule: "c".into(),
                    evidence_source: "prom".into(),
                    trigger: "prom.series".into(),
                    method: "deterministic".into(),
                    query_or_rubric: "prom.ready_buffer_deficit_mean".into(),
                    comparator: "=".into(),
                    target: 0.0,
                    unit: "tickets".into(),
                },
            ],
        }];
        let adapters = declared_adapters(&specs);
        let results = evaluate_rules(&specs[0].rules, &adapters);
        assert_eq!(results[0].status, RuleStatus::Unavailable);
        assert!(results[0].reason.as_deref().unwrap().contains("ledger"));
        assert_eq!(results[1].status, RuleStatus::JudgementRequired);
        assert_eq!(results[2].status, RuleStatus::Unavailable);
        assert!(results[2].reason.as_deref().unwrap().contains("Prometheus"));
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn prometheus_ready_buffer_mean_is_time_weighted_and_rejects_gaps() {
        let window = EvalWindow {
            start_ms: 0,
            end_ms: 180_000,
        };
        let body = r#"{"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":[[0,"1"],[60,"1"],[120,"3"],[180,"3"]]}]}}"#;
        let mean = ready_buffer_mean_from_response(body, &window).unwrap();
        assert!((mean - (5.0 / 3.0)).abs() < 0.0001);

        let gap = r#"{"status":"success","data":{"resultType":"matrix","result":[{"metric":{},"values":[[0,"1"],[180,"3"]]}]}}"#;
        assert!(
            ready_buffer_mean_from_response(gap, &window)
                .unwrap_err()
                .contains("gap longer than two samples")
        );

        let no_series = r#"{"status":"success","data":{"resultType":"matrix","result":[]}}"#;
        assert_eq!(
            ready_buffer_mean_from_response(no_series, &window).unwrap_err(),
            "Prometheus range query returned no series"
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn linear_reviewed_prs_deduplicates_markers_and_merges_in_window() {
        let prs = [
            super::super::sources::PullRequest {
                repo: "owner/repo".into(),
                number: 1,
                open: false,
                draft: false,
                created_at: 10.0,
                updated_at: 30.0,
                merged_at: Some(25.0),
                closed_at: None,
                reopen_events: None,
                additions: 0,
                deletions: 0,
                review_decision: None,
                force_pushes: None,
            },
            super::super::sources::PullRequest {
                repo: "owner/repo".into(),
                number: 2,
                open: true,
                draft: false,
                created_at: 10.0,
                updated_at: 20.0,
                merged_at: None,
                closed_at: None,
                reopen_events: None,
                additions: 0,
                deletions: 0,
                review_decision: None,
                force_pushes: None,
            },
        ];
        let comments = ["a", "b"].map(|id| crate::MarkerComment {
            id: Some(id.into()),
            created_at: Some(crate::drift::format_timestamp(20.0)),
            body: "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#2 method=review run=1 -->\n\n```yaml\nclean: true\nfindings: []\n```".into(),
        });
        let window = EvalWindow {
            start_ms: 0,
            end_ms: 30_000,
        };
        let (count, evidence) = reviewed_prs_count(&prs, &comments, &window).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            evidence,
            ["github-pr:owner/repo#1:merged", "linear-comment:a",]
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn merged_pull_requests_without_review_markers_are_reported_by_native_merge_time() {
        let pr = |number| super::super::sources::PullRequest {
            repo: "owner/repo".into(),
            number,
            open: false,
            draft: false,
            created_at: 10.0,
            updated_at: 25.0,
            merged_at: Some(20.0),
            closed_at: None,
            reopen_events: None,
            additions: 0,
            deletions: 0,
            review_decision: None,
            force_pushes: None,
        };
        let comment = crate::MarkerComment {
            id: Some("review".into()),
            created_at: Some(crate::drift::format_timestamp(15.0)),
            body:
                "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 method=review run=a head=abc -->"
                    .into(),
        };
        let window = EvalWindow {
            start_ms: 10_000,
            end_ms: 30_000,
        };
        let (count, evidence) =
            merged_prs_without_review_count(&[pr(1), pr(2)], &[comment], &window).unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            evidence,
            ["github-pr:owner/repo#2:merged-without-review-marker"]
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn github_merged_prs_uses_merge_time_and_deduplicates() {
        let pr = super::super::sources::PullRequest {
            repo: "owner/repo".into(),
            number: 7,
            open: false,
            draft: false,
            created_at: 10.0,
            updated_at: 25.0,
            merged_at: Some(20.0),
            closed_at: None,
            reopen_events: None,
            additions: 0,
            deletions: 0,
            review_decision: None,
            force_pushes: None,
        };
        let window = EvalWindow {
            start_ms: 10_000,
            end_ms: 30_000,
        };
        let (count, evidence) = merged_prs_count(&[pr.clone(), pr], &window).unwrap();
        assert_eq!(count, 1);
        assert_eq!(evidence, ["github-pr:owner/repo#7:merged"]);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn gate_log_runs_use_finished_time_and_exclude_repins() {
        let lines = [
            super::super::GateLine {
                pr: "owner/repo#1".into(),
                repin: false,
                requested_lock_at: 10.0,
                acquired_lock_at: 11.0,
                started_at: 12.0,
                finished_at: 20.0,
            },
            super::super::GateLine {
                pr: "owner/repo#2".into(),
                repin: true,
                requested_lock_at: 10.0,
                acquired_lock_at: 11.0,
                started_at: 12.0,
                finished_at: 20.0,
            },
            super::super::GateLine {
                pr: "owner/repo#3".into(),
                repin: false,
                requested_lock_at: 10.0,
                acquired_lock_at: 11.0,
                started_at: 12.0,
                finished_at: 30.0,
            },
        ];
        let window = EvalWindow {
            start_ms: 10_000,
            end_ms: 30_000,
        };
        let (count, evidence) = gate_log_runs_count(&lines, &window).unwrap();
        assert_eq!(count, 1);
        assert_eq!(evidence, ["gate-log:owner/repo#1:1970-01-01T00:00:20Z"]);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn reviewer_method_runs_count_only_distinct_runs_for_reviewed_prs() {
        let comment = |id: &str, run: &str, pr: &str| crate::MarkerComment {
            id: Some(id.into()),
            created_at: Some(crate::drift::format_timestamp(20.0)),
            body: format!(
                "<!-- reviewer repo=owner/repo id=SR-{id} pr=repo#{pr} method=review run={run} -->"
            ),
        };
        let comments = [
            comment("1", "a", "1"),
            comment("2", "a", "1"),
            comment("3", "b", "1"),
            comment("4", "a", "2"),
            comment("5", "a", "2"),
        ];
        let allowed = std::collections::BTreeSet::from(["owner/repo#1".to_owned()]);
        let (count, evidence) = pr_methods_count(&comments, &allowed).unwrap();
        assert_eq!(count, 1);
        assert_eq!(evidence.len(), 3);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn lead_gate_record_marker_requires_a_kind_and_parses_fields() {
        let (kind, fields) = lead_record_fields(
            "<!-- lead-record kind=gate repo=agent-ix/ix-board finished=2026-11-24T08:30:00Z -->",
        )
        .unwrap();
        assert_eq!(kind, "lead-record");
        assert_eq!(fields.get("kind"), Some(&"gate"));
        assert_eq!(fields.get("repo"), Some(&"agent-ix/ix-board"));
        assert_eq!(fields.get("finished"), Some(&"2026-11-24T08:30:00Z"));
        assert!(lead_record_fields("<!-- reviewer repo=agent-ix/ix-board -->").is_some());
        assert!(lead_record_fields("not a marker").is_none());
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn gate_record_join_requires_exact_pull_request_and_finish_time() {
        let line = super::super::GateLine {
            pr: "owner/repo#12".into(),
            repin: false,
            requested_lock_at: 1.0,
            acquired_lock_at: 2.0,
            started_at: 3.0,
            finished_at: 20.0,
        };
        let records = BTreeSet::from([("owner/repo#12".to_owned(), 20_000)]);
        assert!(has_gate_record(&line, &records));

        let other_pr = BTreeSet::from([("owner/repo#13".to_owned(), 20_000)]);
        assert!(!has_gate_record(&line, &other_pr));
        let other_time = BTreeSet::from([("owner/repo#12".to_owned(), 20_001)]);
        assert!(!has_gate_record(&line, &other_time));
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn fleet_ledger_due_action_adapter_uses_due_time_and_action_id() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let config = crate::fleet::FleetConfig {
            ledger_path: Some(path.clone()),
            ..Default::default()
        };
        drop(crate::fleet::ledger::Ledger::open(&path, &config).unwrap());
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "INSERT INTO run(kind,relay_ms) VALUES('tick',86401000);
                 INSERT INTO tick(started_at,committed_at) VALUES(0,0);
                 INSERT INTO action(tick_id,kind,emitted_at,project,requested_count)
                   VALUES(1,'need_ready_tickets',1000,'Project A',3),
                         (1,'need_ready_tickets',1000,'Project B',2);",
            )
            .unwrap();
        drop(connection);

        let specs = [RoleSpec {
            role: "planner".into(),
            metrics: Vec::new(),
            rules: vec![EvaluationRule {
                id: "PLAN-R1".into(),
                rule: "due".into(),
                evidence_source: "ledger".into(),
                trigger: "ledger.need_ready_tickets_due".into(),
                method: "deterministic".into(),
                query_or_rubric: "ledger.need_ready_tickets_due".into(),
                comparator: "=".into(),
                target: 1.0,
                unit: "actions".into(),
            }],
        }];
        let due = 86_401_000;
        let values = ledger_adapters(
            &specs,
            Some(&path),
            Some("Ready"),
            &["Project A".into()],
            &EvalWindow {
                start_ms: due,
                end_ms: due + 1,
            },
        );
        assert_eq!(
            values["ledger.need_ready_tickets_due"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["fleet-ledger:need_ready_tickets_due:1".into()],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn ledger_unavailable_sentinel_preserves_its_reason() {
        let row = serde_json::Map::from_iter([
            ("available".into(), serde_json::Value::from(0_i64)),
            (
                "unavailable_reason".into(),
                serde_json::Value::String("relay all-orders source is unavailable".into()),
            ),
        ]);
        assert_eq!(
            ledger_unavailable_reason("lane_orders_due", &[row]).unwrap(),
            Some("relay all-orders source is unavailable".into())
        );
        let available =
            serde_json::Map::from_iter([("available".into(), serde_json::Value::from(1_i64))]);
        assert_eq!(
            ledger_unavailable_reason("lane_orders_due", &[available]).unwrap(),
            None
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn need_ready_action_counts_only_named_state_entries_in_its_following_day() {
        let events = [
            crate::linear::NamedStateChange {
                at: 100.0,
                from: Some("Backlog".into()),
                to: Some("Ready".into()),
            },
            crate::linear::NamedStateChange {
                at: 200.001,
                from: Some("Ready".into()),
                to: Some("In Progress".into()),
            },
        ];
        assert!(ready_ticket_after_action(
            &events, 100_000, 200_000, "ready"
        ));
        assert!(!ready_ticket_after_action(
            &events, 100_001, 200_000, "ready"
        ));
        assert!(!ready_ticket_after_action(
            &events,
            100_000,
            200_000,
            "unstarted"
        ));
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn linear_marker_metrics_parse_review_data_and_disposition_rounds() {
        let comments = [
            crate::MarkerComment {
                id: Some("review".into()),
                created_at: Some(crate::drift::format_timestamp(20.0)),
                body: "<!-- reviewer repo=owner/repo id=SR-1 method=review pr=repo#7 run=a head=abc -->\n\n```yaml\nclean: false\nscope: full\nfindings:\n  - fnd: FND-1\n    severity: high\n    confidence: high\n    method: review\n    check: behavior\n    artifact_id: A-1\n    path: ''\n    lines: ''\n    excerpt: example\n    finding: example\n```".into(),
            },
            crate::MarkerComment {
                id: Some("disposition".into()),
                created_at: Some(crate::drift::format_timestamp(30.0)),
                body: "<!-- reviewer-dispositions repo=owner/repo id=SR-1 round=1 run=b head=abc mergeable=no -->\n\n```yaml\ndispositions: []\n```".into(),
            },
        ];
        let metrics = parse_linear_marker_metrics(&comments, 0.0, 60.0);
        assert_eq!(
            metrics["linear.review_markers"],
            QueryValue::Value {
                value: 2.0,
                evidence_refs: vec![
                    "linear-comment:disposition".into(),
                    "linear-comment:review".into()
                ],
            }
        );
        assert_eq!(
            metrics["linear.disposition_rounds"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:disposition".into()]
            }
        );
        assert_eq!(
            metrics["linear.review_findings"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:review".into()]
            }
        );
        assert_eq!(
            metrics["linear.findings_without_location_or_scenario"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:review".into()]
            }
        );
        assert_eq!(
            metrics["linear.review_comments_unparseable_or_incomplete"],
            QueryValue::Value {
                value: 0.0,
                evidence_refs: vec![]
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-7")]
    fn session_filter_keeps_only_the_requested_session_row() {
        let session = |id: &str| SessionResult {
            id: id.into(),
            role: "coder".into(),
            objective_metrics: Vec::new(),
            score: Score {
                passed: 0,
                evaluated: 0,
                pass_rate: None,
            },
            rules: Vec::new(),
            failing_rules: Vec::new(),
        };
        let filtered = filter_sessions(vec![session("s1"), session("s2")], Some("s2"));
        assert_eq!(
            filtered
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["s2"]
        );
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn session_metrics_require_one_role_session_for_team_level_values() {
        let spec = RoleSpec {
            role: "coder".into(),
            metrics: Vec::new(),
            rules: vec![EvaluationRule {
                id: "CODE-R1".into(),
                rule: "team metric".into(),
                evidence_source: "retro".into(),
                trigger: "retro.metric".into(),
                method: "deterministic".into(),
                query_or_rubric: "retro.metric".into(),
                comparator: "=".into(),
                target: 2.0,
                unit: "items".into(),
            }],
        };
        let adapters = BTreeMap::from([(
            "retro.metric".into(),
            QueryValue::Value {
                value: 2.0,
                evidence_refs: vec!["retro:metric:sample".into()],
            },
        )]);
        let row = |address: &str| {
            serde_json::Map::from_iter([
                (
                    "session_addr".into(),
                    serde_json::Value::String(address.into()),
                ),
                (
                    "session_role".into(),
                    serde_json::Value::String("coder".into()),
                ),
                ("started_at".into(), serde_json::Value::from(100_i64)),
            ])
        };
        let window = EvalWindow {
            start_ms: 0,
            end_ms: 200,
        };
        let one = evaluate_sessions(
            std::slice::from_ref(&spec),
            &adapters,
            &[row("server:ws:tab")],
            &window,
            None,
        );
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].rules[0].status, RuleStatus::Pass);
        let two = evaluate_sessions(
            &[spec],
            &adapters,
            &[row("server:ws:tab"), row("server:ws:other")],
            &window,
            Some("server:ws:tab"),
        );
        assert_eq!(two.len(), 1);
        assert_eq!(two[0].rules[0].status, RuleStatus::Unavailable);
        assert!(
            two[0].rules[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("session-specific")
        );
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn role_turns_are_counted_from_session_attributed_prompt_events() {
        let spec = RoleSpec {
            role: "minder".into(),
            metrics: Vec::new(),
            rules: vec![EvaluationRule {
                id: "MIND-R7".into(),
                rule: "turns".into(),
                evidence_source: "hooks".into(),
                trigger: "hooks.role_turns".into(),
                method: "deterministic".into(),
                query_or_rubric: "hooks.role_turns".into(),
                comparator: "=".into(),
                target: 2.0,
                unit: "turns".into(),
            }],
        };
        let event = |address: &str, role: &str, started_at: i64| {
            serde_json::Map::from_iter([
                (
                    "session_addr".into(),
                    serde_json::Value::String(address.into()),
                ),
                (
                    "session_role".into(),
                    serde_json::Value::String(role.into()),
                ),
                ("started_at".into(), serde_json::Value::from(started_at)),
            ])
        };
        let session = "test:workspace:minder";
        let rows = [
            event(session, "minder", 110),
            event(session, "minder", 120),
            event("test:workspace:other", "minder", 130),
            event(session, "coder", 140),
            event(session, "minder", 200),
        ];
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 200,
        };
        let results = evaluate_sessions(&[spec], &BTreeMap::new(), &rows, &window, Some(session));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, session);
        assert_eq!(results[0].rules[0].status, RuleStatus::Pass);
        assert_eq!(results[0].rules[0].value, Some(2.0));
        assert_eq!(
            results[0].rules[0].evidence_refs,
            [
                "fleet-ledger:session_turn:test:workspace:minder:110",
                "fleet-ledger:session_turn:test:workspace:minder:120",
            ]
        );
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn role_turn_adapter_reports_count_or_source_error() {
        let spec = RoleSpec {
            role: "minder".into(),
            metrics: Vec::new(),
            rules: vec![EvaluationRule {
                id: "MIND-R7".into(),
                rule: "turns".into(),
                evidence_source: "hooks".into(),
                trigger: "hooks.role_turns".into(),
                method: "deterministic".into(),
                query_or_rubric: "hooks.role_turns".into(),
                comparator: "=".into(),
                target: 1.0,
                unit: "turns".into(),
            }],
        };
        let row = serde_json::Map::from_iter([
            (
                "session_addr".into(),
                serde_json::Value::String("test:workspace:minder".into()),
            ),
            (
                "session_role".into(),
                serde_json::Value::String("minder".into()),
            ),
            ("started_at".into(), serde_json::Value::from(150_i64)),
        ]);
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 200,
        };
        let adapters = session_turn_adapters(&spec, Ok(std::slice::from_ref(&row)), &window);
        assert_eq!(
            adapters["hooks.role_turns"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["fleet-ledger:session_turn:test:workspace:minder:150".into()],
            }
        );
        let unavailable = session_turn_adapters(&spec, Err("view missing"), &window);
        assert_eq!(
            unavailable["hooks.role_turns"],
            QueryValue::Unavailable {
                reason: "session_turn ledger view unavailable: view missing".into(),
                evidence_refs: Vec::new(),
            }
        );
    }
}
