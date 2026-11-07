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
pub mod live;
pub mod metrics;
pub mod sources;
pub mod store;

use thiserror::Error as ThisError;

pub use backfill::{BackfillReport, parse_day, render_backfill_text, run_backfill};
pub use catalog::{
    CATALOG, Direction, Metric, MetricDef, Record, RecordKind, SCHEMA_VERSION, Status,
};
pub use config::{ConfigError, DEFAULT_WINDOW, RetroConfig, format_window, parse_window};
pub use delta::{DeltaReport, render_delta_text, run_delta};
pub use metrics::Frame;
pub use sources::Sources;
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
    store.check_writable()?;
    let frame = Frame {
        now,
        window_secs,
        end_exclusive: false,
    };
    let record = Record {
        schema_version: SCHEMA_VERSION,
        kind: RecordKind::Snapshot,
        taken_at: crate::drift::format_timestamp(now.floor()),
        team: config.team.clone(),
        window: format_window(window_secs),
        metrics: metrics::collect(config, frame, sources),
    };
    store.append(&record)?;
    Ok(record)
}

/// One line per metric: id, status, value and unit, or the reason it is unavailable.
pub fn render_text(record: &Record) -> String {
    CATALOG
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
        .join("\n")
}

/// The stderr notice naming the unavailable metric ids, or `None` when every metric was read.
pub fn unavailable_notice(record: &Record) -> Option<String> {
    let ids = record.unavailable_ids();
    (!ids.is_empty()).then(|| format!("unavailable metrics: {}", ids.join(", ")))
}
