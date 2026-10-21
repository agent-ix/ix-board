// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Typed evidence and deterministic alert classification for `drift` reports.

use crate::WorkflowStateType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use thiserror::Error as ThisError;

/// A configured team leader and the Linear team they own.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftLead {
    /// Stable lead identifier.
    pub lead_id: String,
    /// Linear team key owned by the lead.
    pub team_key: String,
}

/// Thresholds and reporting interval for a drift read.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftConfig {
    /// End of the half-open reporting interval, as RFC 3339.
    pub report_time: String,
    /// Positive interval length in hours.
    pub window_hours: f64,
    /// Positive Coding stall interval in hours.
    pub stall_hours: f64,
    /// Nonnegative observed spend threshold.
    pub spend_threshold: f64,
    /// Maximum allowed net ticket growth before churn is considered.
    pub max_net_ticket_growth: i64,
    /// Minimum merged PR count to avoid growth-based churn.
    pub min_merged_prs_for_churn: u64,
    /// Leads in the order used by every output format.
    pub leads: Vec<DriftLead>,
    /// Explicit active priority goals to monitor for crowding.
    #[serde(default)]
    pub priority_goals: Vec<PriorityGoalConfig>,
}

/// A project or milestone that must retain a minimum assigned worker count.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorityGoalConfig {
    /// Exact project or milestone identifier as exposed by Linear.
    pub goal: String,
    /// Explicit issue identifiers when the goal is not represented by a Linear project or milestone.
    #[serde(default)]
    pub ticket_ids: Vec<String>,
    /// Positive minimum number of distinct active assignees required for the goal.
    pub minimum_workers: u64,
    /// Hours without goal progress before crowding is reported.
    pub no_progress_hours: f64,
}

/// A project state used to reconstruct a queue at a Coding transition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoricalProject {
    /// Stable project ID.
    pub id: String,
    /// Human-readable project identifier, when present.
    pub identifier: Option<String>,
    /// Team keys associated with the project.
    pub team_keys: Vec<String>,
    /// Project run status immediately before the transition.
    pub status: String,
    /// Linear priority (zero means none; lower positive values rank first).
    pub priority: i32,
    /// Manual project order.
    pub sort_order: f64,
}

/// A ticket state used to reconstruct the queue at a Coding transition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoricalTicket {
    /// Stable Linear issue ID.
    pub id: String,
    /// Human-readable ticket ID.
    pub identifier: String,
    /// Owning team key.
    pub team_key: String,
    /// Associated project ID, if any.
    pub project_id: Option<String>,
    /// Workflow state immediately before the transition.
    pub state: String,
    /// Spec workflow state immediately before the transition.
    pub spec_state: String,
    /// Linear ticket priority.
    pub priority: f64,
    /// Manual ticket order.
    pub sort_order: f64,
    /// IDs of direct prerequisites.
    pub blocked_by: Vec<String>,
}

/// Full contemporaneous state immediately before a Coding transition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueueEvidence {
    /// Projects available in the historical read.
    pub projects: Vec<HistoricalProject>,
    /// Tickets available in the historical read, including cross-team blockers.
    pub tickets: Vec<HistoricalTicket>,
    /// Any unenumerated historical input makes this transition unavailable.
    pub missing_inputs: Vec<String>,
}

/// A ticket and project at report time.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftTicket {
    /// Stable Linear issue ID.
    pub id: String,
    /// Human-readable ticket ID.
    pub identifier: String,
    /// Owning team key.
    pub team_key: String,
    /// Current assignee identity, when the Linear issue is assigned.
    #[serde(default)]
    pub assignee_id: Option<String>,
    /// Associated project ID, if any.
    pub project_id: Option<String>,
    /// Current project title, when known.
    pub project_identifier: Option<String>,
    /// Current milestone identifier, when known.
    #[serde(default)]
    pub milestone_identifier: Option<String>,
    /// GitHub pull request URLs attached to this issue in Linear.
    #[serde(default)]
    pub linked_pr_urls: Vec<String>,
    /// Current workflow state.
    pub state: String,
    /// Linear workflow category for custom state names.
    #[serde(default = "default_unstarted_state")]
    pub state_type: WorkflowStateType,
    /// Current spec state.
    pub spec_state: String,
    /// Current ticket priority.
    pub priority: f64,
    /// Current manual ticket order.
    pub sort_order: f64,
    /// Current blocker edges.
    pub blocked_by: Vec<String>,
    /// Ticket creation time as RFC 3339.
    pub created_at: String,
    /// Most recent issue update time as RFC 3339, when available.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Time of entry into the current state, when known.
    pub state_entered_at: Option<String>,
}

fn default_unstarted_state() -> WorkflowStateType {
    WorkflowStateType::Unstarted
}

/// A Linear issue state transition event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueTransition {
    /// Stable event ID used for de-duplication.
    pub event_id: String,
    /// Stable issue ID.
    pub issue_id: String,
    /// Human-readable issue ID.
    pub ticket_id: String,
    /// New workflow state.
    pub to_state: String,
    /// Transition time as RFC 3339.
    pub occurred_at: String,
    /// Historical queue inputs immediately before this transition.
    pub queue_before: Option<QueueEvidence>,
}

/// A GitHub pull request with structured metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftPullRequest {
    /// Repository in `owner/name` form.
    pub repository: String,
    /// Repository-local PR number.
    pub number: u64,
    /// Canonical PR URL.
    pub url: String,
    /// PR title.
    pub title: String,
    /// Head branch name.
    pub branch: String,
    /// Creation time as RFC 3339.
    pub created_at: String,
    /// Merge time as RFC 3339, when merged.
    pub merged_at: Option<String>,
    /// Most recent GitHub update timestamp, when supplied by the source.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Stable GitHub event IDs present in the paginated source response.
    pub event_ids: Vec<String>,
}

/// A commit associated with ticket identifiers in its structured branch or message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftCommit {
    /// Stable Git commit SHA.
    pub sha: String,
    /// Repository in `owner/name` form.
    pub repository: String,
    /// Commit time as RFC 3339.
    pub committed_at: String,
    /// Structured ticket IDs extracted from branch and commit title.
    pub ticket_ids: Vec<String>,
}

/// A signal source and a page or partition that could not be fully read.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissingEvidence {
    /// Source name, such as `github.prs` or `linear.history`.
    pub source: String,
    /// Missing page, partition, cursor, or source failure.
    pub reason: String,
}

/// Complete structured records and explicit completeness for a drift read.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftEvidence {
    /// Current Linear projects.
    pub projects: Vec<HistoricalProject>,
    /// Current Linear tickets.
    pub tickets: Vec<DriftTicket>,
    /// Linear creation and state-transition events.
    pub transitions: Vec<IssueTransition>,
    /// GitHub PR records, including PRs created before the reporting window.
    pub pull_requests: Vec<DriftPullRequest>,
    /// GitHub commit records.
    pub commits: Vec<DriftCommit>,
    /// Current herdr busy state by configured lead ID.
    pub herdr_busy: BTreeMap<String, bool>,
    /// Observed OTel spend by configured lead ID for the reporting window.
    pub otel_spend: BTreeMap<String, f64>,
    /// Missing or incomplete source pages and partitions.
    pub missing: Vec<MissingEvidence>,
}

/// Versioned file envelope accepted by `ix-board drift --evidence`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftEvidenceFile {
    /// Evidence contract version; currently `1`.
    pub schema_version: u32,
    /// Source records and completeness attestations.
    pub evidence: DriftEvidence,
}

/// Configuration error for an unsupported evidence file version.
pub const DRIFT_EVIDENCE_SCHEMA_VERSION: u32 = 1;

/// Shared availability fields for one signal group.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignalAvailability {
    /// Whether all required source records were enumerated.
    pub availability: Availability,
    /// Sorted reasons for unavailable source evidence.
    pub missing_sources: Vec<String>,
}

/// Whether a signal has complete source evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Required source pages and partitions are complete.
    Available,
    /// At least one required source page or partition is missing.
    Unavailable,
}

/// The PR-to-ticket scope signal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrScopeSignal {
    /// Source completeness.
    #[serde(flatten)]
    pub common: SignalAvailability,
    /// Attributed PRs created during the reporting interval.
    pub prs: Vec<ScopedPullRequest>,
}

/// One attributed PR's scope classification.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopedPullRequest {
    /// Repository in `owner/name` form.
    pub repository: String,
    /// PR number.
    pub number: u64,
    /// Canonical PR URL.
    pub url: String,
    /// Attributed ticket identifier.
    pub ticket_id: String,
    /// Project ID, if the ticket has one.
    pub project_id: Option<String>,
    /// Scope relative to the lead's running projects.
    pub classification: ScopeClassification,
}

/// Whether an attributed new PR belongs to a lead's running project scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScopeClassification {
    /// Ticket belongs to a running project of this lead.
    InScope,
    /// Ticket belongs to this lead but not a running project.
    OutOfScope,
}

/// A Coding transition that was outside the ready queue or jumped its order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VeeringTicket {
    /// Human-readable ticket identifier.
    pub ticket_id: String,
    /// Transition time as RFC 3339.
    pub transitioned_at: String,
    /// Why the transition veered from the queue.
    pub reason: VeeringReason,
    /// Highest-ranked skipped ticket for a priority inversion.
    pub skipped_ticket_id: Option<String>,
}

/// Queue-veering reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VeeringReason {
    /// Ticket was outside the lead's running project scope.
    OutsideRunningProject,
    /// A higher-ranked ready ticket was skipped.
    PriorityInversion,
}

/// Coding-state queue veering signal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VeeringSignal {
    /// Source completeness.
    #[serde(flatten)]
    pub common: SignalAvailability,
    /// Proven veering transitions.
    pub tickets: Vec<VeeringTicket>,
}

/// A Coding ticket that has been idle longer than the configured stall interval.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StalledTicket {
    /// Human-readable ticket identifier.
    pub ticket_id: String,
    /// Time the ticket entered Coding.
    pub coding_since: String,
    /// Elapsed Coding time in hours.
    pub elapsed_hours: f64,
    /// Most recent ticket-attributed PR or commit time, if any.
    pub last_activity_at: Option<String>,
}

/// Coding stall signal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StallSignal {
    /// Source completeness.
    #[serde(flatten)]
    pub common: SignalAvailability,
    /// Proven stalls.
    pub tickets: Vec<StalledTicket>,
}

/// Busy or spending without an attributed new PR.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BusyWithoutPrSignal {
    /// Source completeness.
    #[serde(flatten)]
    pub common: SignalAvailability,
    /// Whether activity without a new PR is established; null means unavailable.
    pub finding: Option<bool>,
    /// Current herdr busy observation.
    pub herdr_busy: Option<bool>,
    /// Observed spend, if available.
    pub otel_spend: Option<f64>,
    /// Attributed new PR count, if GitHub enumeration is complete.
    pub new_pr_count: Option<u64>,
}

/// Ticket and merged-PR counts for a lead's running scope.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TicketChurnSignal {
    /// Source completeness.
    #[serde(flatten)]
    pub common: SignalAvailability,
    /// Tickets created during the interval.
    pub created: Option<u64>,
    /// Tickets completed during the interval.
    pub completed: Option<u64>,
    /// Attributed PRs merged during the interval.
    pub merged_prs: Option<u64>,
    /// Created minus completed, when both counts are available.
    pub net_growth: Option<i64>,
}

/// Five evidence-bearing signal groups for a configured lead.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeadSignals {
    /// New attributed PRs and scope.
    pub pr_scope: PrScopeSignal,
    /// Coding transitions that diverged from the contemporaneous queue.
    pub veering: VeeringSignal,
    /// Coding stalls without recent attributed activity.
    pub stall: StallSignal,
    /// Activity without a newly created attributed PR.
    pub busy_without_pr: BusyWithoutPrSignal,
    /// Ticket growth and merged PR counts.
    pub ticket_churn: TicketChurnSignal,
}

/// One of the two configured alerts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftAlert {
    /// Alert kind.
    pub kind: AlertKind,
    /// Evidence-based reasons for the alert.
    pub reasons: Vec<String>,
}

/// The two alert kinds derived from the five signal groups.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Out-of-scope attributed PR or queue-veering transition.
    OutOfScope,
    /// Coding stall, busy-without-PR, or low delivery under ticket growth.
    Churning,
}

/// A configured lead's ordered projects, status, alerts, reasons, and signals.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeadDriftReport {
    /// Configured stable lead ID.
    pub lead_id: String,
    /// Configured Linear team key.
    pub team_key: String,
    /// Running projects in queue order.
    pub running_projects: Vec<RunningProject>,
    /// `on_track`, `flagged`, or `unknown`.
    pub status: LeadStatus,
    /// Zero to two alerts, ordered by alert kind.
    pub alerts: Vec<DriftAlert>,
    /// Reasons for flagged or unknown status; empty when on track.
    pub reasons: Vec<String>,
    /// All five required signal groups.
    pub signals: LeadSignals,
}

/// One running project in queue order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunningProject {
    /// Stable project ID.
    pub id: String,
    /// Human-readable project identifier.
    pub identifier: Option<String>,
}

/// Lead status after evaluating both alerts and required signal completeness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeadStatus {
    /// Both alert conditions were false with complete evidence.
    OnTrack,
    /// At least one alert is proven.
    Flagged,
    /// No alert is proven, but required evidence is unavailable.
    Unknown,
}

/// A PR that could not be attributed to a configured lead.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnassignedPullRequest {
    /// Repository in `owner/name` form.
    pub repository: String,
    /// PR number.
    pub number: u64,
    /// Canonical PR URL.
    pub url: String,
    /// PR title.
    pub title: String,
    /// Head branch.
    pub branch: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Merge timestamp if merged.
    pub merged_at: Option<String>,
    /// Distinct ticket IDs extracted from branch/title.
    pub ticket_ids: Vec<String>,
    /// Why attribution could not be completed.
    pub reason: UnassignedReason,
}

/// Why an observed PR lacks lead attribution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnassignedReason {
    /// No ticket ID was found in branch or title.
    MissingTicketId,
    /// More than one distinct ticket ID was found.
    ConflictingTicketIds,
    /// The unique ticket ID is absent from Linear.
    UnresolvedTicketId,
    /// The ticket belongs to a team with no configured lead.
    UnconfiguredTeam,
}

/// Stable typed output of one `drift` read.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriftReport {
    /// RFC 3339 UTC report time.
    pub report_time: String,
    /// RFC 3339 UTC interval start.
    pub window_start: String,
    /// RFC 3339 UTC interval end, equal to report time.
    pub window_end: String,
    /// Leads in configured order.
    pub leads: Vec<LeadDriftReport>,
    /// Configured priority-goal health, in config order.
    #[serde(default)]
    pub priority_goals: Vec<PriorityGoalReport>,
    /// Global unassigned PRs, ordered by repository and number.
    pub unassigned_prs: Vec<UnassignedPullRequest>,
}

/// Progress, staffing, and competing transitions for one configured priority goal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorityGoalReport {
    /// Configured project or milestone identifier.
    pub goal: String,
    /// Configured threshold for stale progress, in hours.
    pub no_progress_hours: f64,
    /// Last issue completion timestamp on this goal, if observed.
    pub last_completion_at: Option<String>,
    /// Last issue state transition timestamp on this goal, if observed.
    pub last_transition_at: Option<String>,
    /// Elapsed UTC hours since the latest state transition or completion.
    pub hours_since_progress: Option<f64>,
    /// Distinct nonempty assignee IDs on active goal issues and open blockers, if complete.
    pub active_workers: Option<u64>,
    /// Configured minimum worker coverage.
    pub minimum_workers: u64,
    /// Current state of the goal assessment.
    #[serde(default)]
    pub status: PriorityGoalStatus,
    /// Open direct or transitive blockers currently holding goal issues.
    #[serde(default)]
    pub blocked_on: Vec<String>,
    /// Other projects with state transitions in the no-progress window, with ticket IDs.
    pub crowding_out: Vec<CrowdingOutProject>,
    /// Out-of-team activity that might involve this lead but has no worker attribution.
    #[serde(default)]
    pub out_of_team_activity: Vec<CrowdingOutProject>,
    /// Missing evidence that prevents a complete activity-age assessment.
    pub missing_sources: Vec<String>,
    /// False when missing sources can hide linked work relevant to this assessment.
    #[serde(default)]
    pub trusted: bool,
    /// Whether the goal lacks recent activity or active work coverage.
    pub flagged: Option<bool>,
}

/// Whether a priority goal is blocked, drifting, healthy, or lacks enough evidence.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriorityGoalStatus {
    /// Evidence cannot support a reliable activity assessment.
    #[default]
    Unknown,
    /// Every open goal issue is waiting on an open blocker.
    Blocked,
    /// The configured staffing or no-progress threshold was breached.
    Flagged,
    /// The goal has enough evidence and meets its thresholds.
    OnTrack,
}

/// Other-project transitions that coincided with a stalled priority goal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrowdingOutProject {
    /// Project identifier, or `(no project)` when the issue has no project.
    pub project: String,
    /// Owning Linear team for the listed tickets.
    #[serde(default)]
    pub team_key: String,
    /// Transitioning ticket identifiers in deterministic order.
    pub tickets: Vec<String>,
}

/// Invalid drift configuration or evidence that cannot be interpreted safely.
#[derive(Debug, ThisError)]
pub enum DriftError {
    /// A required scalar or lead field was invalid.
    #[error("invalid drift configuration: {reason}")]
    InvalidConfig { reason: String },
    /// The timestamp was not RFC 3339 with a valid calendar date and offset.
    #[error("invalid RFC 3339 timestamp '{value}'")]
    InvalidTimestamp { value: String },
    /// A source value is non-finite and cannot be emitted as JSON.
    #[error("non-finite number in {field}")]
    NonFinite { field: &'static str },
    /// Evidence JSON version is unknown to this binary.
    #[error("unsupported drift evidence schema version {version}; expected {expected}")]
    UnsupportedEvidenceVersion { version: u32, expected: u32 },
}

/// Evaluates complete or explicitly partial source evidence into a stable report.
pub fn evaluate_drift(
    config: &DriftConfig,
    evidence: &DriftEvidence,
) -> Result<DriftReport, DriftError> {
    validate_config(config)?;
    validate_evidence_numbers(evidence)?;
    let report_epoch = timestamp_epoch(&config.report_time)?;
    let window_seconds = config.window_hours * 3_600.0;
    let window_start_epoch = report_epoch - window_seconds;
    let earliest_timestamp = days_from_civil(1, 1, 1) as f64 * 86_400.0;
    if !window_start_epoch.is_finite() || window_start_epoch < earliest_timestamp {
        return Err(DriftError::InvalidConfig {
            reason: "reporting window starts outside the supported RFC 3339 range".to_owned(),
        });
    }
    let window_start = format_timestamp(window_start_epoch);
    let running_projects = configured_running_projects(config, evidence);
    let ticket_by_id: HashMap<_, _> = evidence
        .tickets
        .iter()
        .map(|ticket| (ticket.id.as_str(), ticket))
        .collect();
    let ticket_by_identifier: HashMap<_, _> = evidence
        .tickets
        .iter()
        .map(|ticket| (ticket.identifier.as_str(), ticket))
        .collect();
    let leads_by_team: HashMap<_, _> = config
        .leads
        .iter()
        .map(|lead| (lead.team_key.as_str(), lead))
        .collect();
    let pull_requests = deduplicated_prs(&evidence.pull_requests);
    let transitions = deduplicated_transitions(&evidence.transitions);
    let commits = deduplicated_commits(&evidence.commits);
    let mut leads = Vec::with_capacity(config.leads.len());

    for lead in &config.leads {
        let lead_projects = running_projects
            .get(lead.team_key.as_str())
            .cloned()
            .unwrap_or_default();
        let project_ids: HashSet<_> = lead_projects
            .iter()
            .map(|project| project.id.as_str())
            .collect();
        let mut scope_prs = Vec::new();
        let mut pr_missing = missing_for(
            evidence,
            &["github.prs", "linear.issues", "linear.projects"],
        );
        for pr in &pull_requests {
            let created = timestamp_epoch(&pr.created_at)?;
            let merged_in_window = pr
                .merged_at
                .as_deref()
                .map(timestamp_epoch)
                .transpose()?
                .is_some_and(|merged| in_window(merged, window_start_epoch, report_epoch));
            if !in_window(created, window_start_epoch, report_epoch) && !merged_in_window {
                continue;
            }
            let ticket_ids = extract_ticket_ids(&pr.branch, &pr.title);
            let Some(ticket_id) = ticket_ids
                .first()
                .filter(|_| ticket_ids.len() == 1)
                .map(String::as_str)
            else {
                continue;
            };
            let Some(ticket) = ticket_by_identifier.get(ticket_id).copied() else {
                continue;
            };
            let Some(owner) = leads_by_team.get(ticket.team_key.as_str()) else {
                continue;
            };
            if owner.lead_id != lead.lead_id
                || !in_window(created, window_start_epoch, report_epoch)
            {
                continue;
            }
            if let Some(project_id) = ticket.project_id.as_deref()
                && !evidence
                    .projects
                    .iter()
                    .any(|project| project.id == project_id)
            {
                pr_missing.insert(format!(
                    "linear.projects: project {project_id} for {} is unresolved",
                    ticket.identifier
                ));
                continue;
            }
            scope_prs.push(ScopedPullRequest {
                repository: pr.repository.clone(),
                number: pr.number,
                url: pr.url.clone(),
                ticket_id: ticket.identifier.clone(),
                project_id: ticket.project_id.clone(),
                classification: if ticket
                    .project_id
                    .as_deref()
                    .is_some_and(|project_id| project_ids.contains(project_id))
                {
                    ScopeClassification::InScope
                } else {
                    ScopeClassification::OutOfScope
                },
            });
        }
        scope_prs.sort_by(|left, right| {
            left.repository
                .cmp(&right.repository)
                .then_with(|| left.number.cmp(&right.number))
        });
        let mut veering = Vec::new();
        let mut veering_missing = missing_for(evidence, &["linear.history"]);
        for transition in &transitions {
            let occurred = timestamp_epoch(&transition.occurred_at)?;
            if transition.to_state != "Coding"
                || !in_window(occurred, window_start_epoch, report_epoch)
            {
                continue;
            }
            let Some(ticket) = ticket_by_id.get(transition.issue_id.as_str()).copied() else {
                veering_missing.insert(format!(
                    "linear.history: ticket {} has no current issue record",
                    transition.ticket_id
                ));
                continue;
            };
            if ticket.team_key != lead.team_key {
                continue;
            }
            if matches!(
                ticket.state.to_ascii_lowercase().as_str(),
                "completed" | "done" | "canceled" | "cancelled"
            ) {
                continue;
            }
            match transition
                .queue_before
                .as_ref()
                .map(|queue| reconstruct_veering(queue, &lead.team_key, &ticket.identifier))
            {
                Some(Ok(Some((reason, skipped)))) => veering.push(VeeringTicket {
                    ticket_id: ticket.identifier.clone(),
                    transitioned_at: canonical_timestamp(&transition.occurred_at)?,
                    reason,
                    skipped_ticket_id: skipped,
                }),
                Some(Ok(None)) => {}
                Some(Err(reason)) => {
                    veering_missing.insert(format!("linear.history: {reason}"));
                }
                None => {
                    veering_missing.insert(format!(
                        "linear.history: queue before {} is unavailable",
                        transition.ticket_id
                    ));
                }
            }
        }
        veering.sort_by(|left, right| {
            left.ticket_id
                .cmp(&right.ticket_id)
                .then_with(|| left.transitioned_at.cmp(&right.transitioned_at))
        });

        let mut stalls = Vec::new();
        let mut stall_missing = missing_for(
            evidence,
            &[
                "linear.issues",
                "linear.projects",
                "github.prs",
                "github.commits",
            ],
        );
        let trailing_start = report_epoch - config.stall_hours * 3_600.0;
        let github_activity_complete =
            missing_for(evidence, &["github.prs", "github.commits"]).is_empty();
        for ticket in evidence.tickets.iter().filter(|ticket| {
            github_activity_complete
                && ticket.team_key == lead.team_key
                && ticket.state == "Coding"
                && ticket
                    .project_id
                    .as_deref()
                    .is_some_and(|project_id| project_ids.contains(project_id))
        }) {
            let Some(coding_since) = ticket.state_entered_at.as_deref() else {
                stall_missing.insert(format!(
                    "linear.issues: {} lacks Coding start time",
                    ticket.identifier
                ));
                continue;
            };
            let coding_epoch = timestamp_epoch(coding_since)?;
            let elapsed = (report_epoch - coding_epoch) / 3_600.0;
            if elapsed <= config.stall_hours {
                continue;
            }
            let last_activity = last_activity_for(ticket, &pull_requests, &commits, report_epoch)?;
            if last_activity
                .as_deref()
                .map(timestamp_epoch)
                .transpose()?
                .is_some_and(|at| at >= trailing_start && at < report_epoch)
            {
                continue;
            }
            stalls.push(StalledTicket {
                ticket_id: ticket.identifier.clone(),
                coding_since: canonical_timestamp(coding_since)?,
                elapsed_hours: elapsed,
                last_activity_at: last_activity,
            });
        }
        stalls.sort_by(|left, right| left.ticket_id.cmp(&right.ticket_id));

        let new_pr_missing = missing_for(evidence, &["github.prs", "linear.issues"]);
        let new_pr_count = complete_count(&new_pr_missing)
            .then(|| u64::try_from(scope_prs.len()).ok())
            .flatten();
        let busy_observed = evidence.herdr_busy.get(&lead.lead_id).copied();
        let spend_observed = evidence.otel_spend.get(&lead.lead_id).copied();
        let mut busy_missing =
            missing_for(evidence, &["herdr", "otel", "github.prs", "linear.issues"]);
        if busy_observed.is_none() {
            busy_missing.insert(format!("herdr: no observation for lead {}", lead.lead_id));
        }
        if spend_observed.is_none() {
            busy_missing.insert(format!(
                "otel: no spend observation for lead {}",
                lead.lead_id
            ));
        }
        let activity_proven = busy_observed == Some(true)
            || spend_observed.is_some_and(|spend| spend > 0.0 && spend >= config.spend_threshold);
        let activity_known_false = busy_observed == Some(false)
            && spend_observed
                .is_some_and(|spend| !(spend > 0.0 && spend >= config.spend_threshold));
        let busy_finding = match new_pr_count {
            Some(0) if activity_proven => Some(true),
            Some(0) if activity_known_false && busy_missing.is_empty() => Some(false),
            Some(count) if count > 0 => Some(false),
            _ => None,
        };

        let churn = make_churn_signal(
            config,
            evidence,
            &lead.team_key,
            &project_ids,
            window_start_epoch,
            report_epoch,
            &ticket_by_id,
            &leads_by_team,
        )?;
        let pr_signal = PrScopeSignal {
            common: availability(pr_missing),
            prs: scope_prs,
        };
        let veering_signal = VeeringSignal {
            common: availability(veering_missing),
            tickets: veering,
        };
        let stall_signal = StallSignal {
            common: availability(stall_missing),
            tickets: stalls,
        };
        let busy_signal = BusyWithoutPrSignal {
            common: availability(busy_missing),
            finding: busy_finding,
            herdr_busy: busy_observed,
            otel_spend: spend_observed,
            new_pr_count,
        };
        let signals = LeadSignals {
            pr_scope: pr_signal,
            veering: veering_signal,
            stall: stall_signal,
            busy_without_pr: busy_signal,
            ticket_churn: churn,
        };
        let (alerts, status, reasons) = derive_alerts(&signals, config);
        leads.push(LeadDriftReport {
            lead_id: lead.lead_id.clone(),
            team_key: lead.team_key.clone(),
            running_projects: lead_projects,
            status,
            alerts,
            reasons,
            signals,
        });
    }
    let priority_goals = evaluate_priority_goals(config, evidence, &transitions, report_epoch)?;
    let mut unassigned_prs = collect_unassigned_prs(
        &pull_requests,
        window_start_epoch,
        report_epoch,
        &ticket_by_identifier,
        &leads_by_team,
    )?;
    unassigned_prs.sort_by(|left, right| {
        left.repository
            .cmp(&right.repository)
            .then_with(|| left.number.cmp(&right.number))
    });
    unassigned_prs
        .dedup_by(|left, right| left.repository == right.repository && left.number == right.number);
    Ok(DriftReport {
        report_time: canonical_timestamp(&config.report_time)?,
        window_start,
        window_end: canonical_timestamp(&config.report_time)?,
        leads,
        priority_goals,
        unassigned_prs,
    })
}

/// Validates a versioned evidence envelope and evaluates its contents.
pub fn evaluate_drift_file(
    config: &DriftConfig,
    evidence_file: &DriftEvidenceFile,
) -> Result<DriftReport, DriftError> {
    if evidence_file.schema_version != DRIFT_EVIDENCE_SCHEMA_VERSION {
        return Err(DriftError::UnsupportedEvidenceVersion {
            version: evidence_file.schema_version,
            expected: DRIFT_EVIDENCE_SCHEMA_VERSION,
        });
    }
    evaluate_drift(config, &evidence_file.evidence)
}

fn validate_config(config: &DriftConfig) -> Result<(), DriftError> {
    if config.report_time.trim().is_empty() {
        return Err(DriftError::InvalidConfig {
            reason: "report_time must not be empty".to_owned(),
        });
    }
    timestamp_epoch(&config.report_time)?;
    let mut goals = HashSet::new();
    for goal in &config.priority_goals {
        if goal.goal.trim().is_empty() || goal.minimum_workers == 0 {
            return Err(DriftError::InvalidConfig {
                reason:
                    "priority goal identifier and minimum_workers must be nonempty and positive"
                        .to_owned(),
            });
        }
        if !goal.no_progress_hours.is_finite() || goal.no_progress_hours <= 0.0 {
            return Err(DriftError::InvalidConfig {
                reason: format!(
                    "priority goal '{}' no_progress_hours must be positive and finite",
                    goal.goal
                ),
            });
        }
        if !goals.insert(goal.goal.as_str()) {
            return Err(DriftError::InvalidConfig {
                reason: format!("duplicate priority goal '{}'", goal.goal),
            });
        }
        if goal.ticket_ids.iter().any(|id| id.trim().is_empty()) {
            return Err(DriftError::InvalidConfig {
                reason: format!(
                    "priority goal '{}' has an empty ticket identifier",
                    goal.goal
                ),
            });
        }
    }
    if !config.window_hours.is_finite() || config.window_hours <= 0.0 {
        return Err(DriftError::InvalidConfig {
            reason: "window_hours must be positive and finite".to_owned(),
        });
    }
    if !config.stall_hours.is_finite() || config.stall_hours <= 0.0 {
        return Err(DriftError::InvalidConfig {
            reason: "stall_hours must be positive and finite".to_owned(),
        });
    }
    if !config.spend_threshold.is_finite() || config.spend_threshold < 0.0 {
        return Err(DriftError::InvalidConfig {
            reason: "spend_threshold must be nonnegative and finite".to_owned(),
        });
    }
    if config.max_net_ticket_growth < 0 {
        return Err(DriftError::InvalidConfig {
            reason: "max_net_ticket_growth must be nonnegative".to_owned(),
        });
    }
    let mut ids = HashSet::new();
    let mut teams = HashSet::new();
    for lead in &config.leads {
        if lead.lead_id.trim().is_empty() || lead.team_key.trim().is_empty() {
            return Err(DriftError::InvalidConfig {
                reason: "lead_id and team_key must be nonempty".to_owned(),
            });
        }
        if !ids.insert(lead.lead_id.as_str()) {
            return Err(DriftError::InvalidConfig {
                reason: format!("duplicate lead_id '{}'", lead.lead_id),
            });
        }
        if !teams.insert(lead.team_key.as_str()) {
            return Err(DriftError::InvalidConfig {
                reason: format!("team '{}' is assigned to more than one lead", lead.team_key),
            });
        }
    }
    Ok(())
}

fn evaluate_priority_goals(
    config: &DriftConfig,
    evidence: &DriftEvidence,
    transitions: &[IssueTransition],
    report_epoch: f64,
) -> Result<Vec<PriorityGoalReport>, DriftError> {
    config
        .priority_goals
        .iter()
        .map(|goal| {
            let belongs = |ticket: &DriftTicket| {
                ticket.project_identifier.as_deref() == Some(goal.goal.as_str())
                    || ticket.milestone_identifier.as_deref() == Some(goal.goal.as_str())
                    || goal
                        .ticket_ids
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&ticket.identifier))
            };
            let mut missing_sources = missing_for(
                evidence,
                &[
                    "linear.issues",
                    "linear.history",
                    "github.prs",
                    "github.commits",
                ],
            );
            let missing_goal_history = missing_for(evidence, &["linear.issues", "linear.history"]);
            let tickets_by_id: HashMap<_, _> = evidence
                .tickets
                .iter()
                .map(|ticket| (ticket.id.as_str(), ticket))
                .collect();
            let mut goal_ids = BTreeSet::new();
            let mut open_goal_ids = BTreeSet::new();
            let mut goal_teams = BTreeSet::new();
            let mut pending_blockers = Vec::new();
            for ticket in evidence.tickets.iter().filter(|ticket| belongs(ticket)) {
                if timestamp_epoch(&ticket.created_at)? <= report_epoch {
                    goal_ids.insert(ticket.id.clone());
                    goal_teams.insert(ticket.team_key.clone());
                    if !is_terminal_ticket(ticket) {
                        open_goal_ids.insert(ticket.id.clone());
                        pending_blockers.extend(
                            ticket
                                .blocked_by
                                .iter()
                                .cloned()
                                .map(|blocker_id| (ticket.id.clone(), blocker_id)),
                        );
                    }
                }
            }
            let mut blocker_ids = BTreeSet::new();
            let mut blocked_on = BTreeSet::new();
            let mut missing_blockers = BTreeSet::new();
            let mut blocked_goal_ids = BTreeSet::new();
            let mut visited_blocker_edges = BTreeSet::new();
            while let Some((blocked_goal_id, blocker_id)) = pending_blockers.pop() {
                if goal_ids.contains(&blocker_id)
                    || !visited_blocker_edges.insert((blocked_goal_id.clone(), blocker_id.clone()))
                {
                    continue;
                }
                blocker_ids.insert(blocker_id.clone());
                let Some(blocker) = tickets_by_id.get(blocker_id.as_str()).copied() else {
                    missing_blockers.insert(blocker_id);
                    continue;
                };
                if is_terminal_ticket(blocker) {
                    continue;
                }
                blocked_on.insert(blocker.identifier.clone());
                blocked_goal_ids.insert(blocked_goal_id.clone());
                pending_blockers.extend(
                    blocker
                        .blocked_by
                        .iter()
                        .cloned()
                        .map(|next| (blocked_goal_id.clone(), next)),
                );
            }
            let worker_evidence_complete =
                missing_for(evidence, &["linear.issues"]).is_empty() && missing_blockers.is_empty();
            for blocker_id in missing_blockers {
                missing_sources.insert(format!(
                    "linear.issues: blocker {blocker_id} for '{}' was not returned",
                    goal.goal
                ));
            }
            let linked_ids: BTreeSet<_> = goal_ids.union(&blocker_ids).cloned().collect();
            let goal_transitions: Vec<_> = transitions
                .iter()
                .filter(|event| goal_ids.contains(&event.issue_id))
                .map(|event| Ok((timestamp_epoch(&event.occurred_at)?, event)))
                .collect::<Result<_, DriftError>>()?;
            let last_transition = goal_transitions
                .iter()
                .map(|(at, event)| (*at, *event))
                .filter(|(at, _)| *at <= report_epoch)
                .max_by(|a, b| a.0.total_cmp(&b.0));
            let last_completion = goal_transitions
                .iter()
                .filter(|(_, event)| {
                    matches!(
                        event.to_state.to_ascii_lowercase().as_str(),
                        "completed" | "done"
                    )
                })
                .map(|(at, event)| (*at, *event))
                .filter(|(at, _)| *at <= report_epoch)
                .max_by(|a, b| a.0.total_cmp(&b.0));
            let mut active_workers = BTreeSet::new();
            let active_scope_tickets: Vec<_> = evidence
                .tickets
                .iter()
                .filter(|ticket| {
                    (open_goal_ids.contains(&ticket.id) || blocker_ids.contains(&ticket.id))
                        && timestamp_epoch(&ticket.created_at)
                            .is_ok_and(|created| created <= report_epoch)
                        && is_active_ticket(ticket)
                })
                .collect();
            for ticket in &active_scope_tickets {
                if let Some(assignee) = ticket
                    .assignee_id
                    .as_deref()
                    .filter(|assignee| !assignee.trim().is_empty())
                {
                    active_workers.insert(assignee);
                }
            }
            let history_complete = missing_goal_history.is_empty();
            let baseline = goal_ids
                .iter()
                .filter_map(|id| tickets_by_id.get(id.as_str()))
                .map(|ticket| timestamp_epoch(&ticket.created_at))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .min_by(f64::total_cmp);
            let progress_at = last_completion
                .map(|(at, _)| at)
                .into_iter()
                .chain(last_transition.map(|(at, _)| at))
                .max_by(f64::total_cmp)
                .or(baseline);
            let hours_since_progress = history_complete
                .then(|| progress_at.map(|at| ((report_epoch - at) / 3600.0).max(0.0)))
                .flatten();
            let stale_start = report_epoch - goal.no_progress_hours * 3600.0;
            let mut competing: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
            let mut out_of_team: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
            for event in transitions {
                let at = timestamp_epoch(&event.occurred_at)?;
                if at < stale_start || at > report_epoch || linked_ids.contains(&event.issue_id) {
                    continue;
                }
                let Some(ticket) = evidence
                    .tickets
                    .iter()
                    .find(|ticket| ticket.id == event.issue_id)
                else {
                    continue;
                };
                let project = ticket
                    .project_identifier
                    .as_deref()
                    .unwrap_or("(no project)");
                competing
                    .entry((ticket.team_key.clone(), project.to_owned()))
                    .or_default()
                    .insert(event.ticket_id.clone());
                if !goal_teams.contains(&ticket.team_key)
                    && ticket
                        .assignee_id
                        .as_deref()
                        .is_some_and(|assignee| active_workers.contains(assignee))
                {
                    out_of_team
                        .entry((ticket.team_key.clone(), project.to_owned()))
                        .or_default()
                        .insert(event.ticket_id.clone());
                }
            }
            for pr in &evidence.pull_requests {
                let activity_at = [
                    Some(pr.created_at.as_str()),
                    pr.updated_at.as_deref(),
                    pr.merged_at.as_deref(),
                ]
                .into_iter()
                .flatten()
                .map(timestamp_epoch)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .max_by(f64::total_cmp);
                if activity_at
                    .filter(|at| *at >= stale_start && *at <= report_epoch)
                    .is_none()
                {
                    continue;
                }
                for ticket in evidence.tickets.iter().filter(|ticket| {
                    !linked_ids.contains(&ticket.id) && ticket_links_pr(ticket, pr)
                }) {
                    let project = ticket
                        .project_identifier
                        .clone()
                        .unwrap_or_else(|| "(no project)".to_owned());
                    competing
                        .entry((ticket.team_key.clone(), project.clone()))
                        .or_default()
                        .insert(ticket.identifier.clone());
                    if !goal_teams.contains(&ticket.team_key)
                        && ticket
                            .assignee_id
                            .as_deref()
                            .is_some_and(|assignee| active_workers.contains(assignee))
                    {
                        out_of_team
                            .entry((ticket.team_key.clone(), project))
                            .or_default()
                            .insert(ticket.identifier.clone());
                    }
                }
            }
            for commit in &evidence.commits {
                let at = timestamp_epoch(&commit.committed_at)?;
                if at < stale_start || at > report_epoch {
                    continue;
                }
                for ticket in evidence.tickets.iter().filter(|ticket| {
                    !linked_ids.contains(&ticket.id)
                        && commit.ticket_ids.iter().any(|id| id == &ticket.identifier)
                }) {
                    let project = ticket
                        .project_identifier
                        .clone()
                        .unwrap_or_else(|| "(no project)".to_owned());
                    competing
                        .entry((ticket.team_key.clone(), project.clone()))
                        .or_default()
                        .insert(ticket.identifier.clone());
                    if !goal_teams.contains(&ticket.team_key)
                        && ticket
                            .assignee_id
                            .as_deref()
                            .is_some_and(|assignee| active_workers.contains(assignee))
                    {
                        out_of_team
                            .entry((ticket.team_key.clone(), project))
                            .or_default()
                            .insert(ticket.identifier.clone());
                    }
                }
            }
            let group_rows = |groups: BTreeMap<(String, String), BTreeSet<String>>| {
                groups
                    .into_iter()
                    .map(|((team_key, project), tickets)| CrowdingOutProject {
                        project,
                        team_key,
                        tickets: tickets.into_iter().collect(),
                    })
                    .collect::<Vec<_>>()
            };
            let crowding_out = group_rows(competing);
            let out_of_team_activity = group_rows(out_of_team);
            let stale = hours_since_progress.is_some_and(|hours| hours >= goal.no_progress_hours);
            let every_open_goal_ticket_blocked = !open_goal_ids.is_empty()
                && open_goal_ids
                    .iter()
                    .all(|goal_id| blocked_goal_ids.contains(goal_id));
            let trusted = missing_sources.is_empty();
            let goal_structure_complete = worker_evidence_complete;
            let staffing_shortfall = worker_evidence_complete
                && active_workers.len()
                    < usize::try_from(goal.minimum_workers).unwrap_or(usize::MAX);
            let stale_with_competing_work =
                missing_goal_history.is_empty() && stale && !crowding_out.is_empty();
            let raw_flag =
                !open_goal_ids.is_empty() && (staffing_shortfall || stale_with_competing_work);
            let flagged = if open_goal_ids.is_empty() {
                trusted.then_some(false)
            } else if every_open_goal_ticket_blocked {
                None
            } else if raw_flag {
                Some(true)
            } else {
                (trusted && !every_open_goal_ticket_blocked).then_some(raw_flag)
            };
            let status = if goal_structure_complete && every_open_goal_ticket_blocked {
                PriorityGoalStatus::Blocked
            } else if open_goal_ids.is_empty() && trusted {
                PriorityGoalStatus::OnTrack
            } else {
                match flagged {
                    Some(true) => PriorityGoalStatus::Flagged,
                    Some(false) => PriorityGoalStatus::OnTrack,
                    None => PriorityGoalStatus::Unknown,
                }
            };
            Ok(PriorityGoalReport {
                goal: goal.goal.clone(),
                no_progress_hours: goal.no_progress_hours,
                last_completion_at: history_complete
                    .then_some(last_completion)
                    .flatten()
                    .map(|(_, event)| canonical_timestamp(&event.occurred_at))
                    .transpose()?,
                last_transition_at: history_complete
                    .then_some(last_transition)
                    .flatten()
                    .map(|(_, event)| canonical_timestamp(&event.occurred_at))
                    .transpose()?,
                hours_since_progress,
                active_workers: worker_evidence_complete
                    .then(|| u64::try_from(active_workers.len()).unwrap_or(u64::MAX)),
                minimum_workers: goal.minimum_workers,
                status,
                blocked_on: blocked_on.into_iter().collect(),
                crowding_out,
                out_of_team_activity,
                missing_sources: missing_sources.iter().cloned().collect(),
                trusted,
                flagged,
            })
        })
        .collect()
}

fn ticket_links_pr(ticket: &DriftTicket, pr: &DriftPullRequest) -> bool {
    ticket.linked_pr_urls.iter().any(|url| url == &pr.url)
        || extract_ticket_ids(&pr.branch, &pr.title)
            .iter()
            .any(|identifier| identifier == &ticket.identifier)
}

fn is_terminal_ticket(ticket: &DriftTicket) -> bool {
    matches!(
        ticket.state.to_ascii_lowercase().as_str(),
        "completed" | "done" | "canceled" | "cancelled" | "duplicate"
    )
}

fn is_active_ticket(ticket: &DriftTicket) -> bool {
    ticket.state_type == WorkflowStateType::Started
        || matches!(
            ticket.state.to_ascii_lowercase().as_str(),
            "coding" | "in progress"
        )
}

fn validate_evidence_numbers(evidence: &DriftEvidence) -> Result<(), DriftError> {
    for project in &evidence.projects {
        if !project.sort_order.is_finite() {
            return Err(DriftError::NonFinite {
                field: "project.sort_order",
            });
        }
    }
    for ticket in &evidence.tickets {
        if !ticket.priority.is_finite() || !ticket.sort_order.is_finite() {
            return Err(DriftError::NonFinite {
                field: "ticket priority or sort_order",
            });
        }
    }
    for project in evidence
        .transitions
        .iter()
        .filter_map(|event| event.queue_before.as_ref())
        .flat_map(|queue| &queue.projects)
    {
        if !project.sort_order.is_finite() {
            return Err(DriftError::NonFinite {
                field: "historical project.sort_order",
            });
        }
    }
    for ticket in evidence
        .transitions
        .iter()
        .filter_map(|event| event.queue_before.as_ref())
        .flat_map(|queue| &queue.tickets)
    {
        if !ticket.priority.is_finite() || !ticket.sort_order.is_finite() {
            return Err(DriftError::NonFinite {
                field: "historical ticket priority or sort_order",
            });
        }
    }
    for spend in evidence.otel_spend.values() {
        if !spend.is_finite() || *spend < 0.0 {
            return Err(DriftError::NonFinite {
                field: "otel spend",
            });
        }
    }
    Ok(())
}

fn configured_running_projects(
    config: &DriftConfig,
    evidence: &DriftEvidence,
) -> BTreeMap<String, Vec<RunningProject>> {
    let mut result = BTreeMap::new();
    for lead in &config.leads {
        let mut projects = evidence
            .projects
            .iter()
            .filter(|project| {
                project.status == "Started"
                    && project.team_keys.iter().any(|team| team == &lead.team_key)
            })
            .collect::<Vec<_>>();
        projects.sort_by(|left, right| {
            project_priority(left.priority)
                .cmp(&project_priority(right.priority))
                .then_with(|| left.sort_order.total_cmp(&right.sort_order))
                .then_with(|| {
                    left.identifier
                        .as_deref()
                        .unwrap_or(&left.id)
                        .cmp(right.identifier.as_deref().unwrap_or(&right.id))
                })
                .then_with(|| left.id.cmp(&right.id))
        });
        result.insert(
            lead.team_key.clone(),
            projects
                .into_iter()
                .map(|project| RunningProject {
                    id: project.id.clone(),
                    identifier: project.identifier.clone(),
                })
                .collect(),
        );
    }
    result
}

fn project_priority(priority: i32) -> (bool, i32) {
    (priority == 0, priority)
}

fn missing_for(evidence: &DriftEvidence, sources: &[&str]) -> BTreeSet<String> {
    evidence
        .missing
        .iter()
        .filter(|missing| sources.contains(&missing.source.as_str()))
        .map(|missing| format!("{}: {}", missing.source, missing.reason))
        .collect()
}

fn availability(missing: BTreeSet<String>) -> SignalAvailability {
    SignalAvailability {
        availability: if missing.is_empty() {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        missing_sources: missing.into_iter().collect(),
    }
}

fn complete_count(missing: &BTreeSet<String>) -> bool {
    missing.is_empty()
}

fn in_window(value: f64, start: f64, end: f64) -> bool {
    value >= start && value < end
}

fn deduplicated_prs(values: &[DriftPullRequest]) -> Vec<DriftPullRequest> {
    let mut records = BTreeMap::new();
    for pr in values {
        records
            .entry((pr.repository.as_str(), pr.number))
            .and_modify(|existing: &mut DriftPullRequest| {
                if existing.merged_at.is_none() {
                    existing.merged_at.clone_from(&pr.merged_at);
                }
                if let Some(updated_at) = pr.updated_at.as_deref() {
                    let replace = existing.updated_at.as_deref().is_none_or(|current| {
                        timestamp_epoch(updated_at).ok() > timestamp_epoch(current).ok()
                    });
                    if replace {
                        existing.updated_at = Some(updated_at.to_owned());
                    }
                }
                let mut event_ids = existing.event_ids.iter().cloned().collect::<BTreeSet<_>>();
                event_ids.extend(pr.event_ids.iter().cloned());
                existing.event_ids = event_ids.into_iter().collect();
            })
            .or_insert_with(|| pr.clone());
    }
    records.into_values().collect()
}

fn deduplicated_commits(values: &[DriftCommit]) -> Vec<DriftCommit> {
    let mut records = BTreeMap::new();
    for commit in values {
        records
            .entry(commit.sha.as_str())
            .or_insert_with(|| commit.clone());
    }
    records.into_values().collect()
}

fn deduplicated_transitions(values: &[IssueTransition]) -> Vec<IssueTransition> {
    let mut records = BTreeMap::new();
    for transition in values {
        records
            .entry((transition.issue_id.as_str(), transition.event_id.as_str()))
            .or_insert_with(|| transition.clone());
    }
    records.into_values().collect()
}

fn collect_unassigned_prs(
    pull_requests: &[DriftPullRequest],
    window_start: f64,
    report_time: f64,
    tickets_by_identifier: &HashMap<&str, &DriftTicket>,
    leads_by_team: &HashMap<&str, &DriftLead>,
) -> Result<Vec<UnassignedPullRequest>, DriftError> {
    let mut output = Vec::new();
    for pr in pull_requests {
        let created = timestamp_epoch(&pr.created_at)?;
        let merged_in_window = pr
            .merged_at
            .as_deref()
            .map(timestamp_epoch)
            .transpose()?
            .is_some_and(|merged| in_window(merged, window_start, report_time));
        if !in_window(created, window_start, report_time) && !merged_in_window {
            continue;
        }
        let ticket_ids = extract_ticket_ids(&pr.branch, &pr.title);
        let reason = match ticket_ids.as_slice() {
            [] => Some(UnassignedReason::MissingTicketId),
            [_first, _second, ..] => Some(UnassignedReason::ConflictingTicketIds),
            [ticket_id] => match tickets_by_identifier.get(ticket_id.as_str()) {
                None => Some(UnassignedReason::UnresolvedTicketId),
                Some(ticket) if !leads_by_team.contains_key(ticket.team_key.as_str()) => {
                    Some(UnassignedReason::UnconfiguredTeam)
                }
                Some(_) => None,
            },
        };
        if let Some(reason) = reason {
            output.push(UnassignedPullRequest {
                repository: pr.repository.clone(),
                number: pr.number,
                url: pr.url.clone(),
                title: pr.title.clone(),
                branch: pr.branch.clone(),
                created_at: canonical_timestamp(&pr.created_at)?,
                merged_at: pr
                    .merged_at
                    .as_deref()
                    .map(canonical_timestamp)
                    .transpose()?,
                ticket_ids,
                reason,
            });
        }
    }
    Ok(output)
}

fn extract_ticket_ids(branch: &str, title: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    for source in [branch, title] {
        let bytes = source.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            let prefix_start = index;
            while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
                index += 1;
            }
            let prefix_end = index;
            if prefix_end > prefix_start
                && index < bytes.len()
                && bytes[index] == b'-'
                && index + 1 < bytes.len()
                && bytes[index + 1].is_ascii_digit()
                && bytes[prefix_start].is_ascii_alphabetic()
                && bytes[prefix_start..prefix_end]
                    .iter()
                    .any(u8::is_ascii_alphabetic)
                && (prefix_start == 0 || !bytes[prefix_start - 1].is_ascii_alphanumeric())
            {
                index += 1;
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
                if (index == bytes.len() || !bytes[index].is_ascii_alphanumeric())
                    && let Some(value) = source.get(prefix_start..index)
                {
                    found.insert(value.to_ascii_uppercase());
                }
            } else {
                index = prefix_start + 1;
            }
        }
    }
    found.into_iter().collect()
}

fn reconstruct_veering(
    queue: &QueueEvidence,
    team_key: &str,
    moved_ticket: &str,
) -> Result<Option<(VeeringReason, Option<String>)>, String> {
    if let Some(reason) = queue.missing_inputs.iter().min() {
        return Err(reason.clone());
    }
    let target = queue
        .tickets
        .iter()
        .find(|ticket| ticket.identifier == moved_ticket)
        .ok_or_else(|| format!("historical ticket {moved_ticket} is missing"))?;
    let Some(target_project_id) = target.project_id.as_deref() else {
        return Ok(Some((VeeringReason::OutsideRunningProject, None)));
    };
    let project_by_id: HashMap<_, _> = queue
        .projects
        .iter()
        .map(|project| (project.id.as_str(), project))
        .collect();
    let target_project = project_by_id
        .get(target_project_id)
        .ok_or_else(|| format!("historical project {target_project_id} is missing"))?;
    let is_running = target_project.status == "Started"
        && target_project.team_keys.iter().any(|team| team == team_key);
    if !is_running {
        return Ok(Some((VeeringReason::OutsideRunningProject, None)));
    }
    let ticket_by_id: HashMap<_, _> = queue
        .tickets
        .iter()
        .map(|ticket| (ticket.id.as_str(), ticket))
        .collect();
    let mut ready = Vec::new();
    for ticket in queue
        .tickets
        .iter()
        .filter(|ticket| ticket.team_key == team_key && ticket_is_ready(ticket, &project_by_id))
    {
        if !historical_blockers_completed(ticket, &ticket_by_id)? {
            continue;
        }
        let project_id = ticket.project_id.as_deref().ok_or_else(|| {
            format!(
                "historical project association for {} is missing",
                ticket.identifier
            )
        })?;
        let project = project_by_id
            .get(project_id)
            .ok_or_else(|| format!("historical project {project_id} is missing"))?;
        ready.push((*project, ticket));
    }
    ready.sort_by(
        |(left_project, left_ticket), (right_project, right_ticket)| {
            project_priority(left_project.priority)
                .cmp(&project_priority(right_project.priority))
                .then_with(|| left_project.sort_order.total_cmp(&right_project.sort_order))
                .then_with(|| {
                    issue_priority(left_ticket.priority)
                        .0
                        .cmp(&issue_priority(right_ticket.priority).0)
                        .then_with(|| left_ticket.priority.total_cmp(&right_ticket.priority))
                })
                .then_with(|| left_ticket.sort_order.total_cmp(&right_ticket.sort_order))
                .then_with(|| left_project.id.cmp(&right_project.id))
                .then_with(|| left_ticket.identifier.cmp(&right_ticket.identifier))
                .then_with(|| left_ticket.id.cmp(&right_ticket.id))
        },
    );
    let moved_position = ready
        .iter()
        .position(|(_, ticket)| ticket.identifier == moved_ticket);
    if let Some(position) = moved_position
        && let Some((_, skipped)) = ready.get(..position).and_then(|before| before.first())
    {
        return Ok(Some((
            VeeringReason::PriorityInversion,
            Some(skipped.identifier.clone()),
        )));
    }
    Ok(None)
}

fn ticket_is_ready(
    ticket: &HistoricalTicket,
    projects: &HashMap<&str, &HistoricalProject>,
) -> bool {
    !matches!(
        ticket.state.as_str(),
        "Completed" | "Canceled" | "Coding" | "Review"
    ) && matches!(
        ticket.spec_state.as_str(),
        "Specced" | "Specify" | "Backlog"
    ) && ticket
        .project_id
        .as_deref()
        .and_then(|id| projects.get(id))
        .is_some_and(|project| project.status == "Started")
}

fn historical_blockers_completed(
    ticket: &HistoricalTicket,
    tickets: &HashMap<&str, &HistoricalTicket>,
) -> Result<bool, String> {
    let mut pending = ticket.blocked_by.clone();
    let mut visited = HashSet::new();
    while let Some(blocker_id) = pending.pop() {
        if !visited.insert(blocker_id.clone()) {
            continue;
        }
        let Some(blocker) = tickets.get(blocker_id.as_str()) else {
            return Err(format!(
                "historical blocker {blocker_id} for {} is missing",
                ticket.identifier
            ));
        };
        if blocker.state != "Completed" {
            return Ok(false);
        }
        pending.extend(blocker.blocked_by.iter().cloned());
    }
    Ok(true)
}

fn issue_priority(priority: f64) -> (bool, f64) {
    (priority == 0.0, priority)
}

fn last_activity_for(
    ticket: &DriftTicket,
    pull_requests: &[DriftPullRequest],
    commits: &[DriftCommit],
    report_epoch: f64,
) -> Result<Option<String>, DriftError> {
    let mut activity = Vec::new();
    for pr in pull_requests {
        let ticket_ids = extract_ticket_ids(&pr.branch, &pr.title);
        if ticket_ids.len() == 1 && ticket_ids[0].eq_ignore_ascii_case(&ticket.identifier) {
            if timestamp_epoch(&pr.created_at)? < report_epoch {
                activity.push(pr.created_at.clone());
            }
            if let Some(merged) = &pr.merged_at
                && timestamp_epoch(merged)? < report_epoch
            {
                activity.push(merged.clone());
            }
        }
    }
    for commit in commits {
        if commit
            .ticket_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(&ticket.identifier))
            && timestamp_epoch(&commit.committed_at)? < report_epoch
        {
            activity.push(commit.committed_at.clone());
        }
    }
    activity
        .into_iter()
        .map(|at| timestamp_epoch(&at).map(|epoch| (epoch, at)))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, at)| at)
        .map(|at| canonical_timestamp(&at))
        .transpose()
}

fn make_churn_signal(
    _config: &DriftConfig,
    evidence: &DriftEvidence,
    team_key: &str,
    project_ids: &HashSet<&str>,
    window_start: f64,
    report_time: f64,
    tickets_by_id: &HashMap<&str, &DriftTicket>,
    leads_by_team: &HashMap<&str, &DriftLead>,
) -> Result<TicketChurnSignal, DriftError> {
    let missing = missing_for(
        evidence,
        &[
            "linear.issues",
            "linear.projects",
            "linear.history",
            "github.prs",
        ],
    );
    let mut created_ids = HashSet::new();
    for ticket in evidence.tickets.iter().filter(|ticket| {
        ticket.team_key == team_key
            && ticket
                .project_id
                .as_deref()
                .is_some_and(|id| project_ids.contains(id))
    }) {
        if in_window(
            timestamp_epoch(&ticket.created_at)?,
            window_start,
            report_time,
        ) {
            created_ids.insert(ticket.id.as_str());
        }
    }
    let mut completed_event_ids = HashSet::new();
    for transition in evidence
        .transitions
        .iter()
        .filter(|transition| transition.to_state == "Completed")
    {
        let occurred = timestamp_epoch(&transition.occurred_at)?;
        let Some(ticket) = tickets_by_id.get(transition.issue_id.as_str()) else {
            continue;
        };
        if in_window(occurred, window_start, report_time)
            && ticket.team_key == team_key
            && ticket
                .project_id
                .as_deref()
                .is_some_and(|id| project_ids.contains(id))
        {
            completed_event_ids.insert(transition.event_id.as_str());
        }
    }
    let pulls = deduplicated_prs(&evidence.pull_requests);
    let mut merged_pr_ids = HashSet::new();
    for pr in &pulls {
        let Some(merged_at) = pr.merged_at.as_deref() else {
            continue;
        };
        if !in_window(timestamp_epoch(merged_at)?, window_start, report_time) {
            continue;
        }
        let ticket_ids = extract_ticket_ids(&pr.branch, &pr.title);
        if ticket_ids.len() != 1 {
            continue;
        }
        let Some(ticket) = tickets_by_id
            .values()
            .find(|ticket| ticket.identifier == ticket_ids[0])
        else {
            continue;
        };
        if leads_by_team.contains_key(ticket.team_key.as_str())
            && ticket.team_key == team_key
            && ticket
                .project_id
                .as_deref()
                .is_some_and(|id| project_ids.contains(id))
        {
            merged_pr_ids.insert((pr.repository.as_str(), pr.number));
        }
    }
    let counts_available = missing.is_empty();
    let created = counts_available.then_some(created_ids.len() as u64);
    let completed = counts_available.then_some(completed_event_ids.len() as u64);
    let merged_prs = counts_available.then_some(merged_pr_ids.len() as u64);
    let net_growth = match (created, completed) {
        (Some(created), Some(completed)) => i64::try_from(created)
            .ok()
            .zip(i64::try_from(completed).ok())
            .and_then(|(created, completed)| created.checked_sub(completed)),
        _ => None,
    };
    Ok(TicketChurnSignal {
        common: availability(missing),
        created,
        completed,
        merged_prs,
        net_growth,
    })
}

fn derive_alerts(
    signals: &LeadSignals,
    config: &DriftConfig,
) -> (Vec<DriftAlert>, LeadStatus, Vec<String>) {
    let mut alerts = Vec::new();
    let mut missing_reasons = BTreeSet::new();
    let mut out_of_scope_reasons = BTreeSet::new();
    for pr in &signals.pr_scope.prs {
        if pr.classification == ScopeClassification::OutOfScope {
            out_of_scope_reasons.insert(format!(
                "out_of_scope PR {}#{} references {} outside running scope",
                pr.repository, pr.number, pr.ticket_id
            ));
        }
    }
    for ticket in &signals.veering.tickets {
        let reason = match ticket.reason {
            VeeringReason::OutsideRunningProject => format!(
                "ticket {} entered Coding outside a running project",
                ticket.ticket_id
            ),
            VeeringReason::PriorityInversion => format!(
                "ticket {} entered Coding ahead of ready ticket {}",
                ticket.ticket_id,
                ticket.skipped_ticket_id.as_deref().unwrap_or("(unknown)")
            ),
        };
        out_of_scope_reasons.insert(reason);
    }
    let mut churn_reasons = BTreeSet::new();
    for ticket in &signals.stall.tickets {
        churn_reasons.insert(format!(
            "ticket {} has been in Coding {:.2} hours without recent PR or commit activity",
            ticket.ticket_id, ticket.elapsed_hours
        ));
    }
    if signals.busy_without_pr.finding == Some(true) {
        let mut sources = Vec::new();
        if signals.busy_without_pr.herdr_busy == Some(true) {
            sources.push("herdr busy".to_owned());
        }
        if signals
            .busy_without_pr
            .otel_spend
            .is_some_and(|spend| spend > 0.0 && spend >= config.spend_threshold)
        {
            sources.push(format!(
                "OTel spend {:.2} at threshold {:.2}",
                signals.busy_without_pr.otel_spend.unwrap_or_default(),
                config.spend_threshold
            ));
        }
        churn_reasons.insert(format!(
            "{} with no attributed new PR",
            sources.join(" and ")
        ));
    }
    if let (Some(net_growth), Some(merged_prs)) = (
        signals.ticket_churn.net_growth,
        signals.ticket_churn.merged_prs,
    ) && net_growth > config.max_net_ticket_growth
        && merged_prs < config.min_merged_prs_for_churn
    {
        churn_reasons.insert(format!(
            "net ticket growth {net_growth} exceeds {} with {merged_prs} merged PRs below {}",
            config.max_net_ticket_growth, config.min_merged_prs_for_churn
        ));
    }
    if !out_of_scope_reasons.is_empty() {
        alerts.push(DriftAlert {
            kind: AlertKind::OutOfScope,
            reasons: out_of_scope_reasons.into_iter().collect(),
        });
    }
    if !churn_reasons.is_empty() {
        alerts.push(DriftAlert {
            kind: AlertKind::Churning,
            reasons: churn_reasons.into_iter().collect(),
        });
    }
    if !alerts.is_empty() {
        let mut reasons = alerts
            .iter()
            .flat_map(|alert| alert.reasons.iter().cloned())
            .collect::<Vec<_>>();
        reasons.sort();
        return (alerts, LeadStatus::Flagged, reasons);
    }
    if signals.pr_scope.common.availability == Availability::Unavailable {
        missing_reasons.extend(signals.pr_scope.common.missing_sources.iter().cloned());
    }
    if signals.veering.common.availability == Availability::Unavailable {
        missing_reasons.extend(signals.veering.common.missing_sources.iter().cloned());
    }
    if signals.stall.common.availability == Availability::Unavailable {
        missing_reasons.extend(signals.stall.common.missing_sources.iter().cloned());
    }
    if signals.busy_without_pr.common.availability == Availability::Unavailable {
        missing_reasons.extend(
            signals
                .busy_without_pr
                .common
                .missing_sources
                .iter()
                .cloned(),
        );
    }
    if signals.ticket_churn.common.availability == Availability::Unavailable {
        missing_reasons.extend(signals.ticket_churn.common.missing_sources.iter().cloned());
    }
    if missing_reasons.is_empty() {
        (alerts, LeadStatus::OnTrack, Vec::new())
    } else {
        (
            alerts,
            LeadStatus::Unknown,
            missing_reasons.into_iter().collect(),
        )
    }
}

/// Renders concise lead and configured priority-goal lines in report order.
pub fn render_drift_text(report: &DriftReport) -> String {
    let mut lines: Vec<_> = report
        .leads
        .iter()
        .map(|lead| {
            let status = match lead.status {
                LeadStatus::OnTrack => "on_track",
                LeadStatus::Flagged => "flagged",
                LeadStatus::Unknown => "unknown",
            };
            let alerts = lead
                .alerts
                .iter()
                .map(|alert| match alert.kind {
                    AlertKind::OutOfScope => "out_of_scope",
                    AlertKind::Churning => "churning",
                })
                .collect::<Vec<_>>()
                .join(",");
            let mut reasons = lead.reasons.clone();
            if !report.unassigned_prs.is_empty() {
                reasons.push(format!(
                    "{} unassigned PRs: {}",
                    report.unassigned_prs.len(),
                    report
                        .unassigned_prs
                        .iter()
                        .map(|pr| format!(
                            "{}#{} ({})",
                            pr.repository,
                            pr.number,
                            unassigned_reason_name(pr.reason)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            format!(
                "{} {} alerts=[{}] reasons=[{}]",
                lead.lead_id,
                status,
                alerts,
                reasons.join("; ")
            )
        })
        .collect();
    lines.extend(report.priority_goals.iter().map(|goal| {
        let mut crowding = goal
            .crowding_out
            .iter()
            .map(|project| {
                format!(
                    "{}/{}: {}",
                    project.team_key,
                    project.project,
                    project.tickets.join(",")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        if crowding.is_empty() {
            crowding = "none observed".to_owned();
        }
        let mut out_of_team = goal
            .out_of_team_activity
            .iter()
            .map(|project| {
                format!(
                    "{}/{}: {}",
                    project.team_key,
                    project.project,
                    project.tickets.join(",")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        if out_of_team.is_empty() {
            out_of_team = "none observed".to_owned();
        }
        format!(
            "priority_goal {} status={} flagged={} trusted={} hours_since_progress={} no_progress_hours={} active_workers={}/{} last_completion_at={} last_transition_at={} blocked_on=[{}] crowding_out=[{}] out_of_team_activity=[{}] missing=[{}]",
            goal.goal,
            match goal.status {
                PriorityGoalStatus::Unknown => "unknown",
                PriorityGoalStatus::Blocked => "blocked",
                PriorityGoalStatus::Flagged => "flagged",
                PriorityGoalStatus::OnTrack => "on_track",
            },
            goal.flagged.map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
            goal.trusted,
            goal.hours_since_progress
                .map(|hours| format!("{hours:.1}"))
                .unwrap_or_else(|| "unknown".to_owned()),
            goal.no_progress_hours,
            goal.active_workers
                .map_or_else(|| "unknown".to_owned(), |workers| workers.to_string()),
            goal.minimum_workers,
            goal.last_completion_at.as_deref().unwrap_or("unknown"),
            goal.last_transition_at.as_deref().unwrap_or("unknown"),
            if goal.blocked_on.is_empty() { "none".to_owned() } else { goal.blocked_on.join(",") },
            crowding,
            out_of_team,
            goal.missing_sources.join(","),
        )
    }));
    lines.join("\n")
}

fn unassigned_reason_name(reason: UnassignedReason) -> &'static str {
    match reason {
        UnassignedReason::MissingTicketId => "missing_ticket_id",
        UnassignedReason::ConflictingTicketIds => "conflicting_ticket_ids",
        UnassignedReason::UnresolvedTicketId => "unresolved_ticket_id",
        UnassignedReason::UnconfiguredTeam => "unconfigured_team",
    }
}

pub(crate) fn timestamp_epoch(value: &str) -> Result<f64, DriftError> {
    let invalid = || DriftError::InvalidTimestamp {
        value: value.to_owned(),
    };
    if !value.is_ascii() {
        return Err(invalid());
    }
    if value.len() < 20
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
        || value.as_bytes().get(10) != Some(&b'T')
        || value.as_bytes().get(13) != Some(&b':')
        || value.as_bytes().get(16) != Some(&b':')
    {
        return Err(invalid());
    }
    let year = value[0..4].parse::<i64>().map_err(|_| invalid())?;
    let month = value[5..7].parse::<u32>().map_err(|_| invalid())?;
    let day = value[8..10].parse::<u32>().map_err(|_| invalid())?;
    let hour = value[11..13].parse::<u32>().map_err(|_| invalid())?;
    let minute = value[14..16].parse::<u32>().map_err(|_| invalid())?;
    let second = value[17..19].parse::<u32>().map_err(|_| invalid())?;
    if year < 1 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month {
        return Err(invalid());
    }
    let bytes = value.as_bytes();
    let mut position = 19;
    let mut fraction = 0.0;
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let fraction_start = position;
        let mut place = 0.1;
        while position < bytes.len() && bytes[position].is_ascii_digit() {
            fraction += f64::from(bytes[position] - b'0') * place;
            place *= 0.1;
            position += 1;
        }
        if position == fraction_start {
            return Err(invalid());
        }
    }
    let offset_seconds: i64 = match bytes.get(position) {
        Some(b'Z') if position + 1 == bytes.len() => 0,
        Some(b'+' | b'-')
            if position + 6 == bytes.len() && bytes.get(position + 3) == Some(&b':') =>
        {
            let offset_hour = value[position + 1..position + 3]
                .parse::<i64>()
                .map_err(|_| invalid())?;
            let offset_minute = value[position + 4..position + 6]
                .parse::<i64>()
                .map_err(|_| invalid())?;
            if offset_hour > 23 || offset_minute > 59 {
                return Err(invalid());
            }
            let sign = if bytes[position] == b'+' { 1 } else { -1 };
            sign * (offset_hour * 3_600 + offset_minute * 60)
        }
        _ => return Err(invalid()),
    };
    let days = days_from_civil(year, month, day);
    Ok(
        (days as f64 * 86_400.0) + f64::from(hour * 3_600 + minute * 60 + second)
            - offset_seconds as f64
            + fraction,
    )
}

fn canonical_timestamp(value: &str) -> Result<String, DriftError> {
    timestamp_epoch(value).map(format_timestamp)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn format_timestamp(epoch: f64) -> String {
    let whole_seconds = epoch.floor() as i64;
    let days = whole_seconds.div_euclid(86_400);
    let day_seconds = whole_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    let nanos = ((epoch - whole_seconds as f64) * 1_000_000_000.0).floor() as u32;
    if nanos == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        let fraction = format!("{nanos:09}");
        format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{}Z",
            fraction.trim_end_matches('0')
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "2026-09-27T12:00:00Z";
    fn config() -> DriftConfig {
        DriftConfig {
            report_time: REPORT.to_owned(),
            window_hours: 168.0,
            stall_hours: 24.0,
            spend_threshold: 0.0,
            max_net_ticket_growth: 0,
            min_merged_prs_for_churn: 1,
            priority_goals: Vec::new(),
            leads: vec![DriftLead {
                lead_id: "lead-a".to_owned(),
                team_key: "SWM".to_owned(),
            }],
        }
    }

    fn project(
        id: &str,
        identifier: &str,
        status: &str,
        priority: i32,
        order: f64,
    ) -> HistoricalProject {
        HistoricalProject {
            id: id.to_owned(),
            identifier: Some(identifier.to_owned()),
            team_keys: vec!["SWM".to_owned()],
            status: status.to_owned(),
            priority,
            sort_order: order,
        }
    }

    fn ticket(
        id: &str,
        identifier: &str,
        project_id: &str,
        state: &str,
        priority: f64,
    ) -> DriftTicket {
        DriftTicket {
            id: id.to_owned(),
            identifier: identifier.to_owned(),
            team_key: "SWM".to_owned(),
            assignee_id: None,
            project_id: Some(project_id.to_owned()),
            project_identifier: Some("SWM-P1".to_owned()),
            milestone_identifier: None,
            linked_pr_urls: Vec::new(),
            state: state.to_owned(),
            state_type: if matches!(state, "Coding" | "In Progress" | "Review") {
                WorkflowStateType::Started
            } else if matches!(state, "Done" | "Completed") {
                WorkflowStateType::Completed
            } else {
                WorkflowStateType::Unstarted
            },
            spec_state: "Specced".to_owned(),
            priority,
            sort_order: priority,
            blocked_by: Vec::new(),
            created_at: "2026-09-20T13:00:00Z".to_owned(),
            updated_at: None,
            state_entered_at: (state == "Coding").then(|| "2026-09-20T13:00:00Z".to_owned()),
        }
    }

    fn evidence() -> DriftEvidence {
        DriftEvidence {
            projects: vec![
                project("p1", "SWM-P1", "Started", 1, 1.0),
                project("p2", "SWM-P2", "Paused", 2, 2.0),
            ],
            tickets: vec![
                ticket("i1", "SWM-1", "p1", "Todo", 1.0),
                ticket("i2", "SWM-2", "p2", "Todo", 2.0),
            ],
            transitions: Vec::new(),
            pull_requests: Vec::new(),
            commits: Vec::new(),
            herdr_busy: BTreeMap::from([("lead-a".to_owned(), false)]),
            otel_spend: BTreeMap::from([("lead-a".to_owned(), 0.0)]),
            missing: Vec::new(),
        }
    }

    fn pr(
        number: u64,
        branch: &str,
        title: &str,
        created_at: &str,
        merged_at: Option<&str>,
    ) -> DriftPullRequest {
        DriftPullRequest {
            repository: "agent-ix/example".to_owned(),
            number,
            url: format!("https://github.com/agent-ix/example/pull/{number}"),
            title: title.to_owned(),
            branch: branch.to_owned(),
            created_at: created_at.to_owned(),
            merged_at: merged_at.map(str::to_owned),
            updated_at: None,
            event_ids: vec![format!("pr-{number}")],
        }
    }

    #[test]
    fn scope_unassigned_and_pre_window_merge_are_attributed_by_ticket_only() {
        let mut evidence = evidence();
        evidence.pull_requests = vec![
            pr(1, "feature/SWM-2", "SWM-2", "2026-09-22T00:00:00Z", None),
            pr(
                2,
                "feature/no-id",
                "Docs update",
                "2026-09-22T00:00:00Z",
                None,
            ),
            pr(
                3,
                "feature/SWM-999",
                "SWM-999",
                "2026-09-22T00:00:00Z",
                None,
            ),
            pr(
                4,
                "feature/SWM-1",
                "SWM-1",
                "2026-09-10T00:00:00Z",
                Some("2026-09-25T00:00:00Z"),
            ),
        ];
        let report = evaluate_drift(&config(), &evidence).expect("valid evidence");
        assert_eq!(report.leads[0].signals.pr_scope.prs.len(), 1);
        let out_of_scope = &report.leads[0].signals.pr_scope.prs[0];
        assert_eq!(out_of_scope.ticket_id, "SWM-2");
        assert_eq!(out_of_scope.classification, ScopeClassification::OutOfScope);
        assert_eq!(report.leads[0].signals.ticket_churn.merged_prs, Some(1));
        assert_eq!(report.unassigned_prs.len(), 2);
        assert_eq!(
            report.unassigned_prs[0].reason,
            UnassignedReason::MissingTicketId
        );
        assert_eq!(
            report.unassigned_prs[1].reason,
            UnassignedReason::UnresolvedTicketId
        );
        assert_eq!(report.leads[0].status, LeadStatus::Flagged);
    }

    #[test]
    fn veering_uses_pre_transition_queue_and_reports_highest_skipped_ticket() {
        let mut evidence = evidence();
        evidence.tickets[0].state = "Coding".to_owned();
        evidence.tickets[0].state_entered_at = Some("2026-09-26T00:00:00Z".to_owned());
        evidence.transitions.push(IssueTransition {
            event_id: "transition-2".to_owned(),
            issue_id: "i2".to_owned(),
            ticket_id: "SWM-2".to_owned(),
            to_state: "Coding".to_owned(),
            occurred_at: "2026-09-26T00:00:00Z".to_owned(),
            queue_before: Some(QueueEvidence {
                projects: vec![project("p1", "SWM-P1", "Started", 1, 1.0)],
                tickets: vec![
                    HistoricalTicket {
                        id: "i1".to_owned(),
                        identifier: "SWM-1".to_owned(),
                        team_key: "SWM".to_owned(),
                        project_id: Some("p1".to_owned()),
                        state: "Todo".to_owned(),
                        spec_state: "Specced".to_owned(),
                        priority: 1.0,
                        sort_order: 1.0,
                        blocked_by: Vec::new(),
                    },
                    HistoricalTicket {
                        id: "i2".to_owned(),
                        identifier: "SWM-2".to_owned(),
                        team_key: "SWM".to_owned(),
                        project_id: Some("p1".to_owned()),
                        state: "Todo".to_owned(),
                        spec_state: "Specced".to_owned(),
                        priority: 2.0,
                        sort_order: 2.0,
                        blocked_by: Vec::new(),
                    },
                ],
                missing_inputs: Vec::new(),
            }),
        });
        let report = evaluate_drift(&config(), &evidence).expect("valid evidence");
        let veering = &report.leads[0].signals.veering;
        assert_eq!(veering.common.availability, Availability::Available);
        assert_eq!(veering.tickets.len(), 1);
        assert_eq!(veering.tickets[0].reason, VeeringReason::PriorityInversion);
        assert_eq!(
            veering.tickets[0].skipped_ticket_id.as_deref(),
            Some("SWM-1")
        );
        assert_eq!(report.leads[0].alerts[0].kind, AlertKind::OutOfScope);
    }

    #[test]
    fn historical_queue_excludes_closed_tickets_and_treats_no_project_as_outside_scope() {
        let mut evidence = evidence();
        evidence.transitions.push(IssueTransition {
            event_id: "coding-move".to_owned(),
            issue_id: "i2".to_owned(),
            ticket_id: "SWM-2".to_owned(),
            to_state: "Coding".to_owned(),
            occurred_at: "2026-09-26T00:00:00Z".to_owned(),
            queue_before: Some(QueueEvidence {
                projects: vec![project("p1", "SWM-P1", "Started", 1, 1.0)],
                tickets: vec![
                    HistoricalTicket {
                        id: "i1".to_owned(),
                        identifier: "SWM-1".to_owned(),
                        team_key: "SWM".to_owned(),
                        project_id: Some("p1".to_owned()),
                        state: "Completed".to_owned(),
                        spec_state: "Specced".to_owned(),
                        priority: 1.0,
                        sort_order: 1.0,
                        blocked_by: Vec::new(),
                    },
                    HistoricalTicket {
                        id: "i2".to_owned(),
                        identifier: "SWM-2".to_owned(),
                        team_key: "SWM".to_owned(),
                        project_id: Some("p1".to_owned()),
                        state: "Todo".to_owned(),
                        spec_state: "Specced".to_owned(),
                        priority: 2.0,
                        sort_order: 2.0,
                        blocked_by: Vec::new(),
                    },
                ],
                missing_inputs: Vec::new(),
            }),
        });
        let completed_not_ready =
            evaluate_drift(&config(), &evidence).expect("closed is not ready");
        assert!(
            completed_not_ready.leads[0]
                .signals
                .veering
                .tickets
                .is_empty()
        );

        evidence.transitions[0]
            .queue_before
            .as_mut()
            .expect("history fixture")
            .tickets[0]
            .state = "Canceled".to_owned();
        let canceled_not_ready =
            evaluate_drift(&config(), &evidence).expect("canceled is not ready");
        assert!(
            canceled_not_ready.leads[0]
                .signals
                .veering
                .tickets
                .is_empty()
        );

        evidence.transitions[0]
            .queue_before
            .as_mut()
            .expect("history fixture")
            .tickets[1]
            .project_id = None;
        let no_project =
            evaluate_drift(&config(), &evidence).expect("no project is known outside scope");
        let finding = &no_project.leads[0].signals.veering.tickets[0];
        assert_eq!(finding.reason, VeeringReason::OutsideRunningProject);
    }

    #[test]
    fn zero_threshold_requires_positive_spend_and_activity_observations_are_nullable() {
        let mut evidence = evidence();
        let mut no_growth_alert = config();
        no_growth_alert.max_net_ticket_growth = 10;
        evidence.otel_spend.insert("lead-a".to_owned(), 0.0);
        let idle = evaluate_drift(&no_growth_alert, &evidence).expect("valid evidence");
        assert_eq!(idle.leads[0].signals.busy_without_pr.finding, Some(false));
        evidence.otel_spend.insert("lead-a".to_owned(), 0.01);
        let spending = evaluate_drift(&no_growth_alert, &evidence).expect("valid evidence");
        assert_eq!(
            spending.leads[0].signals.busy_without_pr.finding,
            Some(true)
        );
        assert_eq!(spending.leads[0].alerts[0].kind, AlertKind::Churning);
        evidence.herdr_busy.clear();
        evidence.otel_spend.clear();
        let missing = evaluate_drift(&no_growth_alert, &evidence)
            .expect("partial evidence remains reportable");
        assert_eq!(missing.leads[0].signals.busy_without_pr.finding, None);
        assert_eq!(missing.leads[0].signals.busy_without_pr.otel_spend, None);
        assert_eq!(missing.leads[0].status, LeadStatus::Unknown);
    }

    #[test]
    fn incomplete_pages_preserve_proven_alerts_but_prevent_on_track() {
        let mut evidence = evidence();
        evidence.missing.push(MissingEvidence {
            source: "github.prs".to_owned(),
            reason: "page 3 cursor unresolved".to_owned(),
        });
        let report =
            evaluate_drift(&config(), &evidence).expect("partial evidence remains reportable");
        assert_eq!(
            report.leads[0].signals.pr_scope.common.availability,
            Availability::Unavailable
        );
        assert_eq!(report.leads[0].signals.ticket_churn.created, None);
        assert_eq!(report.leads[0].signals.ticket_churn.merged_prs, None);
        assert_eq!(report.leads[0].status, LeadStatus::Unknown);
        let mut alerted = evidence;
        alerted.pull_requests.push(pr(
            9,
            "feature/SWM-2",
            "SWM-2",
            "2026-09-22T00:00:00Z",
            None,
        ));
        let report =
            evaluate_drift(&config(), &alerted).expect("partial evidence remains reportable");
        assert_eq!(report.leads[0].status, LeadStatus::Flagged);
    }

    #[test]
    fn coding_stall_requires_no_attributed_activity_in_trailing_interval() {
        let mut evidence = evidence();
        evidence.tickets[0].state = "Coding".to_owned();
        evidence.tickets[0].state_entered_at = Some("2026-09-25T00:00:00Z".to_owned());
        let mut no_growth_alert = config();
        no_growth_alert.max_net_ticket_growth = 10;
        let stalled = evaluate_drift(&no_growth_alert, &evidence).expect("valid evidence");
        assert_eq!(stalled.leads[0].signals.stall.tickets.len(), 1);
        assert_eq!(stalled.leads[0].signals.stall.tickets[0].ticket_id, "SWM-1");
        assert_eq!(stalled.leads[0].alerts[0].kind, AlertKind::Churning);

        evidence.commits.push(DriftCommit {
            sha: "abc123".to_owned(),
            repository: "agent-ix/example".to_owned(),
            committed_at: "2026-09-27T11:00:00Z".to_owned(),
            ticket_ids: vec!["SWM-1".to_owned()],
        });
        let active = evaluate_drift(&no_growth_alert, &evidence).expect("valid activity evidence");
        assert!(active.leads[0].signals.stall.tickets.is_empty());
    }

    #[test]
    fn priority_goal_flags_qsl_l2_stalled_while_other_projects_advance() {
        let mut config = config();
        config.report_time = "2026-10-05T18:00:00Z".to_owned();
        config.priority_goals.push(PriorityGoalConfig {
            goal: "QSL-L2".to_owned(),
            ticket_ids: vec![
                "QSL-485".to_owned(),
                "QSL-486".to_owned(),
                "QSL-487".to_owned(),
            ],
            minimum_workers: 1,
            no_progress_hours: 12.0,
        });
        let mut evidence = evidence();
        let target_project_id = "qsl-l2";
        for (id, identifier) in [
            ("qsl485", "QSL-485"),
            ("qsl486", "QSL-486"),
            ("qsl487", "QSL-487"),
        ] {
            let state = match identifier {
                "QSL-485" => "In Progress",
                "QSL-486" => "Coding",
                _ => "Todo",
            };
            let mut item = ticket(id, identifier, target_project_id, state, 1.0);
            item.project_identifier = Some("QSL-L2".to_owned());
            item.assignee_id = Some(if identifier == "QSL-487" {
                "inactive-worker".to_owned()
            } else {
                "goal-worker".to_owned()
            });
            item.state_entered_at = Some("2026-10-04T04:00:00Z".to_owned());
            if identifier == "QSL-487" {
                item.blocked_by.push("age900".to_owned());
            }
            if identifier == "QSL-485" {
                item.linked_pr_urls
                    .push("https://github.com/agent-ix/example/pull/641".to_owned());
            }
            evidence.tickets.push(item);
        }
        let mut unassigned_active =
            ticket("qsl488", "QSL-488", target_project_id, "In Progress", 1.0);
        unassigned_active.project_identifier = Some("QSL-L2".to_owned());
        unassigned_active.assignee_id = Some("   ".to_owned());
        evidence.tickets.push(unassigned_active);
        for (id, identifier, project, state, at) in [
            (
                "qsl641",
                "QSL-641",
                "IR-path",
                "In Progress",
                "2026-10-05T07:00:00Z",
            ),
            (
                "qsl635",
                "QSL-635",
                "IR-path",
                "Coding",
                "2026-10-05T09:00:00Z",
            ),
            (
                "ir626",
                "IR-626",
                "IR-path",
                "In Progress",
                "2026-10-05T11:00:00Z",
            ),
        ] {
            let mut item = ticket(id, identifier, "other", state, 1.0);
            item.project_identifier = Some(project.to_owned());
            if identifier == "QSL-641" || identifier == "IR-626" {
                item.linked_pr_urls.push(format!(
                    "https://github.com/agent-ix/example/pull/{}",
                    if identifier == "QSL-641" { 700 } else { 701 }
                ));
            }
            evidence.tickets.push(item);
            evidence.transitions.push(IssueTransition {
                event_id: format!("transition-{id}"),
                issue_id: id.to_owned(),
                ticket_id: identifier.to_owned(),
                to_state: state.to_owned(),
                occurred_at: at.to_owned(),
                queue_before: None,
            });
        }
        let mut blocker = ticket("age900", "AGE-900", "age-project", "In Progress", 1.0);
        blocker.team_key = "AGE".to_owned();
        blocker.project_identifier = Some("AGE-Project".to_owned());
        blocker.assignee_id = Some("blocker-worker".to_owned());
        evidence.tickets.push(blocker);
        if let Some(ir626) = evidence
            .tickets
            .iter_mut()
            .find(|item| item.identifier == "IR-626")
        {
            ir626.team_key = "IR".to_owned();
            ir626.assignee_id = Some("goal-worker".to_owned());
        }
        for issue in &mut evidence.tickets {
            if matches!(issue.identifier.as_str(), "QSL-635" | "QSL-641") {
                issue.team_key = "QSL".to_owned();
            }
        }
        for (id, identifier) in [
            ("qsl485", "QSL-485"),
            ("qsl486", "QSL-486"),
            ("qsl487", "QSL-487"),
        ] {
            evidence.transitions.push(IssueTransition {
                event_id: format!("last-{id}"),
                issue_id: id.to_owned(),
                ticket_id: identifier.to_owned(),
                to_state: "Todo".to_owned(),
                occurred_at: "2026-10-05T05:00:00Z".to_owned(),
                queue_before: None,
            });
        }
        let mut b4_pr = pr(641, "feature/no-issue", "B4", "2026-10-04T00:00:00Z", None);
        b4_pr.updated_at = Some("2026-10-05T04:00:00Z".to_owned());
        let mut qsl_pr = pr(
            700,
            "feature/QSL-641",
            "QSL-641",
            "2026-10-05T16:00:00Z",
            None,
        );
        qsl_pr.updated_at = Some("2026-10-05T17:00:00Z".to_owned());
        let mut ir_pr = pr(
            701,
            "feature/IR-626",
            "IR-626",
            "2026-10-05T16:00:00Z",
            None,
        );
        ir_pr.updated_at = Some("2026-10-05T17:00:00Z".to_owned());
        evidence.pull_requests.extend([b4_pr, qsl_pr, ir_pr]);
        evidence.commits.push(DriftCommit {
            sha: "goal-progress-commit".to_owned(),
            repository: "agent-ix/example".to_owned(),
            committed_at: "2026-10-05T17:30:00Z".to_owned(),
            ticket_ids: vec!["QSL-485".to_owned()],
        });
        evidence.transitions.push(IssueTransition {
            event_id: "qsl-last-completion".to_owned(),
            issue_id: "qsl485".to_owned(),
            ticket_id: "QSL-485".to_owned(),
            to_state: "Done".to_owned(),
            occurred_at: "2026-10-04T06:00:00Z".to_owned(),
            queue_before: None,
        });
        let report = evaluate_drift(&config, &evidence).expect("valid priority history");
        let goal = &report.priority_goals[0];
        assert_eq!(goal.flagged, Some(true));
        assert_eq!(
            goal.last_completion_at.as_deref(),
            Some("2026-10-04T06:00:00Z")
        );
        assert_eq!(
            goal.last_transition_at.as_deref(),
            Some("2026-10-05T05:00:00Z")
        );
        assert_eq!(goal.hours_since_progress, Some(13.0));
        assert_eq!(goal.no_progress_hours, 12.0);
        assert_eq!(goal.active_workers, Some(2));
        assert_eq!(goal.minimum_workers, 1);
        assert_eq!(goal.crowding_out.len(), 2);
        assert_eq!(goal.crowding_out[0].team_key, "IR");
        assert_eq!(goal.crowding_out[0].tickets, vec!["IR-626"]);
        assert_eq!(goal.crowding_out[1].team_key, "QSL");
        assert_eq!(goal.crowding_out[1].tickets, vec!["QSL-635", "QSL-641"]);
        assert_eq!(goal.out_of_team_activity.len(), 1);
        assert_eq!(goal.out_of_team_activity[0].tickets, vec!["IR-626"]);
        let json = serde_json::to_value(goal).expect("priority goal JSON");
        assert_eq!(json["active_workers"], 2);
        let fields: BTreeSet<_> = json
            .as_object()
            .expect("priority goal object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields,
            BTreeSet::from([
                "goal",
                "no_progress_hours",
                "last_completion_at",
                "last_transition_at",
                "hours_since_progress",
                "active_workers",
                "minimum_workers",
                "status",
                "flagged",
                "trusted",
                "blocked_on",
                "crowding_out",
                "out_of_team_activity",
                "missing_sources",
            ])
        );
        assert!(render_drift_text(&report).contains("active_workers=2/1"));
    }

    #[test]
    fn priority_goal_credits_transitive_blocker_work_and_separates_other_teams() {
        let mut config = config();
        config.report_time = "2026-10-07T15:10:00Z".to_owned();
        config.priority_goals.push(PriorityGoalConfig {
            goal: "QSL L2 plan v2".to_owned(),
            ticket_ids: vec!["QSL-487".to_owned()],
            minimum_workers: 1,
            no_progress_hours: 12.0,
        });
        let mut evidence = evidence();
        evidence.missing.extend([
            MissingEvidence {
                source: "github.prs".to_owned(),
                reason: "live source unavailable".to_owned(),
            },
            MissingEvidence {
                source: "github.commits".to_owned(),
                reason: "live source unavailable".to_owned(),
            },
        ]);

        let mut goal = ticket("goal", "QSL-487", "qsl-l2", "In Progress", 1.0);
        goal.team_key = "QSL".to_owned();
        goal.assignee_id = Some("coder-1".to_owned());
        goal.project_identifier = Some("QSL L2 plan v2".to_owned());
        goal.created_at = "2026-10-01T00:00:00Z".to_owned();
        goal.state_entered_at = Some("2026-10-01T00:00:00Z".to_owned());
        goal.blocked_by = vec!["age-2224".to_owned()];

        let mut direct_blocker = ticket("age-2224", "AGE-2224", "age", "In Progress", 1.0);
        direct_blocker.team_key = "AGE".to_owned();
        direct_blocker.assignee_id = Some("coder-1".to_owned());
        direct_blocker.created_at = "2026-10-01T00:00:00Z".to_owned();
        direct_blocker.updated_at = Some("2026-10-07T15:08:00Z".to_owned());
        direct_blocker.blocked_by = vec!["age-2228".to_owned()];

        let mut transitive_blocker = ticket("age-2228", "AGE-2228", "age", "Coding", 1.0);
        transitive_blocker.team_key = "AGE".to_owned();
        transitive_blocker.assignee_id = Some("coder-1".to_owned());
        transitive_blocker.created_at = "2026-10-01T00:00:00Z".to_owned();
        transitive_blocker.updated_at = Some("2026-10-07T15:07:00Z".to_owned());

        let mut same_team = ticket("qsl-635", "QSL-635", "qsl-ir", "Coding", 1.0);
        same_team.team_key = "QSL".to_owned();
        same_team.project_identifier = Some("QSL-IR".to_owned());
        let mut other_team = ticket("ir-626", "IR-626", "ir", "In Progress", 1.0);
        other_team.team_key = "IR".to_owned();
        other_team.assignee_id = Some("coder-1".to_owned());
        other_team.project_identifier = Some("IR".to_owned());
        let mut unrelated_team = ticket("sapho-39", "SAPHO-39", "sapho", "In Progress", 1.0);
        unrelated_team.team_key = "SAPHO".to_owned();
        unrelated_team.assignee_id = Some("coder-2".to_owned());
        unrelated_team.project_identifier = Some("SAPHO".to_owned());
        evidence.tickets.extend([
            goal,
            direct_blocker,
            transitive_blocker,
            same_team,
            other_team,
            unrelated_team,
        ]);
        for (id, ticket_id, team, at) in [
            ("qsl-event", "QSL-635", "qsl-635", "2026-10-07T15:01:00Z"),
            ("ir-event", "IR-626", "ir-626", "2026-10-07T15:02:00Z"),
            (
                "blocker-event",
                "AGE-2228",
                "age-2228",
                "2026-10-07T15:03:00Z",
            ),
            (
                "unrelated-event",
                "SAPHO-39",
                "sapho-39",
                "2026-10-07T15:04:00Z",
            ),
        ] {
            evidence.transitions.push(IssueTransition {
                event_id: id.to_owned(),
                issue_id: team.to_owned(),
                ticket_id: ticket_id.to_owned(),
                to_state: "In Progress".to_owned(),
                occurred_at: at.to_owned(),
                queue_before: None,
            });
        }

        let report = evaluate_drift(&config, &evidence).expect("blocker-aware report");
        let goal = &report.priority_goals[0];
        assert_eq!(goal.flagged, None);
        assert_eq!(goal.status, PriorityGoalStatus::Blocked);
        assert!(!goal.trusted, "missing GitHub activity is explicit");
        assert_eq!(goal.active_workers, Some(1));
        assert_eq!(goal.blocked_on, vec!["AGE-2224", "AGE-2228"]);
        assert_eq!(goal.crowding_out.len(), 3);
        assert_eq!(goal.crowding_out[0].team_key, "IR");
        assert_eq!(goal.crowding_out[0].tickets, vec!["IR-626"]);
        assert_eq!(goal.crowding_out[1].team_key, "QSL");
        assert_eq!(goal.crowding_out[1].tickets, vec!["QSL-635"]);
        assert_eq!(goal.crowding_out[2].team_key, "SAPHO");
        assert_eq!(goal.crowding_out[2].tickets, vec!["SAPHO-39"]);
        assert_eq!(goal.out_of_team_activity.len(), 1);
        assert_eq!(goal.out_of_team_activity[0].team_key, "IR");
        assert_eq!(goal.out_of_team_activity[0].tickets, vec!["IR-626"]);
        let rendered = render_drift_text(&report);
        assert!(rendered.contains("trusted=false"));
        assert!(rendered.contains("status=blocked flagged=unknown"));
        assert!(rendered.contains("blocked_on=[AGE-2224,AGE-2228]"));
        assert!(rendered.contains("out_of_team_activity=[IR/IR: IR-626]"));
    }

    #[test]
    fn done_goal_ticket_does_not_report_its_open_blocker() {
        let mut config = config();
        config.report_time = "2026-10-07T15:10:00Z".to_owned();
        config.priority_goals.push(PriorityGoalConfig {
            goal: "QSL L2 plan v2".to_owned(),
            ticket_ids: vec!["QSL-472".to_owned()],
            minimum_workers: 1,
            no_progress_hours: 12.0,
        });
        let mut evidence = evidence();
        let mut done_goal = ticket("qsl472", "QSL-472", "qsl-l2", "Done", 1.0);
        done_goal.team_key = "QSL".to_owned();
        done_goal.project_identifier = Some("QSL L2 plan v2".to_owned());
        done_goal.blocked_by = vec!["std126".to_owned()];
        let mut blocker = ticket("std126", "STD-126", "standard", "In Progress", 1.0);
        blocker.team_key = "STD".to_owned();
        evidence.tickets.extend([done_goal, blocker]);

        let report = evaluate_drift(&config, &evidence).expect("completed goal report");
        let goal = &report.priority_goals[0];
        assert!(goal.blocked_on.is_empty());
        assert_eq!(goal.status, PriorityGoalStatus::OnTrack);
        assert_eq!(goal.flagged, Some(false));
        assert_eq!(goal.active_workers, Some(0));
    }

    #[test]
    fn active_linear_transition_with_missing_git_evidence_is_unknown_not_flagged() {
        let mut config = config();
        config.report_time = "2026-10-07T15:10:00Z".to_owned();
        config.priority_goals.push(PriorityGoalConfig {
            goal: "QSL L2 plan v2".to_owned(),
            ticket_ids: vec!["QSL-487".to_owned()],
            minimum_workers: 1,
            no_progress_hours: 12.0,
        });
        let mut evidence = evidence();
        evidence.missing.extend([
            MissingEvidence {
                source: "github.prs".to_owned(),
                reason: "live source unavailable".to_owned(),
            },
            MissingEvidence {
                source: "github.commits".to_owned(),
                reason: "live source unavailable".to_owned(),
            },
        ]);
        let mut active = ticket("qsl487", "QSL-487", "qsl-l2", "Coding", 1.0);
        active.team_key = "QSL".to_owned();
        active.assignee_id = Some("worker-1".to_owned());
        active.project_identifier = Some("QSL L2 plan v2".to_owned());
        active.created_at = "2026-10-01T00:00:00Z".to_owned();
        active.state_entered_at = Some("2026-10-07T15:09:00Z".to_owned());
        evidence.tickets.push(active);
        evidence.transitions.push(IssueTransition {
            event_id: "qsl487-started".to_owned(),
            issue_id: "qsl487".to_owned(),
            ticket_id: "QSL-487".to_owned(),
            to_state: "Coding".to_owned(),
            occurred_at: "2026-10-07T15:09:00Z".to_owned(),
            queue_before: None,
        });

        let report = evaluate_drift(&config, &evidence).expect("partial activity report");
        let goal = &report.priority_goals[0];
        assert!(!goal.trusted);
        assert_eq!(goal.flagged, None);
        assert_eq!(goal.status, PriorityGoalStatus::Unknown);
        assert_eq!(goal.active_workers, Some(1));
        assert!((goal.hours_since_progress.unwrap_or_default() - (1.0 / 60.0)).abs() < 1e-9);
        assert!(
            render_drift_text(&report).contains("status=unknown flagged=unknown trusted=false")
        );
    }

    #[test]
    fn priority_goal_worker_count_is_unknown_when_linear_issue_evidence_is_incomplete() {
        let mut config = config();
        config.report_time = "2026-10-07T15:10:00Z".to_owned();
        config.priority_goals.push(PriorityGoalConfig {
            goal: "QSL L2 plan v2".to_owned(),
            ticket_ids: vec!["QSL-487".to_owned()],
            minimum_workers: 1,
            no_progress_hours: 12.0,
        });
        let mut evidence = evidence();
        evidence.missing.push(MissingEvidence {
            source: "linear.issues".to_owned(),
            reason: "issue page was unavailable".to_owned(),
        });
        let mut active = ticket("qsl487", "QSL-487", "qsl-l2", "Coding", 1.0);
        active.team_key = "QSL".to_owned();
        active.assignee_id = Some("worker-1".to_owned());
        active.project_identifier = Some("QSL L2 plan v2".to_owned());
        evidence.tickets.push(active);

        let report = evaluate_drift(&config, &evidence).expect("partial Linear evidence report");
        let goal = &report.priority_goals[0];
        assert_eq!(goal.active_workers, None);
        assert_eq!(goal.status, PriorityGoalStatus::Unknown);
        assert!(
            goal.missing_sources
                .iter()
                .any(|source| source.contains("linear.issues"))
        );
        let json = serde_json::to_value(goal).expect("priority goal JSON");
        assert!(json["active_workers"].is_null());
        assert!(render_drift_text(&report).contains("active_workers=unknown/1"));
    }

    #[test]
    fn completed_issue_coding_history_does_not_report_missing_queue_noise() {
        let mut evidence = evidence();
        evidence.tickets[0].state = "Done".to_owned();
        evidence.transitions.push(IssueTransition {
            event_id: "completed-coding-transition".to_owned(),
            issue_id: "i1".to_owned(),
            ticket_id: "SWM-1".to_owned(),
            to_state: "Coding".to_owned(),
            occurred_at: "2026-09-28T12:00:00Z".to_owned(),
            queue_before: None,
        });
        let report = evaluate_drift(&config(), &evidence).expect("completed history is harmless");
        assert!(report.leads[0].signals.veering.tickets.is_empty());
        assert!(
            report.leads[0]
                .signals
                .veering
                .common
                .missing_sources
                .iter()
                .all(|source| !source.contains("queue before SWM-1"))
        );
    }

    #[test]
    fn duplicate_team_mapping_and_unknown_evidence_version_are_rejected() {
        let mut duplicate = config();
        duplicate.leads.push(DriftLead {
            lead_id: "lead-b".to_owned(),
            team_key: "SWM".to_owned(),
        });
        assert!(matches!(
            evaluate_drift(&duplicate, &evidence()),
            Err(DriftError::InvalidConfig { .. })
        ));
        let envelope = DriftEvidenceFile {
            schema_version: DRIFT_EVIDENCE_SCHEMA_VERSION + 1,
            evidence: evidence(),
        };
        assert!(matches!(
            evaluate_drift_file(&config(), &envelope),
            Err(DriftError::UnsupportedEvidenceVersion { .. })
        ));
    }

    #[test]
    fn partial_github_pagination_cannot_prove_a_stall() {
        let mut evidence = evidence();
        evidence.tickets[0].state = "Coding".to_owned();
        evidence.tickets[0].state_entered_at = Some("2026-09-25T00:00:00Z".to_owned());
        evidence.missing.push(MissingEvidence {
            source: "github.prs".to_owned(),
            reason: "page 2 unavailable".to_owned(),
        });
        let report = evaluate_drift(&config(), &evidence).expect("partial read is reportable");
        assert!(report.leads[0].signals.stall.tickets.is_empty());
        assert_eq!(
            report.leads[0].signals.stall.common.availability,
            Availability::Unavailable
        );
    }

    #[test]
    fn unresolved_project_mapping_makes_pr_scope_unavailable() {
        let mut evidence = evidence();
        evidence.tickets[0].project_id = Some("missing-project".to_owned());
        evidence.pull_requests.push(pr(
            10,
            "feature/SWM-1",
            "SWM-1",
            "2026-09-25T00:00:00Z",
            None,
        ));
        let report = evaluate_drift(&config(), &evidence).expect("missing mapping is reportable");
        let signal = &report.leads[0].signals.pr_scope;
        assert_eq!(signal.common.availability, Availability::Unavailable);
        assert!(
            signal
                .common
                .missing_sources
                .iter()
                .any(|reason| reason.contains("missing-project"))
        );
        assert!(signal.prs.is_empty());
    }

    #[test]
    fn date_like_numeric_text_is_not_a_ticket_identifier() {
        assert!(extract_ticket_ids("2026-09-27", "Build 2026-09-27").is_empty());
        assert!(extract_ticket_ids("1-2", "2026 release").is_empty());
        assert_eq!(
            extract_ticket_ids("feature/swm-27", "SWM-27 fix"),
            vec!["SWM-27"]
        );
    }

    #[test]
    fn duplicate_pr_rows_keep_merge_metadata_from_the_later_page() {
        let mut evidence = evidence();
        let mut original = pr(11, "feature/SWM-1", "SWM-1", "2026-09-10T00:00:00Z", None);
        original.updated_at = Some("2026-09-24T00:00:00Z".to_owned());
        let mut merged = original.clone();
        merged.merged_at = Some("2026-09-25T00:00:00Z".to_owned());
        merged.updated_at = Some("2026-09-26T00:00:00Z".to_owned());
        merged.event_ids.push("merge-event".to_owned());
        evidence.pull_requests = vec![original, merged];
        assert_eq!(
            deduplicated_prs(&evidence.pull_requests)[0]
                .updated_at
                .as_deref(),
            Some("2026-09-26T00:00:00Z")
        );
        let report = evaluate_drift(&config(), &evidence).expect("duplicate PR pages merge");
        assert_eq!(report.leads[0].signals.ticket_churn.merged_prs, Some(1));
        assert!(report.leads[0].signals.pr_scope.prs.is_empty());
    }

    #[test]
    fn conflicting_ticket_ids_do_not_count_as_stall_activity() {
        let mut evidence = evidence();
        evidence.tickets[0].state = "Coding".to_owned();
        evidence.tickets[0].state_entered_at = Some("2026-09-25T00:00:00Z".to_owned());
        evidence.pull_requests.push(pr(
            12,
            "feature/SWM-1-OPS-9",
            "SWM-1 OPS-9",
            "2026-09-27T11:00:00Z",
            None,
        ));
        let report = evaluate_drift(&config(), &evidence).expect("conflicting PR is unassigned");
        assert_eq!(report.leads[0].signals.stall.tickets.len(), 1);
        assert_eq!(
            report.unassigned_prs[0].reason,
            UnassignedReason::ConflictingTicketIds
        );
    }

    #[test]
    fn timestamp_offsets_round_trip_to_utc_and_empty_leads_stay_empty() {
        let value = timestamp_epoch("2026-09-27T14:00:00+02:00").expect("valid offset timestamp");
        assert_eq!(format_timestamp(value), REPORT);
        assert!(timestamp_epoch("2026-02-30T00:00:00Z").is_err());
        let mut empty = config();
        empty.leads.clear();
        let report = evaluate_drift(&empty, &evidence()).expect("valid empty config");
        assert!(report.leads.is_empty());
        assert!(render_drift_text(&report).is_empty());
    }
}
