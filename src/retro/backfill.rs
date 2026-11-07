// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board retro backfill`: one `backfill` record per UTC day from history (FR-028).

use serde::Serialize;

use super::RetroError;
use super::catalog::{Record, RecordKind, SCHEMA_VERSION};
use super::config::{RetroConfig, format_window};
use super::metrics::{Frame, collect_history};
use super::sources::{GithubSource, LinearSource};
use super::store::Store;

const DAY_SECS: f64 = 86_400.0;

/// What a backfill run did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BackfillReport {
    /// Records appended, oldest first.
    pub appended: Vec<Record>,
    /// `taken_at` of each day skipped because it already has a backfill record.
    pub skipped: Vec<String>,
}

/// The first second of `date` (`YYYY-MM-DD`), Unix seconds.
pub fn parse_day(date: &str) -> Result<f64, RetroError> {
    let bad = || RetroError::Date(date.to_owned());
    let epoch = crate::drift::timestamp_epoch(&format!("{date}T00:00:00Z")).map_err(|_| bad())?;
    if date.len() != 10 || !crate::drift::format_timestamp(epoch).starts_with(date) {
        return Err(bad());
    }
    Ok(epoch)
}

/// Appends a record for every UTC day in `since..=until` that has none. A day's record has
/// `taken_at` at `23:59:59Z` and measures the half-open window `[midnight - window, midnight)`
/// ending at the next UTC midnight, so consecutive days are contiguous. `until` defaults to the
/// last complete UTC day.
pub fn run_backfill(
    config: &RetroConfig,
    window_secs: u64,
    (first_day, until): (f64, Option<f64>),
    now: f64,
    (linear, github): (&dyn LinearSource, &dyn GithubSource),
    store: &Store,
) -> Result<BackfillReport, RetroError> {
    let last_day = until.unwrap_or_else(|| (now / DAY_SECS).floor() * DAY_SECS - DAY_SECS);
    for day in [first_day, last_day] {
        if day + DAY_SECS > now {
            return Err(RetroError::FutureDay(crate::drift::format_timestamp(
                day + DAY_SECS,
            )));
        }
    }
    if last_day < first_day {
        return Err(RetroError::EmptyRange);
    }
    store.check_writable()?;
    let end_of = |day: f64| day + DAY_SECS - 1.0;
    let stored = store.read_all()?;
    let in_range = |taken: &str| {
        crate::drift::timestamp_epoch(taken)
            .is_ok_and(|at| at >= first_day && at < last_day + DAY_SECS)
    };
    if let Some(snapshot) = stored
        .iter()
        .find(|record| record.kind == RecordKind::Snapshot && in_range(&record.taken_at))
    {
        return Err(RetroError::SnapshotInRange(snapshot.taken_at.clone()));
    }
    let mut pending = Vec::new();
    let mut skipped = Vec::new();
    let mut day = first_day;
    while day <= last_day {
        let taken_at = crate::drift::format_timestamp(end_of(day));
        let exists = stored.iter().any(|record| {
            record.kind == RecordKind::Backfill
                && crate::drift::timestamp_epoch(&record.taken_at)
                    .is_ok_and(|at| at >= day && at < day + DAY_SECS)
        });
        if exists {
            skipped.push(taken_at);
        } else {
            pending.push((day + DAY_SECS, taken_at));
        }
        day += DAY_SECS;
    }
    if pending.is_empty() {
        return Ok(BackfillReport {
            appended: Vec::new(),
            skipped,
        });
    }
    let frames = pending
        .iter()
        .map(|(at, _)| Frame {
            now: *at,
            window_secs,
            end_exclusive: true,
        })
        .collect::<Vec<_>>();
    let measured = collect_history(config, &frames, linear, github);
    let mut appended = Vec::new();
    for ((_, taken_at), metrics) in pending.into_iter().zip(measured) {
        let record = Record {
            schema_version: SCHEMA_VERSION,
            kind: RecordKind::Backfill,
            taken_at,
            team: config.team.clone(),
            window: format_window(window_secs),
            metrics,
        };
        store.append(&record)?;
        appended.push(record);
    }
    Ok(BackfillReport { appended, skipped })
}

/// One line per appended and per skipped day.
pub fn render_backfill_text(report: &BackfillReport) -> String {
    let mut lines = report
        .appended
        .iter()
        .map(|record| {
            let unavailable = record.unavailable_ids().len();
            format!(
                "appended backfill {} ({} metrics unavailable)",
                record.taken_at, unavailable
            )
        })
        .collect::<Vec<_>>();
    lines.extend(
        report
            .skipped
            .iter()
            .map(|taken_at| format!("skipped {taken_at}: already has a backfill record")),
    );
    lines.join("\n")
}
