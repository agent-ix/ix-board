// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic relay and action-delivery query adapters (FR-047).

use super::{EvalWindow, QueryValue, RoleSpec};
use ix_trace_rs::implements;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

pub type LedgerViewRows = Result<Vec<Map<String, Value>>, String>;
pub type LedgerViews = BTreeMap<String, LedgerViewRows>;

/// Complete relay and ledger rows normalized by the live reader.
///
/// A successful source result means the requested evaluation window has complete coverage.
/// Truncated, skipped, or otherwise partial reads must be passed as `Err`, never as a partial
/// successful slice.
pub struct RelayEvidence<'a> {
    /// FR-071 order rows. The adapter accepts source field names (`sender`, `recipient`,
    /// `enqueued_at`) and normalized aliases (`sender_addr`, `recipient_addr`, `enqueued_at_ms`).
    pub orders: Result<&'a [Map<String, Value>], &'a str>,
    /// ReceiptEvent rows. The adapter accepts source field names (`original_id`, `sender`,
    /// `target`, `event_time_ms`) and normalized aliases. Non-Queued lifecycle events are ignored.
    pub receipts: Result<&'a [Map<String, Value>], &'a str>,
    /// The fleet ledger's action_send rows, normalized to action_id, receipt_id, sender_role,
    /// sender_addr, recipient_addr, and sent_at.
    pub action_sends: Result<&'a [Map<String, Value>], &'a str>,
    /// Ledger view rows keyed by the view name. Views required by an evaluated query must be
    /// present and complete; a missing or partial view is unavailable.
    pub views: &'a LedgerViews,
    /// Assignment order ids recorded by fleet assign. Needed to distinguish ORCH orders sent
    /// through fleet assign from direct relay orders.
    pub fleet_assign_order_ids: Result<&'a BTreeSet<String>, &'a str>,
    /// Verified role addresses from the fleet configuration.
    pub minder_addr: Option<&'a str>,
    /// Verified orchestrator address from the fleet configuration.
    pub orchestrator_addr: Option<&'a str>,
    /// Tick interval in milliseconds, used to bound same-tick sends.
    pub tick_interval_ms: Option<i64>,
}

const ORDER_WITHOUT_TICKET: &str = "relay.minder_orders_without_expect_ticket";
const ORCH_ORDER_WITHOUT_TICKET: &str = "relay.orchestrator_orders_without_expect_ticket";
const ORCH_ORDER_NOT_ASSIGNED: &str = "relay.orchestrator_orders_not_via_fleet_assign";
const PUSH_UNSENT: &str = "relay.push_next_ticket_unsent_in_tick";
const ESCALATE_BLOCK_UNSENT: &str = "relay.escalate_block_without_lead_message";
const DEAD_UNROUTED: &str = "relay.dead_lane_without_orchestrator_message";
const IDLE_UNROUTED: &str = "relay.idle_empty_list_without_orchestrator_message";
const BLOCKED_SEND: &str = "relay.minder_sends_to_blocked_lane";
const ESCALATE_DRAIN_UNSENT: &str = "relay.escalate_drain_without_lead_message";
const ORCH_ACTION_UNSENT: &str = "relay.orchestrator_executed_actions_without_orchestrator_message";
const NEED_READY_UNSENT: &str = "relay.need_ready_tickets_without_planner_order";

/// Produces values for every relay query used by the selected role specifications.
///
/// The live reader owns bounded source access and completeness checks. This function consumes
/// only its normalized, read-only rows and returns unavailable values whenever a needed source
/// or configuration value is missing.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
pub fn adapters(
    specs: &[RoleSpec],
    evidence: &RelayEvidence<'_>,
    window: &EvalWindow,
) -> BTreeMap<String, QueryValue> {
    let mut values = BTreeMap::new();
    for query in requested_queries(specs) {
        let value = match query.as_str() {
            ORDER_WITHOUT_TICKET => {
                orders_without_ticket(&query, evidence.orders, evidence.minder_addr, window)
            }
            ORCH_ORDER_WITHOUT_TICKET => {
                orders_without_ticket(&query, evidence.orders, evidence.orchestrator_addr, window)
            }
            ORCH_ORDER_NOT_ASSIGNED => orchestrator_orders_not_assigned(evidence, window),
            PUSH_UNSENT => action_send_violations(
                &query,
                evidence,
                "push_next_ticket_actions",
                "minder",
                window,
            ),
            ESCALATE_BLOCK_UNSENT => {
                action_send_violations(&query, evidence, "escalate_block_actions", "minder", window)
            }
            DEAD_UNROUTED => lane_route_violations(&query, evidence, "dead_lane_ticks", window),
            IDLE_UNROUTED => {
                lane_route_violations(&query, evidence, "idle_empty_list_ticks", window)
            }
            BLOCKED_SEND => blocked_lane_sends(&query, evidence, window),
            ESCALATE_DRAIN_UNSENT => {
                action_send_violations(&query, evidence, "escalate_drain_actions", "minder", window)
            }
            ORCH_ACTION_UNSENT => action_send_violations(
                &query,
                evidence,
                "orchestrator_addressed_actions",
                "minder",
                window,
            ),
            NEED_READY_UNSENT => need_ready_violations(evidence, window),
            _ => continue,
        };
        values.insert(query, value);
    }
    values
}

/// Returns the deterministic relay query ids requested by these role specifications.
#[implements("FR-047-AC-1")]
pub fn requested_queries(specs: &[RoleSpec]) -> BTreeSet<String> {
    specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic")
        .map(|rule| rule.query_or_rubric.clone())
        .filter(|query| {
            matches!(
                query.as_str(),
                ORDER_WITHOUT_TICKET
                    | ORCH_ORDER_WITHOUT_TICKET
                    | ORCH_ORDER_NOT_ASSIGNED
                    | PUSH_UNSENT
                    | ESCALATE_BLOCK_UNSENT
                    | DEAD_UNROUTED
                    | IDLE_UNROUTED
                    | BLOCKED_SEND
                    | ESCALATE_DRAIN_UNSENT
                    | ORCH_ACTION_UNSENT
                    | NEED_READY_UNSENT
            )
        })
        .collect()
}

/// Collects complete, bounded relay and Fleet ledger evidence and evaluates the requested relay
/// rules. The relay URL and prefix must come from the caller's verified relay configuration;
/// source failures are represented as `Unavailable` query values and never as empty evidence.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
pub fn live_adapters(
    specs: &[RoleSpec],
    window: &EvalWindow,
    ledger_path: Option<&Path>,
    relay_url: Option<&str>,
    relay_prefix: Option<&str>,
    tick_settings: Option<&crate::fleet::tick::TickSettings>,
    tick_interval: Option<Duration>,
    timeout: Duration,
) -> BTreeMap<String, QueryValue> {
    let requested = requested_queries(specs);
    if requested.is_empty() {
        return BTreeMap::new();
    }
    if window.start_ms < 0 || window.end_ms <= window.start_ms {
        return requested
            .into_iter()
            .map(|query| {
                (
                    query.clone(),
                    unavailable(&query, "relay evaluation window is invalid"),
                )
            })
            .collect();
    }

    let tick_ms = tick_interval.and_then(|interval| i64::try_from(interval.as_millis()).ok());
    let minder_addr = tick_settings.and_then(|settings| settings.minder.as_deref());
    let orchestrator_addr = tick_settings.and_then(|settings| settings.orchestrator.as_deref());
    let wants_orders = requested.iter().any(|query| {
        matches!(
            query.as_str(),
            ORDER_WITHOUT_TICKET
                | ORCH_ORDER_WITHOUT_TICKET
                | ORCH_ORDER_NOT_ASSIGNED
                | NEED_READY_UNSENT
        )
    });
    let wants_assigned = requested.contains(ORCH_ORDER_NOT_ASSIGNED);
    let wants_receipts = requested.iter().any(|query| {
        matches!(
            query.as_str(),
            PUSH_UNSENT
                | ESCALATE_BLOCK_UNSENT
                | DEAD_UNROUTED
                | IDLE_UNROUTED
                | BLOCKED_SEND
                | ESCALATE_DRAIN_UNSENT
                | ORCH_ACTION_UNSENT
        )
    });
    let wants_action_sends = requested.iter().any(|query| {
        matches!(
            query.as_str(),
            PUSH_UNSENT
                | ESCALATE_BLOCK_UNSENT
                | DEAD_UNROUTED
                | IDLE_UNROUTED
                | ESCALATE_DRAIN_UNSENT
                | ORCH_ACTION_UNSENT
        )
    });
    let wants_actions = requested.iter().any(|query| {
        matches!(
            query.as_str(),
            PUSH_UNSENT
                | ESCALATE_BLOCK_UNSENT
                | DEAD_UNROUTED
                | IDLE_UNROUTED
                | ESCALATE_DRAIN_UNSENT
                | ORCH_ACTION_UNSENT
                | NEED_READY_UNSENT
        )
    });
    let wants_blocked_intervals = requested.contains(BLOCKED_SEND);

    let bounded_end = window.end_ms.saturating_sub(1);
    let orders = if wants_orders {
        match (relay_url, relay_prefix) {
            (Some(url), Some(prefix)) => {
                read_orders(url, prefix, timeout, window.start_ms, bounded_end)
            }
            _ => Err("verified relay connection settings are unavailable".to_owned()),
        }
    } else {
        Ok(Vec::new())
    };
    let receipts = if wants_receipts {
        match (relay_url, relay_prefix) {
            (Some(url), Some(prefix)) => {
                let end = bounded_end;
                read_receipts(url, prefix, timeout, window.start_ms, end)
            }
            _ => Err("verified relay connection settings are unavailable".to_owned()),
        }
    } else {
        Ok(Vec::new())
    };

    let action_window = EvalWindow {
        start_ms: window.start_ms,
        end_ms: window.end_ms,
    };
    let (action_sends, views, assigned) = match ledger_path {
        Some(path) if wants_actions || wants_assigned || wants_blocked_intervals => {
            let coverage = if wants_actions || wants_blocked_intervals {
                action_coverage(path, window)
            } else {
                Ok(())
            };
            let actions = if wants_actions {
                read_ledger_rows(path, "action", "emitted_at", &action_window)
            } else {
                Ok(Vec::new())
            };
            let executed_actions = if requested.contains(ORCH_ACTION_UNSENT) {
                read_ledger_rows(path, "action", "executed_at", window)
            } else {
                Ok(Vec::new())
            };
            let sends = if wants_action_sends {
                read_ledger_rows(path, "action_send", "sent_at", &action_window)
                    .and_then(|rows| require_sender_role(rows, path))
            } else {
                Ok(Vec::new())
            };
            let assigned = if wants_assigned {
                read_ledger_rows(path, "assignment_history", "sent_at", window)
                    .and_then(normalize_assigned_order_ids)
            } else {
                Ok(BTreeSet::new())
            };
            let mut views = LedgerViews::new();
            if wants_actions {
                let action_rows = actions.as_ref().map(Vec::as_slice);
                for (name, kind) in [
                    ("push_next_ticket_actions", "push_next_ticket"),
                    ("escalate_block_actions", "escalate_block"),
                    ("escalate_drain_actions", "escalate_drain"),
                    ("need_ready_tickets_actions", "need_ready_tickets"),
                    ("dead_lane_ticks", "route_dead_lane"),
                    ("idle_empty_list_ticks", "route_idle_lane"),
                ] {
                    let view = action_rows
                        .map(|rows| normalize_actions(rows, kind))
                        .map_err(Clone::clone);
                    views.insert(name.to_owned(), view);
                }
            }
            if requested.contains(ORCH_ACTION_UNSENT) {
                let view = executed_actions
                    .as_ref()
                    .map(|rows| normalize_actions(rows, "orchestrator_addressed"))
                    .map_err(Clone::clone);
                views.insert("orchestrator_addressed_actions".to_owned(), view);
            }
            if wants_blocked_intervals {
                let blocked = tick_ms
                    .filter(|interval| *interval > 0)
                    .ok_or_else(|| "fleet tick interval is unavailable".to_owned())
                    .and_then(|interval| read_blocked_intervals(path, window, interval));
                views.insert("blocked_lane_ticks".to_owned(), blocked);
            }
            if let Err(reason) = coverage {
                for rows in views.values_mut() {
                    *rows = Err(reason.clone());
                }
                (Err(reason.clone()), views, Err(reason))
            } else {
                (sends, views, assigned)
            }
        }
        Some(_) => (
            if wants_receipts {
                Err("fleet action ledger source is unavailable".to_owned())
            } else {
                Ok(Vec::new())
            },
            LedgerViews::new(),
            if wants_assigned {
                Err("fleet assignment history is unavailable".to_owned())
            } else {
                Ok(BTreeSet::new())
            },
        ),
        None => (
            Err("fleet ledger path is unavailable".to_owned()),
            LedgerViews::new(),
            Err("fleet ledger path is unavailable".to_owned()),
        ),
    };

    let assigned = assigned.as_ref().map_err(String::as_str);
    let evidence = RelayEvidence {
        orders: orders.as_deref().map_err(String::as_str),
        receipts: receipts.as_deref().map_err(String::as_str),
        action_sends: action_sends.as_deref().map_err(String::as_str),
        views: &views,
        fleet_assign_order_ids: assigned,
        minder_addr,
        orchestrator_addr,
        tick_interval_ms: tick_ms,
    };
    adapters(specs, &evidence, window)
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
fn read_orders(
    url: &str,
    prefix: &str,
    timeout: Duration,
    since_ms: i64,
    until_ms: i64,
) -> Result<Vec<Map<String, Value>>, String> {
    if since_ms < 0 || until_ms < since_ms {
        return Err("relay order window is invalid".to_owned());
    }
    let mut relay = relay_core::Relay::connect(url, timeout)
        .map_err(|_| "relay order source is unavailable".to_owned())?
        .with_prefix(prefix.to_owned());
    let mut cursor = None;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for _ in 0..MAX_RELAY_PAGES {
        let request = relay_core::receipts::OrderListRequest::new(
            since_ms,
            until_ms,
            relay_core::receipts::ORDERS_PAGE_CAP,
            cursor.clone(),
        )
        .map_err(|_| "relay order request is invalid".to_owned())?;
        let page = relay
            .orders(&request)
            .map_err(|_| "relay order source is unavailable".to_owned())?;
        validate_order_page(&page, since_ms, until_ms, cursor.as_ref())?;
        for order in page.orders {
            if !seen.insert(order.order_id.clone()) {
                return Err("relay order pagination repeated an order".to_owned());
            }
            let value = serde_json::to_value(order)
                .map_err(|_| "relay order row could not be normalized".to_owned())?;
            let mut row = value
                .as_object()
                .cloned()
                .ok_or_else(|| "relay order row has an invalid shape".to_owned())?;
            if let Some(at) = row.get("enqueued_at_ms").cloned() {
                row.insert("at_ms".to_owned(), at);
            }
            rows.push(row);
        }
        if !page.truncated {
            if page.next_cursor.is_some() {
                return Err("complete relay order page has an unexpected cursor".to_owned());
            }
            return Ok(rows);
        }
        let Some(next) = page.next_cursor else {
            return Err("truncated relay order page has no cursor".to_owned());
        };
        cursor = Some(next);
    }
    Err("relay order page limit was exceeded".to_owned())
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
fn read_receipts(
    url: &str,
    prefix: &str,
    timeout: Duration,
    since_ms: i64,
    until_ms: i64,
) -> Result<Vec<Map<String, Value>>, String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_i64, |elapsed| {
            elapsed.as_millis().min(i64::MAX as u128) as i64
        });
    if since_ms < 0 || until_ms < since_ms || until_ms > now_ms {
        return Err("relay receipt window is invalid".to_owned());
    }
    let mut relay = relay_core::Relay::connect(url, timeout)
        .map_err(|_| "relay receipt source is unavailable".to_owned())?
        .with_prefix(prefix.to_owned());
    let mut cursor = None;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for _ in 0..MAX_RELAY_PAGES {
        let request = relay_core::receipts::ReceiptRequest {
            since_ms,
            until_ms,
            now_ms,
            page_size: relay_core::receipts::RECEIPT_PAGE_CAP,
            after: cursor.clone(),
        };
        let page = relay
            .receipt_export(&request)
            .map_err(|_| "relay receipt source is unavailable".to_owned())?;
        validate_receipt_page(&page, since_ms, until_ms, cursor.as_deref())?;
        for ordered in page.events {
            if !seen.insert(ordered.cursor) {
                return Err("relay receipt pagination repeated an event".to_owned());
            }
            let value = serde_json::to_value(ordered.event)
                .map_err(|_| "relay receipt row could not be normalized".to_owned())?;
            let mut row = value
                .as_object()
                .cloned()
                .ok_or_else(|| "relay receipt row has an invalid shape".to_owned())?;
            row.insert(
                "receipt_id".to_owned(),
                row.get("original_id").cloned().unwrap_or(Value::Null),
            );
            if let Some(at) = row.get("event_time_ms").cloned() {
                row.insert("at_ms".to_owned(), at);
            }
            rows.push(row);
        }
        if !page.has_more {
            if page.next_cursor.is_some() {
                return Err("complete relay receipt page has an unexpected cursor".to_owned());
            }
            return Ok(rows);
        }
        let Some(next) = page.next_cursor else {
            return Err("relay receipt page has more events but no cursor".to_owned());
        };
        cursor = Some(next);
    }
    Err("relay receipt page limit was exceeded".to_owned())
}

const MAX_RELAY_PAGES: usize = 100;

fn validate_order_page(
    page: &relay_core::receipts::OrderPage,
    since_ms: i64,
    until_ms: i64,
    previous_cursor: Option<&relay_core::receipts::OrderCursor>,
) -> Result<(), String> {
    if !page.complete
        || page.skipped != 0
        || page
            .oldest_available_at
            .is_none_or(|oldest| oldest > since_ms)
        || page.window.since != since_ms
        || page.window.until != until_ms
        || page.window.filter != "enqueued_at"
        || page.window.order != "newest_first"
        || page.limit != relay_core::receipts::ORDERS_PAGE_CAP
    {
        return Err("relay order window is incomplete".to_owned());
    }
    if page.truncated {
        let Some(next) = page.next_cursor.as_ref() else {
            return Err("truncated relay order page has no cursor".to_owned());
        };
        if previous_cursor == Some(next) {
            return Err("relay order cursor did not advance".to_owned());
        }
        if page.orders.last().is_none_or(|order| {
            order.order_id != next.order_id || order.enqueued_at_ms != next.enqueued_at_ms
        }) {
            return Err("relay order cursor does not match the last returned order".to_owned());
        }
    } else if page.next_cursor.is_some() {
        return Err("complete relay order page has an unexpected cursor".to_owned());
    }
    Ok(())
}

fn validate_receipt_page(
    page: &relay_core::receipts::ReceiptExport,
    since_ms: i64,
    until_ms: i64,
    previous_cursor: Option<&str>,
) -> Result<(), String> {
    if !page.complete
        || !page.reasons.is_empty()
        || page.requested_since_ms != since_ms
        || page.requested_until_ms != until_ms
        || page.covered_since_ms.is_none_or(|start| start > since_ms)
        || page.covered_until_ms.is_none_or(|end| end < until_ms)
    {
        return Err("relay receipt window is incomplete".to_owned());
    }
    if page.has_more {
        let Some(next) = page.next_cursor.as_deref() else {
            return Err("relay receipt page has more events but no cursor".to_owned());
        };
        if previous_cursor == Some(next) {
            return Err("relay receipt cursor did not advance".to_owned());
        }
        if page.events.last().is_none_or(|event| event.cursor != next) {
            return Err("relay receipt cursor does not match the last returned event".to_owned());
        }
    } else if page.next_cursor.is_some() {
        return Err("complete relay receipt page has an unexpected cursor".to_owned());
    }
    Ok(())
}

fn read_ledger_rows(
    path: &Path,
    object: &str,
    time_column: &str,
    window: &EvalWindow,
) -> Result<Vec<Map<String, Value>>, String> {
    super::ledger::read_view_rows(path, object, time_column, window)
}

#[implements("FR-047-AC-6")]
fn action_coverage(path: &Path, window: &EvalWindow) -> Result<(), String> {
    let ticks = read_ledger_rows(path, "tick", "committed_at", window)?;
    if ticks.is_empty() {
        return Err(
            "fleet action ledger has no committed tick in this event-time window".to_owned(),
        );
    }
    let runs = read_ledger_rows(path, "run", "relay_ms", window)?;
    if runs.is_empty() {
        return Err(
            "fleet action ledger has no collector run in this event-time window".to_owned(),
        );
    }
    if runs
        .iter()
        .any(|run| run.get("relay_unavailable").and_then(Value::as_i64) == Some(1))
    {
        return Err("fleet action ledger has an unavailable relay collector run".to_owned());
    }
    Ok(())
}

#[implements("FR-047-AC-6")]
fn require_sender_role(
    rows: Vec<Map<String, Value>>,
    path: &Path,
) -> Result<Vec<Map<String, Value>>, String> {
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "fleet action ledger is unavailable".to_owned())?;
    let version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|_| "fleet action ledger schema version is unavailable".to_owned())?;
    let has_sender_role = connection
        .prepare("PRAGMA table_info(action_send)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|_| "fleet action ledger columns are unavailable".to_owned())?
        .iter()
        .any(|column| column == "sender_role");
    if version < 7 || !has_sender_role {
        return Err("fleet action ledger lacks sender_role support (v7 required)".to_owned());
    }
    if rows.iter().any(|row| row.get("sender_role").is_none()) {
        return Err(format!(
            "{}: action_send rows do not record sender_role",
            path.display(),
        ));
    }
    Ok(rows)
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
fn normalize_assigned_order_ids(rows: Vec<Map<String, Value>>) -> Result<BTreeSet<String>, String> {
    let mut ids = BTreeSet::new();
    for row in rows {
        let Some(order_id) = row.get("order_id").and_then(Value::as_str) else {
            return Err("assignment_history row has no stable order id".to_owned());
        };
        ids.insert(order_id.to_owned());
    }
    Ok(ids)
}

#[implements("FR-047-AC-1")]
fn normalize_actions(rows: &[Map<String, Value>], kind: &str) -> Vec<Map<String, Value>> {
    rows.iter()
        .filter(|row| {
            if kind == "orchestrator_addressed" {
                row.get("target_role").and_then(Value::as_str) == Some("orchestrator")
            } else {
                row.get("kind").and_then(Value::as_str) == Some(kind)
            }
        })
        .map(|row| {
            let mut normalized = row.clone();
            if let Some(id) = row.get("id").cloned() {
                normalized.insert("action_id".to_owned(), id);
            }
            if let Some(at) = row.get("emitted_at").cloned() {
                normalized.insert("at_ms".to_owned(), at);
            }
            if kind == "route_dead_lane" || kind == "route_idle_lane" {
                normalized.insert(
                    "lane_addr".to_owned(),
                    row.get("lane").cloned().unwrap_or(Value::Null),
                );
            }
            normalized
        })
        .collect()
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
fn read_blocked_intervals(
    path: &Path,
    window: &EvalWindow,
    tick_interval_ms: i64,
) -> Result<Vec<Map<String, Value>>, String> {
    let start = window
        .start_ms
        .saturating_sub(tick_interval_ms.saturating_mul(2));
    let extended = EvalWindow {
        start_ms: start,
        end_ms: window.end_ms.saturating_add(tick_interval_ms),
    };
    let rows = read_ledger_rows(path, "lane_tick_rows", "at_ms", &extended)?;
    if rows.is_empty() {
        return Err("lane_tick_rows has no coverage for the blocked-lane window".to_owned());
    }
    let max_gap_ms = tick_interval_ms.saturating_mul(2);
    let mut by_lane = BTreeMap::<String, Vec<(i64, String)>>::new();
    for row in rows {
        let (Some(lane), Some(at), Some(class)) = (
            row.get("lane").and_then(Value::as_str),
            row.get("at_ms").and_then(Value::as_i64),
            row.get("class").and_then(Value::as_str),
        ) else {
            return Err("lane_tick_rows has a row without lane, class or time".to_owned());
        };
        by_lane
            .entry(lane.to_owned())
            .or_default()
            .push((at, class.to_ascii_lowercase()));
    }
    let mut intervals = Vec::new();
    for (lane, mut samples) in by_lane {
        samples.sort_by_key(|(at, _)| *at);
        if samples.first().is_none_or(|(at, _)| {
            *at > window.start_ms || window.start_ms.saturating_sub(*at) > max_gap_ms
        }) {
            return Err(format!(
                "lane {lane} lacks a status sample covering window start"
            ));
        }
        if samples
            .windows(2)
            .any(|pair| pair[1].0.saturating_sub(pair[0].0) > max_gap_ms)
        {
            return Err(format!("lane {lane} has a status gap over two fleet ticks"));
        }
        if samples
            .last()
            .is_none_or(|(at, _)| window.end_ms.saturating_sub(*at) > max_gap_ms)
        {
            return Err(format!(
                "lane {lane} lacks a status sample covering window end"
            ));
        }
        let mut index = 0;
        while index < samples.len() {
            if samples[index].1 != "blocked" {
                index += 1;
                continue;
            }
            let start_ms = samples[index].0;
            while index < samples.len() && samples[index].1 == "blocked" {
                index += 1;
            }
            let end_ms = samples
                .get(index)
                .map(|(at, _)| *at)
                .unwrap_or(window.end_ms);
            if end_ms <= window.start_ms || start_ms >= window.end_ms {
                continue;
            }
            intervals.push(Map::from_iter([
                ("lane_addr".to_owned(), Value::String(lane.clone())),
                ("start_ms".to_owned(), Value::from(start_ms)),
                ("end_ms".to_owned(), Value::from(end_ms)),
            ]));
        }
    }
    Ok(intervals)
}

fn orders_without_ticket(
    query: &str,
    rows: Result<&[Map<String, Value>], &str>,
    sender: Option<&str>,
    window: &EvalWindow,
) -> QueryValue {
    let Some(sender) = sender else {
        return unavailable(query, "verified sender address is unavailable");
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(reason) => return unavailable(query, reason),
    };
    let mut evidence_refs = Vec::new();
    for row in rows {
        let (Some(at_ms), Some(row_sender)) = (
            timestamp(row, &["enqueued_at_ms", "enqueued_at"]),
            string(row, &["sender_addr", "sender"]),
        ) else {
            return unavailable(
                query,
                "order population has a row without sender or enqueue time",
            );
        };
        if row_sender.eq_ignore_ascii_case(sender) && in_window(at_ms, window) {
            let Some(order_id) = string(row, &["order_id", "original_id"]) else {
                return unavailable(query, "matching order has no stable order id");
            };
            if string(row, &["expect_ticket"]).is_none_or(str::is_empty) {
                evidence_refs.push(format!("relay-order:{order_id}"));
            }
        }
    }
    value(evidence_refs)
}

fn orchestrator_orders_not_assigned(
    evidence: &RelayEvidence<'_>,
    window: &EvalWindow,
) -> QueryValue {
    let query = ORCH_ORDER_NOT_ASSIGNED;
    let Some(sender) = evidence.orchestrator_addr else {
        return unavailable(query, "verified orchestrator address is unavailable");
    };
    let rows = match evidence.orders {
        Ok(rows) => rows,
        Err(reason) => return unavailable(query, reason),
    };
    let assigned = match evidence.fleet_assign_order_ids {
        Ok(ids) => ids,
        Err(reason) => return unavailable(query, reason),
    };
    let mut evidence_refs = Vec::new();
    for row in rows {
        let (Some(at_ms), Some(row_sender), Some(order_id)) = (
            timestamp(row, &["enqueued_at_ms", "enqueued_at"]),
            string(row, &["sender_addr", "sender"]),
            string(row, &["order_id", "original_id"]),
        ) else {
            return unavailable(
                query,
                "order population has a row missing its id, sender, or enqueue time",
            );
        };
        if row_sender.eq_ignore_ascii_case(sender)
            && in_window(at_ms, window)
            && !assigned.contains(order_id)
        {
            evidence_refs.push(format!("relay-order:{order_id}"));
        }
    }
    value(evidence_refs)
}

fn action_send_violations(
    query: &str,
    evidence: &RelayEvidence<'_>,
    view_name: &str,
    sender_role: &str,
    window: &EvalWindow,
) -> QueryValue {
    let rows = match view_rows(evidence, query, view_name) {
        Ok(rows) => rows,
        Err(value) => return value,
    };
    let sends = match evidence.action_sends {
        Ok(sends) => sends,
        Err(reason) => return unavailable(query, reason),
    };
    let receipts = match evidence.receipts {
        Ok(receipts) => receipts,
        Err(reason) => return unavailable(query, reason),
    };
    let Some(minder) = evidence.minder_addr else {
        return unavailable(query, "verified minder address is unavailable");
    };
    let mut missing = Vec::new();
    for row in rows {
        let Some(at_ms) = timestamp(row, &["at_ms", "emitted_at"]) else {
            return unavailable(
                query,
                format!("{view_name} contains a row without its event time"),
            );
        };
        if view_name == "orchestrator_addressed_actions" {
            match row.get("executed_at_ms").or_else(|| row.get("executed_at")) {
                Some(Value::Null) => continue,
                Some(Value::Number(_)) => {
                    let Some(executed_at) = timestamp(row, &["executed_at_ms", "executed_at"])
                    else {
                        return unavailable(query, "action has malformed execution time");
                    };
                    if !in_window(executed_at, window) {
                        continue;
                    }
                }
                Some(_) => return unavailable(query, "action has malformed execution time"),
                None => return unavailable(query, "orchestrator action view omits executed_at"),
            }
        } else if !in_window(at_ms, window) {
            continue;
        }
        let Some(action_id) = integer(row, &["action_id", "id"]) else {
            return unavailable(
                query,
                format!("{view_name} contains a row without an action id"),
            );
        };
        let due_by = match timestamp(row, &["due_by"]) {
            Some(due_by) => due_by,
            None => match evidence.tick_interval_ms {
                Some(interval) if interval > 0 => at_ms.saturating_add(interval),
                _ => return unavailable(query, "fleet tick interval is unavailable"),
            },
        };
        if due_by > window.end_ms {
            continue;
        }
        let expected_recipient = if view_name == "orchestrator_addressed_actions" {
            evidence.orchestrator_addr
        } else {
            string(row, &["target_addr", "lane_addr"])
        };
        let Some(expected_recipient) = expected_recipient else {
            return unavailable(
                query,
                format!("{view_name} contains a row without its recipient address"),
            );
        };
        let sent = sends.iter().any(|send| {
            integer(send, &["action_id"]) == Some(action_id)
                && string(send, &["sender_role"]).is_some_and(|role| role == sender_role)
                && string(send, &["sender_addr"])
                    .is_some_and(|addr| addr.eq_ignore_ascii_case(minder))
                && string(send, &["recipient_addr"])
                    .is_some_and(|addr| addr.eq_ignore_ascii_case(expected_recipient))
                && timestamp(send, &["sent_at"])
                    .is_some_and(|sent_at| sent_at >= at_ms && sent_at <= due_by)
                && send_has_queued_receipt(send, receipts)
        });
        if !sent {
            missing.push(format!("fleet-action:{action_id}"));
        }
    }
    value(missing)
}

fn lane_route_violations(
    query: &str,
    evidence: &RelayEvidence<'_>,
    view_name: &str,
    window: &EvalWindow,
) -> QueryValue {
    let rows = match view_rows(evidence, query, view_name) {
        Ok(rows) => rows,
        Err(value) => return value,
    };
    let Some(minder) = evidence.minder_addr else {
        return unavailable(query, "verified minder address is unavailable");
    };
    let Some(orchestrator) = evidence.orchestrator_addr else {
        return unavailable(query, "verified orchestrator address is unavailable");
    };
    let sends = match evidence.action_sends {
        Ok(sends) => sends,
        Err(reason) => return unavailable(query, reason),
    };
    let receipts = match queued_receipts(evidence, query) {
        Ok(rows) => rows,
        Err(value) => return value,
    };
    let mut missing = Vec::new();
    for row in rows {
        let (Some(at_ms), Some(lane), Some(action_id)) = (
            timestamp(row, &["at_ms", "started_at", "observed_at"]),
            string(row, &["lane_addr", "lane"]),
            integer(row, &["action_id", "id"]),
        ) else {
            return unavailable(
                query,
                format!("{view_name} contains a row without action id, lane, or event time"),
            );
        };
        if !in_window(at_ms, window) {
            continue;
        }
        let due_by = match timestamp(row, &["due_by"]) {
            Some(due_by) => due_by,
            None => match evidence.tick_interval_ms {
                Some(interval) if interval > 0 => at_ms.saturating_add(interval),
                _ => return unavailable(query, "fleet tick interval is unavailable"),
            },
        };
        if due_by > window.end_ms {
            continue;
        }
        let routed = sends.iter().any(|send| {
            integer(send, &["action_id"]) == Some(action_id)
                && string(send, &["sender_role"]).is_some_and(|role| role == "minder")
                && string(send, &["sender_addr"])
                    .is_some_and(|sender| sender.eq_ignore_ascii_case(minder))
                && string(send, &["recipient_addr"])
                    .is_some_and(|recipient| recipient.eq_ignore_ascii_case(orchestrator))
                && timestamp(send, &["sent_at"])
                    .is_some_and(|sent_at| sent_at >= at_ms && sent_at <= due_by)
                && send_has_queued_receipt(send, receipts)
        });
        if !routed {
            missing.push(format!("fleet-action:{action_id}:{lane}"));
        }
    }
    value(missing)
}

fn blocked_lane_sends(
    query: &str,
    evidence: &RelayEvidence<'_>,
    window: &EvalWindow,
) -> QueryValue {
    let intervals = match view_rows(evidence, query, "blocked_lane_ticks") {
        Ok(rows) => rows,
        Err(value) => return value,
    };
    let Some(minder) = evidence.minder_addr else {
        return unavailable(query, "verified minder address is unavailable");
    };
    let receipts = match queued_receipts(evidence, query) {
        Ok(rows) => rows,
        Err(value) => return value,
    };
    let mut violations = BTreeSet::new();
    for interval in intervals {
        let (Some(lane), Some(start_ms)) = (
            string(interval, &["lane_addr", "lane"]),
            timestamp(interval, &["start_ms", "started_at", "at_ms"]),
        ) else {
            return unavailable(
                query,
                "blocked_lane_ticks contains a row without lane or start time",
            );
        };
        let end_ms = timestamp(interval, &["end_ms", "ended_at"]).unwrap_or(window.end_ms);
        if end_ms <= window.start_ms || start_ms >= window.end_ms {
            continue;
        }
        for receipt in receipts {
            let (Some(sender), Some(target), Some(at_ms), Some(receipt_id)) = (
                string(receipt, &["sender_addr", "sender"]),
                string(receipt, &["recipient_addr", "target", "recipient"]),
                timestamp(receipt, &["at_ms", "event_time_ms", "enqueued_at_ms"]),
                string(receipt, &["receipt_id", "original_id", "event_id"]),
            ) else {
                return unavailable(
                    query,
                    "receipt population has a row missing sender, recipient, id, or time",
                );
            };
            if string(receipt, &["outcome"]).is_none() {
                return unavailable(
                    query,
                    "receipt population has a row without lifecycle outcome",
                );
            }
            if sender.eq_ignore_ascii_case(minder)
                && target.eq_ignore_ascii_case(lane)
                && at_ms >= start_ms.max(window.start_ms)
                && at_ms < end_ms.min(window.end_ms)
                && is_queued(receipt)
            {
                violations.insert(format!("relay-receipt:{receipt_id}"));
            }
        }
    }
    value(violations.into_iter().collect())
}

fn need_ready_violations(evidence: &RelayEvidence<'_>, window: &EvalWindow) -> QueryValue {
    let query = NEED_READY_UNSENT;
    let rows = match view_rows(evidence, query, "need_ready_tickets_actions") {
        Ok(rows) => rows,
        Err(value) => return value,
    };
    let Some(orchestrator) = evidence.orchestrator_addr else {
        return unavailable(query, "verified orchestrator address is unavailable");
    };
    let orders = match evidence.orders {
        Ok(orders) => orders,
        Err(reason) => return unavailable(query, reason),
    };
    let tick_ms = match evidence.tick_interval_ms {
        Some(value) if value > 0 => value,
        _ => return unavailable(query, "fleet tick interval is unavailable"),
    };
    let mut missing = Vec::new();
    let mut matched_order_ids = BTreeSet::new();
    for row in rows {
        let (Some(at_ms), Some(action_id)) = (
            timestamp(row, &["at_ms", "emitted_at"]),
            integer(row, &["action_id", "id"]),
        ) else {
            return unavailable(
                query,
                "need_ready_tickets_actions contains a row without action id or time",
            );
        };
        if !in_window(at_ms, window) {
            continue;
        }
        let Some(target) = string(row, &["planner_addr", "target_addr"]) else {
            return unavailable(
                query,
                "need_ready_tickets_actions contains a row without planner address",
            );
        };
        if at_ms.saturating_add(tick_ms) > window.end_ms {
            continue;
        }
        let matching_order = orders.iter().find(|order| {
            string(order, &["sender_addr", "sender"])
                .is_some_and(|sender| sender.eq_ignore_ascii_case(orchestrator))
                && string(order, &["recipient_addr", "recipient", "target"])
                    .is_some_and(|recipient| recipient.eq_ignore_ascii_case(target))
                && timestamp(order, &["enqueued_at_ms", "enqueued_at"]).is_some_and(|sent_at| {
                    sent_at >= at_ms && sent_at <= at_ms.saturating_add(tick_ms)
                })
                && string(order, &["order_id", "original_id"])
                    .is_some_and(|order_id| !matched_order_ids.contains(order_id))
        });
        let sent = matching_order.is_some();
        if let Some(order) = matching_order
            && let Some(order_id) = string(order, &["order_id", "original_id"])
        {
            matched_order_ids.insert(order_id.to_owned());
        }
        if !sent {
            missing.push(format!("fleet-action:{action_id}"));
        }
    }
    value(missing)
}

fn queued_receipts<'a>(
    evidence: &'a RelayEvidence<'_>,
    query: &str,
) -> Result<&'a [Map<String, Value>], QueryValue> {
    match evidence.receipts {
        Ok(rows) => Ok(rows),
        Err(reason) => Err(unavailable(query, reason)),
    }
}

fn send_has_queued_receipt(send: &Map<String, Value>, receipts: &[Map<String, Value>]) -> bool {
    let Some(receipt_id) = string(send, &["receipt_id"]) else {
        return false;
    };
    receipts.iter().any(|receipt| {
        string(receipt, &["receipt_id", "original_id", "event_id"]) == Some(receipt_id)
            && is_queued(receipt)
    })
}

fn is_queued(receipt: &Map<String, Value>) -> bool {
    string(receipt, &["outcome"]).is_some_and(|outcome| outcome.eq_ignore_ascii_case("queued"))
}

fn view_rows<'a>(
    evidence: &'a RelayEvidence<'_>,
    query: &str,
    name: &str,
) -> Result<&'a [Map<String, Value>], QueryValue> {
    match evidence.views.get(name) {
        Some(Ok(rows)) => Ok(rows),
        Some(Err(reason)) => Err(unavailable(query, format!("{name} unavailable: {reason}"))),
        None => Err(unavailable(
            query,
            format!("ledger view {name} is not registered"),
        )),
    }
}

fn string<'a>(row: &'a Map<String, Value>, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| row.get(*name).and_then(Value::as_str))
}

fn timestamp(row: &Map<String, Value>, names: &[&str]) -> Option<i64> {
    names
        .iter()
        .find_map(|name| row.get(*name).and_then(Value::as_i64))
}

fn integer(row: &Map<String, Value>, names: &[&str]) -> Option<i64> {
    names
        .iter()
        .find_map(|name| row.get(*name).and_then(Value::as_i64))
}

fn in_window(at_ms: i64, window: &EvalWindow) -> bool {
    at_ms >= window.start_ms && at_ms < window.end_ms
}

fn value(evidence_refs: Vec<String>) -> QueryValue {
    QueryValue::Value {
        value: evidence_refs.len() as f64,
        evidence_refs,
    }
}

fn unavailable(query: &str, reason: impl std::fmt::Display) -> QueryValue {
    QueryValue::Unavailable {
        reason: format!("{query}: {reason}"),
        evidence_refs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retro::eval::{EvaluationRule, RuleStatus, evaluate_rules};
    use ix_trace_rs::trace;
    use serde_json::json;

    fn rule(query: &str, id: &str) -> EvaluationRule {
        EvaluationRule {
            id: id.to_owned(),
            rule: id.to_owned(),
            evidence_source: "relay-receipts".to_owned(),
            trigger: query.to_owned(),
            method: "deterministic".to_owned(),
            query_or_rubric: query.to_owned(),
            comparator: "=".to_owned(),
            target: 0.0,
            unit: "events".to_owned(),
        }
    }

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object fixture")
    }

    fn order_page() -> relay_core::receipts::OrderPage {
        relay_core::receipts::OrderPage {
            orders: Vec::new(),
            truncated: false,
            next_cursor: None,
            window: relay_core::receipts::OrderWindow {
                since: 1_000,
                until: 1_999,
                filter: "enqueued_at",
                order: "newest_first",
            },
            limit: relay_core::receipts::ORDERS_PAGE_CAP,
            oldest_available_at: Some(0),
            complete: true,
            incomplete_reason: None,
            skipped: 0,
        }
    }

    fn receipt_export() -> relay_core::receipts::ReceiptExport {
        relay_core::receipts::ReceiptExport {
            requested_since_ms: 1_000,
            requested_until_ms: 1_999,
            covered_since_ms: Some(1_000),
            covered_until_ms: Some(1_999),
            instrumentation_start_ms: Some(0),
            retention_boundary_ms: 0,
            complete: true,
            reasons: Vec::new(),
            events: Vec::new(),
            next_cursor: None,
            has_more: false,
        }
    }

    fn queued_ordered_receipt() -> relay_core::receipts::OrderedReceipt {
        relay_core::receipts::OrderedReceipt {
            cursor: "1700000000000-0".to_owned(),
            event: relay_core::receipts::ReceiptEvent {
                event_id: "event-1".to_owned(),
                original_id: "receipt-1".to_owned(),
                attempt_id: None,
                outcome: relay_core::receipts::ReceiptOutcome::Queued,
                sender: "test:workspace:minder".to_owned(),
                target: "test:workspace:coder".to_owned(),
                event_time_ms: Some(1_500),
                observed_at_ms: 1_500,
                evidence: None,
                reason: None,
                session: None,
                expect_ticket: None,
                delivered_at_ms: None,
            },
        }
    }

    fn evidence<'a>(
        orders: Result<&'a [Map<String, Value>], &'a str>,
        receipts: Result<&'a [Map<String, Value>], &'a str>,
        action_sends: Result<&'a [Map<String, Value>], &'a str>,
        views: &'a LedgerViews,
        assigned: Result<&'a BTreeSet<String>, &'a str>,
    ) -> RelayEvidence<'a> {
        RelayEvidence {
            orders,
            receipts,
            action_sends,
            views,
            fleet_assign_order_ids: assigned,
            minder_addr: Some("test:workspace:minder"),
            orchestrator_addr: Some("test:workspace:orchestrator"),
            tick_interval_ms: Some(300_000),
        }
    }

    fn window() -> EvalWindow {
        EvalWindow {
            start_ms: 1_000,
            end_ms: 2_000,
        }
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn incomplete_order_source_is_unavailable_instead_of_zero() {
        let query = ORCH_ORDER_WITHOUT_TICKET;
        let specs = [RoleSpec {
            role: "orchestrator".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "ORCH-R01-E1")],
        }];
        let views = BTreeMap::new();
        let assigned = BTreeSet::new();
        let source = evidence(
            Err("order page truncated"),
            Ok(&[]),
            Ok(&[]),
            &views,
            Ok(&assigned),
        );
        let values = adapters(&specs, &source, &window());
        assert!(
            matches!(values.get(query), Some(QueryValue::Unavailable { reason, .. }) if reason.contains("order page truncated"))
        );
        let report = evaluate_rules(&specs[0].rules, &values);
        assert_eq!(report[0].status, RuleStatus::Unavailable);
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn order_rules_count_only_windowed_orders_from_the_configured_sender() {
        let query = ORCH_ORDER_WITHOUT_TICKET;
        let specs = [RoleSpec {
            role: "orchestrator".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "ORCH-R01-E1")],
        }];
        let orders = [
            map(
                json!({"order_id":"bad-1", "sender":"test:workspace:orchestrator", "recipient":"test:workspace:coder", "receipt_target":"test:workspace:coder", "expect_ticket":null, "ack_ms":30000, "deadline_at":31000, "enqueued_at":1100, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
            ),
            map(
                json!({"order_id":"good-1", "sender":"test:workspace:orchestrator", "recipient":"test:workspace:coder", "receipt_target":"test:workspace:coder", "expect_ticket":"SWM-221", "ack_ms":30000, "deadline_at":31200, "enqueued_at":1200, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
            ),
            map(
                json!({"order_id":"other-1", "sender":"test:workspace:minder", "recipient":"test:workspace:coder", "receipt_target":"test:workspace:coder", "expect_ticket":null, "ack_ms":30000, "deadline_at":31300, "enqueued_at":1300, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
            ),
            map(
                json!({"order_id":"old-1", "sender":"test:workspace:orchestrator", "recipient":"test:workspace:coder", "receipt_target":"test:workspace:coder", "expect_ticket":null, "ack_ms":30000, "deadline_at":30900, "enqueued_at":900, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
            ),
        ];
        let views = BTreeMap::new();
        let assigned = BTreeSet::new();
        let source = evidence(Ok(&orders), Ok(&[]), Ok(&[]), &views, Ok(&assigned));
        let values = adapters(&specs, &source, &window());
        assert!(
            matches!(values.get(query), Some(QueryValue::Value { value: 1.0, evidence_refs, .. }) if evidence_refs == &["relay-order:bad-1"])
        );
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn orchestrator_direct_order_is_distinguished_from_fleet_assign_order() {
        let query = ORCH_ORDER_NOT_ASSIGNED;
        let specs = [RoleSpec {
            role: "orchestrator".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "ORCH-R02-E1")],
        }];
        let orders = [
            map(
                json!({"order_id":"assign-1", "sender":"test:workspace:orchestrator", "recipient":"test:workspace:coder", "receipt_target":"test:workspace:coder", "expect_ticket":"SWM-221", "ack_ms":30000, "deadline_at":31100, "enqueued_at":1100, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
            ),
            map(
                json!({"order_id":"direct-1", "sender":"test:workspace:orchestrator", "recipient":"test:workspace:coder", "receipt_target":"test:workspace:coder", "expect_ticket":"SWM-222", "ack_ms":30000, "deadline_at":31200, "enqueued_at":1200, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
            ),
        ];
        let assigned = BTreeSet::from(["assign-1".to_owned()]);
        let views = BTreeMap::new();
        let source = evidence(Ok(&orders), Ok(&[]), Ok(&[]), &views, Ok(&assigned));
        let values = adapters(&specs, &source, &window());
        assert!(matches!(
            values.get(query),
            Some(QueryValue::Value { value: 1.0, evidence_refs, .. }) if evidence_refs == &["relay-order:direct-1"]
        ));
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn action_send_requires_timely_minder_send_and_queued_receipt() {
        let query = PUSH_UNSENT;
        let specs = [RoleSpec {
            role: "minder".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "MIND-R03-E1")],
        }];
        let actions = vec![map(
            json!({"action_id":7, "at_ms":1100, "due_by":1400, "target_addr":"test:workspace:coder"}),
        )];
        let views = BTreeMap::from([("push_next_ticket_actions".to_owned(), Ok(actions))]);
        let sends = [map(
            json!({"action_id":7,"receipt_id":"receipt-7","sender_role":"minder","sender_addr":"test:workspace:minder","recipient_addr":"test:workspace:coder","sent_at":1200}),
        )];
        let receipts = [map(
            json!({"event_id":"event-7","original_id":"receipt-7","attempt_id":null,"outcome":"queued","sender":"test:workspace:minder","target":"test:workspace:coder","event_time_ms":1200,"observed_at_ms":1200,"evidence":null,"reason":null,"session":null,"expect_ticket":"SWM-221","delivered_at_ms":null}),
        )];
        let assigned = BTreeSet::new();
        let source = evidence(Ok(&[]), Ok(&receipts), Ok(&sends), &views, Ok(&assigned));
        let values = adapters(&specs, &source, &window());
        assert!(matches!(
            values.get(query),
            Some(QueryValue::Value { value: 0.0, .. })
        ));

        let no_receipt = evidence(Ok(&[]), Ok(&[]), Ok(&sends), &views, Ok(&assigned));
        let values = adapters(&specs, &no_receipt, &window());
        assert!(
            matches!(values.get(query), Some(QueryValue::Value { value: 1.0, evidence_refs, .. }) if evidence_refs == &["fleet-action:7"])
        );
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn action_not_yet_due_at_window_end_is_not_judged_as_missing() {
        let query = PUSH_UNSENT;
        let specs = [RoleSpec {
            role: "minder".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "MIND-R03-E1")],
        }];
        let actions = vec![map(
            json!({"action_id":8, "at_ms":1900, "due_by":2300, "target_addr":"test:workspace:coder"}),
        )];
        let views = BTreeMap::from([("push_next_ticket_actions".to_owned(), Ok(actions))]);
        let assigned = BTreeSet::new();
        let source = evidence(Ok(&[]), Ok(&[]), Ok(&[]), &views, Ok(&assigned));
        let values = adapters(&specs, &source, &window());
        assert!(matches!(
            values.get(query),
            Some(QueryValue::Value { value: 0.0, .. })
        ));
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn blocked_lane_rule_counts_only_minder_sends_during_complete_block_interval() {
        let query = BLOCKED_SEND;
        let specs = [RoleSpec {
            role: "minder".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "MIND-R19-E1")],
        }];
        let intervals = vec![map(
            json!({"lane_addr":"test:workspace:coder", "start_ms":1100, "end_ms":1500}),
        )];
        let views = BTreeMap::from([("blocked_lane_ticks".to_owned(), Ok(intervals))]);
        let receipts = [
            map(
                json!({"event_id":"event-bad","original_id":"bad","attempt_id":null,"outcome":"queued","sender":"test:workspace:minder","target":"test:workspace:coder","event_time_ms":1200,"observed_at_ms":1200,"evidence":null,"reason":null,"session":null,"expect_ticket":null,"delivered_at_ms":null}),
            ),
            map(
                json!({"event_id":"event-other","original_id":"other","attempt_id":null,"outcome":"queued","sender":"test:workspace:minder","target":"test:workspace:lead","event_time_ms":1200,"observed_at_ms":1200,"evidence":null,"reason":null,"session":null,"expect_ticket":null,"delivered_at_ms":null}),
            ),
            map(
                json!({"event_id":"event-outside","original_id":"outside","attempt_id":null,"outcome":"queued","sender":"test:workspace:minder","target":"test:workspace:coder","event_time_ms":1600,"observed_at_ms":1600,"evidence":null,"reason":null,"session":null,"expect_ticket":null,"delivered_at_ms":null}),
            ),
        ];
        let assigned = BTreeSet::new();
        let source = evidence(Ok(&[]), Ok(&receipts), Ok(&[]), &views, Ok(&assigned));
        let values = adapters(&specs, &source, &window());
        assert!(
            matches!(values.get(query), Some(QueryValue::Value { value: 1.0, evidence_refs, .. }) if evidence_refs == &["relay-receipt:bad"])
        );
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn need_ready_actions_require_distinct_orders_to_the_planner_in_the_tick() {
        let query = NEED_READY_UNSENT;
        let specs = [RoleSpec {
            role: "orchestrator".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule(query, "ORCH-R14-E1")],
        }];
        let actions = vec![
            map(json!({"action_id":11, "at_ms":1100, "planner_addr":"test:workspace:planner"})),
            map(json!({"action_id":12, "at_ms":1150, "planner_addr":"test:workspace:planner"})),
        ];
        let views = BTreeMap::from([("need_ready_tickets_actions".to_owned(), Ok(actions))]);
        let orders = [map(
            json!({"order_id":"planner-order-1", "sender":"test:workspace:orchestrator", "recipient":"test:workspace:planner", "receipt_target":"test:workspace:planner", "expect_ticket":null, "ack_ms":30000, "deadline_at":31200, "enqueued_at":1200, "delivered_at":null, "acted_at":null, "state":"queued", "expired":false}),
        )];
        let assigned = BTreeSet::new();
        let mut source = evidence(Ok(&orders), Ok(&[]), Ok(&[]), &views, Ok(&assigned));
        source.tick_interval_ms = Some(300);
        let values = adapters(&specs, &source, &window());
        assert!(matches!(
            values.get(query),
            Some(QueryValue::Value { value: 1.0, evidence_refs, .. }) if evidence_refs == &["fleet-action:12"]
        ));
    }

    /// Trace: FR-047-AC-6
    #[trace("FR-047-AC-6")]
    #[test]
    fn complete_empty_relay_pages_are_accepted_but_partial_pages_are_rejected() {
        let orders = order_page();
        assert!(validate_order_page(&orders, 1_000, 1_999, None).is_ok());

        let mut incomplete_orders = order_page();
        incomplete_orders.complete = false;
        assert!(validate_order_page(&incomplete_orders, 1_000, 1_999, None).is_err());

        let receipts = receipt_export();
        assert!(validate_receipt_page(&receipts, 1_000, 1_999, None).is_ok());

        let mut incomplete_receipts = receipt_export();
        incomplete_receipts
            .reasons
            .push(relay_core::receipts::ReceiptCoverageReason::RecordingGap);
        assert!(validate_receipt_page(&incomplete_receipts, 1_000, 1_999, None).is_err());
    }

    /// Trace: FR-047-AC-6
    #[trace("FR-047-AC-6")]
    #[test]
    fn truncated_relay_pages_require_a_matching_advancing_cursor() {
        let mut page = order_page();
        let state = relay_core::receipts::OrderState {
            order_id: "order-1".to_owned(),
            sender: "test:workspace:orchestrator".to_owned(),
            recipient: "test:workspace:coder".to_owned(),
            receipt_target: "test:workspace:coder".to_owned(),
            expect_ticket: Some("SWM-221".to_owned()),
            ack_ms: 10_000,
            deadline_at: 11_000,
            enqueued_at_ms: 1_000,
            delivered_at_ms: None,
            acted_at_ms: None,
            state: "queued".to_owned(),
            expired: false,
        };
        page.orders.push(state);
        page.truncated = true;
        page.next_cursor = Some(relay_core::receipts::OrderCursor {
            enqueued_at_ms: 1_000,
            order_id: "order-1".to_owned(),
        });
        assert!(validate_order_page(&page, 1_000, 1_999, None).is_ok());
        assert!(validate_order_page(&page, 1_000, 1_999, page.next_cursor.as_ref()).is_err());
        page.next_cursor.as_mut().expect("cursor fixture").order_id = "wrong".to_owned();
        assert!(validate_order_page(&page, 1_000, 1_999, None).is_err());
    }

    /// Trace: FR-047-AC-6
    #[trace("FR-047-AC-6")]
    #[test]
    fn receipt_page_cursor_must_match_last_real_receipt() {
        let mut page = receipt_export();
        let event = queued_ordered_receipt();
        page.next_cursor = Some(event.cursor.clone());
        page.has_more = true;
        page.events.push(event);
        assert!(validate_receipt_page(&page, 1_000, 1_999, None).is_ok());
        assert!(validate_receipt_page(&page, 1_000, 1_999, Some("1700000000000-0")).is_err());
        page.next_cursor = Some("1700000000000-9".to_owned());
        assert!(validate_receipt_page(&page, 1_000, 1_999, None).is_err());
    }

    /// Trace: FR-047-AC-6
    #[trace("FR-047-AC-6")]
    #[test]
    fn live_adapter_does_no_source_io_when_no_relay_rule_is_requested() {
        assert!(
            live_adapters(&[], &window(), None, None, None, None, None, Duration::ZERO,).is_empty()
        );
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn live_action_normalization_uses_fleet_ledger_action_rows() {
        let rows = [map(json!({
            "id": 42,
            "kind": "route_dead_lane",
            "lane": "test:workspace:coder",
            "emitted_at": 1200,
            "target_role": "orchestrator",
            "target_addr": "test:workspace:orchestrator"
        }))];
        let normalized = normalize_actions(&rows, "route_dead_lane");
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0]["action_id"], json!(42));
        assert_eq!(normalized[0]["at_ms"], json!(1200));
        assert_eq!(normalized[0]["lane_addr"], json!("test:workspace:coder"));
    }

    /// Trace: FR-047-AC-6
    #[trace("FR-047-AC-6")]
    #[test]
    fn old_action_send_schema_is_unavailable_even_for_an_empty_table() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("create ledger fixture");
        connection
            .execute_batch("PRAGMA user_version=6; CREATE TABLE action_send(action_id INTEGER, sent_at INTEGER, sender_addr TEXT, recipient_addr TEXT, receipt_id TEXT);")
            .expect("create v6 action_send table");
        assert!(require_sender_role(Vec::new(), &path).is_err());
        connection
            .execute_batch(
                "PRAGMA user_version=7; ALTER TABLE action_send ADD COLUMN sender_role TEXT;",
            )
            .expect("upgrade fixture to v7 action_send shape");
        assert!(require_sender_role(Vec::new(), &path).is_ok());
    }

    /// Trace: FR-047-AC-6
    #[trace("FR-047-AC-6")]
    #[test]
    fn ledger_assignment_rows_without_order_identity_are_incomplete() {
        assert!(normalize_assigned_order_ids(vec![map(json!({"assigned_at":1200}))]).is_err());
        assert!(normalize_assigned_order_ids(Vec::new()).is_ok_and(|ids| ids.is_empty()));
    }

    /// Trace: FR-047-AC-1
    #[trace("FR-047-AC-1")]
    #[test]
    fn blocked_intervals_come_from_complete_lane_tick_history() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("create ledger fixture");
        connection
            .execute_batch(
                "PRAGMA user_version=6;
                 CREATE TABLE lane_tick_rows(lane TEXT, at_ms INTEGER, class TEXT);
                 INSERT INTO lane_tick_rows VALUES
                   ('test:workspace:coder',800,'IDLE'),
                   ('test:workspace:coder',1100,'BLOCKED'),
                   ('test:workspace:coder',1500,'BLOCKED'),
                   ('test:workspace:coder',2100,'IDLE');",
            )
            .expect("write lane status history");
        let intervals = read_blocked_intervals(
            &path,
            &EvalWindow {
                start_ms: 1_000,
                end_ms: 2_000,
            },
            300,
        )
        .expect("complete lane history");
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0]["start_ms"], json!(1100));
        assert_eq!(intervals[0]["end_ms"], json!(2100));

        connection
            .execute("DELETE FROM lane_tick_rows", [])
            .expect("remove lane history");
        assert!(
            read_blocked_intervals(
                &path,
                &EvalWindow {
                    start_ms: 1_000,
                    end_ms: 2_000,
                },
                300,
            )
            .is_err()
        );
    }
}
