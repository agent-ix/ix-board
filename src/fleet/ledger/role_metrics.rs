// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Runtime writers for role-spec ledger evidence (SWM-255).

use super::LedgerError;
use crate::fleet::{ClassKind, FleetReport, focus::Focus, tick::TickFacts};
use ix_trace_rs::implements;
use rusqlite::{Transaction, params};
use std::collections::{BTreeMap, BTreeSet};

const MINIMUM_QUEUE: usize = 2;

/// Records the per-tick plan-coverage snapshot and the short-queue gap lifecycle.
///
/// The ready-work reader currently exposes counts and eligible lanes, but no ticket identities;
/// stale assignment states have no reader. Their counts remain NULL when unavailable, and their
/// open gap rows are left untouched. An unavailable assignment list likewise never closes a gap.
#[implements("FR-036-AC-8")]
pub(super) fn record_plan_coverage(
    tx: &Transaction<'_>,
    tick_id: i64,
    at_ms: i64,
    report: &FleetReport,
    facts: &TickFacts,
) -> Result<(), LedgerError> {
    let assignments = facts.assignments.as_ref();
    let mut short = BTreeMap::new();
    if let Some(assignments) = assignments {
        for lane in report
            .rows
            .iter()
            .filter(|row| row.class != ClassKind::Unreachable)
        {
            let key = lane.lane.to_lowercase();
            let remaining = assignments.get(&key).map_or(0, Vec::len);
            if remaining < MINIMUM_QUEUE {
                short.insert(lane.lane.clone(), remaining);
            }
        }
    }

    let short_count = assignments.map(|_| short.len() as i64);
    let unowned_ready_count = facts.ready.as_ref().map(|ready| ready.unassigned as i64);
    let mut unavailable = BTreeSet::new();
    if assignments.is_none() {
        unavailable.insert("short_queue");
    }
    if facts
        .ready
        .as_ref()
        .is_none_or(|ready| ready.unassigned > 0)
    {
        unavailable.insert("unowned_ready");
    }
    // No live ticket-state reader can distinguish Done, Canceled or merged assigned tickets.
    unavailable.insert("stale_assignment");
    let unavailable = serde_json::to_string(&unavailable)?;
    tx.execute(
        "INSERT INTO plan_coverage_tick(tick_id,unowned_ready_count,short_queue_count,
           stale_assignment_count,unavailable_lists) VALUES(?1,?2,?3,NULL,?4)",
        params![tick_id, unowned_ready_count, short_count, unavailable],
    )?;

    if assignments.is_some() {
        let mut open = tx.prepare(
            "SELECT gap_id,subject FROM plan_coverage_gap
             WHERE kind='short_queue' AND closed_at IS NULL",
        )?;
        let open_rows = open
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(open);

        for (gap_id, subject) in open_rows {
            if !short.contains_key(&subject) {
                tx.execute(
                    "UPDATE plan_coverage_gap SET closed_tick=?1,closed_at=?2
                     WHERE gap_id=?3 AND closed_at IS NULL",
                    params![tick_id, at_ms, gap_id],
                )?;
            }
        }
        for (lane, remaining) in short {
            let detail = serde_json::json!({
                "remaining_count": remaining,
                "minimum": MINIMUM_QUEUE,
            })
            .to_string();
            tx.execute(
                "INSERT INTO plan_coverage_gap(kind,subject,detail,opened_tick,opened_at)
                 SELECT 'short_queue',?1,?2,?3,?4
                 WHERE NOT EXISTS (
                   SELECT 1 FROM plan_coverage_gap
                   WHERE kind='short_queue' AND subject=?1 AND closed_at IS NULL
                 )",
                params![lane, detail, tick_id, at_ms],
            )?;
        }
    }

    // A ready count of zero proves no unowned-ready gap. A positive count lacks ticket identities,
    // so it cannot safely open or close per-ticket rows.
    if facts
        .ready
        .as_ref()
        .is_some_and(|ready| ready.unassigned == 0)
    {
        tx.execute(
            "UPDATE plan_coverage_gap SET closed_tick=?1,closed_at=?2
             WHERE kind='unowned_ready' AND closed_at IS NULL",
            params![tick_id, at_ms],
        )?;
    }
    Ok(())
}

/// Records only drift conclusions backed by a unique focus and complete assignment/ready inputs.
/// Ambiguous, absent, or source-incomplete focus leaves existing rows open.
#[implements("FR-037-AC-7")]
pub(super) fn record_drift_findings(
    tx: &Transaction<'_>,
    tick_id: i64,
    at_ms: i64,
    report: &FleetReport,
    facts: &TickFacts,
    focuses: &BTreeMap<String, Focus>,
) -> Result<(), LedgerError> {
    let Some(assignments) = facts.assignments.as_ref() else {
        return Ok(());
    };
    let mut present = BTreeSet::new();
    let mut evaluable = BTreeMap::new();
    for lane in &report.rows {
        let key = lane.lane.to_lowercase();
        let (Some(Focus::Ticket { ticket }), Some(own)) =
            (focuses.get(&key), assignments.get(&key))
        else {
            // Ambiguous or absent focus cannot close an existing finding.
            continue;
        };
        let normalized = ticket.to_ascii_uppercase();
        let own_has = own
            .iter()
            .any(|assigned| assigned.eq_ignore_ascii_case(ticket));
        let all_have = assignments
            .values()
            .flatten()
            .any(|assigned| assigned.eq_ignore_ascii_case(ticket));
        if !own_has {
            present.insert((
                lane.lane.clone(),
                "not_assigned".to_owned(),
                normalized.clone(),
            ));
        }
        let can_evaluate_not_in_plan = all_have
            || facts
                .ready
                .as_ref()
                .is_some_and(|ready| ready.unassigned == 0);
        if !all_have && can_evaluate_not_in_plan {
            present.insert((
                lane.lane.clone(),
                "not_in_plan".to_owned(),
                normalized.clone(),
            ));
        }
        if lane.class == ClassKind::Working
            && own
                .iter()
                .position(|assigned| assigned.eq_ignore_ascii_case(ticket))
                .is_some_and(|index| index > 0)
        {
            present.insert((
                lane.lane.clone(),
                "priority_inversion".to_owned(),
                normalized.clone(),
            ));
        }
        evaluable.insert(key, (can_evaluate_not_in_plan, normalized));
    }

    let mut open =
        tx.prepare("SELECT finding_id,lane,rule,focus FROM drift_finding WHERE closed_at IS NULL")?;
    let open_rows = open
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(open);
    for (finding_id, lane, rule, focus) in open_rows {
        let key = lane.to_lowercase();
        let Some((can_evaluate_not_in_plan, normalized)) = evaluable.get(&key) else {
            continue;
        };
        if rule == "not_in_plan" && !can_evaluate_not_in_plan {
            continue;
        }
        let currently_present = present.contains(&(
            lane.clone(),
            rule.clone(),
            focus.clone().unwrap_or_else(|| normalized.clone()),
        ));
        if !currently_present {
            tx.execute(
                "UPDATE drift_finding SET closed_at=?1 WHERE finding_id=?2 AND closed_at IS NULL",
                params![at_ms, finding_id],
            )?;
        }
    }

    for (lane, rule, focus) in present {
        let evidence = serde_json::json!({
            "tick_id": tick_id,
            "at_ms": at_ms,
            "focus": focus,
        })
        .to_string();
        tx.execute(
            "INSERT INTO drift_finding(lane,rule,focus,opened_tick,opened_at,evidence_times)
             SELECT ?1,?2,?3,?4,?5,?6 WHERE NOT EXISTS (
               SELECT 1 FROM drift_finding
               WHERE lane=?1 AND rule=?2 AND focus=?3 AND closed_at IS NULL
             )",
            params![lane, rule, focus, tick_id, at_ms, evidence],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::report::{HostSummary, LaneRow};
    use ix_trace_rs::trace;

    fn report(at_ms: i64, lanes: &[(&str, ClassKind)]) -> FleetReport {
        FleetReport {
            schema_version: 1,
            complete: true,
            run_ms: at_ms,
            rows: lanes
                .iter()
                .map(|(lane, class)| LaneRow {
                    lane: (*lane).to_owned(),
                    host: "mac".to_owned(),
                    class: *class,
                    reason: None,
                    state_age_secs: None,
                    last_event_ms: None,
                    evidence: Vec::new(),
                    compile_seconds: None,
                    focus: None,
                    thresholds_defaulted: false,
                    schema_version: 1,
                })
                .collect(),
            hosts: vec![HostSummary {
                host: "mac".to_owned(),
                lanes: lanes.len(),
                working: lanes
                    .iter()
                    .filter(|(_, class)| *class == ClassKind::Working)
                    .count(),
                not_working: lanes
                    .iter()
                    .filter(|(_, class)| *class != ClassKind::Working)
                    .count(),
            }],
            defects: Vec::new(),
            host_busy: BTreeMap::new(),
            relay_unavailable: false,
        }
    }

    fn schema() -> rusqlite::Connection {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE plan_coverage_tick(
                   tick_id INTEGER PRIMARY KEY, unowned_ready_count INTEGER,
                   short_queue_count INTEGER, stale_assignment_count INTEGER,
                   unavailable_lists TEXT);
                 CREATE TABLE plan_coverage_gap(
                   gap_id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL,
                   subject TEXT NOT NULL, detail TEXT, opened_tick INTEGER NOT NULL,
                   opened_at INTEGER NOT NULL, closed_tick INTEGER, closed_at INTEGER);
                 CREATE TABLE drift_finding(
                   finding_id INTEGER PRIMARY KEY AUTOINCREMENT, lane TEXT NOT NULL,
                   rule TEXT NOT NULL, focus TEXT, opened_tick INTEGER, opened_at INTEGER NOT NULL,
                   closed_at INTEGER, evidence_times TEXT);",
            )
            .unwrap();
        connection
    }

    #[trace("FR-036-AC-8")]
    #[test]
    fn short_queue_gaps_open_once_and_close_on_the_first_full_queue_tick() {
        let mut connection = schema();
        let first = report(10, &[("lane-a", ClassKind::Idle)]);
        let mut facts = TickFacts {
            assignments: Some(BTreeMap::from([(
                "lane-a".to_owned(),
                vec!["SWM-1".to_owned()],
            )])),
            ..TickFacts::default()
        };
        {
            let tx = connection.transaction().unwrap();
            record_plan_coverage(&tx, 1, 10, &first, &facts).unwrap();
            tx.commit().unwrap();
        }
        {
            let tx = connection.transaction().unwrap();
            record_plan_coverage(&tx, 2, 20, &first, &facts).unwrap();
            tx.commit().unwrap();
        }
        facts.assignments = Some(BTreeMap::from([(
            "lane-a".to_owned(),
            vec!["SWM-1".to_owned(), "SWM-2".to_owned()],
        )]));
        {
            let tx = connection.transaction().unwrap();
            record_plan_coverage(&tx, 3, 30, &first, &facts).unwrap();
            tx.commit().unwrap();
        }
        let values: (i64, i64, Option<i64>, Option<i64>) = connection
            .query_row(
                "SELECT COUNT(*),MIN(opened_tick),MIN(closed_tick),MIN(closed_at)
                 FROM plan_coverage_gap WHERE kind='short_queue' AND subject='lane-a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(values, (1, 1, Some(3), Some(30)));
    }

    #[trace("FR-036-AC-9")]
    #[test]
    fn unavailable_lists_are_named_and_do_not_close_open_short_queue_gaps() {
        let mut connection = schema();
        let snapshot = report(10, &[("lane-a", ClassKind::Idle)]);
        let mut facts = TickFacts {
            assignments: Some(BTreeMap::from([(
                "lane-a".to_owned(),
                vec!["SWM-1".to_owned()],
            )])),
            ..TickFacts::default()
        };
        {
            let tx = connection.transaction().unwrap();
            record_plan_coverage(&tx, 1, 10, &snapshot, &facts).unwrap();
            tx.commit().unwrap();
        }
        facts.assignments = None;
        {
            let tx = connection.transaction().unwrap();
            record_plan_coverage(&tx, 2, 20, &snapshot, &facts).unwrap();
            tx.commit().unwrap();
        }
        let unavailable: String = connection
            .query_row(
                "SELECT unavailable_lists FROM plan_coverage_tick WHERE tick_id=2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let names: Vec<String> = serde_json::from_str(&unavailable).unwrap();
        assert_eq!(
            names,
            vec!["short_queue", "stale_assignment", "unowned_ready"]
        );
        let closed: Option<i64> = connection
            .query_row(
                "SELECT closed_tick FROM plan_coverage_gap WHERE kind='short_queue'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(closed, None);
    }

    #[trace("FR-036-AC-7")]
    #[test]
    fn unreachable_lanes_do_not_count_as_short_queues() {
        let mut connection = schema();
        let snapshot = report(10, &[("lane-a", ClassKind::Unreachable)]);
        let facts = TickFacts {
            assignments: Some(BTreeMap::from([("lane-a".to_owned(), Vec::new())])),
            ..TickFacts::default()
        };
        let tx = connection.transaction().unwrap();
        record_plan_coverage(&tx, 1, 10, &snapshot, &facts).unwrap();
        tx.commit().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT short_queue_count FROM plan_coverage_tick",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[trace("FR-037-AC-7")]
    #[test]
    fn drift_findings_open_close_by_focus_and_remain_open_when_focus_is_ambiguous() {
        let mut connection = schema();
        let snapshot = report(10, &[("lane-a", ClassKind::Working)]);
        let facts = TickFacts {
            assignments: Some(BTreeMap::from([(
                "lane-a".to_owned(),
                vec!["SWM-1".to_owned(), "SWM-2".to_owned()],
            )])),
            ready: Some(crate::fleet::tick::ReadyWork::default()),
            ..TickFacts::default()
        };
        let second_ticket = BTreeMap::from([(
            "lane-a".to_owned(),
            Focus::Ticket {
                ticket: "SWM-2".to_owned(),
            },
        )]);
        {
            let tx = connection.transaction().unwrap();
            record_drift_findings(&tx, 1, 10, &snapshot, &facts, &second_ticket).unwrap();
            tx.commit().unwrap();
        }
        let ambiguous = BTreeMap::from([(
            "lane-a".to_owned(),
            Focus::Ambiguous {
                recent: "SWM-1".to_owned(),
                tickets: vec!["SWM-1".to_owned(), "SWM-2".to_owned()],
            },
        )]);
        {
            let tx = connection.transaction().unwrap();
            record_drift_findings(&tx, 2, 20, &snapshot, &facts, &ambiguous).unwrap();
            tx.commit().unwrap();
        }
        let first: (i64, Option<i64>) = connection
            .query_row(
                "SELECT COUNT(*),MAX(closed_at) FROM drift_finding
                 WHERE lane='lane-a' AND rule='priority_inversion'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(first, (1, None));

        let first_ticket = BTreeMap::from([(
            "lane-a".to_owned(),
            Focus::Ticket {
                ticket: "SWM-1".to_owned(),
            },
        )]);
        {
            let tx = connection.transaction().unwrap();
            record_drift_findings(&tx, 3, 30, &snapshot, &facts, &first_ticket).unwrap();
            tx.commit().unwrap();
        }
        let final_state: (i64, Option<i64>) = connection
            .query_row(
                "SELECT COUNT(*),MAX(closed_at) FROM drift_finding
                 WHERE lane='lane-a' AND rule='priority_inversion'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(final_state, (1, Some(30)));
    }

    #[trace("FR-037-AC-7")]
    #[test]
    fn drift_reports_not_assigned_and_not_in_plan_only_with_complete_ready_source() {
        let mut connection = schema();
        let snapshot = report(10, &[("lane-a", ClassKind::Idle)]);
        let facts = TickFacts {
            assignments: Some(BTreeMap::from([(
                "lane-a".to_owned(),
                vec!["SWM-1".to_owned()],
            )])),
            ready: Some(crate::fleet::tick::ReadyWork::default()),
            ..TickFacts::default()
        };
        let focuses = BTreeMap::from([(
            "lane-a".to_owned(),
            Focus::Ticket {
                ticket: "SWM-2".to_owned(),
            },
        )]);
        let tx = connection.transaction().unwrap();
        record_drift_findings(&tx, 1, 10, &snapshot, &facts, &focuses).unwrap();
        tx.commit().unwrap();
        let rows: Vec<String> = connection
            .prepare("SELECT rule FROM drift_finding ORDER BY rule")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec!["not_assigned", "not_in_plan"]);
    }
}
