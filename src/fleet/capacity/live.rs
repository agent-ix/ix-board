// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Production Prometheus, relay and Linear measurement source for FR-041.

use super::{
    CapacityError, LaneOutputMeasurement, MeasurementSource, Observation, WindowMeasurements,
    WindowRequest,
};
use crate::fleet::{FleetConfig, live::PromQuery};
use crate::team_metrics::wire::AssignmentStatus;
use crate::team_metrics::{Change, EventPage};
use crate::{LinearGraphQlSource, ReadService};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::{Command, Stdio};

/// Real host measurements from Prometheus and ix-relay, with Linear issue/PR output.
pub struct LiveMeasurementSource<'a> {
    /// Explicit ix-board thresholds and interim slot limits.
    pub config: &'a FleetConfig,
    /// Bounded Prometheus client.
    pub prometheus: Option<&'a dyn PromQuery>,
}

impl LiveMeasurementSource<'_> {
    /// Evidence kind derived from the live host source; absent data never becomes a zero event.
    pub fn derived_evidence_kind(&self, host: &str) -> Option<&'static str> {
        let query = self.prometheus?;
        let host = host.to_lowercase();
        let label = esc(&host);
        let admission = query_value(
            query,
            &format!("increase(ix_build_admission_blocked_seconds_total{{host=\"{label}\"}}[15m])"),
        )
        .ok()
        .flatten()
        .is_some_and(|value| value > 0.0);
        if admission {
            return Some("admission_block");
        }
        let aborted = query_value(
            query,
            &format!("sum(increase(ix_build_builds_total{{host=\"{label}\",outcome=~\"aborted_memory|aborted_thermal|aborted_rss_cap|aborted_pause_exceeded\"}}[15m]))"),
        )
        .ok()
        .flatten()
        .is_some_and(|value| value > 0.0);
        if aborted {
            return Some("build_abort");
        }
        let limits = self.config.capacity_limits.get(&host)?;
        if let Some(reserve) = limits.memory_reserve_bytes {
            let available = query_value(
                query,
                &format!("min_over_time(node_memory_MemAvailable_bytes{{host=\"{label}\"}}[15m])"),
            )
            .ok()
            .flatten()
            .or_else(|| {
                query_value(
                    query,
                    &format!("min_over_time((ix_fleet_host_memory_total_bytes{{host=\"{label}\"}} - ix_fleet_host_memory_used_bytes{{host=\"{label}\"}})[15m:])"),
                )
                .ok()
                .flatten()
            });
            if available.is_some_and(|available| available < reserve as f64) {
                return Some("memory_below_reserve");
            }
        }
        // A temperature at the configured limit is a safety signal, not proof that the
        // scheduler aborted a build. FR-041-AC-20 only corroborates thermal_abort from the
        // closed ix_build_builds_total outcome set above.
        let throttle = query_value(
            query,
            &format!(
                "increase(ix_build_admission_blocked_seconds_total{{host=\"{label}\"}}[1h]) / 3600"
            ),
        )
        .ok()
        .flatten()
        .is_some_and(|share| share > 0.05);
        throttle.then_some("throttle_share")
    }
}

fn query_value(query: &dyn PromQuery, expression: &str) -> Result<Option<f64>, String> {
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
    query: Option<&dyn PromQuery>,
    host: &str,
    expression: String,
    coverage_expression: String,
    source: &str,
    at: i64,
) -> Observation<f64> {
    let Some(query) = query else {
        return unavailable(source, "prometheus_url_unconfigured");
    };
    match query_value(query, &expression) {
        Ok(Some(value)) => match query_value(query, &coverage_expression) {
            Ok(Some(coverage)) if coverage >= 0.9 => Observation {
                value: Some(value),
                availability: super::Availability::Available,
                source: source.to_owned(),
                reason: None,
                coverage: Some(coverage.min(1.0)),
                observed_at_ms: Some(at),
            },
            Ok(coverage) => Observation::unavailable(source, "coverage_below_90_percent", coverage),
            Err(error) => unavailable(source, format!("{host}: coverage read failed: {error}")),
        },
        Ok(None) => unavailable(source, "series_missing"),
        Err(error) => unavailable(source, format!("{host}: {error}")),
    }
}

fn github_commit_count(
    client: &reqwest::blocking::Client,
    token: &str,
    pull_request_url: &str,
    start: i64,
    end: i64,
) -> Result<u64, String> {
    let parsed = reqwest::Url::parse(pull_request_url).map_err(|error| error.to_string())?;
    let parts = parsed
        .path_segments()
        .ok_or_else(|| "pull request URL has no path".to_owned())?
        .collect::<Vec<_>>();
    if parts.len() < 4 || parts[2] != "pull" {
        return Err("pull request URL is not a GitHub pull request".to_owned());
    }
    let mut count = 0_u64;
    for page in 1..=100 {
        let page = page.to_string();
        let response = client
            .get(format!(
                "https://api.github.com/repos/{}/{}/pulls/{}/commits",
                parts[0], parts[1], parts[3]
            ))
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, "ix-board")
            .query(&[("per_page", "100"), ("page", page.as_str())])
            .send()
            .map_err(|error| format!("GitHub commit read: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("GitHub commit read: HTTP {}", response.status()));
        }
        let commits: Vec<Value> = response
            .json()
            .map_err(|error| format!("GitHub commit response: {error}"))?;
        let page_len = commits.len();
        for commit in commits {
            let date = commit["commit"]["author"]["date"]
                .as_str()
                .or_else(|| commit["commit"]["committer"]["date"].as_str())
                .ok_or_else(|| "GitHub commit has no author or committer date".to_owned())?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(date)
                .map_err(|error| format!("GitHub commit timestamp: {error}"))?
                .timestamp();
            if (start..=end).contains(&timestamp) {
                count = count.saturating_add(1);
            }
        }
        if page_len < 100 {
            return Ok(count);
        }
    }
    Err("GitHub commit pagination exceeded 100 pages".to_owned())
}

fn github_pull_request_details(
    client: &reqwest::blocking::Client,
    token: &str,
    pull_request_url: &str,
) -> Result<(String, String, Option<i64>), String> {
    let parsed = reqwest::Url::parse(pull_request_url).map_err(|error| error.to_string())?;
    let parts = parsed
        .path_segments()
        .ok_or_else(|| "pull request URL has no path".to_owned())?
        .collect::<Vec<_>>();
    if parsed.host_str() != Some("github.com") || parts.len() != 4 || parts[2] != "pull" {
        return Err("pull request URL is not a GitHub pull request".to_owned());
    }
    let response = client
        .get(format!(
            "https://api.github.com/repos/{}/{}/pulls/{}",
            parts[0], parts[1], parts[3]
        ))
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header(reqwest::header::USER_AGENT, "ix-board")
        .send()
        .map_err(|error| format!("GitHub pull request read: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub pull request read: HTTP {}",
            response.status()
        ));
    }
    let detail: Value = response
        .json()
        .map_err(|error| format!("GitHub pull request response: {error}"))?;
    let title = detail["title"]
        .as_str()
        .ok_or_else(|| "GitHub pull request has no title".to_owned())?
        .to_owned();
    let branch = detail["head"]["ref"]
        .as_str()
        .ok_or_else(|| "GitHub pull request has no head branch".to_owned())?
        .to_owned();
    let merged_at = detail["merged_at"]
        .as_str()
        .map(|time| {
            chrono::DateTime::parse_from_rfc3339(time)
                .map(|parsed| parsed.timestamp())
                .map_err(|error| format!("GitHub merged_at timestamp: {error}"))
        })
        .transpose()?;
    Ok((title, branch, merged_at))
}

fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| {
            let output = Command::new("gh")
                .args(["auth", "token"])
                .stdin(Stdio::null())
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
            (!token.is_empty()).then_some(token)
        })
}

fn linear_output(
    request: &WindowRequest,
    config: &FleetConfig,
) -> (
    Observation<u64>,
    Observation<u64>,
    Observation<u64>,
    Vec<LaneOutputMeasurement>,
) {
    let timeout = config.host_timeout;
    let fail = |source: &str, reason: String| unavailable(source, reason);
    let source = match LinearGraphQlSource::from_env() {
        Ok(source) => source,
        Err(error) => {
            return (
                fail("linear.issue_completed_at", error.to_string()),
                fail("linear.pull_request.merged_at", error.to_string()),
                fail(
                    "github.pull_request_commits",
                    "linear_source_unavailable".to_owned(),
                ),
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
                fail(
                    "github.pull_request_commits",
                    "linear_source_unavailable".to_owned(),
                ),
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
                fail(
                    "github.pull_request_commits",
                    "linear_source_unavailable".to_owned(),
                ),
                Vec::new(),
            );
        }
    };
    let assignments = match lane_ticket_history(config, request) {
        Ok(assignments) => assignments,
        Err(reason) => {
            return (
                fail(
                    "ledger.assignment_history+agent-msg.work.events",
                    reason.clone(),
                ),
                fail(
                    "ledger.assignment_history+agent-msg.work.events",
                    reason.clone(),
                ),
                fail("github.ticket_linkage", reason),
                Vec::new(),
            );
        }
    };
    let start = request.start_ms / 1000;
    let end = request.end_ms / 1000;
    let closed_at = |time: Option<&str>| {
        time.and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .is_some_and(|parsed| (start..=end).contains(&parsed.timestamp()))
    };
    let issue_map: HashMap<String, _> = snapshot
        .issues
        .iter()
        .map(|issue| (issue.identifier.to_lowercase(), issue))
        .collect();
    let github_token = github_token();
    let github_client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok();
    let mut github_error = if github_token.is_none() {
        Some("GITHUB_TOKEN_or_GH_TOKEN_unconfigured".to_owned())
    } else if github_client.is_none() {
        Some("GitHub client could not be created".to_owned())
    } else {
        None
    };
    let mut github_pr_error = github_error.clone();
    let mut lane_counts: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    for (lane, ticket) in assignments {
        let counts = lane_counts.entry(lane.clone()).or_default();
        {
            let key = ticket.rsplit('/').next().unwrap_or(&ticket).to_lowercase();
            let Some(issue) = issue_map.get(&key) else {
                github_error.get_or_insert_with(|| {
                    format!("Linear snapshot missing assigned ticket {key}")
                });
                continue;
            };
            if closed_at(issue.completed_at.as_deref()) {
                counts.0 = counts.0.saturating_add(1);
            }
            if !issue.attachments_complete {
                github_error.get_or_insert_with(|| {
                    format!("Linear pull request attachments incomplete for {key}")
                });
                github_pr_error.get_or_insert_with(|| {
                    format!("Linear pull request attachments incomplete for {key}")
                });
                continue;
            }
            if let (Some(client), Some(token)) = (&github_client, &github_token) {
                for pr in &issue.pull_requests {
                    match github_pull_request_details(client, token, &pr.url) {
                        Ok((title, branch, merged_at))
                            if title.to_lowercase().contains(&key)
                                || branch.to_lowercase().contains(&key) =>
                        {
                            if merged_at.is_some_and(|merged| (start..=end).contains(&merged)) {
                                counts.1 = counts.1.saturating_add(1);
                            }
                        }
                        Ok(_) => {
                            let reason = format!(
                                "GitHub PR {} has no {key} ticket id in title or branch",
                                pr.url
                            );
                            github_error.get_or_insert_with(|| reason.clone());
                            github_pr_error.get_or_insert(reason);
                            continue;
                        }
                        Err(error) => {
                            github_error.get_or_insert_with(|| error.clone());
                            github_pr_error.get_or_insert(error);
                            continue;
                        }
                    }
                    match github_commit_count(client, token, &pr.url, start, end) {
                        Ok(commits) => counts.2 = counts.2.saturating_add(commits),
                        Err(error) => {
                            github_error.get_or_insert(error);
                        }
                    }
                }
            }
        }
    }
    if lane_counts.is_empty() {
        let reason = "no_ticket_mapping_for_host_window".to_owned();
        return (
            fail("linear.issue_completed_at", reason.clone()),
            fail("linear.pull_request.merged_at", reason.clone()),
            fail("github.pull_request_commits", reason),
            Vec::new(),
        );
    }
    let completed = lane_counts.values().map(|counts| counts.0).sum();
    let merged = lane_counts.values().map(|counts| counts.1).sum();
    let commits: u64 = lane_counts.values().map(|counts| counts.2).sum();
    let prs_observation = github_pr_error.map_or_else(
        || {
            Observation::available(
                merged,
                "github.pull_request.merged_at_by_ticket",
                request.end_ms,
            )
        },
        |reason| unavailable("github.pull_request.merged_at_by_ticket", reason),
    );
    let commits_observation = github_error.map_or_else(
        || Observation::available(commits, "github.pull_request_commits", request.end_ms),
        |reason| unavailable("github.pull_request_commits", reason),
    );
    let lane_output = lane_counts
        .into_iter()
        .map(|(lane, (tickets, prs, commits))| LaneOutputMeasurement {
            lane,
            tickets_closed: Observation::available(
                tickets,
                "linear.issue.completed_at",
                request.end_ms,
            ),
            prs_merged: Observation::available(
                prs,
                "linear.pull_request.merged_at",
                request.end_ms,
            ),
            commits_pushed: if commits_observation.value.is_some() {
                Observation::available(commits, "github.pull_request_commits", request.end_ms)
            } else {
                unavailable(
                    "github.pull_request_commits",
                    commits_observation.reason.clone().unwrap_or_default(),
                )
            },
        })
        .collect();
    let at = request.end_ms;
    (
        Observation::available(completed, "linear.issue.completed_at", at),
        prs_observation,
        commits_observation,
        lane_output,
    )
}

fn lane_ticket_history(
    config: &FleetConfig,
    request: &WindowRequest,
) -> Result<Vec<(String, String)>, String> {
    let path = crate::fleet::ledger::resolve_path(config).map_err(|error| error.to_string())?;
    if !path.exists() {
        return Err("fleet ledger has no assignment history".to_owned());
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT lower(lane),ticket FROM assignment_history
             WHERE lower(lane) LIKE lower(?1) AND assigned_at<=?2
               AND (replaced_at IS NULL OR replaced_at>=?3)",
        )
        .map_err(|error| error.to_string())?;
    let host_prefix = format!("{}:%", request.host);
    let assignments = statement
        .query_map(
            rusqlite::params![host_prefix, request.end_ms, request.start_ms],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if assignments.is_empty() {
        return Err("no ledger assignment overlaps the output window".to_owned());
    }

    let mut cursor = format!("{}-0", request.start_ms);
    let mut work_tickets = BTreeSet::new();
    let mut covered = false;
    let mut finished = false;
    for _ in 0..20 {
        let args = ["work", "events", "--after", &cursor, "--limit", "500"];
        let bytes = crate::team_metrics::read_history("agent-msg", &args, config.host_timeout)
            .map_err(|error| error.to_string())?;
        let page: EventPage = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if page.version != 1 || page.events.len() > 500 || page.truncated {
            return Err("agent-msg work-event history is incomplete".to_owned());
        }
        if !page
            .available_from_ms
            .is_some_and(|from| from <= request.start_ms)
        {
            return Err("agent-msg work-event retention starts after output window".to_owned());
        }
        covered = true;
        for event in &page.events {
            if !(request.start_ms..=request.end_ms).contains(&event.at_ms) {
                continue;
            }
            if let Change::Agent { agent } = &event.change
                && matches!(
                    agent.status,
                    AssignmentStatus::Running | AssignmentStatus::Done
                )
            {
                work_tickets.insert(agent.ticket.to_lowercase());
            }
        }
        if !page.has_more {
            finished = true;
            break;
        }
        if page.next_cursor == cursor {
            return Err("agent-msg work-event cursor did not advance".to_owned());
        }
        cursor = page.next_cursor;
    }
    if !covered || !finished {
        return Err(
            "agent-msg work-event history is unavailable or exceeded page limit".to_owned(),
        );
    }
    let matched = assignments
        .into_iter()
        .filter(|(_, ticket)| {
            work_tickets.contains(&ticket.rsplit('/').next().unwrap_or(ticket).to_lowercase())
        })
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return Err("no assignment ticket maps to a work event in the output window".to_owned());
    }
    Ok(matched)
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
        let window_seconds = (request.end_ms - request.start_ms) / 1000;
        let source = |metric: &str, operation: &str| {
            let selector = format!("{metric}{{host=\"{label}\"}}");
            let expression = format!("{operation}({selector}[{span}])");
            let coverage = format!(
                "sum(count_over_time({selector}[{span}])) * {} / {window_seconds}",
                self.config.push_interval.as_secs()
            );
            (expression, coverage)
        };
        let (busy_query, busy_coverage) = source("ix_build_slots_busy", "max_over_time");
        let busy = source_observation(
            self.prometheus,
            &host,
            busy_query,
            busy_coverage,
            "prometheus.ix_build_slots_busy",
            at,
        );
        let queue_selector = format!("ix_build_queue_wait_seconds{{host=\"{label}\"}}");
        let queue_wait_query = format!("quantile_over_time(0.9, {queue_selector}[{span}])");
        let queue_wait_coverage = format!(
            "sum(count_over_time({queue_selector}[{span}])) * {} / {window_seconds}",
            self.config.push_interval.as_secs()
        );
        let queue_wait = source_observation(
            self.prometheus,
            &host,
            queue_wait_query,
            queue_wait_coverage,
            "prometheus.ix_build_queue_wait_seconds",
            at,
        );
        let (admission_query, admission_coverage) =
            source("ix_build_admission_blocked_seconds_total", "increase");
        let admission = source_observation(
            self.prometheus,
            &host,
            admission_query,
            admission_coverage,
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
        let memory_selector = format!("node_memory_MemAvailable_bytes{{host=\"{label}\"}}");
        let memory_coverage = format!(
            "sum(count_over_time({memory_selector}[{span}])) * {} / {window_seconds}",
            self.config.push_interval.as_secs()
        );
        let memory = source_observation(
            self.prometheus,
            &host,
            format!("min_over_time({memory_selector}[{span}])"),
            memory_coverage,
            "node_exporter.node_memory_MemAvailable_bytes",
            at,
        );
        let memory = if memory.value.is_some() {
            memory
        } else {
            source_observation(
                self.prometheus,
                &host,
                format!(
                    "min_over_time((ix_fleet_host_memory_total_bytes{{host=\"{label}\"}} - ix_fleet_host_memory_used_bytes{{host=\"{label}\"}})[{span}:])"
                ),
                format!(
                    "sum(count_over_time(ix_fleet_host_memory_total_bytes{{host=\"{label}\"}}[{span}])) * {} / {window_seconds}",
                    self.config.push_interval.as_secs()
                ),
                "relay_watcher.ix_fleet_host_memory_total_bytes-ix_fleet_host_memory_used_bytes",
                at,
            )
        };
        let temperature_expression = temp_query(&host, "node_thermal_temperature_celsius", &span);
        let temperature_selector = match host.as_str() {
            "titan" => format!("node_hwmon_temp_celsius{{host=\"{label}\",sensor=\"Tctl\"}}"),
            "luna" => format!(
                "node_hwmon_temp_celsius{{host=\"{label}\"}} or node_thermal_zone_temp{{host=\"{label}\"}}"
            ),
            "terra" => {
                format!("windows_sensors_physical_cpu_temperature_celsius{{host=\"{label}\"}}")
            }
            "draco" => format!("node_thermal_temperature_celsius{{host=\"{label}\"}}"),
            _ => format!("node_thermal_temperature_celsius{{host=\"{label}\"}}"),
        };
        let temperature_coverage = if host == "luna" {
            format!(
                "(sum(count_over_time(node_hwmon_temp_celsius{{host=\"{label}\"}}[{span}])) + sum(count_over_time(node_thermal_zone_temp{{host=\"{label}\"}}[{span}]))) * {} / {window_seconds}",
                self.config.push_interval.as_secs()
            )
        } else {
            format!(
                "sum(count_over_time({temperature_selector}[{span}])) * {} / {window_seconds}",
                self.config.push_interval.as_secs()
            )
        };
        let temperature = source_observation(
            self.prometheus,
            &host,
            temperature_expression,
            temperature_coverage,
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
        let (tickets_closed, prs_merged, commits_pushed, lane_output) =
            linear_output(request, self.config);
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
            _ => match commits_pushed.value {
                Some(commits) => {
                    let hours = (request.end_ms - request.start_ms) as f64 / 3_600_000.0;
                    (
                        Observation::available(
                            commits as f64 / hours,
                            "github.pull_request_commits",
                            at,
                        ),
                        Observation::available(
                            "commits".to_owned(),
                            "ix-board.FR-041.output_basis",
                            at,
                        ),
                    )
                }
                None => (
                    unavailable(
                        "capacity_output",
                        commits_pushed
                            .reason
                            .clone()
                            .unwrap_or_else(|| "output_sources_unavailable".to_owned()),
                    ),
                    unavailable(
                        "capacity_output",
                        "tickets_prs_below_threshold_and_commits_unavailable",
                    ),
                ),
            },
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
            commits_pushed,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::SourceError;
    use crate::fleet::config::HostCapacityLimits;
    use crate::fleet::live::PromQuery;
    use std::time::Duration;

    struct MockPrometheus;

    impl PromQuery for MockPrometheus {
        fn query(&self, expression: &str) -> Result<String, SourceError> {
            let value = if expression.contains("count_over_time") {
                108.0
            } else if expression.contains("ix_build_slots_busy") {
                2.0
            } else if expression.contains("ix_build_queue_wait_seconds") {
                4.0
            } else if expression.contains("ix_build_admission_blocked_seconds_total") {
                0.0
            } else if expression.contains("node_memory_MemAvailable_bytes") {
                2_147_483_648.0
            } else if expression.contains("node_thermal_temperature_celsius") {
                70.0
            } else {
                return Err(SourceError(format!("unexpected query: {expression}")));
            };
            Ok(format!(
                r#"{{"status":"success","data":{{"result":[{{"value":[0,"{value}"]}}]}}}}"#
            ))
        }
    }

    #[test]
    fn live_measurement_reads_host_build_memory_and_thermal_series() {
        let mut config = FleetConfig {
            hosts: vec!["draco".to_owned()],
            push_interval: Duration::from_secs(30),
            ..FleetConfig::default()
        };
        config.capacity_limits.insert(
            "draco".to_owned(),
            HostCapacityLimits {
                build_slots: Some(3),
                memory_reserve_bytes: Some(1_073_741_824),
                thermal_limit_celsius: Some(85.0),
            },
        );
        let prometheus = MockPrometheus;
        let source = LiveMeasurementSource {
            config: &config,
            prometheus: Some(&prometheus),
        };
        let request = WindowRequest {
            host: "draco".to_owned(),
            lanes: 2,
            start_ms: 1_000,
            end_ms: 3_601_000,
            rampup_end_ms: 1_000,
        };
        let measurement = source.measure(&request).unwrap();
        assert_eq!(measurement.build_slots_busy.value, Some(2));
        assert_eq!(measurement.build_slots_sustainable.value, Some(3));
        assert_eq!(measurement.queue_wait_p90_s.value, Some(4.0));
        assert_eq!(measurement.throttled_share.value, Some(0.0));
        assert_eq!(measurement.memory_headroom_min.value, Some(1_073_741_824.0));
        assert_eq!(measurement.thermal_headroom_min.value, Some(15.0));
    }
}
