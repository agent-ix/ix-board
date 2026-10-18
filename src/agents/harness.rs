// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Harness adapters: how to read each coding agent's transcript (FR-018).
//!
//! The probe script locates the files; an adapter turns the transcript tail
//! into the last assistant message. Subagent liveness comes from the child
//! transcript modification times the probe reports for the harness.

use serde::Deserialize;

/// Coding-agent harnesses with a transcript adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessKind {
    /// Claude Code.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
}

impl HarnessKind {
    /// Name used in the probe request.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

/// Reads one harness's transcript format.
pub trait Harness: Sync {
    /// Which harness this adapter serves.
    fn kind(&self) -> HarnessKind;

    /// Text of the last assistant message in a transcript tail. The first
    /// line of a tail may be cut mid-record and is skipped when unparsable.
    fn last_assistant_message(&self, tail: &str) -> Option<String>;
}

static CLAUDE: ClaudeHarness = ClaudeHarness;
static CODEX: CodexHarness = CodexHarness;

/// Adapter for the harness herdr names in its `agent` field, if any.
pub fn adapter_for(herdr_agent: &str) -> Option<&'static dyn Harness> {
    match herdr_agent {
        "claude" => Some(&CLAUDE),
        "codex" => Some(&CODEX),
        _ => None,
    }
}

struct ClaudeHarness;
struct CodexHarness;

#[derive(Deserialize)]
struct ClaudeLine {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    message: Option<ClaudeMessage>,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    content: ClaudeContent,
}

#[derive(Deserialize, Default)]
#[serde(untagged)]
enum ClaudeContent {
    Text(String),
    Blocks(Vec<ClaudeBlock>),
    #[default]
    Other,
}

#[derive(Deserialize)]
struct ClaudeBlock {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    text: String,
}

impl Harness for ClaudeHarness {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Claude
    }

    fn last_assistant_message(&self, tail: &str) -> Option<String> {
        tail.lines().rev().find_map(|line| {
            let line: ClaudeLine = serde_json::from_str(line).ok()?;
            if line.kind != "assistant" {
                return None;
            }
            let text = match line.message?.content {
                ClaudeContent::Text(text) => text,
                ClaudeContent::Blocks(blocks) => blocks
                    .into_iter()
                    .filter(|block| block.kind == "text")
                    .map(|block| block.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                ClaudeContent::Other => return None,
            };
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        })
    }
}

#[derive(Deserialize)]
struct CodexLine {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    last_agent_message: Option<String>,
    #[serde(default)]
    content: Vec<CodexBlock>,
}

#[derive(Deserialize)]
struct CodexBlock {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    text: String,
}

impl Harness for CodexHarness {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Codex
    }

    fn last_assistant_message(&self, tail: &str) -> Option<String> {
        tail.lines().rev().find_map(|line| {
            let line: CodexLine = serde_json::from_str(line).ok()?;
            let payload = line.payload?;
            let text = match (line.kind.as_str(), payload.kind.as_str()) {
                ("event_msg", "task_complete") => payload.last_agent_message?,
                ("response_item", "message") if payload.role == "assistant" => payload
                    .content
                    .into_iter()
                    .filter(|block| block.kind == "output_text")
                    .map(|block| block.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => return None,
            };
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        })
    }
}
