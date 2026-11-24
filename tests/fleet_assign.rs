// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet assign` through relay-core's real paths against a throwaway Redis (`docker`, redis:7),
//! plus a fake bus for the registry refusals the relay cannot be made to produce quickly.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ix_board::fleet::assign::{
    AssignBus, AssignError, AssignLedger, AssignReport, AssignRequest, AssignResult, NoLedger,
    RegistryView, assign,
};
use ix_board::fleet::focus::FocusEvent;
use ix_board::{Issue, IssueBatch, IssueId, ProjectId, TeamKey, WorkflowStateType};
use ix_trace_rs::trace;
use relay_core::assignment::{AssignWriter, AssignedTicket, AssignmentList};
use relay_core::fleet::FleetLane;
use relay_core::receipts::{DeliveryEvidence, OrderSpec, ReceiptEvent};
use relay_core::{
    ActivityEvent, ActivityKind, AgentName, Claim, Harness, HerdrPlaceIds, Message, Now, Place,
    Popper, Registration, Relay,
};
use std::num::NonZeroUsize;
use std::process::Command;
use std::time::Duration;

struct Container(String);

impl Container {
    fn start() -> (Self, u16) {
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
    }
}

#[derive(Default)]
struct Recorder(Vec<AssignReport>);

impl AssignLedger for Recorder {
    fn record(&mut self, row: &AssignReport) -> Result<(), String> {
        self.0.push(row.clone());
        Ok(())
    }
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
        let popped = loop {
            std::thread::sleep(Duration::from_millis(100));
            let pop = relay
                .pop_inbox_budget(
                    &name,
                    NonZeroUsize::new(1).unwrap(),
                    1_000_000,
                    Claim::new(Popper::Watcher).with_lease(Duration::from_secs(600)),
                )
                .unwrap();
            if let Some(batch) = pop.batch {
                relay
                    .observe_delivery(
                        &name,
                        batch,
                        Popper::Watcher,
                        DeliveryEvidence::HerdrPrompt,
                        Now::Server,
                    )
                    .unwrap();
                break pop
                    .messages
                    .first()
                    .map(|m| m.text.clone())
                    .unwrap_or_default();
            }
        };
        std::thread::sleep(delay);
        let mut event = ActivityEvent::new(ActivityKind::PreTool, Harness::Claude, &session, 1);
        event.cwd = cwd;
        event.branch = branch;
        relay
            .record_activity(&addr.parse().unwrap(), &event)
            .unwrap();
        popped
    })
}

fn delivered_only(port: u16, who: &Lane) -> std::thread::JoinHandle<()> {
    let name = who.name.clone();
    std::thread::spawn(move || {
        let mut relay = connect(port);
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let pop = relay
                .pop_inbox_budget(
                    &name,
                    NonZeroUsize::new(1).unwrap(),
                    1_000_000,
                    Claim::new(Popper::Watcher).with_lease(Duration::from_secs(600)),
                )
                .unwrap();
            if let Some(batch) = pop.batch {
                relay
                    .observe_delivery(
                        &name,
                        batch,
                        Popper::Watcher,
                        DeliveryEvidence::HerdrPrompt,
                        Now::Server,
                    )
                    .unwrap();
                return;
            }
        }
    })
}

fn inbox_len(relay: &mut Relay, who: &Lane) -> usize {
    relay.inbox_len(&who.name).unwrap()
}

#[trace("FR-035-AC-1")]
#[trace("FR-035-AC-2")]
#[trace("FR-035-AC-5")]
#[trace("FR-035-AC-9")]
#[trace("FR-035-AC-10")]
#[trace("FR-035-AC-11")]
#[trace("FR-035-AC-7")]
#[test]
fn confirmed_assignment_stores_the_list_carries_the_text_and_joins_by_order_id() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let issues = batch(vec![open("SWM-211"), open("SWM-212")]);
    let mut ledger = Recorder::default();

    let acts = lane_acts(
        port,
        &target,
        "swm-211-fleet-assign",
        "/w/ix-board-swm-211",
        Duration::from_millis(300),
    );
    let report = assign(
        &mut relay,
        &mut ledger,
        &request(&target, &["SWM-211", "SWM-212"], 30),
        &issues,
        &orchestrator(),
    )
    .unwrap();
    let body = acts.join().unwrap();

    assert_eq!(report.result, AssignResult::Confirmed);
    assert_eq!(report.focus_at_deadline.as_deref(), Some("SWM-211"));
    assert!(report.confirm_latency_s.is_some());
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
    let receipts = relay.receipts_since(0).unwrap();
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
    // A list write not made by assign holds no order id.
    relay
        .assign(
            &orchestrator(),
            &target.addr,
            &relay_core::assignment::AssignWrite::Append(vec![AssignedTicket {
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
        None
    );
    // AC-11: the row as computed from the fixture.
    let last = ledger.0.last().unwrap();
    assert_eq!(last.order_id.as_deref(), Some(order_id.as_str()));
    assert_eq!(last.expect_ticket, "SWM-211");
    assert!(last.sent_at_ms.unwrap() < last.deadline_at_ms.unwrap());
    assert_eq!(
        last.deadline_at_ms.unwrap() - last.sent_at_ms.unwrap(),
        30_000
    );
    assert!(last.result_at_ms.unwrap() >= last.sent_at_ms.unwrap());
    assert!(ledger.0.first().unwrap().result == AssignResult::Pending);
    assert_eq!(ledger.0.first().unwrap().order_id, None);

    // AC-7: a second assign replaces the list; the report keeps the previous one.
    let second = assign(
        &mut relay,
        &mut ledger,
        &request(&target, &["SWM-212"], 10),
        &batch(vec![open("SWM-212")]),
        &orchestrator(),
    )
    .unwrap();
    assert_eq!(second.previous_tickets, ["SWM-211", "SWM-212", "SWM-213"]);
    let list = relay.assignment(&target.addr).unwrap().unwrap();
    assert_eq!(list.tickets.len(), 1);
    assert!(ledger.0.iter().any(|r| r.tickets == ["SWM-211", "SWM-212"]));
    assert!(
        ledger
            .0
            .iter()
            .any(|r| r.tickets == ["SWM-212"] && r.sent_at_ms.is_some())
    );
}

#[trace("FR-035-AC-5")]
#[trace("FR-035-AC-11")]
#[test]
fn acted_on_another_ticket_is_acted_not_focused() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let acts = lane_acts(
        port,
        &target,
        "swm-999-other",
        "/w/other",
        Duration::from_millis(100),
    );
    let report = assign(
        &mut relay,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    acts.join().unwrap();
    assert_eq!(report.result, AssignResult::ActedNotFocused);
    assert!(!report.confirmed());
    assert_eq!(report.confirm_latency_s, None);
    assert_eq!(report.focus_at_deadline.as_deref(), Some("SWM-999"));
}

#[trace("FR-035-AC-6")]
#[trace("FR-035-AC-11")]
#[test]
fn delivered_without_action_is_ack_expired() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let delivered = delivered_only(port, &target);
    let mut ledger = Recorder::default();
    let report = assign(
        &mut relay,
        &mut ledger,
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    delivered.join().unwrap();
    assert_eq!(report.result, AssignResult::AckExpired);
    assert_eq!(report.confirm_latency_s, None);
    assert_eq!(ledger.0.last().unwrap().result, AssignResult::AckExpired);
}

#[trace("FR-035-AC-6")]
#[trace("FR-035-AC-11")]
#[test]
fn never_delivered_is_timed_out() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let report = assign(
        &mut relay,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 10),
        &batch(vec![open("SWM-211")]),
        &orchestrator(),
    )
    .unwrap();
    assert_eq!(report.result, AssignResult::TimedOut);
    assert_eq!(report.confirm_latency_s, None);
    assert!(!report.confirmed());
}

/// Delegates to the real relay and fails the chosen step.
struct Flaky<'a> {
    relay: &'a mut Relay,
    fail_send: bool,
    fail_write: bool,
}

impl AssignBus for Flaky<'_> {
    fn registry(&mut self) -> Result<RegistryView, String> {
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
        self.relay.write_list(writer, lane, tickets, order_id)
    }
    fn lane_events(&mut self, lane: &str) -> Result<Vec<FocusEvent>, String> {
        self.relay.lane_events(lane)
    }
}

#[trace("FR-035-AC-3")]
#[test]
fn a_send_failure_leaves_the_list_and_a_list_failure_after_the_send_names_the_order() {
    let (_c, port) = Container::start();
    let mut relay = connect(port);
    let target = lane(&mut relay, "s-one", "one");
    let issues = batch(vec![open("SWM-211")]);

    let mut bus = Flaky {
        relay: &mut relay,
        fail_send: true,
        fail_write: false,
    };
    let error = assign(
        &mut bus,
        &mut NoLedger,
        &request(&target, &["SWM-211"], 10),
        &issues,
        &orchestrator(),
    )
    .unwrap_err();
    assert!(matches!(error, AssignError::Send(_)), "{error}");
    assert_eq!(relay.assignment(&target.addr).unwrap(), None);
    assert_eq!(inbox_len(&mut relay, &target), 0);

    let mut ledger = Recorder::default();
    let mut bus = Flaky {
        relay: &mut relay,
        fail_send: false,
        fail_write: true,
    };
    let report = assign(
        &mut bus,
        &mut ledger,
        &request(&target, &["SWM-211"], 10),
        &issues,
        &orchestrator(),
    )
    .unwrap();
    let order_id = report.order_id.clone().unwrap();
    let message = report.list_write_error.clone().unwrap();
    assert!(message.contains(&order_id) && message.contains("injected list failure"));
    assert!(!report.confirmed() && report.list_write_failed());
    assert_eq!(
        inbox_len(&mut relay, &target),
        1,
        "the accepted order stays sent"
    );
    assert_eq!(relay.assignment(&target.addr).unwrap(), None);
    assert_eq!(
        ledger.0.last().unwrap().order_id.as_deref(),
        Some(order_id.as_str())
    );
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
            &relay_core::assignment::AssignWrite::Set(vec![AssignedTicket {
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

/// A bus that records every call and sends nothing.
struct Fake {
    stale: bool,
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
            registrations: vec![("session/x".parse().unwrap(), "terra:SWM:one".into())],
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
#[test]
fn a_non_orchestrator_an_own_lane_and_a_stale_host_refuse_before_any_send() {
    let issues = batch(vec![open("SWM-211")]);
    let req = AssignRequest {
        lane: "terra:SWM:one".into(),
        tickets: vec!["SWM-211".into()],
        deadline: Duration::from_secs(10),
        poll: Duration::from_millis(10),
        focus_window_ms: 600_000,
    };
    let mut bus = Fake {
        stale: false,
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
        calls: vec![],
    };
    let error = assign(&mut stale, &mut NoLedger, &req, &issues, &orchestrator()).unwrap_err();
    assert!(
        error.to_string().starts_with("host_registry_stale"),
        "{error}"
    );
    for fake in [&bus, &stale] {
        assert!(!fake.calls.contains(&"send") && !fake.calls.contains(&"write"));
    }
}
