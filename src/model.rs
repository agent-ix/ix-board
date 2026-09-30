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
    /// Linear workflow category, independent of its display name.
    pub state_type: WorkflowStateType,
    /// Parent ticket UUID, distinct from prerequisite dependencies.
    pub parent_id: Option<IssueId>,
    /// Team identifier.
    pub team_key: TeamKey,
    /// Associated project UUID.
    pub project_id: Option<ProjectId>,
    /// Linear numeric priority; zero means unprioritized, while lower nonzero values rank higher.
    pub priority: f64,
    /// Linear manual issue order.
    pub sort_order: f64,
    /// UUIDs of issues this issue is blocked by.
    pub blocked_by: Vec<IssueId>,
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
