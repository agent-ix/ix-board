// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The FR-034 class rules, evaluated in the specified order over one lane's evidence.

use serde::Serialize;
use std::time::Duration;

/// The kind of one hook activity event, ordered as a lane lives through them: among events of
/// the same millisecond the later kind wins.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    /// A session started.
    SessionStart,
    /// The user submitted a prompt.
    Prompt,
    /// A tool call is about to run.
    PreTool,
    /// A tool call finished.
    PostTool,
    /// The harness waits on a permission.
    Blocked,
    /// The harness reported an idle prompt.
    Idle,
    /// The turn ended.
    Stop,
    /// The session ended.
    SessionEnd,
}

impl HookKind {
    /// The kind named by the relay event, if known.
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "session_start" => Self::SessionStart,
            "prompt" => Self::Prompt,
            "pre_tool" => Self::PreTool,
            "post_tool" => Self::PostTool,
            "blocked" => Self::Blocked,
            "idle" => Self::Idle,
            "stop" => Self::Stop,
            "session_end" => Self::SessionEnd,
            _ => return None,
        })
    }
}

/// One hook event with its relay server time.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct HookEvent {
    /// Relay server time, Unix milliseconds.
    pub server_ms: i64,
    /// Event kind.
    pub kind: HookKind,
}

/// A weak signal source; a lane without hook evidence needs two distinct agreeing sources.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeakSource {
    /// The herdr terminal title says working.
    HerdrTitle,
    /// The pane text says working.
    PaneText,
    /// The agent's own report says working.
    SelfReport,
}

/// The thresholds one lane is judged with. The first three are read from the relay
/// (`ix.fleet.config`); `None` means the relay did not report it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Thresholds {
    /// Relay working window.
    pub working_window: Option<Duration>,
    /// Relay `max_tool_runtime`.
    pub max_tool_runtime: Option<Duration>,
    /// Relay `no_events_after`.
    pub no_events_after: Option<Duration>,
    /// ix-board `loop_stops`.
    pub loop_stops: u32,
    /// ix-board `dead_after`.
    pub dead_after: Duration,
}

/// The evidence for one lane.
#[derive(Clone, Debug)]
pub struct LaneInput {
    /// `server:workspace:tab`.
    pub addr: String,
    /// The relay marks the lane stale: its host registry lease expired.
    pub stale: bool,
    /// Hook events, sorted ascending.
    pub events: Vec<HookEvent>,
    /// Age of the relay registration, when the lane has one.
    pub registered_age: Option<Duration>,
    /// Whether a herdr pane hosts the lane, when known.
    pub has_pane: Option<bool>,
    /// The relay-resolved `ix_fleet_lane_blocked` reason, if the lane is blocked.
    pub blocked_reason: Option<String>,
    /// An `AckExpired` order to the lane: `true` when a `Delivered` receipt exists.
    pub expired_order: Option<bool>,
    /// Weak signals that say the lane is working.
    pub weak: Vec<WeakSource>,
}

/// One of the eight classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ClassKind {
    /// Hook activity inside the window.
    #[serde(rename = "WORKING")]
    Working,
    /// Nothing to report.
    #[serde(rename = "IDLE")]
    Idle,
    /// Waiting on a dialog, login, usage limit or question.
    #[serde(rename = "BLOCKED")]
    Blocked,
    /// Stopping repeatedly with no tool use.
    #[serde(rename = "LOOP")]
    Loop,
    /// An order went unacknowledged.
    #[serde(rename = "STALE-ORDER")]
    StaleOrder,
    /// A tool call ran past `max_tool_runtime`.
    #[serde(rename = "STUCK")]
    Stuck,
    /// The session ended or the lane vanished.
    #[serde(rename = "DEAD")]
    Dead,
    /// The lane's host registry lease expired.
    #[serde(rename = "UNREACHABLE")]
    Unreachable,
}

/// A class and its reason; only `BLOCKED` and the order and loop classes carry one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Class {
    /// The class.
    pub kind: ClassKind,
    /// The reason, such as `dialog` for `BLOCKED(dialog)`.
    pub reason: Option<String>,
}

impl Class {
    fn new(kind: ClassKind) -> Self {
        Self { kind, reason: None }
    }
    fn with(kind: ClassKind, reason: &str) -> Self {
        Self {
            kind,
            reason: Some(reason.to_owned()),
        }
    }
}

/// One evidence item behind a class.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Evidence {
    /// What was observed.
    pub item: String,
    /// When, in relay server milliseconds, if the item is timed.
    pub ts_ms: Option<i64>,
    /// Which source supplied it.
    pub source: String,
}

/// The class, its evidence and the age of the newest event.
#[derive(Clone, Debug)]
pub struct Classification {
    /// The class.
    pub class: Class,
    /// Evidence items used.
    pub evidence: Vec<Evidence>,
}

/// Age of `ts_ms` at `now_ms`, never negative.
pub(crate) fn age(now_ms: i64, ts_ms: i64) -> Duration {
    Duration::from_millis(u64::try_from(now_ms.saturating_sub(ts_ms)).unwrap_or(0))
}

fn newest(events: &[HookEvent], kind: HookKind) -> Option<HookEvent> {
    events
        .iter()
        .rev()
        .find(|event| event.kind == kind)
        .copied()
}

fn relay_event(kind: &str, ts_ms: i64) -> Evidence {
    Evidence {
        item: kind.to_owned(),
        ts_ms: Some(ts_ms),
        source: "relay.activity".to_owned(),
    }
}

/// The blocked reason a relay series reason maps to; `None` is not a blocked reason.
pub(crate) fn blocked_class(raw: &str) -> Option<&'static str> {
    match raw {
        "dialog" | "permission" => Some("dialog"),
        "question" | "queued_question" => Some("question"),
        "login" => Some("login"),
        "usage_limit" => Some("usage_limit"),
        _ => None,
    }
}

/// An open `pre_tool`: the lane's newest event. A later `post_tool`, `stop`, `idle`, `blocked`
/// or any other event ends it, as in the relay lane-state rule.
fn open_pre_tool(events: &[HookEvent]) -> Option<HookEvent> {
    events
        .last()
        .copied()
        .filter(|event| event.kind == HookKind::PreTool)
}

fn looping(events: &[HookEvent], th: &Thresholds, now_ms: i64) -> bool {
    let Some(window) = th.working_window else {
        return false;
    };
    let mut stops = 0u32;
    for event in events.iter().rev() {
        if age(now_ms, event.server_ms) > window {
            break;
        }
        match event.kind {
            HookKind::Stop => stops += 1,
            HookKind::PreTool => break,
            _ => {}
        }
    }
    stops >= th.loop_stops
}

/// Classifies one lane. The rules run in the FR-034 order and the first match wins.
pub fn classify(input: &LaneInput, th: &Thresholds, now_ms: i64) -> Classification {
    let done = |class: Class, evidence: Vec<Evidence>| Classification { class, evidence };
    let latest = input.events.last().copied();
    if input.stale {
        return done(
            Class::new(ClassKind::Unreachable),
            vec![Evidence {
                item: "host registry lease expired".to_owned(),
                ts_ms: None,
                source: "relay.registry".to_owned(),
            }],
        );
    }
    if let Some(event) = latest.filter(|event| event.kind == HookKind::SessionEnd) {
        return done(
            Class::new(ClassKind::Dead),
            vec![relay_event("session_end", event.server_ms)],
        );
    }
    if latest.is_none()
        && input.has_pane == Some(false)
        && input.registered_age.is_some_and(|a| a > th.dead_after)
    {
        return done(
            Class::new(ClassKind::Dead),
            vec![Evidence {
                item: "no event, no herdr pane, registration older than dead_after".to_owned(),
                ts_ms: None,
                source: "relay.registry".to_owned(),
            }],
        );
    }
    if let Some(reason) = input.blocked_reason.as_deref().and_then(blocked_class) {
        return done(
            Class::with(ClassKind::Blocked, reason),
            vec![Evidence {
                item: format!(
                    "ix_fleet_lane_blocked{{reason={}}}",
                    input.blocked_reason.as_deref().unwrap_or("")
                ),
                ts_ms: None,
                source: "prometheus".to_owned(),
            }],
        );
    }
    if let Some(delivered) = input.expired_order {
        let reason = if delivered {
            "ack_expired"
        } else {
            "timed_out"
        };
        return done(
            Class::with(ClassKind::StaleOrder, reason),
            vec![Evidence {
                item: format!("AckExpired receipt ({reason})"),
                ts_ms: None,
                source: "relay.receipts".to_owned(),
            }],
        );
    }
    let open = open_pre_tool(&input.events);
    if let (Some(pre), Some(limit)) = (open, th.max_tool_runtime)
        && age(now_ms, pre.server_ms) > limit
    {
        return done(
            Class::new(ClassKind::Stuck),
            vec![relay_event("pre_tool without post_tool", pre.server_ms)],
        );
    }
    if input.blocked_reason.as_deref() == Some("wait_loop") {
        return done(
            Class::with(ClassKind::Loop, "wait_loop"),
            vec![Evidence {
                item: "ix_fleet_lane_blocked{reason=wait_loop}".to_owned(),
                ts_ms: None,
                source: "prometheus".to_owned(),
            }],
        );
    }
    if looping(&input.events, th, now_ms) {
        let stops = input
            .events
            .iter()
            .rev()
            .take_while(|e| e.kind != HookKind::PreTool)
            .filter(|e| e.kind == HookKind::Stop)
            .map(|e| relay_event("stop", e.server_ms))
            .collect();
        return done(Class::with(ClassKind::Loop, "stops"), stops);
    }
    let mut evidence = Vec::new();
    if let Some(window) = th.working_window {
        for kind in [HookKind::Prompt, HookKind::PreTool, HookKind::PostTool] {
            if let Some(event) = newest(&input.events, kind)
                && age(now_ms, event.server_ms) <= window
            {
                evidence.push(relay_event(kind_name(kind), event.server_ms));
            }
        }
    }
    if let (Some(pre), Some(limit)) = (open, th.max_tool_runtime)
        && age(now_ms, pre.server_ms) <= limit
        && evidence.is_empty()
    {
        evidence.push(relay_event("pre_tool without post_tool", pre.server_ms));
    }
    if !evidence.is_empty() {
        return done(Class::new(ClassKind::Working), evidence);
    }
    let mut sources = input.weak.clone();
    sources.sort();
    sources.dedup();
    if sources.len() >= 2 {
        let evidence = sources
            .iter()
            .map(|source| Evidence {
                item: format!("weak signal {source:?}"),
                ts_ms: None,
                source: "herdr".to_owned(),
            })
            .collect();
        return done(Class::new(ClassKind::Working), evidence);
    }
    let evidence = latest
        .map(|event| vec![relay_event(kind_name(event.kind), event.server_ms)])
        .unwrap_or_default();
    done(Class::new(ClassKind::Idle), evidence)
}

fn kind_name(kind: HookKind) -> &'static str {
    match kind {
        HookKind::SessionStart => "session_start",
        HookKind::Prompt => "prompt",
        HookKind::PreTool => "pre_tool",
        HookKind::PostTool => "post_tool",
        HookKind::Blocked => "blocked",
        HookKind::Idle => "idle",
        HookKind::Stop => "stop",
        HookKind::SessionEnd => "session_end",
    }
}
