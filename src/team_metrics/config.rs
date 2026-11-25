// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::Error;
use crate::{ScopeSelection, TeamKey};
use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    path::{Path, PathBuf},
};

fn retention() -> u64 {
    604800
}
fn budget() -> u64 {
    64 * 1024 * 1024
}
fn interval() -> u64 {
    30
}
fn tracker_interval() -> u64 {
    300
}
fn tick_interval() -> u64 {
    300
}

/// Runtime configuration, with no credential values in the wire payload.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Configuration schema, currently one.
    pub version: u32,
    /// Metrics listener; use loopback outside a private cluster network.
    pub listen: SocketAddr,
    /// SQLite event/cursor state on a persistent local filesystem.
    pub state_path: PathBuf,
    /// Maximum SQLite file size, including indices and cached state.
    #[serde(default = "budget")]
    pub max_bytes: u64,
    /// Event retention; cannot exceed seven days in this delivery.
    #[serde(default = "retention")]
    pub retention_seconds: u64,
    /// Lifecycle polling interval, in seconds.
    #[serde(default = "interval")]
    pub activity_interval_seconds: u64,
    /// Tracker/VCS polling interval, in seconds.
    #[serde(default = "tracker_interval")]
    pub tracker_interval_seconds: u64,
    /// Fleet ledger whose last committed `fleet tick` the exporter serves (FR-038); unset serves none.
    #[serde(default)]
    pub fleet_ledger: Option<PathBuf>,
    /// The configured `fleet tick` interval; a tick older than twice this is not served.
    #[serde(default = "tick_interval")]
    pub fleet_tick_interval_seconds: u64,
    /// At most four explicitly monitored scopes within the initial 1,000-series budget.
    pub scopes: Vec<ScopeConfig>,
}

/// Native work selectors, not display labels.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selectors {
    /// Linear team boundary; alone intentionally selects the whole team.
    pub team: Option<String>,
    /// Initiative UUIDs or exact authoritative selectors.
    #[serde(default)]
    pub initiatives: Vec<String>,
    /// Project UUIDs or exact authoritative selectors.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Ticket UUIDs or identifiers, including descendants.
    #[serde(default)]
    pub tickets: Vec<String>,
    /// Select the unique started team project whose name contains `release`. This follows the
    /// current release path without freezing a project ID.
    #[serde(default)]
    pub current_release_project: bool,
}
impl Selectors {
    pub(crate) fn selection(&self) -> ScopeSelection {
        ScopeSelection {
            team: self.team.clone().map(TeamKey),
            initiatives: self.initiatives.clone(),
            projects: self.projects.clone(),
            tickets: self.tickets.clone(),
            ..Default::default()
        }
    }
}

/// One operational team's assigned work, independent of its Linear team key.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeConfig {
    /// Stable operational team ID used as a bounded metric label.
    pub team_id: String,
    /// Stable assigned-scope ID used as a bounded metric label.
    pub scope_id: String,
    /// Current authoritative selectors; selector revisions are stored outside labels.
    pub selectors: Selectors,
    /// Canonical server:workspace:tab identities authorized to report this scope.
    pub leads: Vec<String>,
    /// Refresh `leads` from the relay directory's verified team workspace on each activity poll.
    #[serde(default)]
    pub derive_leads_from_relay: bool,
    /// Herdr workspaces to query; assigned leads still restrict attribution.
    pub herdr_teams: Vec<String>,
    /// Existing retro configuration, including gates, policies and source parameters.
    pub retro_config: PathBuf,
}
impl ScopeConfig {
    pub(crate) fn key(&self) -> String {
        format!("{}:{}", self.team_id, self.scope_id)
    }
    pub(crate) fn owns(&self, address: &str) -> bool {
        self.leads.iter().any(|a| a.eq_ignore_ascii_case(address))
    }
}
impl Config {
    /// Reads and validates configuration before binding sockets or reading sources.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let config: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        config.validate()?;
        Ok(config)
    }
    /// Rejects unbounded state, unsafe label values and ambiguous monitored identities.
    #[implements("FR-033-AC-16")]
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 || self.scopes.len() > 4 || self.scopes.is_empty() {
            return Err(Error::Invalid(
                "version must be one and scopes must contain 1..4 entries",
            ));
        }
        if !(1024 * 1024..=64 * 1024 * 1024).contains(&self.max_bytes)
            || self.retention_seconds == 0
            || self.retention_seconds > 604800
            || !(10..=60).contains(&self.activity_interval_seconds)
            || !(60..=3600).contains(&self.tracker_interval_seconds)
            || self.fleet_tick_interval_seconds == 0
        {
            return Err(Error::Invalid(
                "invalid retention, storage budget or polling interval",
            ));
        }
        let mut keys = BTreeSet::new();
        for scope in &self.scopes {
            for id in [&scope.team_id, &scope.scope_id] {
                if id.is_empty()
                    || id.len() > 64
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                {
                    return Err(Error::Invalid(
                        "team/scope IDs must be 1..64 ASCII letters, digits, underscore or hyphen",
                    ));
                }
            }
            if scope.selectors.team.as_ref().is_some_and(|team| {
                team.is_empty()
                    || team.len() > 64
                    || !team
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            }) {
                return Err(Error::Invalid(
                    "Linear team key has unsafe label characters",
                ));
            }
            if !keys.insert(scope.key())
                || (scope.leads.is_empty() && !scope.derive_leads_from_relay)
                || scope.leads.len() > 32
                || scope.herdr_teams.len() > 16
            {
                return Err(Error::Invalid(
                    "duplicate scope or missing/unbounded leads/workspaces",
                ));
            }
            if scope
                .leads
                .iter()
                .any(|a| a.split(':').count() != 3 || a.split(':').any(str::is_empty))
            {
                return Err(Error::Invalid(
                    "lead identities require server:workspace:tab",
                ));
            }
            let s = &scope.selectors;
            if scope.derive_leads_from_relay && s.team.is_none() {
                return Err(Error::Invalid(
                    "relay lead discovery requires a Linear team selector",
                ));
            }
            if s.current_release_project
                && (!s.projects.is_empty() || !s.initiatives.is_empty() || !s.tickets.is_empty())
            {
                return Err(Error::Invalid(
                    "current release project cannot be combined with fixed selectors",
                ));
            }
            if s.team.is_none()
                && s.projects.is_empty()
                && s.initiatives.is_empty()
                && s.tickets.is_empty()
            {
                return Err(Error::Invalid(
                    "empty scope must not select the entire workspace",
                ));
            }
        }
        Ok(())
    }
}
