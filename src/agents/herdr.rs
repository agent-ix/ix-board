// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Decoders for the `herdr` JSON the detector reads (FR-018).
//!
//! herdr is a separate tool whose output grows fields over time, so these
//! structs name only the fields the detector uses and ignore the rest.

use super::AgentsError;
use serde::{Deserialize, Serialize};

/// herdr's lifecycle classification of an agent pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HerdrState {
    /// The agent is mid-turn.
    Working,
    /// Ready for input, seen.
    Idle,
    /// Ready for input, finished since last viewed.
    Done,
    /// An approval or question dialog is showing.
    Blocked,
    /// herdr cannot classify the agent.
    Unknown,
}

impl HerdrState {
    fn from_wire(raw: &str) -> Self {
        match raw {
            "working" => Self::Working,
            "idle" => Self::Idle,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            _ => Self::Unknown,
        }
    }
}

/// One agent pane from `herdr agent list`.
#[derive(Clone, Debug, Deserialize)]
pub struct HerdrAgent {
    /// Harness kind reported by herdr, for example `claude` or `codex`.
    pub agent: String,
    #[serde(default)]
    agent_session: Option<AgentSession>,
    /// Raw status string exactly as herdr reported it.
    pub agent_status: String,
    /// Working directory of the pane.
    #[serde(default)]
    pub cwd: String,
    /// Pane identifier, unique within one herdr server.
    pub pane_id: String,
    /// Owning workspace identifier.
    #[serde(default)]
    pub workspace_id: String,
    /// Terminal title with status glyphs removed.
    #[serde(default)]
    pub terminal_title_stripped: String,
}

#[derive(Clone, Debug, Deserialize)]
struct AgentSession {
    value: String,
}

impl HerdrAgent {
    /// Session id herdr recorded for the agent, when it recorded one.
    pub fn session_id(&self) -> Option<&str> {
        self.agent_session
            .as_ref()
            .map(|session| session.value.as_str())
    }

    /// Classified lifecycle state.
    pub fn state(&self) -> HerdrState {
        HerdrState::from_wire(&self.agent_status)
    }
}

/// One workspace from `herdr workspace list`.
#[derive(Clone, Debug, Deserialize)]
pub struct HerdrWorkspace {
    /// Workspace identifier.
    pub workspace_id: String,
    /// Human label; this is the team name.
    pub label: String,
}

/// One saved machine from `herdr machine list --json`.
#[derive(Clone, Debug, Deserialize)]
pub struct HerdrMachine {
    /// Label accepted by `herdr --machine`.
    pub label: String,
    /// SSH target for the machine.
    pub target: String,
    /// Whether the profile is enabled.
    pub enabled: bool,
}

#[derive(Deserialize)]
struct Envelope<T> {
    result: T,
}

#[derive(Deserialize)]
struct AgentList {
    agents: Vec<HerdrAgent>,
}

#[derive(Deserialize)]
struct WorkspaceList {
    workspaces: Vec<HerdrWorkspace>,
}

fn decode<'a, T: Deserialize<'a>>(what: &'static str, text: &'a str) -> Result<T, AgentsError> {
    serde_json::from_str(text).map_err(|source| AgentsError::Decode { what, source })
}

/// Decodes `herdr agent list` output.
pub fn parse_agents(text: &str) -> Result<Vec<HerdrAgent>, AgentsError> {
    Ok(decode::<Envelope<AgentList>>("herdr agent list", text)?
        .result
        .agents)
}

/// Decodes `herdr workspace list` output.
pub fn parse_workspaces(text: &str) -> Result<Vec<HerdrWorkspace>, AgentsError> {
    Ok(
        decode::<Envelope<WorkspaceList>>("herdr workspace list", text)?
            .result
            .workspaces,
    )
}

/// Decodes `herdr machine list --json` output.
pub fn parse_machines(text: &str) -> Result<Vec<HerdrMachine>, AgentsError> {
    decode("herdr machine list", text)
}
