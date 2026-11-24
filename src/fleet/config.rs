// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `[fleet]` table of the ix-board configuration file.
//!
//! The working window, `max_tool_runtime` and `no_events_after` belong to the relay
//! (`ix.fleet.config`); a local override of any of them is refused.

use super::FleetError;
use std::time::Duration;

const RELAY_OWNED: [&str; 6] = [
    "working_window",
    "working_window_secs",
    "max_tool_runtime",
    "max_tool_runtime_secs",
    "no_events_after",
    "no_events_after_secs",
];
const KNOWN: [&str; 8] = [
    "prometheus_url",
    "lanes",
    "hosts",
    "loop_stops",
    "dead_after_secs",
    "max_clock_skew_secs",
    "push_interval_secs",
    "host_timeout_secs",
];

/// The relay defaults for the three relay-owned thresholds (ix-relay `[fleet]` documentation).
/// The one definition in ix-board: used only for a lane whose host did not report
/// `ix.fleet.config`, and every such row is marked `thresholds_defaulted`.
pub const RELAY_DEFAULT_WORKING_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Relay default `max_tool_runtime`; see [`RELAY_DEFAULT_WORKING_WINDOW`].
pub const RELAY_DEFAULT_MAX_TOOL_RUNTIME: Duration = Duration::from_secs(2 * 60 * 60);
/// Relay default `no_events_after`; see [`RELAY_DEFAULT_WORKING_WINDOW`].
pub const RELAY_DEFAULT_NO_EVENTS_AFTER: Duration = Duration::from_secs(30 * 60);

/// ix-board-owned fleet settings and the configured lanes and hosts.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// `IX_PROMETHEUS_URL` and beats the file.
    ///
    /// # Errors
    ///
    /// Refuses a relay-owned threshold, an unknown key and a malformed value.
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
        }
        if let Some(url) = env_prometheus.filter(|url| !url.trim().is_empty()) {
            config.prometheus_url = Some(url);
        }
        Ok(config)
    }
}
