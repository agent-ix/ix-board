// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `assignment_history` rows `fleet assign` writes (FR-035 behavior 10, FR-039): one row per
//! assignment, written before the send, completed with the order and again with the outcome.
//! A row left `pending` by a killed command, or confirmed before its deadline, is completed by
//! the first later committed `fleet status` run from the order's receipts and the focus at the
//! deadline.

use super::{Ledger, LedgerError, millis, resolve_path};
use crate::fleet::assign::{
    AssignBus, AssignLedger, AssignReport, AssignResult, OrderEvidence, Timing, decide, focus_keys,
};
use crate::fleet::focus;
use crate::fleet::{FleetConfig, tick::TickSettings};
use ix_trace_rs::implements;
use rusqlite::{OptionalExtension, params};
use std::path::PathBuf;

impl Ledger {
    /// Inserts the row of `report` (`row` unset) or updates it, and marks the lane's earlier
    /// rows replaced once the order is sent. Returns the row id.
    ///
    /// # Errors
    ///
    /// A database failure; nothing is changed.
    #[implements("FR-039-AC-12")]
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
                   confirm_latency_s=?10, focus_at_deadline=?11, sender_role=?12,
                   sender_addr=?13 WHERE id=?14",
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
                    report.sender_role,
                    report.sender_addr,
                    id
                ],
            )?;
            id
        } else {
            tx.execute(
                "INSERT INTO assignment_history(lane, ticket, assigned_at, order_id, expect_ticket,
                   sent_at, deadline_at, result, result_at, confirm_latency_s, focus_at_deadline,
                   sender_role, sender_addr)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
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
                    report.sender_role,
                    report.sender_addr
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
        if let (Some(order_id), Some(deadline_at), Some(label)) = (
            report.order_id.as_deref(),
            report.deadline_at_ms,
            report.focus_at_deadline.as_deref(),
        ) && report.result != AssignResult::Pending
        {
            let state = focus_state_from_label(label);
            record_order_focus(
                &tx,
                &OrderFocusRow {
                    order_id,
                    sender_role: &report.sender_role,
                    sender_addr: &report.sender_addr,
                    lane: &report.lane,
                    expect_ticket: &report.expect_ticket,
                    sent_at: report.sent_at_ms,
                    deadline_at,
                    focus_ticket: (state == "ticket").then_some(label),
                    focus_state: state,
                    followup_at: None,
                    outcome: report.result.name(),
                },
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

struct OrderFocusRow<'a> {
    order_id: &'a str,
    sender_role: &'a str,
    sender_addr: &'a str,
    lane: &'a str,
    expect_ticket: &'a str,
    sent_at: Option<i64>,
    deadline_at: i64,
    focus_ticket: Option<&'a str>,
    focus_state: &'a str,
    followup_at: Option<i64>,
    outcome: &'a str,
}

struct CompletedExternalOrder {
    order_id: String,
    sender_role: String,
    sender_addr: String,
    lane: String,
    expect_ticket: String,
    sent_at: i64,
    deadline_at: i64,
    focus_ticket: Option<String>,
    focus_state: String,
    outcome: String,
}

#[implements("FR-039-AC-12")]
fn focus_state(focus: &focus::Focus) -> &'static str {
    match focus {
        focus::Focus::Ticket { .. } => "ticket",
        focus::Focus::Ambiguous { .. } => "ambiguous",
        focus::Focus::None { .. } => "none",
    }
}

#[implements("FR-039-AC-12")]
fn focus_state_from_label(label: &str) -> &'static str {
    match label {
        "no_ticket" | "no_worktree" => "none",
        _ if label.starts_with("ambiguous:") => "ambiguous",
        _ => "ticket",
    }
}

#[implements("FR-039-AC-12")]
fn record_order_focus(
    tx: &rusqlite::Connection,
    row: &OrderFocusRow<'_>,
) -> Result<(), LedgerError> {
    tx.execute(
        "INSERT INTO order_focus_at_deadline(order_id,sender_role,sender_addr,lane,expect_ticket,
           sent_at,deadline_at,focus_ticket,focus_state,followup_at,outcome)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(order_id) DO NOTHING",
        params![
            row.order_id,
            row.sender_role,
            row.sender_addr,
            row.lane,
            row.expect_ticket,
            row.sent_at,
            row.deadline_at,
            row.focus_ticket,
            row.focus_state,
            row.followup_at,
            row.outcome
        ],
    )?;
    Ok(())
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
    sender_role: String,
    sender_addr: String,
}

/// Completes every sent assignment whose deadline has passed and whose row is `pending` or has no
/// `focus_at_deadline`, from the order's receipts and the lane's events up to the deadline.
/// Returns how many rows were completed. A ledger that does not exist yet has none.
///
/// # Errors
///
/// A ledger or relay read failure; rows completed before it stay completed.
#[implements("FR-039-AC-12")]
pub fn complete_pending(
    config: &FleetConfig,
    settings: &TickSettings,
    bus: &mut impl AssignBus,
) -> Result<usize, String> {
    let registry = bus.registry()?;
    let now_ms = registry.now_ms;
    let expired_orders = bus.expired_orders()?;
    let order_snapshot = read_order_snapshot(bus, now_ms);
    let path = resolve_path(config).map_err(|e| e.to_string())?;
    if !path.exists() {
        return Ok(0);
    }
    let ledger = Ledger::open(&path, config).map_err(|e| e.to_string())?;
    ledger
        .record_order_snapshot(now_ms, &order_snapshot)
        .map_err(|e| e.to_string())?;
    let open = open_rows(&ledger, now_ms).map_err(|e| e.to_string())?;
    let oldest_unsent_action = oldest_unsent_action(&ledger).map_err(|e| e.to_string())?;
    let oldest_order_followup = oldest_order_followup(&ledger).map_err(|e| e.to_string())?;
    drop(ledger);
    if open.is_empty()
        && expired_orders.orders.is_empty()
        && oldest_unsent_action.is_none()
        && oldest_order_followup.is_none()
        && expired_orders.skipped == 0
        && !expired_orders.truncated
    {
        return Ok(0);
    }
    let since = open
        .iter()
        .map(|row| row.sent_at)
        .chain(
            expired_orders
                .orders
                .iter()
                .map(|order| order.enqueued_at_ms),
        )
        .chain(oldest_unsent_action)
        .chain(oldest_order_followup)
        .min()
        .unwrap_or(now_ms);
    // Relay receipt retention is seven days; old unresolved rows become incomplete evidence.
    let receipt_floor = now_ms.saturating_sub(7 * 24 * 60 * 60 * 1_000);
    let receipts = bus.receipts_since(since.saturating_sub(1_000).max(receipt_floor))?;
    let mut activity = std::collections::BTreeMap::new();
    let window_ms = i64::try_from(config.focus_window.as_millis()).unwrap_or(i64::MAX);
    let mut done = 0;
    let mut coverage_issues = Vec::new();
    let mut external_rows = Vec::new();
    for order in &expired_orders.orders {
        let Some(expect_ticket) = order.expect_ticket.as_deref() else {
            continue;
        };
        let Some(queued) = receipts.iter().find(|event| {
            event.original_id == order.original_id
                && event.outcome == relay_core::receipts::ReceiptOutcome::Queued
        }) else {
            coverage_issues.push(format!(
                "relay order {} has no retained Queued receipt for sender attribution",
                order.original_id
            ));
            continue;
        };
        let Some(receipt_target) = receipt_target_addr(&registry, &queued.target) else {
            coverage_issues.push(format!(
                "relay order {} recipient queue {} has no verified lane address",
                order.original_id, queued.target
            ));
            continue;
        };
        let Some(sender_addr) = receipt_sender_addr(&registry, &queued.sender) else {
            coverage_issues.push(format!(
                "relay order {} sender {} has no verified address",
                order.original_id, queued.sender
            ));
            continue;
        };
        if queued.expect_ticket.as_deref() != Some(expect_ticket)
            || !receipt_target.eq_ignore_ascii_case(&order.recipient)
        {
            coverage_issues.push(format!(
                "relay order {} does not match its retained Queued receipt",
                order.original_id
            ));
            continue;
        }
        let sender_role = settings.role_of(&sender_addr);
        if !activity.contains_key(&order.recipient) {
            activity.insert(order.recipient.clone(), bus.lane_events(&order.recipient)?);
        }
        let events = activity.get(&order.recipient).expect("inserted activity");
        let keys = focus_keys(&config.team_keys, &[expect_ticket.to_owned()]);
        let mut evidence = OrderEvidence::default();
        evidence.absorb(&receipts, &order.original_id);
        let decision = decide(
            events,
            evidence,
            Timing {
                sent_at_ms: order.enqueued_at_ms,
                deadline_at_ms: order.deadline_ms,
                now_ms,
                focus_window_ms: window_ms,
            },
            &keys,
            expect_ticket,
        )
        .ok_or_else(|| {
            format!(
                "relay expired order {} has no completed outcome at its deadline",
                order.original_id
            )
        })?;
        let focus = focus::derive(events, order.deadline_ms, window_ms, None, &keys);
        let focus_ticket = focus.ticket();
        let focus_state = focus_state(&focus);
        external_rows.push(CompletedExternalOrder {
            order_id: order.original_id.clone(),
            sender_role: sender_role.to_owned(),
            sender_addr,
            lane: order.recipient.clone(),
            expect_ticket: expect_ticket.to_owned(),
            sent_at: order.enqueued_at_ms,
            deadline_at: order.deadline_ms,
            focus_ticket: focus_ticket.map(str::to_owned),
            focus_state: focus_state.to_owned(),
            outcome: decision.result.name().to_owned(),
        });
    }
    for row in &open {
        if row.sent_at >= receipt_floor && !activity.contains_key(&row.lane) {
            activity.insert(row.lane.clone(), bus.lane_events(&row.lane)?);
        }
    }
    let ledger = Ledger::open(&path, config).map_err(|e| e.to_string())?;
    write_action_sends(&ledger.connection, settings, &registry, &receipts)
        .map_err(|e| e.to_string())?;
    for row in external_rows {
        record_order_focus(
            &ledger.connection,
            &OrderFocusRow {
                order_id: &row.order_id,
                sender_role: &row.sender_role,
                sender_addr: &row.sender_addr,
                lane: &row.lane,
                expect_ticket: &row.expect_ticket,
                sent_at: Some(row.sent_at),
                deadline_at: row.deadline_at,
                focus_ticket: row.focus_ticket.as_deref(),
                focus_state: &row.focus_state,
                followup_at: None,
                outcome: &row.outcome,
            },
        )
        .map_err(|error| error.to_string())?;
        done += 1;
    }
    for row in open {
        if row.sent_at < receipt_floor {
            coverage_issues.push(format!(
                "assignment order {} predates relay receipt retention; historical focus is not backfilled",
                row.order_id
            ));
            continue;
        }
        let events = activity
            .get(&row.lane)
            .expect("recent assignment activity was prefetched");
        let keys = focus_keys(&config.team_keys, &row.tickets);
        let mut evidence = OrderEvidence::default();
        evidence.absorb(&receipts, &row.order_id);
        let timing = Timing {
            sent_at_ms: row.sent_at,
            deadline_at_ms: row.deadline_at,
            now_ms,
            focus_window_ms: window_ms,
        };
        let Some(decision) = decide(events, evidence, timing, &keys, &row.expect) else {
            continue;
        };
        let deadline_focus = focus::derive(events, row.deadline_at, window_ms, None, &keys);
        let focus = deadline_focus.label();
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
        record_order_focus(
            &ledger.connection,
            &OrderFocusRow {
                order_id: &row.order_id,
                sender_role: &row.sender_role,
                sender_addr: &row.sender_addr,
                lane: &row.lane,
                expect_ticket: &row.expect,
                sent_at: Some(row.sent_at),
                deadline_at: row.deadline_at,
                focus_ticket: deadline_focus.ticket(),
                focus_state: focus_state(&deadline_focus),
                followup_at: None,
                outcome: decision.result.name(),
            },
        )
        .map_err(|e| e.to_string())?;
        done += 1;
    }
    update_order_followups(&ledger.connection, settings, &registry, &receipts)
        .map_err(|e| e.to_string())?;
    if expired_orders.skipped > 0 || expired_orders.truncated {
        coverage_issues.push(format!(
            "relay expired-order coverage is incomplete (skipped {}, truncated {})",
            expired_orders.skipped, expired_orders.truncated
        ));
    }
    if !expired_orders.orders.is_empty() && expired_orders.indexed_since_ms.is_none() {
        coverage_issues.push("relay expired-order index has not started".to_owned());
    }
    if !coverage_issues.is_empty() {
        return Err(coverage_issues.join("; "));
    }
    Ok(done)
}

struct OrderSnapshot {
    available: bool,
    unavailable_reason: Option<String>,
    oldest_available_at: Option<i64>,
    window_since: i64,
    window_until: i64,
    skipped: usize,
    orders: Vec<relay_core::receipts::OrderState>,
}

impl Ledger {
    #[implements("FR-039-AC-12")]
    fn record_order_snapshot(
        &self,
        observed_at: i64,
        snapshot: &OrderSnapshot,
    ) -> Result<(), LedgerError> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO relay_order_snapshot(observed_at,window_since,window_until,
               oldest_available_at,available,unavailable_reason,skipped)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                observed_at,
                snapshot.window_since,
                snapshot.window_until,
                snapshot.oldest_available_at,
                snapshot.available,
                snapshot.unavailable_reason,
                snapshot.skipped,
            ],
        )?;
        let snapshot_id = tx.last_insert_rowid();
        if snapshot.available {
            let mut insert = tx.prepare(
                "INSERT INTO relay_order(snapshot_id,order_id,sender,recipient,receipt_target,
                   expect_ticket,ack_ms,deadline_at,enqueued_at,delivered_at,acted_at,state,expired)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            )?;
            for order in &snapshot.orders {
                insert.execute(rusqlite::params![
                    snapshot_id,
                    order.order_id,
                    order.sender,
                    order.recipient,
                    order.receipt_target,
                    order.expect_ticket,
                    order.ack_ms,
                    order.deadline_at,
                    order.enqueued_at_ms,
                    order.delivered_at_ms,
                    order.acted_at_ms,
                    order.state,
                    order.expired,
                ])?;
            }
        }
        let cutoff = observed_at.saturating_sub(millis(self.retention));
        tx.execute(
            "DELETE FROM relay_order WHERE snapshot_id IN
               (SELECT snapshot_id FROM relay_order_snapshot WHERE observed_at < ?1)",
            [cutoff],
        )?;
        tx.execute(
            "DELETE FROM relay_order_snapshot WHERE observed_at < ?1",
            [cutoff],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[implements("FR-039-AC-12")]
fn read_order_snapshot(bus: &mut impl AssignBus, now_ms: i64) -> OrderSnapshot {
    const RETENTION_MS: i64 = 8 * 24 * 60 * 60 * 1_000;
    let retention_floor = now_ms.saturating_sub(RETENTION_MS).max(0);
    let confirmed_since = match bus.orders_complete_since() {
        Ok(Some(boundary)) => boundary,
        Ok(None) => {
            return unavailable_order_snapshot(
                retention_floor,
                now_ms,
                "coverage_unconfirmed".to_owned(),
            );
        }
        Err(error) => return unavailable_order_snapshot(retention_floor, now_ms, error),
    };
    read_order_snapshot_since(bus, now_ms, retention_floor.max(confirmed_since), false)
}

#[implements("FR-039-AC-12")]
fn read_order_snapshot_since(
    bus: &mut impl AssignBus,
    now_ms: i64,
    since_ms: i64,
    retried_boundary: bool,
) -> OrderSnapshot {
    const PAGE_LIMIT: usize = 500;
    const MAX_PAGES: usize = 1_000;
    let mut cursor: Option<relay_core::receipts::OrderCursor> = None;
    let mut all = Vec::new();
    let mut previous: Option<(i64, String)> = None;
    for _ in 0..MAX_PAGES {
        let request = match relay_core::receipts::OrderListRequest::new(
            since_ms,
            now_ms,
            PAGE_LIMIT,
            cursor.clone(),
        ) {
            Ok(request) => request,
            Err(error) => return unavailable_order_snapshot(since_ms, now_ms, error),
        };
        let page = match bus.orders(&request) {
            Ok(page) => page,
            Err(error) => return unavailable_order_snapshot(since_ms, now_ms, error),
        };
        if page.window.since != since_ms
            || page.window.until != now_ms
            || page.window.filter != "enqueued_at"
            || page.window.order != "newest_first"
            || page.limit != PAGE_LIMIT
        {
            return unavailable_order_snapshot(
                since_ms,
                now_ms,
                "relay order page does not match the requested window contract".to_owned(),
            );
        }
        if !page.complete || page.skipped > 0 || page.incomplete_reason.is_some() {
            if !retried_boundary
                && all.is_empty()
                && cursor.is_none()
                && page.skipped == 0
                && matches!(
                    page.incomplete_reason.as_deref(),
                    Some("before_index_start" | "before_complete_coverage" | "outside_retention")
                )
                && page
                    .oldest_available_at
                    .is_some_and(|oldest| oldest > since_ms)
            {
                return read_order_snapshot_since(
                    bus,
                    now_ms,
                    page.oldest_available_at.expect("checked boundary"),
                    true,
                );
            }
            let reason = page.incomplete_reason.unwrap_or_else(|| {
                if page.skipped > 0 {
                    "unreadable_order_records".to_owned()
                } else {
                    "relay order page is incomplete".to_owned()
                }
            });
            let mut snapshot = unavailable_order_snapshot(since_ms, now_ms, reason);
            snapshot.skipped = page.skipped;
            return snapshot;
        }
        let Some(oldest) = page.oldest_available_at else {
            return unavailable_order_snapshot(
                since_ms,
                now_ms,
                "relay order index has no complete-coverage boundary".to_owned(),
            );
        };
        if oldest > since_ms {
            return unavailable_order_snapshot(
                since_ms,
                now_ms,
                "relay order index does not cover the requested retention window".to_owned(),
            );
        }
        for order in &page.orders {
            let key = (order.enqueued_at_ms, order.order_id.clone());
            if !(since_ms..=now_ms).contains(&order.enqueued_at_ms)
                || previous.as_ref().is_some_and(|prior| key >= *prior)
            {
                return unavailable_order_snapshot(
                    since_ms,
                    now_ms,
                    "relay order page is outside its window or not strictly newest-first"
                        .to_owned(),
                );
            }
            previous = Some(key);
        }
        if page.truncated != page.next_cursor.is_some() {
            return unavailable_order_snapshot(
                since_ms,
                now_ms,
                "relay order page has an invalid cursor/truncated combination".to_owned(),
            );
        }
        if let Some(next) = page.next_cursor {
            let Some(last) = page.orders.last() else {
                return unavailable_order_snapshot(
                    since_ms,
                    now_ms,
                    "relay returned a cursor for an empty order page".to_owned(),
                );
            };
            if next.enqueued_at_ms != last.enqueued_at_ms
                || next.order_id != last.order_id
                || cursor.as_ref().is_some_and(|prior| {
                    (next.enqueued_at_ms, next.order_id.as_str())
                        >= (prior.enqueued_at_ms, prior.order_id.as_str())
                })
            {
                return unavailable_order_snapshot(
                    since_ms,
                    now_ms,
                    "relay order cursor did not advance strictly".to_owned(),
                );
            }
            all.extend(page.orders);
            cursor = Some(next);
        } else {
            all.extend(page.orders);
            return OrderSnapshot {
                available: true,
                unavailable_reason: None,
                oldest_available_at: Some(oldest),
                window_since: since_ms,
                window_until: now_ms,
                skipped: 0,
                orders: all,
            };
        }
    }
    unavailable_order_snapshot(
        since_ms,
        now_ms,
        "relay order pagination exceeded the safety limit".to_owned(),
    )
}

fn unavailable_order_snapshot(since_ms: i64, now_ms: i64, reason: String) -> OrderSnapshot {
    OrderSnapshot {
        available: false,
        unavailable_reason: Some(reason),
        oldest_available_at: None,
        window_since: since_ms,
        window_until: now_ms,
        skipped: 0,
        orders: Vec::new(),
    }
}

#[implements("FR-039-AC-13")]
fn oldest_unsent_action(ledger: &Ledger) -> Result<Option<i64>, LedgerError> {
    Ok(ledger.connection.query_row(
        "SELECT MIN(a.emitted_at) FROM action a WHERE a.target_addr IS NOT NULL
           AND a.resolved_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM action_send s WHERE s.action_id = a.id)",
        [],
        |row| row.get(0),
    )?)
}

#[implements("FR-039-AC-12")]
fn oldest_order_followup(ledger: &Ledger) -> Result<Option<i64>, LedgerError> {
    Ok(ledger.connection.query_row(
        "SELECT MIN(deadline_at) FROM order_focus_at_deadline WHERE followup_at IS NULL
           AND outcome IN ('acted_not_focused','ack_expired','timed_out')",
        [],
        |row| row.get(0),
    )?)
}

#[implements("FR-039-AC-12")]
fn update_order_followups(
    connection: &rusqlite::Connection,
    settings: &TickSettings,
    registry: &super::super::assign::RegistryView,
    receipts: &[relay_core::receipts::ReceiptEvent],
) -> Result<usize, LedgerError> {
    let mut statement = connection.prepare(
        "SELECT order_id,sender_addr,lane,deadline_at FROM order_focus_at_deadline
         WHERE followup_at IS NULL",
    )?;
    let pending = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let mut updated = 0;
    for (order_id, sender, lane, deadline_at) in pending {
        let lead = settings.leads.get(&lane.to_ascii_lowercase());
        let followup_at = receipts
            .iter()
            .filter(|event| {
                event.outcome == relay_core::receipts::ReceiptOutcome::Queued
                    && receipt_sender_addr(registry, &event.sender)
                        .is_some_and(|address| address.eq_ignore_ascii_case(&sender))
                    && event.observed_at_ms > deadline_at
                    && (receipt_target_addr(registry, &event.target)
                        .is_some_and(|address| address.eq_ignore_ascii_case(&lane))
                        || (receipt_target_addr(registry, &event.target).is_some_and(|address| {
                            lead.is_some_and(|lead| address.eq_ignore_ascii_case(lead))
                        })))
            })
            .map(|event| event.observed_at_ms)
            .min();
        if let Some(followup_at) = followup_at {
            updated += connection.execute(
                "UPDATE order_focus_at_deadline SET followup_at=?1
                 WHERE order_id=?2 AND followup_at IS NULL",
                params![followup_at, order_id],
            )?;
        }
    }
    Ok(updated)
}

/// Matches each retained minder send to at most one action row in emission order.
#[implements("FR-039-AC-13")]
fn write_action_sends(
    connection: &rusqlite::Connection,
    settings: &TickSettings,
    registry: &super::super::assign::RegistryView,
    receipts: &[relay_core::receipts::ReceiptEvent],
) -> Result<usize, LedgerError> {
    let mut recorded = 0;
    for receipt in receipts
        .iter()
        .filter(|event| event.outcome == relay_core::receipts::ReceiptOutcome::Queued)
    {
        let sent_at = receipt.observed_at_ms;
        let Some(sender_addr) = receipt_sender_addr(registry, &receipt.sender) else {
            continue;
        };
        let Some(recipient_addr) = receipt_target_addr(registry, &receipt.target) else {
            continue;
        };
        let sender_role = if settings
            .orchestrator
            .as_deref()
            .is_some_and(|orchestrator| sender_addr.eq_ignore_ascii_case(orchestrator))
        {
            "orchestrator"
        } else if settings
            .minder
            .as_deref()
            .is_some_and(|minder| sender_addr.eq_ignore_ascii_case(minder))
        {
            "minder"
        } else {
            continue;
        };
        let action_id: Option<i64> = connection
            .query_row(
                "SELECT a.id FROM action a WHERE a.target_addr=?1 AND a.emitted_at <= ?2
                   AND a.resolved_at IS NULL
                   AND (?4=?5 OR ?4=?6)
                   AND NOT EXISTS (SELECT 1 FROM action_send s WHERE s.action_id = a.id)
                   AND NOT EXISTS (SELECT 1 FROM action_send s WHERE s.receipt_id = ?3)
                 ORDER BY COALESCE(a.rung,0), a.emitted_at, a.id LIMIT 1",
                params![
                    recipient_addr,
                    sent_at,
                    receipt.original_id,
                    sender_addr,
                    settings.orchestrator,
                    settings.minder
                ],
                |row| row.get(0),
            )
            .optional()?;
        let Some(action_id) = action_id else {
            continue;
        };
        recorded += connection.execute(
            "INSERT INTO action_send(action_id,sent_at,sender_addr,recipient_addr,receipt_id,sender_role)
             VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(action_id,receipt_id) DO NOTHING",
            params![
                action_id,
                sent_at,
                sender_addr,
                recipient_addr,
                receipt.original_id,
                sender_role
            ],
        )?;
    }
    Ok(recorded)
}

fn receipt_target_addr(
    registry: &super::super::assign::RegistryView,
    target: &str,
) -> Option<String> {
    registry
        .registrations
        .iter()
        .find(|(queue, _)| queue.as_str().eq_ignore_ascii_case(target))
        .map(|(_, address)| address.clone())
        .or_else(|| {
            target
                .parse::<relay_core::Address>()
                .ok()
                .map(|address| address.to_string())
        })
}

fn receipt_sender_addr(
    registry: &super::super::assign::RegistryView,
    sender: &str,
) -> Option<String> {
    receipt_target_addr(registry, sender)
}

#[implements("FR-039-AC-12")]
fn open_rows(ledger: &Ledger, now_ms: i64) -> Result<Vec<Open>, LedgerError> {
    let mut statement = ledger.connection.prepare(
        "SELECT id, order_id, lane, ticket, expect_ticket, sent_at, deadline_at, result,
                COALESCE(sender_role,'orchestrator'), COALESCE(sender_addr,'ix-board')
         FROM assignment_history
         WHERE order_id IS NOT NULL AND sent_at IS NOT NULL AND deadline_at IS NOT NULL
           AND deadline_at < ?1 AND result = 'pending'
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
            sender_role: r.get(8)?,
            sender_addr: r.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::assign::{AssignBus, RegistryView};
    use crate::fleet::focus::FocusEvent;
    use ix_trace_rs::trace;
    use relay_core::assignment::{AssignWriter, AssignedTicket, AssignmentList};
    use relay_core::receipts::{
        ExpiredOrderState, ExpiredOrders, OrderCursor, OrderEscalationState, OrderPage, OrderSpec,
        OrderState, OrderWindow, ReceiptEvent, ReceiptOutcome,
    };
    use relay_core::{AgentName, Message};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    struct ExpiredBus {
        now_ms: i64,
        expired: ExpiredOrders,
        receipts: Vec<ReceiptEvent>,
        order_pages: Vec<OrderPage>,
        coverage_since: Option<i64>,
    }

    impl AssignBus for ExpiredBus {
        fn registry(&mut self) -> Result<RegistryView, String> {
            Ok(RegistryView {
                now_ms: self.now_ms,
                lanes: Vec::new(),
                registrations: vec![
                    (
                        AgentName::try_from("session/74657374/636f646572".to_owned()).unwrap(),
                        "test-server:test-workspace:test-coder".to_owned(),
                    ),
                    (
                        AgentName::try_from("session/706c616e6e6572/706c616e6e6572".to_owned())
                            .unwrap(),
                        "planner:one".to_owned(),
                    ),
                ],
            })
        }

        fn assignments(&mut self) -> Result<Vec<AssignmentList>, String> {
            Err("unused in this test".to_owned())
        }

        fn send_order(
            &mut self,
            _msg: &Message,
            _spec: &OrderSpec,
        ) -> Result<Vec<AgentName>, String> {
            Err("unused in this test".to_owned())
        }

        fn receipts_since(&mut self, _since_ms: i64) -> Result<Vec<ReceiptEvent>, String> {
            Ok(self.receipts.clone())
        }

        fn expired_orders(&mut self) -> Result<ExpiredOrders, String> {
            Ok(self.expired.clone())
        }

        fn orders(
            &mut self,
            _request: &relay_core::receipts::OrderListRequest,
        ) -> Result<OrderPage, String> {
            if self.order_pages.is_empty() {
                Err("relay all-orders source is unavailable".to_owned())
            } else {
                Ok(self.order_pages.remove(0))
            }
        }

        fn orders_complete_since(&mut self) -> Result<Option<i64>, String> {
            Ok(self.coverage_since)
        }

        fn write_list(
            &mut self,
            _writer: &AssignWriter,
            _lane: &str,
            _tickets: Vec<AssignedTicket>,
            _order_id: &str,
        ) -> Result<AssignmentList, String> {
            Err("unused in this test".to_owned())
        }

        fn lane_events(&mut self, _lane: &str) -> Result<Vec<FocusEvent>, String> {
            Ok(Vec::new())
        }
    }

    fn expired_bus(truncated: bool) -> ExpiredBus {
        let order_id = "external-order";
        let sender = "test-server:test-workspace:test-minder";
        let target = "test-server:test-workspace:test-coder";
        let target_queue = "session/74657374/636f646572";
        let expect_ticket = "SWM-42";
        let mut receipts = Vec::new();
        for (event_id, outcome, observed_at_ms) in [
            ("queued", ReceiptOutcome::Queued, 100),
            ("delivered", ReceiptOutcome::Delivered, 500),
            ("expired", ReceiptOutcome::AckExpired, 1_100),
        ] {
            receipts.push(ReceiptEvent {
                event_id: event_id.to_owned(),
                original_id: order_id.to_owned(),
                attempt_id: None,
                outcome,
                sender: sender.to_owned(),
                target: target_queue.to_owned(),
                event_time_ms: Some(100),
                observed_at_ms,
                evidence: None,
                reason: None,
                session: None,
                expect_ticket: Some(expect_ticket.to_owned()),
                delivered_at_ms: None,
            });
        }
        receipts.push(ReceiptEvent {
            event_id: "followup-event".to_owned(),
            original_id: "followup-message".to_owned(),
            attempt_id: None,
            outcome: ReceiptOutcome::Queued,
            sender: sender.to_owned(),
            target: "test-server:test-workspace:test-lead".to_owned(),
            event_time_ms: Some(1_200),
            observed_at_ms: 1_200,
            evidence: None,
            reason: None,
            session: None,
            expect_ticket: None,
            delivered_at_ms: None,
        });
        ExpiredBus {
            now_ms: 1_200,
            expired: ExpiredOrders {
                orders: vec![ExpiredOrderState {
                    original_id: order_id.to_owned(),
                    recipient: target.to_owned(),
                    expect_ticket: Some(expect_ticket.to_owned()),
                    enqueued_at_ms: 100,
                    deadline_ms: 1_000,
                    expired_at_ms: 1_100,
                    delivered_before_expiry: true,
                    last_state: "delivered".to_owned(),
                    escalation: OrderEscalationState::Pending,
                }],
                skipped: 0,
                indexed_since_ms: Some(0),
                truncated,
            },
            receipts,
            order_pages: Vec::new(),
            coverage_since: None,
        }
    }

    fn order_page(
        since: i64,
        until: i64,
        orders: Vec<OrderState>,
        cursor: Option<OrderCursor>,
        complete: bool,
        incomplete_reason: Option<&str>,
        skipped: usize,
    ) -> OrderPage {
        OrderPage {
            truncated: cursor.is_some(),
            next_cursor: cursor,
            window: OrderWindow {
                since,
                until,
                filter: "enqueued_at",
                order: "newest_first",
            },
            limit: 500,
            oldest_available_at: Some(since),
            complete,
            incomplete_reason: incomplete_reason.map(str::to_owned),
            skipped,
            orders,
        }
    }

    fn order_state(id: &str, enqueued_at_ms: i64, state: &str, expired: bool) -> OrderState {
        OrderState {
            order_id: id.to_owned(),
            sender: "test-server:test-workspace:test-minder".to_owned(),
            recipient: "test-server:test-workspace:test-coder".to_owned(),
            receipt_target: "session/74657374/636f646572".to_owned(),
            expect_ticket: Some("SWM-42".to_owned()),
            ack_ms: 900,
            deadline_at: enqueued_at_ms + 900,
            enqueued_at_ms,
            delivered_at_ms: Some(enqueued_at_ms + 10),
            acted_at_ms: None,
            state: state.to_owned(),
            expired,
        }
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn order_reader_paginates_and_preserves_terminal_state_and_expired_flag() {
        let mut bus = expired_bus(false);
        bus.coverage_since = Some(0);
        bus.order_pages = vec![
            order_page(
                0,
                1_200,
                vec![order_state("z", 1_100, "expired", true)],
                Some(OrderCursor {
                    enqueued_at_ms: 1_100,
                    order_id: "z".to_owned(),
                }),
                true,
                None,
                0,
            ),
            order_page(
                0,
                1_200,
                vec![order_state("a", 1_100, "acted", true)],
                None,
                true,
                None,
                0,
            ),
        ];
        let snapshot = read_order_snapshot(&mut bus, 1_200);
        assert!(snapshot.available);
        assert_eq!(snapshot.orders.len(), 2);
        assert_eq!(snapshot.orders[0].state, "expired");
        assert!(snapshot.orders[0].expired);
        assert_eq!(snapshot.orders[1].state, "acted");
        assert!(snapshot.orders[1].expired);
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn incomplete_or_skipped_order_page_is_unavailable_not_a_partial_population() {
        let mut bus = expired_bus(false);
        bus.coverage_since = Some(0);
        bus.order_pages = vec![order_page(
            0,
            1_200,
            vec![order_state("z", 1_100, "open", false)],
            None,
            false,
            Some("unreadable_order_records"),
            1,
        )];
        let snapshot = read_order_snapshot(&mut bus, 1_200);
        assert!(!snapshot.available);
        assert_eq!(
            snapshot.unavailable_reason.as_deref(),
            Some("unreadable_order_records")
        );
        assert_eq!(snapshot.skipped, 1);
        assert!(snapshot.orders.is_empty());
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn confirmed_coverage_boundary_clamps_the_order_window_before_snapshotting() {
        let mut bus = expired_bus(false);
        bus.coverage_since = Some(1_000);
        let mut before_boundary = order_page(
            1_000,
            1_200,
            Vec::new(),
            None,
            false,
            Some("before_index_start"),
            0,
        );
        before_boundary.oldest_available_at = Some(1_100);
        bus.order_pages = vec![
            before_boundary,
            order_page(
                1_100,
                1_200,
                vec![order_state("new", 1_150, "open", false)],
                None,
                true,
                None,
                0,
            ),
        ];
        let snapshot = read_order_snapshot(&mut bus, 1_200);
        assert!(snapshot.available);
        assert_eq!(snapshot.window_since, 1_100);
        assert_eq!(snapshot.oldest_available_at, Some(1_100));
        assert_eq!(snapshot.orders.len(), 1);
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn no_operator_coverage_confirmation_keeps_order_metrics_unavailable() {
        let mut bus = expired_bus(false);
        let snapshot = read_order_snapshot(&mut bus, 1_200);
        assert!(!snapshot.available);
        assert_eq!(
            snapshot.unavailable_reason.as_deref(),
            Some("coverage_unconfirmed")
        );
        assert!(snapshot.orders.is_empty());
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn coverage_unconfirmed_page_with_null_boundary_is_unavailable() {
        let mut bus = expired_bus(false);
        bus.coverage_since = Some(1_000);
        let mut page = order_page(
            1_000,
            1_200,
            Vec::new(),
            None,
            false,
            Some("coverage_unconfirmed"),
            0,
        );
        page.oldest_available_at = None;
        bus.order_pages = vec![page];
        let snapshot = read_order_snapshot(&mut bus, 1_200);
        assert!(!snapshot.available);
        assert_eq!(
            snapshot.unavailable_reason.as_deref(),
            Some("coverage_unconfirmed")
        );
        assert!(snapshot.oldest_available_at.is_none());
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn every_page_must_be_complete_or_the_snapshot_discards_earlier_rows() {
        let mut bus = expired_bus(false);
        bus.coverage_since = Some(0);
        bus.order_pages = vec![
            order_page(
                0,
                1_200,
                vec![order_state("z", 1_100, "open", false)],
                Some(OrderCursor {
                    enqueued_at_ms: 1_100,
                    order_id: "z".to_owned(),
                }),
                true,
                None,
                0,
            ),
            order_page(
                0,
                1_200,
                vec![order_state("a", 1_000, "open", false)],
                None,
                false,
                Some("unreadable_order_records"),
                1,
            ),
        ];
        let snapshot = read_order_snapshot(&mut bus, 1_200);
        assert!(!snapshot.available);
        assert_eq!(snapshot.skipped, 1);
        assert!(snapshot.orders.is_empty());
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn lane_order_views_use_full_snapshot_and_keep_unavailable_reason() {
        let (_directory, config, path) = test_ledger();
        let ledger = Ledger::open(&path, &config).unwrap();
        let snapshot = OrderSnapshot {
            available: true,
            unavailable_reason: None,
            oldest_available_at: Some(0),
            window_since: 0,
            window_until: 1_200,
            skipped: 0,
            orders: vec![order_state("external", 100, "expired", true)],
        };
        ledger.record_order_snapshot(1_200, &snapshot).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let due: (String, String, i64) = connection
            .query_row(
                "SELECT order_id,state,available FROM lane_orders_due",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(due, ("external".to_owned(), "expired".to_owned(), 1));
        let not_taken_up: i64 = connection
            .query_row("SELECT COUNT(*) FROM lane_orders_not_taken_up", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(not_taken_up, 1);
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn newest_incomplete_order_snapshot_hides_older_complete_population() {
        let (_directory, config, path) = test_ledger();
        let ledger = Ledger::open(&path, &config).unwrap();
        let complete = OrderSnapshot {
            available: true,
            unavailable_reason: None,
            oldest_available_at: Some(0),
            window_since: 0,
            window_until: 1_199,
            skipped: 0,
            orders: vec![order_state("older", 100, "expired", true)],
        };
        ledger.record_order_snapshot(1_199, &complete).unwrap();
        let mut incomplete =
            unavailable_order_snapshot(0, 1_200, "unreadable_order_records".to_owned());
        incomplete.skipped = 1;
        ledger.record_order_snapshot(1_200, &incomplete).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let sentinel: (Option<String>, i64, String) = connection
            .query_row(
                "SELECT order_id,available,unavailable_reason FROM lane_orders_due",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(sentinel, (None, 0, "unreadable_order_records".to_owned()));
        let rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM lane_orders_due WHERE order_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn later_lane_order_sent_before_deadline_supersedes_the_earlier_due_row() {
        let (_directory, config, path) = test_ledger();
        let ledger = Ledger::open(&path, &config).unwrap();
        let snapshot = OrderSnapshot {
            available: true,
            unavailable_reason: None,
            oldest_available_at: Some(0),
            window_since: 0,
            window_until: 1_200,
            skipped: 0,
            orders: vec![
                order_state("old", 100, "expired", true),
                order_state("new", 500, "open", false),
            ],
        };
        ledger.record_order_snapshot(1_200, &snapshot).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let due: i64 = connection
            .query_row("SELECT COUNT(*) FROM lane_orders_due", [], |row| row.get(0))
            .unwrap();
        let not_taken_up: i64 = connection
            .query_row("SELECT COUNT(*) FROM lane_orders_not_taken_up", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(due, 0);
        assert_eq!(not_taken_up, 0);
    }

    fn test_ledger() -> (tempfile::TempDir, FleetConfig, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fleet.sqlite3");
        let config = FleetConfig {
            ledger_path: Some(path.clone()),
            ..FleetConfig::default()
        };
        drop(Ledger::open(&path, &config).unwrap());
        (directory, config, path)
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn external_expired_orders_record_role_deadline_focus_and_receipt_outcome() {
        let (_directory, config, path) = test_ledger();
        let settings = TickSettings {
            minder: Some("test-server:test-workspace:test-minder".to_owned()),
            leads: BTreeMap::from([(
                "test-server:test-workspace:test-coder".to_owned(),
                "test-server:test-workspace:test-lead".to_owned(),
            )]),
            ..TickSettings::default()
        };
        let mut bus = expired_bus(false);

        assert_eq!(complete_pending(&config, &settings, &mut bus).unwrap(), 1);
        let connection = rusqlite::Connection::open(path).unwrap();
        let row: (
            String,
            String,
            String,
            i64,
            Option<String>,
            String,
            Option<i64>,
            String,
        ) = connection
            .query_row(
                "SELECT sender_role,sender_addr,lane,deadline_at,focus_ticket,focus_state,
                        followup_at,outcome
                   FROM order_focus_at_deadline WHERE order_id='external-order'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "minder");
        assert_eq!(row.1, "test-server:test-workspace:test-minder");
        assert_eq!(row.2, "test-server:test-workspace:test-coder");
        assert_eq!(row.3, 1_000);
        assert_eq!(row.4, None);
        assert_eq!(row.5, "none");
        assert_eq!(row.6, Some(1_200));
        assert_eq!(row.7, "ack_expired");
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn incomplete_external_expiry_coverage_is_reported_after_known_rows_are_written() {
        let (_directory, config, path) = test_ledger();
        let settings = TickSettings {
            minder: Some("test-server:test-workspace:test-minder".to_owned()),
            ..TickSettings::default()
        };
        let mut bus = expired_bus(true);

        let error = complete_pending(&config, &settings, &mut bus).unwrap_err();
        assert!(error.contains("truncated true"));
        let connection = rusqlite::Connection::open(path).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM order_focus_at_deadline", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn an_expired_order_without_its_queued_receipt_does_not_abort_other_order_rows() {
        let (_directory, config, path) = test_ledger();
        let settings = TickSettings {
            minder: Some("test-server:test-workspace:test-minder".to_owned()),
            ..TickSettings::default()
        };
        let mut bus = expired_bus(false);
        bus.expired.orders.push(ExpiredOrderState {
            original_id: "aged-out-order".to_owned(),
            recipient: "test-server:test-workspace:test-coder".to_owned(),
            expect_ticket: Some("SWM-99".to_owned()),
            enqueued_at_ms: 20,
            deadline_ms: 900,
            expired_at_ms: 901,
            delivered_before_expiry: true,
            last_state: "delivered".to_owned(),
            escalation: OrderEscalationState::Pending,
        });

        let error = complete_pending(&config, &settings, &mut bus).unwrap_err();
        assert!(error.contains("aged-out-order"));
        let connection = rusqlite::Connection::open(path).unwrap();
        let rows: Vec<String> = connection
            .prepare("SELECT order_id FROM order_focus_at_deadline ORDER BY order_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, ["external-order"]);
    }

    #[trace("FR-039-AC-12")]
    #[trace("FR-035-AC-11")]
    #[test]
    fn recent_confirmed_orders_fill_deadline_focus_once_without_changing_the_outcome() {
        let (_directory, config, path) = test_ledger();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch(
            "INSERT INTO assignment_history(lane,ticket,assigned_at,order_id,expect_ticket,
               sent_at,deadline_at,result,result_at,confirm_latency_s,focus_at_deadline,
               sender_role,sender_addr)
             VALUES('test-server:test-workspace:test-coder','SWM-9',100,'recent-order','SWM-9',100,200,
               'confirmed',150,1,NULL,'minder','test-server:test-workspace:test-minder');",
        ).unwrap();
        let mut bus = expired_bus(false);
        bus.expired.orders.clear();
        bus.receipts.clear();
        let settings = TickSettings::default();
        assert_eq!(complete_pending(&config, &settings, &mut bus).unwrap(), 1);
        let stored: (String, i64, i64, String) = connection.query_row(
            "SELECT result,result_at,confirm_latency_s,focus_at_deadline FROM assignment_history
             WHERE order_id='recent-order'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).unwrap();
        assert_eq!(stored, ("confirmed".into(), 150, 1, "no_worktree".into()));
        let focus_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM order_focus_at_deadline WHERE order_id='recent-order'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(focus_rows, 1);
        assert_eq!(complete_pending(&config, &settings, &mut bus).unwrap(), 0);
    }

    #[trace("FR-039-AC-12")]
    #[test]
    fn completed_local_orders_outside_receipt_retention_are_not_reprocessed() {
        let (_directory, config, path) = test_ledger();
        let settings = TickSettings::default();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO assignment_history(lane,ticket,assigned_at,order_id,expect_ticket,
                   sent_at,deadline_at,result,result_at,confirm_latency_s,focus_at_deadline,
                   sender_role,sender_addr)
                 VALUES('test-server:test-workspace:test-coder','SWM-9',100,'old-order','SWM-9',100,200,
                   'confirmed',150,1,NULL,'minder','test-server:test-workspace:test-minder')",
                [],
            )
            .unwrap();
        drop(connection);
        let mut bus = expired_bus(false);
        bus.now_ms = 7 * 24 * 60 * 60 * 1_000 + 1_200;
        bus.expired.orders.clear();
        bus.receipts.clear();

        assert_eq!(complete_pending(&config, &settings, &mut bus).unwrap(), 0);
        let connection = rusqlite::Connection::open(path).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM order_focus_at_deadline WHERE order_id='old-order'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[trace("FR-039-AC-13")]
    #[test]
    fn one_queued_executor_receipt_skips_resolved_actions_and_satisfies_only_one_open_action() {
        let (_directory, _config, path) = test_ledger();
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute_batch(
                "INSERT INTO tick(started_at,committed_at) VALUES(0,0);
                 INSERT INTO action(tick_id,kind,lane,emitted_at,resolved_at,target_role,target_addr)
                   VALUES(1,'need_ready_tickets',NULL,1,2,'planner','planner:one');
                 INSERT INTO action(tick_id,kind,lane,emitted_at,target_role,target_addr)
                   VALUES(1,'need_ready_tickets',NULL,2,'planner','planner:one');",
            )
            .unwrap();
        let settings = TickSettings {
            orchestrator: Some("test-server:test-workspace:test-orchestrator".to_owned()),
            ..TickSettings::default()
        };
        let receipt = ReceiptEvent {
            event_id: "event-1".to_owned(),
            original_id: "message-1".to_owned(),
            attempt_id: None,
            outcome: ReceiptOutcome::Queued,
            sender: "test-server:test-workspace:test-orchestrator".to_owned(),
            target: "session/706c616e6e6572/706c616e6e6572".to_owned(),
            event_time_ms: Some(3),
            observed_at_ms: 3,
            evidence: None,
            reason: None,
            session: None,
            expect_ticket: None,
            delivered_at_ms: None,
        };

        assert_eq!(
            write_action_sends(
                &connection,
                &settings,
                &RegistryView {
                    now_ms: 4,
                    lanes: Vec::new(),
                    registrations: vec![(
                        AgentName::try_from("session/706c616e6e6572/706c616e6e6572".to_owned(),)
                            .unwrap(),
                        "planner:one".to_owned(),
                    )],
                },
                std::slice::from_ref(&receipt),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            write_action_sends(
                &connection,
                &settings,
                &RegistryView {
                    now_ms: 4,
                    lanes: Vec::new(),
                    registrations: vec![(
                        AgentName::try_from("session/706c616e6e6572/706c616e6e6572".to_owned(),)
                            .unwrap(),
                        "planner:one".to_owned(),
                    )],
                },
                std::slice::from_ref(&receipt),
            )
            .unwrap(),
            0
        );
        let sends = connection
            .prepare("SELECT action_id, sent_at FROM action_send ORDER BY action_id")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(sends, vec![(2, 3)]);
    }
}
