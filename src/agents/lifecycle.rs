// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Harness-independent worker identities, lifecycle evidence and exact counts (FR-018).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Identity within one harness session's worker graph.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct AgentId(pub String);

/// The latest established assignment state; activity alone never changes it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// An assignment is executing.
    Running,
    /// An unfinished assignment is waiting on delegated work.
    Waiting,
    /// The thread is available but has no active assignment.
    Idle,
    /// The latest assignment completed.
    Completed,
    /// The assignment was interrupted or cancelled.
    Stopped,
    /// Its state could not be established.
    #[default]
    Unknown,
}

impl WorkerState {
    /// Whether this state represents an unfinished assignment.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Waiting)
    }
}

/// Worker threads and internal harness machinery are reported separately.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkerRole {
    /// Delegated work on behalf of the lead.
    Worker,
    /// Approval reviewers or other explicitly internal threads.
    Internal,
    /// Unrecognized thread source or missing metadata.
    #[default]
    Unknown,
}

/// Which adapter established the lifecycle facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// Claude metadata reconciled with transcript transitions.
    Claude,
    /// Codex structured session and task records.
    Codex,
    /// An adapter supplied by a future harness.
    Adapter,
}

/// One child's identity and lifecycle as of a machine-local observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkerObservation {
    /// Stable child identity, never its nickname.
    pub id: AgentId,
    /// Immediate parent; absence means ancestry is unavailable.
    pub parent_id: Option<AgentId>,
    /// Harness path, when present, for matching parent lifecycle observations.
    pub agent_path: Option<String>,
    /// Depth declared by the harness, reconciled against immutable parent links.
    pub declared_depth: Option<u64>,
    /// Whether this thread is user work or internal machinery.
    pub role: WorkerRole,
    /// Latest established assignment state.
    pub state: WorkerState,
    /// Last structured activity timestamp, not a liveness deadline.
    pub last_activity_at: Option<u64>,
    /// Last explicit lifecycle transition timestamp.
    pub state_changed_at: Option<u64>,
    /// Why the transition has no timestamp (metadata does not supply one).
    pub timestamp_unavailable_reason: Option<String>,
    /// Machine-local sample time.
    pub observed_at: u64,
    /// Provenance of these facts.
    pub source: EvidenceSource,
    /// Bounded source locators, without transcript content.
    pub provenance: Vec<String>,
    /// Reasons this observation is uncertain.
    pub issues: Vec<String>,
}

/// Availability of one independent source. Empty results can be complete.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Availability {
    /// Whether the source was completely collected and understood.
    pub complete: bool,
    /// Machine-readable diagnostic codes with bounded context, without transcript text.
    pub reasons: Vec<String>,
}

impl Availability {
    /// A successfully collected source, including an empty one.
    pub fn available() -> Self {
        Self {
            complete: true,
            reasons: Vec::new(),
        }
    }

    /// Adds an availability failure without discarding observations.
    pub fn missing(&mut self, reason: impl Into<String>) {
        self.complete = false;
        let reason = reason.into();
        if self.reasons.len() < 32 && !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

/// Independent source coverage for one pane.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAvailability {
    /// Child discovery, identity, ancestry and lifecycle evidence.
    pub lifecycle: Availability,
    /// Parent transcript and lifecycle evidence.
    pub transcript: Availability,
    /// Host process evidence; unsupported platforms are explicit.
    pub processes: Availability,
    /// Pane tail used to distinguish approvals and pending waits.
    pub pane: Availability,
    /// Git ages in the pane's working directory.
    pub git: Availability,
}

/// Deduplicated observations and nullable exact counts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerCounts {
    /// Unfinished direct workers, or unknown when evidence is incomplete.
    pub direct: Option<usize>,
    /// Unfinished workers at depth two or greater.
    pub nested: Option<usize>,
    /// Proven direct workers, even when the exact count is unknown.
    pub known_direct_active: usize,
    /// Proven nested workers, even when the exact count is unknown.
    pub known_nested_active: usize,
    /// Deduplicated, identity-ordered observations including excluded internal threads.
    pub workers: Vec<WorkerObservation>,
    /// Full coverage of the worker graph.
    pub availability: Availability,
}

/// Counts assignments from lifecycle evidence, independently of harness or file ages.
pub fn count_workers(
    root: &AgentId,
    observations: &[WorkerObservation],
    mut availability: Availability,
) -> WorkerCounts {
    let mut by_id = BTreeMap::<AgentId, WorkerObservation>::new();
    if observations.len() > 1024 {
        availability.missing("worker_state_limit");
    }
    for worker in observations.iter().take(1024) {
        if let Some(previous) = by_id.get_mut(&worker.id) {
            let mut comparable = worker.clone();
            comparable.observed_at = previous.observed_at;
            if previous != &comparable {
                if previous.role != worker.role || previous.source != worker.source {
                    previous.role = WorkerRole::Unknown;
                }
                if previous.parent_id != worker.parent_id {
                    previous.parent_id = None;
                }
                previous.state = WorkerState::Unknown;
                previous
                    .issues
                    .push("conflicting_identity_observations".into());
            }
        } else {
            by_id.insert(worker.id.clone(), worker.clone());
        }
    }
    let mut direct_complete = availability.complete;
    let mut nested_complete = availability.complete;
    let mut direct = 0;
    let mut nested = 0;
    for worker in by_id.values() {
        if worker.role == WorkerRole::Internal {
            continue;
        }
        let mut uncertain = worker.role == WorkerRole::Unknown
            || worker.state == WorkerState::Unknown
            || !worker.issues.is_empty()
            || &worker.id == root;
        let mut ancestors = BTreeSet::from([&worker.id]);
        let mut parent = worker.parent_id.as_ref();
        let mut depth = 1;
        loop {
            match parent {
                Some(id) if id == root => break,
                Some(id) if ancestors.insert(id) => {
                    parent = by_id.get(id).and_then(|w| w.parent_id.as_ref());
                    depth += 1;
                }
                _ => {
                    availability.missing(format!("unresolved_ancestry:{}", worker.id.0));
                    direct_complete = false;
                    nested_complete = false;
                    break;
                }
            }
        }
        if worker
            .declared_depth
            .is_some_and(|declared| u64::try_from(depth).ok() != Some(declared))
        {
            availability.missing("declared_depth_conflict");
            direct_complete = false;
            nested_complete = false;
            uncertain = true;
        }
        if uncertain {
            availability.missing(format!("uncertain_worker:{}", worker.id.0));
            if parent == Some(root) && depth == 1 {
                direct_complete = false;
            } else if parent == Some(root) {
                nested_complete = false;
            } else {
                direct_complete = false;
                nested_complete = false;
            }
        } else if parent == Some(root) && worker.state.is_active() {
            if depth == 1 {
                direct += 1;
            } else {
                nested += 1;
            }
        }
    }
    WorkerCounts {
        direct: direct_complete.then_some(direct),
        nested: nested_complete.then_some(nested),
        known_direct_active: direct,
        known_nested_active: nested,
        workers: by_id.into_values().collect(),
        availability,
    }
}

/// Streaming adapter state. It retains lifecycle facts, never full transcript messages.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    /// Latest assignment state.
    pub state: WorkerState,
    /// Timestamp of the latest transition.
    pub state_changed_at: Option<u64>,
    /// Latest structured record timestamp.
    pub last_activity_at: Option<u64>,
    /// Codex thread identity and ancestry from its header.
    pub identity: Option<(AgentId, Option<AgentId>)>,
    /// Thread path reported by the harness.
    pub agent_path: Option<String>,
    /// Harness-declared depth for cross-checking parent links.
    pub declared_depth: Option<u64>,
    /// Worker versus internal thread classification.
    pub role: WorkerRole,
    /// Successfully launched child identities or paths from the parent.
    pub expected_children: BTreeSet<String>,
    /// Latest parent-side snapshots of children, keyed by identity or path.
    pub child_states: BTreeMap<String, (u64, WorkerState)>,
    /// Immutable source thread for each correlated child snapshot.
    pub child_owners: BTreeMap<String, AgentId>,
    /// Claude record identity, checked against its discovered filename.
    pub record_agent_id: Option<AgentId>,
    /// Claude root ownership, checked against the selected lead.
    pub root_session_id: Option<AgentId>,
    /// Unsupported or malformed lifecycle evidence.
    pub issues: Vec<String>,
    /// Last proven assignment generation, when supplied by the harness.
    pub generation: Option<String>,
    /// Number of explicit assignments seen in a Claude child transcript.
    pub assignment_starts: u32,
    /// Most recent assistant text, bounded and cleared by a later assignment/tool call.
    pub assistant: Option<String>,
    /// Time the assistant text was observed.
    pub assistant_at: Option<u64>,
    /// Latest explicit end of lead execution, distinct from a worker's assignment.
    pub execution_ended_at: Option<u64>,
    /// Calls waiting for a correlated result; arguments/prompts are never retained.
    pub calls: BTreeMap<String, PendingCall>,
}

/// Bounded correlation metadata for one lifecycle tool call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingCall {
    /// Tool name with namespace removed.
    pub name: String,
    /// Target path/identity, never the assigned message.
    pub target: Option<String>,
}

impl Lifecycle {
    /// Adds a bounded, deduplicated diagnostic.
    pub fn issue(&mut self, reason: &str) {
        if self.issues.len() < 32 && !self.issues.iter().any(|s| s == reason) {
            self.issues.push(reason.into());
        }
    }
    /// Records an explicit transition in chronological order. Untimed transitions are uncertain.
    pub fn transition(&mut self, state: WorkerState, at: Option<u64>) {
        let Some(at) = at else {
            self.issue("lifecycle_timestamp_missing");
            return;
        };
        // Record order within one transcript is authoritative, including
        // transitions that occur within the same second.
        if self.state_changed_at.is_none_or(|previous| at >= previous) {
            self.state = state;
            self.state_changed_at = Some(at);
            if state == WorkerState::Running {
                self.execution_ended_at = None;
            }
            if matches!(state, WorkerState::Completed | WorkerState::Stopped) {
                self.execution_ended_at = Some(at);
            }
        } else {
            self.issue("out_of_order_transition");
        }
    }
}
