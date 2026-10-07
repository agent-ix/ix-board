// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Typed records and command results for structured Linear reads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use crate::Error;

/// Stable Linear issue UUID used for relationships.
#[derive(
    Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(transparent)]
pub struct IssueId(pub String);

/// Stable Linear project UUID used for lookup and relationships.
#[derive(
    Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ProjectId(pub String);

/// Linear team key used as a read scope.
#[derive(
    Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(transparent)]
pub struct TeamKey(pub String);

/// Project lifecycle command accepted by the project status mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectRunStatus {
    /// Run the project and include its otherwise-ready tickets in `next`.
    Started,
    /// Hold the project and exclude its tickets from `next`.
    Paused,
}

impl ProjectRunStatus {
    /// Canonical CLI token for this run status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "Started",
            Self::Paused => "Paused",
        }
    }
}

impl Display for ProjectRunStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProjectRunStatus {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "Started" => Ok(Self::Started),
            "Paused" => Ok(Self::Paused),
            _ => Err(Error::InvalidProjectStatus {
                value: value.to_owned(),
            }),
        }
    }
}

/// Linear project priority accepted by the project priority mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectPriority {
    /// Linear priority value 0.
    None,
    /// Linear priority value 1.
    Urgent,
    /// Linear priority value 2.
    High,
    /// Linear priority value 3.
    Medium,
    /// Linear priority value 4.
    Low,
}

impl ProjectPriority {
    /// Numeric Linear API value for this priority.
    pub fn linear_value(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Urgent => 1,
            Self::High => 2,
            Self::Medium => 3,
            Self::Low => 4,
        }
    }

    /// Human-readable Linear priority label.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No priority",
            Self::Urgent => "Urgent",
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }
}

impl Display for ProjectPriority {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.linear_value(), self.label())
    }
}

impl FromStr for ProjectPriority {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "0" => Ok(Self::None),
            "1" => Ok(Self::Urgent),
            "2" => Ok(Self::High),
            "3" => Ok(Self::Medium),
            "4" => Ok(Self::Low),
            _ => Err(Error::InvalidProjectPriority {
                value: value.to_owned(),
            }),
        }
    }
}

/// Stable structured project record fetched from Linear.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Linear UUID.
    pub id: ProjectId,
    /// Human-readable project identifier, when configured.
    pub identifier: Option<String>,
    /// Project name.
    pub name: String,
    /// Project description, used as the project goal.
    pub description: String,
    /// Project content in markdown.
    pub content: Option<String>,
    /// Canonical run state (`Started`) or the Linear display name for other statuses.
    pub status: String,
    /// Linear project priority; zero means unprioritized, while lower nonzero values rank higher.
    pub priority: i32,
    /// Linear manual project order.
    pub sort_order: f64,
    /// Linear project target date, when configured.
    pub target_date: Option<String>,
    /// Teams associated with this project.
    pub team_keys: Vec<TeamKey>,
    /// Project milestones in Linear order.
    pub milestones: Vec<Milestone>,
    /// Initiatives containing this project.
    pub initiatives: Vec<Initiative>,
}

/// A project milestone record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Milestone {
    /// Linear UUID.
    pub id: String,
    /// Milestone name.
    pub name: String,
    /// Milestone description.
    pub description: Option<String>,
    /// Linear manual order.
    pub sort_order: f64,
    /// Linear milestone target date, when configured.
    pub target_date: Option<String>,
}

/// A structured Linear issue record and its prerequisite identifiers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    /// Linear UUID.
    pub id: IssueId,
    /// Human-readable issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Issue description, searched as structured issue data only.
    pub description: Option<String>,
    /// Workflow state name.
    pub state: String,
    /// Linear workflow-state display name before terminal-state canonicalization.
    pub state_name: String,
    /// Linear workflow category, independent of its display name.
    pub state_type: WorkflowStateType,
    /// Parent ticket UUID, distinct from prerequisite dependencies.
    pub parent_id: Option<IssueId>,
    /// Team identifier.
    pub team_key: TeamKey,
    /// Associated project UUID.
    pub project_id: Option<ProjectId>,
    /// Associated project milestone UUID, when the issue belongs to a milestone.
    pub project_milestone_id: Option<String>,
    /// Linear numeric priority; zero means unprioritized, while lower nonzero values rank higher.
    pub priority: f64,
    /// Linear manual issue order.
    pub sort_order: f64,
    /// UUIDs of issues this issue is blocked by.
    pub blocked_by: Vec<IssueId>,
    /// UTC creation timestamp from Linear.
    pub created_at: Option<String>,
    /// UTC last-update timestamp from Linear.
    pub updated_at: Option<String>,
    /// Structured attachment URLs returned by Linear.
    pub attachment_urls: Vec<String>,
    /// Whether Linear returned every attachment record for this issue.
    pub attachments_complete: bool,
}

/// Exact issue records returned for a batch, including unresolved selectors.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueBatch {
    /// Found issues in first-request order, with duplicate issues removed.
    pub issues: Vec<Issue>,
    /// Unknown selectors in first-request order.
    pub missing: Vec<String>,
    /// True when every selector resolved.
    pub complete: bool,
}

/// A complete structured read snapshot used by all read commands.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    /// Projects in the Linear workspace.
    pub projects: Vec<Project>,
    /// Issues in the Linear workspace, including cross-team prerequisites.
    pub issues: Vec<Issue>,
    /// Unix seconds of the actual source fetch, preserved by cached reads.
    pub fetched_at: i64,
}

/// Next step inferred from a structured spec state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NextStep {
    /// The issue should be specified.
    Specify,
    /// The issue can move to coding.
    Coding,
}

/// One eligible issue in a team's ready queue.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadyItem {
    /// Project identifier or name.
    pub project: String,
    /// Issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Next spec step.
    pub next_step: NextStep,
    /// Human-readable facts grounding readiness and order.
    pub reason: String,
}

/// All data returned by `project-chunk`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectChunk {
    /// Project identifier, falling back to its Linear UUID.
    pub identifier: String,
    /// Project name.
    pub name: String,
    /// Project goal from its structured description.
    pub goal: String,
    /// Milestones in manual order.
    pub milestones: Vec<Milestone>,
    /// Open project issues in identifier order.
    pub open_tickets: Vec<ChunkIssue>,
    /// Dependency layers reachable from open project tickets, prerequisites before dependents.
    pub blocker_layers: Vec<Vec<String>>,
}

/// Inline selectors resolved into a deterministic Linear workspace focus.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FocusSelection {
    /// Initiative UUIDs, identifiers, or exact names.
    pub initiatives: Vec<String>,
    /// Project UUIDs, identifiers, or exact names.
    pub projects: Vec<String>,
    /// Milestone UUIDs or exact names; a name must identify one milestone.
    pub milestones: Vec<String>,
    /// Issue UUIDs or identifiers, including descendants.
    pub issues: Vec<String>,
}

/// Focus expansion for a single current workspace snapshot.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusExpansion {
    /// Unix seconds when the source snapshot was fetched.
    pub fetched_at: i64,
    /// Selected projects and their milestone/project-level issues.
    pub projects: Vec<FocusProject>,
    /// Deduplicated union of all focus issues.
    pub focus_issues: Vec<FocusIssue>,
    /// Issues selected by explicit issue selectors and their descendants.
    pub issue_selector_issues: Vec<FocusIssue>,
    /// Complete blocker closures, prerequisites before dependents.
    pub blocker_layers: Vec<Vec<String>>,
}

/// Combined focus status, blocker attribution, and eligible ready work.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusStatus {
    /// Unix seconds when the source snapshot was fetched.
    pub fetched_at: i64,
    /// Exact milestone IDs or names limiting blocker roots and ready work; empty means all focus work.
    pub work_milestones: Vec<String>,
    /// Ready issues outside the selected work milestones; zero without a filter.
    pub excluded_ready_count: usize,
    /// Selected projects and milestone status, in deterministic order.
    pub projects: Vec<FocusProjectStatus>,
    /// Complete prerequisite closure in FR-002 layer order.
    pub blocker_layers: Vec<Vec<FocusBlocker>>,
    /// Focus issues eligible under FR-001, in its deterministic order.
    pub ready_queue: Vec<ReadyItem>,
}

/// Versioned ix-board focus value for storage or transport by an opaque data service.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusRecord {
    /// Wire contract version for readers that do not share this Rust crate.
    pub schema_version: u32,
    /// Canonical typed focus members using Linear IDs.
    pub selection: Vec<FocusMember>,
    /// Unix seconds when ix-board produced this record.
    pub reported_at: i64,
    /// Linear-backed status and source fetch time.
    pub status: FocusStatus,
}

/// One saved focus member, identified by its canonical Linear ID.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum FocusMember {
    Initiative(String),
    Project(String),
    Milestone(String),
    Issue(String),
}

/// A bounded dependency-ordered handoff for an inline focus.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusPlanChunk {
    /// Unix seconds when the source snapshot was fetched.
    pub fetched_at: i64,
    /// Exact milestone IDs or names limiting handoff roots; empty means all focus work.
    pub work_milestones: Vec<String>,
    /// Ordered open focus issues included in the handoff.
    pub issues: Vec<FocusPlanIssue>,
}

/// One open focus issue in a dependency-ordered handoff.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusPlanIssue {
    /// Stable Linear issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Milestone name, or null for project-level and unprojected issues.
    pub milestone: Option<String>,
    /// Longest-path dependency layer from an issue without prerequisites.
    pub dependency_layer: usize,
    /// Whether this issue satisfies FR-001 ready-queue eligibility.
    pub ready: bool,
    /// Structured facts explaining its FR-001 readiness result.
    pub reasons: Vec<String>,
    /// Sorted identifiers of other open focus issues transitively blocked by this issue.
    pub unblocks: Vec<String>,
    /// Bounded Acceptance Criteria section excerpt from the structured description.
    pub acceptance_criteria: Option<String>,
}

/// Status summary for one selected Linear project.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusProjectStatus {
    /// Linear project UUID.
    pub id: ProjectId,
    /// Human-readable project identifier, falling back to its UUID.
    pub identifier: String,
    /// Project name.
    pub name: String,
    /// Whether the entire project was selected or this row shows partial focus work.
    pub membership: FocusProjectMembership,
    /// Linear project target date, when configured.
    pub target_date: Option<String>,
    /// Open focus issue counts by Linear workflow-state display name.
    pub open_by_state: Vec<FocusStateCount>,
    /// Started focus issues assigned to this project.
    pub in_progress: Vec<FocusIssue>,
    /// Progress over countable focus leaf issues assigned to this project.
    pub progress: FocusProgress,
    /// Selected milestone status, in Linear milestone order.
    pub milestones: Vec<FocusMilestoneStatus>,
    /// Status of focus issues assigned to the project without a milestone.
    pub unmilestoned: FocusUnmilestonedStatus,
}

/// Scope represented by one project status row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FocusProjectMembership {
    FullProject,
    PartialFocus,
}

/// Status summary for one selected Linear project milestone.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusMilestoneStatus {
    /// Linear milestone UUID.
    pub id: String,
    /// Milestone name.
    pub name: String,
    /// Linear milestone target date, when configured.
    pub target_date: Option<String>,
    /// Open focus issue counts by Linear workflow-state display name.
    pub open_by_state: Vec<FocusStateCount>,
    /// Started focus issues assigned to this milestone.
    pub in_progress: Vec<FocusIssue>,
    /// Progress over countable focus leaf issues assigned to this milestone.
    pub progress: FocusProgress,
}

/// Status of project focus issues without a milestone assignment.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusUnmilestonedStatus {
    /// Open focus issue counts by Linear workflow-state display name.
    pub open_by_state: Vec<FocusStateCount>,
    /// Started focus issues without a milestone.
    pub in_progress: Vec<FocusIssue>,
    /// Progress over countable focus leaf issues without a milestone.
    pub progress: FocusProgress,
}

/// Open issue count for one workflow-state display name.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusStateCount {
    /// Linear workflow-state display name.
    pub state: String,
    /// Number of open resolved focus issues in this state and group.
    pub count: usize,
}

/// Leaf progress counts and completion percentage for one focus group.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusProgress {
    /// Completed countable leaf issues.
    pub done: usize,
    /// All countable leaf issues, excluding canceled and duplicate leaves.
    pub total: usize,
    /// Completion percentage from zero through 100, or null for no countable leaves.
    pub completed_percent: Option<f64>,
}

/// One issue in the complete prerequisite closure with its focus roots.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusBlocker {
    /// Stable Linear issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Linear workflow-state display name.
    pub state: String,
    /// FR-001 classification of the issue state.
    pub classification: FocusIssueClassification,
    /// Sorted stable identifiers of focus issues whose blocker closure contains this issue.
    pub focus_root_ids: Vec<String>,
}

/// FR-001 open and terminal workflow-state classes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "PascalCase")]
pub enum FocusIssueClassification {
    /// Issue is completed.
    Completed,
    /// Issue is canceled.
    Canceled,
    /// Issue remains open under FR-001.
    Open,
}

/// One selected project with issues grouped by their milestone association.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusProject {
    /// Linear project UUID.
    pub id: ProjectId,
    /// Human-readable project identifier, falling back to UUID.
    pub identifier: String,
    /// Project name.
    pub name: String,
    /// Project milestones in deterministic Linear order.
    pub milestones: Vec<FocusMilestone>,
    /// Issues assigned to this project without a milestone.
    pub project_level_issues: Vec<FocusIssue>,
}

/// Milestone metadata and its selected issues.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusMilestone {
    /// Linear milestone UUID.
    pub id: String,
    /// Milestone name.
    pub name: String,
    /// Optional milestone description.
    pub description: Option<String>,
    /// Issues assigned to this milestone.
    pub issues: Vec<FocusIssue>,
}

/// Stable summary of a Linear issue included in focus.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusIssue {
    /// Linear issue UUID.
    pub id: IssueId,
    /// Human-readable issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Workflow state name.
    pub state: String,
}

/// Issue summary included in a project chunk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChunkIssue {
    /// Issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
    /// Structured issue description.
    pub description: Option<String>,
    /// Workflow state name.
    pub state: String,
    /// Direct prerequisite identifiers, including completed and cross-team issues.
    pub blocked_by: Vec<String>,
}

/// Kind of record returned from structured search.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    /// Project record.
    Project,
    /// Issue record.
    Issue,
}

/// One matching structured project or issue.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchResult {
    /// Record kind.
    pub kind: SearchKind,
    /// Structured identifier, or UUID where no project identifier exists.
    pub identifier: String,
    /// Structured name or title.
    pub name: String,
}

pub(crate) fn issue_is_open(issue: &Issue) -> bool {
    !matches!(
        issue.state_type,
        WorkflowStateType::Completed | WorkflowStateType::Canceled | WorkflowStateType::Duplicate
    )
}

pub(crate) fn compare_f64(left: f64, right: f64) -> Ordering {
    left.total_cmp(&right)
}

#[cfg(test)]
mod tests {
    use super::{ProjectPriority, ProjectRunStatus};
    use std::str::FromStr;

    #[test]
    fn project_priority_tokens_map_exactly_to_linear_values() {
        let priorities = [
            ("0", ProjectPriority::None, "No priority"),
            ("1", ProjectPriority::Urgent, "Urgent"),
            ("2", ProjectPriority::High, "High"),
            ("3", ProjectPriority::Medium, "Medium"),
            ("4", ProjectPriority::Low, "Low"),
        ];
        for (token, priority, label) in priorities {
            let parsed = ProjectPriority::from_str(token).expect("supported priority");
            assert_eq!(parsed, priority);
            assert_eq!(
                parsed.linear_value(),
                i32::from_str(token).expect("integer token")
            );
            assert_eq!(parsed.label(), label);
        }
        for token in ["-1", "5", "1.0", "Urgent", "01"] {
            assert!(matches!(
                ProjectPriority::from_str(token),
                Err(crate::Error::InvalidProjectPriority { .. })
            ));
        }
    }

    #[test]
    fn project_status_tokens_are_exact_and_closed() {
        assert_eq!(
            ProjectRunStatus::from_str("Started").expect("Started token"),
            ProjectRunStatus::Started
        );
        assert_eq!(
            ProjectRunStatus::from_str("Paused").expect("Paused token"),
            ProjectRunStatus::Paused
        );
        assert!(matches!(
            ProjectRunStatus::from_str("Completed"),
            Err(crate::Error::InvalidProjectStatus { .. })
        ));
    }
}

/// Initiative identity and name attached to a project.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Initiative {
    /// Linear UUID.
    pub id: String,
    /// Human-readable Linear initiative identifier, such as I-10.
    pub identifier: String,
    /// Initiative name.
    pub name: String,
}

/// Linear workflow category catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStateType {
    /// Requires triage.
    Triage,
    /// Deferred work.
    Backlog,
    /// Work not yet started.
    Unstarted,
    /// Work in progress.
    Started,
    /// Completed work.
    Completed,
    /// Canceled work.
    Canceled,
    /// Duplicate work.
    Duplicate,
}
