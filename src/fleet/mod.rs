// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `fleet status`: one class per registered agent lane from fused structured evidence (FR-034).
//!
//! The classifier is pure over [`LaneInput`]. The report builder reads the relay snapshot, the
//! per-host Prometheus series and the order receipts through the traits below, so every source
//! can be faked. Time is the relay server clock: no host-local timestamp enters an age.

pub mod assign;
pub mod capacity;
mod classify;
mod config;
pub mod focus;
pub mod ledger;
pub mod live;
mod render;
mod report;
pub mod tick;

pub use classify::{
    Class, ClassKind, Classification, Evidence, HookEvent, HookKind, LaneInput, Thresholds,
    WeakSource, classify,
};
pub use config::{
    FleetConfig, RELAY_DEFAULT_MAX_TOOL_RUNTIME, RELAY_DEFAULT_NO_EVENTS_AFTER,
    RELAY_DEFAULT_WORKING_WINDOW,
};
pub use render::render_text;
pub use report::{
    Coverage, Defect, DefectKind, ExpiredOrder, FleetReport, HostSeries, HostSummary, LaneRow,
    OrderReceipts, ProbeReading, RawEvent, ReceiptRead, RegInfo, RelayConfig, RelaySnapshot,
    SCHEMA_VERSION, SeriesSource, Sources, UnresolvedOrder, WeakSignals, build_report, parse_event,
};

use thiserror::Error as ThisError;

/// A fleet-status configuration error.
#[derive(Debug, ThisError)]
pub enum FleetError {
    /// The `[fleet]` configuration was refused.
    #[error("invalid fleet configuration: {0}")]
    Config(String),
    /// The ledger could not be read.
    #[error("fleet ledger: {0}")]
    Ledger(String),
    /// The tick could not evaluate the fleet and committed nothing.
    #[error("fleet tick could not evaluate: {0}")]
    Tick(String),
}

/// A source that could not be read; the text is the defect reason.
#[derive(Clone, Debug, Eq, PartialEq, ThisError)]
#[error("{0}")]
pub struct SourceError(pub String);
