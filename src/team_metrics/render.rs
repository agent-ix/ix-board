// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::{Config, Error, State};
use crate::retro::{Status, department::Value};
use ix_trace_rs::implements;
use std::{collections::BTreeSet, fmt::Write};

struct Text {
    text: String,
    types: BTreeSet<String>,
    count: usize,
}
impl Text {
    fn metric(&mut self, name: &str, labels: &str, value: f64, counter: bool) -> Result<(), Error> {
        if !value.is_finite() {
            return Err(Error::Invalid("non-finite metric"));
        }
        self.count += 1;
        if self.count > 1000 {
            return Err(Error::Invalid("team exporter series budget exceeded"));
        }
        if self.types.insert(name.into()) {
            let _ = writeln!(
                self.text,
                "# TYPE {name} {}",
                if counter { "counter" } else { "gauge" }
            );
        }
        let _ = writeln!(self.text, "{name}{{{labels}}} {value}");
        Ok(())
    }
}
fn flag(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn coverage(text: &mut Text, labels: &str, metric: &str, value: f64) -> Result<(), Error> {
    text.metric(
        "ix_team_metric_coverage",
        &format!("{labels},metric=\"{metric}\""),
        value.clamp(0.0, 1.0),
        false,
    )
}
const LEGACY: &[(&str, &str, f64)] = &[
    ("path_remaining", "path_remaining", 1.0),
    ("path_closed", "path_closed_24h", 1.0),
    ("path_closed_unverified", "path_closed_unverified_24h", 1.0),
    ("intake_new", "intake_new_24h", 1.0),
    ("net_closed_minus_new", "net_closed_minus_new_24h", 1.0),
    ("gates_remaining", "gates_remaining", 1.0),
    ("prs_merged_per_hour", "pr_merges_per_hour_24h", 1.0),
    (
        "pr_cycle_median_hours",
        "pr_cycle_median_seconds_24h",
        3600.0,
    ),
    ("pr_cycle_p90_hours", "pr_cycle_p90_seconds_24h", 3600.0),
    ("prs_open", "prs_open", 1.0),
    ("prs_idle", "prs_idle", 1.0),
    ("prs_ready_unmerged", "prs_ready_unmerged", 1.0),
    ("fix_rounds_total", "fix_rounds_24h", 1.0),
    ("fix_rounds_max", "fix_rounds_max_24h", 1.0),
    ("done_reopened", "done_reopened_24h", 1.0),
    ("full_gates_max", "full_gates_max_24h", 1.0),
    ("repins_total", "repins_24h", 1.0),
    ("main_red_incidents", "main_red_incidents_24h", 1.0),
    ("main_red_minutes", "main_red_seconds_24h", 60.0),
    ("in_progress_stale", "in_progress_stale", 1.0),
    ("decisions_open", "decisions_open", 1.0),
    ("decision_oldest_hours", "decision_oldest_seconds", 3600.0),
    ("build_lock_wait_minutes", "build_queue_seconds_24h", 60.0),
];
const DEPARTMENT: &[(&str, &str, f64)] = &[
    ("wip_active", "wip_active", 1.0),
    ("claims_without_movement", "claims_without_movement", 1.0),
    ("merged_open", "merged_open", 1.0),
    ("idle_with_ready", "idle_with_ready", 1.0),
    ("open_chain_length", "open_chain_length", 1.0),
    ("oldest_gated_hours", "oldest_gated_seconds", 3600.0),
    ("ruling_waiting_issues", "ruling_waiting_issues", 1.0),
    (
        "spec_merge_waiting_issues",
        "spec_merge_waiting_issues",
        1.0,
    ),
];

/// Encodes a bounded allowlist in Prometheus text format. Missing data emits
/// availability=0 and omits its numeric value, never substitutes zero.
#[implements("FR-033-AC-16", "FR-033-AC-17", "FR-033-AC-18")]
pub fn render(
    config: &Config,
    state: &State,
    journal_bytes: u64,
    now: i64,
) -> Result<String, Error> {
    let mut out = Text {
        text: String::new(),
        types: BTreeSet::new(),
        count: 0,
    };
    let frontier = state
        .quota_frontier
        .as_ref()
        .filter(|q| (0..=90000).contains(&now.saturating_sub(q.at_ms)));
    out.metric(
        "ix_team_linear_quota_frontier_available",
        "",
        flag(frontier.is_some()),
        false,
    )?;
    if let Some(frontier) = frontier {
        out.metric(
            "ix_team_linear_quota_observations",
            "",
            frontier.count as f64,
            false,
        )?;
        out.metric(
            "ix_team_linear_quota_frontier_timestamp_seconds",
            "",
            frontier.at_ms as f64 / 1000.0,
            false,
        )?;
    }
    out.metric(
        "ix_team_exporter_journal_bytes",
        "",
        journal_bytes as f64,
        false,
    )?;
    out.metric(
        "ix_team_exporter_journal_limit_bytes",
        "",
        config.max_bytes as f64,
        false,
    )?;
    out.metric(
        "ix_team_exporter_history_gaps_total",
        "",
        state.history_gaps as f64,
        true,
    )?;
    out.metric(
        "ix_team_exporter_history_available_from_timestamp_seconds",
        "",
        state
            .history_from_ms
            .unwrap_or(now)
            .max(state.journal_from_ms) as f64
            / 1000.0,
        false,
    )?;
    for scope in &config.scopes {
        let labels = format!(
            "team_id=\"{}\",team_key=\"{}\",scope_id=\"{}\"",
            scope.team_id,
            scope.selectors.team.as_deref().unwrap_or(""),
            scope.scope_id
        );
        let Some(s) = state.scopes.get(&scope.key()) else {
            continue;
        };
        if s.comparable_since_ms > 0 {
            out.metric(
                "ix_team_scope_comparable_since_timestamp_seconds",
                &labels,
                s.comparable_since_ms as f64 / 1000.0,
                false,
            )?;
        }
        let tracker_fresh = s.tracker_ok
            && now.saturating_sub(s.tracker_at_ms)
                <= i64::try_from(config.tracker_interval_seconds * 2000).unwrap_or(i64::MAX);
        let life_fresh = s.lifecycle_ok
            && now.saturating_sub(s.lifecycle_success_at_ms)
                <= i64::try_from(config.activity_interval_seconds * 3000).unwrap_or(i64::MAX);
        let history_fresh = state.history_ok
            && now.saturating_sub(state.history_at_ms)
                <= i64::try_from(config.activity_interval_seconds * 3000).unwrap_or(i64::MAX);
        let github_fresh = s.github_observed_at_ms > 0
            && (0..super::github::TTL_MS).contains(&now.saturating_sub(s.github_observed_at_ms));
        let pr_fresh = tracker_fresh
            && s.pr_success_at_ms > 0
            && (0..super::github::TTL_MS).contains(&now.saturating_sub(s.pr_success_at_ms));
        let review_fresh = tracker_fresh
            && s.review_success_at_ms > 0
            && (0..super::github::TTL_MS).contains(&now.saturating_sub(s.review_success_at_ms));
        for (source, ok, at) in [
            ("tracker", tracker_fresh, s.tracker_at_ms),
            ("lifecycle", life_fresh, s.lifecycle_success_at_ms),
            ("history", history_fresh, state.history_at_ms),
            (
                "github_prs",
                pr_fresh && s.pr_stats.is_some(),
                s.pr_success_at_ms,
            ),
            (
                "review_records",
                review_fresh && s.review_stats.is_some(),
                s.review_success_at_ms,
            ),
        ] {
            let l = format!("{labels},source=\"{source}\"");
            out.metric("ix_team_source_available", &l, flag(ok), false)?;
            if at > 0 {
                out.metric(
                    "ix_team_source_last_success_timestamp_seconds",
                    &l,
                    at as f64 / 1000.0,
                    false,
                )?;
            }
        }
        if tracker_fresh {
            if let Some(n) = s.ready_issues {
                out.metric("ix_team_ready_issues", &labels, n as f64, false)?;
            }
            if let Some(n) = s.blocked_issues {
                out.metric("ix_team_blocked_issues", &labels, n as f64, false)?;
            }
            out.metric(
                "ix_team_scope_members",
                &labels,
                s.members.len() as f64,
                false,
            )?;
        }
        let hosts: BTreeSet<_> = s.bindings.values().map(|b| b.machine.as_str()).collect();
        for host in hosts {
            // Relay host IDs are validated again before becoming a metric label.
            if !host.is_empty()
                && host.len() <= 64
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                out.metric(
                    "ix_team_host_info",
                    &format!("{labels},machine=\"{host}\""),
                    1.0,
                    false,
                )?;
            }
        }
        out.metric(
            "ix_team_execution_seconds_total",
            &labels,
            *s.totals.get("execution_seconds").unwrap_or(&0.0),
            true,
        )?;
        out.metric(
            "ix_team_observed_agent_seconds_total",
            &labels,
            *s.totals.get("observed_agent_seconds").unwrap_or(&0.0),
            true,
        )?;
        out.metric(
            "ix_team_observation_covered_seconds_total",
            &labels,
            s.covered_seconds,
            true,
        )?;
        if tracker_fresh && history_fresh {
            out.metric(
                "ix_team_assigned_workers",
                &labels,
                s.assignments
                    .values()
                    .filter(|a| s.members.values().any(|m| m.ticket == a.assignment.ticket))
                    .count() as f64,
                false,
            )?;
        }
        let execution_observation_fresh = s.execution_observed_at_ms > 0
            && (0..=90000).contains(&now.saturating_sub(s.execution_observed_at_ms));
        let expected_leads = if scope.derive_leads_from_relay {
            s.relay_leads_expected
        } else {
            scope.leads.len()
        };
        let execution_coverage = if execution_observation_fresh && expected_leads > 0 {
            (s.execution_observed_count as f64 / expected_leads as f64).min(1.0)
        } else {
            0.0
        };
        let executing_agents = (execution_observation_fresh && s.execution_observed_count > 0)
            .then_some(s.execution_working_count as f64);
        coverage(&mut out, &labels, "executing_agents", execution_coverage)?;
        out.metric(
            "ix_team_metric_available",
            &format!("{labels},metric=\"executing_agents\""),
            flag(executing_agents.is_some()),
            false,
        )?;
        if let Some(count) = executing_agents {
            out.metric("ix_team_executing_agents", &labels, count, false)?;
        }
        let paused = !s.pauses.is_empty()
            && scope
                .leads
                .iter()
                .all(|lead| s.pauses.get(&lead.to_lowercase()) == Some(&true));
        out.metric("ix_team_explicitly_paused", &labels, flag(paused), false)?;
        if s.population_observed && now.saturating_sub(s.lifecycle_at_ms) <= 90000 {
            out.metric(
                "ix_team_unattributed_executing_agents",
                &labels,
                s.unattributed_executing_agents as f64,
                false,
            )?;
        }
        if tracker_fresh {
            out.metric(
                "ix_team_scope_leaf_remaining",
                &labels,
                s.members
                    .values()
                    .filter(|m| m.leaf && m.state != crate::WorkflowStateType::Completed)
                    .count() as f64,
                false,
            )?;
            out.metric(
                "ix_team_scope_leaf_completed",
                &labels,
                s.members
                    .values()
                    .filter(|m| m.leaf && m.state == crate::WorkflowStateType::Completed)
                    .count() as f64,
                false,
            )?;
        }
        for kind in [
            "scope_added",
            "scope_removed",
            "canceled",
            "reopened",
            "scope_revisions",
            "reported_scope_revisions",
            "state_changes",
            "reported_completions",
            "split_or_reparented",
        ] {
            out.metric(
                "ix_team_scope_changes_total",
                &format!("{labels},change=\"{kind}\""),
                *s.totals.get(kind).unwrap_or(&0.0),
                true,
            )?;
        }
        for stage in [
            "coding",
            "review",
            "fix",
            "gate",
            "build_queue",
            "decision",
            "other",
        ] {
            for status in ["assigned", "waiting"] {
                let value = *s
                    .totals
                    .get(&format!("assignment_seconds:{stage}:{status}"))
                    .unwrap_or(&0.0);
                out.metric(
                    "ix_team_assignment_seconds_total",
                    &format!("{labels},stage=\"{stage}\",state=\"{status}\""),
                    value,
                    true,
                )?;
            }
            if let Some(values) = s.stages.get(stage).filter(|v| !v.is_empty()) {
                let mut sorted: Vec<_> = values.iter().map(|v| v.1).collect();
                sorted.sort_by(f64::total_cmp);
                out.metric(
                    "ix_team_recent_stage_duration_count",
                    &format!("{labels},stage=\"{stage}\""),
                    values.len() as f64,
                    false,
                )?;
                for (stat, fraction) in [("median", 0.5), ("p90", 0.9)] {
                    let rank = (fraction * sorted.len() as f64).ceil() as usize;
                    if let Some(value) = sorted.get(rank.saturating_sub(1)) {
                        out.metric(
                            "ix_team_recent_stage_duration_seconds",
                            &format!("{labels},stage=\"{stage}\",statistic=\"{stat}\""),
                            *value,
                            false,
                        )?;
                    }
                }
            }
            let active = s
                .assignments
                .values()
                .filter(|a| {
                    tracker_fresh
                        && history_fresh
                        && super::timeline::stage(&a.assignment.role) == stage
                        && s.members.values().any(|m| m.ticket == a.assignment.ticket)
                })
                .collect::<Vec<_>>();
            let oldest = active
                .iter()
                .filter(|a| a.start_known)
                .map(|a| now.saturating_sub(a.started_ms).max(0))
                .max();
            let age = if active.is_empty() {
                (tracker_fresh && history_fresh && life_fresh && s.population_observed)
                    .then_some(0.0)
            } else {
                oldest.map(|age| age as f64 / 1000.0)
            };
            let known = active.iter().filter(|a| a.start_known).count();
            let age_coverage = if active.is_empty() {
                if age.is_some() { 1.0 } else { 0.0 }
            } else {
                known as f64 / active.len() as f64
            };
            coverage(
                &mut out,
                &labels,
                &format!("stage_oldest_age_seconds_{stage}"),
                age_coverage,
            )?;
            out.metric(
                "ix_team_metric_available",
                &format!("{labels},metric=\"stage_oldest_age_seconds\",stage=\"{stage}\""),
                flag(age.is_some()),
                false,
            )?;
            if let Some(age) = age {
                out.metric(
                    "ix_team_stage_oldest_age_seconds",
                    &format!("{labels},stage=\"{stage}\""),
                    age,
                    false,
                )?;
            }
        }
        for (id, name, factor) in LEGACY {
            let metric = s.record.as_ref().and_then(|r| r.metrics.get(*id));
            let value = metric
                .filter(|m| {
                    m.status == Status::Ok
                        && tracker_fresh
                        && (!m.source.split('+').any(|s| s == "github") || github_fresh)
                })
                .and_then(|m| m.value);
            out.metric(
                "ix_team_metric_available",
                &format!("{labels},metric=\"{name}\""),
                flag(value.is_some()),
                false,
            )?;
            if let Some(value) = value {
                out.metric(&format!("ix_team_{name}"), &labels, value * factor, false)?;
            }
        }
        for (id, name, factor) in DEPARTMENT {
            let m = s
                .record
                .as_ref()
                .and_then(|r| r.department.as_ref())
                .and_then(|d| d.measurements.get(*id));
            let value = m
                .filter(|m| m.status == Status::Ok && tracker_fresh)
                .and_then(|m| match m.value.as_ref() {
                    Some(Value::Scalar(v)) => Some(*v),
                    _ => None,
                });
            out.metric(
                "ix_team_metric_available",
                &format!("{labels},metric=\"{name}\""),
                flag(value.is_some()),
                false,
            )?;
            if let Some(value) = value {
                out.metric(&format!("ix_team_{name}"), &labels, value * factor, false)?;
            }
        }
        for (names, values) in [
            (
                super::pr_stats::NAMES,
                s.pr_stats.as_ref().filter(|_| pr_fresh).map(|p| &p.values),
            ),
            (
                super::reviews::NAMES,
                s.review_stats
                    .as_ref()
                    .filter(|_| review_fresh)
                    .map(|p| &p.values),
            ),
        ] {
            for name in names {
                let value = values.filter(|_| tracker_fresh).and_then(|v| v.get(*name));
                if *name == "pr_changes_requested_share" {
                    let ratio = values
                        .and_then(|v| v.get("pr_changes_requested_coverage"))
                        .copied()
                        .unwrap_or(0.0);
                    coverage(&mut out, &labels, name, ratio)?;
                } else if *name == "new_pr_first_review_median_seconds_24h" {
                    let ratio = values
                        .and_then(|v| v.get("new_pr_first_review_coverage_24h"))
                        .copied()
                        .unwrap_or(0.0);
                    coverage(&mut out, &labels, name, ratio)?;
                }
                let value = value.filter(|_| {
                    !name.starts_with("review_findings_")
                        || values
                            .and_then(|v| v.get("review_recorded_prs_24h"))
                            .is_some_and(|n| *n > 0.0)
                });
                out.metric(
                    "ix_team_metric_available",
                    &format!("{labels},metric=\"{name}\""),
                    flag(value.is_some()),
                    false,
                )?;
                if let Some(value) = value {
                    out.metric(&format!("ix_team_{name}"), &labels, *value, false)?;
                }
            }
        }
        let verified = s
            .record
            .as_ref()
            .filter(|_| tracker_fresh && github_fresh)
            .and_then(|r| {
                let closed = r.metrics.get("path_closed")?.value?;
                let unverified = r.metrics.get("path_closed_unverified")?.value?;
                (closed >= unverified).then_some(closed - unverified)
            });
        let effort = verified
            .zip(
                s.effort_points
                    .iter()
                    .rev()
                    .find(|p| p.0 >= now - 86400000 && p.0 <= now - 86100000),
            )
            .and_then(|(closures, (_, execution, covered))| {
                (closures > 0.0
                    && life_fresh
                    && history_fresh
                    && s.covered_seconds - covered >= 86100.0)
                    .then(|| {
                        (s.totals.get("execution_seconds").unwrap_or(&0.0) - execution) / closures
                    })
            });
        let effort_coverage = s
            .effort_points
            .iter()
            .rev()
            .find(|p| p.0 >= now - 86400000 && p.0 <= now - 86100000)
            .map(|(_, _, covered)| ((s.covered_seconds - covered).max(0.0) / 86400.0).min(1.0))
            .unwrap_or(0.0);
        coverage(
            &mut out,
            &labels,
            "effort_per_verified_path_closure_seconds_24h",
            effort_coverage,
        )?;
        out.metric(
            "ix_team_metric_available",
            &format!("{labels},metric=\"effort_per_verified_path_closure_seconds_24h\""),
            flag(effort.is_some()),
            false,
        )?;
        if let Some(value) = verified {
            out.metric("ix_team_verified_path_closed_24h", &labels, value, false)?;
        }
        if let Some(value) = effort {
            out.metric(
                "ix_team_effort_per_verified_path_closure_seconds_24h",
                &labels,
                value,
                false,
            )?;
        }
        for days_ago in 0..7 {
            let closures = tracker_fresh
                .then_some(s.closures_daily.as_ref())
                .flatten()
                .and_then(|values| values.get(days_ago));
            let merges = github_fresh
                .then_some(s.merged_prs_daily.as_ref())
                .flatten()
                .and_then(|values| values.get(days_ago));
            coverage(
                &mut out,
                &labels,
                &format!("closures_day_{days_ago}"),
                flag(closures.is_some()),
            )?;
            coverage(
                &mut out,
                &labels,
                &format!("merged_prs_day_{days_ago}"),
                flag(merges.is_some()),
            )?;
            if let Some(value) = closures {
                out.metric(
                    "ix_team_closures_day",
                    &format!("{labels},days_ago=\"{days_ago}\""),
                    *value as f64,
                    false,
                )?;
            }
            if let Some(value) = merges {
                out.metric(
                    "ix_team_merged_prs_day",
                    &format!("{labels},days_ago=\"{days_ago}\""),
                    *value as f64,
                    false,
                )?;
            }
        }
        if tracker_fresh && let Some(seconds) = s.build_execution_seconds_24h {
            out.metric(
                "ix_team_build_execution_seconds_24h",
                &labels,
                seconds,
                false,
            )?;
        }
    }
    let count = out.count + 1;
    out.metric("ix_team_exporter_series", "", count as f64, false)?;
    Ok(out.text)
}
