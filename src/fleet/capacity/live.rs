// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Production Prometheus, relay and Linear measurement source for FR-041.

use super::{
    Availability, CapacityError, LaneOutputMeasurement, MeasurementSource, Observation,
    WindowMeasurements, WindowRequest,
};
use crate::fleet::{
    FleetConfig,
    live::{PromHttp, PromQuery},
};
use crate::{LinearGraphQlSource, ReadService};
use serde_json::Value;
use std::collections::BTreeMap;

/// Real host measurements from Prometheus and ix-relay, with Linear issue/PR output.
pub struct LiveMeasurementSource<'a> {
    /// Explicit ix-board thresholds and interim slot limits.
    pub config: &'a FleetConfig,
    /// Bounded Prometheus client.
    pub prometheus: Option<PromHttp>,
}

fn query_value(query: &PromHttp, expression: &str) -> Result<Option<f64>, String> {
    let body = query.query(expression).map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    if value["status"] != "success" {
        return Err("Prometheus query did not succeed".to_owned());
    }
    let result = value["data"]["result"]
        .as_array()
        .ok_or_else(|| "Prometheus response has no result vector".to_owned())?;
    if result.len() > 1 {
        return Err("Prometheus query returned multiple host values".to_owned());
    }
    result.first().map_or(Ok(None), |sample| {
        sample["value"][1]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or_else(|| "Prometheus value is not finite numeric data".to_owned())
    })
}

fn esc(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unavailable<T>(source: impl Into<String>, reason: impl Into<String>) -> Observation<T> {
    Observation::unavailable(source, reason, None)
}

fn observed<T>(value: Option<T>, source: &str, reason: &str, at: i64) -> Observation<T> {
    value.map_or_else(
        || unavailable(source, reason),
        |value| Observation::available(value, source, at),
    )
}

fn duration(window: &WindowRequest) -> Result<String, CapacityError> {
    let seconds = window.end_ms.saturating_sub(window.start_ms) / 1000;
    if seconds < 60 {
        return Err(CapacityError::Invalid(
            "capacity measurement window must be at least one minute".to_owned(),
        ));
    }
    Ok(format!("{seconds}s"))
}

fn temp_query(host: &str, selector: &str, span: &str) -> String {
    match host.to_lowercase().as_str() {
        "titan" => format!(
            "max_over_time(node_hwmon_temp_celsius{{host=\"{}\",sensor=\"Tctl\"}}[{span}])",
            esc(host)
        ),
        "luna" => format!(
            "max(max_over_time(node_hwmon_temp_celsius{{host=\"{}\"}}[{span}])) or max(max_over_time(node_thermal_zone_temp{{host=\"{}\"}}[{span}]))",
            esc(host),
            esc(host)
        ),
        "terra" => format!(
            "max_over_time(windows_sensors_physical_cpu_temperature_celsius{{host=\"{}\"}}[{span}])",
            esc(host)
        ),
        "draco" => format!(
            "max_over_time(node_thermal_temperature_celsius{{host=\"{}\"}}[{span}])",
            esc(host)
        ),
        _ => format!(
            "max_over_time({selector}{{host=\"{}\"}}[{span}])",
            esc(host)
        ),
    }
}

fn source_observation(
    query: Option<&PromHttp>,
    host: &str,
    expression: String,
    source: &str,
    at: i64,
) -> Observation<f64> {
    let Some(query) = query else {
        return unavailable(source, "prometheus_url_unconfigured");
    };
    match query_value(query, &expression) {
        Ok(value) => observed(value, source, "series_missing", at),
        Err(error) => unavailable(source, format!("{host}: {error}")),
    }
}

fn linear_output(
    request: &WindowRequest,
) -> (
    Observation<u64>,
    Observation<u64>,
    Vec<LaneOutputMeasurement>,
) {
    let fail = |source: &str, reason: String| unavailable(source, reason);
    let source = match LinearGraphQlSource::from_env() {
        Ok(source) => source,
        Err(error) => {
            return (
                fail("linear.issue_completed_at", error.to_string()),
                fail("linear.pull_request.merged_at", error.to_string()),
                Vec::new(),
            );
        }
    };
    let mut service = match ReadService::open_default(source) {
        Ok(service) => service,
        Err(error) => {
            return (
                fail("linear.issue_completed_at", error.to_string()),
                fail("linear.pull_request.merged_at", error.to_string()),
                Vec::new(),
            );
        }
    };
    let snapshot = match service.workspace_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return (
                fail("linear.issue_completed_at", error.to_string()),
                fail("linear.pull_request.merged_at", error.to_string()),
                Vec::new(),
            );
        }
    };
    let start = request.start_ms / 1000;
    let end = request.end_ms / 1000;
    let completed = snapshot
        .issues
        .iter()
        .filter(|issue| {
            issue.completed_at.as_deref().is_some_and(|time| {
                chrono::DateTime::parse_from_rfc3339(time)
                    .ok()
                    .is_some_and(|parsed| (start..=end).contains(&parsed.timestamp()))
            })
        })
        .count() as u64;
    let merged = snapshot
        .issues
        .iter()
        .flat_map(|issue| issue.pull_requests.iter())
        .filter(|pr| {
            pr.status == crate::PullRequestStatus::Merged
                && pr.merged_at.as_deref().is_some_and(|time| {
                    chrono::DateTime::parse_from_rfc3339(time)
                        .ok()
                        .is_some_and(|parsed| (start..=end).contains(&parsed.timestamp()))
                })
        })
        .count() as u64;
    let at = request.end_ms;
    (
        Observation::available(completed, "linear.issue.completed_at", at),
        Observation::available(merged, "linear.pull_request.merged_at", at),
        Vec::new(),
    )
}

impl MeasurementSource for LiveMeasurementSource<'_> {
    fn measure(&self, request: &WindowRequest) -> Result<WindowMeasurements, CapacityError> {
        if request.end_ms <= request.start_ms || request.rampup_end_ms > request.start_ms {
            return Err(CapacityError::Invalid(
                "measurement interval is empty or precedes ramp-up completion".to_owned(),
            ));
        }
        let span = duration(request)?;
        let host = request.host.to_lowercase();
        let label = esc(&host);
        let at = request.end_ms;
        let limits = self.config.capacity_limits.get(&host);
        let configured_slots = limits.and_then(|limit| limit.build_slots);
        let sustainable = observed(
            configured_slots,
            &format!("ix_board_config:fleet.capacity_limits.{host}.build_slots"),
            "build_slots_not_configured",
            at,
        );
        let source = |metric: &str, operation: &str| {
            format!("{operation}({metric}{{host=\"{label}\"}}[{span}])")
        };
        let busy = source_observation(
            self.prometheus.as_ref(),
            &host,
            source("ix_build_slots_busy", "max_over_time"),
            "prometheus.ix_build_slots_busy",
            at,
        );
        let queue_wait = source_observation(
            self.prometheus.as_ref(),
            &host,
            format!(
                "quantile_over_time(0.9, ix_build_queue_wait_seconds{{host=\"{label}\"}}[{span}])"
            ),
            "prometheus.ix_build_queue_wait_seconds",
            at,
        );
        let admission = source_observation(
            self.prometheus.as_ref(),
            &host,
            format!(
                "increase(ix_build_admission_blocked_seconds_total{{host=\"{label}\"}}[{span}])"
            ),
            "prometheus.ix_build_admission_blocked_seconds_total",
            at,
        );
        let throttled_share = match (
            admission.value,
            request.end_ms.checked_sub(request.start_ms),
        ) {
            (Some(blocked), Some(milliseconds)) if milliseconds > 0 => Observation::available(
                (blocked / (milliseconds as f64 / 1000.0)).clamp(0.0, 1.0),
                "admission_only:ix_build_admission_blocked_seconds_total",
                at,
            ),
            _ => unavailable(
                "admission_only:ix_build_admission_blocked_seconds_total",
                "admission_blocked_source_unavailable",
            ),
        };
        let memory = source_observation(
            self.prometheus.as_ref(),
            &host,
            format!("min_over_time(node_memory_MemAvailable_bytes{{host=\"{label}\"}}[{span}])"),
            "node_exporter.node_memory_MemAvailable_bytes",
            at,
        );
        let memory = if memory.value.is_some() {
            memory
        } else {
            source_observation(
                self.prometheus.as_ref(),
                &host,
                format!(
                    "min_over_time((ix_fleet_host_memory_total_bytes{{host=\"{label}\"}} - ix_fleet_host_memory_used_bytes{{host=\"{label}\"}})[{span}:])"
                ),
                "relay_watcher.ix_fleet_host_memory_total_bytes-ix_fleet_host_memory_used_bytes",
                at,
            )
        };
        let temperature = source_observation(
            self.prometheus.as_ref(),
            &host,
            temp_query(&host, "node_thermal_temperature_celsius", &span),
            match host.as_str() {
                "draco" => "node_exporter.node_thermal_temperature_celsius",
                "titan" => "node_exporter.node_hwmon_temp_celsius:Tctl",
                "luna" => "node_exporter.hwmon_or_thermal_zone",
                "terra" => "windows_sensors.physical_cpu",
                _ => "node_exporter.host_temperature",
            },
            at,
        );
        let memory_headroom = limits
            .and_then(|limit| limit.memory_reserve_bytes)
            .zip(memory.value)
            .map(|(reserve, available)| available - reserve as f64);
        let thermal_headroom = limits
            .and_then(|limit| limit.thermal_limit_celsius)
            .zip(temperature.value)
            .map(|(limit, current)| limit - current);
        let (tickets_closed, prs_merged, lane_output) = linear_output(request);
        let total_output = tickets_closed
            .value
            .zip(prs_merged.value)
            .map(|(tickets, prs)| tickets.saturating_add(prs));
        let (host_output_per_hour, output_basis) = match total_output {
            Some(output) if output >= 5 => {
                let hours = (request.end_ms - request.start_ms) as f64 / 3_600_000.0;
                (
                    Observation::available(
                        output as f64 / hours,
                        "linear.tickets_plus_merged_prs",
                        at,
                    ),
                    Observation::available(
                        "tickets_prs".to_owned(),
                        "ix-board.FR-041.output_basis",
                        at,
                    ),
                )
            }
            _ => (
                unavailable(
                    "github.commits",
                    "fewer_than_five_tickets_and_prs; commit_reader_unavailable",
                ),
                unavailable(
                    "github.commits",
                    "fewer_than_five_tickets_and_prs; commit_reader_unavailable",
                ),
            ),
        };
        let slot_equal = match (busy.value, sustainable.value) {
            (Some(busy), Some(sustainable)) => Observation::available(
                busy == f64::from(sustainable),
                "prometheus.ix_build_slots_busy/config:fleet.capacity_limits.build_slots",
                at,
            ),
            _ => unavailable(
                "prometheus.ix_build_slots_busy/config:fleet.capacity_limits.build_slots",
                "slot_values_unavailable",
            ),
        };
        let mut derived_sources = BTreeMap::new();
        derived_sources.insert("throttled_share".to_owned(), "admission_only".to_owned());
        let build_slots_busy = match busy.value {
            Some(value) if value >= 0.0 && value.fract() == 0.0 && value <= f64::from(u32::MAX) => {
                Observation {
                    value: Some(value as u32),
                    availability: busy.availability,
                    source: busy.source.clone(),
                    reason: busy.reason.clone(),
                    coverage: busy.coverage,
                    observed_at_ms: busy.observed_at_ms,
                }
            }
            _ => unavailable("prometheus.ix_build_slots_busy", "slot_count_unavailable"),
        };
        Ok(WindowMeasurements {
            lanes: Observation::available(request.lanes, "relay.capacity_window.lanes", at),
            build_slots_busy,
            build_slots_sustainable: sustainable,
            window_start_ms: request.start_ms,
            window_end_ms: request.end_ms,
            rampup_end_ms: request.rampup_end_ms,
            tickets_closed,
            prs_merged,
            commits_pushed: unavailable("github.commits", "commit_reader_not_configured"),
            host_output_per_hour,
            output_basis,
            queue_wait_p90_s: queue_wait,
            throttled_share,
            memory_headroom_min: observed(
                memory_headroom,
                "host_memory-ix_board_config.memory_reserve_bytes",
                "memory_reading_or_reserve_unavailable",
                at,
            ),
            thermal_headroom_min: observed(
                thermal_headroom,
                "ix_board_config.thermal_limit_celsius-host_temperature",
                "temperature_or_limit_unavailable",
                at,
            ),
            slots_busy_equals_sustainable: slot_equal,
            derived_sources,
            lane_output,
        })
    }
}
