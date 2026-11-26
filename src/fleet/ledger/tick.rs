// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `fleet tick` writer: the tick, its `lane_tick` rows (source `tick`), the actions with
//! their ladder rows, and the read of the last committed tick for the exporter (FR-038,
//! FR-039).

use super::{
    Ledger, LedgerError, Reader, class_name, count, millis, prune, reconcile_defects, user_version,
};
use crate::fleet::tick::{
    Action, ActionKind, Capacity, OpenAction, PlanInput, StandingOrders, TICK_SCHEMA_VERSION,
    TickFacts, TickOutput, TickSettings, plan,
};
use crate::fleet::{ClassKind, FleetReport};
use ix_trace_rs::implements;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use std::collections::BTreeMap;
use std::path::Path;

/// What one tick commits.
pub struct TickCommit<'a> {
    /// The status report of the tick; its relay clock is the tick time.
    pub report: &'a FleetReport,
    /// Tick settings.
    pub settings: &'a TickSettings,
    /// Facts from the other sources.
    pub facts: &'a TickFacts,
    /// Standing-order file state.
    pub orders: &'a StandingOrders,
    /// The session address that ran the tick.
    pub session: Option<&'a str>,
    /// Lane (lower case) to the ticket its derived focus names (FR-037), for lanes with an open
    /// `push_next_ticket` condition.
    pub focus: &'a BTreeMap<String, String>,
}

#[implements("FR-038-AC-24")]
fn load_open(tx: &Transaction<'_>) -> Result<Vec<OpenAction>, LedgerError> {
    let mut statement = tx.prepare(
        "SELECT id, kind, COALESCE(project, lane), condition_started_at, due_by, rung, parent_action_id
         FROM action WHERE resolved_at IS NULL AND tick_id IS NOT NULL ORDER BY id",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut open = Vec::new();
    for row in rows {
        let (id, kind, subject, started, due_by, rung, parent) = row?;
        let (Some(kind), Some(subject)) = (ActionKind::from_name(&kind), subject) else {
            continue;
        };
        open.push(OpenAction {
            id,
            kind,
            subject,
            started_at: started.unwrap_or_default(),
            due_by: due_by.unwrap_or_default(),
            rung: rung.and_then(|n| u8::try_from(n).ok()),
            parent,
        });
    }
    Ok(open)
}

fn load_capacity(tx: &Transaction<'_>) -> Result<Capacity, LedgerError> {
    let mut capacity = Capacity::default();
    let lower = |s: String| s.to_lowercase();
    let mut runs = tx.prepare("SELECT host FROM capacity_run WHERE ended_at IS NULL")?;
    for host in runs.query_map([], |r| r.get::<_, String>(0))? {
        capacity.open_runs.insert(lower(host?));
    }
    let mut levels =
        tx.prepare("SELECT host, level FROM capacity_level WHERE superseded_at IS NULL")?;
    for row in levels.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (host, level) = row?;
        capacity
            .levels
            .insert(lower(host), u32::try_from(level).unwrap_or(0));
    }
    let mut cool = tx.prepare(
        "SELECT host, MAX(ceiling_until) FROM capacity_event
         WHERE kind='step_down' AND ceiling_until IS NOT NULL GROUP BY host",
    )?;
    for row in cool.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (host, until) = row?;
        capacity.cool_off_until.insert(lower(host), until);
    }
    let mut drains = tx.prepare(
        "SELECT lane, host, at FROM capacity_event e
         WHERE kind='draining_mark' AND lane IS NOT NULL AND NOT EXISTS (
           SELECT 1 FROM capacity_event f WHERE f.lane = e.lane
             AND f.kind IN ('drain_done','lane_removed') AND f.at >= e.at)",
    )?;
    for row in drains.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (lane, host, at) = row?;
        capacity.draining.insert(lower(lane), (lower(host), at));
    }
    Ok(capacity)
}

/// Whether positive evidence says an action was executed when its condition cleared.
/// `answer_dialog` expects the lane out of BLOCKED; `act_on_undetermined` expects a determined
/// class; `push_next_ticket` expects the lane WORKING with its FR-037 focus on the pushed ticket,
/// the head of its FR-072 list (the tick leaves the list unchanged). `resend_order` needs an
/// `Acted` receipt join, deferred to SWM-255, so it has no `executed_at`.
fn executed(commit: &TickCommit<'_>, kind: ActionKind, subject: &str) -> bool {
    let report = commit.report;
    let Some(row) = report
        .rows
        .iter()
        .find(|r| r.lane.eq_ignore_ascii_case(subject))
    else {
        return false;
    };
    match kind {
        ActionKind::AnswerDialog => row.class != ClassKind::Blocked,
        ActionKind::ActOnUndetermined => row.class != ClassKind::Unreachable,
        ActionKind::PushNextTicket => {
            let key = subject.to_lowercase();
            let pushed = commit
                .facts
                .assignments
                .as_ref()
                .and_then(|a| a.get(&key))
                .and_then(|tickets| tickets.first());
            row.class == ClassKind::Working
                && pushed.is_some_and(|ticket| {
                    commit
                        .focus
                        .get(&key)
                        .is_some_and(|focus| focus.eq_ignore_ascii_case(ticket))
                })
        }
        _ => false,
    }
}

impl Ledger {
    /// Lanes (lower case) that have an unresolved `push_next_ticket` action.
    ///
    /// # Errors
    ///
    /// A database failure.
    pub fn open_push_lanes(&self) -> Result<Vec<String>, LedgerError> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT lane FROM action
             WHERE kind = 'push_next_ticket' AND resolved_at IS NULL AND lane IS NOT NULL",
        )?;
        let lanes = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(lanes.into_iter().map(|l| l.to_lowercase()).collect())
    }

    /// Commits one `fleet tick`: the run, the tick, a `lane_tick` row per lane, the defects,
    /// the planned actions and ladder rows, and the resolution of cleared conditions, all in
    /// one transaction. Returns the action list with its ledger ids.
    ///
    /// # Errors
    ///
    /// A database failure; nothing of the tick is written then.
    #[implements("FR-038-AC-24")]
    #[implements("FR-041-AC-15")]
    #[implements("FR-041-AC-21")]
    pub fn commit_tick(&mut self, commit: &TickCommit<'_>) -> Result<TickOutput, LedgerError> {
        let run_ms = commit.report.run_ms;
        let facts = commit.facts;
        let failed = facts.failed_defects(run_ms);
        let merged;
        let report = if failed.is_empty() {
            commit.report
        } else {
            let mut with = commit.report.clone();
            with.defects.extend(failed);
            with.defects.sort();
            with.complete = false;
            merged = with;
            &merged
        };
        let retention = millis(self.retention);
        let interval_ms = self.tick_interval_s * 1000;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO run(kind, relay_ms, relay_unavailable, tick_interval_s, cycle_interval_s,
               grace_s) VALUES('tick', ?1, 0, ?2, ?3, ?4)",
            params![
                run_ms,
                self.tick_interval_s,
                self.cycle_interval_s,
                self.grace_s
            ],
        )?;
        let sequence = tx.last_insert_rowid();
        let role = commit.session.map(|s| commit.settings.role_of(s));
        tx.execute(
            "INSERT INTO tick(started_at, session_addr, session_role) VALUES(?1, ?2, ?3)",
            params![run_ms, commit.session, role],
        )?;
        let tick_id = tx.last_insert_rowid();
        record_lane_capacity_events(&tx, report)?;
        for event in &facts.drain_done {
            tx.execute(
                "INSERT INTO capacity_event(host,lane,kind,reason,evidence_kind,at,source_basis)
                 VALUES(?1,?2,?3,?4,NULL,?5,?6)",
                params![
                    event.host.to_lowercase(),
                    event.lane.to_lowercase(),
                    if event.lane_lost {
                        "lane_lost"
                    } else {
                        "drain_done"
                    },
                    if event.lane_lost {
                        "assignment_disappeared"
                    } else {
                        "current_non_working_no_build"
                    },
                    run_ms,
                    event.source_basis
                ],
            )?;
        }
        for sample in &facts.host_output_samples {
            tx.execute(
                "INSERT INTO host_output_sample(host,at,lanes,tickets_closed,prs_merged,
                   commits_pushed,source_basis) VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(host,at) DO UPDATE SET lanes=excluded.lanes,
                   tickets_closed=excluded.tickets_closed,prs_merged=excluded.prs_merged,
                   commits_pushed=excluded.commits_pushed,source_basis=excluded.source_basis",
                params![
                    sample.host.to_lowercase(),
                    sample.at_ms,
                    sample.lanes.map(i64::from),
                    sample.tickets_closed.and_then(|n| i64::try_from(n).ok()),
                    sample.prs_merged.and_then(|n| i64::try_from(n).ok()),
                    sample.commits_pushed.and_then(|n| i64::try_from(n).ok()),
                    sample.source_basis,
                ],
            )?;
        }
        for event in &facts.overload_events {
            let host = event.host.to_lowercase();
            let previous_active = tx
                .query_row(
                    "SELECT active FROM capacity_overload_state WHERE lower(host)=lower(?1) AND cause=?2",
                    params![host, event.cause],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .is_some_and(|active| active != 0);
            if event.active && !previous_active {
                tx.execute(
                    "INSERT INTO capacity_event(host,kind,reason,evidence_kind,at,source_basis)
                     VALUES(?1,'overload_event',?2,?2,?3,?4)",
                    params![host, event.cause, run_ms, event.source_basis],
                )?;
            }
            tx.execute(
                "INSERT INTO capacity_overload_state(host,cause,active,updated_at,source_basis)
                 VALUES(?1,?2,?3,?4,?5) ON CONFLICT(host,cause) DO UPDATE SET
                 active=excluded.active,updated_at=excluded.updated_at,source_basis=excluded.source_basis",
                params![host, event.cause, i64::from(event.active), run_ms, event.source_basis],
            )?;
        }
        for sample in &facts.capacity_throttle_samples {
            tx.execute(
                "INSERT INTO capacity_throttle_sample(host,at,interval_s,throttled_share,
                   coverage,unavailable_reason,source_basis)
                 VALUES(?1,?2,?3,?4,?5,?6,'admission_only')
                 ON CONFLICT(host,at) DO UPDATE SET interval_s=excluded.interval_s,
                   throttled_share=excluded.throttled_share,coverage=excluded.coverage,
                   unavailable_reason=excluded.unavailable_reason,
                   source_basis=excluded.source_basis",
                params![
                    sample.host.to_lowercase(),
                    sample.at_ms,
                    i64::try_from(sample.interval_secs).unwrap_or(i64::MAX),
                    sample.share,
                    sample.coverage,
                    sample.unavailable_reason,
                ],
            )?;
        }
        {
            let mut lane = tx.prepare(
                "INSERT INTO lane_tick(run_seq, tick_id, source, lane, host, class, reason,
                   remaining_assigned, ready_work_available, observed_ms, last_event_ms, evidence)
                 VALUES(?1,?2,'tick',?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )?;
            for row in &report.rows {
                let key = row.lane.to_lowercase();
                let remaining = facts
                    .assignments
                    .as_ref()
                    .map(|a| a.get(&key).map_or(0, Vec::len));
                // Unknown stays NULL: only a lane with a remaining ticket or an eligible unowned
                // ready ticket is known to have ready work.
                let ready = match (remaining, &facts.ready) {
                    (Some(n), Some(ready)) => Some(n > 0 || ready.lanes.contains(&key)),
                    (Some(n), None) if n > 0 => Some(true),
                    (None, Some(ready)) if ready.lanes.contains(&key) => Some(true),
                    _ => None,
                };
                lane.execute(params![
                    sequence,
                    tick_id,
                    row.lane,
                    row.host,
                    class_name(row.class),
                    row.reason,
                    remaining.map(count),
                    ready,
                    run_ms,
                    row.last_event_ms,
                    serde_json::to_string(&row.evidence)?,
                ])?;
            }
            let mut host = tx.prepare(
                "INSERT INTO host_tick(run_seq, host, observed_ms, lanes, working, not_working)
                 VALUES(?1,?2,?3,?4,?5,?6)",
            )?;
            for summary in &report.hosts {
                host.execute(params![
                    sequence,
                    summary.host,
                    run_ms,
                    count(summary.lanes),
                    count(summary.working),
                    count(summary.not_working),
                ])?;
            }
        }
        let first_seen = reconcile_defects(&tx, &report.defects, run_ms, false, Reader::Tick)?;
        let open = load_open(&tx)?;
        let capacity = load_capacity(&tx)?;
        let planned = plan(&PlanInput {
            report,
            first_seen: &first_seen,
            settings: commit.settings,
            interval_ms,
            facts,
            capacity: &capacity,
            orders: commit.orders,
            open: &open,
        });
        let mut ids: BTreeMap<(ActionKind, String), i64> = open
            .iter()
            .filter(|o| o.kind != ActionKind::LadderRung)
            .map(|o| ((o.kind, o.subject.clone()), o.id))
            .collect();
        let mut actions: Vec<Action> = Vec::new();
        for item in &planned.actions {
            let mut action = item.action.clone();
            if item.is_new && action.kind != ActionKind::LadderRung {
                action.id = insert_action(&tx, tick_id, run_ms, &action)?;
                ids.insert(item.key.clone(), action.id);
            } else if action.kind == ActionKind::NeedReadyTickets {
                tx.execute(
                    "UPDATE action SET project = ?1, requested_count = ?2 WHERE id = ?3",
                    params![action.subject, requested_count(&action), action.id],
                )?;
            }
            if action.kind == ActionKind::LadderRung {
                action.parent_action_id = ids.get(&item.key).copied();
                action.id = insert_action(&tx, tick_id, run_ms, &action)?;
            }
            if !item.record_only {
                actions.push(action);
            }
        }
        for open in &planned.resolved {
            let done = executed(commit, open.kind, &open.subject);
            tx.execute(
                "UPDATE action SET resolved_at = ?1,
                   executed_at = CASE WHEN ?2 THEN ?1 ELSE executed_at END WHERE id = ?3",
                params![run_ms, done, open.id],
            )?;
        }
        for item in &planned.suppressed {
            if item.kind == ActionKind::AddLane {
                tx.execute(
                    "INSERT INTO audit_sample(at, lane, detail) VALUES(?1, NULL, ?2)",
                    params![
                        run_ms,
                        serde_json::json!({
                            "kind": "add_lane_suppressed", "tick_id": tick_id,
                            "host": item.subject, "reason": item.reason,
                        })
                        .to_string()
                    ],
                )?;
            }
        }
        tx.execute(
            "UPDATE tick SET committed_at = ?1 WHERE id = ?2",
            params![run_ms, tick_id],
        )?;
        prune(&tx, run_ms.saturating_sub(retention))?;
        tx.commit()?;
        Ok(TickOutput {
            schema_version: TICK_SCHEMA_VERSION,
            tick_id,
            relay_ms: run_ms,
            complete: report.complete,
            actions,
            suppressed: planned.suppressed,
            unavailable: facts.unavailable.clone(),
        })
    }
}

#[implements("FR-038-AC-24")]
fn insert_action(
    tx: &Transaction<'_>,
    tick_id: i64,
    run_ms: i64,
    action: &Action,
) -> Result<i64, LedgerError> {
    tx.execute(
        "INSERT INTO action(tick_id, kind, lane, emitted_at, severity, due_by, target_role,
           target_addr, condition_started_at, parent_action_id, rung, project, requested_count)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            tick_id,
            action.kind.name(),
            (action.kind != ActionKind::NeedReadyTickets).then_some(action.subject.as_str()),
            run_ms,
            action.severity.name(),
            action.due_by,
            action.target.role,
            action.target.addr,
            action.condition_started_at,
            action.parent_action_id,
            action.rung.map(i64::from),
            (action.kind == ActionKind::NeedReadyTickets).then_some(action.subject.as_str()),
            requested_count(action),
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

#[implements("FR-038-AC-24")]
fn requested_count(action: &Action) -> Option<i64> {
    (action.kind == ActionKind::NeedReadyTickets)
        .then(|| {
            action
                .detail
                .get("need")
                .and_then(serde_json::Value::as_i64)
        })
        .flatten()
}

#[implements("FR-041-AC-19")]
#[implements("FR-041-AC-23")]
fn record_lane_capacity_events(
    tx: &Transaction<'_>,
    report: &FleetReport,
) -> Result<(), LedgerError> {
    let previous_ms: Option<i64> = tx.query_row(
        "SELECT MAX(observed_ms) FROM lane_tick WHERE source='tick'",
        [],
        |row| row.get(0),
    )?;
    let Some(previous_ms) = previous_ms else {
        return Ok(());
    };
    let mut previous = BTreeMap::new();
    {
        let mut statement = tx.prepare(
            "SELECT lane,host,class FROM lane_tick
             WHERE source='tick' AND observed_ms=?1",
        )?;
        for row in statement.query_map([previous_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (lane, host, class) = row?;
            previous.insert(lane.to_lowercase(), (host.to_lowercase(), class));
        }
    }
    let current: BTreeMap<String, (&str, crate::fleet::ClassKind)> = report
        .rows
        .iter()
        .map(|row| (row.lane.to_lowercase(), (row.host.as_str(), row.class)))
        .collect();
    let active_counts = report.rows.iter().try_fold(
        BTreeMap::<String, u32>::new(),
        |mut counts, row| -> Result<_, LedgerError> {
            let draining: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM capacity_event e
                 WHERE lower(e.lane)=lower(?1) AND e.kind='draining_mark'
                   AND NOT EXISTS (SELECT 1 FROM capacity_event f
                     WHERE lower(f.lane)=lower(e.lane)
                       AND f.kind IN ('drain_done','lane_removed') AND f.at>=e.at))",
                [row.lane.as_str()],
                |result| result.get(0),
            )?;
            if !draining {
                let count = counts.entry(row.host.to_lowercase()).or_default();
                *count = count.saturating_add(1);
            }
            Ok(counts)
        },
    )?;

    for (lane, (host, _class)) in &current {
        if previous.contains_key(lane) {
            continue;
        }
        let active = active_counts
            .get(&host.to_lowercase())
            .copied()
            .unwrap_or(0);
        let step_pending: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM capacity_window
             WHERE lower(host)=lower(?1) AND lanes=?2)",
            params![host, i64::from(active)],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO capacity_event(host,lane,kind,reason,at,source_basis)
             VALUES(?1,?2,'lane_added',?3,?4,'relay.fleet_status+lane_tick')",
            params![
                host.to_lowercase(),
                lane,
                if step_pending {
                    "capacity_step"
                } else {
                    "lane_change_unrecorded"
                },
                report.run_ms,
            ],
        )?;
    }

    for (lane, (host, previous_class)) in &previous {
        if current.contains_key(lane) {
            continue;
        }
        let recorded_remove: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM capacity_event
             WHERE lower(lane)=lower(?1) AND kind='lane_removed' AND at>=?2)",
            params![lane, previous_ms],
            |row| row.get(0),
        )?;
        if recorded_remove {
            continue;
        }
        let draining_mark: Option<(Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT e.reason,e.evidence_kind FROM capacity_event e
                 WHERE lower(e.lane)=lower(?1) AND e.kind='draining_mark'
                   AND NOT EXISTS (SELECT 1 FROM capacity_event f
                     WHERE lower(f.lane)=lower(e.lane)
                       AND f.kind IN ('drain_done','lane_removed') AND f.at>=e.at)
                 ORDER BY e.at DESC,e.event_id DESC LIMIT 1",
                [lane],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let lost =
            draining_mark.is_some() || matches!(previous_class.as_str(), "DEAD" | "UNREACHABLE");
        let (kind, reason, evidence) = if let Some((reason, evidence)) = draining_mark {
            ("lane_removed", reason, evidence)
        } else if lost {
            ("lane_lost", Some(previous_class.clone()), None)
        } else {
            (
                "lane_removed",
                Some("lane_change_unrecorded".to_owned()),
                None,
            )
        };
        tx.execute(
            "INSERT INTO capacity_event(host,lane,kind,reason,evidence_kind,at,source_basis)
             VALUES(?1,?2,?3,?4,?5,?6,'relay.fleet_status+lane_tick')",
            params![host, lane, kind, reason, evidence, report.run_ms],
        )?;
    }
    Ok(())
}

/// The tick metrics of the last committed tick, in Prometheus text format, for the exporter
/// (FR-033). Empty when no tick is committed or the last one is older than twice
/// `tick_interval_ms` at `now_ms`, so an external alert sees a missing tick. The ledger is
/// opened read-only; nothing is written, migrated or locked.
///
/// # Errors
///
/// A missing or newer ledger or a database failure.
pub fn tick_metrics(
    path: &Path,
    now_ms: i64,
    tick_interval_ms: i64,
) -> Result<String, LedgerError> {
    if !path.exists() {
        return Err(LedgerError::Missing(path.to_owned()));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let found = user_version(&connection)?;
    if found != super::schema::VERSION {
        return Err(LedgerError::UnsupportedVersion {
            path: path.to_owned(),
            found,
        });
    }
    let last: Option<i64> = connection.query_row(
        "SELECT MAX(committed_at) FROM tick WHERE committed_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let Some(last) = last.filter(|t| now_ms.saturating_sub(*t) <= 2 * tick_interval_ms) else {
        return Ok(String::new());
    };
    let mut counts: BTreeMap<&str, i64> = ActionKind::ALL.iter().map(|k| (k.name(), 0)).collect();
    let mut statement = connection.prepare(
        "SELECT kind, COUNT(*) FROM action
         WHERE emitted_at <= ?1 AND tick_id IS NOT NULL
           AND ((kind <> 'ladder_rung' AND (resolved_at IS NULL OR resolved_at > ?1))
             OR (kind = 'ladder_rung' AND emitted_at = ?1
               AND NOT (rung = 2 AND target_role = 'lead' AND target_addr IS NULL)))
         GROUP BY kind",
    )?;
    let rows = statement.query_map([last], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (kind, n) = row?;
        if let Some(slot) = ActionKind::ALL.iter().find(|k| k.name() == kind) {
            counts.insert(slot.name(), n);
        }
    }
    let mut text = format!(
        "# TYPE ix_fleet_tick_timestamp_seconds gauge\nix_fleet_tick_timestamp_seconds {}\n\
         # TYPE ix_fleet_tick_actions gauge\n",
        last / 1000
    );
    for (kind, n) in counts {
        text.push_str(&format!("ix_fleet_tick_actions{{kind=\"{kind}\"}} {n}\n"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::report::{HostSummary, LaneRow};
    use crate::fleet::tick::{NeedReadyTickets, TickSettings};
    use crate::fleet::{ClassKind, FleetConfig, FleetReport};
    use ix_trace_rs::trace;

    #[trace("FR-041-AC-16")]
    #[test]
    fn overload_events_record_only_breach_transitions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let report = |run_ms| FleetReport {
            schema_version: 1,
            complete: true,
            run_ms,
            rows: Vec::new(),
            hosts: Vec::new(),
            defects: Vec::new(),
            host_busy: BTreeMap::new(),
            relay_unavailable: false,
        };
        let settings = TickSettings::default();
        let orders = StandingOrders::Missing;
        let focus = BTreeMap::new();
        let event = |active| crate::fleet::tick::OverloadEventSample {
            host: "host-a".to_owned(),
            cause: "thermal_limit_breach".to_owned(),
            active,
            source_basis: "temperature_and_configured_limit".to_owned(),
        };
        for (at, active) in [
            (10_000, true),
            (20_000, true),
            (30_000, false),
            (40_000, true),
        ] {
            let facts = TickFacts {
                overload_events: vec![event(active)],
                ..TickFacts::default()
            };
            ledger
                .commit_tick(&TickCommit {
                    report: &report(at),
                    settings: &settings,
                    facts: &facts,
                    orders: &orders,
                    session: None,
                    focus: &focus,
                })
                .unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM capacity_event WHERE kind='overload_event'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[trace("FR-038-AC-24")]
    #[test]
    fn need_ready_ticket_action_persists_requested_count_and_resolution_times() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let report = |run_ms| FleetReport {
            schema_version: 1,
            complete: true,
            run_ms,
            rows: Vec::new(),
            hosts: Vec::new(),
            defects: Vec::new(),
            host_busy: BTreeMap::new(),
            relay_unavailable: false,
        };
        let settings = TickSettings::default();
        let orders = StandingOrders::Missing;
        let focus = BTreeMap::new();
        let facts = TickFacts {
            need_ready_tickets: Some(vec![NeedReadyTickets {
                project: "SWM".to_owned(),
                planner: "draco:workspace:planner".to_owned(),
                need: 7,
                ready: 1,
                lanes: 3,
            }]),
            ..TickFacts::default()
        };
        let emitted = ledger
            .commit_tick(&TickCommit {
                report: &report(10_000),
                settings: &settings,
                facts: &facts,
                orders: &orders,
                session: None,
                focus: &focus,
            })
            .unwrap();
        assert_eq!(emitted.actions.len(), 1);
        assert_eq!(emitted.actions[0].kind, ActionKind::NeedReadyTickets);

        let reduced = TickFacts {
            need_ready_tickets: Some(vec![NeedReadyTickets {
                project: "SWM".to_owned(),
                planner: "draco:workspace:planner".to_owned(),
                need: 5,
                ready: 3,
                lanes: 3,
            }]),
            ..TickFacts::default()
        };
        let persisted = ledger
            .commit_tick(&TickCommit {
                report: &report(20_000),
                settings: &settings,
                facts: &reduced,
                orders: &orders,
                session: None,
                focus: &focus,
            })
            .unwrap();
        assert_eq!(persisted.actions[0].id, emitted.actions[0].id);

        let empty = TickFacts {
            need_ready_tickets: Some(Vec::new()),
            ..TickFacts::default()
        };
        ledger
            .commit_tick(&TickCommit {
                report: &report(30_000),
                settings: &settings,
                facts: &empty,
                orders: &orders,
                session: None,
                focus: &focus,
            })
            .unwrap();

        let connection = Connection::open(&path).unwrap();
        let row: (Option<String>, String, i64, i64, i64) = connection
            .query_row(
                "SELECT lane, project, requested_count, emitted_at, resolved_at FROM action
                 WHERE kind='need_ready_tickets' AND project='SWM'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row, (None, "SWM".to_owned(), 5, 10_000, 30_000));
    }

    #[trace("FR-041-AC-15", "FR-041-AC-21")]
    #[test]
    fn tick_persists_drain_completion_and_host_output_source_basis() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let report = FleetReport {
            schema_version: 1,
            complete: true,
            run_ms: 40_000,
            rows: Vec::new(),
            hosts: Vec::new(),
            defects: Vec::new(),
            host_busy: BTreeMap::new(),
            relay_unavailable: false,
        };
        let facts = TickFacts {
            drain_done: vec![crate::fleet::tick::DrainDoneSample {
                host: "draco".to_owned(),
                lane: "draco:workspace:build".to_owned(),
                source_basis: "watcher_state+compile_increase_300s".to_owned(),
                lane_lost: false,
            }],
            host_output_samples: vec![crate::fleet::tick::HostOutputSample {
                host: "draco".to_owned(),
                at_ms: 40_000,
                lanes: Some(1),
                tickets_closed: None,
                prs_merged: Some(2),
                commits_pushed: None,
                source_basis: r#"{"prs_merged":{"source":"github.ticket_id"}}"#.to_owned(),
            }],
            ..TickFacts::default()
        };
        ledger
            .commit_tick(&TickCommit {
                report: &report,
                settings: &TickSettings::default(),
                facts: &facts,
                orders: &StandingOrders::Missing,
                session: None,
                focus: &BTreeMap::new(),
            })
            .unwrap();
        let event_basis: String = ledger
            .connection
            .query_row(
                "SELECT source_basis FROM capacity_event WHERE kind='drain_done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(event_basis, "watcher_state+compile_increase_300s");
        let output: (Option<i64>, Option<i64>, String) = ledger
            .connection
            .query_row(
                "SELECT tickets_closed,prs_merged,source_basis FROM host_output_sample
                 WHERE host='draco' AND at=40000",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(output.0, None);
        assert_eq!(output.1, Some(2));
        assert!(output.2.contains("github.ticket_id"));
    }

    #[trace("FR-041-AC-19", "FR-041-AC-23")]
    #[test]
    fn tick_records_lane_additions_and_losses_from_consecutive_reports() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let report = |run_ms, lanes: &[(&str, ClassKind)]| {
            let rows: Vec<_> = lanes
                .iter()
                .map(|(lane, class)| LaneRow {
                    lane: (*lane).to_owned(),
                    host: "draco".to_owned(),
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
                .collect();
            let working = lanes
                .iter()
                .filter(|(_, class)| *class == ClassKind::Working)
                .count();
            FleetReport {
                schema_version: 1,
                complete: true,
                run_ms,
                rows,
                hosts: vec![HostSummary {
                    host: "draco".to_owned(),
                    lanes: lanes.len(),
                    working,
                    not_working: lanes.len().saturating_sub(working),
                }],
                defects: Vec::new(),
                host_busy: BTreeMap::new(),
                relay_unavailable: false,
            }
        };
        let settings = TickSettings::default();
        let orders = StandingOrders::Missing;
        let facts = TickFacts::default();
        let focus = BTreeMap::new();
        for report in [
            report(10_000, &[("draco:workspace:a", ClassKind::Working)]),
            report(
                20_000,
                &[
                    ("draco:workspace:a", ClassKind::Working),
                    ("draco:workspace:b", ClassKind::Working),
                ],
            ),
            report(
                30_000,
                &[
                    ("draco:workspace:a", ClassKind::Dead),
                    ("draco:workspace:b", ClassKind::Working),
                ],
            ),
            report(40_000, &[("draco:workspace:b", ClassKind::Working)]),
        ] {
            ledger
                .commit_tick(&TickCommit {
                    report: &report,
                    settings: &settings,
                    facts: &facts,
                    orders: &orders,
                    session: None,
                    focus: &focus,
                })
                .unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        let events: Vec<(String, String, String)> = connection
            .prepare("SELECT lane,kind,reason FROM capacity_event ORDER BY at,event_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            events,
            vec![
                (
                    "draco:workspace:b".to_owned(),
                    "lane_added".to_owned(),
                    "lane_change_unrecorded".to_owned()
                ),
                (
                    "draco:workspace:a".to_owned(),
                    "lane_lost".to_owned(),
                    "DEAD".to_owned()
                )
            ]
        );
    }
}
