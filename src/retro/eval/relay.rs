// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic relay and action-delivery query adapters (FR-047).

use super::{EvalWindow, QueryValue, RoleSpec};
use ix_trace_rs::implements;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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

fn requested_queries(specs: &[RoleSpec]) -> BTreeSet<String> {
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
        let source = evidence(Ok(&orders), Ok(&[]), Ok(&[]), &views, Ok(&assigned));
        let values = adapters(&specs, &source, &window());
        assert!(matches!(
            values.get(query),
            Some(QueryValue::Value { value: 1.0, evidence_refs, .. }) if evidence_refs == &["fleet-action:12"]
        ));
    }
}
