// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board retro delta`: compares the latest snapshot with the previous one and the baseline,
//! reading only the store (FR-027).

use serde::Serialize;

use super::RetroError;
use super::catalog::{CATALOG, Direction, Metric, Record, RecordKind, Status};
use super::config::RetroConfig;
use super::store::Store;

/// How one metric compares with one reference record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
    /// Both sides are `ok` with equal queries; `change` holds the difference.
    Changed,
    /// Either side is `unavailable`.
    Unavailable,
    /// Both sides are `ok` but their queries differ.
    QueryChanged,
}

/// The queries of the two sides of a `query_changed` comparison.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Queries {
    /// Query of the latest record.
    pub latest: String,
    /// Query of the reference record.
    pub reference: String,
}

/// The comparison of the latest value with one reference record.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Comparison {
    /// `taken_at` of the reference record.
    pub reference_taken_at: String,
    /// Outcome of the comparison.
    pub state: ChangeState,
    /// Latest minus reference; present only when `state` is `changed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<f64>,
    /// Both queries; present only when `state` is `query_changed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queries: Option<Queries>,
}

/// One metric of the latest record with its changes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MetricDelta {
    /// Metric id.
    pub id: String,
    /// Unit of the value.
    pub unit: String,
    /// Good direction.
    pub direction: Direction,
    /// `ok` or `unavailable` in the latest record.
    pub status: Status,
    /// Latest value, null when unavailable.
    pub value: Option<f64>,
    /// Reason, present when the latest value is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Comparison with the previous snapshot.
    pub since_previous: Comparison,
    /// Comparison with the baseline.
    pub since_baseline: Comparison,
    /// True when the change moved against the direction in each of the latest two comparisons.
    pub regressing: bool,
}

/// The delta report. There is deliberately no combined score.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeltaReport {
    /// Team of the store.
    pub team: String,
    /// Set when no delta exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// `taken_at` of the latest snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// `taken_at` of the previous snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    /// `taken_at` of the baseline record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<String>,
    /// Kind of the baseline record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_kind: Option<RecordKind>,
    /// One entry per metric of the latest record, in catalog order.
    pub metrics: Vec<MetricDelta>,
}

/// Compares the stored records of the configured team. Reads no source.
pub fn run_delta(config: &RetroConfig, store: &Store) -> Result<DeltaReport, RetroError> {
    let records = store.read_all()?;
    let baseline = match &config.baseline_taken_at {
        Some(wanted) => {
            let wanted_epoch = epoch(wanted);
            Some(
                records
                    .iter()
                    .find(|record| epoch(&record.taken_at) == wanted_epoch)
                    .ok_or_else(|| RetroError::BaselineNotFound(wanted.clone()))?,
            )
        }
        None => records.first(),
    };
    let snapshots = records
        .iter()
        .filter(|record| record.kind == RecordKind::Snapshot)
        .collect::<Vec<_>>();
    let [.., previous, latest] = snapshots.as_slice() else {
        return Ok(DeltaReport {
            team: config.team.clone(),
            message: Some(format!(
                "no delta: {} snapshot record(s) in the store, two are needed",
                snapshots.len()
            )),
            latest: None,
            previous: None,
            baseline: baseline.map(|record| record.taken_at.clone()),
            baseline_kind: baseline.map(|record| record.kind),
            metrics: Vec::new(),
        });
    };
    let baseline = baseline.unwrap_or(latest);
    // The snapshot before the previous one, when the two latest comparisons both exist.
    let before = snapshots.len().checked_sub(3).map(|index| snapshots[index]);
    let metrics = CATALOG
        .iter()
        .filter_map(|entry| latest.metrics.get(entry.id))
        .map(|metric| {
            let since_previous = compare(metric, previous);
            let since_baseline = compare(metric, baseline);
            let regressing = metric.direction != Direction::Neutral
                && moved_against(metric.direction, &since_previous)
                && before.is_some_and(|older| {
                    previous
                        .metrics
                        .get(&metric.id)
                        .is_some_and(|mid| moved_against(metric.direction, &compare(mid, older)))
                });
            MetricDelta {
                id: metric.id.clone(),
                unit: metric.unit.clone(),
                direction: metric.direction,
                status: metric.status,
                value: metric.value,
                reason: metric.reason.clone(),
                since_previous,
                since_baseline,
                regressing,
            }
        })
        .collect();
    Ok(DeltaReport {
        team: config.team.clone(),
        message: None,
        latest: Some(latest.taken_at.clone()),
        previous: Some(previous.taken_at.clone()),
        baseline: Some(baseline.taken_at.clone()),
        baseline_kind: Some(baseline.kind),
        metrics,
    })
}

/// The instant a stored or configured UTC time names, with its fractional seconds.
fn epoch(text: &str) -> Option<f64> {
    crate::drift::timestamp_epoch(text).ok()
}

/// `metric` against the same metric of `reference`.
fn compare(metric: &Metric, reference: &Record) -> Comparison {
    let reference_taken_at = reference.taken_at.clone();
    let other = reference.metrics.get(&metric.id);
    let readable = other.filter(|other| {
        metric.status == Status::Ok
            && other.status == Status::Ok
            && metric.value.is_some()
            && other.value.is_some()
    });
    let Some(other) = readable else {
        return Comparison {
            reference_taken_at,
            state: ChangeState::Unavailable,
            change: None,
            queries: None,
        };
    };
    if other.query != metric.query {
        return Comparison {
            reference_taken_at,
            state: ChangeState::QueryChanged,
            change: None,
            queries: Some(Queries {
                latest: metric.query.clone(),
                reference: other.query.clone(),
            }),
        };
    }
    Comparison {
        reference_taken_at,
        state: ChangeState::Changed,
        change: metric.value.zip(other.value).map(|(now, then)| now - then),
        queries: None,
    }
}

fn moved_against(direction: Direction, comparison: &Comparison) -> bool {
    match (direction, comparison.change) {
        (Direction::Down, Some(change)) => change > 0.0,
        (Direction::Up, Some(change)) => change < 0.0,
        _ => false,
    }
}

fn describe(comparison: &Comparison) -> String {
    match (comparison.state, comparison.change, &comparison.queries) {
        (ChangeState::Changed, Some(change), _) => format!("{change:+}"),
        (ChangeState::QueryChanged, _, Some(queries)) => format!(
            "query_changed (latest: {}; then: {})",
            queries.latest, queries.reference
        ),
        _ => "unavailable".to_owned(),
    }
}

/// One line per metric, or the no-delta message.
pub fn render_delta_text(report: &DeltaReport) -> String {
    if let Some(message) = &report.message {
        return message.clone();
    }
    let mut lines = vec![format!(
        "latest {}; previous {}; baseline {} ({})",
        report.latest.as_deref().unwrap_or("-"),
        report.previous.as_deref().unwrap_or("-"),
        report.baseline.as_deref().unwrap_or("-"),
        report.baseline_kind.map_or("-", |kind| match kind {
            RecordKind::Snapshot => "snapshot",
            RecordKind::Backfill => "backfill",
        }),
    )];
    lines.extend(report.metrics.iter().map(|metric| {
        let value = match (metric.value, &metric.reason) {
            (Some(value), _) => format!("{value} {}", metric.unit),
            (None, reason) => format!(
                "unavailable ({})",
                reason.as_deref().unwrap_or("no reason recorded")
            ),
        };
        format!(
            "{}: {value}; since previous {}; since baseline {}{}",
            metric.id,
            describe(&metric.since_previous),
            describe(&metric.since_baseline),
            if metric.regressing {
                "; regressing"
            } else {
                ""
            }
        )
    }));
    lines.join("\n")
}
