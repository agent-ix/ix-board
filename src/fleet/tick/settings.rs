// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `[fleet_tick]` table: owners, thresholds and per-host targets of `fleet tick` (FR-038).

use crate::fleet::FleetError;
use ix_trace_rs::implements;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

const KNOWN: [&str; 11] = [
    "stale_pr_after_secs",
    "drain_build_window_secs",
    "drain_cap_secs",
    "orchestrator",
    "minder",
    "standing_orders_path",
    "default_busy_target",
    "leads",
    "busy_targets",
    "lane_capacity",
    "planners",
];

/// Settings of `fleet tick` beyond the shared `[fleet]` table.
#[derive(Clone, Debug, PartialEq)]
pub struct TickSettings {
    /// A lane-owned pull request without activity this long is stale (default 3h).
    pub stale_pr_after: Duration,
    /// Build quiet window required before a drain can complete.
    pub drain_build_window: Duration,
    /// A lane draining this long gets `escalate_drain` (default 4h).
    pub drain_cap: Duration,
    /// Address of the orchestrator session; owner of `fix_source` and rung 3.
    pub orchestrator: Option<String>,
    /// Address of the minder session, to attribute a tick to its role.
    pub minder: Option<String>,
    /// The SWM-226 standing-order file.
    pub standing_orders_path: Option<PathBuf>,
    /// Busy ratio a host must reach before it is left alone (default 0.85).
    pub default_busy_target: f64,
    /// Lane address (lower case) to the address of its lead.
    pub leads: BTreeMap<String, String>,
    /// Host (lower case) to its busy-ratio target.
    pub busy_targets: BTreeMap<String, f64>,
    /// Host (lower case) to its lane capacity, the bound of a free lane slot.
    pub lane_capacity: BTreeMap<String, u32>,
    /// Project identifier or name (lower case) to its planner session.
    pub planners: BTreeMap<String, String>,
}

impl Default for TickSettings {
    fn default() -> Self {
        Self {
            stale_pr_after: Duration::from_secs(3 * 3600),
            drain_build_window: Duration::from_secs(300),
            drain_cap: Duration::from_secs(4 * 3600),
            orchestrator: None,
            minder: None,
            standing_orders_path: None,
            default_busy_target: 0.85,
            leads: BTreeMap::new(),
            busy_targets: BTreeMap::new(),
            lane_capacity: BTreeMap::new(),
            planners: BTreeMap::new(),
        }
    }
}

fn bad(key: &str, what: &str) -> FleetError {
    FleetError::Config(format!("fleet_tick.{key} must be {what}"))
}

fn secs(table: &toml::Table, key: &str, into: &mut Duration) -> Result<(), FleetError> {
    if let Some(value) = table.get(key) {
        let n = value
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| bad(key, "a positive whole number"))?;
        *into = Duration::from_secs(n);
    }
    Ok(())
}

fn text(table: &toml::Table, key: &str) -> Result<Option<String>, FleetError> {
    table
        .get(key)
        .map(|v| {
            v.as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| bad(key, "a non-empty string"))
        })
        .transpose()
}

fn ratio(value: &toml::Value, key: &str) -> Result<f64, FleetError> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
        .filter(|r| r.is_finite() && *r > 0.0 && *r <= 1.0)
        .ok_or_else(|| bad(key, "a ratio in (0, 1]"))
}

fn sub<'a>(table: &'a toml::Table, key: &str) -> Result<Option<&'a toml::Table>, FleetError> {
    table
        .get(key)
        .map(|v| v.as_table().ok_or_else(|| bad(key, "a table")))
        .transpose()
}

impl TickSettings {
    /// Reads the `[fleet_tick]` table of a parsed configuration file; absent means defaults.
    ///
    /// # Errors
    ///
    /// Refuses an unknown key and a malformed value.
    #[implements("FR-038-AC-5")]
    #[implements("FR-038-AC-15")]
    pub fn from_toml(file: Option<&toml::Value>) -> Result<Self, FleetError> {
        let mut out = Self::default();
        let Some(table) = file
            .and_then(|f| f.get("fleet_tick"))
            .and_then(toml::Value::as_table)
        else {
            return Ok(out);
        };
        if let Some(key) = table.keys().find(|k| !KNOWN.contains(&k.as_str())) {
            return Err(FleetError::Config(format!("unknown key fleet_tick.{key}")));
        }
        secs(table, "stale_pr_after_secs", &mut out.stale_pr_after)?;
        secs(
            table,
            "drain_build_window_secs",
            &mut out.drain_build_window,
        )?;
        secs(table, "drain_cap_secs", &mut out.drain_cap)?;
        out.orchestrator = text(table, "orchestrator")?;
        out.minder = text(table, "minder")?;
        out.standing_orders_path = text(table, "standing_orders_path")?.map(PathBuf::from);
        if let Some(value) = table.get("default_busy_target") {
            out.default_busy_target = ratio(value, "default_busy_target")?;
        }
        if let Some(leads) = sub(table, "leads")? {
            for (lane, lead) in leads {
                let lead = lead.as_str().ok_or_else(|| bad("leads", "strings"))?;
                out.leads.insert(lane.to_lowercase(), lead.to_owned());
            }
        }
        if let Some(targets) = sub(table, "busy_targets")? {
            for (host, value) in targets {
                out.busy_targets
                    .insert(host.to_lowercase(), ratio(value, "busy_targets")?);
            }
        }
        if let Some(capacity) = sub(table, "lane_capacity")? {
            for (host, value) in capacity {
                let n = value
                    .as_integer()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| bad("lane_capacity", "whole numbers"))?;
                out.lane_capacity.insert(host.to_lowercase(), n);
            }
        }
        if let Some(planners) = sub(table, "planners")? {
            for (project, planner) in planners {
                let planner = planner
                    .as_str()
                    .filter(|planner| !planner.trim().is_empty())
                    .ok_or_else(|| bad("planners", "non-empty session addresses"))?;
                out.planners
                    .insert(project.to_lowercase(), planner.to_owned());
            }
        }
        Ok(out)
    }

    /// The role configured for a session address.
    pub fn role_of(&self, addr: &str) -> &'static str {
        let is = |c: &Option<String>| c.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(addr));
        if is(&self.orchestrator) {
            "orchestrator"
        } else if is(&self.minder) {
            "minder"
        } else {
            "other"
        }
    }
}
