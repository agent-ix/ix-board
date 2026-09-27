// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Typed records and command results for structured Linear reads.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Stable Linear issue UUID used for relationships.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IssueId(pub String);

/// Stable Linear project UUID used for lookup and relationships.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);

/// Linear team key used as a read scope.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TeamKey(pub String);

/// Stable structured project record fetched from Linear.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    /// Structured lifecycle status name.
    pub status: String,
    /// Linear project priority; zero means unprioritized, while lower nonzero values rank higher.
    pub priority: i32,
    /// Linear manual project order.
    pub sort_order: f64,
    /// Teams associated with this project.
    pub team_keys: Vec<TeamKey>,
    /// Project milestones in Linear order.
    pub milestones: Vec<Milestone>,
}

/// A project milestone record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    /// Projects in the Linear workspace.
    pub projects: Vec<Project>,
    /// Issues in the Linear workspace, including cross-team prerequisites.
    pub issues: Vec<Issue>,
}

/// Next step inferred from a structured spec state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NextStep {
    /// The issue should be specified.
    Specify,
    /// The issue can move to coding.
    Coding,
}

/// One eligible issue in a team's ready queue.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    /// Project record.
    Project,
    /// Issue record.
    Issue,
}

/// One matching structured project or issue.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    !matches!(issue.state.as_str(), "Completed" | "Canceled")
}

pub(crate) fn compare_f64(left: f64, right: f64) -> Ordering {
    left.total_cmp(&right)
}
