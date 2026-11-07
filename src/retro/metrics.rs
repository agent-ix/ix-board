// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Computes the metric catalog from the read-only sources (FR-024, FR-026).
//!
//! Every source is read at most once per run. A metric whose source failed, was incomplete or
//! is not configured is `unavailable` with a reason and a null value, never zero.

use std::collections::{BTreeMap, BTreeSet};

use super::catalog::{CATALOG, Metric, Status, definition};
use super::config::{RetroConfig, format_window};
use super::sources::{
    AGENTS_IDLE_AFTER_SECS, CheckState, GateLine, GithubSource, LinearSource, MainCommit,
    PullRequest, Sources, StateChange,
};
use crate::chain::evaluate_chain;
use crate::model::issue_is_open;
use crate::{Issue, IssueId, WorkflowStateType, WorkspaceSnapshot};

const NOT_CONFIGURED: &str = "not configured";
const EMPTY: &str = "empty population";

/// A computed value, or the reason it is unavailable.
type Outcome = Result<f64, String>;

/// Window and clock one run measures with.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Snapshot time, Unix seconds.
    pub now: f64,
    /// Window length in seconds.
    pub window_secs: u64,
    /// True for the half-open window `[now - window, now)` a backfilled day uses; false for the
    /// snapshot window `(now - window, now]`.
    pub end_exclusive: bool,
}

impl Frame {
    fn start(self) -> f64 {
        self.now - self.window_secs as f64
    }

    fn contains(self, at: f64) -> bool {
        if self.end_exclusive {
            at >= self.start() && at < self.now
        } else {
            at > self.start() && at <= self.now
        }
    }

    fn hours(self) -> f64 {
        self.window_secs as f64 / 3600.0
    }
}

/// Computes every catalog metric once.
pub fn collect(
    config: &RetroConfig,
    frame: Frame,
    sources: &Sources<'_>,
) -> BTreeMap<String, Metric> {
    let mut out = Out {
        config,
        frame,
        metrics: BTreeMap::new(),
    };
    let stale_hours = config.thresholds.in_progress_stale_hours.unwrap_or(0);
    // The stale rule looks back further than the window when its threshold is larger.
    let since = frame
        .start()
        .min(frame.now - stale_hours as f64 * 3600.0)
        .max(0.0);
    let workspace = sources.linear.workspace(since);
    let mut history = History {
        source: sources.linear,
        cache: BTreeMap::new(),
    };
    let prs = list_pull_requests(config, sources.github, frame.start());
    match &workspace {
        Ok(snapshot) => {
            let scope = Scope::new(config, snapshot, frame);
            progress(&mut out, &scope, &mut history, sources);
            rework_issues(&mut out, &scope, &mut history);
            issue_scope(&mut out, &scope, &mut history, sources);
        }
        Err(error) => {
            let reason = format!("Linear unavailable: {error}");
            for id in LINEAR_SNAPSHOT_IDS {
                out.unavailable(id, reason.clone());
            }
        }
    }
    decisions(&mut out, sources);
    fix_rounds(&mut out, sources, &prs);
    flow(&mut out, &prs);
    stability(&mut out, sources);
    utilization(&mut out, sources);
    resources(&mut out, sources);
    gate_log(&mut out, sources);
    for entry in CATALOG {
        debug_assert!(
            out.metrics.contains_key(entry.id),
            "{} not computed",
            entry.id
        );
    }
    out.metrics
}

const LINEAR_SNAPSHOT_IDS: [&str; 11] = [
    "path_remaining",
    "path_closed",
    "intake_new",
    "net_closed_minus_new",
    "gates_remaining",
    "path_closed_unverified",
    "done_reopened",
    "issues_without_project",
    "issues_without_milestone",
    "in_progress_stale",
    "source_count_disagreement",
];

struct Out<'a> {
    config: &'a RetroConfig,
    frame: Frame,
    metrics: BTreeMap<String, Metric>,
}

impl Out<'_> {
    fn put(&mut self, id: &'static str, outcome: Outcome) {
        let Some(entry) = definition(id) else {
            return;
        };
        let (source, query) = self.describe(id);
        let (status, value, reason) = match outcome {
            Ok(value) => (Status::Ok, Some(value), None),
            Err(reason) => (Status::Unavailable, None, Some(reason)),
        };
        self.metrics.insert(
            id.to_owned(),
            Metric {
                id: id.to_owned(),
                status,
                value,
                unit: entry.unit.to_owned(),
                direction: entry.direction,
                source: source.to_owned(),
                query,
                reason,
            },
        );
    }

    fn unavailable(&mut self, id: &'static str, reason: String) {
        if definition(id).is_some() {
            self.put(id, Err(reason));
        }
    }

    fn not_configured(&mut self, id: &'static str) {
        self.unavailable(id, NOT_CONFIGURED.to_owned());
    }

    /// The source name and counting rule (with window and thresholds) of one metric.
    fn describe(&self, id: &str) -> (&'static str, String) {
        let window = format!("window={}", format_window(self.frame.window_secs));
        let t = &self.config.thresholds;
        let threshold = |value: Option<u64>| value.map_or("unset".to_owned(), |v| v.to_string());
        let gates = self.config.gate_tickets.join(",");
        let (source, rule): (&'static str, String) = match id {
            "path_remaining" => (
                "linear",
                format!("open issues transitively blocking a gate ticket; gates={gates}"),
            ),
            "path_closed" => (
                "linear",
                format!("path issues with a state change to completed in the window; gates={gates}; {window}"),
            ),
            "intake_new" => ("linear", format!("issues created in the window in the configured projects; {window}")),
            "net_closed_minus_new" => (
                "linear",
                format!("project and path issues completed in the window (counted once) minus intake_new; {window}"),
            ),
            "gates_remaining" => ("linear", format!("gate tickets not completed; gates={gates}")),
            "path_closed_unverified" => (
                "linear+github",
                format!("path issues completed in the window with no merged pull request attached; gates={gates}; {window}"),
            ),
            "prs_merged_per_hour" => ("github", format!("pull requests merged in the window divided by window hours; {window}")),
            "pr_cycle_median_hours" => ("github", format!("median (nearest rank) creation-to-merge hours of pull requests merged in the window; {window}")),
            "pr_cycle_p90_hours" => ("github", format!("90th percentile (nearest rank) creation-to-merge hours of pull requests merged in the window; {window}")),
            "prs_open" => ("github", "open pull requests".to_owned()),
            "prs_idle" => ("github", format!("open pull requests with no update for pr_idle_hours={}", threshold(t.pr_idle_hours))),
            "prs_oversize" => ("github", format!("open pull requests with added plus deleted lines above pr_oversize_lines={}", threshold(t.pr_oversize_lines))),
            "prs_ready_unmerged" => ("github", "open non-draft pull requests with review decision APPROVED".to_owned()),
            "fix_rounds_max" => ("linear+github", format!("highest round= of reviewer-dispositions markers per pull request open or merged in the window, maximum; first-line markers only; {window}")),
            "fix_rounds_total" => ("linear+github", format!("highest round= of reviewer-dispositions markers per pull request open or merged in the window, summed; first-line markers only; {window}")),
            "head_force_pushes_max" => ("github", format!("largest number of force-push events on one pull request that is open or was merged in the window; {window}")),
            "done_reopened" => ("linear", format!("issues with a state change from completed to a non-completed state in the window; {window}")),
            "main_red_incidents" => ("github", self.main_rule("passing-to-failing transitions of the main branch across commits in the window")),
            "main_red_minutes" => ("github", self.main_rule("minutes from each incident's first failing commit to the next passing commit, or to the snapshot time while failing")),
            "issues_without_project" => ("linear", format!("open issues of linear_teams={} lacking a project", self.config.linear_teams.join(","))),
            "issues_without_milestone" => ("linear", format!("open issues of linear_teams={} lacking a milestone", self.config.linear_teams.join(","))),
            "in_progress_stale" => ("linear+github", format!("started issues with no state change and no attached pull request update for in_progress_stale_hours={}", threshold(t.in_progress_stale_hours))),
            "source_count_disagreement" => ("linear", "absolute difference between the open-issue count of the configured projects from the issue snapshot and the project-chunk read".to_owned()),
            "agents_working_share" => ("herdr", format!("agents with verdict WORKING (idle_after={}m) over agents reported for herdr_teams={}", AGENTS_IDLE_AFTER_SECS / 60, self.config.herdr_teams.join(","))),
            "agents_idle_minutes_max" => ("herdr", format!("largest minutes since the last turn among agents of herdr_teams={} (idle_after={}m)", self.config.herdr_teams.join(","), AGENTS_IDLE_AFTER_SECS / 60)),
            "worktrees_prunable" => ("local git", "prunable worktrees git reports for each directory of local_paths".to_owned()),
            "disk_bytes" => ("local disk", "total bytes below each directory of local_paths".to_owned()),
            "orphan_slots" => ("local processes", format!("processes whose command contains slot_process={:?}, whose parent exited, older than orphan_slot_hours={}", self.config.slot_process.clone().unwrap_or_default(), threshold(self.config.orphan_slot_hours))),
            "full_gates_max" => ("gate log", format!("largest number of gate lines for one pr among lines finished in the window; {window}")),
            "repins_total" => ("gate log", format!("repin lines finished in the window; {window}")),
            "build_lock_wait_minutes" => ("gate log", format!("sum of acquired_lock_at minus requested_lock_at over lines finished in the window; {window}")),
            "messages_undelivered" => ("ix-relay", format!("messages pending longer than message_stale_minutes={} in the queues of relay_leads", threshold(self.config.message_stale_minutes))),
            "decisions_open" => ("linear", format!("open issues carrying decision_label={:?}", self.config.decision_label.clone().unwrap_or_default())),
            "decision_oldest_hours" => ("linear", format!("age in hours of the oldest open issue carrying decision_label={:?}", self.config.decision_label.clone().unwrap_or_default())),
            _ => ("unknown", String::new()),
        };
        (source, rule)
    }

    fn main_rule(&self, rule: &str) -> String {
        let target = self
            .config
            .main_check
            .as_ref()
            .map_or("unset".to_owned(), |check| {
                format!("{}@{}", check.repo, check.branch)
            });
        format!(
            "{rule}; check-runs of main_check={target}; window={}",
            format_window(self.frame.window_secs)
        )
    }
}

/// Configured projects, their issues and the gate path in one Linear snapshot.
struct Scope<'a> {
    config: &'a RetroConfig,
    frame: Frame,
    snapshot: &'a WorkspaceSnapshot,
    project_issues: Vec<&'a Issue>,
    project_error: Option<String>,
    path: Result<Vec<&'a Issue>, String>,
}

impl<'a> Scope<'a> {
    fn new(config: &'a RetroConfig, snapshot: &'a WorkspaceSnapshot, frame: Frame) -> Self {
        let mut project_ids = BTreeSet::new();
        let mut project_error = None;
        for name in &config.projects {
            let found = snapshot
                .projects
                .iter()
                .filter(|project| project.name.eq_ignore_ascii_case(name))
                .map(|project| project.id.clone())
                .collect::<Vec<_>>();
            if found.is_empty() {
                project_error
                    .get_or_insert_with(|| format!("project {name:?} not found in Linear"));
            }
            project_ids.extend(found);
        }
        let project_issues = snapshot
            .issues
            .iter()
            .filter(|issue| {
                issue
                    .project_id
                    .as_ref()
                    .is_some_and(|id| project_ids.contains(id))
            })
            .collect();
        Self {
            config,
            frame,
            snapshot,
            project_issues,
            project_error,
            path: path_issues(config, snapshot, frame),
        }
    }

    fn projects_ok(&self) -> Result<&[&'a Issue], String> {
        match &self.project_error {
            Some(error) => Err(format!("Linear unavailable: {error}")),
            None => Ok(&self.project_issues),
        }
    }

    fn path_ok(&self) -> Result<&[&'a Issue], String> {
        self.path.as_deref().map_err(Clone::clone)
    }
}

/// Issues that transitively block a gate ticket, from the blocker-chain expansion.
fn path_issues<'a>(
    config: &RetroConfig,
    snapshot: &'a WorkspaceSnapshot,
    frame: Frame,
) -> Result<Vec<&'a Issue>, String> {
    let mut starts = Vec::new();
    for gate in &config.gate_tickets {
        let issue = snapshot
            .issues
            .iter()
            .find(|issue| issue.identifier.eq_ignore_ascii_case(gate))
            .ok_or_else(|| format!("Linear unavailable: gate ticket {gate} not found"))?;
        starts.push(issue.id.clone());
    }
    let report = evaluate_chain(snapshot, &starts, None, frame.now)
        .map_err(|error| format!("Linear unavailable: blocker expansion failed: {error}"))?;
    if !report.warnings.is_empty() {
        return Err(format!(
            "Linear unavailable: blocker expansion truncated or partial: {}",
            report.warnings.join("; ")
        ));
    }
    let edges = report
        .nodes
        .iter()
        .map(|node| (node.identifier.as_str(), &node.blocked_by))
        .collect::<BTreeMap<_, _>>();
    let mut reached = BTreeSet::new();
    let mut stack = report.starts.iter().map(String::as_str).collect::<Vec<_>>();
    while let Some(identifier) = stack.pop() {
        for blocker in edges
            .get(identifier)
            .into_iter()
            .flat_map(|list| list.iter())
        {
            if reached.insert(blocker.as_str()) {
                stack.push(blocker);
            }
        }
    }
    let mut issues = snapshot
        .issues
        .iter()
        .filter(|issue| reached.contains(issue.identifier.as_str()))
        .collect::<Vec<_>>();
    issues.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    Ok(issues)
}

/// Lazily read, memoized issue state history: one read per issue per run.
struct History<'a> {
    source: &'a dyn super::sources::LinearSource,
    cache: BTreeMap<IssueId, Result<Vec<StateChange>, String>>,
}

impl History<'_> {
    fn get(&mut self, issue: &Issue) -> Result<&[StateChange], String> {
        let source = self.source;
        self.cache
            .entry(issue.id.clone())
            .or_insert_with(|| {
                source.state_history(&issue.id.0).map_err(|error| {
                    format!(
                        "Linear unavailable: history of {}: {error}",
                        issue.identifier
                    )
                })
            })
            .as_deref()
            .map_err(Clone::clone)
    }

    /// Reads, in one bounded-concurrency batch, the histories of the issues updated at or
    /// after `since`. An issue not updated since then cannot have changed state since, so its
    /// history is not needed. Later `get` calls hit the cache.
    fn prefetch(&mut self, issues: &[&Issue], since: f64) {
        let mut wanted = issues
            .iter()
            .filter(|issue| !self.cache.contains_key(&issue.id))
            .filter(|issue| {
                !issue
                    .updated_at
                    .as_deref()
                    .and_then(epoch)
                    .is_some_and(|at| at < since)
            })
            .map(|issue| (*issue).clone())
            .collect::<Vec<_>>();
        wanted.sort_by(|left, right| left.id.cmp(&right.id));
        wanted.dedup_by(|left, right| left.id == right.id);
        let ids = wanted
            .iter()
            .map(|issue| issue.id.0.clone())
            .collect::<Vec<_>>();
        let results = self.source.state_histories(&ids);
        for (issue, result) in wanted.into_iter().zip(results) {
            self.cache.insert(
                issue.id.clone(),
                result.map_err(|error| {
                    format!(
                        "Linear unavailable: history of {}: {error}",
                        issue.identifier
                    )
                }),
            );
        }
    }

    /// Issues with a state change inside the window that satisfies `matches`. An issue not
    /// updated since the window opened cannot have changed state inside it, so its history is
    /// not read.
    fn changed_in_window(
        &mut self,
        issues: &[&Issue],
        frame: Frame,
        matches: impl Fn(&StateChange) -> bool,
    ) -> Result<BTreeSet<IssueId>, String> {
        let mut found = BTreeSet::new();
        for issue in issues {
            if issue
                .updated_at
                .as_deref()
                .and_then(epoch)
                .is_some_and(|at| at < frame.start())
            {
                continue;
            }
            if self
                .get(issue)?
                .iter()
                .any(|change| frame.contains(change.at) && matches(change))
            {
                found.insert(issue.id.clone());
            }
        }
        Ok(found)
    }
}

fn epoch(text: &str) -> Option<f64> {
    crate::drift::timestamp_epoch(text).ok()
}

fn to_completed(change: &StateChange) -> bool {
    change.to == Some(WorkflowStateType::Completed)
}

/// `count * unit` seconds, or a reason when the product does not fit.
fn span_secs(count: u64, unit: u64, what: &str) -> Result<f64, String> {
    count
        .checked_mul(unit)
        .map(|seconds| seconds as f64)
        .ok_or_else(|| format!("{what} is too large to measure"))
}

fn count(value: usize) -> f64 {
    value as f64
}

fn progress(
    out: &mut Out<'_>,
    scope: &Scope<'_>,
    history: &mut History<'_>,
    sources: &Sources<'_>,
) {
    let path_done = window_progress(out, scope, history);
    out.put(
        "path_remaining",
        scope
            .path_ok()
            .map(|path| count(path.iter().filter(|issue| issue_is_open(issue)).count())),
    );
    let gates = gate_issues(scope).map(|gates| {
        count(
            gates
                .iter()
                .filter(|issue| issue.state_type != WorkflowStateType::Completed)
                .count(),
        )
    });
    out.put("gates_remaining", gates);
    let unverified = path_done.and_then(|done| {
        let candidates = scope
            .path_ok()?
            .iter()
            .filter(|issue| done.contains(&issue.id))
            .copied()
            .collect::<Vec<_>>();
        unverified_count(&candidates, sources)
    });
    out.put("path_closed_unverified", unverified);
}

/// `path_closed`, `intake_new` and `net_closed_minus_new`: the progress metrics that depend only
/// on issue creation and state history inside the frame. Returns the path issues completed in
/// the frame.
fn window_progress(
    out: &mut Out<'_>,
    scope: &Scope<'_>,
    history: &mut History<'_>,
) -> Result<BTreeSet<IssueId>, String> {
    let frame = scope.frame;
    let mut wanted = scope.path_ok().unwrap_or_default().to_vec();
    wanted.extend(scope.projects_ok().unwrap_or_default().iter().copied());
    history.prefetch(&wanted, frame.start());
    let path_done = scope
        .path_ok()
        .and_then(|path| history.changed_in_window(path, frame, to_completed));
    out.put(
        "path_closed",
        path_done.clone().map(|done| count(done.len())),
    );
    let created = scope.projects_ok().map(|issues| {
        issues
            .iter()
            .filter(|issue| {
                issue
                    .created_at
                    .as_deref()
                    .and_then(epoch)
                    .is_some_and(|at| frame.contains(at))
            })
            .count()
    });
    out.put("intake_new", created.clone().map(count));
    let net = (|| {
        let mut union = scope.projects_ok()?.to_vec();
        union.extend(scope.path_ok()?.iter().copied());
        union.sort_by(|left, right| left.id.cmp(&right.id));
        union.dedup_by(|left, right| left.id == right.id);
        let closed = history.changed_in_window(&union, frame, to_completed)?;
        Ok::<_, String>(count(closed.len()) - count(created.clone()?))
    })();
    out.put("net_closed_minus_new", net);
    path_done
}

fn gate_issues<'a>(scope: &Scope<'a>) -> Result<Vec<&'a Issue>, String> {
    scope
        .config
        .gate_tickets
        .iter()
        .map(|gate| {
            scope
                .snapshot
                .issues
                .iter()
                .find(|issue| issue.identifier.eq_ignore_ascii_case(gate))
                .ok_or_else(|| format!("Linear unavailable: gate ticket {gate} not found"))
        })
        .collect()
}

/// Splits a GitHub pull request URL into repository and number.
fn pull_request_ref(url: &str) -> Option<(String, u64)> {
    let rest = url.split("github.com/").nth(1)?;
    let mut parts = rest.split('/');
    let (owner, name, kind, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (kind == "pull").then_some(())?;
    let number = number.split(['?', '#']).next()?.parse().ok()?;
    Some((format!("{owner}/{name}"), number))
}

fn attached_pull_requests(issue: &Issue) -> Vec<(String, u64)> {
    issue
        .attachment_urls
        .iter()
        .filter_map(|url| pull_request_ref(url))
        .collect()
}

fn unverified_count(candidates: &[&Issue], sources: &Sources<'_>) -> Outcome {
    let mut unverified = 0;
    for issue in candidates {
        if !issue.attachments_complete {
            return Err(format!(
                "Linear unavailable: attachments of {} were not fully read",
                issue.identifier
            ));
        }
        let mut merged = false;
        for (repo, number) in attached_pull_requests(issue) {
            merged |= sources
                .github
                .pull_request(&repo, number)
                .map_err(|error| format!("GitHub unavailable: {repo}#{number}: {error}"))?
                .merged;
        }
        unverified += usize::from(!merged);
    }
    Ok(count(unverified))
}

fn rework_issues(out: &mut Out<'_>, scope: &Scope<'_>, history: &mut History<'_>) {
    let frame = scope.frame;
    let reopened = (|| {
        let mut union = scope.projects_ok()?.to_vec();
        union.extend(scope.path_ok()?.iter().copied());
        union.sort_by(|left, right| left.id.cmp(&right.id));
        union.dedup_by(|left, right| left.id == right.id);
        let reopened = history.changed_in_window(&union, frame, |change| {
            change.from == Some(WorkflowStateType::Completed)
                && change
                    .to
                    .is_some_and(|to| to != WorkflowStateType::Completed)
        })?;
        Ok::<_, String>(count(reopened.len()))
    })();
    out.put("done_reopened", reopened);
}

fn issue_scope(
    out: &mut Out<'_>,
    scope: &Scope<'_>,
    history: &mut History<'_>,
    sources: &Sources<'_>,
) {
    let config = scope.config;
    let teams = config
        .linear_teams
        .iter()
        .map(|team| team.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    let open_in_teams = scope
        .snapshot
        .issues
        .iter()
        .filter(|issue| {
            issue_is_open(issue) && teams.contains(&issue.team_key.0.to_ascii_uppercase())
        })
        .collect::<Vec<_>>();
    out.put(
        "issues_without_project",
        Ok(count(
            open_in_teams
                .iter()
                .filter(|issue| issue.project_id.is_none())
                .count(),
        )),
    );
    out.put(
        "issues_without_milestone",
        Ok(count(
            open_in_teams
                .iter()
                .filter(|issue| issue.project_milestone_id.is_none())
                .count(),
        )),
    );
    let disagreement = (|| {
        let snapshot_open = scope
            .projects_ok()?
            .iter()
            .filter(|issue| issue_is_open(issue))
            .count();
        let mut chunk_open = 0;
        for project in &config.projects {
            chunk_open += sources
                .linear
                .project_open_count(project)
                .map_err(|error| {
                    format!("Linear unavailable: project-chunk read of {project}: {error}")
                })?;
        }
        Ok::<_, String>(count(snapshot_open.abs_diff(chunk_open)))
    })();
    out.put("source_count_disagreement", disagreement);
    let stale = match config.thresholds.in_progress_stale_hours {
        None => Err(NOT_CONFIGURED.to_owned()),
        Some(hours) => stale_count(scope, history, sources, &open_in_teams, hours),
    };
    out.put("in_progress_stale", stale);
}

fn stale_count(
    scope: &Scope<'_>,
    history: &mut History<'_>,
    sources: &Sources<'_>,
    open_in_teams: &[&Issue],
    hours: u64,
) -> Outcome {
    let cutoff = scope.frame.now - span_secs(hours, 3600, "in_progress_stale_hours")?;
    let mut stale = 0;
    let started = open_in_teams
        .iter()
        .filter(|issue| issue.state_type == WorkflowStateType::Started)
        .copied()
        .collect::<Vec<_>>();
    history.prefetch(&started, cutoff);
    for issue in started {
        // An issue not updated since the cutoff cannot have changed state since it.
        let updated = issue.updated_at.as_deref().and_then(epoch);
        let last_change = match updated {
            Some(at) if at <= cutoff => at,
            _ => history
                .get(issue)?
                .iter()
                .map(|change| change.at)
                .fold(f64::NEG_INFINITY, f64::max),
        };
        let last_change = if last_change.is_finite() {
            last_change
        } else {
            issue.created_at.as_deref().and_then(epoch).ok_or_else(|| {
                format!(
                    "Linear unavailable: {} has no creation time",
                    issue.identifier
                )
            })?
        };
        if last_change > cutoff {
            continue;
        }
        if !issue.attachments_complete {
            return Err(format!(
                "Linear unavailable: attachments of {} were not fully read",
                issue.identifier
            ));
        }
        // Each attached pull request is read from its own repository, whatever its state.
        let mut recent = false;
        for (repo, number) in attached_pull_requests(issue) {
            let info = sources
                .github
                .pull_request(&repo, number)
                .map_err(|error| format!("GitHub unavailable: {repo}#{number}: {error}"))?;
            recent |= info.updated_at > cutoff;
        }
        stale += usize::from(!recent);
    }
    Ok(count(stale))
}

fn decisions(out: &mut Out<'_>, sources: &Sources<'_>) {
    let Some(label) = out.config.decision_label.clone() else {
        out.not_configured("decisions_open");
        out.not_configured("decision_oldest_hours");
        return;
    };
    match sources.linear.open_issues_with_label(&label) {
        Err(error) => {
            let reason = format!("Linear unavailable: label {label:?}: {error}");
            out.unavailable("decisions_open", reason.clone());
            out.unavailable("decision_oldest_hours", reason);
        }
        Ok(issues) => {
            out.put("decisions_open", Ok(count(issues.len())));
            let oldest = issues
                .iter()
                .map(|issue| issue.created_at)
                .fold(f64::INFINITY, f64::min);
            out.put(
                "decision_oldest_hours",
                if issues.is_empty() {
                    Err(EMPTY.to_owned())
                } else {
                    Ok(((out.frame.now - oldest) / 3600.0).max(0.0))
                },
            );
        }
    }
}

/// Fields of a `<!-- reviewer ... -->` first line, or `None` for any other line.
fn marker_fields(line: &str) -> Option<(&str, BTreeMap<&str, &str>)> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .trim_end()
        .strip_suffix("-->")?;
    let mut tokens = inner.split_whitespace();
    let kind = tokens.next()?;
    matches!(kind, "reviewer" | "reviewer-dispositions").then_some(())?;
    let fields = tokens.filter_map(|token| token.split_once('=')).collect();
    Some((kind, fields))
}

/// The `name#number` key a marker's `pr=` field uses for a pull request.
fn marker_key(pr: &PullRequest) -> String {
    format!(
        "{}#{}",
        pr.repo.rsplit('/').next().unwrap_or(&pr.repo),
        pr.number
    )
}

/// Pull requests that are open or were merged inside the window.
fn relevant_prs(prs: &[PullRequest], frame: Frame) -> Vec<&PullRequest> {
    prs.iter()
        .filter(|pr| pr.open || pr.merged_at.is_some_and(|at| frame.contains(at)))
        .collect()
}

/// Per pull request (as `name#n`), the highest disposition round, from marker first lines,
/// for the pull requests in `allowed` only. A disposition counts toward a pull request when
/// its `repo=` and `id=` both match a `reviewer` marker of that pull request, and that marker's
/// `repo=` is one of `repos` and names the pull request's repository.
pub fn fix_rounds_by_pr(
    lines: &[String],
    allowed: &BTreeSet<String>,
    repos: &[String],
) -> BTreeMap<String, u64> {
    let mut prs_by_review: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    let mut rounds: Vec<((&str, &str), u64)> = Vec::new();
    for line in lines {
        let Some((kind, fields)) = marker_fields(line) else {
            continue;
        };
        let (Some(repo), Some(id)) = (fields.get("repo"), fields.get("id")) else {
            continue;
        };
        if kind == "reviewer" {
            if let Some(pr) = fields.get("pr")
                && allowed.contains(*pr)
                && repos.iter().any(|configured| configured == repo)
                && repo.rsplit('/').next() == pr.split_once('#').map(|(name, _)| name)
            {
                prs_by_review.entry((repo, id)).or_default().insert(pr);
            }
        } else if let Some(round) = fields.get("round").and_then(|round| round.parse().ok()) {
            rounds.push(((repo, id), round));
        }
    }
    let mut highest = BTreeMap::new();
    for (review, round) in rounds {
        for pr in prs_by_review.get(&review).into_iter().flatten() {
            let entry = highest.entry((*pr).to_owned()).or_insert(0);
            *entry = round.max(*entry);
        }
    }
    highest
}

/// First line of a comment body: the only part of a comment the command keeps.
pub fn first_line(body: &str) -> &str {
    body.lines().next().unwrap_or("")
}

fn fix_rounds(out: &mut Out<'_>, sources: &Sources<'_>, prs: &Result<Vec<PullRequest>, String>) {
    let frame = out.frame;
    let allowed = match prs {
        Ok(prs) => relevant_prs(prs, frame)
            .into_iter()
            .map(marker_key)
            .collect::<BTreeSet<_>>(),
        Err(error) => {
            let reason = format!("GitHub unavailable: {error}");
            out.unavailable("fix_rounds_max", reason.clone());
            out.unavailable("fix_rounds_total", reason);
            return;
        }
    };
    match marker_lines_for(sources, &allowed) {
        Err(error) => {
            let reason = format!("Linear unavailable: reviewer markers: {error}");
            out.unavailable("fix_rounds_max", reason.clone());
            out.unavailable("fix_rounds_total", reason);
        }
        Ok(lines) => {
            let rounds = fix_rounds_by_pr(&lines, &allowed, &out.config.repos);
            out.put(
                "fix_rounds_max",
                Ok(rounds.values().copied().max().unwrap_or(0) as f64),
            );
            out.put("fix_rounds_total", Ok(rounds.values().sum::<u64>() as f64));
        }
    }
}

/// Reads the `reviewer` markers of the allowed pull requests, then the dispositions of the
/// review ids those markers carry; nothing else is read.
fn marker_lines_for(
    sources: &Sources<'_>,
    allowed: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    let reviewer = allowed
        .iter()
        .map(|key| ("<!-- reviewer ".to_owned(), format!("pr={key} ")))
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    for result in sources.linear.marker_lines_batch(&reviewer) {
        lines.extend(result?);
    }
    let ids = lines
        .iter()
        .filter_map(|line| marker_fields(line))
        .filter(|(kind, _)| *kind == "reviewer")
        .filter_map(|(_, fields)| fields.get("id").map(|id| (*id).to_owned()))
        .collect::<BTreeSet<_>>();
    let dispositions = ids
        .iter()
        .map(|id| {
            (
                "<!-- reviewer-dispositions ".to_owned(),
                format!("id={id} "),
            )
        })
        .collect::<Vec<_>>();
    for result in sources.linear.marker_lines_batch(&dispositions) {
        lines.extend(result?);
    }
    Ok(lines)
}

fn list_pull_requests(
    config: &RetroConfig,
    github: &dyn GithubSource,
    since: f64,
) -> Result<Vec<PullRequest>, String> {
    let mut all = Vec::new();
    for repo in &config.repos {
        all.extend(
            github
                .pull_requests(repo, since)
                .map_err(|error| format!("{repo}: {error}"))?,
        );
    }
    Ok(all)
}

/// Nearest-rank percentile of a sorted, non-empty list.
fn nearest_rank(sorted: &[f64], fraction: f64) -> f64 {
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn flow(out: &mut Out<'_>, prs: &Result<Vec<PullRequest>, String>) {
    const IDS: [&str; 8] = [
        "prs_merged_per_hour",
        "pr_cycle_median_hours",
        "pr_cycle_p90_hours",
        "prs_open",
        "prs_idle",
        "prs_oversize",
        "prs_ready_unmerged",
        "head_force_pushes_max",
    ];
    let frame = out.frame;
    let prs = match prs {
        Ok(prs) => prs,
        Err(error) => {
            for id in &IDS {
                out.unavailable(id, format!("GitHub unavailable: {error}"));
            }
            return;
        }
    };
    let merged = prs
        .iter()
        .filter(|pr| pr.merged_at.is_some_and(|at| frame.contains(at)))
        .collect::<Vec<_>>();
    out.put(
        "prs_merged_per_hour",
        Ok(count(merged.len()) / frame.hours()),
    );
    let mut cycles = merged
        .iter()
        .filter_map(|pr| pr.merged_at.map(|at| (at - pr.created_at) / 3600.0))
        .collect::<Vec<_>>();
    cycles.sort_by(f64::total_cmp);
    for (id, fraction) in [("pr_cycle_median_hours", 0.5), ("pr_cycle_p90_hours", 0.9)] {
        out.put(
            id,
            if cycles.is_empty() {
                Err(EMPTY.to_owned())
            } else {
                Ok(nearest_rank(&cycles, fraction))
            },
        );
    }
    let open = prs.iter().filter(|pr| pr.open).collect::<Vec<_>>();
    out.put("prs_open", Ok(count(open.len())));
    let thresholds = out.config.thresholds.clone();
    out.put(
        "prs_idle",
        thresholds
            .pr_idle_hours
            .map_or(Err(NOT_CONFIGURED.to_owned()), |hours| {
                let cutoff = frame.now - span_secs(hours, 3600, "pr_idle_hours")?;
                Ok(count(
                    open.iter().filter(|pr| pr.updated_at <= cutoff).count(),
                ))
            }),
    );
    out.put(
        "prs_oversize",
        thresholds
            .pr_oversize_lines
            .map_or(Err(NOT_CONFIGURED.to_owned()), |lines| {
                Ok(count(
                    open.iter()
                        .filter(|pr| pr.additions + pr.deletions > lines)
                        .count(),
                ))
            }),
    );
    out.put(
        "prs_ready_unmerged",
        Ok(count(
            open.iter()
                .filter(|pr| !pr.draft && pr.review_decision.as_deref() == Some("APPROVED"))
                .count(),
        )),
    );
    let relevant = relevant_prs(prs, frame);
    let heads = relevant
        .iter()
        .map(|pr| {
            pr.force_pushes.ok_or_else(|| {
                format!(
                    "GitHub unavailable: timeline of {}#{} was incomplete",
                    pr.repo, pr.number
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|heads| {
            heads
                .into_iter()
                .max()
                .map(|max| max as f64)
                .ok_or_else(|| EMPTY.to_owned())
        });
    out.put("head_force_pushes_max", heads);
}

/// Incident count and minutes from the commits of the window, oldest first.
pub fn main_red(commits: &[MainCommit], now: f64) -> (usize, f64) {
    let mut ordered = commits
        .iter()
        .filter(|commit| commit.state != CheckState::Pending)
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.committed_at.total_cmp(&right.committed_at));
    let (mut incidents, mut minutes) = (0, 0.0);
    let mut failing_since: Option<f64> = None;
    let mut previous: Option<CheckState> = None;
    for commit in ordered {
        match (previous, commit.state) {
            (Some(CheckState::Pass), CheckState::Fail) => {
                incidents += 1;
                failing_since = Some(commit.committed_at);
            }
            (_, CheckState::Pass) => {
                if let Some(since) = failing_since.take() {
                    minutes += (commit.committed_at - since) / 60.0;
                }
            }
            _ => {}
        }
        previous = Some(commit.state);
    }
    if let Some(since) = failing_since {
        minutes += (now - since) / 60.0;
    }
    (incidents, minutes)
}

fn stability(out: &mut Out<'_>, sources: &Sources<'_>) {
    let Some(check) = out.config.main_check.clone() else {
        out.not_configured("main_red_incidents");
        out.not_configured("main_red_minutes");
        return;
    };
    let frame = out.frame;
    match sources
        .github
        .main_commits(&check.repo, &check.branch, frame.start())
    {
        Err(error) => {
            let reason = format!(
                "GitHub unavailable: {}@{}: {error}",
                check.repo, check.branch
            );
            out.unavailable("main_red_incidents", reason.clone());
            out.unavailable("main_red_minutes", reason);
        }
        Ok(commits) => {
            let inside = commits
                .into_iter()
                .filter(|commit| frame.contains(commit.committed_at))
                .collect::<Vec<_>>();
            let (incidents, minutes) = main_red(&inside, frame.now);
            out.put("main_red_incidents", Ok(count(incidents)));
            out.put("main_red_minutes", Ok(minutes));
        }
    }
}

fn utilization(out: &mut Out<'_>, sources: &Sources<'_>) {
    if out.config.herdr_teams.is_empty() {
        out.not_configured("agents_working_share");
        out.not_configured("agents_idle_minutes_max");
    } else {
        match sources.agents.report(&out.config.herdr_teams) {
            Err(error) => {
                let reason = format!("herdr unavailable: {error}");
                out.unavailable("agents_working_share", reason.clone());
                out.unavailable("agents_idle_minutes_max", reason);
            }
            Ok(report) if !report.errors.is_empty() => {
                let reason = format!("herdr unavailable: {}", report.errors.join("; "));
                out.unavailable("agents_working_share", reason.clone());
                out.unavailable("agents_idle_minutes_max", reason);
            }
            Ok(report) => {
                let total = report.agents.len();
                let working = report
                    .agents
                    .iter()
                    .filter(|agent| agent.verdict == crate::agents::Verdict::Working)
                    .count();
                out.put(
                    "agents_working_share",
                    if total == 0 {
                        Err(EMPTY.to_owned())
                    } else {
                        Ok(count(working) / count(total))
                    },
                );
                let idle = report
                    .agents
                    .iter()
                    .filter_map(|agent| agent.last_turn_age_secs)
                    .max();
                out.put(
                    "agents_idle_minutes_max",
                    match (total, idle) {
                        (0, _) => Err(EMPTY.to_owned()),
                        (_, None) => {
                            Err("herdr unavailable: no agent reported a last-turn age".to_owned())
                        }
                        (_, Some(secs)) => Ok(secs as f64 / 60.0),
                    },
                );
            }
        }
    }
    relay(out, sources);
}

fn relay(out: &mut Out<'_>, sources: &Sources<'_>) {
    let (leads, stale) = (
        out.config.relay_leads.clone(),
        out.config.message_stale_minutes,
    );
    let Some(stale) = stale.filter(|_| !leads.is_empty()) else {
        out.not_configured("messages_undelivered");
        return;
    };
    let total = (|| {
        let cutoff_ms = (out.frame.now - span_secs(stale, 60, "message_stale_minutes")?) * 1000.0;
        let mut total = 0;
        for lead in &leads {
            let sent = sources
                .relay
                .pending_sent_at_ms(lead)
                .map_err(|error| format!("ix-relay unavailable: {lead}: {error}"))?;
            total += sent.iter().filter(|at| (**at as f64) < cutoff_ms).count();
        }
        Ok::<_, String>(count(total))
    })();
    out.put("messages_undelivered", total);
}

fn resources(out: &mut Out<'_>, sources: &Sources<'_>) {
    if out.config.local_paths.is_empty() {
        out.not_configured("worktrees_prunable");
        out.not_configured("disk_bytes");
    } else {
        let paths = out.config.local_paths.clone();
        let sum = |read: &dyn Fn(&std::path::Path) -> Result<u64, String>, who: &str| {
            let mut total = 0_u64;
            for path in &paths {
                total += read(path)
                    .map_err(|error| format!("{who} unavailable: {}: {error}", path.display()))?;
            }
            Ok::<_, String>(total as f64)
        };
        out.put(
            "worktrees_prunable",
            sum(&|dir| sources.local.prunable_worktrees(dir), "local git"),
        );
        out.put(
            "disk_bytes",
            sum(&|dir| sources.local.disk_bytes(dir), "local disk"),
        );
    }
    let (Some(name), Some(hours)) = (
        out.config.slot_process.clone(),
        out.config.orphan_slot_hours,
    ) else {
        out.not_configured("orphan_slots");
        return;
    };
    let orphans = (|| {
        let limit = hours
            .checked_mul(3600)
            .ok_or_else(|| "orphan_slot_hours is too large to measure".to_owned())?;
        let rows = sources
            .local
            .processes()
            .map_err(|error| format!("local processes unavailable: {error}"))?;
        Ok(count(
            rows.iter()
                .filter(|row| {
                    row.command.contains(&name) && !row.parent_alive && row.age_secs > limit
                })
                .count(),
        ))
    })();
    out.put("orphan_slots", orphans);
}

fn gate_log(out: &mut Out<'_>, sources: &Sources<'_>) {
    let Some(path) = out.config.gate_log.clone() else {
        for id in ["full_gates_max", "repins_total", "build_lock_wait_minutes"] {
            out.not_configured(id);
        }
        return;
    };
    let frame = out.frame;
    match sources.gate_log.lines(&path) {
        Err(error) => {
            let reason = format!("gate log unavailable: {}: {error}", path.display());
            for id in ["full_gates_max", "repins_total", "build_lock_wait_minutes"] {
                out.unavailable(id, reason.clone());
            }
        }
        Ok(lines) => {
            let finished = lines
                .iter()
                .filter(|line| frame.contains(line.finished_at))
                .collect::<Vec<&GateLine>>();
            let mut per_pr: BTreeMap<&str, usize> = BTreeMap::new();
            for line in finished.iter().filter(|line| !line.repin) {
                *per_pr.entry(line.pr.as_str()).or_default() += 1;
            }
            out.put(
                "full_gates_max",
                Ok(per_pr.values().copied().max().unwrap_or(0) as f64),
            );
            out.put(
                "repins_total",
                Ok(count(finished.iter().filter(|line| line.repin).count())),
            );
            out.put(
                "build_lock_wait_minutes",
                Ok(finished
                    .iter()
                    .map(|line| (line.acquired_lock_at - line.requested_lock_at) / 60.0)
                    .sum()),
            );
        }
    }
}

/// Reason of every metric a past day cannot reconstruct.
pub const NOT_RECONSTRUCTIBLE: &str = "not reconstructible";

/// The metrics a past day can be measured for from issue creation, state history and pull
/// request timestamps (FR-028).
pub const RECONSTRUCTIBLE: [&str; 7] = [
    "prs_merged_per_hour",
    "pr_cycle_median_hours",
    "pr_cycle_p90_hours",
    "path_closed",
    "intake_new",
    "net_closed_minus_new",
    "done_reopened",
];

/// Measures the reconstructible metrics for each past frame, reusing the snapshot computations
/// on historical frames. Linear and GitHub are each read once for the whole set of frames; a
/// failed or incomplete read makes the metrics that depend on it unavailable for every frame.
/// Every other catalog metric is `unavailable` with reason `not reconstructible`.
pub fn collect_history(
    config: &RetroConfig,
    frames: &[Frame],
    linear: &dyn LinearSource,
    github: &dyn GithubSource,
) -> Vec<BTreeMap<String, Metric>> {
    let earliest = frames
        .iter()
        .map(|frame| frame.start())
        .fold(f64::INFINITY, f64::min)
        .max(0.0);
    let workspace = linear.workspace(earliest);
    let prs = list_pull_requests(config, github, earliest);
    let mut history = History {
        source: linear,
        cache: BTreeMap::new(),
    };
    frames
        .iter()
        .map(|frame| {
            let mut out = Out {
                config,
                frame: *frame,
                metrics: BTreeMap::new(),
            };
            match &workspace {
                Ok(snapshot) => {
                    let scope = Scope::new(config, snapshot, *frame);
                    let _ = window_progress(&mut out, &scope, &mut history);
                    rework_issues(&mut out, &scope, &mut history);
                }
                Err(error) => {
                    let reason = format!("Linear unavailable: {error}");
                    for id in LINEAR_SNAPSHOT_IDS {
                        out.unavailable(id, reason.clone());
                    }
                }
            }
            flow(&mut out, &prs);
            for entry in CATALOG {
                if !RECONSTRUCTIBLE.contains(&entry.id) {
                    out.unavailable(entry.id, NOT_RECONSTRUCTIBLE.to_owned());
                }
            }
            out.metrics
        })
        .collect()
}
