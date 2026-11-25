// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `[fleet]` table of the ix-board configuration file.
//!
//! The working window, `max_tool_runtime` and `no_events_after` belong to the relay
//! (`ix.fleet.config`); a local override of any of them is refused.

use super::FleetError;
use ix_trace_rs::implements;
use std::collections::BTreeMap;
use std::time::Duration;

const RELAY_OWNED: [&str; 6] = [
    "working_window",
    "working_window_secs",
    "max_tool_runtime",
    "max_tool_runtime_secs",
    "no_events_after",
    "no_events_after_secs",
];
/// Default orchestrator cycle, ten minutes (FR-039).
const DEFAULT_CYCLE_SECS: u64 = 600;
const KNOWN: [&str; 23] = [
    "team_keys",
    "focus_window_secs",
    "ledger_path",
    "ledger_retention_days",
    "ledger_lock_wait_secs",
    "tick_interval_secs",
    "cycle_interval_secs",
    "grace_secs",
    "prometheus_url",
    "lanes",
    "hosts",
    "loop_stops",
    "dead_after_secs",
    "max_clock_skew_secs",
    "push_interval_secs",
    "host_timeout_secs",
    "capacity_limits",
    "capacity_rampup_secs",
    "capacity_window_min_secs",
    "saturation_window_secs",
    "min_duty_observation_secs",
    "saturation_queue_depth",
    "saturation_projects",
];

/// Explicit per-host capacity limits. Values are never inferred from telemetry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostCapacityLimits {
    /// Interim sustainable build slots.
    pub build_slots: Option<u32>,
    /// Optional operator-entered limit on lane demand.
    pub lane_capacity: Option<u32>,
    /// Minimum free memory reserved for the host, in bytes.
    pub memory_reserve_bytes: Option<u64>,
    /// Maximum permitted CPU temperature, in Celsius.
    pub thermal_limit_celsius: Option<f64>,
}

/// The relay defaults for the three relay-owned thresholds (ix-relay `[fleet]` documentation).
/// The one definition in ix-board: used only for a lane whose host did not report
/// `ix.fleet.config`, and every such row is marked `thresholds_defaulted`.
pub const RELAY_DEFAULT_WORKING_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Relay default `max_tool_runtime`; see [`RELAY_DEFAULT_WORKING_WINDOW`].
pub const RELAY_DEFAULT_MAX_TOOL_RUNTIME: Duration = Duration::from_secs(2 * 60 * 60);
/// Relay default `no_events_after`; see [`RELAY_DEFAULT_WORKING_WINDOW`].
pub const RELAY_DEFAULT_NO_EVENTS_AFTER: Duration = Duration::from_secs(30 * 60);

/// ix-board-owned fleet settings and the configured lanes and hosts.
#[derive(Clone, Debug, PartialEq)]
pub struct FleetConfig {
    /// Prometheus base URL; unset makes the Prometheus source unreadable.
    pub prometheus_url: Option<String>,
    /// Lanes that must appear in the registry.
    pub lanes: Vec<String>,
    /// Hosts that must have lanes in the registry.
    pub hosts: Vec<String>,
    /// Consecutive stops with no tool use that make a loop.
    pub loop_stops: u32,
    /// Age after which a lane with no event and no pane is dead.
    pub dead_after: Duration,
    /// Largest tolerated difference between the relay clock and this clock.
    pub max_clock_skew: Duration,
    /// How often the watcher pushes; a series older than twice this is stale.
    pub push_interval: Duration,
    /// Longest one host may take to read.
    pub host_timeout: Duration,
    /// Ledger file; unset means the user state directory.
    pub ledger_path: Option<std::path::PathBuf>,
    /// Rows older than this are pruned from the ledger.
    pub ledger_retention: Duration,
    /// Longest a writer waits for the ledger lock.
    pub ledger_lock_wait: Duration,
    /// Interval of `fleet tick`; a span with no tick for twice this is a gap.
    pub tick_interval: Duration,
    /// Orchestrator cycle interval used by `open_cycles`.
    pub cycle_interval: Duration,
    /// Grace of `open_cycles`; unset means one cycle plus one tick.
    pub grace: Option<Duration>,
    /// Linear team keys a ticket identifier is recognised by in branch and directory names
    /// (FR-037). The keys of an assignment are always recognised as well.
    pub team_keys: Vec<String>,
    /// The focus window of FR-037; ten minutes by default.
    pub focus_window: Duration,
    /// Explicit host build-slot, memory-reserve and thermal limits.
    pub capacity_limits: BTreeMap<String, HostCapacityLimits>,
    /// Ramp-up duration after adding a lane (default 30 minutes).
    pub capacity_rampup: Duration,
    /// Minimum post-ramp-up measurement duration (default one hour).
    pub capacity_window_min: Duration,
    /// Trailing window for fleet saturation duty evidence.
    pub saturation_window: Duration,
    /// Minimum watcher/compiler observation interval for a usable duty estimate.
    pub min_duty_observation: Duration,
    /// Ready-ticket target per needed lane.
    pub saturation_queue_depth: u32,
    /// Linear projects whose ready tickets feed fleet buffer demand.
    pub saturation_projects: Vec<String>,
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            prometheus_url: None,
            lanes: Vec::new(),
            hosts: Vec::new(),
            loop_stops: 3,
            dead_after: Duration::from_secs(15 * 60),
            max_clock_skew: Duration::from_secs(30),
            push_interval: Duration::from_secs(30),
            host_timeout: Duration::from_secs(20),
            ledger_path: None,
            ledger_retention: Duration::from_secs(30 * 86_400),
            ledger_lock_wait: Duration::from_secs(10),
            tick_interval: Duration::from_secs(300),
            cycle_interval: Duration::from_secs(DEFAULT_CYCLE_SECS),
            grace: None,
            team_keys: Vec::new(),
            focus_window: Duration::from_mins(10),
            capacity_limits: BTreeMap::new(),
            capacity_rampup: Duration::from_secs(30 * 60),
            capacity_window_min: Duration::from_secs(60 * 60),
            saturation_window: Duration::from_secs(24 * 60 * 60),
            min_duty_observation: Duration::from_secs(60 * 60),
            saturation_queue_depth: 2,
            saturation_projects: Vec::new(),
        }
    }
}

fn secs(table: &toml::Table, key: &str, into: &mut Duration) -> Result<(), FleetError> {
    if let Some(value) = table.get(key) {
        let n = value
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| FleetError::Config(format!("{key} must be a positive whole number")))?;
        *into = Duration::from_secs(n);
    }
    Ok(())
}

fn strings(table: &toml::Table, key: &str) -> Result<Vec<String>, FleetError> {
    let Some(value) = table.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(|| FleetError::Config(format!("{key} must be a list of strings")))
}

impl FleetConfig {
    /// Reads the `[fleet]` table of a parsed configuration file. `env_prometheus` is
    /// `IX_PROMETHEUS_URL` and beats the file; `IX_BOARD_LEDGER_PATH` beats `ledger_path`. The
    /// ledger environment is read here, at configuration load, and nowhere else, so a
    /// hand-built [`FleetConfig`] always names the ledger it writes.
    ///
    /// # Errors
    ///
    /// Refuses a relay-owned threshold, an unknown key and a malformed value.
    #[implements("FR-041-AC-5")]
    pub fn from_toml(
        file: Option<&toml::Value>,
        env_prometheus: Option<String>,
    ) -> Result<Self, FleetError> {
        let mut config = Self::default();
        if let Some(table) = file
            .and_then(|f| f.get("fleet"))
            .and_then(toml::Value::as_table)
        {
            for key in table.keys() {
                if RELAY_OWNED.contains(&key.as_str()) {
                    return Err(FleetError::Config(format!(
                        "{key} is read from the relay ix.fleet.config and cannot be set locally"
                    )));
                }
                if !KNOWN.contains(&key.as_str()) {
                    return Err(FleetError::Config(format!("unknown key {key}")));
                }
            }
            config.prometheus_url = table
                .get("prometheus_url")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            config.lanes = strings(table, "lanes")?;
            config.hosts = strings(table, "hosts")?;
            config.team_keys = strings(table, "team_keys")?;
            secs(table, "focus_window_secs", &mut config.focus_window)?;
            if let Some(value) = table.get("loop_stops") {
                config.loop_stops = value
                    .as_integer()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or_else(|| {
                        FleetError::Config("loop_stops must be a positive whole number".to_owned())
                    })?;
            }
            secs(table, "dead_after_secs", &mut config.dead_after)?;
            secs(table, "max_clock_skew_secs", &mut config.max_clock_skew)?;
            secs(table, "push_interval_secs", &mut config.push_interval)?;
            secs(table, "host_timeout_secs", &mut config.host_timeout)?;
            secs(table, "capacity_rampup_secs", &mut config.capacity_rampup)?;
            secs(
                table,
                "capacity_window_min_secs",
                &mut config.capacity_window_min,
            )?;
            secs(
                table,
                "saturation_window_secs",
                &mut config.saturation_window,
            )?;
            secs(
                table,
                "min_duty_observation_secs",
                &mut config.min_duty_observation,
            )?;
            config.saturation_projects = strings(table, "saturation_projects")?;
            if let Some(value) = table.get("saturation_queue_depth") {
                config.saturation_queue_depth = value
                    .as_integer()
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        FleetError::Config(
                            "saturation_queue_depth must be a positive whole number".to_owned(),
                        )
                    })?;
            }
            secs(table, "ledger_lock_wait_secs", &mut config.ledger_lock_wait)?;
            secs(table, "tick_interval_secs", &mut config.tick_interval)?;
            secs(table, "cycle_interval_secs", &mut config.cycle_interval)?;
            if table.contains_key("grace_secs") {
                let mut grace = Duration::ZERO;
                secs(table, "grace_secs", &mut grace)?;
                config.grace = Some(grace);
            }
            if table.contains_key("ledger_retention_days") {
                let mut days = Duration::ZERO;
                secs(table, "ledger_retention_days", &mut days)?;
                config.ledger_retention = days * 86_400;
            }
            config.ledger_path = table
                .get("ledger_path")
                .and_then(toml::Value::as_str)
                .map(std::path::PathBuf::from);
            if let Some(limits) = table.get("capacity_limits") {
                let limits = limits.as_table().ok_or_else(|| {
                    FleetError::Config("capacity_limits must be a host table".to_owned())
                })?;
                for (host, value) in limits {
                    let fields = value.as_table().ok_or_else(|| {
                        FleetError::Config(format!("capacity_limits.{host} must be a table"))
                    })?;
                    if let Some(key) = fields.keys().find(|key| {
                        !matches!(
                            key.as_str(),
                            "build_slots"
                                | "lane_capacity"
                                | "memory_reserve_bytes"
                                | "thermal_limit_celsius"
                        )
                    }) {
                        return Err(FleetError::Config(format!(
                            "unknown key capacity_limits.{host}.{key}"
                        )));
                    }
                    let positive_u64 = |key: &str| -> Result<Option<u64>, FleetError> {
                        fields
                            .get(key)
                            .map(|value| {
                                value
                                    .as_integer()
                                    .and_then(|n| u64::try_from(n).ok())
                                    .filter(|n| *n > 0)
                                    .ok_or_else(|| FleetError::Config(format!(
                                        "capacity_limits.{host}.{key} must be a positive integer"
                                    )))
                            })
                            .transpose()
                    };
                    let build_slots = positive_u64("build_slots")?
                        .map(u32::try_from)
                        .transpose()
                        .map_err(|_| {
                            FleetError::Config(format!(
                                "capacity_limits.{host}.build_slots is too large"
                            ))
                        })?;
                    let lane_capacity = positive_u64("lane_capacity")?
                        .map(u32::try_from)
                        .transpose()
                        .map_err(|_| {
                            FleetError::Config(format!(
                                "capacity_limits.{host}.lane_capacity is too large"
                            ))
                        })?;
                    let thermal_limit_celsius = fields
                        .get("thermal_limit_celsius")
                        .map(|value| {
                            value
                                .as_float()
                                .or_else(|| value.as_integer().map(|n| n as f64))
                                .filter(|n| n.is_finite() && *n > 0.0)
                                .ok_or_else(|| FleetError::Config(format!(
                                    "capacity_limits.{host}.thermal_limit_celsius must be positive"
                                )))
                        })
                        .transpose()?;
                    config.capacity_limits.insert(
                        host.to_lowercase(),
                        HostCapacityLimits {
                            build_slots,
                            lane_capacity,
                            memory_reserve_bytes: positive_u64("memory_reserve_bytes")?,
                            thermal_limit_celsius,
                        },
                    );
                }
            }
        }
        if let Some(path) = std::env::var_os("IX_BOARD_LEDGER_PATH").filter(|p| !p.is_empty()) {
            config.ledger_path = Some(std::path::PathBuf::from(path));
        }
        if let Some(url) = env_prometheus.filter(|url| !url.trim().is_empty()) {
            config.prometheus_url = Some(url);
        }
        Ok(config)
    }
}
