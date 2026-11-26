// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet tick` (FR-038, SWM-256) over fixtures of status reports and ledger state across ticks.

use ix_board::fleet::ledger::{Ledger, TickCommit, tick_metrics};
use ix_board::fleet::tick::{
    Action, ActionKind, LanePr, LanePrRead, NeedReadyTicketsRead, ReadyWork, StandingOrders,
    TickFacts, TickOutput, TickSettings, TickSources, Unread, caller_from,
};
use ix_board::fleet::{
    ClassKind, Defect, DefectKind, Evidence, FleetConfig, FleetReport, HostSummary, LaneRow,
    SourceError,
};
use ix_trace_rs::trace;
use rusqlite::Connection;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

const T0: i64 = 1_000_000_000_000;
const LANE: &str = "h:w:1";
const LEAD: &str = "lead:w:1";
const ORCH: &str = "orch:w:0";
const MINDER: &str = "minder:w:0";

fn row(lane: &str, class: ClassKind, reason: Option<&str>, relay: Option<&str>) -> LaneRow {
    LaneRow {
        lane: lane.to_owned(),
        host: lane.split(':').next().unwrap().to_owned(),
        class,
        reason: reason.map(str::to_owned),
        state_age_secs: None,
        last_event_ms: None,
        evidence: relay
            .map(|r| Evidence {
                item: format!("ix_fleet_lane_blocked{{reason={r}}}"),
                ts_ms: None,
                source: "prometheus".to_owned(),
            })
            .into_iter()
            .collect(),
        compile_seconds: None,
        focus: None,
        thresholds_defaulted: false,
        schema_version: 1,
    }
}

fn idle(lane: &str) -> LaneRow {
    row(lane, ClassKind::Idle, None, None)
}

fn report(now: i64, rows: Vec<LaneRow>, defects: Vec<Defect>) -> FleetReport {
    let mut hosts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for r in &rows {
        let e = hosts.entry(r.host.clone()).or_default();
        e.0 += 1;
        e.1 += usize::from(r.class == ClassKind::Working);
    }
    FleetReport {
        schema_version: 1,
        complete: defects.is_empty(),
        run_ms: now,
        rows,
        hosts: hosts
            .into_iter()
            .map(|(host, (lanes, working))| HostSummary {
                host,
                lanes,
                working,
                not_working: lanes - working,
            })
            .collect(),
        defects,
        host_busy: BTreeMap::new(),
        relay_unavailable: false,
    }
}

fn defect(kind: DefectKind, host: Option<&str>, lane: Option<&str>, now: i64) -> Defect {
    Defect {
        kind,
        source: "relay".to_owned(),
        host: host.map(str::to_owned),
        lane: lane.map(str::to_owned),
        reason: format!("{kind:?}"),
        observed_ms: now,
        first_seen_ms: None,
    }
}

struct Rig {
    dir: tempfile::TempDir,
    config: FleetConfig,
    settings: TickSettings,
    orders: StandingOrders,
    facts: TickFacts,
    focus: BTreeMap<String, String>,
}

impl Rig {
    fn new(interval_secs: u64) -> Self {
        let mut settings = TickSettings {
            orchestrator: Some(ORCH.to_owned()),
            ..TickSettings::default()
        };
        settings.leads.insert(LANE.to_owned(), LEAD.to_owned());
        let dir = tempfile::tempdir().unwrap();
        let config = FleetConfig {
            tick_interval: Duration::from_secs(interval_secs),
            ledger_path: Some(dir.path().join("ledger.sqlite3")),
            ..FleetConfig::default()
        };
        Self {
            dir,
            config,
            settings,
            orders: StandingOrders::Missing,
            facts: TickFacts::default(),
            focus: BTreeMap::new(),
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join("ledger.sqlite3")
    }

    fn assign(&mut self, lane: &str, tickets: &[&str]) {
        self.facts
            .assignments
            .get_or_insert_with(BTreeMap::new)
            .insert(
                lane.to_lowercase(),
                tickets.iter().map(|t| (*t).to_owned()).collect(),
            );
    }

    fn tick(&self, report: &FleetReport) -> TickOutput {
        let mut ledger = Ledger::open(&self.path(), &self.config).unwrap();
        ledger
            .commit_tick(&TickCommit {
                report,
                settings: &self.settings,
                facts: &self.facts,
                orders: &self.orders,
                session: None,
                focus: &self.focus,
            })
            .unwrap()
    }

    fn sql<T>(&self, query: &str, map: impl Fn(&rusqlite::Row<'_>) -> T) -> Vec<T> {
        let connection = Connection::open(self.path()).unwrap();
        let mut statement = connection.prepare(query).unwrap();
        statement
            .query_map([], |r| Ok(map(r)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

fn of(out: &TickOutput, kind: ActionKind) -> Vec<&Action> {
    out.actions.iter().filter(|a| a.kind == kind).collect()
}

fn one(out: &TickOutput, kind: ActionKind) -> &Action {
    let found = of(out, kind);
    assert_eq!(found.len(), 1, "{kind:?} in {:?}", out.actions);
    found[0]
}

fn rungs(out: &TickOutput) -> Vec<(u8, Option<String>, String)> {
    of(out, ActionKind::LadderRung)
        .iter()
        .map(|a| (a.rung.unwrap(), a.target.addr.clone(), a.reason.clone()))
        .collect()
}

fn stuck_lane() -> LaneRow {
    row(LANE, ClassKind::Stuck, None, None)
}

#[trace("FR-038-AC-1")]
#[test]
fn each_action_kind_is_emitted_by_its_condition_and_not_by_its_negation() {
    let mut rig = Rig::new(300);
    // push_next_ticket: idle with an assigned ticket, not idle without one.
    let out = rig.tick(&report(T0, vec![idle(LANE)], vec![]));
    assert!(of(&out, ActionKind::PushNextTicket).is_empty());
    rig.assign(LANE, &["SWM-1", "SWM-2"]);
    let out = rig.tick(&report(T0 + 1000, vec![idle(LANE)], vec![]));
    let push = one(&out, ActionKind::PushNextTicket);
    assert_eq!(push.detail["ticket"], "SWM-1");
    assert_eq!(push.detail["assignment_list"]["source"], "ix-relay FR-072");
    assert_eq!(push.target.role, "lane");
    // resend_order only for STALE-ORDER.
    let stale = row(LANE, ClassKind::StaleOrder, Some("ack_expired"), None);
    let out = rig.tick(&report(T0 + 2000, vec![stale], vec![]));
    assert_eq!(
        one(&out, ActionKind::ResendOrder).target.addr.as_deref(),
        Some(LANE)
    );
    assert!(of(&out, ActionKind::PushNextTicket).is_empty());
    // escalate_stuck for STUCK, nothing for WORKING.
    let out = rig.tick(&report(T0 + 3000, vec![stuck_lane()], vec![]));
    assert_eq!(of(&out, ActionKind::EscalateStuck).len(), 1);
    let out = rig.tick(&report(
        T0 + 4000,
        vec![row(LANE, ClassKind::Working, None, None)],
        vec![],
    ));
    assert!(out.actions.is_empty());
}

#[trace("FR-038-AC-1", "FR-038-AC-5")]
#[test]
fn a_pull_request_is_stale_only_beyond_stale_pr_after_and_goes_to_the_lane_lead() {
    let mut rig = Rig::new(300);
    let three_hours = 3 * 3600 * 1000;
    let prs = |last: i64| TickFacts {
        prs: Some(vec![LanePr {
            lane: LANE.to_owned(),
            pr: "agent-ix/ix-board#9".to_owned(),
            last_activity_ms: last,
        }]),
        ..TickFacts::default()
    };
    let now = T0 + 10 * three_hours;
    let working = || vec![row(LANE, ClassKind::Working, None, None)];
    rig.facts = prs(now - three_hours);
    let out = rig.tick(&report(now, working(), vec![]));
    assert!(
        of(&out, ActionKind::EscalateStalePr).is_empty(),
        "exactly 3h is not stale"
    );
    rig.facts = prs(now - three_hours - 1);
    let out = rig.tick(&report(now + 1000, working(), vec![]));
    let stale = one(&out, ActionKind::EscalateStalePr);
    assert_eq!(
        (stale.target.role, stale.target.addr.as_deref()),
        ("lead", Some(LEAD))
    );
    // A configured value overrides the default.
    rig.settings.stale_pr_after = Duration::from_secs(3600);
    rig.facts = prs(now + 2000 - 3600 * 1000);
    let out = rig.tick(&report(now + 2000, working(), vec![]));
    assert!(of(&out, ActionKind::EscalateStalePr).is_empty());
    rig.facts = prs(now + 3000 - 3600 * 1000 - 1);
    let out = rig.tick(&report(now + 3000, working(), vec![]));
    assert_eq!(of(&out, ActionKind::EscalateStalePr).len(), 1);
}

#[trace("FR-038-AC-2")]
#[test]
fn identical_inputs_give_identical_lists_and_ids_persist_while_the_condition_persists() {
    let build = || {
        let mut rig = Rig::new(300);
        rig.assign(LANE, &["SWM-1"]);
        rig
    };
    let rows = || vec![idle(LANE), stuck_lane_at("h:w:2"), idle("h:w:3")];
    let (a, b) = (build(), build());
    let first_a = a.tick(&report(T0, rows(), vec![]));
    let first_b = b.tick(&report(T0, rows(), vec![]));
    assert_eq!(
        serde_json::to_string(&first_a).unwrap(),
        serde_json::to_string(&first_b).unwrap()
    );
    let order: Vec<_> = first_a
        .actions
        .iter()
        .map(|x| (x.severity, x.subject.clone()))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted, "severity then lane");
    let second = a.tick(&report(T0 + 300_000, rows(), vec![]));
    let ids = |o: &TickOutput| {
        o.actions
            .iter()
            .filter(|x| x.kind != ActionKind::LadderRung)
            .map(|x| (x.kind, x.subject.clone(), x.id, x.condition_started_at))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&first_a), ids(&second));
}

fn stuck_lane_at(lane: &str) -> LaneRow {
    row(lane, ClassKind::Stuck, None, None)
}

fn blocked(reason: &str, relay: &str) -> LaneRow {
    row(LANE, ClassKind::Blocked, Some(reason), Some(relay))
}

#[trace("FR-038-AC-3", "FR-038-AC-18")]
#[test]
fn answer_dialog_needs_a_readable_standing_order_file_and_a_queued_question_qualifies() {
    let mut rig = Rig::new(300);
    let dialog = || vec![blocked("dialog", "permission")];
    let queued = || vec![blocked("question", "queued_question")];
    let free = || vec![blocked("question", "question")];
    // Missing and unreadable files never answer.
    for orders in [
        StandingOrders::Missing,
        StandingOrders::Unreadable("denied".to_owned()),
    ] {
        rig.orders = orders;
        let out = rig.tick(&report(T0, dialog(), vec![]));
        assert!(of(&out, ActionKind::AnswerDialog).is_empty());
        assert_eq!(of(&out, ActionKind::EscalateBlock).len(), 1);
    }
    let unreadable = StandingOrders::load(Some(rig.dir.path()));
    assert!(matches!(unreadable, StandingOrders::Unreadable(_)));
    assert_eq!(
        StandingOrders::load(Some(&rig.dir.path().join("none.toml"))),
        StandingOrders::Missing
    );
    let file = rig.dir.path().join("orders.toml");
    std::fs::write(&file, "").unwrap();
    rig.orders = StandingOrders::load(Some(&file));
    assert_eq!(rig.orders, StandingOrders::Present);
    let out = rig.tick(&report(T0 + 1000, dialog(), vec![]));
    let answer = one(&out, ActionKind::AnswerDialog);
    assert_eq!(answer.detail["relay_reason"], "permission");
    assert!(of(&out, ActionKind::EscalateBlock).is_empty());
    let out = rig.tick(&report(T0 + 2000, queued(), vec![]));
    assert_eq!(
        one(&out, ActionKind::AnswerDialog).detail["relay_reason"],
        "queued_question"
    );
    // A free-form question is never answered, whatever the file holds.
    let out = rig.tick(&report(T0 + 3000, free(), vec![]));
    assert!(of(&out, ActionKind::AnswerDialog).is_empty());
    assert_eq!(one(&out, ActionKind::EscalateBlock).reason, "question");
}

#[trace("FR-038-AC-4")]
#[test]
fn blocks_go_to_the_lead_with_high_severity_and_stuck_and_loop_escalate_stuck() {
    let rig = Rig::new(300);
    for (reason, relay) in [
        ("login", "login"),
        ("usage_limit", "usage_limit"),
        ("question", "question"),
    ] {
        let out = rig.tick(&report(T0, vec![blocked(reason, relay)], vec![]));
        let block = one(&out, ActionKind::EscalateBlock);
        assert_eq!(
            (block.target.role, block.target.addr.as_deref()),
            ("lead", Some(LEAD))
        );
        assert_eq!(block.severity.name(), "high");
        assert_eq!(block.reason, reason);
    }
    for class in [ClassKind::Stuck, ClassKind::Loop] {
        let out = rig.tick(&report(T0 + 1, vec![row(LANE, class, None, None)], vec![]));
        assert_eq!(
            one(&out, ActionKind::EscalateStuck).target.addr.as_deref(),
            Some(LEAD)
        );
    }
}

fn host_facts(busy: f64, unassigned: usize) -> TickFacts {
    TickFacts {
        host_busy: Some(BTreeMap::from([("h".to_owned(), busy)])),
        ready: Some(ReadyWork {
            unassigned,
            lanes: BTreeSet::new(),
        }),
        ..TickFacts::default()
    }
}

fn working(lane: &str) -> LaneRow {
    row(lane, ClassKind::Working, None, None)
}

#[trace("FR-038-AC-6")]
#[test]
fn a_host_below_its_busy_target_with_ready_work_and_a_free_slot_gets_add_lane() {
    let mut rig = Rig::new(300);
    rig.settings.lane_capacity.insert("h".to_owned(), 3);
    let level = |rig: &Rig, n: i64, at: i64| {
        let connection = Connection::open(rig.path()).unwrap();
        connection
            .execute(
                "INSERT INTO capacity_level(host, level, recorded_at) VALUES('h', ?1, ?2)",
                [n, at],
            )
            .unwrap();
    };
    let rows = || vec![working("h:w:1")];
    rig.facts = host_facts(0.84, 2);
    rig.tick(&report(T0, rows(), vec![])); // creates the ledger
    level(&rig, 3, T0);
    let out = rig.tick(&report(T0 + 1000, rows(), vec![]));
    let add = one(&out, ActionKind::AddLane);
    assert_eq!(
        (add.target.role, add.target.addr.as_deref()),
        ("host", Some("h"))
    );
    // At the default 0.85 target, without ready work or on a configured target it does not.
    rig.facts = host_facts(0.85, 2);
    assert!(
        of(
            &rig.tick(&report(T0 + 2000, rows(), vec![])),
            ActionKind::AddLane
        )
        .is_empty()
    );
    rig.facts = host_facts(0.84, 0);
    assert!(
        of(
            &rig.tick(&report(T0 + 3000, rows(), vec![])),
            ActionKind::AddLane
        )
        .is_empty()
    );
    rig.facts = host_facts(0.84, 2);
    rig.settings.busy_targets.insert("h".to_owned(), 0.8);
    assert!(
        of(
            &rig.tick(&report(T0 + 4000, rows(), vec![])),
            ActionKind::AddLane
        )
        .is_empty()
    );
}

#[trace("FR-038-AC-14")]
#[test]
fn add_lane_is_suppressed_with_a_recorded_reason_and_draining_lanes_do_not_count() {
    let mut rig = Rig::new(300);
    rig.settings.lane_capacity.insert("h".to_owned(), 2);
    rig.facts = host_facts(0.1, 3);
    let rows = || vec![working("h:w:1")];
    let exec = |rig: &Rig, sql: &str| {
        Connection::open(rig.path())
            .unwrap()
            .execute(sql, [])
            .unwrap()
    };
    let mut now = T0;
    let mut next = |rig: &Rig| {
        now += 1000;
        rig.tick(&report(now, rows(), vec![]))
    };
    let out = next(&rig);
    assert!(of(&out, ActionKind::AddLane).is_empty());
    assert_eq!(out.suppressed[0].reason, "no_recorded_level");
    exec(
        &rig,
        "INSERT INTO capacity_level(host, level, recorded_at) VALUES('h', 1, 1)",
    );
    assert_eq!(next(&rig).suppressed[0].reason, "at_recorded_level");
    exec(&rig, "UPDATE capacity_level SET level = 2");
    exec(
        &rig,
        &format!(
            "INSERT INTO capacity_event(host, kind, at, ceiling_until) VALUES('h','step_down',1,{})",
            T0 + 10_000_000
        ),
    );
    assert_eq!(next(&rig).suppressed[0].reason, "cool_off");
    exec(
        &rig,
        "INSERT INTO capacity_run(host, started_at) VALUES('h', 1)",
    );
    assert_eq!(next(&rig).suppressed[0].reason, "discovery_active");
    let recorded = rig.sql("SELECT detail FROM audit_sample ORDER BY at", |r| {
        r.get::<_, String>(0).unwrap()
    });
    assert_eq!(recorded.len(), 4);
    assert!(recorded[3].contains("discovery_active") && recorded[3].contains("\"host\":\"h\""));
    exec(&rig, "UPDATE capacity_run SET ended_at = 2");
    exec(&rig, "DELETE FROM capacity_event");
    let out = next(&rig);
    assert_eq!(of(&out, ActionKind::AddLane).len(), 1);
    // A draining lane counts as neither a lane nor a free slot.
    exec(
        &rig,
        "INSERT INTO capacity_event(host, lane, kind, at) VALUES('h','h:w:2','draining_mark',1)",
    );
    now += 1000;
    let two = vec![working("h:w:1"), working("h:w:2")];
    let out = rig.tick(&report(now, two, vec![]));
    assert_eq!(
        of(&out, ActionKind::AddLane).len(),
        1,
        "h:w:2 drains, one lane, one free slot"
    );
}

#[trace("FR-038-AC-15")]
#[test]
fn a_lane_draining_past_the_cap_emits_escalate_drain_to_its_lead() {
    let rig = Rig::new(300);
    let cap = 4 * 3600 * 1000;
    let mark = |at: i64| {
        let connection = Connection::open(rig.path()).unwrap();
        connection
            .execute("DELETE FROM capacity_event", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO capacity_event(host, lane, kind, at) VALUES('h','h:w:1','draining_mark',?1)",
                [at],
            )
            .unwrap();
    };
    let now = T0 + 10 * cap;
    rig.tick(&report(now - 1, vec![working(LANE)], vec![]));
    mark(now - cap);
    let out = rig.tick(&report(now, vec![working(LANE)], vec![]));
    assert!(
        of(&out, ActionKind::EscalateDrain).is_empty(),
        "exactly the cap is within it"
    );
    mark(now - cap - 1);
    let out = rig.tick(&report(now + 1000, vec![working(LANE)], vec![]));
    assert_eq!(
        one(&out, ActionKind::EscalateDrain).target.addr.as_deref(),
        Some(LEAD)
    );
}

#[trace("FR-038-AC-9", "FR-038-AC-10")]
#[test]
fn source_defects_become_fix_source_in_the_tick_they_appear_and_a_stale_host_never_gets_undetermined()
 {
    let mut rig = Rig::new(300);
    rig.assign(LANE, &["SWM-1"]);
    let rows = || vec![idle(LANE), row("u:w:1", ClassKind::Unreachable, None, None)];
    let defects = |now| {
        vec![
            defect(DefectKind::NoEvents, Some("h"), Some(LANE), now),
            defect(DefectKind::NoEvents, Some("u"), Some("u:w:1"), now),
            defect(DefectKind::StaleHostRegistry, Some("u"), None, now),
        ]
    };
    let out = rig.tick(&report(T0, rows(), defects(T0)));
    let undetermined = one(&out, ActionKind::ActOnUndetermined);
    assert_eq!(undetermined.subject, LANE);
    assert_eq!(undetermined.detail["treat_as"], "idle");
    assert_eq!(
        undetermined.detail["class"], "IDLE",
        "the FR-034 class is kept"
    );
    assert_eq!(undetermined.detail["remaining_ticket"], "SWM-1");
    let fixes = of(&out, ActionKind::FixSource);
    let first_ids: Vec<_> = fixes.iter().map(|f| f.id).collect();
    let again = rig.tick(&report(T0 + 1000, rows(), defects(T0 + 1000)));
    let again_ids: Vec<_> = of(&again, ActionKind::FixSource)
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(
        first_ids, again_ids,
        "the id persists while the defect does"
    );
    assert_eq!(fixes.len(), 3);
    assert!(
        fixes
            .iter()
            .all(|f| (f.target.role, f.target.addr.as_deref()) == ("orchestrator", Some(ORCH)))
    );
    // The lane's other actions are still emitted, and the stale host's lane has no undetermined.
    assert_eq!(of(&out, ActionKind::PushNextTicket).len(), 1);
    assert_eq!(of(&out, ActionKind::ActOnUndetermined).len(), 1);
    assert!(
        of(&out, ActionKind::ActOnUndetermined)
            .iter()
            .all(|a| a.subject == LANE)
    );
}

fn ladder(interval: u64) {
    let rig = Rig::new(interval);
    let step = i64::try_from(interval).unwrap() * 1000;
    let tick = |n: i64| rig.tick(&report(T0 + n * step, vec![stuck_lane()], vec![]));
    let first = tick(0);
    let id = one(&first, ActionKind::EscalateStuck).id;
    assert!(rungs(&first).is_empty(), "rung 0 is the action itself");
    assert!(rungs(&tick(0).clone()).is_empty());
    let r1 = rungs(&tick(1));
    assert_eq!(r1, vec![(1, Some(LANE.to_owned()), "worker".to_owned())]);
    let r2 = rungs(&tick(2));
    assert_eq!(r2, vec![(2, Some(LEAD.to_owned()), "lead".to_owned())]);
    // A missed tick: the next tick is at 4 intervals, the condition persists, rung 3 is due.
    let late = tick(4);
    assert_eq!(
        rungs(&late),
        vec![(3, Some(ORCH.to_owned()), "orchestrator".to_owned())]
    );
    let a = one(&late, ActionKind::EscalateStuck);
    assert_eq!((a.id, a.condition_started_at), (id, T0));
    assert_eq!(
        of(&late, ActionKind::LadderRung)[0].parent_action_id,
        Some(id)
    );
    assert!(rungs(&tick(5)).is_empty(), "each rung is emitted once");
}

#[trace("FR-038-AC-11")]
#[test]
fn the_ladder_sends_each_rung_once_at_one_two_and_three_intervals_at_both_intervals() {
    ladder(300);
    ladder(120);
}

#[trace("FR-038-AC-11")]
#[test]
fn a_rung_is_emitted_in_the_first_tick_at_or_after_its_time() {
    let rig = Rig::new(300);
    rig.tick(&report(T0, vec![stuck_lane()], vec![]));
    let early = rig.tick(&report(T0 + 299_999, vec![stuck_lane()], vec![]));
    assert!(rungs(&early).is_empty());
    let exact = rig.tick(&report(T0 + 300_000, vec![stuck_lane()], vec![]));
    assert_eq!(rungs(&exact).len(), 1);
    // After a long gap the overdue rungs 2 and 3 come together, once.
    let gap = rig.tick(&report(T0 + 3_000_000, vec![stuck_lane()], vec![]));
    let mut numbers: Vec<u8> = rungs(&gap).iter().map(|r| r.0).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, vec![2, 3]);
}

#[trace("FR-038-AC-12")]
#[test]
fn rungs_ignore_earlier_messages_end_with_the_condition_and_never_address_the_owner() {
    let mut rig = Rig::new(300);
    // The lead already receives rung 0; rung 2 goes to the same lead anyway.
    let mut all = Vec::new();
    for n in 0..=3 {
        all.push(rig.tick(&report(T0 + n * 300_000, vec![stuck_lane()], vec![])));
    }
    assert_eq!(all[0].actions[0].target.addr.as_deref(), Some(LEAD));
    assert_eq!(rungs(&all[2])[0].1.as_deref(), Some(LEAD));
    let cleared = rig.tick(&report(T0 + 4 * 300_000, vec![working(LANE)], vec![]));
    assert!(cleared.actions.is_empty());
    let after = rig.tick(&report(T0 + 9 * 300_000, vec![working(LANE)], vec![]));
    assert!(after.actions.is_empty());
    let open = rig.sql(
        "SELECT COUNT(*) FROM action WHERE resolved_at IS NULL",
        |r| r.get::<_, i64>(0).unwrap(),
    );
    assert_eq!(open, vec![0], "rung rows resolve with their condition");
    for out in &all {
        for action in &out.actions {
            assert!(
                ["lane", "lead", "orchestrator", "planner", "host"].contains(&action.target.role)
            );
        }
    }
    // Plant the addresses of every non-recipient the configuration names (the minder that runs
    // the tick) and read back every address the ledger recorded: only lanes, leads, the
    // orchestrator and hosts may ever be recipients.
    rig.settings.minder = Some(MINDER.to_owned());
    rig.settings
        .leads
        .insert("h:w:2".to_owned(), "lead:w:2".to_owned());
    for n in 10..=14 {
        rig.tick(&report(
            T0 + n * 300_000,
            vec![
                stuck_lane(),
                stuck_lane_at("h:w:2"),
                row("h:w:3", ClassKind::Dead, None, None),
            ],
            vec![],
        ));
    }
    let addrs = rig.sql(
        "SELECT DISTINCT target_addr FROM action WHERE target_addr IS NOT NULL",
        |r| r.get::<_, String>(0).unwrap(),
    );
    let allowed = [LANE, "h:w:2", "h:w:3", LEAD, "lead:w:2", ORCH];
    assert!(
        addrs.iter().all(|a| allowed.contains(&a.as_str())),
        "{addrs:?}"
    );
    assert!(!addrs.iter().any(|a| a == MINDER));
    let roles = rig.sql("SELECT DISTINCT target_role FROM action", |r| {
        r.get::<_, String>(0).unwrap()
    });
    assert!(
        roles
            .iter()
            .all(|r| ["lane", "lead", "orchestrator", "planner", "host"].contains(&r.as_str()))
    );
}

#[trace("FR-038-AC-13", "FR-038-AC-16")]
#[test]
fn a_lane_with_no_lead_gets_the_orchestrator_as_owner_and_rung_three_falls_through() {
    let mut rig = Rig::new(300);
    rig.settings.leads.clear();
    rig.facts = TickFacts {
        prs: Some(vec![LanePr {
            lane: LANE.to_owned(),
            pr: "a/b#1".to_owned(),
            last_activity_ms: 0,
        }]),
        ..TickFacts::default()
    };
    let first = rig.tick(&report(T0, vec![stuck_lane()], vec![]));
    for kind in [ActionKind::EscalateStuck, ActionKind::EscalateStalePr] {
        let a = one(&first, kind);
        assert_eq!(
            (a.target.role, a.target.addr.as_deref()),
            ("orchestrator", Some(ORCH))
        );
        assert_eq!(a.reason, "lead_unresolved");
    }
    let r1 = rig.tick(&report(T0 + 300_000, vec![stuck_lane()], vec![]));
    assert_eq!(
        rungs(&r1).iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![1, 1]
    );
    let r2 = rig.tick(&report(T0 + 600_000, vec![stuck_lane()], vec![]));
    let sent: Vec<_> = rungs(&r2);
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter()
            .all(|r| r.0 == 3 && r.2 == "lead_unresolved_fallthrough")
    );
    assert_eq!(
        r2.suppressed
            .iter()
            .filter(|s| s.reason == "lead_unresolved")
            .count(),
        2
    );
    let recorded = rig.sql(
        "SELECT rung, target_role, target_addr FROM action WHERE kind='ladder_rung' AND rung=2",
        |r| {
            (
                r.get::<_, i64>(0).unwrap(),
                r.get::<_, String>(1).unwrap(),
                r.get::<_, Option<String>>(2).unwrap(),
            )
        },
    );
    assert_eq!(recorded.len(), 2);
    assert!(recorded.iter().all(|r| r.1 == "lead" && r.2.is_none()));
    let r3 = rig.tick(&report(T0 + 900_000, vec![stuck_lane()], vec![]));
    assert!(
        rungs(&r3).is_empty(),
        "rung 3 is not emitted again at 3 intervals"
    );
}

#[trace("FR-038-AC-17")]
#[test]
fn a_blocked_lane_skips_rung_one_and_gets_rungs_two_and_three() {
    let mut rig = Rig::new(300);
    rig.settings.leads.insert(LANE.to_owned(), LEAD.to_owned());
    let lane = || vec![blocked("login", "login")];
    rig.tick(&report(T0, lane(), vec![]));
    let one_interval = rig.tick(&report(T0 + 300_000, lane(), vec![]));
    assert!(
        rungs(&one_interval).is_empty(),
        "no relay message to a blocked pane"
    );
    let two = rig.tick(&report(T0 + 600_000, lane(), vec![]));
    assert_eq!(
        rungs(&two),
        vec![(2, Some(LEAD.to_owned()), "lead".to_owned())]
    );
    let three = rig.tick(&report(T0 + 900_000, lane(), vec![]));
    assert_eq!(
        rungs(&three),
        vec![(3, Some(ORCH.to_owned()), "orchestrator".to_owned())]
    );
    let sent_to_lane = rig.sql(
        "SELECT COUNT(*) FROM action WHERE kind='ladder_rung' AND target_role='lane'",
        |r| r.get::<_, i64>(0).unwrap(),
    );
    assert_eq!(sent_to_lane, vec![0]);
    // A lane that is not blocked still gets rung 1.
    let other = Rig::new(300);
    other.tick(&report(T0, vec![stuck_lane()], vec![]));
    assert_eq!(
        rungs(&other.tick(&report(T0 + 300_000, vec![stuck_lane()], vec![])))[0].0,
        1
    );
}

#[trace("FR-038-AC-7")]
#[test]
fn the_tick_metric_reads_the_last_committed_tick_and_is_omitted_when_older_than_two_intervals() {
    let mut rig = Rig::new(300);
    let interval = 300_000;
    assert!(
        tick_metrics(&rig.path(), T0, interval).is_err(),
        "no ledger yet"
    );
    rig.assign(LANE, &["SWM-1"]);
    rig.tick(&report(
        T0,
        vec![idle(LANE), stuck_lane_at("h:w:2")],
        vec![],
    ));
    let text = tick_metrics(&rig.path(), T0 + 1000, interval).unwrap();
    assert!(text.contains(&format!("ix_fleet_tick_timestamp_seconds {}", T0 / 1000)));
    assert!(text.contains("ix_fleet_tick_actions{kind=\"push_next_ticket\"} 1"));
    assert!(text.contains("ix_fleet_tick_actions{kind=\"escalate_stuck\"} 1"));
    assert!(text.contains("ix_fleet_tick_actions{kind=\"fix_source\"} 0"));
    let edge = tick_metrics(&rig.path(), T0 + 2 * interval, interval).unwrap();
    assert!(
        !edge.is_empty(),
        "exactly twice the interval is still served"
    );
    assert!(
        tick_metrics(&rig.path(), T0 + 2 * interval + 1, interval)
            .unwrap()
            .is_empty()
    );
    // A persisting condition counts again in the next tick, a cleared one does not.
    rig.tick(&report(
        T0 + interval,
        vec![idle(LANE), working("h:w:2")],
        vec![],
    ));
    let text = tick_metrics(&rig.path(), T0 + interval, interval).unwrap();
    assert!(text.contains("ix_fleet_tick_actions{kind=\"push_next_ticket\"} 1"));
    assert!(text.contains("ix_fleet_tick_actions{kind=\"escalate_stuck\"} 0"));
}

#[trace("FR-038-AC-7")]
#[test]
fn a_tick_that_cannot_evaluate_commits_no_record() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.sqlite3");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    let out = assert_cmd::Command::cargo_bin("ix-board")
        .unwrap()
        .env("IX_BOARD_CONFIG_PATH", &config)
        .env("IX_RELAY_URL", "redis://127.0.0.1:1")
        .env("IX_BOARD_LEDGER_PATH", &ledger)
        .env_remove("IX_PROMETHEUS_URL")
        .args(["--json", "fleet", "tick"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty() || !String::from_utf8_lossy(&out.stdout).contains("\"actions\""));
    if ledger.exists() {
        let ticks: i64 = Connection::open(&ledger)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM tick", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ticks, 0);
    }
}

struct Recording {
    calls: Cell<u32>,
}

impl TickSources for Recording {
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, Unread> {
        self.calls.set(self.calls.get() + 1);
        Ok(BTreeMap::new())
    }
    fn host_busy(&self) -> Result<BTreeMap<String, f64>, Unread> {
        self.calls.set(self.calls.get() + 1);
        Err(Unread::Failed(SourceError("down".to_owned())))
    }
    fn lane_prs(&self) -> Result<LanePrRead, Unread> {
        self.calls.set(self.calls.get() + 1);
        Err(Unread::Unwired("SWM-282"))
    }
    fn ready_work(&self) -> Result<ReadyWork, Unread> {
        self.calls.set(self.calls.get() + 1);
        Ok(ReadyWork::default())
    }
    fn need_ready_tickets(&self) -> Result<NeedReadyTicketsRead, Unread> {
        self.calls.set(self.calls.get() + 1);
        Ok(NeedReadyTicketsRead {
            complete: true,
            ..NeedReadyTicketsRead::default()
        })
    }
    fn capacity_throttle_samples(
        &self,
        _: i64,
        _: u64,
    ) -> Vec<ix_board::fleet::tick::CapacityThrottleSample> {
        self.calls.set(self.calls.get() + 1);
        Vec::new()
    }
}

#[trace("FR-038-AC-8")]
#[test]
fn the_tick_reads_each_source_once_writes_only_the_ledger_and_names_an_unread_source() {
    // The source trait has read methods only: a tick has no way to write to the relay, Linear
    // or GitHub. The recording fake sees exactly one read per source.
    let sources = Recording {
        calls: Cell::new(0),
    };
    let facts = TickFacts::read(&sources, 1_000, 300);
    assert_eq!(sources.calls.get(), 6);
    assert_eq!(facts.unavailable.len(), 2);
    assert_eq!(facts.unavailable[0].source, "host_busy");
    assert!(!facts.unavailable[0].unwired, "a failed read");
    assert_eq!(facts.unavailable[1].source, "lane_prs");
    assert!(facts.unavailable[1].unwired, "a named seam, not a failure");
    assert!(facts.host_busy.is_none() && facts.assignments.is_some());
    let mut rig = Rig::new(300);
    rig.facts = facts;
    let out = rig.tick(&report(T0, vec![idle(LANE)], vec![]));
    assert_eq!(out.unavailable.len(), 2);
    // Only the failed read is a source defect, routed to the orchestrator in this tick.
    assert!(!out.complete);
    let fixes = of(&out, ActionKind::FixSource);
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].detail["source"], "tick.host_busy");
    assert_eq!(fixes[0].target.addr.as_deref(), Some(ORCH));
}

fn executed_of(rig: &Rig, kind: ActionKind) -> Vec<(Option<i64>, Option<i64>)> {
    rig.sql(
        &format!(
            "SELECT executed_at, resolved_at FROM action WHERE kind = '{}' ORDER BY id",
            kind.name()
        ),
        |r| (r.get(0).unwrap(), r.get(1).unwrap()),
    )
}

#[trace("FR-039-AC-6")]
#[test]
fn executed_at_needs_positive_evidence_for_each_kind() {
    let mut rig = Rig::new(300);
    let file = rig.dir.path().join("orders.toml");
    std::fs::write(&file, "").unwrap();
    rig.orders = StandingOrders::load(Some(&file));
    // answer_dialog: executed when the lane leaves BLOCKED after the answer was emitted.
    rig.tick(&report(T0, vec![blocked("dialog", "permission")], vec![]));
    rig.tick(&report(T0 + 1000, vec![working(LANE)], vec![]));
    assert_eq!(
        executed_of(&rig, ActionKind::AnswerDialog),
        vec![(Some(T0 + 1000), Some(T0 + 1000))]
    );
    // A fallback to escalate_block (the order file vanished) is not execution.
    rig.tick(&report(
        T0 + 2000,
        vec![blocked("dialog", "permission")],
        vec![],
    ));
    rig.orders = StandingOrders::Missing;
    let out = rig.tick(&report(
        T0 + 3000,
        vec![blocked("dialog", "permission")],
        vec![],
    ));
    assert_eq!(of(&out, ActionKind::EscalateBlock).len(), 1);
    assert_eq!(
        executed_of(&rig, ActionKind::AnswerDialog)[1],
        (None, Some(T0 + 3000))
    );
    // An escalation is aimed at a lead: no executed_at, and it resolves when the block clears.
    rig.tick(&report(T0 + 4000, vec![working(LANE)], vec![]));
    assert_eq!(
        executed_of(&rig, ActionKind::EscalateBlock),
        vec![(None, Some(T0 + 4000))]
    );
    // act_on_undetermined: executed once the lane's evidence is determined again.
    let flawed = || vec![defect(DefectKind::NoEvents, Some("h"), Some(LANE), T0)];
    rig.tick(&report(T0 + 5000, vec![idle(LANE)], flawed()));
    rig.tick(&report(T0 + 6000, vec![working(LANE)], vec![]));
    assert_eq!(
        executed_of(&rig, ActionKind::ActOnUndetermined),
        vec![(Some(T0 + 6000), Some(T0 + 6000))]
    );
    // ... but not when the lane merely became unreachable.
    rig.tick(&report(T0 + 7000, vec![idle(LANE)], flawed()));
    rig.tick(&report(
        T0 + 8000,
        vec![row(LANE, ClassKind::Unreachable, None, None)],
        vec![],
    ));
    assert_eq!(
        executed_of(&rig, ActionKind::ActOnUndetermined)[1],
        (None, Some(T0 + 8000))
    );
    // push_next_ticket: executed when the lane WORKS with its focus on the pushed ticket, never
    // for any WORKING lane.
    rig.assign(LANE, &["SWM-1"]);
    rig.tick(&report(T0 + 9000, vec![idle(LANE)], vec![]));
    rig.focus.insert(LANE.to_owned(), "SWM-9".to_owned());
    rig.tick(&report(T0 + 10_000, vec![working(LANE)], vec![]));
    assert_eq!(
        executed_of(&rig, ActionKind::PushNextTicket),
        vec![(None, Some(T0 + 10_000))],
        "working on another ticket is not execution"
    );
    rig.tick(&report(T0 + 10_500, vec![idle(LANE)], vec![]));
    rig.focus.insert(LANE.to_owned(), "swm-1".to_owned());
    rig.tick(&report(T0 + 11_000, vec![working(LANE)], vec![]));
    assert_eq!(
        executed_of(&rig, ActionKind::PushNextTicket)[1],
        (Some(T0 + 11_000), Some(T0 + 11_000))
    );
    // resend_order needs an Acted receipt join (deferred to SWM-255): no executed_at.
    rig.focus.clear();
    rig.tick(&report(
        T0 + 12_000,
        vec![row(LANE, ClassKind::StaleOrder, Some("ack_expired"), None)],
        vec![],
    ));
    rig.tick(&report(T0 + 13_000, vec![working(LANE)], vec![]));
    assert_eq!(
        executed_of(&rig, ActionKind::ResendOrder),
        vec![(None, Some(T0 + 13_000))]
    );
}

#[trace("FR-039-AC-11")]
#[test]
fn tick_rows_carry_ready_work_and_win_over_a_status_row_at_the_same_relay_time() {
    let mut rig = Rig::new(300);
    rig.assign(LANE, &["SWM-1"]);
    rig.assign("h:w:2", &[]);
    rig.facts.ready = Some(ReadyWork {
        unassigned: 1,
        lanes: BTreeSet::from(["h:w:3".to_owned()]),
    });
    let rows = || vec![idle(LANE), idle("h:w:2"), idle("h:w:3"), idle("h:w:4")];
    {
        let mut ledger = Ledger::open(&rig.path(), &rig.config).unwrap();
        ledger.record_status(&report(T0, rows(), vec![])).unwrap();
    }
    rig.tick(&report(T0, rows(), vec![]));
    let ticks = rig.sql(
        "SELECT lane, source, remaining_assigned, ready_work_available FROM lane_tick
         WHERE source = 'tick' ORDER BY lane",
        |r| {
            (
                r.get::<_, String>(0).unwrap(),
                r.get::<_, Option<i64>>(2).unwrap(),
                r.get::<_, Option<i64>>(3).unwrap(),
            )
        },
    );
    assert_eq!(
        ticks,
        vec![
            (LANE.to_owned(), Some(1), Some(1)),
            ("h:w:2".to_owned(), Some(0), Some(0)),
            ("h:w:3".to_owned(), Some(0), Some(1)),
            ("h:w:4".to_owned(), Some(0), Some(0)),
        ]
    );
    // Status and tick rows at the same relay time count once, the tick row winning.
    let idle_ready = rig.sql(
        "SELECT lane, source FROM idle_lane_with_ready_work_ticks ORDER BY lane",
        |r| {
            (
                r.get::<_, String>(0).unwrap(),
                r.get::<_, String>(1).unwrap(),
            )
        },
    );
    assert_eq!(
        idle_ready,
        vec![
            (LANE.to_owned(), "tick".to_owned()),
            ("h:w:3".to_owned(), "tick".to_owned())
        ]
    );
    let status_rows = rig.sql(
        "SELECT COUNT(*) FROM lane_tick_rows WHERE lane = 'h:w:4'",
        |r| r.get::<_, i64>(0).unwrap(),
    );
    assert_eq!(status_rows, vec![1]);
    // With ready work unreadable a lane without a ticket is unknown, not "no ready work".
    let mut other = Rig::new(300);
    other.assign(LANE, &[]);
    other.tick(&report(T0, vec![idle(LANE)], vec![]));
    let unknown = other.sql("SELECT ready_work_available FROM lane_tick", |r| {
        r.get::<_, Option<i64>>(0).unwrap()
    });
    assert_eq!(unknown, vec![None]);
}

#[trace("FR-039-AC-14")]
#[test]
fn a_tick_records_its_session_address_and_configured_role() {
    let mut rig = Rig::new(300);
    rig.settings.minder = Some(MINDER.to_owned());
    let run = |rig: &Rig, session: Option<&str>, at: i64| {
        let mut ledger = Ledger::open(&rig.path(), &rig.config).unwrap();
        ledger
            .commit_tick(&TickCommit {
                report: &report(at, vec![working(LANE)], vec![]),
                settings: &rig.settings,
                facts: &rig.facts,
                orders: &rig.orders,
                session,
                focus: &rig.focus,
            })
            .unwrap();
    };
    run(&rig, Some(MINDER), T0);
    run(&rig, Some(ORCH.to_uppercase().as_str()), T0 + 1000);
    run(&rig, Some("x:y:z"), T0 + 2000);
    let sessions = rig.sql(
        "SELECT session_addr, session_role FROM tick ORDER BY id",
        |r| {
            (
                r.get::<_, String>(0).unwrap(),
                r.get::<_, String>(1).unwrap(),
            )
        },
    );
    assert_eq!(sessions[0], (MINDER.to_owned(), "minder".to_owned()));
    assert_eq!(sessions[1].1, "orchestrator");
    assert_eq!(sessions[2], ("x:y:z".to_owned(), "other".to_owned()));
}

#[trace("FR-039-AC-14")]
#[test]
fn without_a_session_flag_the_caller_is_the_registration_of_this_pane_on_this_machine() {
    let regs = vec![
        (
            "m1".to_owned(),
            Some("w1:p1".to_owned()),
            Some("m1:ws:a".to_owned()),
        ),
        (
            "m2".to_owned(),
            Some("w1:p1".to_owned()),
            Some("m2:ws:b".to_owned()),
        ),
        ("m2".to_owned(), Some("w1:p2".to_owned()), None),
    ];
    assert_eq!(
        caller_from(&regs, "m2", "w1:p1").as_deref(),
        Some("m2:ws:b")
    );
    assert_eq!(
        caller_from(&regs, "m1", "w1:p1").as_deref(),
        Some("m1:ws:a")
    );
    assert_eq!(
        caller_from(&regs, "m2", "w1:p2"),
        None,
        "no verified address"
    );
    assert_eq!(caller_from(&regs, "m3", "w1:p1"), None);
}

#[trace("FR-038-AC-19", "FR-038-AC-20")]
#[test]
fn a_dead_lane_and_an_idle_lane_with_an_empty_list_route_to_the_orchestrator() {
    let mut rig = Rig::new(300);
    rig.assign(LANE, &[]);
    rig.assign("h:w:2", &["SWM-2"]);
    let rows = || {
        vec![
            idle(LANE),
            idle("h:w:2"),
            row("h:w:3", ClassKind::Dead, None, None),
            working("h:w:4"),
        ]
    };
    let out = rig.tick(&report(T0, rows(), vec![]));
    let idle_empty = one(&out, ActionKind::RouteIdleLane);
    assert_eq!(idle_empty.subject, LANE);
    assert_eq!(
        (idle_empty.target.role, idle_empty.target.addr.as_deref()),
        ("orchestrator", Some(ORCH))
    );
    let dead = one(&out, ActionKind::RouteDeadLane);
    assert_eq!(dead.subject, "h:w:3");
    assert_eq!(dead.target.addr.as_deref(), Some(ORCH));
    assert_eq!(dead.rung, Some(0), "rung 0 of the ladder");
    assert_eq!(
        of(&out, ActionKind::PushNextTicket).len(),
        1,
        "the lane with a ticket is pushed"
    );
    // An unreadable list is not an empty one: no idle-lane route without the list.
    let blind = Rig::new(300);
    let out = blind.tick(&report(T0, rows(), vec![]));
    assert!(of(&out, ActionKind::RouteIdleLane).is_empty());
    // The ladder follows and ends with the condition.
    let later = rig.tick(&report(T0 + 300_000, rows(), vec![]));
    assert_eq!(
        rungs(&later).len(),
        3,
        "the push, idle route and dead route each get rung 1"
    );
    let cleared = rig.tick(&report(
        T0 + 600_000,
        vec![working(LANE), working("h:w:3")],
        vec![],
    ));
    assert!(of(&cleared, ActionKind::RouteDeadLane).is_empty() && rungs(&cleared).is_empty());
}

#[trace("FR-038-AC-21")]
#[test]
fn act_on_undetermined_never_goes_to_a_blocked_lane_but_to_its_lead() {
    let rig = Rig::new(300);
    let flawed = |now| vec![defect(DefectKind::NoEvents, Some("h"), Some(LANE), now)];
    let out = rig.tick(&report(
        T0,
        vec![blocked("dialog", "permission")],
        flawed(T0),
    ));
    let undetermined = one(&out, ActionKind::ActOnUndetermined);
    assert_eq!(
        (
            undetermined.target.role,
            undetermined.target.addr.as_deref()
        ),
        ("lead", Some(LEAD))
    );
    assert_eq!(undetermined.reason, "lane_blocked_route_to_lead");
    let idle_out = rig.tick(&report(T0 + 1000, vec![idle(LANE)], flawed(T0 + 1000)));
    assert_eq!(
        one(&idle_out, ActionKind::ActOnUndetermined).target.role,
        "lane"
    );
}

struct FailingAssignments;

impl TickSources for FailingAssignments {
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, Unread> {
        Err(Unread::Failed(SourceError("redis down".to_owned())))
    }
    fn host_busy(&self) -> Result<BTreeMap<String, f64>, Unread> {
        Ok(BTreeMap::new())
    }
    fn lane_prs(&self) -> Result<LanePrRead, Unread> {
        Err(Unread::Unwired("SWM-282"))
    }
    fn ready_work(&self) -> Result<ReadyWork, Unread> {
        Err(Unread::Unwired("SWM-212"))
    }
}

#[trace("FR-038-AC-22")]
#[test]
fn a_failed_source_read_is_a_defect_routed_in_its_tick_and_a_named_seam_is_not() {
    let mut rig = Rig::new(300);
    rig.facts = TickFacts::read(&FailingAssignments, 1_000, 300);
    let out = rig.tick(&report(T0, vec![idle(LANE)], vec![]));
    assert!(!out.complete, "a failed read is a defect, not a clean tick");
    let fixes = of(&out, ActionKind::FixSource);
    assert_eq!(fixes.len(), 1, "the remaining named seams raise nothing");
    assert_eq!(fixes[0].detail["source"], "tick.assignments");
    assert_eq!(fixes[0].target.addr.as_deref(), Some(ORCH));
    assert!(of(&out, ActionKind::PushNextTicket).is_empty());
    let defects = rig.sql(
        "SELECT source, kind FROM defect WHERE closed_at IS NULL",
        |r| {
            (
                r.get::<_, String>(0).unwrap(),
                r.get::<_, String>(1).unwrap(),
            )
        },
    );
    assert_eq!(
        defects,
        vec![(
            "tick.assignments".to_owned(),
            "unreadable_source".to_owned()
        )]
    );
    // The defect closes when the read works again.
    rig.facts = TickFacts::default();
    let healed = rig.tick(&report(T0 + 1000, vec![idle(LANE)], vec![]));
    assert!(of(&healed, ActionKind::FixSource).is_empty());
    assert_eq!(
        rig.sql("SELECT COUNT(*) FROM defect WHERE closed_at IS NULL", |r| r
            .get::<_, i64>(0)
            .unwrap()),
        vec![0]
    );
}

#[trace("FR-038-AC-7")]
#[test]
fn the_metric_counts_only_emitted_actions() {
    let mut rig = Rig::new(300);
    rig.settings.leads.clear();
    rig.tick(&report(T0, vec![stuck_lane()], vec![]));
    rig.tick(&report(T0 + 300_000, vec![stuck_lane()], vec![]));
    rig.tick(&report(T0 + 600_000, vec![stuck_lane()], vec![]));
    // Rung 2 (no lead) is recorded and not sent; rung 3 is emitted with it.
    let text = tick_metrics(&rig.path(), T0 + 600_000, 300_000).unwrap();
    assert!(
        text.contains("ix_fleet_tick_actions{kind=\"ladder_rung\"} 1"),
        "{text}"
    );
}

#[trace("FR-038-AC-1")]
#[test]
fn every_other_action_kind_is_emitted_by_its_condition_and_not_by_its_negation() {
    // need_ready_tickets is deferred to SWM-217 and is not an ActionKind yet.
    let mut rig = Rig::new(300);
    rig.settings.lane_capacity.insert("h".to_owned(), 5);
    rig.facts = host_facts(0.1, 1);
    rig.assign(LANE, &["SWM-1"]);
    let file = rig.dir.path().join("orders.toml");
    std::fs::write(&file, "").unwrap();
    rig.orders = StandingOrders::load(Some(&file));
    let quiet = rig.tick(&report(T0, vec![working(LANE)], vec![]));
    assert!(
        quiet.actions.is_empty(),
        "WORKING with no defect emits nothing: {:?}",
        quiet.actions
    );
    let conn = Connection::open(rig.path()).unwrap();
    conn.execute(
        "INSERT INTO capacity_level(host, level, recorded_at) VALUES('h', 5, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO capacity_event(host, lane, kind, at) VALUES('h', 'h:w:9', 'draining_mark', 1)",
        [],
    )
    .unwrap();
    let busy = rig.tick(&report(T0 + 1000, vec![working(LANE)], vec![]));
    assert_eq!(of(&busy, ActionKind::AddLane).len(), 1);
    assert_eq!(of(&busy, ActionKind::EscalateDrain).len(), 1);
    let flawed = vec![
        defect(DefectKind::NoEvents, Some("h"), Some(LANE), T0),
        defect(DefectKind::UnreadableSource, None, None, T0),
    ];
    let out = rig.tick(&report(T0 + 2000, vec![blocked("login", "login")], flawed));
    for kind in [
        ActionKind::EscalateBlock,
        ActionKind::ActOnUndetermined,
        ActionKind::FixSource,
    ] {
        assert!(
            !of(&out, kind).is_empty(),
            "{kind:?} is emitted by its condition"
        );
    }
    assert!(of(&out, ActionKind::AnswerDialog).is_empty());
    let dialog = rig.tick(&report(
        T0 + 3000,
        vec![blocked("dialog", "permission")],
        vec![],
    ));
    assert_eq!(of(&dialog, ActionKind::AnswerDialog).len(), 1);
    assert!(
        of(&dialog, ActionKind::FixSource).is_empty()
            && of(&dialog, ActionKind::EscalateBlock).is_empty()
    );
}

#[trace("FR-038-AC-22")]
#[test]
fn a_status_run_between_two_ticks_neither_closes_nor_resets_a_tick_owned_defect() {
    let mut rig = Rig::new(300);
    rig.facts = TickFacts::read(&FailingAssignments, 1_000, 300);
    rig.tick(&report(T0, vec![idle(LANE)], vec![]));
    let first = |rig: &Rig| {
        rig.sql(
            "SELECT first_seen_at, closed_at FROM defect WHERE source = 'tick.assignments'",
            |r| {
                (
                    r.get::<_, Option<i64>>(0).unwrap(),
                    r.get::<_, Option<i64>>(1).unwrap(),
                )
            },
        )
    };
    assert_eq!(first(&rig), vec![(Some(T0), None)]);
    // A status run does not read the tick's sources, so it must leave their defects alone.
    {
        let mut ledger = Ledger::open(&rig.path(), &rig.config).unwrap();
        ledger
            .record_status(&report(T0 + 1000, vec![idle(LANE)], vec![]))
            .unwrap();
    }
    assert_eq!(
        first(&rig),
        vec![(Some(T0), None)],
        "status left the tick defect open"
    );
    let again = rig.tick(&report(T0 + 2000, vec![idle(LANE)], vec![]));
    assert_eq!(of(&again, ActionKind::FixSource).len(), 1);
    assert_eq!(
        first(&rig),
        vec![(Some(T0), None)],
        "first_seen_at survives"
    );
    // A status-owned defect is still closed by a status run that no longer sees it.
    let seen = vec![defect(DefectKind::StaleSource, None, None, T0 + 3000)];
    {
        let mut ledger = Ledger::open(&rig.path(), &rig.config).unwrap();
        ledger
            .record_status(&report(T0 + 3000, vec![idle(LANE)], seen))
            .unwrap();
        ledger
            .record_status(&report(T0 + 4000, vec![idle(LANE)], vec![]))
            .unwrap();
    }
    let closed = rig.sql("SELECT closed_at FROM defect WHERE source = 'relay'", |r| {
        r.get::<_, Option<i64>>(0).unwrap()
    });
    assert_eq!(closed, vec![Some(T0 + 4000)]);
}

struct FakeBus {
    events: Vec<ix_board::fleet::focus::FocusEvent>,
}

impl ix_board::fleet::assign::AssignBus for FakeBus {
    fn registry(&mut self) -> Result<ix_board::fleet::assign::RegistryView, String> {
        Ok(ix_board::fleet::assign::RegistryView {
            now_ms: T0 + 60_000_000,
            lanes: Vec::new(),
            registrations: Vec::new(),
        })
    }
    fn assignments(&mut self) -> Result<Vec<relay_core::assignment::AssignmentList>, String> {
        Ok(Vec::new())
    }
    fn send_order(
        &mut self,
        _: &relay_core::Message,
        _: &relay_core::receipts::OrderSpec,
    ) -> Result<Vec<relay_core::AgentName>, String> {
        Err("a tick sends nothing".to_owned())
    }
    fn receipts_since(
        &mut self,
        _: i64,
    ) -> Result<Vec<relay_core::receipts::ReceiptEvent>, String> {
        Ok(Vec::new())
    }
    fn write_list(
        &mut self,
        _: &relay_core::assignment::AssignWriter,
        _: &str,
        _: Vec<relay_core::assignment::AssignedTicket>,
        _: &str,
    ) -> Result<relay_core::assignment::AssignmentList, String> {
        Err("a tick writes no list".to_owned())
    }
    fn lane_events(&mut self, _: &str) -> Result<Vec<ix_board::fleet::focus::FocusEvent>, String> {
        Ok(self.events.clone())
    }
}

#[trace("FR-039-AC-6")]
#[test]
fn a_tick_completes_pending_assignments_and_reads_focus_through_the_shared_commit() {
    let mut rig = Rig::new(300);
    rig.assign(LANE, &["SWM-1"]);
    rig.tick(&report(T0, vec![idle(LANE)], vec![]));
    Connection::open(rig.path())
        .unwrap()
        .execute(
            "INSERT INTO assignment_history(lane, ticket, assigned_at, order_id, expect_ticket,
               sent_at, deadline_at, result) VALUES(?1, 'SWM-1', 1, 'o-1', 'SWM-1', 1, 2, 'pending')",
            [LANE],
        )
        .unwrap();
    let mut bus = FakeBus {
        events: vec![ix_board::fleet::focus::FocusEvent {
            server_ms: T0 + 200_000,
            cwd: "/w/ix-board-swm-1".to_owned(),
            branch: "swm-1-thing".to_owned(),
        }],
    };
    let mut status = report(T0 + 300_000, vec![working(LANE)], vec![]);
    let inputs = ix_board::fleet::tick::TickInputs {
        settings: &rig.settings,
        facts: &rig.facts,
        orders: &rig.orders,
        session: None,
    };
    let out = ix_board::fleet::tick::commit_completing(&rig.config, &mut status, &inputs, &mut bus)
        .unwrap();
    assert!(out.actions.is_empty());
    let result = rig.sql("SELECT result FROM assignment_history", |r| {
        r.get::<_, String>(0).unwrap()
    });
    assert_ne!(
        result,
        vec!["pending".to_owned()],
        "the tick completed the pending row"
    );
    // The pushed lane works on the pushed ticket by derived focus: executed.
    assert_eq!(
        executed_of(&rig, ActionKind::PushNextTicket),
        vec![(Some(T0 + 300_000), Some(T0 + 300_000))]
    );
}

/// A rig writes the ledger its configuration names; `IX_BOARD_LEDGER_PATH` is read only when
/// the command loads its configuration, so an exported value never redirects an in-process rig.
#[test]
fn a_hand_built_config_resolves_to_its_own_ledger_path() {
    let dir = tempfile::tempdir().unwrap();
    let own = dir.path().join("own.sqlite3");
    let config = FleetConfig {
        ledger_path: Some(own.clone()),
        ..FleetConfig::default()
    };
    assert_eq!(ix_board::fleet::ledger::resolve_path(&config).unwrap(), own);
}

/// The command line still honors `IX_BOARD_LEDGER_PATH` over `[fleet] ledger_path`.
#[test]
fn the_cli_ledger_environment_beats_the_configured_path() {
    let dir = tempfile::tempdir().unwrap();
    let env_ledger = dir.path().join("env.sqlite3");
    let file_ledger = dir.path().join("file.sqlite3");
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[fleet]\nledger_path = {:?}\n",
            file_ledger.display().to_string()
        ),
    )
    .unwrap();
    let _ = assert_cmd::Command::cargo_bin("ix-board")
        .unwrap()
        .env("IX_BOARD_CONFIG_PATH", &config)
        .env("IX_RELAY_URL", "redis://127.0.0.1:1")
        .env("IX_BOARD_LEDGER_PATH", &env_ledger)
        .env_remove("IX_PROMETHEUS_URL")
        .args(["--json", "fleet", "status"])
        .output()
        .unwrap();
    assert!(env_ledger.exists());
    assert!(!file_ledger.exists());
}
