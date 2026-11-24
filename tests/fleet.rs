// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Fleet lane status acceptance tests over fake relay data and fake Prometheus responses.

use ix_board::fleet::live::{self, PromQuery, PrometheusSeries};
use ix_board::fleet::*;
use ix_trace_rs::trace;
use relay_core::fleet::HostLaneList;
use relay_core::receipts::{ExpiredOrderState, ExpiredOrders, OrderEscalationState};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const NOW: i64 = 1_000_000_000;
const WINDOW: u64 = 600;
const TOOL: u64 = 7200;
const NO_EVENTS: u64 = 1800;

fn ms_ago(secs: u64) -> i64 {
    NOW - i64::try_from(secs).unwrap() * 1000
}

fn ev(kind: HookKind, secs_ago: u64) -> HookEvent {
    HookEvent {
        server_ms: ms_ago(secs_ago),
        kind,
    }
}

fn raw(addr: &str, kind: HookKind, secs_ago: u64) -> RawEvent {
    RawEvent {
        addr: addr.to_owned(),
        event: ev(kind, secs_ago),
    }
}

fn list(machine: &str, lanes: &[&str]) -> HostLaneList {
    HostLaneList {
        machine: machine.parse().unwrap(),
        refreshed_ms: NOW,
        lanes: lanes.iter().map(|l| (*l).to_owned()).collect(),
    }
}

fn series(window: u64, blocked: &[(&str, &str)]) -> HostSeries {
    HostSeries {
        blocked: blocked
            .iter()
            .map(|(lane, reason)| (lane.to_lowercase(), (*reason).to_owned()))
            .collect(),
        probes: vec![ProbeReading {
            source: "cpu".to_owned(),
            ok: true,
        }],
        config: RelayConfig {
            working_window: Some(Duration::from_secs(window)),
            max_tool_runtime: Some(Duration::from_secs(TOOL)),
            no_events_after: Some(Duration::from_secs(NO_EVENTS)),
        },
        compile_seconds: BTreeMap::new(),
        sample_age: Some(Duration::from_secs(10)),
        absent: Vec::new(),
    }
}

#[derive(Clone)]
struct FakeSeries {
    hosts: BTreeMap<String, Result<HostSeries, SourceError>>,
    slow: Option<(String, Duration)>,
}

impl SeriesSource for FakeSeries {
    fn host_series(&self, host: &str) -> Result<HostSeries, SourceError> {
        if let Some((slow, delay)) = &self.slow
            && slow == host
        {
            std::thread::sleep(*delay);
        }
        self.hosts
            .get(host)
            .cloned()
            .unwrap_or_else(|| Err(SourceError("host not faked".to_owned())))
    }
}

struct Receipts(Result<Vec<ExpiredOrder>, SourceError>, ReceiptRead);
impl OrderReceipts for Receipts {
    fn expired_orders(&self) -> Result<ReceiptRead, SourceError> {
        self.0.clone().map(|orders| ReceiptRead {
            orders,
            ..self.1.clone()
        })
    }
}

struct Weak(BTreeMap<String, Vec<WeakSource>>);
impl WeakSignals for Weak {
    fn signals(&self, addr: &str) -> Vec<WeakSource> {
        self.0.get(addr).cloned().unwrap_or_default()
    }
}

struct World {
    relay: RelaySnapshot,
    series: FakeSeries,
    receipts: Result<Vec<ExpiredOrder>, SourceError>,
    read_extra: ReceiptRead,
    weak: BTreeMap<String, Vec<WeakSource>>,
    cfg: FleetConfig,
}

impl World {
    /// One host `h` with the given lanes, healthy series and the given blocked reasons.
    fn one_host(lanes: &[&str], events: Vec<RawEvent>, blocked: &[(&str, &str)]) -> Self {
        let mut hosts = BTreeMap::new();
        hosts.insert("h".to_owned(), Ok(series(WINDOW, blocked)));
        Self {
            relay: RelaySnapshot {
                now_ms: NOW,
                lists: vec![list("h", lanes)],
                events,
                ..RelaySnapshot::default()
            },
            series: FakeSeries { hosts, slow: None },
            receipts: Ok(Vec::new()),
            read_extra: ReceiptRead::default(),
            weak: BTreeMap::new(),
            cfg: FleetConfig::default(),
        }
    }

    fn run(&self) -> FleetReport {
        self.run_at(NOW)
    }

    fn run_at(&self, local_now_ms: i64) -> FleetReport {
        let receipts = Receipts(self.receipts.clone(), self.read_extra.clone());
        let weak = Weak(self.weak.clone());
        let sources = Sources {
            relay: Ok(self.relay.clone()),
            series: Some(Arc::new(self.series.clone())),
            receipts: &receipts,
            weak: &weak,
        };
        build_report(&self.cfg, &sources, local_now_ms)
    }
}

fn order(addr: &str, delivered: bool) -> ExpiredOrder {
    ExpiredOrder {
        addr: addr.to_owned(),
        delivered,
    }
}
fn class_of(report: &FleetReport, lane: &str) -> (ClassKind, Option<String>) {
    let row = report
        .rows
        .iter()
        .find(|r| r.lane == lane)
        .unwrap_or_else(|| panic!("no row for {lane}"));
    (row.class, row.reason.clone())
}

fn has_defect(report: &FleetReport, kind: DefectKind) -> bool {
    report.defects.iter().any(|d| d.kind == kind)
}

#[trace("FR-034-AC-1")]
#[test]
fn n_registered_lanes_across_hosts_give_n_rows_with_a_defined_class() {
    let mut hosts = BTreeMap::new();
    for host in ["a", "b", "c"] {
        hosts.insert(host.to_owned(), Ok(series(WINDOW, &[("b:w:2", "login")])));
    }
    let world = World {
        relay: RelaySnapshot {
            now_ms: NOW,
            lists: vec![
                list("a", &["a:w:1", "a:w:2"]),
                list("b", &["b:w:1", "b:w:2"]),
                list("c", &["c:w:1"]),
            ],
            events: vec![
                raw("a:w:1", HookKind::PostTool, 5),
                raw("b:w:1", HookKind::SessionEnd, 5),
                raw("c:w:1", HookKind::Stop, 5),
            ],
            ..RelaySnapshot::default()
        },
        series: FakeSeries { hosts, slow: None },
        receipts: Ok(Vec::new()),
        read_extra: ReceiptRead::default(),
        weak: BTreeMap::new(),
        cfg: FleetConfig::default(),
    };
    let report = world.run();
    assert_eq!(report.rows.len(), 5);
    assert_eq!(class_of(&report, "a:w:1").0, ClassKind::Working);
    assert_eq!(class_of(&report, "b:w:1").0, ClassKind::Dead);
    assert_eq!(
        class_of(&report, "b:w:2"),
        (ClassKind::Blocked, Some("login".to_owned()))
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("UNKNOWN"));
}

#[trace("FR-034-AC-2", "FR-034-AC-12")]
#[test]
fn hook_evidence_alone_is_working_inside_the_window_and_idle_one_second_beyond() {
    for kind in [HookKind::Prompt, HookKind::PreTool, HookKind::PostTool] {
        let at = |secs| {
            let mut events = vec![raw("h:w:1", kind, secs)];
            if kind == HookKind::PreTool {
                events.push(raw("h:w:1", HookKind::PostTool, secs));
            }
            World::one_host(&["h:w:1"], events, &[]).run()
        };
        assert_eq!(
            class_of(&at(WINDOW), "h:w:1").0,
            ClassKind::Working,
            "{kind:?}"
        );
        assert_eq!(
            class_of(&at(WINDOW + 1), "h:w:1").0,
            ClassKind::Idle,
            "{kind:?}"
        );
    }
    let report = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Prompt, 5)], &[]).run();
    assert!(report.rows[0].evidence.iter().all(|e| e.source != "herdr"));
}

#[trace("FR-034-AC-3")]
#[test]
fn weak_signals_need_two_distinct_sources() {
    let run = |signals: Vec<WeakSource>| {
        let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5000)], &[]);
        world.weak.insert("h:w:1".to_owned(), signals);
        class_of(&world.run(), "h:w:1").0
    };
    for single in [
        WeakSource::HerdrTitle,
        WeakSource::PaneText,
        WeakSource::SelfReport,
    ] {
        assert_eq!(run(vec![single]), ClassKind::Idle);
    }
    assert_eq!(
        run(vec![WeakSource::HerdrTitle, WeakSource::HerdrTitle]),
        ClassKind::Idle
    );
    assert_eq!(
        run(vec![WeakSource::HerdrTitle, WeakSource::SelfReport]),
        ClassKind::Working
    );
}

#[trace("FR-034-AC-4", "FR-034-AC-12")]
#[test]
fn an_open_pre_tool_is_working_until_max_tool_runtime_then_stuck() {
    let run = |events| class_of(&World::one_host(&["h:w:1"], events, &[]).run(), "h:w:1").0;
    assert_eq!(
        run(vec![raw("h:w:1", HookKind::PreTool, TOOL)]),
        ClassKind::Working
    );
    assert_eq!(
        run(vec![raw("h:w:1", HookKind::PreTool, TOOL + 1)]),
        ClassKind::Stuck
    );
    assert_eq!(
        run(vec![
            raw("h:w:1", HookKind::PreTool, TOOL + 100),
            raw("h:w:1", HookKind::PostTool, TOOL + 50),
        ]),
        ClassKind::Idle
    );
}

#[trace("FR-034-AC-5")]
#[test]
fn wait_loop_and_repeated_stops_make_a_loop_but_fewer_or_interleaved_do_not() {
    let stops = |n: u64| -> Vec<RawEvent> {
        (0..n)
            .map(|i| raw("h:w:1", HookKind::Stop, 100 + i * 10))
            .collect()
    };
    let run = |events, blocked: &[(&str, &str)]| {
        class_of(&World::one_host(&["h:w:1"], events, blocked).run(), "h:w:1").0
    };
    assert_eq!(run(vec![], &[("h:w:1", "wait_loop")]), ClassKind::Loop);
    assert_eq!(run(stops(3), &[]), ClassKind::Loop);
    assert_eq!(run(stops(2), &[]), ClassKind::Idle);
    let mut interleaved = stops(3);
    interleaved.push(raw("h:w:1", HookKind::PreTool, 115));
    interleaved.push(raw("h:w:1", HookKind::PostTool, 114));
    assert_ne!(run(interleaved, &[]), ClassKind::Loop);
}

#[trace("FR-034-AC-6")]
#[test]
fn relay_blocked_reasons_map_to_blocked_classes_and_session_end_is_dead() {
    let cases = [
        ("dialog", "dialog"),
        ("permission", "dialog"),
        ("question", "question"),
        ("queued_question", "question"),
        ("login", "login"),
        ("usage_limit", "usage_limit"),
    ];
    for (relay, class) in cases {
        let report = World::one_host(
            &["h:w:1"],
            vec![raw("h:w:1", HookKind::Prompt, 5)],
            &[("h:w:1", relay)],
        )
        .run();
        assert_eq!(
            class_of(&report, "h:w:1"),
            (ClassKind::Blocked, Some(class.to_owned())),
            "{relay}"
        );
    }
    let dead = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::SessionEnd, 5)], &[]).run();
    assert_eq!(class_of(&dead, "h:w:1").0, ClassKind::Dead);
}

#[trace("FR-034-AC-7")]
#[test]
fn the_class_follows_the_relay_series_not_raw_hook_or_pane_evidence() {
    // A raw `blocked` hook event and a herdr pane signal, with the relay series saying nothing
    // is blocked: the lane is not BLOCKED.
    let mut world = World::one_host(
        &["h:w:1", "h:w:2"],
        vec![
            raw("h:w:1", HookKind::Blocked, 5),
            raw("h:w:2", HookKind::Prompt, 5),
        ],
        &[("h:w:2", "login")],
    );
    world
        .weak
        .insert("h:w:1".to_owned(), vec![WeakSource::PaneText]);
    let report = world.run();
    assert_eq!(class_of(&report, "h:w:1").0, ClassKind::Idle);
    // The series says login while the raw hook says working: the series wins.
    assert_eq!(
        class_of(&report, "h:w:2"),
        (ClassKind::Blocked, Some("login".to_owned()))
    );
    let source = include_str!("../src/fleet/classify.rs");
    assert!(!source.contains("HookKind::Blocked)") && !source.contains("precedence"));
}

#[trace("FR-034-AC-8")]
#[test]
fn ack_expired_orders_are_stale_with_or_without_a_delivered_receipt() {
    let mut world = World::one_host(
        &["h:w:1", "h:w:2"],
        vec![
            raw("h:w:1", HookKind::Stop, 5),
            raw("h:w:2", HookKind::Stop, 5),
        ],
        &[],
    );
    world.receipts = Ok(vec![order("h:w:1", true), order("h:w:2", false)]);
    let report = world.run();
    assert_eq!(
        class_of(&report, "h:w:1"),
        (ClassKind::StaleOrder, Some("ack_expired".to_owned()))
    );
    assert_eq!(
        class_of(&report, "h:w:2"),
        (ClassKind::StaleOrder, Some("timed_out".to_owned()))
    );
}

#[trace("FR-034-AC-8", "FR-034-AC-9")]
#[test]
fn unavailable_order_receipts_are_a_defect_not_an_empty_list() {
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5)], &[]);
    world.receipts = Err(SourceError("relay receipts: connection refused".to_owned()));
    let report = world.run();
    assert!(has_defect(&report, DefectKind::UnreadableSource));
    assert!(!report.complete);
    assert_eq!(report.rows.len(), 1);
}

#[trace("FR-034-AC-9")]
#[test]
fn quiet_stale_failed_and_old_sources_add_defects_and_keep_the_lanes() {
    let mut world = World::one_host(
        &["h:w:1", "h:w:2", "h:w:3"],
        vec![
            raw("h:w:1", HookKind::Stop, NO_EVENTS + 1),
            raw("h:w:2", HookKind::Prompt, NO_EVENTS + 1),
        ],
        &[],
    );
    world.relay.lists.push(HostLaneList {
        refreshed_ms: NOW - 90_000,
        ..list("old", &["old:w:1"])
    });
    let mut bad = series(WINDOW, &[]);
    bad.probes[0].ok = false;
    bad.sample_age = Some(Duration::from_secs(61));
    world.series.hosts.insert("old".to_owned(), Ok(bad));
    let report = world.run();
    assert_eq!(class_of(&report, "h:w:1").0, ClassKind::Idle);
    let no_events = |lane: &str| {
        report
            .defects
            .iter()
            .any(|d| d.kind == DefectKind::NoEvents && d.lane.as_deref() == Some(lane))
    };
    assert!(
        no_events("h:w:2"),
        "a stale prompt with no later event is not quiet"
    );
    assert!(!no_events("h:w:1"), "a newest stop is quiet");
    assert!(!no_events("h:w:3"), "a lane with no event is quiet");
    assert_eq!(class_of(&report, "old:w:1").0, ClassKind::Unreachable);
    assert!(has_defect(&report, DefectKind::StaleHostRegistry));
    assert!(has_defect(&report, DefectKind::StaleSource));
    assert!(
        report
            .defects
            .iter()
            .any(|d| d.kind == DefectKind::UnreadableSource && d.source == "probe:cpu")
    );
    assert_eq!(report.rows.len(), 4);
}

#[trace("FR-034-AC-10")]
#[test]
fn a_missing_lane_or_host_is_a_defect_and_the_report_is_partial() {
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Prompt, 5)], &[]);
    world.cfg.lanes = vec!["h:w:1".to_owned(), "h:w:9".to_owned()];
    world.cfg.hosts = vec!["h".to_owned(), "ghost".to_owned()];
    world
        .series
        .hosts
        .insert("ghost".to_owned(), Ok(series(WINDOW, &[])));
    let report = world.run();
    assert!(has_defect(&report, DefectKind::MissingLane));
    assert!(has_defect(&report, DefectKind::MissingHost));
    assert!(!report.complete);
    assert_eq!(report.coverage(), Coverage::Partial);
}

#[trace("FR-034-AC-11")]
#[test]
fn the_working_window_is_the_relay_value_and_no_second_constant_exists() {
    let events = vec![raw("h:w:1", HookKind::Prompt, 300)];
    let mut short = World::one_host(&["h:w:1"], events.clone(), &[]);
    short
        .series
        .hosts
        .insert("h".to_owned(), Ok(series(200, &[])));
    assert_eq!(class_of(&short.run(), "h:w:1").0, ClassKind::Idle);
    let long = World::one_host(&["h:w:1"], events, &[]);
    assert_eq!(class_of(&long.run(), "h:w:1").0, ClassKind::Working);
    let sources = [
        include_str!("../src/fleet/classify.rs"),
        include_str!("../src/fleet/config.rs"),
        include_str!("../src/fleet/report.rs"),
        include_str!("../src/fleet/live.rs"),
    ]
    .concat();
    for constant in [
        "from_secs(10 * 60)",
        "from_secs(2 * 60 * 60)",
        "from_secs(30 * 60)",
    ] {
        assert_eq!(
            sources.matches(constant).count(),
            1,
            "{constant} is defined once"
        );
    }
    for literal in ["from_secs(600)", "from_secs(7200)", "from_secs(1800)"] {
        assert!(!sources.contains(literal), "{literal}");
    }
}

#[trace("FR-034-AC-16", "FR-034-AC-12")]
#[test]
fn relay_owned_thresholds_are_refused_locally_and_ix_board_ones_override() {
    for key in [
        "working_window_secs",
        "max_tool_runtime_secs",
        "no_events_after_secs",
    ] {
        let file: toml::Value = toml::from_str(&format!("[fleet]\n{key} = 5\n")).unwrap();
        let error = FleetConfig::from_toml(Some(&file), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("relay"), "{error}");
    }
    let file: toml::Value =
        toml::from_str("[fleet]\nloop_stops = 5\ndead_after_secs = 60\nmax_clock_skew_secs = 2\n")
            .unwrap();
    let config = FleetConfig::from_toml(Some(&file), None).unwrap();
    assert_eq!(config.loop_stops, 5);
    assert_eq!(config.dead_after, Duration::from_secs(60));
    assert_eq!(config.max_clock_skew, Duration::from_secs(2));
}

#[trace("FR-034-AC-12")]
#[test]
fn dead_after_and_loop_stops_boundaries_classify_as_specified() {
    let registered = |age_secs: u64| {
        let mut world = World::one_host(&["h:w:1"], vec![], &[]);
        world.relay.registrations = vec![RegInfo {
            addr: "h:w:1".to_owned(),
            queue: String::new(),
            last_seen_ms: ms_ago(age_secs),
            has_pane: false,
        }];
        world.cfg.dead_after = Duration::from_secs(900);
        class_of(&world.run(), "h:w:1").0
    };
    assert_eq!(registered(900), ClassKind::Idle);
    assert_eq!(registered(901), ClassKind::Dead);
    let mut world = World::one_host(
        &["h:w:1"],
        vec![
            raw("h:w:1", HookKind::Stop, 50),
            raw("h:w:1", HookKind::Stop, 40),
        ],
        &[],
    );
    world.cfg.loop_stops = 2;
    assert_eq!(class_of(&world.run(), "h:w:1").0, ClassKind::Loop);
}

#[trace("FR-034-AC-13")]
#[test]
fn clock_skew_raises_a_defect_and_ages_stay_on_the_relay_clock() {
    let world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Prompt, 100)], &[]);
    let baseline = world.run_at(NOW);
    assert!(!has_defect(&baseline, DefectKind::ClockSkew));
    let skewed = world.run_at(NOW + 31_000);
    assert!(has_defect(&skewed, DefectKind::ClockSkew));
    assert_eq!(skewed.rows[0].state_age_secs, Some(100));
    assert_eq!(skewed.rows[0].class, ClassKind::Working);
    assert!(!has_defect(
        &world.run_at(NOW + 30_000),
        DefectKind::ClockSkew
    ));
}

#[trace("FR-034-AC-14")]
#[test]
fn one_slow_host_is_a_defect_within_the_bound_and_leaves_other_rows_alone() {
    let mut hosts = BTreeMap::new();
    for host in ["a", "b"] {
        hosts.insert(host.to_owned(), Ok(series(WINDOW, &[])));
    }
    let mut world = World {
        relay: RelaySnapshot {
            now_ms: NOW,
            lists: vec![list("a", &["a:w:1"]), list("b", &["b:w:1"])],
            events: vec![
                raw("a:w:1", HookKind::Prompt, 5),
                raw("b:w:1", HookKind::Prompt, 5),
            ],
            ..RelaySnapshot::default()
        },
        series: FakeSeries { hosts, slow: None },
        receipts: Ok(Vec::new()),
        read_extra: ReceiptRead::default(),
        weak: BTreeMap::new(),
        cfg: FleetConfig::default(),
    };
    let baseline = world.run();
    world.cfg.host_timeout = Duration::from_millis(200);
    world.series.slow = Some(("b".to_owned(), Duration::from_secs(3)));
    let started = Instant::now();
    let report = world.run();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        serde_json::to_value(&report.rows[0]).unwrap(),
        serde_json::to_value(&baseline.rows[0]).unwrap()
    );
    assert_eq!(report.rows.len(), 2);
    assert!(
        report
            .defects
            .iter()
            .any(|d| d.kind == DefectKind::UnreadableSource && d.host.as_deref() == Some("b"))
    );
}

#[trace("FR-034-AC-15")]
#[test]
fn text_and_json_carry_the_same_report_and_identical_evidence_gives_identical_json() {
    let mut world = World::one_host(
        &["h:w:1", "h:w:2"],
        vec![raw("h:w:1", HookKind::Prompt, 5)],
        &[("h:w:2", "dialog")],
    );
    world.cfg.lanes = vec!["h:w:3".to_owned()];
    let report = world.run();
    let text = render_text(&report);
    let json = serde_json::to_value(&report).unwrap();
    for row in json["rows"].as_array().unwrap() {
        let lane = row["lane"].as_str().unwrap();
        let line = text.lines().find(|l| l.starts_with(lane)).unwrap();
        let class = match row["reason"].as_str() {
            Some(reason) => format!("{}({reason})", row["class"].as_str().unwrap()),
            None => row["class"].as_str().unwrap().to_owned(),
        };
        assert!(line.contains(&class), "{line} vs {class}");
    }
    assert_eq!(
        text.lines().filter(|l| l.starts_with("defect:")).count(),
        json["defects"].as_array().unwrap().len()
    );
    assert!(text.ends_with(&format!("complete: {}", json["complete"])));
    let mask = |mut value: serde_json::Value| {
        value["run_ms"] = serde_json::Value::Null;
        for row in value["rows"].as_array_mut().unwrap() {
            row["state_age_secs"] = serde_json::Value::Null;
        }
        value
    };
    assert_eq!(mask(json), mask(serde_json::to_value(world.run()).unwrap()));
}

struct CannedProm(BTreeMap<&'static str, &'static str>);
impl PromQuery for CannedProm {
    fn query(&self, expr: &str) -> Result<String, SourceError> {
        self.0
            .iter()
            .find(|(prefix, _)| expr.starts_with(*prefix))
            .map(|(_, body)| (*body).to_owned())
            .ok_or_else(|| SourceError(format!("unexpected query {expr}")))
    }
}

fn vector(samples: &str) -> &'static str {
    Box::leak(
        format!(r#"{{"status":"success","data":{{"resultType":"vector","result":[{samples}]}}}}"#)
            .into_boxed_str(),
    )
}

#[trace("FR-034-AC-6", "FR-034-AC-11", "FR-034-AC-9")]
#[test]
fn prometheus_responses_become_host_series_with_relay_thresholds() {
    let mut canned = BTreeMap::new();
    canned.insert(
        "ix_fleet_lane_blocked",
        vector(r#"{"metric":{"lane":"Titan:wB:pJ","reason":"permission","host":"titan"},"value":[1,"1"]},{"metric":{"lane":"titan:wB:pM","reason":"login"},"value":[1,"0"]}"#),
    );
    canned.insert(
        "ix_fleet_probe_ok",
        vector(r#"{"metric":{"source":"cpu"},"value":[1,"1"]},{"metric":{"source":"compilers"},"value":[1,"0"]}"#),
    );
    canned.insert(
        "ix_fleet_config",
        vector(r#"{"metric":{"name":"working_window"},"value":[1,"300"]},{"metric":{"name":"max_tool_runtime"},"value":[1,"7200"]},{"metric":{"name":"no_events_after"},"value":[1,"1800"]}"#),
    );
    canned.insert(
        "ix_fleet_lane_compile_seconds_total",
        vector(r#"{"metric":{"lane":"titan:wB:pN"},"value":[1,"42.5"]}"#),
    );
    canned.insert(
        "time() - timestamp",
        vector(r#"{"metric":{},"value":[1,"12"]}"#),
    );
    let source = PrometheusSeries {
        query: CannedProm(canned),
    };
    let read = source.host_series("titan").unwrap();
    assert_eq!(
        read.blocked.get("titan:wb:pj").map(String::as_str),
        Some("permission")
    );
    assert!(!read.blocked.contains_key("titan:wb:pm"));
    assert_eq!(read.config.working_window, Some(Duration::from_secs(300)));
    assert_eq!(read.compile_seconds.get("titan:wb:pn"), Some(&42.5));
    assert_eq!(read.sample_age, Some(Duration::from_secs(12)));
    assert_eq!(read.probes.iter().filter(|p| !p.ok).count(), 1);
    let failing = PrometheusSeries {
        query: CannedProm(BTreeMap::new()),
    };
    assert!(failing.host_series("titan").is_err());
    let bad = PrometheusSeries {
        query: CannedProm(BTreeMap::from([("ix_fleet", r#"{"status":"error"}"#)])),
    };
    assert!(bad.host_series("titan").is_err());
}

#[trace("FR-034-AC-9")]
#[test]
fn a_failed_relay_read_is_a_defect_with_no_rows() {
    let receipts = Receipts(Ok(Vec::new()), ReceiptRead::default());
    let weak = Weak(BTreeMap::new());
    let sources = Sources {
        relay: Err(SourceError("connection refused".to_owned())),
        series: Some(Arc::new(FakeSeries {
            hosts: BTreeMap::new(),
            slow: None,
        })),
        receipts: &receipts,
        weak: &weak,
    };
    let report = build_report(&FleetConfig::default(), &sources, NOW);
    assert!(report.rows.is_empty() && !report.complete);
    assert_eq!(report.coverage(), Coverage::Failed);
}

#[trace("FR-034-AC-1")]
#[test]
fn stored_relay_events_parse_and_unreadable_ones_are_refused() {
    let good = r#"{"addr":"h:w:1","server_ms":5,"kind":"pre_tool","tool":"Bash"}"#;
    let parsed = parse_event(good).unwrap();
    assert_eq!(parsed.addr, "h:w:1");
    assert_eq!(parsed.event.kind, HookKind::PreTool);
    assert!(parse_event(r#"{"addr":"h:w:1","server_ms":5,"kind":"mystery"}"#).is_none());
    assert!(parse_event("not json").is_none());
}

#[trace("FR-034-AC-12", "FR-034-AC-8")]
#[test]
fn stale_order_follows_only_the_relay_recorded_ack_expired() {
    // The relay applies the order deadline. With no recorded AckExpired an order is never
    // stale, however old; with one recorded it is, and `delivered` picks the reason.
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5)], &[]);
    world.receipts = Ok(Vec::new());
    assert_ne!(class_of(&world.run(), "h:w:1").0, ClassKind::StaleOrder);
    world.receipts = Ok(vec![order("h:w:1", true)]);
    assert_eq!(
        class_of(&world.run(), "h:w:1"),
        (ClassKind::StaleOrder, Some("ack_expired".to_owned()))
    );
    world.receipts = Ok(vec![order("h:w:1", false)]);
    assert_eq!(
        class_of(&world.run(), "h:w:1"),
        (ClassKind::StaleOrder, Some("timed_out".to_owned()))
    );
    let sources = [
        include_str!("../src/fleet/classify.rs"),
        include_str!("../src/fleet/report.rs"),
    ]
    .concat();
    let code = sources
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"));
    assert!(
        code.into_iter().all(|l| !l.contains("deadline")),
        "ix-board recomputes no order deadline"
    );
}

#[trace("FR-034-AC-4")]
#[test]
fn a_later_stop_idle_or_blocked_event_ends_an_open_pre_tool() {
    for later in [HookKind::Stop, HookKind::Idle, HookKind::Blocked] {
        // An interrupted tool: the pre_tool is older than max_tool_runtime but not the newest
        // event, so the lane is neither STUCK nor kept WORKING by it.
        let report = World::one_host(
            &["h:w:1"],
            vec![
                raw("h:w:1", HookKind::PreTool, TOOL + 100),
                raw("h:w:1", later, TOOL + 50),
            ],
            &[],
        )
        .run();
        assert_eq!(class_of(&report, "h:w:1").0, ClassKind::Idle, "{later:?}");
        // Inside the working window the pre_tool still counts as hook evidence.
        let recent = World::one_host(
            &["h:w:1"],
            vec![
                raw("h:w:1", HookKind::PreTool, 100),
                raw("h:w:1", later, 50),
            ],
            &[],
        )
        .run();
        assert_eq!(
            class_of(&recent, "h:w:1").0,
            ClassKind::Working,
            "{later:?}"
        );
    }
    // Still open when it is the newest event.
    let stuck = World::one_host(
        &["h:w:1"],
        vec![raw("h:w:1", HookKind::PreTool, TOOL + 1)],
        &[],
    )
    .run();
    assert_eq!(class_of(&stuck, "h:w:1").0, ClassKind::Stuck);
}

#[trace("FR-034-AC-17")]
#[test]
fn absent_sources_and_unresolvable_registrations_are_run_level_defects() {
    let mut world = World::one_host(&["h:w:1"], vec![], &[]);
    world.relay.activity_absent = true;
    world.relay.lane_registry_absent = true;
    world.relay.registration_defects = vec![
        "registration lead-a: no resolvable address".to_owned(),
        "registration lead-b: record did not decode".to_owned(),
    ];
    let report = world.run();
    assert!(has_defect(&report, DefectKind::RelayActivityAbsent));
    assert!(has_defect(&report, DefectKind::LaneRegistryAbsent));
    let registrations: Vec<_> = report
        .defects
        .iter()
        .filter(|d| d.source == "relay.registrations")
        .collect();
    assert_eq!(registrations.len(), 2);
    assert!(registrations.iter().any(|d| d.reason.contains("lead-a")));
    assert!(!report.complete);
    assert_eq!(report.coverage(), Coverage::Partial);

    // A watcher series absent on every host is one run-level defect; absent on one host is not.
    let mut world = World::one_host(&["h:w:1", "g:w:1"], vec![], &[]);
    let mut gone = series(WINDOW, &[]);
    gone.absent = vec!["ix_fleet_lane_compile_seconds_total".to_owned()];
    world.series.hosts.insert("h".to_owned(), Ok(gone.clone()));
    world
        .series
        .hosts
        .insert("g".to_owned(), Ok(series(WINDOW, &[])));
    assert!(!has_defect(&world.run(), DefectKind::SeriesAbsent));
    world.series.hosts.insert("g".to_owned(), Ok(gone));
    let report = world.run();
    assert_eq!(
        report
            .defects
            .iter()
            .filter(|d| d.kind == DefectKind::SeriesAbsent)
            .count(),
        1
    );
}

#[trace("FR-034-AC-18", "FR-034-AC-9")]
#[test]
fn the_report_carries_per_host_capacity_with_unreachable_as_not_working() {
    let mut world = World::one_host(
        &["h:w:1", "h:w:2"],
        vec![raw("h:w:1", HookKind::Prompt, 5)],
        &[],
    );
    world.relay.lists.push(HostLaneList {
        refreshed_ms: NOW - 90_000,
        ..list("old", &["old:w:1"])
    });
    world
        .series
        .hosts
        .insert("old".to_owned(), Ok(series(WINDOW, &[])));
    let report = world.run();
    let summary: Vec<_> = report
        .hosts
        .iter()
        .map(|h| (h.host.as_str(), h.lanes, h.working, h.not_working))
        .collect();
    assert_eq!(summary, [("h", 2, 1, 1), ("old", 1, 0, 1)]);
    assert!(render_text(&report).contains("host old: lanes=1 working=0 not_working=1"));
}

#[trace("FR-034-AC-19")]
#[test]
fn an_unreadable_prometheus_keeps_hook_evidence_with_marked_default_thresholds() {
    let events = vec![
        raw("h:w:1", HookKind::Prompt, 100),
        raw("h:w:2", HookKind::PreTool, TOOL + 1),
    ];
    let mut world = World::one_host(&["h:w:1", "h:w:2"], events, &[]);
    // Unset URL: one run-level defect with the remedy, no per-host defects.
    let receipts = Receipts(Ok(Vec::new()), ReceiptRead::default());
    let weak = Weak(BTreeMap::new());
    let sources = Sources {
        relay: Ok(world.relay.clone()),
        series: None,
        receipts: &receipts,
        weak: &weak,
    };
    let report = build_report(&world.cfg, &sources, NOW);
    assert_eq!(class_of(&report, "h:w:1").0, ClassKind::Working);
    assert_eq!(class_of(&report, "h:w:2").0, ClassKind::Stuck);
    assert!(report.rows.iter().all(|r| r.thresholds_defaulted));
    let prom: Vec<_> = report
        .defects
        .iter()
        .filter(|d| d.source == "prometheus")
        .collect();
    assert_eq!(prom.len(), 1);
    assert!(
        prom[0]
            .reason
            .contains("set IX_PROMETHEUS_URL or [fleet] prometheus_url")
    );
    // A host that fails to read is marked the same way, with its own reason.
    world.series.hosts.insert(
        "h".to_owned(),
        Err(SourceError("client build failed: tls".to_owned())),
    );
    let report = world.run();
    assert_eq!(class_of(&report, "h:w:1").0, ClassKind::Working);
    assert!(report.rows.iter().all(|r| r.thresholds_defaulted));
    assert!(
        report
            .defects
            .iter()
            .any(|d| d.reason.contains("client build failed"))
    );
    // A healthy host is not marked.
    let healthy = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Prompt, 5)], &[]).run();
    assert!(healthy.rows.iter().all(|r| !r.thresholds_defaulted));
}

#[trace("FR-034-AC-19", "FR-034-AC-9")]
#[test]
fn a_registration_does_not_override_a_lapsed_host_list() {
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Prompt, 5)], &[]);
    world.relay.lists.push(HostLaneList {
        refreshed_ms: NOW - 90_000,
        ..list("old", &["old:w:1"])
    });
    world.relay.registrations = vec![RegInfo {
        addr: "old:w:1".to_owned(),
        queue: String::new(),
        last_seen_ms: NOW - 1000,
        has_pane: true,
    }];
    world
        .series
        .hosts
        .insert("old".to_owned(), Ok(series(WINDOW, &[])));
    let report = world.run();
    assert_eq!(class_of(&report, "old:w:1").0, ClassKind::Unreachable);
    assert!(has_defect(&report, DefectKind::StaleHostRegistry));
}

#[trace("FR-034-AC-17")]
#[test]
fn empty_prometheus_results_mark_the_required_series_absent_under_one_set_of_names() {
    use ix_board::fleet::live::series;
    let empty = vector("");
    let source = PrometheusSeries {
        query: CannedProm(BTreeMap::from([
            ("ix_fleet_lane_blocked", empty),
            ("ix_fleet_probe_ok", empty),
            ("ix_fleet_config", empty),
            ("ix_fleet_lane_compile_seconds_total", empty),
            ("time() - timestamp", empty),
        ])),
    };
    let read = source.host_series("h").unwrap();
    assert_eq!(
        read.absent,
        [series::PROBE_OK, series::CONFIG, series::COMPILE_SECONDS]
    );
    assert_eq!(series::LANE_BLOCKED, "ix_fleet_lane_blocked_ratio");
    assert_eq!(series::PROBE_OK, "ix_fleet_probe_ok_ratio");
    assert_eq!(series::CONFIG, "ix_fleet_config_seconds");
    assert_eq!(
        series::COMPILE_SECONDS,
        "ix_fleet_lane_compile_seconds_total"
    );
}

#[trace("FR-034-AC-20")]
#[test]
fn a_readable_prometheus_without_the_always_present_series_raises_one_naming_defect() {
    use ix_board::fleet::live::series;
    let mut world = World::one_host(&["h:w:1", "g:w:1"], vec![], &[]);
    let mut wrong = series(WINDOW, &[]);
    wrong.absent = vec![series::PROBE_OK.to_owned(), series::CONFIG.to_owned()];
    world.series.hosts.insert("h".to_owned(), Ok(wrong.clone()));
    world.series.hosts.insert("g".to_owned(), Ok(wrong));
    let report = world.run();
    let naming: Vec<_> = report
        .defects
        .iter()
        .filter(|d| d.kind == DefectKind::SeriesNamingUnverified)
        .collect();
    assert_eq!(naming.len(), 1);
    assert!(
        naming[0].reason.contains(series::PROBE_OK) && naming[0].reason.contains(series::CONFIG)
    );
    assert!(!has_defect(&report, DefectKind::SeriesAbsent));
    // One of the two series present on any host: no naming defect.
    let mut partial = series(WINDOW, &[]);
    partial.absent = vec![series::PROBE_OK.to_owned()];
    world.series.hosts.insert("h".to_owned(), Ok(partial));
    assert!(!has_defect(
        &world.run(),
        DefectKind::SeriesNamingUnverified
    ));
    // An unreadable Prometheus is not a naming problem.
    world
        .series
        .hosts
        .insert("h".to_owned(), Err(SourceError("503".to_owned())));
    world
        .series
        .hosts
        .insert("g".to_owned(), Err(SourceError("503".to_owned())));
    assert!(!has_defect(
        &world.run(),
        DefectKind::SeriesNamingUnverified
    ));
}

#[trace("FR-034-AC-21")]
#[test]
fn per_host_absence_is_visible_even_when_other_hosts_have_data() {
    let mut world = World::one_host(
        &["h:w:1", "g:w:1"],
        vec![raw("h:w:1", HookKind::Prompt, 5)],
        &[],
    );
    // Host `n` has a registration and no published lane list; host `g` has a list and lanes
    // with no hook event while `h` has events.
    world.relay.lists.push(list("g", &["g:w:1"]));
    world
        .relay
        .lists
        .retain(|l| l.machine.as_str() != "h" || !l.lanes.is_empty());
    world.relay.registrations = vec![RegInfo {
        addr: "n:w:1".to_owned(),
        queue: String::new(),
        last_seen_ms: NOW - 1000,
        has_pane: true,
    }];
    world
        .series
        .hosts
        .insert("n".to_owned(), Ok(series(WINDOW, &[])));
    world
        .series
        .hosts
        .insert("g".to_owned(), Ok(series(WINDOW, &[])));
    let report = world.run();
    assert_eq!(class_of(&report, "n:w:1").0, ClassKind::Unreachable);
    let on = |kind, host: &str| {
        report
            .defects
            .iter()
            .filter(|d| d.kind == kind && d.host.as_deref() == Some(host))
            .count()
    };
    assert_eq!(on(DefectKind::HostLaneListAbsent, "n"), 1);
    assert_eq!(on(DefectKind::HostHookEventsAbsent, "g"), 1);
    assert_eq!(on(DefectKind::HostHookEventsAbsent, "n"), 1);
    assert_eq!(on(DefectKind::HostHookEventsAbsent, "h"), 0);
    assert_eq!(on(DefectKind::HostLaneListAbsent, "h"), 0);
    assert_eq!(class_of(&report, "h:w:1").0, ClassKind::Working);
    // With the whole stream absent only the run-level defect is raised, not one per host.
    world.relay.activity_absent = true;
    assert_eq!(on_count(&world.run(), DefectKind::HostHookEventsAbsent), 0);
}

fn on_count(report: &FleetReport, kind: DefectKind) -> usize {
    report.defects.iter().filter(|d| d.kind == kind).count()
}

fn state(id: &str, recipient: &str, delivered: bool) -> ExpiredOrderState {
    ExpiredOrderState {
        original_id: id.to_owned(),
        recipient: recipient.to_owned(),
        expect_ticket: None,
        enqueued_at_ms: 1,
        deadline_ms: 2,
        expired_at_ms: 3,
        delivered_before_expiry: delivered,
        last_state: "Queued".to_owned(),
        escalation: OrderEscalationState::Pending,
    }
}

fn listed(orders: Vec<ExpiredOrderState>) -> ExpiredOrders {
    ExpiredOrders {
        orders,
        skipped: 0,
        indexed_since_ms: Some(1),
        truncated: false,
    }
}

fn lane_addrs(lanes: &[&str]) -> Vec<String> {
    lanes.iter().map(|l| (*l).to_owned()).collect()
}

#[trace("FR-034-AC-12")]
#[test]
fn the_relays_expired_orders_become_expired_orders_per_lane_address() {
    let read = live::receipt_read_from(
        &listed(vec![state("a", "H:W:1", true), state("b", "h:w:2", false)]),
        &lane_addrs(&["h:w:1", "h:w:2"]),
    );
    assert_eq!(
        read.orders,
        vec![order("h:w:1", true), order("h:w:2", false)]
    );
    assert!(read.unresolved.is_empty() && !read.truncated && read.skipped == 0);
    assert!(!read.index_not_started);
    let source = include_str!("../src/fleet/live.rs");
    assert!(!source.contains("ack_deadline") && !source.contains("deadline_ms"));
}

#[trace("FR-034-AC-12", "FR-034-AC-17", "FR-034-AC-22")]
#[test]
fn an_expired_order_to_an_unregistered_recipient_is_a_defect_row() {
    let read = live::receipt_read_from(
        &listed(vec![state("d", "s:gone:9", false)]),
        &lane_addrs(&["h:w:1"]),
    );
    assert!(read.orders.is_empty());
    assert_eq!(
        read.unresolved,
        vec![UnresolvedOrder {
            order_id: "d".to_owned(),
            target: "s:gone:9".to_owned()
        }]
    );
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5)], &[]);
    world.read_extra = read;
    let report = world.run();
    assert!(!report.complete);
    let defect = report
        .defects
        .iter()
        .find(|d| d.source == "relay.receipts")
        .expect("receipts defect");
    assert_eq!(defect.kind, DefectKind::UnreadableSource);
    assert!(defect.reason.contains('d') && defect.reason.contains("s:gone:9"));
}

#[trace("FR-034-AC-22")]
#[test]
fn unresolved_expired_orders_are_one_defect_per_target_listing_the_order_ids() {
    let read = live::receipt_read_from(
        &listed(vec![
            state("o1", "s:gone:9", false),
            state("o2", "s:gone:9", false),
            state("o3", "s:other:9", false),
        ]),
        &[],
    );
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5)], &[]);
    world.read_extra = read;
    let report = world.run();
    let defects: Vec<_> = report
        .defects
        .iter()
        .filter(|d| d.source == "relay.receipts")
        .collect();
    assert_eq!(defects.len(), 2);
    let gone = defects
        .iter()
        .find(|d| d.reason.contains("s:gone:9"))
        .unwrap();
    assert!(gone.reason.contains("o1") && gone.reason.contains("o2"));
}

#[trace("FR-034-AC-22")]
#[test]
fn a_truncated_skipped_or_unstarted_expired_order_list_is_a_defect_and_keeps_the_rows() {
    let mut list = listed(vec![
        state("o1", "h:w:1", true),
        state("o2", "H:W:9", false),
    ]);
    list.skipped = 2;
    list.truncated = true;
    list.indexed_since_ms = None;
    let read = live::receipt_read_from(&list, &lane_addrs(&["h:w:1"]));
    assert_eq!(read.orders, vec![order("h:w:1", true)]);
    assert_eq!(read.unresolved.len(), 1);
    assert!(read.truncated && read.skipped == 2 && read.index_not_started);
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5)], &[]);
    world.receipts = Ok(read.orders.clone());
    world.read_extra = read;
    let report = world.run();
    assert_eq!(class_of(&report, "h:w:1").0, ClassKind::StaleOrder);
    let reasons: Vec<_> = report
        .defects
        .iter()
        .filter(|d| d.source == "relay.receipts")
        .map(|d| d.reason.as_str())
        .collect();
    assert!(reasons.iter().any(|r| r.contains("truncated")));
    assert!(reasons.iter().any(|r| r.contains("2 expired-order rows")));
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("not indexed expired orders yet"))
    );
    assert!(!report.complete);
    // A complete list with a started index raises nothing.
    let clean = live::receipt_read_from(&listed(vec![]), &[]);
    let mut world = World::one_host(&["h:w:1"], vec![raw("h:w:1", HookKind::Stop, 5)], &[]);
    world.read_extra = clean;
    assert!(
        world
            .run()
            .defects
            .iter()
            .all(|d| d.source != "relay.receipts")
    );
}
