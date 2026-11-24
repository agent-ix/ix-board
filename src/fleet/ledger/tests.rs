// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use super::*;
use crate::fleet::report::{DefectKind, HostSummary, LaneRow};
use ix_trace_rs::trace;

const MIN: i64 = 60_000;

fn row(lane: &str, class: ClassKind) -> LaneRow {
    LaneRow {
        lane: lane.to_owned(),
        host: "mac".to_owned(),
        class,
        reason: None,
        state_age_secs: None,
        last_event_ms: None,
        evidence: Vec::new(),
        compile_seconds: None,
        focus: None,
        thresholds_defaulted: false,
        schema_version: 1,
    }
}

fn report(at_min: i64, lanes: &[(&str, ClassKind)]) -> FleetReport {
    FleetReport {
        schema_version: 1,
        complete: true,
        run_ms: at_min * MIN,
        rows: lanes.iter().map(|(l, c)| row(l, *c)).collect(),
        hosts: vec![HostSummary {
            host: "mac".to_owned(),
            lanes: lanes.len(),
            working: lanes
                .iter()
                .filter(|(_, c)| *c == ClassKind::Working)
                .count(),
            not_working: lanes
                .iter()
                .filter(|(_, c)| *c != ClassKind::Working)
                .count(),
        }],
        defects: Vec::new(),
    }
}

fn defect(source: &str, at_min: i64) -> Defect {
    Defect {
        kind: DefectKind::UnreadableSource,
        source: source.to_owned(),
        host: None,
        lane: None,
        reason: "down".to_owned(),
        observed_ms: at_min * MIN,
        first_seen_ms: None,
    }
}

fn path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    (dir, path)
}

fn open(path: &Path) -> Ledger {
    Ledger::open(path, &FleetConfig::default()).unwrap()
}

fn rows(path: &Path, view: &str) -> Vec<serde_json::Map<String, serde_json::Value>> {
    query(path, view, None).unwrap().rows
}

fn count(path: &Path, sql: &str) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

#[trace("FR-039-AC-1")]
#[test]
fn working_idle_idle_working_gives_ten_idle_minutes() {
    let (_dir, path) = path();
    let mut ledger = open(&path);
    for (at, class) in [
        (0, ClassKind::Working),
        (5, ClassKind::Idle),
        (10, ClassKind::Idle),
        (15, ClassKind::Working),
    ] {
        ledger.record_status(&report(at, &[("a", class)])).unwrap();
    }
    let idle = rows(&path, "idle_minutes");
    assert_eq!(idle.len(), 1);
    assert_eq!(idle[0]["class"], "IDLE");
    assert_eq!(idle[0]["minutes"], 10.0);
    assert_eq!(idle[0]["minutes_with_ready_work"], 0.0);
}

#[trace("FR-039-AC-2")]
#[test]
fn a_span_longer_than_twice_the_tick_interval_yields_no_minutes() {
    let (_dir, path) = path();
    let mut ledger = open(&path);
    for (at, class) in [
        (0, ClassKind::Idle),
        (5, ClassKind::Idle),
        (25, ClassKind::Idle),
        (30, ClassKind::Working),
    ] {
        ledger.record_status(&report(at, &[("a", class)])).unwrap();
    }
    // 0-5 and 25-30 count; the 20 minute span from 5 to 25 is a gap.
    assert_eq!(rows(&path, "idle_minutes")[0]["minutes"], 10.0);
}

#[trace("FR-039-AC-3")]
#[test]
fn concurrent_writers_serialize_with_increasing_sequences() {
    let (_dir, path) = path();
    let handles: Vec<_> = (0..2)
        .map(|worker| {
            let path = path.clone();
            std::thread::spawn(move || {
                let config = FleetConfig {
                    ledger_lock_wait: Duration::from_secs(20),
                    ..FleetConfig::default()
                };
                for n in 0..5 {
                    let mut ledger = Ledger::open(&path, &config).unwrap();
                    ledger
                        .record_status(&report(worker * 100 + n, &[]))
                        .unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let connection = Connection::open(&path).unwrap();
    let mut statement = connection
        .prepare("SELECT sequence FROM run ORDER BY rowid")
        .unwrap();
    let sequences: Vec<i64> = statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(sequences, (1..=10).collect::<Vec<_>>());
}

#[trace("FR-039-AC-3")]
#[test]
fn a_writer_that_cannot_take_the_lock_fails_without_writing() {
    let (_dir, path) = path();
    let mut holder = open(&path);
    holder
        .record_status(&report(0, &[("a", ClassKind::Idle)]))
        .unwrap();
    let config = FleetConfig {
        ledger_lock_wait: Duration::from_millis(150),
        ..FleetConfig::default()
    };
    let error = Ledger::open(&path, &config).err().unwrap();
    assert!(matches!(error, LedgerError::LockTimeout { .. }), "{error}");
    drop(holder);
    assert_eq!(count(&path, "SELECT COUNT(*) FROM run"), 1);
}

#[trace("FR-039-AC-4")]
#[test]
fn a_crash_before_commit_leaves_the_previous_committed_state() {
    let (_dir, path) = path();
    let mut ledger = open(&path);
    let mut first = report(0, &[("a", ClassKind::Idle)]);
    first.defects.push(defect("relay", 0));
    ledger.record_status(&first).unwrap();
    let mut second = report(5, &[("a", ClassKind::Working), ("b", ClassKind::Idle)]);
    second.defects.push(defect("prometheus", 5));
    assert!(ledger.record_inner(&second, true).is_err());
    drop(ledger);
    assert_eq!(count(&path, "SELECT COUNT(*) FROM run"), 1);
    assert_eq!(count(&path, "SELECT COUNT(*) FROM lane_tick"), 1);
    assert_eq!(
        count(&path, "SELECT COUNT(*) FROM defect WHERE closed_at IS NULL"),
        1
    );
    assert_eq!(count(&path, "SELECT COUNT(*) FROM defect"), 1);
}

#[trace("FR-039-AC-5")]
#[test]
fn a_ledger_path_inside_a_git_work_tree_is_refused_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let inside = dir.path().join("sub").join("ledger.sqlite3");
    let error = Ledger::open(&inside, &FleetConfig::default())
        .err()
        .unwrap();
    assert!(matches!(error, LedgerError::InsideGit { .. }), "{error}");
    assert!(!inside.exists());
    let config = FleetConfig {
        ledger_path: Some(inside),
        ..FleetConfig::default()
    };
    let mut live = report(0, &[("a", ClassKind::Idle)]);
    record_status(&config, &mut live);
    assert!(!live.complete);
    assert_eq!(live.rows.len(), 1);
    let unwritable: Vec<_> = live
        .defects
        .iter()
        .filter(|d| d.kind == DefectKind::LedgerUnwritable)
        .collect();
    assert_eq!(unwritable.len(), 1);
    assert_eq!(unwritable[0].source, "ledger");
}

#[trace("FR-039-AC-5")]
#[test]
fn the_default_path_is_under_the_state_directory() {
    let path = default_path().unwrap();
    assert!(path.ends_with("ix-board/fleet-ledger.sqlite3"));
}

#[trace("FR-039-AC-7")]
#[test]
fn query_returns_idle_minutes_in_json_and_text_without_writing() {
    let (_dir, path) = path();
    let mut ledger = open(&path);
    for (at, class) in [
        (0, ClassKind::Idle),
        (5, ClassKind::Idle),
        (10, ClassKind::Working),
    ] {
        ledger.record_status(&report(at, &[("a", class)])).unwrap();
    }
    drop(ledger);
    let before = std::fs::read(&path).unwrap();
    let result = query(&path, "idle_minutes", None).unwrap();
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["view"], "idle_minutes");
    assert_eq!(json["rows"][0]["lane"], "a");
    assert_eq!(json["rows"][0]["minutes"], 10.0);
    assert!(result.render_text().contains("minutes=10.0"));
    let recent = query(&path, "idle_minutes", Some(Duration::from_secs(60))).unwrap();
    assert_eq!(recent.since_ms, Some(9 * MIN));
    assert!(recent.rows.is_empty());
    assert!(matches!(
        query(&path, "no_such_view", None),
        Err(LedgerError::UnknownView(_))
    ));
    assert!(matches!(
        query(
            &path,
            "idle_lane_episode_with_ready_work_max",
            Some(Duration::from_secs(60))
        ),
        Err(LedgerError::NoSince(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[trace("FR-039-AC-8")]
#[test]
fn an_unsupported_schema_version_is_refused_without_a_change() {
    let (_dir, path) = path();
    drop(open(&path));
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 9)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = Ledger::open(&path, &FleetConfig::default()).err().unwrap();
    assert!(matches!(
        error,
        LedgerError::UnsupportedVersion { found: 9, .. }
    ));
    assert!(matches!(
        query(&path, "defect_rows", None),
        Err(LedgerError::UnsupportedVersion { found: 9, .. })
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[trace("FR-039-AC-8")]
#[test]
fn retention_prunes_only_rows_older_than_the_configured_age() {
    let (_dir, path) = path();
    let config = FleetConfig {
        ledger_retention: Duration::from_secs(3600),
        ..FleetConfig::default()
    };
    let mut ledger = Ledger::open(&path, &config).unwrap();
    ledger
        .record_status(&report(0, &[("a", ClassKind::Idle)]))
        .unwrap();
    ledger
        .record_status(&report(30, &[("a", ClassKind::Idle)]))
        .unwrap();
    assert_eq!(count(&path, "SELECT COUNT(*) FROM lane_tick"), 2);
    // At minute 70 the cutoff is minute 10: the run at 0 goes, the run at 30 stays.
    ledger
        .record_status(&report(70, &[("a", ClassKind::Idle)]))
        .unwrap();
    assert_eq!(
        count(
            &path,
            "SELECT COUNT(*) FROM lane_tick WHERE observed_ms < 600000"
        ),
        0
    );
    assert_eq!(count(&path, "SELECT COUNT(*) FROM lane_tick"), 2);
    assert_eq!(count(&path, "SELECT COUNT(*) FROM run"), 2);
}

fn seed_v1(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(schema::V1).unwrap();
    connection
        .execute_batch(
            "INSERT INTO defect(source,kind,first_seen_at) VALUES('relay','unreadable_source',7);
             INSERT INTO action(kind,lane,emitted_at) VALUES('push_next_ticket','a',9);
             INSERT INTO tick(started_at) VALUES(11);
             PRAGMA user_version=1;",
        )
        .unwrap();
}

fn v1_rows(path: &Path) -> (i64, i64, i64) {
    (
        count(path, "SELECT first_seen_at FROM defect"),
        count(path, "SELECT emitted_at FROM action"),
        count(path, "SELECT started_at FROM tick"),
    )
}

#[trace("FR-039-AC-9")]
#[test]
fn a_crash_mid_migration_leaves_the_version_one_ledger_intact_and_openable() {
    let (_dir, path) = path();
    seed_v1(&path);
    let total = schema::V2.len() + schema::views().len();
    for fail_at in [0, 20, total - 1] {
        let error = Ledger::open_with(&path, &FleetConfig::default(), Some(fail_at))
            .err()
            .unwrap();
        assert!(matches!(error, LedgerError::Sqlite(_)));
        assert_eq!(count(&path, "PRAGMA user_version"), 1);
        assert_eq!(
            count(
                &path,
                "SELECT COUNT(*) FROM sqlite_master WHERE name='lane_tick'"
            ),
            0
        );
        assert_eq!(
            count(&path, "SELECT COUNT(*) FROM pragma_table_info('defect')"),
            6
        );
        assert_eq!(v1_rows(&path), (7, 9, 11));
    }
    drop(open(&path));
    assert_eq!(count(&path, "PRAGMA user_version"), 2);
    assert_eq!(v1_rows(&path), (7, 9, 11));
    assert_eq!(
        count(&path, "SELECT COUNT(*) FROM defect WHERE closed_at IS NULL"),
        1
    );
}

#[trace("FR-039-AC-9")]
#[test]
fn migration_is_one_transaction_and_a_later_version_is_refused() {
    let (_dir, path) = path();
    seed_v1(&path);
    let ledger = open(&path);
    assert_eq!(ledger.path(), path);
    drop(ledger);
    for table in [
        "lane_tick",
        "plan_coverage_tick",
        "plan_coverage_gap",
        "drift_finding",
        "order_focus_at_deadline",
        "action_send",
        "session_turn",
        "cycle_event",
        "cycle_missed",
        "dialog_answer",
        "capacity_run",
        "capacity_step",
        "capacity_step_lane_output",
        "capacity_level",
        "capacity_event",
        "host_output_sample",
    ] {
        assert_eq!(
            count(
                &path,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{table}'"
                )
            ),
            1,
            "{table}"
        );
    }
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 3)
        .unwrap();
    assert!(matches!(
        Ledger::open(&path, &FleetConfig::default()),
        Err(LedgerError::UnsupportedVersion { found: 3, .. })
    ));
}

#[trace("FR-039-AC-11")]
#[test]
fn idle_with_ready_work_views_equal_the_hand_computed_values() {
    let (_dir, path) = path();
    let mut ledger = open(&path);
    // Lane a: IDLE with ready work at minutes 0..=30 (7 ticks), one WORKING tick, then IDLE
    // with ready work once at 40. Lane b: IDLE without ready work.
    let ticks: Vec<(i64, ClassKind)> = (0..=6)
        .map(|n| (n * 5, ClassKind::Idle))
        .chain([(35, ClassKind::Working), (40, ClassKind::Idle)])
        .collect();
    for (at, class) in &ticks {
        ledger
            .record_status(&report(*at, &[("a", *class), ("b", ClassKind::Idle)]))
            .unwrap();
    }
    ledger
        .connection
        .execute(
            "UPDATE lane_tick SET ready_work_available = (lane = 'a' AND class = 'IDLE')",
            [],
        )
        .unwrap();
    drop(ledger);
    assert_eq!(rows(&path, "idle_lane_with_ready_work_ticks").len(), 8);
    assert_eq!(rows(&path, "lanes_with_ready_work").len(), 8);
    let episodes = rows(&path, "idle_lane_episodes_with_ready_work");
    assert_eq!(episodes.len(), 2);
    // First: 0 to 35 minutes, (35 - 15) / 10 = 2 cycles. Second: one tick, floor of 1.
    assert_eq!(episodes[0]["open_cycles"], 2);
    assert_eq!(episodes[0]["ticks"], 7);
    assert_eq!(episodes[1]["open_cycles"], 1);
    assert_eq!(
        rows(&path, "idle_lane_episode_with_ready_work_max")[0]["open_cycles"],
        2
    );
    let idle = rows(&path, "idle_minutes");
    let a = idle.iter().find(|r| r["lane"] == "a").unwrap();
    assert_eq!(a["minutes"], 35.0);
    assert_eq!(a["minutes_with_ready_work"], 35.0);
    let b = idle.iter().find(|r| r["lane"] == "b").unwrap();
    assert_eq!(b["minutes_with_ready_work"], 0.0);
}

#[trace("FR-039-AC-15")]
#[test]
fn defect_rows_carry_first_seen_and_close_when_the_defect_clears() {
    let (_dir, path) = path();
    let mut ledger = open(&path);
    let mut first = report(0, &[]);
    first.defects.push(defect("relay", 0));
    assert_eq!(ledger.record_status(&first).unwrap(), vec![0]);
    let mut second = report(5, &[]);
    second.defects.push(defect("relay", 5));
    assert_eq!(ledger.record_status(&second).unwrap(), vec![0]);
    assert_eq!(
        ledger.record_status(&report(10, &[])).unwrap(),
        Vec::<i64>::new()
    );
    let mut fourth = report(15, &[]);
    fourth.defects.push(defect("relay", 15));
    assert_eq!(ledger.record_status(&fourth).unwrap(), vec![15 * MIN]);
    drop(ledger);
    let defects = rows(&path, "defect_rows");
    assert_eq!(defects.len(), 2);
    assert_eq!(defects[0]["first_seen_at"], 0);
    assert_eq!(defects[0]["closed_at"], 10 * MIN);
    assert!(defects[1]["closed_at"].is_null());
}
