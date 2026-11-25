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
mod pr;
mod settings;

pub use live::{TickInputs, caller_from, commit_completing, run};
pub use plan::{Capacity, OpenAction, PlanInput, plan};
pub use settings::TickSettings;

use super::SourceError;
use ix_trace_rs::implements;
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
    /// Route a source defect to the orchestrator.
    FixSource,
    /// Route a DEAD lane to the orchestrator with its status evidence.
    RouteDeadLane,
    /// Route an IDLE lane whose assignment list is empty to the orchestrator.
    RouteIdleLane,
    /// Ask a project's planner to supply the ready-ticket buffer deficit.
    NeedReadyTickets,
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
            Self::RouteDeadLane => "route_dead_lane",
            Self::RouteIdleLane => "route_idle_lane",
            Self::NeedReadyTickets => "need_ready_tickets",
        }
    }

    /// Every kind, in declaration order.
    pub const ALL: [Self; 14] = [
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
        Self::RouteDeadLane,
        Self::RouteIdleLane,
        Self::NeedReadyTickets,
    ];

    /// The kind named in the ledger.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether a condition of this kind runs the in-fleet ladder.
    pub const fn has_ladder(self) -> bool {
        !matches!(
            self,
            Self::AddLane | Self::LadderRung | Self::FixSource | Self::NeedReadyTickets
        )
    }
}

/// A project's positive ready-buffer shortage and configured planner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NeedReadyTickets {
    /// Project identifier or name.
    pub project: String,
    /// Planner session address.
    pub planner: String,
    /// Additional ready tickets needed.
    pub need: u32,
    /// Ready, unassigned tickets now.
    pub ready: u32,
    /// Reachable lanes assigned to the project.
    pub lanes: u32,
}

/// Deficit rows from saturation, with completeness kept separate from positive known needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeedReadyTicketsRead {
    /// Positive project deficits computed from available hosts.
    pub projects: Vec<NeedReadyTickets>,
    /// False when any required host or queue input is unavailable.
    pub complete: bool,
    /// Unavailable source bases and reasons retained for operators.
    pub unavailable: Vec<String>,
}

/// Admission-blocked share observed for one host during a tick interval.
#[derive(Clone, Debug, PartialEq)]
pub struct CapacityThrottleSample {
    /// Host name.
    pub host: String,
    /// Relay time at the end of the observation interval.
    pub at_ms: i64,
    /// Observation interval length.
    pub interval_secs: u64,
    /// Share; absent when the metric or coverage is unavailable.
    pub share: Option<f64>,
    /// Covered fraction of the interval.
    pub coverage: Option<f64>,
    /// Reason when the share is unavailable.
    pub unavailable_reason: Option<String>,
}

/// A completed drain proven by current watcher state and a clear lane compile window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DrainDoneSample {
    /// Host name.
    pub host: String,
    /// Lane address.
    pub lane: String,
    /// Auditable source basis.
    pub source_basis: String,
}

/// Per-host output observed over the trailing hour, with nullable values retained.
#[derive(Clone, Debug, PartialEq)]
pub struct HostOutputSample {
    /// Host name.
    pub host: String,
    /// Relay time at the end of the observation window.
    pub at_ms: i64,
    /// Current lane count.
    pub lanes: Option<u32>,
    /// Tickets closed during the hour.
    pub tickets_closed: Option<u64>,
    /// Pull requests merged during the hour.
    pub prs_merged: Option<u64>,
    /// Commits pushed during the hour.
    pub commits_pushed: Option<u64>,
    /// Per-field source basis, encoded as JSON in the ledger.
    pub source_basis: String,
}

/// A live host reserve or thermal-limit breach observed during a tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverloadEventSample {
    /// Host name.
    pub host: String,
    /// `memory_below_reserve` or `thermal_abort`.
    pub cause: String,
    /// Source metric and configured limit basis.
    pub source_basis: String,
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

/// Why a source gave the tick nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Unread {
    /// No reader exists yet; the ticket that owns it is named. Not a defect.
    Unwired(&'static str),
    /// The read failed. A source defect: the tick raises it and routes it to the orchestrator.
    Failed(SourceError),
}

/// A source the tick could not read; its actions are not emitted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Unavailable {
    /// Source name.
    pub source: String,
    /// Why.
    pub reason: String,
    /// The reader is not built yet (a named seam), as opposed to a read that failed.
    pub unwired: bool,
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

/// A known hole in the live lane-to-PR mapping, retained with its original source basis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanePrUnavailable {
    /// Lane, when attribution reached a verified lane.
    pub lane: Option<String>,
    /// Ticket whose PR mapping could not be resolved.
    pub ticket: Option<String>,
    /// PR URL or identifier, when known.
    pub pr: Option<String>,
    /// Source that could not establish the mapping.
    pub source_basis: String,
    /// Why it could not establish the mapping.
    pub reason: String,
}

/// Live PR read, including partial rows and explicit source gaps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LanePrRead {
    /// PRs whose lane attribution and activity were verified.
    pub items: Vec<LanePr>,
    /// Unavailable mappings or activity reads.
    pub unavailable: Vec<LanePrUnavailable>,
    /// Earliest time represented by the paged event source, if supplied.
    pub available_from_ms: Option<i64>,
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
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, Unread>;
    /// Per host (lower case), the busy ratio from the watcher series.
    ///
    /// # Errors
    ///
    /// Why the series could not be read.
    fn host_busy(&self) -> Result<BTreeMap<String, f64>, Unread>;
    /// Lane-owned pull requests with their last activity.
    ///
    /// # Errors
    ///
    /// Why the pull requests could not be read.
    fn lane_prs(&self) -> Result<LanePrRead, Unread>;
    /// Ready work no lane holds.
    ///
    /// # Errors
    ///
    /// Why the ready queue could not be read.
    fn ready_work(&self) -> Result<ReadyWork, Unread>;
    /// Positive ready-buffer deficits by owning planner project.
    fn need_ready_tickets(&self) -> Result<NeedReadyTicketsRead, Unread> {
        Err(Unread::Unwired("SWM-217"))
    }
    /// Admission-blocked shares; missing coverage remains an unavailable row.
    fn capacity_throttle_samples(
        &self,
        _at_ms: i64,
        _interval_secs: u64,
    ) -> Vec<CapacityThrottleSample> {
        Vec::new()
    }
    /// Draining lanes with fresh non-working state and no compile increase.
    fn drain_done(&self) -> Result<Vec<DrainDoneSample>, Unread> {
        Ok(Vec::new())
    }
    /// Per-host, trailing-hour output samples.
    fn host_output_samples(&self, _at_ms: i64) -> Vec<HostOutputSample> {
        Vec::new()
    }
    /// Host reserve/thermal limit breaches.
    fn overload_events(&self, _at_ms: i64) -> Vec<OverloadEventSample> {
        Vec::new()
    }
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
    /// Known holes in live lane-to-PR attribution.
    pub pr_unavailable: Vec<LanePrUnavailable>,
    /// Start of the event source's retained history.
    pub pr_available_from_ms: Option<i64>,
    /// Ready, unassigned work.
    pub ready: Option<ReadyWork>,
    /// Ready-buffer deficits by project.
    pub need_ready_tickets: Option<Vec<NeedReadyTickets>>,
    /// True only when saturation inputs were complete for every current host.
    pub need_ready_tickets_complete: Option<bool>,
    /// One admission-blocked share observation per host and tick interval.
    pub capacity_throttle_samples: Vec<CapacityThrottleSample>,
    /// Drain completions proven by current watcher state and build evidence.
    pub drain_done: Vec<DrainDoneSample>,
    /// Source-backed host outputs over the trailing hour.
    pub host_output_samples: Vec<HostOutputSample>,
    /// Host limit breach events observed by live tick sources.
    pub overload_events: Vec<OverloadEventSample>,
    /// Sources that failed, with the reason.
    pub unavailable: Vec<Unavailable>,
}

impl TickFacts {
    /// Reads every source once; a failure is recorded, never silently an empty value.
    #[implements("FR-038-AC-24")]
    #[implements("FR-041-AC-24")]
    pub fn read(sources: &dyn TickSources, at_ms: i64, interval_secs: u64) -> Self {
        let mut facts = Self {
            capacity_throttle_samples: sources.capacity_throttle_samples(at_ms, interval_secs),
            ..Self::default()
        };
        let mut unavailable_rows = Vec::new();
        fn note(source: &str, error: Unread) -> Unavailable {
            let (reason, unwired) = match error {
                Unread::Unwired(ticket) => (format!("no reader yet; owned by {ticket}"), true),
                Unread::Failed(error) => (error.0, false),
            };
            Unavailable {
                source: source.to_owned(),
                reason,
                unwired,
            }
        }
        let drain_done = sources.drain_done();
        if let Err(error) = &drain_done {
            unavailable_rows.push(note("drain_done", error.clone()));
        }
        facts.drain_done = drain_done.unwrap_or_default();
        facts.host_output_samples = sources.host_output_samples(at_ms);
        facts.overload_events = sources.overload_events(at_ms);
        let assignments = sources.assignments();
        let busy = sources.host_busy();
        let prs = sources.lane_prs();
        let ready = sources.ready_work();
        let need_ready_tickets = sources.need_ready_tickets();
        let (a, b, r) = (
            assignments
                .map_err(|e| unavailable_rows.push(note("assignments", e)))
                .ok(),
            busy.map_err(|e| unavailable_rows.push(note("host_busy", e)))
                .ok(),
            ready
                .map_err(|e| unavailable_rows.push(note("ready_work", e)))
                .ok(),
        );
        let p = match prs {
            Ok(read) => {
                for unavailable in &read.unavailable {
                    unavailable_rows.push(Unavailable {
                        source: unavailable.source_basis.clone(),
                        reason: format!(
                            "lane={:?} ticket={:?} pr={:?}: {}",
                            unavailable.lane,
                            unavailable.ticket,
                            unavailable.pr,
                            unavailable.reason
                        ),
                        unwired: false,
                    });
                }
                facts.pr_unavailable = read.unavailable;
                facts.pr_available_from_ms = read.available_from_ms;
                Some(read.items)
            }
            Err(error) => {
                unavailable_rows.push(note("lane_prs", error));
                None
            }
        };
        facts.assignments = a;
        facts.host_busy = b;
        facts.prs = p;
        facts.ready = r;
        facts.need_ready_tickets = match need_ready_tickets {
            Ok(read) => {
                facts.need_ready_tickets_complete = Some(read.complete);
                unavailable_rows.extend(read.unavailable.into_iter().map(|reason| Unavailable {
                    source: "need_ready_tickets".to_owned(),
                    reason,
                    unwired: false,
                }));
                Some(read.projects)
            }
            Err(error) => {
                unavailable_rows.push(note("need_ready_tickets", error));
                None
            }
        };
        facts.unavailable.extend(unavailable_rows);
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

impl TickFacts {
    /// The source defects of the failed (not unwired) reads, for the ledger and `fix_source`.
    pub fn failed_defects(&self, run_ms: i64) -> Vec<super::Defect> {
        self.unavailable
            .iter()
            .filter(|u| !u.unwired)
            .map(|u| super::Defect {
                kind: super::DefectKind::UnreadableSource,
                source: format!("tick.{}", u.source),
                host: None,
                lane: None,
                reason: u.reason.clone(),
                observed_ms: run_ms,
                first_seen_ms: None,
            })
            .collect()
    }
}
