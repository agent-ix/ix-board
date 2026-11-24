// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet tick`: the deterministic action list for the minder and its in-fleet escalation
//! ladder (FR-038).
//!
//! [`plan`] is pure over the fleet status report, the facts read from other sources, the
//! capacity state and the open actions of the ledger. The ledger writer
//! ([`crate::fleet::ledger::Ledger::commit_tick`]) feeds it and stores what it returns.

mod live;
mod plan;
mod settings;

pub use live::run;
pub use plan::{Capacity, OpenAction, PlanInput, plan};
pub use settings::TickSettings;

use super::SourceError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Version of the JSON action-list schema.
pub const TICK_SCHEMA_VERSION: u32 = 1;

/// The kind of one action.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Send an idle lane the next ticket of its FR-072 list.
    PushNextTicket,
    /// Answer a dialog or queued question per a standing order.
    AnswerDialog,
    /// Route a block to the lane's lead.
    EscalateBlock,
    /// Route a stuck or looping lane to its lead.
    EscalateStuck,
    /// Re-send an order that expired unacted.
    ResendOrder,
    /// Add a lane to a host that is not busy enough.
    AddLane,
    /// Route a stale pull request to the lead of its lane.
    EscalateStalePr,
    /// Treat a lane with lane-scoped defective evidence as idle.
    ActOnUndetermined,
    /// One rung of the in-fleet escalation ladder.
    LadderRung,
    /// Route a lane that drains past the cap to its lead.
    EscalateDrain,
    /// Route a persistent source defect to the orchestrator.
    FixSource,
}

impl ActionKind {
    /// The name stored in the ledger and printed in JSON.
    pub const fn name(self) -> &'static str {
        match self {
            Self::PushNextTicket => "push_next_ticket",
            Self::AnswerDialog => "answer_dialog",
            Self::EscalateBlock => "escalate_block",
            Self::EscalateStuck => "escalate_stuck",
            Self::ResendOrder => "resend_order",
            Self::AddLane => "add_lane",
            Self::EscalateStalePr => "escalate_stale_pr",
            Self::ActOnUndetermined => "act_on_undetermined",
            Self::LadderRung => "ladder_rung",
            Self::EscalateDrain => "escalate_drain",
            Self::FixSource => "fix_source",
        }
    }

    /// The kind named in the ledger.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::PushNextTicket,
            Self::AnswerDialog,
            Self::EscalateBlock,
            Self::EscalateStuck,
            Self::ResendOrder,
            Self::AddLane,
            Self::EscalateStalePr,
            Self::ActOnUndetermined,
            Self::LadderRung,
            Self::EscalateDrain,
            Self::FixSource,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
    }

    /// Whether a condition of this kind runs the in-fleet ladder.
    pub const fn has_ladder(self) -> bool {
        !matches!(self, Self::AddLane | Self::LadderRung | Self::FixSource)
    }
}

/// How urgent an action is; the list sorts by it, high first.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Needs a person or lead.
    High,
    /// Needs attention within the tick.
    Medium,
    /// Routine.
    Low,
}

impl Severity {
    /// The name stored in the ledger.
    pub const fn name(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Who an action is addressed to. The tick never addresses the owner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Target {
    /// `lane`, `lead`, `orchestrator`, `planner` or `host`.
    pub role: &'static str,
    /// The recipient address, or the host name for `host`; empty when the role is not resolved.
    pub addr: Option<String>,
}

/// One action of the list.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Action {
    /// Ledger id, stable while the condition persists.
    pub id: i64,
    /// Kind.
    pub kind: ActionKind,
    /// The lane, host or defect the action is about.
    pub subject: String,
    /// Recipient.
    pub target: Target,
    /// Why, for example `idle_with_assignment` or `lead_unresolved`.
    pub reason: String,
    /// Severity.
    pub severity: Severity,
    /// Relay time by which the minder executes it, Unix milliseconds.
    pub due_by: i64,
    /// Relay time of the tick that first emitted the condition.
    pub condition_started_at: i64,
    /// Ladder rung 0 to 3; empty for a kind without a ladder.
    pub rung: Option<u8>,
    /// The condition's action id, on a rung 1 to 3.
    pub parent_action_id: Option<i64>,
    /// Evidence references.
    pub evidence: Vec<String>,
    /// Kind-specific fields.
    pub detail: serde_json::Value,
}

/// An add_lane suppression or a rung that cannot be addressed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Suppressed {
    /// What was not emitted.
    pub kind: ActionKind,
    /// Host or lane.
    pub subject: String,
    /// `discovery_active`, `no_recorded_level`, `at_recorded_level`, `cool_off` or
    /// `lead_unresolved`.
    pub reason: String,
}

/// A source the tick could not read; its actions are not emitted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Unavailable {
    /// Source name.
    pub source: String,
    /// Why.
    pub reason: String,
}

/// A lane-owned pull request and its last activity.
#[derive(Clone, Debug, PartialEq)]
pub struct LanePr {
    /// The lane address.
    pub lane: String,
    /// `owner/repo#N`.
    pub pr: String,
    /// Relay time of the last head or review activity.
    pub last_activity_ms: i64,
}

/// Ready, unassigned work: the total and the lanes an unowned ready ticket is eligible for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadyWork {
    /// Ready tickets no lane holds.
    pub unassigned: usize,
    /// Lanes (lower case) for which such a ticket exists in an eligible team or project.
    pub lanes: BTreeSet<String>,
}

/// The sources the tick reads besides `fleet status`.
pub trait TickSources {
    /// Per lane (lower case), the remaining tickets of its FR-072 list in order.
    ///
    /// # Errors
    ///
    /// Why the lists could not be read.
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, SourceError>;
    /// Per host (lower case), the busy ratio from the watcher series.
    ///
    /// # Errors
    ///
    /// Why the series could not be read.
    fn host_busy(&self) -> Result<BTreeMap<String, f64>, SourceError>;
    /// Lane-owned pull requests with their last activity.
    ///
    /// # Errors
    ///
    /// Why the pull requests could not be read.
    fn lane_prs(&self) -> Result<Vec<LanePr>, SourceError>;
    /// Ready work no lane holds.
    ///
    /// # Errors
    ///
    /// Why the ready queue could not be read.
    fn ready_work(&self) -> Result<ReadyWork, SourceError>;
}

/// What the tick read from [`TickSources`]; `None` is an unavailable source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickFacts {
    /// FR-072 lists by lane.
    pub assignments: Option<BTreeMap<String, Vec<String>>>,
    /// Host busy ratios.
    pub host_busy: Option<BTreeMap<String, f64>>,
    /// Lane-owned pull requests.
    pub prs: Option<Vec<LanePr>>,
    /// Ready, unassigned work.
    pub ready: Option<ReadyWork>,
    /// Sources that failed, with the reason.
    pub unavailable: Vec<Unavailable>,
}

impl TickFacts {
    /// Reads every source once; a failure is recorded, never silently an empty value.
    pub fn read(sources: &dyn TickSources) -> Self {
        let mut facts = Self::default();
        let mut note = |source: &str, error: SourceError| {
            facts.unavailable.push(Unavailable {
                source: source.to_owned(),
                reason: error.0,
            });
        };
        let assignments = sources.assignments();
        let busy = sources.host_busy();
        let prs = sources.lane_prs();
        let ready = sources.ready_work();
        let (a, b, p, r) = (
            assignments.map_err(|e| note("assignments", e)).ok(),
            busy.map_err(|e| note("host_busy", e)).ok(),
            prs.map_err(|e| note("lane_prs", e)).ok(),
            ready.map_err(|e| note("ready_work", e)).ok(),
        );
        facts.assignments = a;
        facts.host_busy = b;
        facts.prs = p;
        facts.ready = r;
        facts
    }
}

/// The SWM-226 standing-order file as the tick sees it.
///
/// The machine-readable order format and the match conditions are owned by SWM-226 and are not
/// specified yet. Until they are, a readable file means orders exist: the tick emits
/// `answer_dialog` carrying the relay reason and no order id, and the minder matches the order
/// from `standing-orders.md`. A missing or unreadable file never produces `answer_dialog`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StandingOrders {
    /// No file is configured or it does not exist.
    Missing,
    /// The file exists and cannot be read.
    Unreadable(String),
    /// The file is readable.
    Present,
}

impl StandingOrders {
    /// Reads the configured file's state.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::Missing;
        };
        match std::fs::read(path) {
            Ok(_) => Self::Present,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.to_string()),
        }
    }
}

/// The result of one committed tick.
#[derive(Clone, Debug, Serialize)]
pub struct TickOutput {
    /// JSON schema version.
    pub schema_version: u32,
    /// Ledger id of the tick.
    pub tick_id: i64,
    /// Relay time of the tick, Unix milliseconds.
    pub relay_ms: i64,
    /// `false` when `fleet status` found any defect.
    pub complete: bool,
    /// The actions in emission order: severity, subject, kind.
    pub actions: Vec<Action>,
    /// Actions not emitted, with the reason.
    pub suppressed: Vec<Suppressed>,
    /// Sources the tick could not read.
    pub unavailable: Vec<Unavailable>,
}

impl TickOutput {
    /// One line per action, then suppressions and unread sources.
    pub fn render_text(&self) -> String {
        use std::fmt::Write;
        let mut out = format!("tick {} at {}\n", self.tick_id, self.relay_ms);
        for a in &self.actions {
            let _ = writeln!(
                out,
                "action {} {} {} -> {}{} reason={} severity={} due_by={}{}",
                a.id,
                a.kind.name(),
                a.subject,
                a.target.role,
                a.target
                    .addr
                    .as_ref()
                    .map_or_else(String::new, |t| format!(":{t}")),
                a.reason,
                a.severity.name(),
                a.due_by,
                a.rung.map_or_else(String::new, |r| format!(" rung={r}")),
            );
        }
        for s in &self.suppressed {
            let _ = writeln!(
                out,
                "suppressed {} {} reason={}",
                s.kind.name(),
                s.subject,
                s.reason
            );
        }
        for u in &self.unavailable {
            let _ = writeln!(out, "unavailable {}: {}", u.source, u.reason);
        }
        let _ = write!(out, "complete: {}", self.complete);
        out
    }
}
