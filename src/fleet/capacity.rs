// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Pluggable measurements used by capacity-discovery windows (FR-041).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error as ThisError;

/// Whether a source supplied usable coverage for a capacity field.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// The source returned a value with sufficient coverage.
    Available,
    /// The source or window did not provide enough evidence.
    Unavailable,
}

/// A measured value and its auditable source basis.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Observation<T> {
    /// The value, null when coverage is unavailable.
    pub value: Option<T>,
    /// Whether the value can be used.
    pub availability: Availability,
    /// Stable source name and query basis, for example `node_exporter:node_memory_MemAvailable_bytes`.
    pub source: String,
    /// Why a value is unavailable; present for every unavailable field.
    pub reason: Option<String>,
    /// Fraction of the requested window covered by usable samples.
    pub coverage: Option<f64>,
    /// Source observation time in relay milliseconds.
    pub observed_at_ms: Option<i64>,
}

impl<T> Observation<T> {
    /// Records a source value and its observation time.
    pub fn available(value: T, source: impl Into<String>, observed_at_ms: i64) -> Self {
        Self {
            value: Some(value),
            availability: Availability::Available,
            source: source.into(),
            reason: None,
            coverage: Some(1.0),
            observed_at_ms: Some(observed_at_ms),
        }
    }

    /// Records a missing value without substituting zero.
    pub fn unavailable(
        source: impl Into<String>,
        reason: impl Into<String>,
        coverage: Option<f64>,
    ) -> Self {
        Self {
            value: None,
            availability: Availability::Unavailable,
            source: source.into(),
            reason: Some(reason.into()),
            coverage,
            observed_at_ms: None,
        }
    }
}

/// Provenance retained beside one capacity measurement in the ledger and JSON output.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FieldBasis {
    /// Whether this field has usable coverage.
    pub availability: Availability,
    /// Source name and metric/query basis.
    pub source: String,
    /// Why this field is unavailable.
    pub reason: Option<String>,
    /// Fraction of the requested window covered by usable samples.
    pub coverage: Option<f64>,
    /// Source observation time in relay milliseconds.
    pub observed_at_ms: Option<i64>,
}

impl<T> From<&Observation<T>> for FieldBasis {
    fn from(observation: &Observation<T>) -> Self {
        Self {
            availability: observation.availability,
            source: observation.source.clone(),
            reason: observation.reason.clone(),
            coverage: observation.coverage,
            observed_at_ms: observation.observed_at_ms,
        }
    }
}

/// Measurements collected for one host hold window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WindowMeasurements {
    /// Concurrent non-draining lanes during the window.
    pub lanes: Observation<u32>,
    /// Build slots occupied during the window.
    pub build_slots_busy: Observation<u32>,
    /// Build slots sustainable for this host.
    pub build_slots_sustainable: Observation<u32>,
    /// Window start in relay milliseconds.
    pub window_start_ms: i64,
    /// Window end in relay milliseconds.
    pub window_end_ms: i64,
    /// End of ramp-up in relay milliseconds.
    pub rampup_end_ms: i64,
    /// Closed tickets attributed to this host and window.
    pub tickets_closed: Observation<u64>,
    /// Merged pull requests attributed to this host and window.
    pub prs_merged: Observation<u64>,
    /// Commits pushed attributed to this host and window.
    pub commits_pushed: Observation<u64>,
    /// Output per hour on the selected output basis.
    pub host_output_per_hour: Observation<f64>,
    /// Which output basis was used.
    pub output_basis: Observation<String>,
    /// 90th percentile build queue wait, seconds.
    pub queue_wait_p90_s: Observation<f64>,
    /// Throttle share over the window, in [0, 1].
    pub throttled_share: Observation<f64>,
    /// Minimum memory headroom over the window, in bytes or profile units documented by source.
    pub memory_headroom_min: Observation<f64>,
    /// Minimum thermal headroom over the window, in degrees Celsius.
    pub thermal_headroom_min: Observation<f64>,
    /// Whether build slots stayed at the sustainable count for the whole window.
    pub slots_busy_equals_sustainable: Observation<bool>,
    /// Source basis for values computed from multiple observations.
    pub derived_sources: BTreeMap<String, String>,
    /// Per-lane output used to attribute host throughput.
    pub lane_output: Vec<LaneOutputMeasurement>,
}

/// Per-lane contribution to a capacity window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaneOutputMeasurement {
    /// Lane address.
    pub lane: String,
    /// Tickets closed during the window.
    pub tickets_closed: Observation<u64>,
    /// Pull requests merged during the window.
    pub prs_merged: Observation<u64>,
    /// Commits pushed during the window.
    pub commits_pushed: Observation<u64>,
}

/// Input passed to a capacity measurement source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowRequest {
    /// Host being measured.
    pub host: String,
    /// Lane count for the hold.
    pub lanes: u32,
    /// Start of the measured window in relay milliseconds.
    pub start_ms: i64,
    /// End of the measured window in relay milliseconds.
    pub end_ms: i64,
    /// End of lane warm-up in relay milliseconds.
    pub rampup_end_ms: i64,
}

/// A source plugin for one capacity measurement window.
pub trait MeasurementSource: Send + Sync {
    /// Collects every measurement needed to record a window, with source basis attached.
    ///
    /// # Errors
    ///
    /// A required source could not supply a measurement for the requested window.
    fn measure(&self, request: &WindowRequest) -> Result<WindowMeasurements, CapacityError>;
}

/// A refused, incomplete or unreadable capacity measurement.
#[derive(Clone, Debug, Eq, PartialEq, ThisError)]
pub enum CapacityError {
    /// A source could not supply a required field.
    #[error("capacity measurement source {source_name} could not read {field}: {reason}")]
    Source {
        /// Source name.
        source_name: String,
        /// Field name.
        field: String,
        /// Cause.
        reason: String,
    },
    /// The source returned a malformed measurement.
    #[error("invalid capacity measurement: {0}")]
    Invalid(String),
}
