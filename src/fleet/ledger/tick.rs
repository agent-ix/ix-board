// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `fleet tick` writer: the tick, its `lane_tick` rows (source `tick`), the actions with
//! their ladder rows, and the read of the last committed tick for the exporter (FR-038,
//! FR-039).

use super::{
    Ledger, LedgerError, class_name, count, millis, prune, reconcile_defects, user_version,
};
use crate::fleet::tick::{
    Action, ActionKind, Capacity, OpenAction, PlanInput, StandingOrders, TICK_SCHEMA_VERSION,
    TickFacts, TickOutput, TickSettings, plan,
};
use crate::fleet::{ClassKind, FleetReport};
use rusqlite::{Connection, OpenFlags, Transaction, params};
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
}

fn load_open(tx: &Transaction<'_>) -> Result<Vec<OpenAction>, LedgerError> {
    let mut statement = tx.prepare(
        "SELECT id, kind, lane, condition_started_at, due_by, rung, parent_action_id
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
/// class. `push_next_ticket` needs the lane focus (FR-037) and `resend_order` an `Acted` receipt
/// (SWM-255), neither of which the tick reads yet, so they have no `executed_at`.
fn executed(kind: ActionKind, report: &FleetReport, subject: &str) -> bool {
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
        _ => false,
    }
}

impl Ledger {
    /// Commits one `fleet tick`: the run, the tick, a `lane_tick` row per lane, the defects,
    /// the planned actions and ladder rows, and the resolution of cleared conditions, all in
    /// one transaction. Returns the action list with its ledger ids.
    ///
    /// # Errors
    ///
    /// A database failure; nothing of the tick is written then.
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
        let first_seen = reconcile_defects(&tx, &report.defects, run_ms, false)?;
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
            let done = executed(open.kind, report, &open.subject);
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

fn insert_action(
    tx: &Transaction<'_>,
    tick_id: i64,
    run_ms: i64,
    action: &Action,
) -> Result<i64, LedgerError> {
    tx.execute(
        "INSERT INTO action(tick_id, kind, lane, emitted_at, severity, due_by, target_role,
           target_addr, condition_started_at, parent_action_id, rung)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            tick_id,
            action.kind.name(),
            action.subject,
            run_ms,
            action.severity.name(),
            action.due_by,
            action.target.role,
            action.target.addr,
            action.condition_started_at,
            action.parent_action_id,
            action.rung.map(i64::from),
        ],
    )?;
    Ok(tx.last_insert_rowid())
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
