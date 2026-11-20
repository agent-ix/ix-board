// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic policy assessment and persisted-only comparisons (FR-029).
use super::{
    Basis, CLASSES, ClassConfig, Comparator, Config, Measurement, OwnerAction, Policy, Report,
    Statistic, Value,
};
use crate::retro::Status;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Severity precedence is independent from evidence completeness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Every required value is assessable and below boundaries.
    Green,
    /// At least one required value is unassessable.
    Unknown,
    /// A required value breaches amber.
    Amber,
    /// A required value breaches red.
    Red,
}
wire! {
    /// Severity with independently preserved missing-evidence diagnostics.
    Summary { /// Canonical worst severity.
        severity: Severity, /// All required members assessable.
        complete: bool, /// Unknown native metric/key/class IDs and reasons.
        missing: BTreeMap<String, String> }
}
wire! {
    /// Assessment for one scalar or expected key.
    KeyAssessment { /// Selected current or difference value.
        value: Option<f64>, /// Result of explicit threshold policy.
        severity: Severity, /// Missing or unassessed reason.
        reason: Option<String> }
}
wire! {
    /// One metric policy result retaining each selected key.
    MetricAssessment { /// Applied policy, if any.
        policy: Option<Policy>, /// Per-key assessment; scalar uses the empty key.
        keys: BTreeMap<String, KeyAssessment>, /// Reduced result with missing-key reasons.
        summary: Summary, /// Explicit unassessed reason.
        reason: Option<String> }
}
/// An owner/action route is either configured or explicitly absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "status",
    content = "route",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Route {
    /// Configured owner/action.
    Routed(OwnerAction),
    /// No configured owner.
    Unrouted,
}
wire! {
    /// One canonical class; optional metrics never affect its summary.
    ClassAssessment { /// Canonical ID.
        id: String, /// Required IDs.
        required: BTreeSet<String>, /// Optional display-only IDs.
        optional: BTreeSet<String>, /// Metric policy assessments.
        metrics: BTreeMap<String, MetricAssessment>, /// Severity and independent completeness.
        summary: Summary, /// Oldest required source age when every age is known.
        source_age_seconds: Option<f64>, /// Configured owner/action or explicit missing route.
        route: Route }
}
/// A comparison can never infer categorical ordering.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonState {
    /// Available numeric or unequal categorical values.
    Changed,
    /// Equal categorical values.
    Same,
    /// Identity mismatch, never a numeric trend.
    QueryChanged,
    /// Missing side or unavailable evidence.
    Unavailable,
}
wire! {
    /// Shared numeric/categorical comparison, preserving both identities and values.
    Comparison { /// Comparison availability and compatibility.
        state: ComparisonState, /// Current measurement identity.
        current_identity: String, /// Reference identity, if present.
        reference_identity: Option<String>, /// Current typed observation.
        current: Option<Value>, /// Reference typed observation.
        reference: Option<Value>, /// Numeric differences by selected key/component only.
        changes: BTreeMap<String, f64>, /// Unavailable reason.
        reason: Option<String> }
}
wire! {
    /// Two persisted comparison bases for one measurement.
    MetricComparisons { /// Latest minus previous.
        since_previous: Comparison, /// Latest minus baseline.
        since_baseline: Comparison }
}
wire! {
    /// Offline department delta and current persisted report.
    Delta { /// Current persisted department envelope.
        current: Report, /// Per-metric comparisons.
        measurements: BTreeMap<String, MetricComparisons> }
}

/// Red outranks amber, which outranks unknown, which outranks green.
pub fn summarize(members: &BTreeMap<String, KeyAssessment>) -> Summary {
    let mut severity = Severity::Green;
    let mut missing = BTreeMap::new();
    for (id, member) in members {
        severity = worst(severity, member.severity);
        if member.severity == Severity::Unknown {
            missing.insert(
                id.clone(),
                member.reason.clone().unwrap_or_else(|| "unassessed".into()),
            );
        }
    }
    if members.is_empty() {
        missing.insert("required".into(), "empty required set".into());
        severity = Severity::Unknown;
    }
    Summary {
        severity,
        complete: !members.is_empty() && missing.is_empty(),
        missing,
    }
}
fn worst(a: Severity, b: Severity) -> Severity {
    let rank = |s| match s {
        Severity::Green => 0,
        Severity::Unknown => 1,
        Severity::Amber => 2,
        Severity::Red => 3,
    };
    if rank(a) >= rank(b) { a } else { b }
}
fn component(value: &Value, key: &str, statistic: Statistic) -> Option<f64> {
    match value {
        Value::Scalar(v) if statistic == Statistic::Value && key.is_empty() => Some(*v),
        Value::Keyed(rows) if statistic == Statistic::Value => rows.get(key).copied(),
        Value::Durations(rows) => rows.get(key).and_then(|row| match statistic {
            Statistic::Count => Some(row.count as f64),
            Statistic::Median => Some(row.median),
            Statistic::P90 => Some(row.p90),
            Statistic::Value => None,
        }),
        _ => None,
    }
}
fn unknown(reason: impl Into<String>) -> KeyAssessment {
    KeyAssessment {
        value: None,
        severity: Severity::Unknown,
        reason: Some(reason.into()),
    }
}
fn breach(op: Comparator, value: f64, boundary: f64) -> bool {
    match op {
        Comparator::Gt => value > boundary,
        Comparator::Gte => value >= boundary,
        Comparator::Lt => value < boundary,
        Comparator::Lte => value <= boundary,
    }
}

/// Applies one policy independently to every expected native key.
pub fn assess_metric(
    metric: Option<&Measurement>,
    policy: Option<&Policy>,
    reference: Option<&Measurement>,
) -> MetricAssessment {
    let reason = match metric {
        None => Some("missing measurement".into()),
        Some(m) if m.status != Status::Ok || m.value.is_none() => Some(
            m.reason
                .clone()
                .unwrap_or_else(|| "unavailable measurement".into()),
        ),
        Some(m) if matches!(m.value, Some(Value::SourceObservation(_))) => {
            Some("categorical evidence only".into())
        }
        _ if policy.is_none() => Some("no configured policy".into()),
        _ => None,
    };
    let mut keys = BTreeMap::new();
    let selected: BTreeSet<String> = policy
        .filter(|p| !p.expected_keys.is_empty())
        .map_or_else(|| [String::new()].into(), |p| p.expected_keys.clone());
    for key in selected {
        let result = if let Some(reason) = &reason {
            unknown(reason.clone())
        } else if let (Some(m), Some(p)) = (metric, policy) {
            let compatible = match p.basis {
                Basis::Current => true,
                _ => reference.is_some_and(|r| {
                    r.query_identity == m.query_identity && r.status == Status::Ok
                }),
            };
            let valid_age = m
                .source_age_seconds
                .is_some_and(|age| age >= 0.0 && age <= p.freshness_seconds as f64);
            if m.window_seconds != p.window_seconds {
                unknown("policy.window_seconds differs from measurement")
            } else if !valid_age {
                unknown("missing or stale source timestamp")
            } else if !compatible {
                unknown("missing or query_changed comparison reference")
            } else {
                let current = m
                    .value
                    .as_ref()
                    .and_then(|v| component(v, &key, p.statistic));
                let value = if p.basis == Basis::Current {
                    current
                } else {
                    current
                        .zip(
                            reference
                                .and_then(|r| r.value.as_ref())
                                .and_then(|v| component(v, &key, p.statistic)),
                        )
                        .map(|(a, b)| a - b)
                };
                match value.filter(|v| v.is_finite()) {
                    None => unknown(format!(
                        "missing or unavailable expected key {key:?}/selected statistic"
                    )),
                    Some(value) => KeyAssessment {
                        value: Some(value),
                        severity: if breach(p.comparator, value, p.red) {
                            Severity::Red
                        } else if breach(p.comparator, value, p.amber) {
                            Severity::Amber
                        } else {
                            Severity::Green
                        },
                        reason: None,
                    },
                }
            }
        } else {
            unknown("unassessed")
        };
        keys.insert(key, result);
    }
    let summary = summarize(&keys);
    MetricAssessment {
        policy: policy.cloned(),
        keys,
        summary,
        reason,
    }
}

/// Assesses all ten classes, with no invented thresholds or numeric health score.
pub fn assess(
    report: &mut Report,
    config: &Config,
    previous: Option<&Report>,
    baseline: Option<&Report>,
) {
    let mut classes = BTreeMap::new();
    let mut class_members = BTreeMap::new();
    for id in CLASSES {
        let empty = ClassConfig {
            id: id.into(),
            required: BTreeSet::new(),
            optional: BTreeSet::new(),
            route: None,
        };
        let class = config.classes.iter().find(|c| c.id == id).unwrap_or(&empty);
        let mut metrics = BTreeMap::new();
        for metric_id in class.required.union(&class.optional) {
            let policy = config.policies.iter().find(|p| &p.metric_id == metric_id);
            let reference = policy
                .and_then(|p| match p.basis {
                    Basis::Current => None,
                    Basis::SincePrevious => previous,
                    Basis::SinceBaseline => baseline,
                })
                .and_then(|r| r.measurements.get(metric_id));
            metrics.insert(
                metric_id.clone(),
                assess_metric(report.measurements.get(metric_id), policy, reference),
            );
        }
        let members = class
            .required
            .iter()
            .map(|id| {
                let result = &metrics[id];
                (
                    id.clone(),
                    KeyAssessment {
                        value: None,
                        severity: result.summary.severity,
                        reason: (!result.summary.complete)
                            .then(|| format!("{:?}", result.summary.missing)),
                    },
                )
            })
            .collect();
        let summary = summarize(&members);
        class_members.insert(
            id.into(),
            KeyAssessment {
                value: None,
                severity: summary.severity,
                reason: (!summary.complete).then(|| format!("{:?}", summary.missing)),
            },
        );
        let ages: Option<Vec<f64>> = class
            .required
            .iter()
            .map(|id| {
                report
                    .measurements
                    .get(id)
                    .and_then(|m| m.source_age_seconds)
            })
            .collect();
        classes.insert(
            id.into(),
            ClassAssessment {
                id: id.into(),
                required: class.required.clone(),
                optional: class.optional.clone(),
                metrics,
                summary,
                source_age_seconds: ages.and_then(|ages| ages.into_iter().max_by(f64::total_cmp)),
                route: class.route.clone().map_or(Route::Unrouted, Route::Routed),
            },
        );
    }
    report.classes = classes;
    report.rollup = summarize(&class_members);
}

/// Compares only persisted measurements. No source reader is involved.
pub fn compare(current: &Measurement, reference: Option<&Measurement>) -> Comparison {
    let mut result = Comparison {
        state: ComparisonState::Unavailable,
        current_identity: current.query_identity.clone(),
        reference_identity: reference.map(|r| r.query_identity.clone()),
        current: current.value.clone(),
        reference: reference.and_then(|r| r.value.clone()),
        changes: BTreeMap::new(),
        reason: None,
    };
    let Some(reference) = reference else {
        result.reason = Some("reference lacks department measurement".into());
        return result;
    };
    if current.query_identity != reference.query_identity {
        result.state = ComparisonState::QueryChanged;
        result.reason = Some("measurement identity changed".into());
        return result;
    }
    let (Some(a), Some(b)) = (&current.value, &reference.value) else {
        result.reason = Some("one side unavailable".into());
        return result;
    };
    if current.status != Status::Ok || reference.status != Status::Ok {
        result.reason = Some("one side unavailable".into());
        return result;
    }
    if let (Value::SourceObservation(a), Value::SourceObservation(b)) = (a, b) {
        if a.keys().ne(b.keys()) {
            result.reason = Some("missing categorical native key".into());
        } else if a.iter().any(|(key, row)| {
            b.get(key)
                .is_some_and(|old| old.source_schema_version != row.source_schema_version)
        }) {
            result.state = ComparisonState::QueryChanged;
            result.reason = Some("source schema changed".into());
        } else {
            result.state = if a
                .iter()
                .all(|(key, row)| b.get(key).is_some_and(|old| old.category == row.category))
            {
                ComparisonState::Same
            } else {
                ComparisonState::Changed
            };
        }
        return result;
    }
    fn flatten(value: &Value) -> BTreeMap<String, f64> {
        match value {
            Value::Scalar(v) => [(String::new(), *v)].into(),
            Value::Keyed(rows) => rows.clone(),
            Value::Durations(rows) => rows
                .iter()
                .flat_map(|(key, row)| {
                    [
                        (format!("{key}/count"), row.count as f64),
                        (format!("{key}/median"), row.median),
                        (format!("{key}/p90"), row.p90),
                    ]
                })
                .collect(),
            Value::SourceObservation(_) => BTreeMap::new(),
        }
    }
    let a = flatten(a);
    let b = flatten(b);
    if a.keys().ne(b.keys()) {
        result.reason = Some("keyed population changed or missing expected key".into());
        return result;
    }
    result.changes = a
        .into_iter()
        .filter_map(|(key, v)| b.get(&key).map(|old| (key, v - old)))
        .collect();
    result.state = ComparisonState::Changed;
    result
}
/// Produces both offline comparison bases using the canonical persisted envelope.
pub fn delta(current: &Report, previous: Option<&Report>, baseline: Option<&Report>) -> Delta {
    Delta {
        current: current.clone(),
        measurements: current
            .measurements
            .iter()
            .map(|(id, m)| {
                (
                    id.clone(),
                    MetricComparisons {
                        since_previous: compare(m, previous.and_then(|r| r.measurements.get(id))),
                        since_baseline: compare(m, baseline.and_then(|r| r.measurements.get(id))),
                    },
                )
            })
            .collect(),
    }
}
