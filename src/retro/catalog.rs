// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The fixed metric catalog and the stored record types (FR-024, FR-025, FR-026).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Record schema version this build writes and understands.
pub const SCHEMA_VERSION: u32 = 1;

/// Which way a metric should move.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// A larger value is better.
    Up,
    /// A smaller value is better.
    Down,
    /// Neither direction is better.
    Neutral,
}

/// Whether a metric value was read.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The value was computed from a complete source.
    Ok,
    /// The source was unreadable, incomplete or not configured.
    Unavailable,
}

/// One catalog entry: a stable id with its family, unit and good direction.
#[derive(Clone, Copy, Debug)]
pub struct MetricDef {
    /// Stable metric id.
    pub id: &'static str,
    /// Catalog family.
    pub family: &'static str,
    /// Unit of the value.
    pub unit: &'static str,
    /// Good direction.
    pub direction: Direction,
}

const fn def(
    id: &'static str,
    family: &'static str,
    unit: &'static str,
    direction: Direction,
) -> MetricDef {
    MetricDef {
        id,
        family,
        unit,
        direction,
    }
}

use Direction::{Down, Neutral, Up};

/// Every metric id of the catalog, in report order.
pub const CATALOG: &[MetricDef] = &[
    def("path_remaining", "progress", "issues", Down),
    def("path_closed", "progress", "issues", Up),
    def("intake_new", "progress", "issues", Neutral),
    def("net_closed_minus_new", "progress", "issues", Up),
    def("gates_remaining", "progress", "issues", Down),
    def("path_closed_unverified", "progress", "issues", Down),
    def("prs_merged_per_hour", "flow", "pull_requests_per_hour", Up),
    def("pr_cycle_median_hours", "flow", "hours", Down),
    def("pr_cycle_p90_hours", "flow", "hours", Down),
    def("prs_open", "flow", "pull_requests", Neutral),
    def("prs_idle", "flow", "pull_requests", Down),
    def("prs_oversize", "flow", "pull_requests", Down),
    def("prs_ready_unmerged", "flow", "pull_requests", Down),
    def("fix_rounds_max", "rework", "rounds", Down),
    def("fix_rounds_total", "rework", "rounds", Down),
    def("head_shas_per_pr_max", "rework", "commits", Down),
    def("done_reopened", "rework", "issues", Down),
    def("main_red_incidents", "stability", "incidents", Down),
    def("main_red_minutes", "stability", "minutes", Down),
    def("issues_without_project", "scope", "issues", Down),
    def("issues_without_milestone", "scope", "issues", Down),
    def("in_progress_stale", "scope", "issues", Down),
    def("source_count_disagreement", "scope", "issues", Down),
    def("agents_working_share", "utilization", "ratio", Up),
    def("agents_idle_minutes_max", "utilization", "minutes", Down),
    def("worktrees_prunable", "resource", "worktrees", Down),
    def("disk_bytes", "resource", "bytes", Down),
    def("orphan_slots", "resource", "processes", Down),
    def("full_gates_max", "rework", "gates", Down),
    def("repins_total", "rework", "repins", Down),
    def("build_lock_wait_minutes", "resource", "minutes", Down),
    def("messages_undelivered", "utilization", "messages", Down),
    def("decisions_open", "scope", "issues", Down),
    def("decision_oldest_hours", "scope", "hours", Down),
];

/// Looks up a catalog entry by id.
pub fn definition(id: &str) -> Option<&'static MetricDef> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// One metric as stored in a record (FR-026).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Metric {
    /// Stable metric id.
    pub id: String,
    /// `ok` or `unavailable`.
    pub status: Status,
    /// The number, or null when unavailable.
    pub value: Option<f64>,
    /// Unit of the value.
    pub unit: String,
    /// Good direction.
    pub direction: Direction,
    /// Source the value was read from.
    pub source: String,
    /// Counting rule with the window and thresholds applied.
    pub query: String,
    /// Failed or missing source, present when unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What kind of command wrote a record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordKind {
    /// Written by `retro snapshot`.
    Snapshot,
    /// Written by `retro backfill`.
    Backfill,
}

/// One line of the store (FR-025).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Record {
    /// Record schema version.
    pub schema_version: u32,
    /// Writing command kind.
    pub kind: RecordKind,
    /// UTC time the record was taken.
    pub taken_at: String,
    /// Team the record belongs to.
    pub team: String,
    /// Window the window metrics were measured over.
    pub window: String,
    /// Metrics keyed by id.
    pub metrics: BTreeMap<String, Metric>,
}

impl Record {
    /// Ids of the metrics that are unavailable, in catalog order.
    pub fn unavailable_ids(&self) -> Vec<&str> {
        CATALOG
            .iter()
            .filter(|entry| {
                self.metrics
                    .get(entry.id)
                    .is_some_and(|metric| metric.status == Status::Unavailable)
            })
            .map(|entry| entry.id)
            .collect()
    }
}
