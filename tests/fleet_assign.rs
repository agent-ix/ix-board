// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet assign` through relay-core's real paths against a throwaway Redis (`docker`, redis:7),
//! plus a fake bus for the registry refusals the relay cannot be made to produce quickly.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ix_board::fleet::FleetConfig;
use ix_board::fleet::assign::{
    AssignBus, AssignError, AssignLedger, AssignReport, AssignRequest, AssignResult, NoLedger,
    OrderEvidence, RegistryView, Timing, assign, decide, focus_keys,
};
use ix_board::fleet::focus::FocusEvent;
use ix_board::fleet::ledger::{AssignSink, complete_pending, record_status_completing};
use ix_board::fleet::live::{NoWeakSignals, ReadReceipts};
use ix_board::fleet::{FleetReport, ReceiptRead, RelaySnapshot, Sources, build_report};
use ix_board::{Issue, IssueBatch, IssueId, ProjectId, TeamKey, WorkflowStateType};
use ix_trace_rs::trace;
use relay_core::assignment::{AssignWrite, AssignWriter, AssignedTicket, AssignmentList};
use relay_core::fleet::FleetLane;
use relay_core::receipts::{DeliveryEvidence, OrderSpec, ReceiptEvent};
use relay_core::{
    ActivityEvent, ActivityKind, AgentName, Claim, Harness, HerdrPlaceIds, Message, Now, Place,
    Popper, Registration, Relay,
};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

struct Container(String);

impl Container {
    fn start_uninstrumented() -> (Self, u16) {
        let out = Command::new("docker")
            .args(["run", "-d", "--rm", "-p", "127.0.0.1::6379", "redis:7"])
            .output()
            .expect("docker");
        assert!(out.status.success(), "docker run failed");
        let me = Self(String::from_utf8(out.stdout).unwrap().trim().to_owned());
        let port = Command::new("docker")
            .args(["port", &me.0, "6379/tcp"])
            .output()
            .unwrap();
        let text = String::from_utf8(port.stdout).unwrap();
        let port = text
            .lines()
            .next()
            .and_then(|l| l.rsplit(':').next())
            .and_then(|p| p.trim().parse().ok())
            .expect("mapped port");
        (me, port)
    }

    fn start() -> (Self, u16) {
        let (me, port) = Self::start_uninstrumented();
        // The assignment reader requires complete history from one second before
        // its first registry observation. Instrument this fresh relay before that
        // interval using the real producer, rather than fabricating coverage.
        let mut relay = connect(port);
        let bootstrap = lane(&mut relay, "receipt-bootstrap", "receipt-bootstrap");
        relay
            .send_as(
                &Message {
                    from: "fixture".parse().unwrap(),
                    to: relay_core::Target::Agent(bootstrap.name.clone()),
                    text: "initialize receipt history".into(),
                    sent_at_ms: 1,
                    forwarded: false,
                },
                None,
            )
            .unwrap();
        deliver(&mut relay, &bootstrap.name);
        let now = relay.lane_lists().unwrap().now_ms;
        wait_past(&mut relay, now + 1_000);
        (me, port)
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

fn connect(port: u16) -> Relay {
    let url = format!("redis://127.0.0.1:{port}");
    for _ in 0..50 {
        if let Ok(mut relay) = Relay::connect(&url, Duration::from_secs(5))
            && relay.lane_lists().is_ok()
        {
            return relay.with_prefix("t:".to_owned());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("redis did not come up");
}

struct Lane {
    name: AgentName,
    addr: String,
    session: String,
}

fn lane(relay: &mut Relay, session: &str, tab: &str) -> Lane {
    let machine = "terra".parse().unwrap();
    let session_id: relay_core::SessionId = session.parse().unwrap();
    let name = AgentName::for_session(&machine, &session_id);
    let reg = Registration {
        name: name.clone(),
        machine: machine.clone(),
        kind: "claude".into(),
        session_id: Some(session_id.clone()),
        cwd: None,
        herdr_pane: Some("w1:p1".parse().unwrap()),
        place: Some(Place {
            workspace: "SWM".into(),
            tab: tab.into(),
            herdr_ids: Some(HerdrPlaceIds {
                workspace: "w1".parse().unwrap(),
                tab: "w1:t1".parse().unwrap(),
                pane: "w1:p1".parse().unwrap(),
                session_id,
                verified_at_ms: 1,
            }),
        }),
        last_seen_ms: 1,
    };
    relay.register(&reg).unwrap();
    let addr = reg.verified_place().unwrap().address(&machine).to_string();
    Lane {
        name,
        addr,
        session: session.to_owned(),
    }
}

fn issue(id: &str, state: &str, kind: WorkflowStateType, description: &str) -> Issue {
    Issue {
        id: IssueId(format!("uuid-{id}")),
        identifier: id.into(),
        title: format!("Title of {id}"),
        description: Some(description.into()),
        state: state.into(),
        state_name: state.into(),
        state_type: kind,
        parent_id: None,
        team_key: TeamKey("SWM".into()),
        assignee_id: None,
        assignee_name: None,
        project_id: Some(ProjectId("p".into())),
        project_milestone_id: None,
        priority: 2.0,
        sort_order: 0.0,
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
        attachment_urls: vec![],
        attachments_complete: true,
        started_at: None,
        completed_at: None,
        pull_requests: Vec::new(),
    }
}

fn open(id: &str) -> Issue {
    issue(
        id,
        "Coding",
        WorkflowStateType::Started,
        &format!("Do {id}.\n\nAcceptance criteria:\n- [ ] first\n- [ ] second"),
    )
}

fn batch(issues: Vec<Issue>) -> IssueBatch {
    IssueBatch {
        issues,
        missing: vec![],
        complete: true,
    }
}

fn orchestrator() -> AssignWriter {
    AssignWriter {
        name: "orchestrator@terra".into(),
        orchestrator: true,
        caller_lane: None,
    }
}

fn request(lane: &Lane, tickets: &[&str], deadline_secs: u64) -> AssignRequest {
    AssignRequest {
        lane: lane.addr.clone(),
        tickets: tickets.iter().map(|t| (*t).to_owned()).collect(),
        deadline: Duration::from_secs(deadline_secs),
        poll: Duration::from_millis(200),
        focus_window_ms: 600_000,
        team_keys: vec![],
    }
}

/// A throwaway ledger file and the production writer for it.
struct Ledger {
    _dir: tempfile::TempDir,
    path: PathBuf,
    config: FleetConfig,
}

impl Ledger {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        let config = FleetConfig {
            ledger_path: Some(path.clone()),
            ..FleetConfig::default()
        };
        Self {
            _dir: dir,
            path,
            config,
        }
    }

    fn sink(&self) -> AssignSink {
        AssignSink::at(self.path.clone(), &self.config)
    }

    fn rows(&self) -> Vec<Row> {
        let connection = rusqlite::Connection::open(&self.path).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT lane, ticket, order_id, expect_ticket, assigned_at, sent_at, deadline_at,
                   result, result_at, confirm_latency_s, focus_at_deadline, replaced_at
                 FROM assignment_history ORDER BY id",
            )
            .unwrap();
        statement
            .query_map([], |r| {
                Ok(Row {
                    lane: r.get(0)?,
                    ticket: r.get(1)?,
                    order_id: r.get(2)?,
                    expect_ticket: r.get(3)?,
                    assigned_at: r.get(4)?,
                    sent_at: r.get(5)?,
                    deadline_at: r.get(6)?,
                    result: r.get(7)?,
                    result_at: r.get(8)?,
                    confirm_latency_s: r.get(9)?,
                    focus_at_deadline: r.get(10)?,
                    replaced_at: r.get(11)?,
                })
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

#[derive(Debug)]
struct Row {
    lane: String,
    ticket: String,
    order_id: Option<String>,
    expect_ticket: Option<String>,
    assigned_at: i64,
    sent_at: Option<i64>,
    deadline_at: Option<i64>,
    result: String,
    result_at: Option<i64>,
    confirm_latency_s: Option<i64>,
    focus_at_deadline: Option<String>,
    replaced_at: Option<i64>,
}

/// The lane's session takes the order: the watcher delivers it, then the session emits a tool
/// event in `cwd` on `branch`.
fn lane_acts(
    port: u16,
    who: &Lane,
    branch: &str,
    cwd: &str,
    delay: Duration,
) -> std::thread::JoinHandle<String> {
    let (name, addr, session) = (who.name.clone(), who.addr.clone(), who.session.clone());
    let (branch, cwd) = (branch.to_owned(), cwd.to_owned());
    std::thread::spawn(move || {
        let mut relay = connect(port);
        let popped = deliver(&mut relay, &name);
        std::thread::sleep(delay);
        tool_event(&mut relay, &addr, &session, &branch, &cwd);
        popped
    })
}

fn tool_event(relay: &mut Relay, addr: &str, session: &str, branch: &str, cwd: &str) {
    let mut event = ActivityEvent::new(ActivityKind::PreTool, Harness::Claude, session, 1);
    event.cwd = cwd.to_owned();
    event.branch = branch.to_owned();
    relay
        .record_activity(&addr.parse().unwrap(), &event)
        .unwrap();
}

/// Waits for the order, claims it and records the `Delivered` receipt; returns its text.
fn deliver(relay: &mut Relay, name: &AgentName) -> String {
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let pop = relay
            .pop_inbox_budget(
                name,
                NonZeroUsize::new(1).unwrap(),
                1_000_000,
                Claim::new(Popper::Watcher).with_lease(Duration::from_secs(600)),
            )
            .unwrap();
        if let Some(batch) = pop.batch {
            relay
                .observe_delivery(
                    name,
                    batch,
                    Popper::Watcher,
                    DeliveryEvidence::HerdrPrompt,
                    Now::Server,
                )
                .unwrap();
            return pop
                .messages
                .first()
                .map(|m| m.text.clone())
                .unwrap_or_default();
        }
    }
}

fn delivered_only(port: u16, who: &Lane) -> std::thread::JoinHandle<()> {
    let name = who.name.clone();
    std::thread::spawn(move || {
        let mut relay = connect(port);
        deliver(&mut relay, &name);
    })
}

fn inbox_len(relay: &mut Relay, who: &Lane) -> usize {
    relay.inbox_len(&who.name).unwrap()
}

/// Waits until the relay clock is past `ms`.
fn wait_past(relay: &mut Relay, ms: i64) {
    while relay.registry().unwrap().now_ms <= ms {
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[trace("FR-039-AC-12")]
fn a_cold_relay_reports_unidentified_pending_instead_of_claiming_complete_history() {
    let (_container, port) = Container::start_uninstrumented();
    let mut relay = connect(port);
    let target = lane(&mut relay, "cold-start", "cold-start");
    let report = assign(
        &mut relay,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 30),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    assert_eq!(report.result, AssignResult::Pending);
    assert!(report.order_id.is_none());
    let error = report.list_write_error.unwrap();
    assert!(
        error.contains("order accepted but not identified"),
        "{error}"
    );
    assert!(error.contains("before_instrumentation"), "{error}");
    assert!(relay.assignment(&target.addr).unwrap().is_none());
}

#[trace("FR-035-AC-1")]
#[trace("FR-035-AC-2")]
#[trace("FR-035-AC-5")]
#[trace("FR-035-AC-7")]
#[trace("FR-035-AC-9")]
#[trace("FR-035-AC-10")]
#[trace("FR-035-AC-11")]
#[test]
fn confirmed_assignment_stores_the_list_carries_the_text_and_joins_by_order_id() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let issues = batch(vec![open("SWM-211"), open("SWM-212")]);
    let ledger = Ledger::new();

    let acts = lane_acts(
        port,
        &target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(300),
    );
    let report = assign(
        &mut relay,
        &mut ledger.sink(),
        &request(&target, &["SWM-211", "SWM-212"], 30),
        &issues,
        &orchestrator(),
    )
    .unwrap();
    let body = acts.join().unwrap();

    assert_eq!(report.result, AssignResult::Confirmed);
    assert!(report.confirmed() && report.defects.is_empty());
    // AC-1: the relay holds the ordered list.
    let list = relay.assignment(&target.addr).unwrap().unwrap();
    assert_eq!(
        list.tickets
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["SWM-211", "SWM-212"]
    );
    // AC-2: the exact text, in order, with the deadline.
    let want_211 = "Description:\nDo SWM-211.\n\nAcceptance criteria:\n- [ ] first\n- [ ] second\n";
    assert!(body.contains(want_211), "{body}");
    assert!(body.find("SWM-211 ===").unwrap() < body.find("SWM-212 ===").unwrap());
    assert!(body.contains("Title: Title of SWM-212\nState: Coding\n"));
    assert!(body.contains("within 30s"));
    // AC-9 and AC-10: the expectation rode on the order; the transition holds the order id.
    let order_id = report.order_id.clone().unwrap();
    let receipts = relay.receipts_since(report.sent_at_ms.unwrap()).unwrap();
    let acted = receipts
        .iter()
        .find(|e: &&ReceiptEvent| e.original_id == order_id && e.expect_ticket.is_some())
        .expect("Acted receipt with the expectation");
    assert_eq!(acted.expect_ticket.as_deref(), Some("SWM-211"));
    let events = relay.assignment_events("0-0", 100).unwrap().events;
    assert_eq!(
        events.last().unwrap().order_id.as_deref(),
        Some(order_id.as_str())
    );
    relay
        .assign(
            &orchestrator(),
            &target.addr,
            &AssignWrite::Append(vec![AssignedTicket {
                id: "SWM-213".into(),
                reference: "SWM-213".into(),
            }]),
            None,
        )
        .unwrap();
    assert_eq!(
        relay
            .assignment_events("0-0", 100)
            .unwrap()
            .events
            .last()
            .unwrap()
            .order_id,
        None,
        "a list write not made by assign holds no order id"
    );
    // AC-11: the stored row as computed from the fixture.
    let rows = ledger.rows();
    let [row] = rows.as_slice() else {
        panic!("{rows:?}")
    };
    assert_eq!(row.lane, target.addr);
    assert_eq!(row.ticket, "SWM-211,SWM-212");
    assert_eq!(row.order_id.as_deref(), Some(order_id.as_str()));
    assert_eq!(row.expect_ticket.as_deref(), Some("SWM-211"));
    assert_eq!(row.assigned_at, row.sent_at.unwrap());
    assert_eq!(row.deadline_at.unwrap() - row.sent_at.unwrap(), 30_000);
    assert_eq!(row.result, "confirmed");
    let result_at = row.result_at.unwrap();
    assert!((row.sent_at.unwrap()..=row.deadline_at.unwrap()).contains(&result_at));
    assert_eq!(
        row.confirm_latency_s,
        Some((result_at - row.sent_at.unwrap()) / 1_000)
    );
    assert_eq!(
        row.focus_at_deadline, None,
        "the focus at the deadline is filled by the first run after it"
    );

    // AC-7: a second assign replaces the list; the ledger retains both lists with timestamps.
    let second = assign(
        &mut relay,
        &mut ledger.sink(),
        &request(&target, &["SWM-212"], 10),
        &batch(vec![open("SWM-212")]),
        &orchestrator(),
    )
    .unwrap();
    assert_eq!(second.previous_tickets, ["SWM-211", "SWM-212", "SWM-213"]);
    assert_eq!(
        relay
            .assignment(&target.addr)
            .unwrap()
            .unwrap()
            .tickets
            .len(),
        1
    );
    let rows = ledger.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ticket, "SWM-211,SWM-212");
    assert_eq!(rows[1].ticket, "SWM-212");
    assert_eq!(rows[0].replaced_at, rows[1].sent_at);
    assert_eq!(rows[1].replaced_at, None);
}

#[trace("FR-035-AC-5")]
#[trace("FR-035-AC-11")]
#[test]
fn acted_on_another_ticket_is_acted_not_focused() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let acts = lane_acts(
        port,
        &target,
        "swm-999-other",
        "/w/other",
        Duration::from_millis(100),
    );
    let report = assign(
        &mut relay,
        &mut ledger.sink(),
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    acts.join().unwrap();
    assert_eq!(report.result, AssignResult::ActedNotFocused);
    assert!(!report.confirmed());
    let rows = ledger.rows();
    let row = &rows[0];
    assert_eq!(row.result, "acted_not_focused");
    assert_eq!(row.order_id, report.order_id);
    assert_eq!(row.expect_ticket.as_deref(), Some("SWM-211"));
    assert_eq!(row.deadline_at.unwrap() - row.sent_at.unwrap(), 10_000);
    assert!(row.result_at.unwrap() >= row.deadline_at.unwrap());
    assert_eq!(row.confirm_latency_s, None);
    assert_eq!(row.focus_at_deadline.as_deref(), Some("SWM-999"));
}

#[trace("FR-035-AC-6")]
#[trace("FR-035-AC-11")]
#[test]
fn delivered_without_action_is_ack_expired_and_leaves_the_ledger_row() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let delivered = delivered_only(port, &target);
    let ledger = Ledger::new();
    let report = assign(
        &mut relay,
        &mut ledger.sink(),
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    delivered.join().unwrap();
    assert_eq!(report.result, AssignResult::AckExpired);
    assert!(!report.confirmed());
    let rows = ledger.rows();
    let row = &rows[0];
    assert_eq!(row.result, "ack_expired");
    assert_eq!(row.order_id, report.order_id);
    assert!(row.result_at.unwrap() >= row.deadline_at.unwrap());
    assert_eq!(row.confirm_latency_s, None);
    assert_eq!(row.focus_at_deadline.as_deref(), Some("no_worktree"));
}

#[trace("FR-035-AC-6")]
#[trace("FR-035-AC-11")]
#[test]
fn never_delivered_is_timed_out_and_leaves_the_ledger_row() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let report = assign(
        &mut relay,
        &mut ledger.sink(),
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    assert_eq!(report.result, AssignResult::TimedOut);
    assert!(!report.confirmed());
    let rows = ledger.rows();
    let row = &rows[0];
    assert_eq!(row.result, "timed_out");
    assert_eq!(row.order_id, report.order_id);
    assert_eq!(row.confirm_latency_s, None);
    assert_eq!(row.focus_at_deadline.as_deref(), Some("no_worktree"));
}

#[trace("FR-035-AC-6")]
#[test]
fn an_ack_expired_receipt_ends_the_wait_before_the_deadline_passes_locally() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let delivered = delivered_only(port, &target);
    let expirer = std::thread::spawn(move || {
        let mut relay = connect(port);
        std::thread::sleep(Duration::from_millis(11_500));
        relay.expire_orders(Now::Server).unwrap();
    });
    let report = assign(
        &mut relay,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    delivered.join().unwrap();
    expirer.join().unwrap();
    assert_eq!(report.result, AssignResult::AckExpired);
}

/// Delegates to the real relay and fails the chosen steps.
struct Flaky<'a> {
    relay: &'a mut Relay,
    fail_send: bool,
    fail_write: bool,
    /// Reads of the wait that fail before reads work again.
    fail_reads: u32,
    armed: bool,
    panic_in_wait: bool,
}

impl<'a> Flaky<'a> {
    fn new(relay: &'a mut Relay) -> Self {
        Self {
            relay,
            fail_send: false,
            fail_write: false,
            fail_reads: 0,
            armed: false,
            panic_in_wait: false,
        }
    }

    fn read_fails(&mut self) -> bool {
        if self.armed && self.fail_reads > 0 {
            self.fail_reads -= 1;
            return true;
        }
        false
    }
}

impl AssignBus for Flaky<'_> {
    fn registry(&mut self) -> Result<RegistryView, String> {
        assert!(!(self.armed && self.panic_in_wait), "killed while waiting");
        if self.read_fails() {
            return Err("injected registry failure".into());
        }
        self.relay.registry()
    }
    fn assignments(&mut self) -> Result<Vec<AssignmentList>, String> {
        AssignBus::assignments(self.relay)
    }
    fn send_order(&mut self, msg: &Message, spec: &OrderSpec) -> Result<Vec<AgentName>, String> {
        if self.fail_send {
            return Err("injected send failure".into());
        }
        self.relay.send_order(msg, spec)
    }
    fn receipts_since(&mut self, since_ms: i64) -> Result<Vec<ReceiptEvent>, String> {
        if self.read_fails() {
            return Err("injected receipts failure".into());
        }
        self.relay.receipts_since(since_ms)
    }
    fn write_list(
        &mut self,
        writer: &AssignWriter,
        lane: &str,
        tickets: Vec<AssignedTicket>,
        order_id: &str,
    ) -> Result<AssignmentList, String> {
        if self.fail_write {
            return Err("injected list failure".into());
        }
        let list = self.relay.write_list(writer, lane, tickets, order_id);
        self.armed = true;
        list
    }
    fn lane_events(&mut self, lane: &str) -> Result<Vec<FocusEvent>, String> {
        if self.read_fails() {
            return Err("injected events failure".into());
        }
        self.relay.lane_events(lane)
    }
}

/// A ledger that fails every write from the `fail_from`th call.
struct FailingLedger {
    inner: AssignSink,
    calls: u32,
    fail_from: u32,
}

impl AssignLedger for FailingLedger {
    fn record(&mut self, row: &AssignReport) -> Result<(), String> {
        self.calls += 1;
        if self.calls >= self.fail_from {
            return Err("injected ledger failure".into());
        }
        self.inner.record(row)
    }
    fn withdraw(&mut self) -> Result<(), String> {
        self.inner.withdraw()
    }
}

#[trace("FR-035-AC-3")]
#[test]
fn a_send_failure_leaves_the_list_and_withdraws_the_row() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let mut bus = Flaky::new(&mut relay);
    bus.fail_send = true;
    let error = assign(
        &mut bus,
        &mut ledger.sink(),
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap_err();
    assert!(matches!(error, AssignError::Send(_)), "{error}");
    assert!(error.operational());
    assert_eq!(relay.assignment(&target.addr).unwrap(), None);
    assert_eq!(inbox_len(&mut relay, &target), 0);
    assert!(ledger.rows().is_empty(), "the pending row is withdrawn");
}

#[trace("FR-035-AC-3")]
#[test]
fn a_list_failure_after_the_send_names_the_order_and_records_it() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let mut bus = Flaky::new(&mut relay);
    bus.fail_write = true;
    let report = assign(
        &mut bus,
        &mut ledger.sink(),
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    let order_id = report.order_id.clone().unwrap();
    let message = report.list_write_error.clone().unwrap();
    assert!(message.contains(&order_id) && message.contains("injected list failure"));
    assert!(!report.confirmed() && report.operational_failure());
    assert_eq!(
        inbox_len(&mut relay, &target),
        1,
        "the accepted order stays sent"
    );
    assert_eq!(relay.assignment(&target.addr).unwrap(), None);
    let rows = ledger.rows();
    assert_eq!(rows[0].order_id.as_deref(), Some(order_id.as_str()));
    assert_eq!(rows[0].result, "pending");
}

#[trace("FR-035-AC-14")]
#[test]
fn a_ledger_failure_after_the_send_is_a_defect_and_a_failed_exit() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let delivered = delivered_only(port, &target);
    let mut failing = FailingLedger {
        inner: ledger.sink(),
        calls: 0,
        fail_from: 3,
    };
    let report = assign(
        &mut relay,
        &mut failing,
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    delivered.join().unwrap();
    assert!(
        report
            .defects
            .iter()
            .any(|d| d.kind == "ledger_unwritable" && d.detail.contains("injected ledger failure"))
    );
    assert!(report.operational_failure() && !report.confirmed());
    assert_eq!(ledger.rows()[0].result, "pending");
}

#[trace("FR-035-AC-4")]
#[test]
fn refusals_before_the_send_name_the_holding_lane_and_send_nothing() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let one = lane(&mut relay, "s-one", "one");
    let two = lane(&mut relay, "s-two", "two");
    relay
        .assign(
            &orchestrator(),
            &two.addr,
            &AssignWrite::Set(vec![AssignedTicket {
                id: "SWM-211".into(),
                reference: "SWM-211".into(),
            }]),
            None,
        )
        .unwrap();
    let all = batch(vec![
        open("SWM-211"),
        open("SWM-212"),
        issue("SWM-300", "Done", WorkflowStateType::Completed, "x"),
        issue("SWM-301", "Canceled", WorkflowStateType::Canceled, "x"),
        issue("SWM-302", "Merged", WorkflowStateType::Started, "x"),
    ]);
    let run = |relay: &mut Relay, tickets: &[&str]| {
        assign(
            relay,
            &mut NoLedger,
            &request(&one, tickets, 10),
            &all,
            &orchestrator(),
        )
        .unwrap_err()
    };
    match run(&mut relay, &["SWM-212", "SWM-211"]) {
        AssignError::AssignedElsewhere { ticket, lane } => {
            assert_eq!(ticket, "SWM-211");
            assert_eq!(lane, two.addr);
        }
        other => panic!("{other}"),
    }
    for closed in ["SWM-300", "SWM-301", "SWM-302"] {
        assert!(matches!(
            run(&mut relay, &[closed]),
            AssignError::Closed { .. }
        ));
    }
    assert!(matches!(
        run(&mut relay, &["SWM-999"]),
        AssignError::Unreadable(_)
    ));
    assert_eq!(inbox_len(&mut relay, &one), 0);
    assert_eq!(relay.assignment(&one.addr).unwrap(), None);
}

#[trace("FR-035-AC-9")]
#[test]
fn a_first_ticket_id_of_65_bytes_fails_before_any_send() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let long = format!("SWM-{}", "1".repeat(61));
    assert_eq!(long.len(), 65);
    let error = assign(
        &mut relay,
        &mut NoLedger,
        &request(&target, &[&long], 10),
        &batch(vec![open(&long)]),
        &orchestrator(),
    )
    .unwrap_err();
    assert!(matches!(error, AssignError::Order(_)), "{error}");
    assert_eq!(inbox_len(&mut relay, &target), 0);
}

#[trace("FR-035-AC-13")]
#[test]
fn an_order_over_the_relay_bound_or_with_unsafe_text_is_refused_before_the_send() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let big = issue(
        "SWM-211",
        "Coding",
        WorkflowStateType::Started,
        &"x".repeat(5_000),
    );
    let bad = issue(
        "SWM-212",
        "Coding",
        WorkflowStateType::Started,
        "a\u{202E}b",
    );
    for (ticket, issue) in [("SWM-211", big), ("SWM-212", bad)] {
        let error = assign(
            &mut relay,
            &mut NoLedger,
            &request(&target, &[ticket], 10),
            &batch(vec![issue]),
            &orchestrator(),
        )
        .unwrap_err();
        assert!(matches!(error, AssignError::Text(_)), "{error}");
    }
    assert_eq!(inbox_len(&mut relay, &target), 0);
}

#[trace("FR-035-AC-15")]
#[test]
fn a_lane_that_touched_another_ticket_before_the_order_still_confirms() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    tool_event(
        &mut relay,
        &target.addr,
        &target.session,
        "swm-999-old",
        "/w/old",
    );
    let acts = lane_acts(
        port,
        &target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(300),
    );
    let report = assign(
        &mut relay,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 30),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    acts.join().unwrap();
    assert_eq!(report.result, AssignResult::Confirmed);
}

#[trace("FR-035-AC-14")]
#[test]
fn a_transient_relay_read_error_does_not_end_the_wait_and_is_reported() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let acts = lane_acts(
        port,
        &target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(1_500),
    );
    let mut bus = Flaky::new(&mut relay);
    bus.fail_reads = 4;
    let report = assign(
        &mut bus,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 30),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    acts.join().unwrap();
    assert_eq!(report.result, AssignResult::Confirmed);
    let defect = report
        .defects
        .iter()
        .find(|d| d.kind == "relay_read_errors")
        .expect("the dropped read errors are recorded");
    assert!(defect.detail.contains("injected"));
}

#[trace("FR-035-AC-12")]
#[test]
fn a_command_killed_after_the_send_leaves_pending_and_the_next_run_completes_it() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let acts = lane_acts(
        port,
        &target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(300),
    );
    let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut bus = Flaky::new(&mut relay);
        bus.panic_in_wait = true;
        assign(
            &mut bus,
            &mut ledger.sink(),
            &request(&target, &["SWM-211"], 10),
            &batch(vec![open("SWM-211")]),
            &orchestrator(),
        )
    }));
    assert!(killed.is_err(), "the command was killed in the wait");
    acts.join().unwrap();
    let rows = ledger.rows();
    assert_eq!(rows[0].result, "pending");
    assert!(rows[0].order_id.is_some() && rows[0].result_at.is_none());

    // Before the deadline a run leaves the row alone.
    assert_eq!(
        complete_pending(
            &ledger.config,
            &ix_board::fleet::tick::TickSettings::default(),
            &mut relay,
        )
        .unwrap(),
        0
    );
    assert_eq!(ledger.rows()[0].result, "pending");

    wait_past(&mut relay, rows[0].deadline_at.unwrap());
    assert_eq!(
        complete_pending(
            &ledger.config,
            &ix_board::fleet::tick::TickSettings::default(),
            &mut relay,
        )
        .unwrap(),
        1
    );
    let rows = ledger.rows();
    let row = &rows[0];
    assert_eq!(row.result, "confirmed");
    assert!(row.confirm_latency_s.is_some());
    assert_eq!(row.focus_at_deadline.as_deref(), Some("SWM-211"));
    assert_eq!(
        complete_pending(
            &ledger.config,
            &ix_board::fleet::tick::TickSettings::default(),
            &mut relay,
        )
        .unwrap(),
        0
    );
}

#[trace("FR-035-AC-11")]
#[test]
fn an_early_confirmation_gets_its_focus_at_the_deadline_from_the_next_run() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let acts = lane_acts(
        port,
        &target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(300),
    );
    let report = assign(
        &mut relay,
        &mut ledger.sink(),
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    acts.join().unwrap();
    assert_eq!(report.result, AssignResult::Confirmed);
    assert_eq!(report.focus_at_deadline, None);
    // The lane moves to another ticket after confirming and before the deadline passes.
    tool_event(
        &mut relay,
        &target.addr,
        &target.session,
        "swm-999-z",
        "/w/z",
    );
    wait_past(&mut relay, report.deadline_at_ms.unwrap());
    assert_eq!(
        complete_pending(
            &ledger.config,
            &ix_board::fleet::tick::TickSettings::default(),
            &mut relay,
        )
        .unwrap(),
        1
    );
    let rows = ledger.rows();
    assert_eq!(rows[0].result, "confirmed");
    assert!(
        rows[0]
            .focus_at_deadline
            .as_deref()
            .is_some_and(|f| f.starts_with("ambiguous:")),
        "{:?}",
        rows[0].focus_at_deadline
    );
}

/// A bus that records every call and sends nothing.
struct Fake {
    stale: bool,
    registrations: usize,
    calls: Vec<&'static str>,
}

impl AssignBus for Fake {
    fn registry(&mut self) -> Result<RegistryView, String> {
        self.calls.push("registry");
        Ok(RegistryView {
            now_ms: 1,
            lanes: vec![FleetLane {
                address: "terra:SWM:one".into(),
                stale: self.stale,
            }],
            registrations: (0..self.registrations)
                .map(|n| {
                    (
                        format!("session/x{n}").parse().unwrap(),
                        "terra:SWM:one".into(),
                    )
                })
                .collect(),
        })
    }
    fn assignments(&mut self) -> Result<Vec<AssignmentList>, String> {
        self.calls.push("assignments");
        Ok(vec![])
    }
    fn send_order(&mut self, _: &Message, _: &OrderSpec) -> Result<Vec<AgentName>, String> {
        self.calls.push("send");
        Ok(vec![])
    }
    fn receipts_since(&mut self, _: i64) -> Result<Vec<ReceiptEvent>, String> {
        self.calls.push("receipts");
        Ok(vec![])
    }
    fn write_list(
        &mut self,
        _: &AssignWriter,
        _: &str,
        _: Vec<AssignedTicket>,
        _: &str,
    ) -> Result<AssignmentList, String> {
        self.calls.push("write");
        Err("unreachable".into())
    }
    fn lane_events(&mut self, _: &str) -> Result<Vec<FocusEvent>, String> {
        Ok(vec![])
    }
}

#[trace("FR-035-AC-8")]
#[trace("FR-035-AC-13")]
#[test]
fn a_non_orchestrator_an_own_lane_a_stale_host_and_no_recipient_refuse_before_any_send() {
    let issues = batch(vec![open("SWM-211")]);
    let req = AssignRequest {
        lane: "terra:SWM:one".into(),
        tickets: vec!["SWM-211".into()],
        deadline: Duration::from_secs(10),
        poll: Duration::from_millis(10),
        focus_window_ms: 600_000,
        team_keys: vec![],
    };
    let mut bus = Fake {
        stale: false,
        registrations: 1,
        calls: vec![],
    };
    let plain = AssignWriter {
        orchestrator: false,
        ..orchestrator()
    };
    assert!(matches!(
        assign(&mut bus, &mut NoLedger, &req, &issues, &plain).unwrap_err(),
        AssignError::NotOrchestrator
    ));
    let own = AssignWriter {
        caller_lane: Some("terra:SWM:one".parse().unwrap()),
        ..orchestrator()
    };
    assert!(matches!(
        assign(&mut bus, &mut NoLedger, &req, &issues, &own).unwrap_err(),
        AssignError::OwnLane
    ));
    let mut stale = Fake {
        stale: true,
        registrations: 1,
        calls: vec![],
    };
    let error = assign(&mut stale, &mut NoLedger, &req, &issues, &orchestrator()).unwrap_err();
    assert!(
        error.to_string().starts_with("host_registry_stale"),
        "{error}"
    );
    let mut two = Fake {
        stale: false,
        registrations: 2,
        calls: vec![],
    };
    assert!(matches!(
        assign(&mut two, &mut NoLedger, &req, &issues, &orchestrator()).unwrap_err(),
        AssignError::NoRecipient(_)
    ));
    for fake in [&bus, &stale, &two] {
        assert!(!fake.calls.contains(&"send") && !fake.calls.contains(&"write"));
    }
}

fn ev(ms: i64, branch: &str) -> FocusEvent {
    FocusEvent {
        server_ms: ms,
        cwd: format!("/w/{branch}"),
        branch: branch.into(),
    }
}

fn timing(now_ms: i64) -> Timing {
    Timing {
        sent_at_ms: 1_000,
        deadline_at_ms: 301_000,
        now_ms,
        focus_window_ms: 600_000,
    }
}

#[trace("FR-035-AC-15")]
#[test]
fn focus_uses_the_configured_team_keys_and_the_focus_at_the_deadline() {
    let keys = focus_keys(&["QSL".into()], &["SWM-211".into()]);
    assert_eq!(keys, ["QSL", "SWM"]);
    // A lane on QSL-5 is not confirmed for SWM-211, and a lane with both is ambiguous.
    let only_other = [ev(2_000, "qsl-5-x")];
    let early = timing(100_000);
    assert_eq!(
        decide(
            &only_other,
            OrderEvidence::default(),
            early,
            &keys,
            "SWM-211"
        ),
        None
    );
    let both = [ev(2_000, "qsl-5-x"), ev(3_000, "swm-211-y")];
    assert_eq!(
        decide(&both, OrderEvidence::default(), early, &keys, "SWM-211"),
        None,
        "two tickets after the order are ambiguous, not confirmed"
    );
    let done = decide(
        &only_other,
        OrderEvidence {
            delivered: true,
            acted: true,
            expired: false,
        },
        timing(301_000),
        &keys,
        "SWM-211",
    )
    .unwrap();
    assert_eq!(done.result, AssignResult::ActedNotFocused);
    assert_eq!(done.focus_at_deadline.as_deref(), Some("QSL-5"));

    // Confirmed early: no focus at the deadline yet; confirmed late: the focus at the deadline,
    // not at the confirming event.
    let switch = [ev(2_000, "swm-211-y"), ev(200_000, "swm-999-z")];
    let early = decide(
        &switch,
        OrderEvidence::default(),
        timing(5_000),
        &keys,
        "SWM-211",
    )
    .unwrap();
    assert_eq!(early.result, AssignResult::Confirmed);
    assert_eq!(early.confirm_latency_s, Some(1));
    assert_eq!(early.focus_at_deadline, None);
    let late = decide(
        &switch,
        OrderEvidence::default(),
        timing(400_000),
        &keys,
        "SWM-211",
    )
    .unwrap();
    assert_eq!(late.result, AssignResult::Confirmed);
    assert_eq!(late.focus_at_deadline.as_deref(), Some("ambiguous:SWM-999"));
}

fn status_report() -> FleetReport {
    let receipts = ReadReceipts(Ok(ReceiptRead::default()));
    let sources = Sources {
        relay: Ok(RelaySnapshot::default()),
        series: None,
        receipts: &receipts,
        weak: &NoWeakSignals,
    };
    build_report(&FleetConfig::default(), &sources, 1)
}

/// Leaves the lane's order sent, its list written and its ledger row `pending`: the command was
/// killed in the wait. Returns the deadline.
fn leave_pending(port: u16, relay: &mut Relay, target: &Lane, ledger: &Ledger) -> i64 {
    let acts = lane_acts(
        port,
        target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(300),
    );
    let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut bus = Flaky::new(relay);
        bus.panic_in_wait = true;
        assign(
            &mut bus,
            &mut ledger.sink(),
            &request(target, &["SWM-211"], 10),
            &batch(vec![open("SWM-211")]),
            &orchestrator(),
        )
    }));
    assert!(killed.is_err());
    acts.join().unwrap();
    ledger.rows()[0].deadline_at.unwrap()
}

fn count(ledger: &Ledger, sql: &str) -> i64 {
    rusqlite::Connection::open(&ledger.path)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[trace("FR-035-AC-12")]
#[test]
fn a_status_run_commits_the_completed_row_with_its_run() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let deadline = leave_pending(port, &mut relay, &target, &ledger);
    wait_past(&mut relay, deadline);
    let mut report = status_report();
    record_status_completing(
        &ledger.config,
        &ix_board::fleet::tick::TickSettings::default(),
        &mut report,
        &mut relay,
    );
    let rows = ledger.rows();
    assert_eq!(rows[0].result, "confirmed");
    assert_eq!(rows[0].focus_at_deadline.as_deref(), Some("SWM-211"));
    assert_eq!(count(&ledger, "SELECT COUNT(*) FROM run"), 1);
    assert_eq!(
        count(
            &ledger,
            "SELECT COUNT(*) FROM defect WHERE kind='ledger_unwritable'"
        ),
        0
    );
}

#[trace("FR-035-AC-12")]
#[trace("FR-039-AC-12")]
#[test]
fn a_failed_source_completion_is_an_unreadable_source_committed_with_the_run() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let ledger = Ledger::new();
    let deadline = leave_pending(port, &mut relay, &target, &ledger);
    wait_past(&mut relay, deadline);
    let mut bus = Flaky::new(&mut relay);
    bus.armed = true;
    bus.fail_reads = 100;
    let mut report = status_report();
    record_status_completing(
        &ledger.config,
        &ix_board::fleet::tick::TickSettings::default(),
        &mut report,
        &mut bus,
    );
    assert!(!report.complete);
    assert_eq!(ledger.rows()[0].result, "pending");
    assert_eq!(count(&ledger, "SELECT COUNT(*) FROM run"), 1);
    assert_eq!(
        count(
            &ledger,
            "SELECT COUNT(*) FROM defect WHERE kind='unreadable_source' AND source='relay:order_history'"
        ),
        1,
        "the completion defect is stored with the run"
    );
    assert_eq!(
        count(
            &ledger,
            "SELECT COUNT(*) FROM defect WHERE kind='ledger_unwritable'"
        ),
        0,
        "a source read failure is not a ledger write failure"
    );
}
