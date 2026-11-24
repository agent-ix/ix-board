// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet tick` (FR-038, SWM-256) over fixtures of status reports and ledger state across ticks.

use ix_board::fleet::ledger::{Ledger, TickCommit, tick_metrics};
use ix_board::fleet::tick::{
    Action, ActionKind, LanePr, ReadyWork, StandingOrders, TickFacts, TickOutput, TickSettings,
    TickSources,
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
        receipt_gap: false,
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
}

impl Rig {
    fn new(interval_secs: u64) -> Self {
        let mut settings = TickSettings {
            orchestrator: Some(ORCH.to_owned()),
            ..TickSettings::default()
        };
        settings.leads.insert(LANE.to_owned(), LEAD.to_owned());
        let config = FleetConfig {
            tick_interval: Duration::from_secs(interval_secs),
            ..FleetConfig::default()
        };
        Self {
            dir: tempfile::tempdir().unwrap(),
            config,
            settings,
            orders: StandingOrders::Missing,
            facts: TickFacts::default(),
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
fn source_defects_become_fix_source_after_their_age_and_a_stale_host_never_gets_undetermined() {
    let mut rig = Rig::new(300);
    rig.assign(LANE, &["SWM-1"]);
    let age = rig.settings.source_defect_age.as_millis() as i64;
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
    assert!(
        of(&out, ActionKind::FixSource).is_empty(),
        "not yet beyond its age"
    );
    let out = rig.tick(&report(T0 + age, rows(), defects(T0 + age)));
    assert!(
        of(&out, ActionKind::FixSource).is_empty(),
        "exactly the age is not beyond it"
    );
    let out = rig.tick(&report(T0 + age + 1000, rows(), defects(T0 + age + 1000)));
    let fixes = of(&out, ActionKind::FixSource);
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
    rig.settings.orchestrator = None;
    let out = rig.tick(&report(T0 + 10 * 300_000, vec![stuck_lane()], vec![]));
    assert!(out.actions.iter().all(|a| a.target.role != "owner"));
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
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, SourceError> {
        self.calls.set(self.calls.get() + 1);
        Ok(BTreeMap::new())
    }
    fn host_busy(&self) -> Result<BTreeMap<String, f64>, SourceError> {
        self.calls.set(self.calls.get() + 1);
        Err(SourceError("down".to_owned()))
    }
    fn lane_prs(&self) -> Result<Vec<LanePr>, SourceError> {
        self.calls.set(self.calls.get() + 1);
        Ok(Vec::new())
    }
    fn ready_work(&self) -> Result<ReadyWork, SourceError> {
        self.calls.set(self.calls.get() + 1);
        Ok(ReadyWork::default())
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
    let facts = TickFacts::read(&sources);
    assert_eq!(sources.calls.get(), 4);
    assert_eq!(facts.unavailable.len(), 1);
    assert_eq!(facts.unavailable[0].source, "host_busy");
    assert!(facts.host_busy.is_none() && facts.assignments.is_some());
    let mut rig = Rig::new(300);
    rig.facts = facts;
    let before = rig.tick(&report(T0, vec![idle(LANE)], vec![]));
    assert_eq!(before.unavailable.len(), 1);
}
