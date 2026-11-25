// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet assign`: send a lane its ordered tickets as a relay order, store the list through the
//! FR-072 assignment list, and report whether the lane took up the first ticket (FR-035).
//!
//! The command talks to the relay through [`AssignBus`], implemented for [`Relay`], so the
//! decision logic is exercised through relay-core's real paths. The result is written to the
//! fleet ledger (FR-039) through [`AssignLedger`] at the send and at the outcome; a run of
//! `fleet status` completes a row whose command was killed ([`decide`] is shared by both).

use super::focus::{self, FocusEvent};
use crate::model::{Issue, IssueBatch, WorkflowStateType};
use ix_trace_rs::implements;
use relay_core::assignment::{
    AssignWrite, AssignWriter, AssignedTicket, AssignmentList, MAX_TICKET_ID_BYTES, lane_address,
};
use relay_core::fleet::{FleetLane, fleet_registry};
use relay_core::receipts::{
    OrderListRequest, OrderPage, OrderSpec, RECEIPT_PAGE_CAP, ReceiptEvent, ReceiptOutcome,
    ReceiptRequest,
};
use relay_core::{ACTIVITY_MAX_EVENTS, AgentName, Message, Relay, Target, text};
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use thiserror::Error as ThisError;

/// A refusal or failure of `fleet assign`. The relay list is unchanged and, except for
/// [`AssignError::Relay`] after the send, no order was sent.
#[derive(Debug, ThisError)]
pub enum AssignError {
    /// No ticket was named, or one was named twice.
    #[error("assign needs at least one ticket and names each once: {0}")]
    Tickets(String),
    /// The ticket could not be read from the cache.
    #[error("ticket {0} is unreadable: not in the Linear cache read")]
    Unreadable(String),
    /// The ticket is finished.
    #[error("ticket {ticket} is {state} and cannot be assigned")]
    Closed {
        /// The ticket.
        ticket: String,
        /// Its state.
        state: String,
    },
    /// The caller does not hold the orchestrator role.
    #[error(
        "not_orchestrator: set `[assign] orchestrator = true` in the ix-relay config.toml to assign"
    )]
    NotOrchestrator,
    /// The caller is the target lane.
    #[error("own_lane: a lane does not write its own assignment list")]
    OwnLane,
    /// The lane is not a `server:workspace:tab` address.
    #[error("invalid_lane: expected server:workspace:tab, got {0}")]
    InvalidLane(String),
    /// The lane is in no registry.
    #[error("unknown_lane: {0} is not in the fleet registry")]
    UnknownLane(String),
    /// The lane's host lease has expired.
    #[error("host_registry_stale: the registry lease of the host of {0} has expired")]
    HostRegistryStale(String),
    /// The lane has no unique registered session to send to.
    #[error("no_recipient: {0}")]
    NoRecipient(String),
    /// Another lane already holds the ticket.
    #[error("ticket {ticket} is already assigned to lane {lane}")]
    AssignedElsewhere {
        /// The ticket.
        ticket: String,
        /// The lane holding it.
        lane: String,
    },
    /// The order text is over the relay's message bound or holds an unsafe character.
    #[error("order text refused: {0}")]
    Text(String),
    /// The order spec was refused (deadline range or expected ticket).
    #[error("order refused: {0}")]
    Order(String),
    /// A relay read failed before the send.
    #[error("relay: {0}")]
    Relay(String),
    /// The send failed; nothing was stored.
    #[error("send failed, list unchanged: {0}")]
    Send(String),
    /// The ledger refused the pending row.
    #[error("ledger: {0}")]
    Ledger(String),
}

impl AssignError {
    /// Whether the failure is operational (the relay, the send or the ledger) and not a refusal
    /// of the request.
    #[must_use]
    pub fn operational(&self) -> bool {
        matches!(self, Self::Relay(_) | Self::Send(_) | Self::Ledger(_))
    }
}

/// The result of one assignment (FR-035 behavior 6).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignResult {
    /// Sent and not yet decided.
    Pending,
    /// The lane's derived focus is the first ticket before the deadline.
    Confirmed,
    /// The session acted, but its focus is not the first ticket.
    ActedNotFocused,
    /// The deadline passed after a delivery with no action.
    AckExpired,
    /// The deadline passed and the order was never delivered.
    TimedOut,
}

impl AssignResult {
    /// The stable name printed and recorded.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirmed => "confirmed",
            Self::ActedNotFocused => "acted_not_focused",
            Self::AckExpired => "ack_expired",
            Self::TimedOut => "timed_out",
        }
    }

    /// The result named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Pending,
            Self::Confirmed,
            Self::ActedNotFocused,
            Self::AckExpired,
            Self::TimedOut,
        ]
        .into_iter()
        .find(|r| r.name() == name)
    }
}

/// A failure the command reports instead of dropping.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssignDefect {
    /// `ledger_unwritable` or `relay_read_errors`.
    pub kind: String,
    /// The cause.
    pub detail: String,
}

/// The assignment as reported, and the shape of an `assignment_history` row (FR-039).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AssignReport {
    /// Role authorized to send this order.
    pub sender_role: String,
    /// Relay identity that sent this order.
    pub sender_addr: String,
    /// The lane address as the registry spells it.
    pub lane: String,
    /// The assigned ticket ids in order.
    pub tickets: Vec<String>,
    /// The relay identity of the accepted order; absent before the relay accepts it.
    pub order_id: Option<String>,
    /// The first ticket, carried on the order.
    pub expect_ticket: String,
    /// Relay time the command began, Unix milliseconds.
    pub started_at_ms: i64,
    /// Relay time the order was queued, Unix milliseconds.
    pub sent_at_ms: Option<i64>,
    /// Relay time the acknowledgement deadline falls, Unix milliseconds.
    pub deadline_at_ms: Option<i64>,
    /// The result.
    pub result: AssignResult,
    /// Relay time of the result, Unix milliseconds.
    pub result_at_ms: Option<i64>,
    /// Whole seconds from the send to the first event whose derived focus is the first ticket;
    /// absent when unconfirmed.
    pub confirm_latency_s: Option<i64>,
    /// The derived focus at the deadline. Absent while the deadline has not passed (a lane
    /// confirmed early): the first run after the deadline fills it from the events up to the
    /// deadline.
    pub focus_at_deadline: Option<String>,
    /// The list version the write left.
    pub list_version: Option<u64>,
    /// The list the lane held before this assignment.
    pub previous_tickets: Vec<String>,
    /// Why the list write failed after the order was accepted.
    pub list_write_error: Option<String>,
    /// Failures that did not stop the command but make its result incomplete.
    pub defects: Vec<AssignDefect>,
}

impl AssignReport {
    /// Whether the command exits zero.
    #[must_use]
    pub fn confirmed(&self) -> bool {
        self.result == AssignResult::Confirmed && !self.operational_failure()
    }

    /// Whether the command failed after the relay accepted the order: the list was not stored,
    /// or the ledger could not take the result.
    #[must_use]
    pub fn operational_failure(&self) -> bool {
        self.list_write_error.is_some()
            || self.defects.iter().any(|d| d.kind == "ledger_unwritable")
    }

    fn defect(&mut self, kind: &str, detail: String) {
        match self.defects.iter_mut().find(|d| d.kind == kind) {
            Some(existing) => existing.detail = detail,
            None => self.defects.push(AssignDefect {
                kind: kind.to_owned(),
                detail,
            }),
        }
    }
}

/// The fleet ledger's `assignment_history` write. Each call upserts the row of this
/// assignment.
pub trait AssignLedger {
    /// Records the row as it stands.
    ///
    /// # Errors
    ///
    /// The ledger could not take the row.
    fn record(&mut self, row: &AssignReport) -> Result<(), String>;

    /// Withdraws the row of an assignment whose order was never accepted.
    ///
    /// # Errors
    ///
    /// The ledger could not remove the row.
    fn withdraw(&mut self) -> Result<(), String>;
}

/// A ledger that keeps nothing, for callers that only want the report.
pub struct NoLedger;

impl AssignLedger for NoLedger {
    fn record(&mut self, _row: &AssignReport) -> Result<(), String> {
        Ok(())
    }

    fn withdraw(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// What the registry holds, read at one relay time.
#[derive(Clone, Debug)]
pub struct RegistryView {
    /// Relay server time, Unix milliseconds.
    pub now_ms: i64,
    /// The fleet registry.
    pub lanes: Vec<FleetLane>,
    /// Registered sessions with a verified lane address: queue name and address.
    pub registrations: Vec<(AgentName, String)>,
}

/// The relay operations `fleet assign` uses.
pub trait AssignBus {
    /// The registry and the relay clock.
    ///
    /// # Errors
    ///
    /// The relay could not be read.
    fn registry(&mut self) -> Result<RegistryView, String>;
    /// Every lane's assignment list.
    ///
    /// # Errors
    ///
    /// The relay could not be read.
    fn assignments(&mut self) -> Result<Vec<AssignmentList>, String>;
    /// Sends `msg` as an order.
    ///
    /// # Errors
    ///
    /// The relay refused or could not take the order.
    fn send_order(&mut self, msg: &Message, spec: &OrderSpec) -> Result<Vec<AgentName>, String>;
    /// Receipts observed at or after `since_ms`.
    ///
    /// # Errors
    ///
    /// The receipts could not be read completely.
    fn receipts_since(&mut self, since_ms: i64) -> Result<Vec<ReceiptEvent>, String>;
    /// Orders for which the relay has recorded `AckExpired`, including the source deadline.
    fn expired_orders(&mut self) -> Result<relay_core::receipts::ExpiredOrders, String> {
        Ok(relay_core::receipts::ExpiredOrders::default())
    }
    /// Reads a bounded page of every retained order outcome.
    ///
    /// # Errors
    ///
    /// The relay page could not be read completely.
    fn orders(&mut self, request: &OrderListRequest) -> Result<OrderPage, String> {
        let _ = request;
        Err("relay all-orders source is unavailable".to_owned())
    }
    /// Returns the relay-time boundary confirmed by the operator, if present.
    fn orders_complete_since(&mut self) -> Result<Option<i64>, String> {
        Err("relay all-order coverage is not confirmed".to_owned())
    }
    /// Replaces the lane's list, carrying `order_id`.
    ///
    /// # Errors
    ///
    /// The relay refused the write.
    fn write_list(
        &mut self,
        writer: &AssignWriter,
        lane: &str,
        tickets: Vec<AssignedTicket>,
        order_id: &str,
    ) -> Result<AssignmentList, String>;
    /// The hook events of one lane.
    ///
    /// # Errors
    ///
    /// The activity stream could not be read.
    fn lane_events(&mut self, lane: &str) -> Result<Vec<FocusEvent>, String>;

    /// Hook events for several lanes from one source read when available.
    fn lane_events_for(
        &mut self,
        lanes: &[String],
    ) -> Result<BTreeMap<String, Vec<FocusEvent>>, String> {
        lanes
            .iter()
            .map(|lane| Ok((lane.to_lowercase(), self.lane_events(lane)?)))
            .collect()
    }
}

#[implements("FR-039-AC-12")]
fn require_complete_receipt_history(
    events: Vec<ReceiptEvent>,
    gaps: Vec<String>,
) -> Result<Vec<ReceiptEvent>, String> {
    if gaps.is_empty() {
        Ok(events)
    } else {
        Err(format!(
            "relay receipt history is incomplete: {}",
            gaps.join(", ")
        ))
    }
}

impl AssignBus for Relay {
    fn registry(&mut self) -> Result<RegistryView, String> {
        let lists = self.lane_lists().map_err(|e| e.to_string())?;
        let agents = self.agents().map_err(|e| e.to_string())?;
        let registrations: Vec<(AgentName, String)> = agents
            .registrations
            .iter()
            .filter_map(|r| {
                r.verified_place()
                    .map(|p| (r.name.clone(), p.address(&r.machine).to_string()))
            })
            .collect();
        let addresses: Vec<String> = registrations.iter().map(|r| r.1.clone()).collect();
        Ok(RegistryView {
            now_ms: lists.now_ms,
            lanes: fleet_registry(&lists.lists, &addresses, lists.now_ms),
            registrations,
        })
    }

    fn assignments(&mut self) -> Result<Vec<AssignmentList>, String> {
        Relay::assignments(self).map_err(|e| e.to_string())
    }

    fn send_order(&mut self, msg: &Message, spec: &OrderSpec) -> Result<Vec<AgentName>, String> {
        self.send_as(msg, Some(spec)).map_err(|e| e.to_string())
    }

    #[implements("FR-039-AC-12")]
    fn receipts_since(&mut self, since_ms: i64) -> Result<Vec<ReceiptEvent>, String> {
        let now_ms = self.lane_lists().map_err(|e| e.to_string())?.now_ms;
        let (events, gaps) = super::live::collect_receipts(|after| {
            self.receipt_export(&ReceiptRequest {
                since_ms,
                until_ms: now_ms,
                now_ms,
                page_size: RECEIPT_PAGE_CAP,
                after,
            })
            .map_err(|e| super::SourceError(format!("relay receipts: {e}")))
        })
        .map_err(|e| e.to_string())?;
        require_complete_receipt_history(events, gaps)
    }

    #[implements("FR-039-AC-12")]
    fn expired_orders(&mut self) -> Result<relay_core::receipts::ExpiredOrders, String> {
        Relay::expired_orders(self).map_err(|error| error.to_string())
    }

    #[implements("FR-039-AC-12")]
    fn orders(&mut self, request: &OrderListRequest) -> Result<OrderPage, String> {
        Relay::orders(self, request).map_err(|error| error.to_string())
    }

    #[implements("FR-039-AC-12")]
    fn orders_complete_since(&mut self) -> Result<Option<i64>, String> {
        Relay::orders_complete_since(self).map_err(|error| error.to_string())
    }

    fn write_list(
        &mut self,
        writer: &AssignWriter,
        lane: &str,
        tickets: Vec<AssignedTicket>,
        order_id: &str,
    ) -> Result<AssignmentList, String> {
        self.assign(writer, lane, &AssignWrite::Set(tickets), Some(order_id))
            .map_err(|e| e.to_string())
    }

    fn lane_events(&mut self, lane: &str) -> Result<Vec<FocusEvent>, String> {
        let stream = self
            .activity_events(ACTIVITY_MAX_EVENTS)
            .map_err(|e| e.to_string())?;
        Ok(stream
            .iter()
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).ok()?;
                value
                    .get("addr")?
                    .as_str()?
                    .eq_ignore_ascii_case(lane)
                    .then_some(())?;
                Some(FocusEvent {
                    server_ms: value.get("server_ms")?.as_i64()?,
                    cwd: value["cwd"].as_str().unwrap_or_default().to_owned(),
                    branch: value["branch"].as_str().unwrap_or_default().to_owned(),
                })
            })
            .collect())
    }

    fn lane_events_for(
        &mut self,
        lanes: &[String],
    ) -> Result<BTreeMap<String, Vec<FocusEvent>>, String> {
        let wanted = lanes
            .iter()
            .map(|lane| lane.to_lowercase())
            .collect::<std::collections::BTreeSet<_>>();
        let mut events = wanted
            .iter()
            .map(|lane| (lane.clone(), Vec::new()))
            .collect::<BTreeMap<_, _>>();
        for line in self
            .activity_events(ACTIVITY_MAX_EVENTS)
            .map_err(|error| error.to_string())?
        {
            let Some((lane, event)) = parse_focus_event(&line) else {
                continue;
            };
            if let Some(rows) = events.get_mut(&lane.to_lowercase()) {
                rows.push(event);
            }
        }
        Ok(events)
    }
}

fn parse_focus_event(line: &str) -> Option<(String, FocusEvent)> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    Some((
        value.get("addr")?.as_str()?.to_owned(),
        FocusEvent {
            server_ms: value.get("server_ms")?.as_i64()?,
            cwd: value["cwd"].as_str().unwrap_or_default().to_owned(),
            branch: value["branch"].as_str().unwrap_or_default().to_owned(),
        },
    ))
}

/// One `fleet assign` request.
#[derive(Clone, Debug)]
pub struct AssignRequest {
    /// The lane address `server:workspace:tab`.
    pub lane: String,
    /// The ticket identifiers in order.
    pub tickets: Vec<String>,
    /// The acknowledgement deadline.
    pub deadline: Duration,
    /// How often the wait reads the relay.
    pub poll: Duration,
    /// The focus window in milliseconds.
    pub focus_window_ms: i64,
    /// The configured team keys (FR-037); the keys of the assigned tickets are always added.
    pub team_keys: Vec<String>,
}

fn closed(issue: &Issue) -> bool {
    matches!(
        issue.state_type,
        WorkflowStateType::Completed | WorkflowStateType::Canceled | WorkflowStateType::Duplicate
    ) || ["done", "canceled", "cancelled", "merged"]
        .iter()
        .any(|s| issue.state.eq_ignore_ascii_case(s) || issue.state_name.eq_ignore_ascii_case(s))
}

fn validate_tickets<'a>(
    request: &AssignRequest,
    batch: &'a IssueBatch,
) -> Result<Vec<&'a Issue>, AssignError> {
    if request.tickets.is_empty() {
        return Err(AssignError::Tickets("none given".to_owned()));
    }
    let mut found = Vec::new();
    for (index, id) in request.tickets.iter().enumerate() {
        if request.tickets[..index]
            .iter()
            .any(|seen| seen.eq_ignore_ascii_case(id))
        {
            return Err(AssignError::Tickets(format!("{id} is named twice")));
        }
        let issue = batch
            .issues
            .iter()
            .find(|i| i.identifier.eq_ignore_ascii_case(id) || i.id.0.eq_ignore_ascii_case(id))
            .ok_or_else(|| AssignError::Unreadable(id.clone()))?;
        if closed(issue) {
            return Err(AssignError::Closed {
                ticket: issue.identifier.clone(),
                state: issue.state.clone(),
            });
        }
        found.push(issue);
    }
    Ok(found)
}

/// The order body: per ticket in order, the identifier, title, state and description exactly as
/// read (the description carries the acceptance criteria), the queue position, and the deadline.
#[must_use]
pub fn order_text(lane: &str, issues: &[&Issue], deadline: Duration) -> String {
    let total = issues.len();
    let mut out = format!(
        "ASSIGNMENT for lane {lane}: work these {total} ticket(s) in order, starting with {first}.\n\
         Acknowledge by acting within {secs}s of this order.\n",
        first = issues[0].identifier,
        secs = deadline.as_secs(),
    );
    for (index, issue) in issues.iter().enumerate() {
        out.push_str(&format!(
            "\n=== {pos}/{total} {id} ===\nTitle: {title}\nState: {state}\nDescription:\n{description}\n",
            pos = index + 1,
            id = issue.identifier,
            title = issue.title,
            state = issue.state,
            description = issue.description.as_deref().unwrap_or("(none)"),
        ));
    }
    out
}

fn same_lane(a: &str, b: &str) -> bool {
    match (lane_address(a), lane_address(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// The team keys a focus is read with: the configured keys and the keys of `tickets`.
#[must_use]
pub fn focus_keys(configured: &[String], tickets: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = configured
        .iter()
        .cloned()
        .chain(
            tickets
                .iter()
                .filter_map(|t| t.rsplit_once('-').map(|(key, _)| key.to_owned())),
        )
        .map(|key| key.to_ascii_uppercase())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// What the receipts show about one order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OrderEvidence {
    /// A `Delivered` receipt exists.
    pub delivered: bool,
    /// An `Acted` receipt exists.
    pub acted: bool,
    /// An `AckExpired` receipt exists.
    pub expired: bool,
}

impl OrderEvidence {
    /// Folds in the receipts of `order_id`.
    pub fn absorb(&mut self, receipts: &[ReceiptEvent], order_id: &str) {
        for event in receipts.iter().filter(|e| e.original_id == order_id) {
            match event.outcome {
                ReceiptOutcome::Delivered => self.delivered = true,
                ReceiptOutcome::Acted => self.acted = true,
                ReceiptOutcome::AckExpired => self.expired = true,
                _ => {}
            }
        }
    }
}

/// A decided assignment outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    /// The result; never `Pending`.
    pub result: AssignResult,
    /// Relay time of the result, Unix milliseconds.
    pub result_at_ms: i64,
    /// Whole seconds from the send to the confirming event.
    pub confirm_latency_s: Option<i64>,
    /// The focus at the deadline, once the deadline has passed.
    pub focus_at_deadline: Option<String>,
}

/// The inputs of [`decide`] besides the evidence.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Relay time the order was queued.
    pub sent_at_ms: i64,
    /// Relay time of the acknowledgement deadline.
    pub deadline_at_ms: i64,
    /// Relay time now.
    pub now_ms: i64,
    /// The focus window.
    pub focus_window_ms: i64,
}

/// Decides the outcome of an order from the lane's events and the order's receipts, or `None`
/// while it is open. `Confirmed` needs an event after `sent_at` and up to the deadline whose
/// derived focus (over the events after `sent_at` only) is `first`; after the deadline, or on an
/// `AckExpired` receipt, the result is `acted_not_focused` (acted), `ack_expired` (delivered) or
/// `timed_out`.
#[must_use]
pub fn decide(
    events: &[FocusEvent],
    evidence: OrderEvidence,
    timing: Timing,
    keys: &[String],
    first: &str,
) -> Option<Decision> {
    let Timing {
        sent_at_ms,
        deadline_at_ms,
        now_ms,
        focus_window_ms,
    } = timing;
    let past = now_ms >= deadline_at_ms;
    let at_deadline =
        || past.then(|| focus::derive(events, deadline_at_ms, focus_window_ms, None, keys).label());
    let mut moments: Vec<i64> = events
        .iter()
        .map(|e| e.server_ms)
        .filter(|ms| (sent_at_ms..=deadline_at_ms).contains(ms))
        .collect();
    moments.sort_unstable();
    let confirmed = moments.into_iter().find(|ms| {
        focus::derive(events, *ms, focus_window_ms, Some(sent_at_ms), keys).ticket() == Some(first)
    });
    if let Some(at) = confirmed {
        return Some(Decision {
            result: AssignResult::Confirmed,
            result_at_ms: at,
            confirm_latency_s: Some((at - sent_at_ms) / 1_000),
            focus_at_deadline: at_deadline(),
        });
    }
    if !(evidence.expired || past) {
        return None;
    }
    let result = if evidence.acted {
        AssignResult::ActedNotFocused
    } else if evidence.delivered {
        AssignResult::AckExpired
    } else {
        AssignResult::TimedOut
    };
    Some(Decision {
        result,
        result_at_ms: now_ms.min(deadline_at_ms.max(sent_at_ms)).max(sent_at_ms),
        confirm_latency_s: None,
        focus_at_deadline: Some(
            focus::derive(events, deadline_at_ms, focus_window_ms, None, keys).label(),
        ),
    })
}

/// Assigns `request.tickets` to the lane and waits for the outcome.
///
/// # Errors
///
/// A refusal before any send, or a failed send, as [`AssignError`]. A list-write failure after
/// an accepted send is an `Ok` report whose `list_write_error` is set.
#[implements("FR-039-AC-12")]
pub fn assign(
    bus: &mut impl AssignBus,
    ledger: &mut impl AssignLedger,
    request: &AssignRequest,
    batch: &IssueBatch,
    writer: &AssignWriter,
) -> Result<AssignReport, AssignError> {
    let relay = AssignError::Relay;
    let issues = validate_tickets(request, batch)?;
    let ids: Vec<String> = issues.iter().map(|i| i.identifier.clone()).collect();
    let first = ids[0].clone();
    if first.len() > MAX_TICKET_ID_BYTES {
        return Err(AssignError::Order(format!(
            "ticket id {first} is over {MAX_TICKET_ID_BYTES} bytes"
        )));
    }
    let deadline_ms = i64::try_from(request.deadline.as_millis()).unwrap_or(i64::MAX);
    let spec = OrderSpec::new(deadline_ms, Some(first.clone())).map_err(AssignError::Order)?;
    if !writer.orchestrator {
        return Err(AssignError::NotOrchestrator);
    }
    let lane = lane_address(&request.lane)
        .map_err(|_| AssignError::InvalidLane(request.lane.clone()))?
        .to_string();
    if writer
        .caller_lane
        .as_ref()
        .is_some_and(|own| same_lane(&own.to_string(), &lane))
    {
        return Err(AssignError::OwnLane);
    }
    let registry = bus.registry().map_err(relay)?;
    let entry = registry
        .lanes
        .iter()
        .find(|l| same_lane(&l.address, &lane))
        .ok_or_else(|| AssignError::UnknownLane(lane.clone()))?;
    if entry.stale {
        return Err(AssignError::HostRegistryStale(entry.address.clone()));
    }
    let lane = entry.address.clone();
    let recipients: Vec<&AgentName> = registry
        .registrations
        .iter()
        .filter(|(_, address)| same_lane(address, &lane))
        .map(|(name, _)| name)
        .collect();
    let [recipient] = recipients.as_slice() else {
        return Err(AssignError::NoRecipient(format!(
            "{lane} has {} registered sessions, expected one",
            recipients.len()
        )));
    };
    let recipient = (*recipient).clone();
    let held = bus.assignments().map_err(AssignError::Relay)?;
    let mut previous_tickets = Vec::new();
    for list in &held {
        if same_lane(&list.lane, &lane) {
            previous_tickets = list.tickets.iter().map(|t| t.id.clone()).collect();
            continue;
        }
        if let Some(ticket) = ids
            .iter()
            .find(|id| list.tickets.iter().any(|t| t.id.eq_ignore_ascii_case(id)))
        {
            return Err(AssignError::AssignedElsewhere {
                ticket: ticket.clone(),
                lane: list.lane.clone(),
            });
        }
    }
    let body = order_text(&lane, &issues, request.deadline);
    if let Some((offset, c)) = text::first_unsafe(&body) {
        return Err(AssignError::Text(format!(
            "ticket text holds U+{:04X} at byte {offset}; clean the ticket in Linear",
            u32::from(c)
        )));
    }
    if body.len() > relay_core::MAX_MESSAGE_BYTES {
        return Err(AssignError::Text(format!(
            "the exact ticket text is {} bytes, over the relay limit of {}; assign fewer tickets",
            body.len(),
            relay_core::MAX_MESSAGE_BYTES
        )));
    }
    let sender: AgentName = writer
        .caller_lane
        .as_ref()
        .map_or_else(|| "ix-board".parse(), |lane| lane.to_string().parse())
        .map_err(|e: relay_core::Error| AssignError::Order(e.to_string()))?;
    let before_ms = registry.now_ms;
    let mut report = AssignReport {
        sender_role: "orchestrator".to_owned(),
        sender_addr: sender.to_string(),
        lane: lane.clone(),
        tickets: ids.clone(),
        order_id: None,
        expect_ticket: first.clone(),
        started_at_ms: before_ms,
        sent_at_ms: None,
        deadline_at_ms: None,
        result: AssignResult::Pending,
        result_at_ms: None,
        confirm_latency_s: None,
        focus_at_deadline: None,
        list_version: None,
        previous_tickets,
        list_write_error: None,
        defects: Vec::new(),
    };
    ledger.record(&report).map_err(AssignError::Ledger)?;

    let message = Message {
        from: sender.clone(),
        to: Target::Agent(recipient.clone()),
        text: body,
        sent_at_ms: before_ms,
        forwarded: false,
    };
    if let Err(error) = bus.send_order(&message, &spec) {
        let withdrawn = ledger
            .withdraw()
            .err()
            .map_or_else(String::new, |e| format!("; ledger row not withdrawn: {e}"));
        return Err(AssignError::Send(format!("{error}{withdrawn}")));
    }

    let queued = bus
        .receipts_since(before_ms.saturating_sub(1_000))
        .map_err(AssignError::Relay)
        .and_then(|events| {
            events
                .into_iter()
                .rfind(|e| {
                    e.outcome == ReceiptOutcome::Queued
                        && e.sender == sender.as_str()
                        && e.target == recipient.as_str()
                        && e.observed_at_ms >= before_ms
                })
                .ok_or_else(|| {
                    AssignError::Relay("the accepted order has no Queued receipt".to_owned())
                })
        });
    let queued = match queued {
        Ok(event) => event,
        Err(error) => {
            report.list_write_error = Some(format!("order accepted but not identified: {error}"));
            record_after_send(ledger, &mut report);
            return Ok(report);
        }
    };
    let order_id = queued.original_id.clone();
    let sent_at = queued.observed_at_ms;
    let deadline_at = sent_at.saturating_add(deadline_ms);
    report.order_id = Some(order_id.clone());
    report.sent_at_ms = Some(sent_at);
    report.deadline_at_ms = Some(deadline_at);

    let refs = ids
        .iter()
        .map(|id| AssignedTicket {
            id: id.clone(),
            reference: id.clone(),
        })
        .collect();
    match bus.write_list(writer, &lane, refs, &order_id) {
        Ok(list) => report.list_version = Some(list.version),
        Err(error) => {
            report.list_write_error = Some(format!(
                "order {order_id} was accepted by the relay but the list write failed: {error}"
            ));
            record_after_send(ledger, &mut report);
            return Ok(report);
        }
    }
    record_after_send(ledger, &mut report);

    let keys = focus_keys(&request.team_keys, &ids);
    wait_for_outcome(bus, request, &mut report, &order_id, &keys);
    record_after_send(ledger, &mut report);
    Ok(report)
}

fn record_after_send(ledger: &mut impl AssignLedger, report: &mut AssignReport) {
    if let Err(error) = ledger.record(report) {
        report.defect("ledger_unwritable", error);
    }
}

/// Polls until the outcome is decided. A read that fails is retried on the next poll and is
/// reported as a `relay_read_errors` defect; the relay clock is carried forward locally while
/// it cannot be read, and the outcome is decided at the deadline from what was read.
fn wait_for_outcome(
    bus: &mut impl AssignBus,
    request: &AssignRequest,
    report: &mut AssignReport,
    order_id: &str,
    keys: &[String],
) {
    let (sent_at, deadline_at) = (
        report.sent_at_ms.unwrap_or_default(),
        report.deadline_at_ms.unwrap_or_default(),
    );
    let mut known_now = (sent_at, Instant::now());
    let mut evidence = OrderEvidence::default();
    let mut events: Vec<FocusEvent> = Vec::new();
    let mut errors = 0_u32;
    let mut last_error = String::new();
    let decision = loop {
        let mut failed = false;
        match bus.registry() {
            Ok(view) => known_now = (view.now_ms, Instant::now()),
            Err(error) => (failed, errors, last_error) = (true, errors + 1, error),
        }
        match bus.receipts_since(sent_at.saturating_sub(1_000)) {
            Ok(receipts) => evidence.absorb(&receipts, order_id),
            Err(error) => (failed, errors, last_error) = (true, errors + 1, error),
        }
        match bus.lane_events(&report.lane) {
            Ok(read) => events = read,
            Err(error) => (failed, errors, last_error) = (true, errors + 1, error),
        }
        let elapsed = i64::try_from(known_now.1.elapsed().as_millis()).unwrap_or(i64::MAX);
        let now_ms = known_now.0.saturating_add(elapsed);
        let timing = Timing {
            sent_at_ms: sent_at,
            deadline_at_ms: deadline_at,
            now_ms,
            focus_window_ms: request.focus_window_ms,
        };
        if let Some(decision) = decide(&events, evidence, timing, keys, &report.expect_ticket) {
            // A failed read before the deadline may hide the evidence that decides otherwise.
            if !failed || now_ms >= deadline_at {
                break decision;
            }
        }
        std::thread::sleep(request.poll);
    };
    if errors > 0 {
        report.defect(
            "relay_read_errors",
            format!("{errors} relay read(s) failed while waiting; last: {last_error}"),
        );
    }
    report.result = decision.result;
    report.result_at_ms = Some(decision.result_at_ms);
    report.confirm_latency_s = decision.confirm_latency_s;
    report.focus_at_deadline = decision.focus_at_deadline;
}

/// The plain-text rendering of a report.
#[must_use]
pub fn render_text(report: &AssignReport) -> String {
    let mut lines = vec![format!(
        "{} lane={} tickets={}",
        report.result.name(),
        report.lane,
        report.tickets.join(",")
    )];
    if let Some(id) = &report.order_id {
        lines.push(format!("order_id: {id}"));
    }
    lines.push(format!("expect_ticket: {}", report.expect_ticket));
    if let Some(focus) = &report.focus_at_deadline {
        lines.push(format!("focus_at_deadline: {focus}"));
    }
    if let Some(latency) = report.confirm_latency_s {
        lines.push(format!("confirm_latency_s: {latency}"));
    }
    if let Some(error) = &report.list_write_error {
        lines.push(format!("error: {error}"));
    }
    for defect in &report.defects {
        lines.push(format!("defect: {} {}", defect.kind, defect.detail));
    }
    lines.join("\n")
}

#[cfg(test)]
mod receipt_coverage_tests {
    use super::*;
    use ix_trace_rs::trace;
    use relay_core::receipts::ReceiptOutcome;

    #[trace("FR-039-AC-12")]
    #[test]
    fn receipt_history_with_any_coverage_gap_is_not_used_as_complete() {
        let event = ReceiptEvent {
            event_id: "event-1".to_owned(),
            original_id: "order-1".to_owned(),
            attempt_id: None,
            outcome: ReceiptOutcome::Queued,
            sender: "minder:ix-relay:main".to_owned(),
            target: "session/74657374/636f646572".to_owned(),
            event_time_ms: Some(10),
            observed_at_ms: 10,
            evidence: None,
            reason: None,
            session: None,
            expect_ticket: Some("SWM-1".to_owned()),
            delivered_at_ms: None,
        };
        assert_eq!(
            require_complete_receipt_history(vec![event.clone()], Vec::new()).unwrap(),
            vec![event.clone()]
        );
        let error = require_complete_receipt_history(vec![event], vec!["retention".to_owned()])
            .unwrap_err();
        assert!(error.contains("retention"));
    }
}
