// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Pure reducers for read-only Fleet ledger view results (FR-047).

use super::{EvalWindow, QueryValue, RoleSpec};
use ix_trace_rs::implements;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Complete read-only rows, keyed by the stable ledger view name. A failed or
/// partial view read is represented by `Err` and cannot become a numeric zero.
pub(super) type ViewRows = BTreeMap<String, Result<Vec<Map<String, Value>>, String>>;

#[derive(Clone, Copy)]
enum Reduction {
    Count,
    Sum(&'static str),
    Max(&'static str),
    Ratio(&'static str, &'static str),
    NotApplicable(&'static str),
    Unsupported(&'static str),
}

/// Computes values only for the deterministic ledger queries requested by the
/// selected role specifications. Views not in the registry are unavailable.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-6")]
pub(super) fn adapters(
    specs: &[RoleSpec],
    views: &ViewRows,
    window: &EvalWindow,
    coverage: Result<(), &str>,
    capacity_coverage: Result<(), &str>,
) -> BTreeMap<String, QueryValue> {
    let requested = specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .filter(|rule| rule.method == "deterministic")
        .map(|rule| rule.query_or_rubric.as_str())
        .filter(|query| query.starts_with("ledger."))
        .collect::<BTreeSet<_>>();

    requested
        .into_iter()
        .map(|query| {
            let view = view_for(query);
            let source_coverage = if needs_capacity_coverage(query) {
                &capacity_coverage
            } else {
                &coverage
            };
            let value = match view.reduction {
                Reduction::NotApplicable(reason) => QueryValue::NotApplicable {
                    reason: reason.to_owned(),
                    evidence_refs: Vec::new(),
                },
                Reduction::Unsupported(reason) => unavailable(reason),
                reduction => match (source_coverage, views.get(view.name)) {
                    (Err(reason), _) => unavailable((*reason).to_owned()),
                    (Ok(()), Some(Ok(rows))) => {
                        reduce_view(query, view.name, rows, window, reduction)
                    }
                    (Ok(()), Some(Err(reason))) => unavailable(reason.clone()),
                    (Ok(()), None) => {
                        unavailable(format!("ledger view {} was not read", view.name))
                    }
                },
            };
            (query.to_owned(), value)
        })
        .collect()
}

struct ViewSpec<'a> {
    name: &'a str,
    reduction: Reduction,
}

/// Returns the stable view to read for a supported query. Queries whose public
/// view cannot be reduced over an event window are deliberately not queried.
pub(super) fn readable_view<'a>(query: &'a str) -> Option<&'a str> {
    let view = view_for(query);
    (!matches!(
        view.reduction,
        Reduction::Unsupported(_) | Reduction::NotApplicable(_)
    ))
    .then_some(view.name)
}

#[implements("FR-047-AC-6")]
pub(super) fn needs_capacity_coverage(query: &str) -> bool {
    matches!(
        query.strip_prefix("ledger.").unwrap_or_default(),
        "discovery_steps"
            | "discovery_steps_violating_ramp"
            | "discovery_steps_judged_without_demand"
            | "discovery_knee_signals"
            | "discovery_raised_past_knee"
            | "discovery_runs_ended"
            | "discovery_runs_without_recorded_level"
            | "unrecorded_hosts_with_demand"
            | "unrecorded_hosts_without_discovery_run"
            | "hosts_with_recorded_level"
            | "host_step_downs"
            | "raises_during_cool_off"
            | "discovery_runs"
            | "discovery_runs_per_host_day"
            | "discovery_runs_per_host_day_max"
            | "host_minutes_above_recorded_level_outside_bounded_discovery"
            | "overload_events_without_draining_mark_in_cycle"
            | "lane_removals"
            | "lane_removals_before_drain"
            | "lane_change_unrecorded"
            | "lanes_lost"
            | "draining_lanes"
            | "drains_over_cap_without_escalation"
            | "discovery_inconclusive_windows"
            | "no_demand_runs_not_ended"
            | "discovery_slot_saturated_windows"
            | "discovery_raised_past_slot_saturation"
            | "lane_count_reductions"
            | "lane_reductions_without_throttle_evidence"
            | "host_output_share_of_recorded_level"
    )
}

#[implements("FR-047-AC-6")]
pub(super) fn time_column(query: &str) -> &'static str {
    match query.strip_prefix("ledger.").unwrap_or_default() {
        "cycle_missed" => "detected_at",
        _ => "at_ms",
    }
}

/// Reads one event-time slice through SQLite's read-only API. The fleet writer
/// changes its exact schema version as migrations land; retro evaluation only
/// needs stable views and therefore accepts the v3-v8 stack range without
/// opening the ledger for migration or write access.
#[implements("FR-047-AC-6")]
pub(super) fn read_view_rows(
    path: &std::path::Path,
    object: &str,
    time_column: &str,
    window: &EvalWindow,
) -> Result<Vec<Map<String, Value>>, String> {
    read_rows(path, object, time_column, window)
}

#[implements("FR-047-AC-6")]
pub(super) fn read_coverage(path: &std::path::Path, window: &EvalWindow) -> Result<(), String> {
    if read_rows(path, "run", "relay_ms", window)?.is_empty() {
        Err("fleet ledger has no collector run coverage in this event-time window".to_owned())
    } else {
        Ok(())
    }
}

#[implements("FR-047-AC-6")]
pub(super) fn read_capacity_coverage(
    path: &std::path::Path,
    window: &EvalWindow,
) -> Result<(), String> {
    for (table, time_column) in [
        ("capacity_run", "started_at"),
        ("capacity_step", "window_end"),
        ("capacity_level", "recorded_at"),
        ("capacity_event", "at"),
        ("saturation_sample", "at"),
    ] {
        match read_rows(path, table, time_column, window) {
            Ok(rows) if !rows.is_empty() => return Ok(()),
            Ok(_) => {}
            Err(reason)
                if reason.contains("ledger object ") && reason.contains(" is unavailable:") => {}
            Err(reason) => return Err(reason),
        }
    }
    Err("fleet capacity ledger has no event-time data coverage in this window".to_owned())
}

#[implements("FR-047-AC-6")]
fn read_rows(
    path: &std::path::Path,
    object: &str,
    time_column: &str,
    window: &EvalWindow,
) -> Result<Vec<Map<String, Value>>, String> {
    if !is_identifier(object) || !is_identifier(time_column) {
        return Err("fleet ledger query contains an invalid identifier".to_owned());
    }
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("{}: {error}", path.display()))?;
    let version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| {
            format!(
                "{}: cannot read ledger schema version: {error}",
                path.display()
            )
        })?;
    if !(3..=8).contains(&version) {
        return Err(format!(
            "{}: unsupported ledger schema version {version}; retro evaluator supports 3 through 8",
            path.display()
        ));
    }
    let object_type = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = ?1 AND type IN ('view', 'table')",
            [object],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| {
            format!(
                "{}: ledger object {object} is unavailable: {error}",
                path.display()
            )
        })?;
    let has_time_column = connection
        .prepare(&format!("PRAGMA table_info(\"{object}\")"))
        .and_then(|mut statement| {
            let names = statement.query_map([], |row| row.get::<_, String>(1))?;
            names.collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| {
            format!(
                "{}: cannot inspect ledger {object}: {error}",
                path.display()
            )
        })?
        .iter()
        .any(|column| column == time_column);
    if !has_time_column {
        return Err(format!(
            "ledger {object_type} {object} has no {time_column} column"
        ));
    }
    let sql = format!(
        "SELECT * FROM \"{object}\" WHERE \"{time_column}\" >= ?1 AND \"{time_column}\" < ?2"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("{}: cannot read ledger {object}: {error}", path.display()))?;
    let columns = statement
        .column_names()
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let rows = statement
        .query_map([window.start_ms, window.end_ms], |row| {
            let mut values = Map::new();
            for (index, name) in columns.iter().enumerate() {
                let value = match row.get_ref(index)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(value) => Value::from(value),
                    rusqlite::types::ValueRef::Real(value) => {
                        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
                    }
                    rusqlite::types::ValueRef::Text(value) => Value::String(
                        std::str::from_utf8(value)
                            .map_err(|error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    index,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            })?
                            .to_owned(),
                    ),
                    rusqlite::types::ValueRef::Blob(_) => {
                        return Err(rusqlite::Error::InvalidColumnType(
                            index,
                            name.clone(),
                            rusqlite::types::Type::Blob,
                        ));
                    }
                };
                values.insert(name.clone(), value);
            }
            Ok(values)
        })
        .map_err(|error| format!("{}: cannot query ledger {object}: {error}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "{}: invalid row in ledger {object}: {error}",
                path.display()
            )
        })?;
    Ok(rows)
}

#[implements("FR-047-AC-6")]
fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn view_for(query: &str) -> ViewSpec<'_> {
    use Reduction::{Count, NotApplicable, Ratio, Sum, Unsupported};

    let short = query.strip_prefix("ledger.").unwrap_or_default();
    let spec = match short {
        "idle_lane_episode_with_ready_work_max"
        | "plan_coverage_gap_open_cycles_max"
        | "short_queue_open_cycles_max"
        | "drift_finding_open_cycles_max"
        | "orchestrator_rung_open_cycles_max"
        | "add_lane_action_open_cycles_max"
        | "assignment_confirm_latency_max"
        | "escalation_rung_send_latency_max" => ViewSpec {
            name: short,
            reduction: Unsupported(
                "ledger view exposes an all-history maximum, so this event window cannot be isolated",
            ),
        },
        "time_to_action_p90" | "detection_to_action_p90" | "assignment_confirm_latency_p90" => {
            ViewSpec {
                name: short,
                reduction: Unsupported(
                    "ledger view exposes daily p90 values; a window-wide p90 cannot be reconstructed",
                ),
            }
        }
        "idle_lane_minutes_during_build_pause" => ViewSpec {
            name: short,
            reduction: NotApplicable("no pause producer"),
        },
        "idle_capacity_minutes_with_ready_work" => ViewSpec {
            name: short,
            reduction: Sum("slot_minutes"),
        },
        "missed_idle_minutes" | "blocked_lane_minutes" => ViewSpec {
            name: short,
            reduction: Sum("minutes"),
        },
        "drift_lane_minutes_share" => ViewSpec {
            name: short,
            reduction: Ratio("drift_minutes", "working_minutes"),
        },
        "host_minutes_above_recorded_level_outside_bounded_discovery" => ViewSpec {
            name: short,
            reduction: Unsupported(
                "ledger view exposes a cross-window aggregate without event-time rows",
            ),
        },
        "host_output_share_of_recorded_level" => ViewSpec {
            name: short,
            reduction: Unsupported(
                "ledger view contains only each host's latest sample, not a complete event window",
            ),
        },
        "discovery_runs_per_host_day_max" => ViewSpec {
            name: short,
            reduction: Unsupported(
                "ledger view exposes a global maximum, so this event window cannot be isolated",
            ),
        },
        "discovery_runs_per_host_day" => ViewSpec {
            name: short,
            reduction: Sum("runs"),
        },
        // These views return one row per qualifying event and carry an at_ms
        // timestamp, so counting the selected rows is event-window correct.
        "discovery_steps"
        | "discovery_steps_violating_ramp"
        | "discovery_steps_judged_without_demand"
        | "discovery_knee_signals"
        | "discovery_raised_past_knee"
        | "discovery_runs_ended"
        | "discovery_runs_without_recorded_level"
        | "unrecorded_hosts_with_demand"
        | "unrecorded_hosts_without_discovery_run"
        | "hosts_with_recorded_level"
        | "host_step_downs"
        | "raises_during_cool_off"
        | "discovery_runs"
        | "overload_events_without_draining_mark_in_cycle"
        | "lane_removals"
        | "lane_removals_before_drain"
        | "lane_change_unrecorded"
        | "lanes_lost"
        | "draining_lanes"
        | "drains_over_cap_without_escalation"
        | "discovery_inconclusive_windows"
        | "no_demand_runs_not_ended"
        | "discovery_slot_saturated_windows"
        | "discovery_raised_past_slot_saturation"
        | "lane_count_reductions"
        | "lane_reductions_without_throttle_evidence"
        | "lane_orders_due"
        | "lane_orders_not_taken_up"
        | "need_ready_tickets_due"
        | "need_ready_tickets_without_planner_order"
        | "lanes_with_ready_work"
        | "idle_lane_with_ready_work_ticks"
        | "defect_rows"
        | "plan_coverage_gap_rows"
        | "drift_findings"
        | "assignments_sent"
        | "unconfirmed_assign_results"
        | "unconfirmed_assignments_without_followup"
        | "ladder_rungs_emitted"
        | "escalation_rungs_suppressed"
        | "escalation_rung"
        | "orchestrator_rungs_emitted"
        | "act_on_undetermined_actions"
        | "push_next_ticket_actions"
        | "answer_dialog_actions"
        | "escalate_block_actions"
        | "add_lane_actions"
        | "act_on_undetermined_unexecuted_ticks"
        | "minder_addressed_actions"
        | "tick_actions_unexecuted_by_minder"
        | "minder_turns_without_committed_tick"
        | "minder_orders_unconfirmed_at_deadline"
        | "unconfirmed_orders_not_resent_or_escalated"
        | "need_ready_tickets_actions"
        | "orchestrator_cycle_events"
        | "cycle_missed"
        | "failed_probe_ticks"
        | "probe_failure_without_routed_defect" => ViewSpec {
            name: short,
            reduction: Count,
        },
        _ => ViewSpec {
            name: short,
            reduction: Unsupported("ledger query has no registered window-correct reducer"),
        },
    };
    spec
}

fn reduce_view(
    query: &str,
    view: &str,
    rows: &[Map<String, Value>],
    window: &EvalWindow,
    reduction: Reduction,
) -> QueryValue {
    let mut selected = Vec::new();
    for row in rows {
        if unavailable_row(row) {
            let reason = row
                .get("unavailable_reason")
                .and_then(Value::as_str)
                .unwrap_or("ledger view marks source data unavailable");
            if view == "idle_lane_minutes_during_build_pause" && reason == "no pause producer" {
                return QueryValue::NotApplicable {
                    reason: reason.to_owned(),
                    evidence_refs: Vec::new(),
                };
            }
            return unavailable(reason.to_owned());
        }
        let Some(at_ms) = row.get("at_ms").and_then(Value::as_i64) else {
            return unavailable(format!("ledger view {view} row has no integer at_ms"));
        };
        if at_ms >= window.start_ms && at_ms < window.end_ms {
            selected.push(row);
        }
    }

    let mut evidence_refs = Vec::new();
    for row in &selected {
        if let Some(identity) = identity(row) {
            evidence_refs.push(format!("fleet-ledger:{view}:{identity}"));
        }
    }
    evidence_refs.sort();
    evidence_refs.dedup();

    let observed = match reduction {
        Reduction::Count => selected.len() as f64,
        Reduction::Sum(field) => {
            let mut total = 0.0;
            for row in &selected {
                let Some(value) = row.get(field).and_then(Value::as_f64) else {
                    return unavailable(format!("ledger view {view} row has no numeric {field}"));
                };
                total += value;
            }
            total
        }
        Reduction::Max(field) => {
            let Some(maximum) = selected
                .iter()
                .filter_map(|row| row.get(field).and_then(Value::as_f64))
                .reduce(f64::max)
            else {
                if selected.is_empty() {
                    return value(0.0, evidence_refs);
                }
                return unavailable(format!("ledger view {view} has no numeric {field}"));
            };
            maximum
        }
        Reduction::Ratio(numerator, denominator) => {
            let mut numerator_total = 0.0;
            let mut denominator_total = 0.0;
            for row in &selected {
                let (Some(n), Some(d)) = (
                    row.get(numerator).and_then(Value::as_f64),
                    row.get(denominator).and_then(Value::as_f64),
                ) else {
                    return unavailable(format!(
                        "ledger view {view} row lacks numeric {numerator}/{denominator}"
                    ));
                };
                numerator_total += n;
                denominator_total += d;
            }
            if denominator_total <= 0.0 {
                return unavailable(format!("ledger view {view} has no positive denominator"));
            }
            numerator_total / denominator_total
        }
        Reduction::NotApplicable(reason) => {
            return QueryValue::NotApplicable {
                reason: reason.to_owned(),
                evidence_refs,
            };
        }
        Reduction::Unsupported(reason) => return unavailable(reason.to_owned()),
    };
    if !observed.is_finite() {
        return unavailable(format!(
            "ledger query {query} from view {view} produced a non-finite result"
        ));
    }
    // A zero count is valid only after a complete, successful view read and
    // full event-time filtering. Empty rows in a missing view never reach here.
    value(observed, evidence_refs)
}

fn unavailable_row(row: &Map<String, Value>) -> bool {
    row.get("available")
        .and_then(Value::as_i64)
        .is_some_and(|available| available == 0)
        || row
            .get("available")
            .and_then(Value::as_bool)
            .is_some_and(|available| !available)
}

#[implements("FR-047-AC-1")]
fn identity(row: &Map<String, Value>) -> Option<String> {
    if let (Some(run_id), Some(step_no)) = (scalar(row.get("run_id")), scalar(row.get("step_no"))) {
        return Some(format!("run:{run_id}:step:{step_no}"));
    }
    [
        "action_id",
        "order_id",
        "defect_id",
        "finding_id",
        "run_id",
        "step_no",
        "event_id",
        "lane",
        "host",
        "day",
        "at_ms",
    ]
    .into_iter()
    .find_map(|field| scalar(row.get(field)))
}

fn scalar(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn unavailable(reason: impl Into<String>) -> QueryValue {
    QueryValue::Unavailable {
        reason: reason.into(),
        evidence_refs: Vec::new(),
    }
}

fn value(value: f64, evidence_refs: Vec<String>) -> QueryValue {
    QueryValue::Value {
        value,
        evidence_refs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retro::eval::{EvaluationRule, ObjectiveMetricSpec};
    use ix_trace_rs::trace;

    fn spec(query: &str) -> RoleSpec {
        RoleSpec {
            role: "orchestrator".to_owned(),
            metrics: Vec::<ObjectiveMetricSpec>::new(),
            rules: vec![EvaluationRule {
                id: "ORCH-R1".to_owned(),
                rule: "ledger metric".to_owned(),
                evidence_source: "fleet ledger".to_owned(),
                trigger: "event".to_owned(),
                method: "deterministic".to_owned(),
                query_or_rubric: query.to_owned(),
                comparator: ">=".to_owned(),
                target: 1.0,
                unit: "events".to_owned(),
            }],
        }
    }

    fn row(fields: &[(&str, Value)]) -> Map<String, Value> {
        fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn counts_complete_discovery_rows_in_the_half_open_window() {
        let views = BTreeMap::from([(
            "discovery_steps".to_owned(),
            Ok(vec![
                row(&[
                    ("run_id", Value::from(4)),
                    ("step_no", Value::from(1)),
                    ("at_ms", Value::from(100)),
                ]),
                row(&[
                    ("run_id", Value::from(4)),
                    ("step_no", Value::from(2)),
                    ("at_ms", Value::from(200)),
                ]),
                row(&[
                    ("run_id", Value::from(4)),
                    ("step_no", Value::from(3)),
                    ("at_ms", Value::from(300)),
                ]),
            ]),
        )]);
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 300,
        };
        let result = adapters(
            &[spec("ledger.discovery_steps")],
            &views,
            &window,
            Ok(()),
            Ok(()),
        );
        let Some(QueryValue::Value {
            value: observed,
            evidence_refs,
        }) = result.get("ledger.discovery_steps")
        else {
            panic!("successful discovery view should produce a numeric row");
        };
        assert_eq!(*observed, 2.0);
        assert_eq!(evidence_refs.len(), 2);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn counts_unrecorded_hosts_with_demand_rows() {
        let views = BTreeMap::from([(
            "unrecorded_hosts_with_demand".to_owned(),
            Ok(vec![row(&[
                ("host", Value::from("draco")),
                ("at_ms", Value::from(150)),
                ("ready_buffer", Value::from(3)),
                ("active_lanes", Value::from(2)),
                ("deficit", Value::from(7)),
            ])]),
        )]);
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 200,
        };
        let result = adapters(
            &[spec("ledger.unrecorded_hosts_with_demand")],
            &views,
            &window,
            Ok(()),
            Ok(()),
        );
        assert_eq!(
            result["ledger.unrecorded_hosts_with_demand"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["fleet-ledger:unrecorded_hosts_with_demand:draco".into()],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn source_failure_and_missing_view_are_unavailable_not_zero() {
        let views = BTreeMap::from([(
            "failed_probe_ticks".to_owned(),
            Err("ledger is stale".to_owned()),
        )]);
        let window = EvalWindow {
            start_ms: 1,
            end_ms: 2,
        };
        for query in ["ledger.failed_probe_ticks", "ledger.unrecognized_view"] {
            let result = adapters(&[spec(query)], &views, &window, Ok(()), Ok(()));
            assert!(matches!(
                result.get(query),
                Some(QueryValue::Unavailable { .. })
            ));
        }
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn known_pause_absence_is_not_applicable_with_exact_reason() {
        let views = ViewRows::new();
        let window = EvalWindow {
            start_ms: 0,
            end_ms: 10,
        };
        let result = adapters(
            &[spec("ledger.idle_lane_minutes_during_build_pause")],
            &views,
            &window,
            Err("missing coverage"),
            Err("missing capacity coverage"),
        );
        assert!(matches!(
            result.get("ledger.idle_lane_minutes_during_build_pause"),
            Some(QueryValue::NotApplicable { reason, .. }) if reason == "no pause producer"
        ));
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn read_only_window_reader_accepts_stacked_schema_six_and_filters_event_time() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("create ledger fixture");
        connection
            .execute_batch(
                "PRAGMA user_version = 6;
                 CREATE TABLE sample(at_ms INTEGER NOT NULL, host TEXT NOT NULL);
                 INSERT INTO sample VALUES (99, 'before'), (100, 'start'), (199, 'inside'), (200, 'end');
                 CREATE VIEW discovery_steps AS SELECT at_ms, host FROM sample;",
            )
            .expect("write ledger fixture");
        drop(connection);

        let rows = read_view_rows(
            &path,
            "discovery_steps",
            "at_ms",
            &EvalWindow {
                start_ms: 100,
                end_ms: 200,
            },
        )
        .expect("read v6 view without migrating it");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["host"], Value::from("start"));
        assert_eq!(rows[1]["host"], Value::from("inside"));
        let connection = rusqlite::Connection::open(&path).expect("reopen ledger");
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .expect("read unchanged schema version"),
            6
        );
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn read_only_window_reader_rejects_unknown_future_schema_versions() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("create ledger fixture");
        connection
            .pragma_update(None, "user_version", 9)
            .expect("set future version");
        drop(connection);
        let error = read_view_rows(
            &path,
            "discovery_steps",
            "at_ms",
            &EvalWindow {
                start_ms: 0,
                end_ms: 10,
            },
        )
        .expect_err("future schema must fail closed");
        assert!(error.contains("supports 3 through 8"));
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn empty_view_is_unavailable_without_collector_coverage() {
        let views = BTreeMap::from([("discovery_steps".to_owned(), Ok(Vec::new()))]);
        let result = adapters(
            &[spec("ledger.discovery_steps")],
            &views,
            &EvalWindow {
                start_ms: 100,
                end_ms: 200,
            },
            Ok(()),
            Err("fleet capacity ledger has no event-time data coverage in this window"),
        );
        assert_eq!(
            result["ledger.discovery_steps"],
            QueryValue::Unavailable {
                reason: "fleet capacity ledger has no event-time data coverage in this window"
                    .to_owned(),
                evidence_refs: Vec::new(),
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn ledger_coverage_requires_a_collector_run_inside_the_window() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("create ledger fixture");
        connection
            .execute_batch(
                "PRAGMA user_version = 6;
                 CREATE TABLE run(relay_ms INTEGER NOT NULL);
                 INSERT INTO run VALUES (99), (100), (199), (200);",
            )
            .expect("write run fixture");
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 200,
        };
        assert!(read_coverage(&path, &window).is_ok());
        connection
            .execute(
                "DELETE FROM run WHERE relay_ms >= 100 AND relay_ms < 200",
                [],
            )
            .expect("remove in-window runs");
        assert!(
            read_coverage(&path, &window)
                .expect_err("uncovered window must fail")
                .contains("no collector run coverage")
        );
    }

    #[test]
    #[trace("FR-047-AC-6")]
    fn capacity_view_coverage_requires_capacity_data_inside_the_window() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("create ledger fixture");
        connection
            .execute_batch(
                "PRAGMA user_version = 6;
                 CREATE TABLE run(relay_ms INTEGER NOT NULL);
                 CREATE TABLE capacity_run(started_at INTEGER NOT NULL);
                 CREATE TABLE capacity_step(window_end INTEGER NOT NULL);
                 CREATE TABLE capacity_level(recorded_at INTEGER NOT NULL);
                 CREATE TABLE capacity_event(at INTEGER NOT NULL);
                 CREATE TABLE saturation_sample(at INTEGER NOT NULL);
                 INSERT INTO run VALUES (150);",
            )
            .expect("write empty capacity fixture");
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 200,
        };
        assert!(read_coverage(&path, &window).is_ok());
        assert!(
            read_capacity_coverage(&path, &window)
                .expect_err("collector runs do not prove capacity data coverage")
                .contains("no event-time data coverage")
        );
        connection
            .execute("INSERT INTO capacity_step VALUES (150)", [])
            .expect("add capacity coverage row");
        assert!(read_capacity_coverage(&path, &window).is_ok());
    }
}
