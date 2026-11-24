// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `assignment_history` rows `fleet assign` writes (FR-035 behavior 10, FR-039): one row per
//! assignment, written before the send, completed with the order and again with the outcome.
//! A row left `pending` by a killed command, or confirmed before its deadline, is completed by
//! the first later committed `fleet status` run from the order's receipts and the focus at the
//! deadline.

use super::{Ledger, LedgerError, resolve_path};
use crate::fleet::FleetConfig;
use crate::fleet::assign::{
    AssignBus, AssignLedger, AssignReport, AssignResult, OrderEvidence, Timing, decide, focus_keys,
};
use crate::fleet::focus;
use rusqlite::params;
use std::path::PathBuf;

impl Ledger {
    /// Inserts the row of `report` (`row` unset) or updates it, and marks the lane's earlier
    /// rows replaced once the order is sent. Returns the row id.
    ///
    /// # Errors
    ///
    /// A database failure; nothing is changed.
    pub fn upsert_assignment(
        &mut self,
        row: Option<i64>,
        report: &AssignReport,
    ) -> Result<i64, LedgerError> {
        let tx = self.connection.transaction()?;
        let ticket = report.tickets.join(",");
        let assigned_at = report.sent_at_ms.unwrap_or(report.started_at_ms);
        let id = if let Some(id) = row {
            tx.execute(
                "UPDATE assignment_history SET lane=?1, ticket=?2, assigned_at=?3, order_id=?4,
                   expect_ticket=?5, sent_at=?6, deadline_at=?7, result=?8, result_at=?9,
                   confirm_latency_s=?10, focus_at_deadline=?11 WHERE id=?12",
                params![
                    report.lane,
                    ticket,
                    assigned_at,
                    report.order_id,
                    report.expect_ticket,
                    report.sent_at_ms,
                    report.deadline_at_ms,
                    report.result.name(),
                    report.result_at_ms,
                    report.confirm_latency_s,
                    report.focus_at_deadline,
                    id
                ],
            )?;
            id
        } else {
            tx.execute(
                "INSERT INTO assignment_history(lane, ticket, assigned_at, order_id, expect_ticket,
                   sent_at, deadline_at, result, result_at, confirm_latency_s, focus_at_deadline)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    report.lane,
                    ticket,
                    assigned_at,
                    report.order_id,
                    report.expect_ticket,
                    report.sent_at_ms,
                    report.deadline_at_ms,
                    report.result.name(),
                    report.result_at_ms,
                    report.confirm_latency_s,
                    report.focus_at_deadline
                ],
            )?;
            tx.last_insert_rowid()
        };
        if let Some(sent) = report.sent_at_ms {
            tx.execute(
                "UPDATE assignment_history SET replaced_at=?1
                 WHERE lane=?2 AND id<>?3 AND replaced_at IS NULL AND sent_at IS NOT NULL",
                params![sent, report.lane, id],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Deletes the row of an assignment whose order was never accepted.
    ///
    /// # Errors
    ///
    /// A database failure.
    pub fn withdraw_assignment(&mut self, id: i64) -> Result<(), LedgerError> {
        self.connection
            .execute("DELETE FROM assignment_history WHERE id=?1", [id])?;
        Ok(())
    }
}

/// The `fleet assign` writer: each call opens the ledger as the writer, writes, and releases the
/// lock, so a wait of minutes never holds it against `fleet status`.
pub struct AssignSink {
    path: PathBuf,
    config: FleetConfig,
    row: Option<i64>,
}

impl AssignSink {
    /// A sink writing to the ledger the configuration resolves.
    ///
    /// # Errors
    ///
    /// The ledger path cannot be resolved.
    pub fn resolve(config: &FleetConfig) -> Result<Self, LedgerError> {
        Ok(Self::at(resolve_path(config)?, config))
    }

    /// A sink writing to `path`.
    #[must_use]
    pub fn at(path: PathBuf, config: &FleetConfig) -> Self {
        Self {
            path,
            config: config.clone(),
            row: None,
        }
    }
}

impl AssignLedger for AssignSink {
    fn record(&mut self, report: &AssignReport) -> Result<(), String> {
        let mut ledger = Ledger::open(&self.path, &self.config).map_err(|e| e.to_string())?;
        self.row = Some(
            ledger
                .upsert_assignment(self.row, report)
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }

    fn withdraw(&mut self) -> Result<(), String> {
        let Some(id) = self.row.take() else {
            return Ok(());
        };
        let mut ledger = Ledger::open(&self.path, &self.config).map_err(|e| e.to_string())?;
        ledger.withdraw_assignment(id).map_err(|e| e.to_string())
    }
}

struct Open {
    id: i64,
    order_id: String,
    lane: String,
    tickets: Vec<String>,
    expect: String,
    sent_at: i64,
    deadline_at: i64,
    result: AssignResult,
}

/// Completes every sent assignment whose deadline has passed and whose row is `pending` or has no
/// `focus_at_deadline`, from the order's receipts and the lane's events up to the deadline.
/// Returns how many rows were completed. A ledger that does not exist yet has none.
///
/// # Errors
///
/// A ledger or relay read failure; rows completed before it stay completed.
pub fn complete_pending(config: &FleetConfig, bus: &mut impl AssignBus) -> Result<usize, String> {
    let path = resolve_path(config).map_err(|e| e.to_string())?;
    if !path.exists() {
        return Ok(0);
    }
    let ledger = Ledger::open(&path, config).map_err(|e| e.to_string())?;
    let now_ms = bus.registry()?.now_ms;
    let open = open_rows(&ledger, now_ms).map_err(|e| e.to_string())?;
    let Some(since) = open.iter().map(|r| r.sent_at).min() else {
        return Ok(0);
    };
    let receipts = bus.receipts_since(since.saturating_sub(1_000))?;
    let window_ms = i64::try_from(config.focus_window.as_millis()).unwrap_or(i64::MAX);
    let mut done = 0;
    for row in open {
        let events = bus.lane_events(&row.lane)?;
        let keys = focus_keys(&config.team_keys, &row.tickets);
        let mut evidence = OrderEvidence::default();
        evidence.absorb(&receipts, &row.order_id);
        let timing = Timing {
            sent_at_ms: row.sent_at,
            deadline_at_ms: row.deadline_at,
            now_ms,
            focus_window_ms: window_ms,
        };
        let Some(decision) = decide(&events, evidence, timing, &keys, &row.expect) else {
            continue;
        };
        let focus = decision.focus_at_deadline.clone().unwrap_or_else(|| {
            focus::derive(&events, row.deadline_at, window_ms, None, &keys).label()
        });
        if row.result == AssignResult::Pending {
            ledger
                .connection
                .execute(
                    "UPDATE assignment_history SET result=?1, result_at=?2, confirm_latency_s=?3,
                       focus_at_deadline=?4 WHERE id=?5",
                    params![
                        decision.result.name(),
                        decision.result_at_ms,
                        decision.confirm_latency_s,
                        focus,
                        row.id
                    ],
                )
                .map_err(|e| e.to_string())?;
        } else {
            ledger
                .connection
                .execute(
                    "UPDATE assignment_history SET focus_at_deadline=?1 WHERE id=?2",
                    params![focus, row.id],
                )
                .map_err(|e| e.to_string())?;
        }
        done += 1;
    }
    Ok(done)
}

fn open_rows(ledger: &Ledger, now_ms: i64) -> Result<Vec<Open>, LedgerError> {
    let mut statement = ledger.connection.prepare(
        "SELECT id, order_id, lane, ticket, expect_ticket, sent_at, deadline_at, result
         FROM assignment_history
         WHERE order_id IS NOT NULL AND sent_at IS NOT NULL AND deadline_at IS NOT NULL
           AND deadline_at < ?1 AND (result = 'pending' OR focus_at_deadline IS NULL)
         ORDER BY id",
    )?;
    let rows = statement.query_map([now_ms], |r| {
        let tickets: String = r.get(3)?;
        let result: String = r.get(7)?;
        Ok(Open {
            id: r.get(0)?,
            order_id: r.get(1)?,
            lane: r.get(2)?,
            tickets: tickets.split(',').map(str::to_owned).collect(),
            expect: r.get(4)?,
            sent_at: r.get(5)?,
            deadline_at: r.get(6)?,
            result: AssignResult::parse(&result).unwrap_or(AssignResult::Pending),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
