// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Streaming harness adapters. Only bounded lifecycle facts survive a record.

use super::lifecycle::{AgentId, EvidenceSource, Lifecycle, PendingCall, WorkerRole, WorkerState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Coding-agent harnesses with a transcript adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind {
    /// Claude Code.
    Claude,
    /// Codex CLI.
    Codex,
    /// An injected adapter.
    Other,
}
impl HarnessKind {
    /// Name reported in evidence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Other => "other",
        }
    }
}

/// Translates structured records into the shared lifecycle contract.
pub trait Harness: Sync {
    /// Harness represented by this adapter.
    fn kind(&self) -> HarnessKind;
    /// Applies one record without retaining prompts, instructions or tool result text.
    fn observe_record(&self, record: &Value, state: &mut Lifecycle);
    /// Evidence provenance.
    fn evidence_source(&self) -> EvidenceSource;
    /// Extracts the latest assistant message from a bounded transcript sample.
    fn last_assistant_message(&self, tail: &str) -> Option<String> {
        let mut state = Lifecycle::default();
        for line in tail.lines() {
            if let Ok(record) = serde_json::from_str(line) {
                self.observe_record(&record, &mut state);
            }
        }
        state.assistant
    }
}
struct Claude;
struct Codex;
static CLAUDE: Claude = Claude;
static CODEX: Codex = Codex;
/// Registered filesystem adapter, if this harness is supported.
pub fn adapter_for(name: &str) -> Option<&'static dyn Harness> {
    match name {
        "claude" => Some(&CLAUDE),
        "codex" => Some(&CODEX),
        _ => None,
    }
}

pub(super) fn record_time(record: &Value) -> Option<u64> {
    u64::try_from(
        chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
            .ok()?
            .timestamp(),
    )
    .ok()
}
fn activity(state: &mut Lifecycle, at: Option<u64>) {
    state.last_activity_at = state.last_activity_at.into_iter().chain(at).max();
}
fn text_blocks(value: &Value, kind: &str) -> String {
    if let Some(text) = value.as_str() {
        return text.chars().take(4096).collect();
    }
    let mut text = String::new();
    for block in value.as_array().into_iter().flatten() {
        if block["type"] == kind
            && let Some(part) = block["text"].as_str()
        {
            let remaining = 4096usize.saturating_sub(text.chars().count());
            text.extend(part.chars().take(remaining));
        }
    }
    text
}
fn assistant(state: &mut Lifecycle, text: String, at: Option<u64>) {
    if !text.trim().is_empty() {
        state.assistant = Some(text);
        state.assistant_at = at;
    }
}
fn clear_assistant(state: &mut Lifecycle) {
    state.assistant = None;
    state.assistant_at = None;
}
fn insert_child(state: &mut Lifecycle, id: &str) {
    if id.len() > 512 || state.expected_children.len() >= 1024 {
        state.issue("child_state_limit");
    } else {
        state.expected_children.insert(id.into());
    }
}

impl Harness for Claude {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Claude
    }
    fn evidence_source(&self) -> EvidenceSource {
        EvidenceSource::Claude
    }
    fn observe_record(&self, record: &Value, state: &mut Lifecycle) {
        let at = record_time(record);
        if let Some(id) = bounded_string(&record["agentId"]) {
            if state
                .record_agent_id
                .as_ref()
                .is_some_and(|old| old.0 != id)
            {
                state.issue("child_record_identity_conflict");
            }
            state.record_agent_id = Some(AgentId(id));
        }
        if let Some(id) = bounded_string(&record["sessionId"]) {
            if state
                .root_session_id
                .as_ref()
                .is_some_and(|old| old.0 != id)
            {
                state.issue("child_root_identity_conflict");
            }
            state.root_session_id = Some(AgentId(id));
        }

        match record["type"].as_str() {
            Some("user") if record["message"]["content"].is_string() => {
                let text = record["message"]["content"].as_str().unwrap_or_default();
                if text.starts_with("[Request interrupted by user") {
                    state.transition(WorkerState::Stopped, at);
                    activity(state, at);
                } else if !text.contains("<agent-message") && !text.contains("<task-notification") {
                    state.assignment_starts = state.assignment_starts.saturating_add(1);
                    state.transition(WorkerState::Running, at);
                    activity(state, at);
                    clear_assistant(state);
                }
            }
            Some("user") => {
                // Structured launch receipts are authoritative; arbitrary tool text is not.
                let receipt = &record["toolUseResult"];
                if receipt["status"] == "async_launched"
                    && let Some(id) = receipt["agentId"].as_str()
                {
                    insert_child(state, id);
                }
            }
            Some("assistant") => {
                activity(state, at);
                assistant(
                    state,
                    text_blocks(&record["message"]["content"], "text"),
                    at,
                );
                for block in record["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if block["type"] == "tool_use" {
                        clear_assistant(state);
                        if block["name"] == "TaskOutput" {
                            state.transition(WorkerState::Waiting, at);
                        }
                    }
                }
            }
            Some("system") if record["subtype"] == "turn_duration" => {
                // A lead turn ended, but this does not complete a worker assignment.
                activity(state, at);
                state.execution_ended_at = at;
            }
            _ => {}
        }
    }
}

impl Harness for Codex {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Codex
    }
    fn evidence_source(&self) -> EvidenceSource {
        EvidenceSource::Codex
    }
    fn observe_record(&self, record: &Value, state: &mut Lifecycle) {
        let at = record_time(record);
        let p = &record["payload"];
        match (record["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => header(p, state),
            (Some("event_msg"), Some("task_started" | "turn_started")) => {
                if matches!(state.state, WorkerState::Completed | WorkerState::Stopped)
                    && state
                        .generation
                        .as_deref()
                        .zip(p["turn_id"].as_str())
                        .is_some_and(|(a, b)| a == b)
                {
                    state.issue("reused_terminal_generation");
                    return;
                }
                clear_assistant(state);
                activity(state, at);
                state.generation = bounded_string(&p["turn_id"]);
                if state.generation.is_none() {
                    state.issue("assignment_generation_missing");
                }
                state.transition(WorkerState::Running, at);
            }
            (
                Some("event_msg"),
                Some("task_complete" | "turn_complete" | "turn_aborted" | "task_cancelled"),
            ) => {
                if state
                    .generation
                    .as_deref()
                    .zip(p["turn_id"].as_str())
                    .is_some_and(|(a, b)| a != b)
                {
                    state.issue("generation_conflict");
                    return;
                }
                activity(state, at);
                let stopped = matches!(p["type"].as_str(), Some("turn_aborted" | "task_cancelled"));
                state.transition(
                    if stopped {
                        WorkerState::Stopped
                    } else {
                        WorkerState::Completed
                    },
                    at,
                );
                if let Some(text) = p["last_agent_message"].as_str() {
                    assistant(state, text.chars().take(4096).collect(), at);
                }
            }
            (Some("event_msg"), Some("collab_waiting_begin")) => {
                activity(state, at);
                state.transition(WorkerState::Waiting, at);
            }
            (Some("event_msg"), Some("collab_waiting_end")) => {
                activity(state, at);
                state.transition(WorkerState::Running, at);
            }
            (Some("response_item"), Some("message")) if p["role"] == "assistant" => {
                activity(state, at);
                assistant(state, text_blocks(&p["content"], "output_text"), at);
            }
            (Some("response_item"), Some("function_call")) => call(p, state, at),
            (Some("response_item"), Some("function_call_output")) => result(p, state, at),
            (Some("event_msg"), Some("item_completed")) => {
                let item = &p["item"];
                if item["type"] == "SubAgentActivity"
                    && item["kind"] == "interacted"
                    && let Some(id) = item["id"].as_str()
                    && state
                        .calls
                        .get(id)
                        .is_some_and(|call| call.name == "followup_task")
                {
                    if let Some(id) = item["agent_thread_id"].as_str() {
                        insert_child(state, id);
                    }
                    state.calls.remove(id);
                    activity(state, at);
                    // Interaction alone does not reopen the child's assignment.
                }
            }
            (Some("event_msg"), Some(kind))
                if kind.starts_with("task_") || kind.starts_with("turn_") =>
            {
                state.issue("unsupported_lifecycle_record")
            }
            _ => {}
        }
    }
}
fn bounded_string(v: &Value) -> Option<String> {
    v.as_str().filter(|s| s.len() <= 512).map(str::to_owned)
}
fn header(p: &Value, state: &mut Lifecycle) {
    let nested = &p["source"]["subagent"]["thread_spawn"];
    let a = p["parent_thread_id"].as_str();
    let b = nested["parent_thread_id"].as_str();
    state.declared_depth = nested["depth"].as_u64();
    if a.zip(b).is_some_and(|(a, b)| a != b) {
        state.issue("parent_identity_conflict");
    }
    state.identity = bounded_string(&p["id"]).map(|id| {
        (
            AgentId(id),
            a.or(b)
                .filter(|s| s.len() <= 512)
                .map(|id| AgentId(id.into())),
        )
    });
    if state.identity.is_none() {
        state.issue("thread_identity_missing");
    }
    state.agent_path =
        bounded_string(&p["agent_path"]).or_else(|| bounded_string(&nested["agent_path"]));
    if state.agent_path.is_none() && a.or(b).is_none() {
        state.agent_path = Some("/root".into());
    }
    let source = p["source"].as_str().or_else(|| p["thread_source"].as_str());
    state.role = if matches!(
        source,
        Some("guardian_review" | "review" | "memory_consolidation")
    ) || p["source"]["subagent"].get("guardian_review").is_some()
        || matches!(
            p["source"]["subagent"].as_str(),
            Some("guardian_review" | "memory_consolidation" | "review")
        ) {
        WorkerRole::Internal
    } else if nested.is_object() || matches!(source, Some("cli" | "tui" | "exec" | "subagent")) {
        WorkerRole::Worker
    } else {
        WorkerRole::Unknown
    };
}
fn call(p: &Value, state: &mut Lifecycle, at: Option<u64>) {
    activity(state, at);
    clear_assistant(state);
    let Some(name) = p["name"].as_str().and_then(|s| s.rsplit('.').next()) else {
        return;
    };
    if !matches!(
        name,
        "list_agents" | "spawn_agent" | "followup_task" | "wait_agent" | "wait"
    ) {
        return;
    }
    if matches!(name, "wait_agent" | "wait") {
        state.transition(WorkerState::Waiting, at);
    }
    let Some(id) = bounded_string(&p["call_id"]) else {
        state.issue("call_identity_missing");
        return;
    };
    if state.calls.len() >= 1024 {
        state.issue("pending_call_limit");
        return;
    }
    let target = p["arguments"]
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| bounded_string(&v["target"]));
    state.calls.insert(
        id,
        PendingCall {
            name: name.into(),
            target,
        },
    );
}
fn result(p: &Value, state: &mut Lifecycle, at: Option<u64>) {
    let Some(call) = p["call_id"].as_str().and_then(|id| state.calls.remove(id)) else {
        return;
    };
    if call.name == "followup_task" {
        // Real successful follow-ups have empty output. A child task_started
        // with a new generation establishes execution; the output cannot.
        if p["output"] != "" {
            state.issue("unsupported_followup_result");
        }
        return;
    }
    let Some(value) = p["output"]
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
    else {
        state.issue("unsupported_lifecycle_tool_result");
        return;
    };
    match call.name.as_str() {
        "spawn_agent" => {
            if let Some(path) = value["task_name"].as_str() {
                insert_child(state, path);
            } else {
                state.issue("unsupported_spawn_result");
            }
        }
        "list_agents" => {
            let Some(rows) = value["agents"].as_array() else {
                state.issue("unsupported_child_snapshot");
                return;
            };
            for row in rows.iter().take(1025) {
                if state.child_states.len() >= 1024 {
                    state.issue("child_state_limit");
                    break;
                }
                let Some(name) = bounded_string(&row["agent_name"]) else {
                    state.issue("snapshot_identity_missing");
                    continue;
                };
                if Some(name.as_str()) == state.agent_path.as_deref() {
                    continue;
                }
                if state.agent_path.as_ref().is_some_and(|owner| {
                    !name
                        .strip_prefix(owner)
                        .is_some_and(|rest| rest.starts_with('/'))
                }) {
                    continue;
                }
                let status = &row["agent_status"];
                let status = if status.is_object() && status.get("completed").is_some() {
                    Some("completed")
                } else {
                    status.as_str()
                };
                let status = match status {
                    Some("running") => WorkerState::Running,
                    Some("waiting") => WorkerState::Waiting,
                    Some("idle") => WorkerState::Idle,
                    Some("completed") => WorkerState::Completed,
                    Some("interrupted" | "stopped") => WorkerState::Stopped,
                    _ => WorkerState::Unknown,
                };
                insert_child(state, &name);
                if let Some(at) = at {
                    if let Some((owner, _)) = &state.identity {
                        state.child_owners.insert(name.clone(), owner.clone());
                    }
                    state.child_states.insert(name, (at, status));
                } else {
                    state.issue("snapshot_timestamp_missing");
                }
            }
        }
        _ => {}
    }
}
