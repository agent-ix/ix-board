// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Bounded operational-team evidence and cached Prometheus export.
mod collect;
mod config;
mod history;
mod journal;
mod pr_stats;
mod quota_frontier;
mod render;
mod reviews;
mod timeline;
pub(crate) use relay_work_wire as wire;

pub use collect::Collector;
pub use config::{Config, ScopeConfig, Selectors};
pub use history::{History, read_history};
pub use journal::Journal;
pub use render::render;
use thiserror::Error as ThisError;
pub use timeline::{ScopeState, State};
pub use wire::{Assignment, Binding, Change, Event, EventPage};

/// Typed exporter boundary failures; callers never branch on diagnostic prose.
#[derive(Debug, ThisError)]
pub enum Error {
    /// Invalid or unsupported configuration/evidence.
    #[error("invalid team metrics input: {0}")]
    Invalid(&'static str),
    /// A filesystem or socket operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A typed payload failed to decode.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The bounded evidence store failed.
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    /// A source command failed, timed out or exceeded its output bound.
    #[error("team metrics source failed: {0}")]
    Source(String),
}

/// Wall clock in checked Unix milliseconds.
pub fn now_ms() -> Result<i64, Error> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Invalid("clock predates Unix epoch"))?;
    i64::try_from(elapsed.as_millis()).map_err(|_| Error::Invalid("clock overflow"))
}

#[cfg(test)]
mod tests;
