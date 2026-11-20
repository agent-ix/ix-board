// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Source-native evidence. Missing capabilities are isolated per fact family (FR-029).
use super::Scope;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

wire! {
    /// Attribution for classifications; absence is never interpreted as a classification.
    Attribution {
        /// Human or tool native actor.
        actor: String,
        /// Classifier release.
        classifier_version: String,
        /// Native record offset start.
        offset_start: u64,
        /// Native record offset end.
        offset_end: u64
    }
}
wire! {
    /// Stable normalized event and its persisted source row.
    Observation {
        /// Stable source event ID, never derived from display labels.
        event_id: String,
        /// Source-native entity identity.
        entity_id: String,
        /// Source event UTC time, if supplied.
        event_at: Option<String>,
        /// Actual source observation UTC time.
        observed_at: Option<String>,
        /// Stable source sequence for equal event timestamps.
        sequence: Option<u64>,
        /// Native selected populations.
        scope: Scope,
        /// Whether the source row is complete.
        complete: bool,
        /// Native record reference, without private transcript bodies.
        source_ref: String,
        /// Optional explicit classification attribution.
        attribution: Option<Attribution>,
        /// Typed source facts.
        fact: Fact
    }
}
/// Facts carry only source observations, never inferred semantic prose.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Fact {
    /// Current tracker issue.
    Issue(IssueFact),
    /// Complete native state entry.
    State(StateFact),
    /// Current lane readiness/activity observation.
    Lane(LaneFact),
    /// Pull request review and diff evidence.
    PullRequest(PrFact),
    /// Resolved crate pin and native reachability evidence.
    Pin(PinFact),
    /// Tag or repin event for one dependency.
    PinEvent(PinEvent),
    /// Attributed invocation with explicit scope.
    Check(CheckFact),
    /// Original lifecycle report, preserved without reclassification.
    Lifecycle(crate::agents::AgentsReport),
}
wire! {
    /// Current issue with capability-specific absence explicit.
    IssueFact {
        /// Source-native team key.
        team: String,
        /// Native project identity.
        project: Option<String>,
        /// Native workflow state identity.
        state: String,
        /// Started category supplied by the tracker.
        started: bool,
        /// Terminal, including canceled/duplicate.
        terminal: bool,
        /// Native blocker UUIDs; complete batch resolves their reachability.
        blockers: BTreeSet<String>,
        /// Latest verified started/claim UTC timestamp.
        claim_at: Option<String>,
        /// Whether movement history covers the claim interval.
        movement_complete: bool,
        /// Latest verified PR-head, commit or worktree movement.
        movement_at: Option<String>,
        /// Latest verified entry to blocked condition.
        blocked_at: Option<String>,
        /// Merged attached pull request, unknown when attachment read incomplete.
        merged_pr: Option<bool>,
        /// Verified named completion obligations.
        obligations: Option<BTreeSet<String>>
    }
}
wire! {
    /// One native state entry; full history includes initial entry.
    StateFact { /// Native state identity.
        state: String }
}
wire! {
    /// Verified lane activity and structured ready count.
    LaneFact { /// Verified idle state.
        idle: bool, /// Structured ready population.
        ready_count: u64 }
}
wire! {
    /// Review/diff/check attributes for a pull request.
    PrFact {
        /// Source-native repository.
        repo: String,
        /// Open at the observed head.
        open: bool,
        /// Draft status.
        draft: bool,
        /// Native merge timestamp.
        merged_at: Option<String>,
        /// Review-ready event timestamp; creation is not substituted.
        review_ready_at: Option<String>,
        /// Assigned reviewer count, unknown if history unavailable.
        assigned_reviewers: Option<u64>,
        /// Qualifying review count.
        qualifying_reviews: Option<u64>,
        /// Attributed substantive diff classification.
        repin_only: Option<bool>,
        /// Complete reviewer fix round count.
        fix_rounds: Option<u64>,
        /// Complete attributed full gate count for this PR.
        full_gates: Option<u64>
    }
}
wire! {
    /// One dependency resolution at a scoped repository head.
    PinFact { /// Repository native ID.
        repo: String, /// Dependency crate ID.
        dependency: String, /// Exact resolved revision.
        revision: String, /// Reachable ancestor of dependency default head.
        on_main: Option<bool> }
}
/// Tag/repin event kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PinEventKind {
    /// Published native tag.
    Tag,
    /// Committed dependency resolution change.
    Repin,
}
wire! {
    /// Ordered event linked to its dependency.
    PinEvent { /// Dependency native identity.
        dependency: String, /// Source event type.
        event: PinEventKind }
}
/// Explicit invocation scope; never inferred from a command name.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckScope {
    /// Focused affected-crate invocation.
    Focused,
    /// Workspace invocation.
    Workspace,
}
wire! {
    /// Logged check invocation.
    CheckFact { /// Attributed scope.
        scope: CheckScope, /// Native PR ID, when attributed.
        pr: Option<String> }
}
wire! {
    /// Complete source pass with independent capability coverage.
    #[derive(Default)]
    Evidence {
        /// Source adapter name used by freshness configuration.
        source: String,
        /// Actual source observation UTC time; missing remains unknown.
        observed_at: Option<String>,
        /// Fully paginated fact families (issue, state, lane, pull_request, pin, pin_event, check).
        complete: BTreeSet<String>,
        /// Missing family capability reason.
        reasons: BTreeMap<String, String>,
        /// Normalized source rows.
        observations: Vec<Observation>
    }
}

/// Converts a complete current tracker snapshot without inventing unavailable histories.
pub fn workspace_evidence(snapshot: &crate::WorkspaceSnapshot, scope: &Scope) -> Evidence {
    let observed_at = crate::drift::format_timestamp(snapshot.fetched_at as f64);
    let observations = snapshot
        .issues
        .iter()
        .map(|issue| Observation {
            event_id: format!(
                "linear:issue:{}:{}",
                issue.id.0,
                issue.updated_at.as_deref().unwrap_or("unknown")
            ),
            entity_id: issue.id.0.clone(),
            event_at: issue.updated_at.clone(),
            observed_at: Some(observed_at.clone()),
            sequence: None,
            scope: scope.clone(),
            complete: true,
            source_ref: format!("linear:issue:{}", issue.id.0),
            attribution: None,
            fact: Fact::Issue(IssueFact {
                team: issue.team_key.0.clone(),
                project: issue.project_id.as_ref().map(|id| id.0.clone()),
                state: issue.state_name.clone(),
                started: issue.state_type == crate::WorkflowStateType::Started,
                terminal: !crate::model::issue_is_open(issue),
                blockers: issue.blocked_by.iter().map(|id| id.0.clone()).collect(),
                claim_at: None,
                movement_complete: false,
                movement_at: None,
                blocked_at: None,
                merged_pr: None,
                obligations: None,
            }),
        })
        .collect();
    Evidence {
        source: "linear".into(),
        observed_at: Some(observed_at),
        complete: ["issue".into()].into(),
        reasons: [(
            "state".into(),
            "native ordered complete state events unavailable from tracker adapter".into(),
        )]
        .into(),
        observations,
    }
}
