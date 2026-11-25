// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board retro`: a repeatable, stored measurement of a team's convergence (US-009).
//!
//! `retro snapshot` ([FR-024]) reads the sources behind the traits in [`sources`], computes the
//! metric catalog in [`metrics`], and appends one record to the store in [`store`] ([FR-025]).
//! A metric that cannot be read is `unavailable` with a reason, never zero ([FR-026]).

pub mod backfill;
pub mod catalog;
pub mod config;
pub mod delta;
pub mod department;
pub mod eval;
pub mod live;
pub mod metrics;
mod saved;
pub mod sources;
pub mod store;

use thiserror::Error as ThisError;

pub use backfill::{BackfillReport, parse_day, render_backfill_text, run_backfill};
pub use catalog::{
    CATALOG, Direction, Metric, MetricDef, Record, RecordKind, SCHEMA_VERSION, SnapshotOutput,
    Status,
};
pub use config::{ConfigError, DEFAULT_WINDOW, RetroConfig, format_window, parse_window};
pub use delta::{DeltaReport, render_delta_text, run_delta};
pub use metrics::Frame;
#[cfg(test)]
pub(crate) use metrics::marker_fields;
pub use saved::{SavedReport, SavedReportError, decode_saved_report, read_saved_report};
pub use sources::Sources;
pub(crate) use sources::{GateLine, GateLogSource};
pub use store::{Store, StoreError};

/// Failures of a retro command.
#[derive(Debug, ThisError)]
pub enum RetroError {
    /// Configuration or window problem.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Store problem.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A snapshot output was requested for a metric outside the fixed catalog.
    #[error("metric {0:?} is not in the retro catalog")]
    UnknownMetricDefinition(String),
    /// `baseline_taken_at` matches no stored record.
    #[error("baseline_taken_at {0:?} matches no record in the store")]
    BaselineNotFound(String),
    /// A `--since` or `--until` value is not a UTC date.
    #[error("date {0:?} must be a real UTC date written YYYY-MM-DD")]
    Date(String),
    /// `--until` is before `--since`.
    #[error("the range is empty: --until is before --since")]
    EmptyRange,
    /// The last requested day has not ended.
    #[error("a requested day ends at {0}, which has not happened yet")]
    FutureDay(String),
    /// Role-spec evaluation could not read its required inputs.
    #[error("retro evaluation failed: {0}")]
    Evaluation(String),
    /// A snapshot record lies inside the requested range.
    #[error("refusing to backfill: the range contains the snapshot record taken at {0}")]
    SnapshotInRange(String),
}

/// Computes the catalog at `now` and appends one `snapshot` record to the store.
///
/// The store path is checked before any source is read, so a refused path costs no reads and
/// creates nothing.
pub fn run_snapshot(
    config: &RetroConfig,
    window_secs: u64,
    now: f64,
    sources: &Sources<'_>,
    store: &Store,
) -> Result<Record, RetroError> {
    config.validate_department()?;
    store.check_writable()?;
    let stored = store.read_all()?;
    let frame = Frame {
        now,
        window_secs,
        end_exclusive: false,
    };
    let previous = stored
        .iter()
        .rev()
        .find(|r| r.kind == RecordKind::Snapshot)
        .and_then(|r| r.department.as_ref());
    let baseline = config
        .baseline_taken_at
        .as_ref()
        .and_then(|at| stored.iter().find(|r| &r.taken_at == at))
        .or_else(|| stored.first())
        .and_then(|r| r.department.as_ref());
    let record = collect_snapshot(config, frame, sources, previous, baseline)?;
    store.append(&record)?;
    Ok(record)
}

/// Collects the same snapshot as the writer without reading or writing a store.
/// Optional comparison envelopes must already be loaded by the caller.
pub fn collect_snapshot(
    config: &RetroConfig,
    frame: Frame,
    sources: &Sources<'_>,
    previous: Option<&department::Report>,
    baseline: Option<&department::Report>,
) -> Result<Record, RetroError> {
    config.validate_department()?;
    let window_secs = frame.window_secs;
    let now = frame.now;
    let metrics = metrics::collect(config, frame, sources);
    let department = config.department.as_ref().map(|department| {
        let mut batches = vec![
            sources
                .linear
                .department_evidence(&department.scope, frame.now - window_secs as f64),
            sources
                .github
                .department_evidence(department, frame.now - window_secs as f64),
        ];
        department::enrich_merged_attachments(department, &mut batches, sources.github);
        let repositories: Vec<_> = department
            .scope
            .repositories
            .iter()
            .map(|s| s.id.clone())
            .collect();
        let allowed = batches
            .iter()
            .flat_map(|b| &b.observations)
            .filter_map(|o| match &o.fact {
                department::Fact::PullRequest(_) => {
                    o.entity_id.rsplit_once('/').map(|(_, key)| key.to_owned())
                }
                _ => None,
            })
            .collect();
        if let Ok(lines) = metrics::marker_lines_for(sources, &allowed) {
            department::enrich_reviewer_rounds(&mut batches, &lines, &repositories);
        }
        department::read_source_logs(department, frame, &mut batches, |path| {
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
        });
        if let Some(operations) = &department.operations {
            batches.extend(department::read_operational_files(
                operations, department, frame,
            ));
        }
        let mut report = department::collect(department, frame, &batches, &metrics);
        department::assess(&mut report, department, previous, baseline);
        report
    });
    let record = Record {
        schema_version: if department.is_some() {
            2
        } else {
            SCHEMA_VERSION
        },
        kind: RecordKind::Snapshot,
        taken_at: crate::drift::format_timestamp(now.floor()),
        team: config.team.clone(),
        window: format_window(window_secs),
        metrics,
        department,
    };
    Ok(record)
}

/// One line per metric: id, status, value and unit, or the reason it is unavailable.
pub fn render_text(record: &Record) -> String {
    let mut text = CATALOG
        .iter()
        .filter_map(|entry| record.metrics.get(entry.id))
        .map(|metric| match (metric.value, &metric.reason) {
            (Some(value), _) => format!("{} ok {value} {}", metric.id, metric.unit),
            (None, reason) => format!(
                "{} unavailable {}",
                metric.id,
                reason.as_deref().unwrap_or("no reason recorded")
            ),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(report) = &record.department {
        for (id, class) in &report.classes {
            text.push_str(&format!(
                "\n{id}: {:?}; complete={}; route={:?}; missing={:?}",
                class.summary.severity, class.summary.complete, class.route, class.summary.missing
            ));
        }
    }
    text
}

/// The stderr notice naming the unavailable metric ids, or `None` when every metric was read.
pub fn unavailable_notice(record: &Record) -> Option<String> {
    let ids = record.unavailable_ids();
    (!ids.is_empty()).then(|| format!("unavailable metrics: {}", ids.join(", ")))
}
