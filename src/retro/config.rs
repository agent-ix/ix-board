// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The retro configuration file and the `--window` duration (FR-024).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error as ThisError;

/// Fields every configuration must carry.
const REQUIRED_FIELDS: [&str; 4] = ["team", "gate_tickets", "projects", "repos"];

/// Default window when `--window` is absent.
pub const DEFAULT_WINDOW: &str = "24h";

/// Failures loading the configuration or parsing the window.
#[derive(Debug, ThisError)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("retro config {path}: cannot read: {source}")]
    Read {
        /// Configuration path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file is not valid JSON or has a field of the wrong shape.
    #[error("retro config {path}: invalid: {message}")]
    Invalid {
        /// Configuration path.
        path: PathBuf,
        /// Decoder message.
        message: String,
    },
    /// A required field is absent.
    #[error("retro config {path}: required field `{field}` is missing")]
    MissingField {
        /// Configuration path.
        path: PathBuf,
        /// Missing field name.
        field: &'static str,
    },
    /// A field holds a value the command cannot use.
    #[error("retro config {path}: field `{field}`: {message}")]
    BadValue {
        /// Configuration path.
        path: PathBuf,
        /// Field name.
        field: &'static str,
        /// What is wrong.
        message: String,
    },
    /// The `--window` text is not a duration.
    #[error("window {0:?} must be a positive whole number followed by s, m, h or d")]
    Window(String),
}

/// `pr_idle_hours`, `pr_oversize_lines` and `in_progress_stale_hours`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// Hours without update after which an open pull request is idle.
    pub pr_idle_hours: Option<u64>,
    /// Added plus deleted lines above which a pull request is oversize.
    pub pr_oversize_lines: Option<u64>,
    /// Hours without a state change or pull-request update after which a started issue is stale.
    pub in_progress_stale_hours: Option<u64>,
}

/// Repository and branch whose check-runs define main status.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MainCheck {
    /// `owner/name`.
    pub repo: String,
    /// Branch name.
    pub branch: String,
}

/// The retro configuration (FR-024 Configuration table).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetroConfig {
    /// Store directory name and default Linear team key.
    pub team: String,
    /// Linear team keys whose open issues count toward scope metrics; defaults to `team`.
    #[serde(default)]
    pub linear_teams: Vec<String>,
    /// Linear project names whose issues count toward progress metrics.
    pub projects: Vec<String>,
    /// Issue identifiers whose transitive blockers define the path.
    pub gate_tickets: Vec<String>,
    /// `owner/name` repositories whose pull requests belong to the team.
    pub repos: Vec<String>,
    /// Herdr workspace labels read for utilization metrics.
    #[serde(default)]
    pub herdr_teams: Vec<String>,
    /// Metric thresholds.
    #[serde(default)]
    pub thresholds: Thresholds,
    /// Optional main-branch check configuration.
    #[serde(default)]
    pub main_check: Option<MainCheck>,
    /// Optional directories read for prunable worktrees and disk bytes.
    #[serde(default)]
    pub local_paths: Vec<PathBuf>,
    /// Optional process-name substring identifying build-slot wrappers.
    #[serde(default)]
    pub slot_process: Option<String>,
    /// Age in hours after which an orphaned slot wrapper counts.
    #[serde(default)]
    pub orphan_slot_hours: Option<u64>,
    /// Optional path to the gate log JSONL.
    #[serde(default)]
    pub gate_log: Option<PathBuf>,
    /// Optional relay addresses read for undelivered messages.
    #[serde(default)]
    pub relay_leads: Vec<String>,
    /// Minutes after which a pending relay message is stale.
    #[serde(default)]
    pub message_stale_minutes: Option<u64>,
    /// Optional Linear label marking issues that wait on an owner decision.
    #[serde(default)]
    pub decision_label: Option<String>,
    /// Optional UTC time naming the baseline record.
    #[serde(default)]
    pub baseline_taken_at: Option<String>,
}

impl RetroConfig {
    /// Reads and validates the configuration file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(path, &text)
    }

    /// Validates configuration text read from `path`.
    pub fn parse(path: &Path, text: &str) -> Result<Self, ConfigError> {
        let invalid = |message: String| ConfigError::Invalid {
            path: path.to_owned(),
            message,
        };
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| invalid(error.to_string()))?;
        let object = value
            .as_object()
            .ok_or_else(|| invalid("expected a JSON object".to_owned()))?;
        for field in REQUIRED_FIELDS {
            if object.get(field).is_none_or(serde_json::Value::is_null) {
                return Err(ConfigError::MissingField {
                    path: path.to_owned(),
                    field,
                });
            }
        }
        let mut config: Self =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        if config.team.trim().is_empty() {
            return Err(ConfigError::BadValue {
                path: path.to_owned(),
                field: "team",
                message: "must not be empty".to_owned(),
            });
        }
        if config.linear_teams.is_empty() {
            config.linear_teams = vec![config.team.clone()];
        }
        if let Some(baseline) = &config.baseline_taken_at
            && crate::drift::timestamp_epoch(baseline).is_err()
        {
            return Err(ConfigError::BadValue {
                path: path.to_owned(),
                field: "baseline_taken_at",
                message: format!("{baseline:?} is not a UTC timestamp"),
            });
        }
        Ok(config)
    }
}

/// Parses `90s`, `20m`, `4h` or `2d` into seconds.
pub fn parse_window(text: &str) -> Result<u64, ConfigError> {
    let bad = || ConfigError::Window(text.to_owned());
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
    let (digits, unit) = text.split_at(split);
    let count: u64 = digits.parse().map_err(|_| bad())?;
    let scale = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return Err(bad()),
    };
    match count.checked_mul(scale) {
        Some(0) | None => Err(bad()),
        Some(seconds) => Ok(seconds),
    }
}

/// Renders seconds in the largest exact unit, the form stored in `window`.
pub fn format_window(seconds: u64) -> String {
    for (scale, unit) in [(86_400, "d"), (3600, "h"), (60, "m")] {
        if seconds.is_multiple_of(scale) {
            return format!("{}{unit}", seconds / scale);
        }
    }
    format!("{seconds}s")
}
