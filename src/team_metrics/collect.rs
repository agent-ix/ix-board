// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::github::SelectedGithub;
use super::history::CommandHistory;
use super::timeline::{ExecutionSample, Member};
use super::{Config, Error, Event, History, Journal, ScopeState, State};
use crate::retro::sources::{AgentSource, GithubSource, LinearSource};
use crate::{WorkflowStateType, WorkspaceSnapshot, retro};
use std::collections::{BTreeMap, BTreeSet};

/// A completed tracker refresh, passed across a bounded worker channel.
pub struct TrackerBatch {
    at_ms: i64,
    results: Vec<(String, Result<TrackerResult, String>)>,
}
struct TrackerResult {
    members: BTreeMap<String, Member>,
    record: retro::Record,
    build_execution_seconds_24h: Option<f64>,
    ready_issues: Option<usize>,
    blocked_issues: Option<usize>,
    query_identity: String,
    resolved_selectors: super::Selectors,
    pr_stats: Option<super::pr_stats::PrStats>,
    review_stats: Option<super::reviews::ReviewStats>,
    github_at_ms: i64,
}

/// Owns durable state; network collection never runs on the HTTP listener.
pub struct Collector {
    pub(crate) config: Config,
    pub(crate) state: State,
    journal: Journal,
}
impl Collector {
    /// Restores bounded history and establishes configured scope identities.
    pub fn open(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let journal = Journal::open(&config.state_path, &config)?;
        let mut state = journal.load()?;
        state.configure(&config);
        // Restored state is useful for counters, never fresh source evidence.
        state.history_ok = false;
        for scope in state.scopes.values_mut() {
            scope.tracker_ok = false;
            scope.lifecycle_ok = false;
            scope.samples.clear();
        }
        Ok(Self {
            config,
            state,
            journal,
        })
    }
    /// Reads at most sixteen bounded pages per tick and checkpoints each page.
    pub fn history(&mut self, source: &impl History, now: i64) -> Result<(), Error> {
        for _ in 0..16 {
            self.state.history_ok = false;
            let page = match source.page(&self.state.cursor) {
                Ok(p) => p,
                Err(e) => {
                    self.state.history_ok = false;
                    return Err(e);
                }
            };
            if page.version != 1
                || page.observed_at_ms < self.state.history_at_ms
                || page.observed_at_ms < 0
                || page.observed_at_ms > now.saturating_add(30000)
                || page.events.len() > 500
                || page.next_cursor.len() > 64
                || page
                    .available_from_ms
                    .is_some_and(|at| at < 0 || at > page.observed_at_ms)
            {
                return Err(Error::Invalid("history protocol/clock skew"));
            }
            let previous = super::timeline::numeric_cursor(&self.state.cursor)?;
            let next = super::timeline::numeric_cursor(&page.next_cursor)?;
            let observed = u64::try_from(page.observed_at_ms)
                .map_err(|_| Error::Invalid("history timestamp"))?;
            if next < previous || next.0 > observed {
                return Err(Error::Invalid("history cursor regression/future"));
            }
            let mut candidate = self.state.clone();
            if page.truncated {
                candidate.history_gaps = candidate.history_gaps.saturating_add(1);
                for state in candidate.scopes.values_mut() {
                    state.assignments.clear();
                    state.samples.clear();
                    state.reported_scope.clear();
                    state.effort_points.clear();
                }
            }
            let mut accepted: Vec<Event> = Vec::new();
            for event in &page.events {
                if event.id.len() > 64
                    || event.at_ms > page.observed_at_ms
                    || u64::try_from(event.at_ms).ok()
                        != Some(super::timeline::numeric_cursor(&event.id)?.0)
                {
                    return Err(Error::Invalid("event follows source watermark"));
                }
                if candidate.apply(&self.config, event)? {
                    accepted.push(event.clone());
                }
            }
            if page.events.is_empty() && next > previous {
                // Version-one relay history advances an empty stream to its seven-day
                // retention floor. Source coverage can start later than that floor.
                let floor = page.observed_at_ms.saturating_sub(604_800_000).max(0);
                if page.has_more
                    || page.next_cursor != format!("{floor}-0")
                    || page.available_from_ms.is_some_and(|at| at < floor)
                {
                    return Err(Error::Invalid(
                        "empty history cursor lacks retention coverage",
                    ));
                }
                candidate.cursor = page.next_cursor.clone();
            }
            if page.next_cursor != candidate.cursor || (page.has_more && accepted.is_empty()) {
                return Err(Error::Invalid("history cursor did not match page"));
            }
            candidate.history_from_ms = page.available_from_ms;
            candidate.history_at_ms = page.observed_at_ms;
            candidate.history_ok = true;
            candidate.journal_from_ms = self.journal.checkpoint(&candidate, &accepted, now)?;
            self.state = candidate;
            if !page.has_more {
                return Ok(());
            }
        }
        self.state.history_ok = false;
        Err(Error::Source(
            "history backlog exceeds per-tick page budget".into(),
        ))
    }
    /// Polls the installed relay command and lifecycle sources on their own cadence.
    pub fn activity(&mut self, now: i64) -> Result<(), Error> {
        self.state.quota_frontier = crate::default_cache_path()
            .ok()
            .and_then(|path| super::quota_frontier::read(&path, now).ok());
        let history = self.history(&CommandHistory, now);
        let leads: Vec<_> = self
            .config
            .scopes
            .iter()
            .flat_map(|s| s.leads.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for lead in leads {
            match super::history::read_history(
                "agent-msg",
                &["work", "--lead", &lead, "snapshot"],
                std::time::Duration::from_secs(10),
            )
            .and_then(|bytes| Ok(serde_json::from_slice::<relay_work_wire::Snapshot>(&bytes)?))
            {
                Ok(snapshot)
                    if snapshot.version == 1
                        && snapshot.binding.address.eq_ignore_ascii_case(&lead)
                        && (snapshot.observed_at_ms - now).abs() <= 30000 =>
                {
                    self.state.bootstrap(&self.config, &snapshot)
                }
                Ok(_) => eprintln!("work snapshot identity/protocol/clock mismatch for {lead}"),
                Err(error) => eprintln!("work snapshot for {lead}: {error}"),
            }
        }
        let teams: Vec<_> = self
            .config
            .scopes
            .iter()
            .flat_map(|s| s.herdr_teams.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let machines = self
            .config
            .scopes
            .iter()
            .flat_map(|s| &s.leads)
            .filter_map(|lead| lead.split(':').next().map(str::to_owned))
            .collect();
        let agents = retro::live::LiveAgents::start_on_machines(teams.clone(), machines);
        let result = agents.report(&teams);
        if let Err(error) = &result {
            eprintln!("lifecycle evidence: {error}");
        }
        let mut samples = Vec::new();
        let complete = result.is_ok();
        if let Ok(report) = result {
            for row in report.agents.iter().filter(|r| !r.partial) {
                // Agent reports label this host "local". Resolve that alias
                // only through an exact native session AND pane match to a
                // verified relay binding; never infer it from the hostname.
                let machines: BTreeSet<_> = self
                    .state
                    .scopes
                    .values()
                    .flat_map(|s| s.bindings.values())
                    .filter(|b| {
                        row.session_id.as_deref() == Some(b.session.as_str())
                            && row.pane_id == b.pane
                    })
                    .map(|b| b.machine.as_str())
                    .collect();
                let machine = if row.machine == "local" && machines.len() == 1 {
                    (*machines.first().expect("one machine")).to_owned()
                } else {
                    row.machine.clone()
                };
                let clock = i64::try_from(row.collection_finished_at)
                    .ok()
                    .and_then(|s| s.checked_mul(1000));
                let Some(at) = clock.filter(|at| (now - *at).abs() <= 90000) else {
                    continue;
                };
                if let Some(session) = &row.session_id {
                    samples.push(ExecutionSample {
                        machine: machine.clone(),
                        session: session.clone(),
                        parent_session: None,
                        working: row.verdict == crate::agents::Verdict::Working,
                        at_ms: at,
                    });
                }
                for worker in row
                    .workers
                    .iter()
                    .filter(|w| w.role == crate::agents::WorkerRole::Worker && w.issues.is_empty())
                {
                    samples.push(ExecutionSample {
                        machine: machine.clone(),
                        session: worker.id.0.clone(),
                        parent_session: worker.parent_id.as_ref().map(|id| id.0.clone()),
                        working: worker.state == crate::agents::WorkerState::Running,
                        at_ms: at,
                    });
                }
            }
        }
        samples.sort_by(|a, b| (&a.machine, &a.session).cmp(&(&b.machine, &b.session)));
        samples.dedup_by(|a, b| a.machine == b.machine && a.session == b.session);
        self.state.observe(
            &self.config,
            &samples,
            now,
            complete && self.state.history_ok,
        );
        self.journal.checkpoint(&self.state, &[], now)?;
        history
    }
    /// Collects tracker/VCS data using shared retro calculations, outside the activity worker.
    pub fn tracker(config: &Config, now: i64) -> TrackerBatch {
        let source = retro::live::linear_or_unreadable();
        let workspace = tracker_workspace(source.as_ref(), now as f64 / 1000.0 - 86400.0);
        // The native workspace reader stamps completed observations. Anchor
        // calculations after that read, rather than mistaking read duration
        // for source clock skew against the poll's starting instant.
        let now = super::now_ms().unwrap_or(now).max(now);
        let source_sync_ms = verified_sync_ms(source.cache_freshness()).unwrap_or(now);
        let results = config
            .scopes
            .iter()
            .map(|scope| {
                let result = (|| {
                    let workspace = workspace.as_ref().map_err(Clone::clone)?;
                    let summary = crate::select_scope(workspace, &scope.selectors.selection())
                        .map_err(|e| e.to_string())?;
                    let resolved_selectors = super::Selectors {
                        team: scope.selectors.team.clone(),
                        initiatives: scope
                            .selectors
                            .initiatives
                            .iter()
                            .map(|s| {
                                crate::scope::resolve_initiative_id(workspace, s)
                                    .map_err(|e| e.to_string())
                            })
                            .collect::<Result<_, _>>()?,
                        projects: scope
                            .selectors
                            .projects
                            .iter()
                            .map(|s| {
                                crate::scope::resolve_project_id(workspace, s)
                                    .map(|id| id.0)
                                    .map_err(|e| e.to_string())
                            })
                            .collect::<Result<_, _>>()?,
                        tickets: scope
                            .selectors
                            .tickets
                            .iter()
                            .map(|s| {
                                crate::scope::resolve_ticket_id(workspace, s)
                                    .map(|id| id.0)
                                    .map_err(|e| e.to_string())
                            })
                            .collect::<Result<_, _>>()?,
                    };
                    let mut cfg =
                        retro::RetroConfig::load(&scope.retro_config).map_err(|e| e.to_string())?;
                    // Native selected populations replace broad repository attribution.
                    cfg.projects = workspace
                        .projects
                        .iter()
                        .filter(|p| summary.project_ids.contains(&p.id))
                        .map(|p| p.name.clone())
                        .collect();
                    if let Some(team) = &scope.selectors.team {
                        cfg.linear_teams = vec![team.clone()];
                    }
                    let selected: Vec<_> = workspace
                        .issues
                        .iter()
                        .filter(|i| summary.issue_ids.contains(&i.id))
                        .cloned()
                        .collect();
                    let selected_ids: BTreeSet<_> =
                        selected.iter().map(|i| i.identifier.as_str()).collect();
                    let teams: BTreeSet<_> =
                        selected.iter().map(|i| i.team_key.0.as_str()).collect();
                    let ready_issues = teams
                        .iter()
                        .try_fold(0usize, |n, team| {
                            crate::ready_from_snapshot(workspace, team).map(|items| {
                                n + items
                                    .iter()
                                    .filter(|i| selected_ids.contains(i.identifier.as_str()))
                                    .count()
                            })
                        })
                        .ok();
                    let mut blocked_issues = Some(0usize);
                    for issue in selected.iter().filter(|i| {
                        summary.leaf_ids.contains(&i.id)
                            && !matches!(
                                i.state_type,
                                WorkflowStateType::Completed
                                    | WorkflowStateType::Canceled
                                    | WorkflowStateType::Duplicate
                            )
                    }) {
                        let blockers = issue
                            .blocked_by
                            .iter()
                            .map(|id| workspace.issues.iter().find(|i| i.id == *id))
                            .collect::<Option<Vec<_>>>();
                        match blockers {
                            Some(rows)
                                if rows.iter().any(|i| {
                                    !matches!(
                                        i.state_type,
                                        WorkflowStateType::Completed
                                            | WorkflowStateType::Canceled
                                            | WorkflowStateType::Duplicate
                                    )
                                }) =>
                            {
                                if let Some(n) = blocked_issues.as_mut() {
                                    *n += 1;
                                }
                            }
                            None => blocked_issues = None,
                            _ => {}
                        }
                    }
                    let attachments_complete = selected.iter().all(|i| i.attachments_complete);
                    let allowed: BTreeSet<(String, u64)> = selected
                        .iter()
                        .flat_map(|i| &i.attachment_urls)
                        .filter_map(|u| retro::metrics::pull_request_ref(u))
                        .collect();
                    let linear = SelectedLinear {
                        inner: source.as_ref(),
                        snapshot: WorkspaceSnapshot {
                            issues: selected.clone(),
                            projects: workspace
                                .projects
                                .iter()
                                .filter(|p| summary.project_ids.contains(&p.id))
                                .cloned()
                                .collect(),
                            fetched_at: workspace.fetched_at,
                        },
                    };
                    if cfg.repos.is_empty() {
                        cfg.repos = allowed
                            .iter()
                            .map(|(repo, _)| repo.clone())
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                    }
                    let build_execution_seconds_24h = cfg.gate_log.as_ref().and_then(|path| {
                        use crate::retro::sources::GateLogSource;
                        retro::live::FileGateLog.lines(path).ok().and_then(|lines| {
                            let mut seen = BTreeSet::new();
                            let end = now as f64 / 1000.0;
                            let start = end - 86400.0;
                            let mut seconds = 0.0;
                            for line in lines.into_iter().filter(|l| !l.repin) {
                                let (repo, n) = line.pr.rsplit_once('#').and_then(|(r, n)| {
                                    n.parse::<u64>().ok().map(|n| (r.to_owned(), n))
                                })?;
                                if !allowed.contains(&(repo, n)) {
                                    continue;
                                }
                                if !line.started_at.is_finite()
                                    || !line.finished_at.is_finite()
                                    || line.finished_at < line.started_at
                                {
                                    return None;
                                }
                                if seen.insert((
                                    line.pr,
                                    line.started_at.to_bits(),
                                    line.finished_at.to_bits(),
                                )) {
                                    seconds += (line.finished_at.min(end)
                                        - line.started_at.max(start))
                                    .max(0.0);
                                }
                            }
                            Some(seconds)
                        })
                    });
                    let github = SelectedGithub::new(config, allowed, attachments_complete, now);
                    let agents = NoAgents;
                    // Agent share requires session-qualified polling, not a second broad read.
                    cfg.herdr_teams.clear();
                    if let Some(dept) = cfg.department.as_mut() {
                        dept.scope.projects = summary
                            .project_ids
                            .iter()
                            .map(|id| retro::department::Selector {
                                id: id.0.clone(),
                                label: id.0.clone(),
                            })
                            .collect();
                    }
                    let sources = retro::Sources {
                        linear: &linear,
                        github: &github,
                        agents: &agents,
                        gate_log: &retro::live::FileGateLog,
                        relay: &retro::live::RedisCliRelay,
                        local: &retro::live::SystemLocal,
                    };
                    use sha2::Digest;
                    let query_identity = format!(
                        "{:x}",
                        sha2::Sha256::digest(serde_json::to_vec(&cfg).map_err(|e| e.to_string())?)
                    );
                    let mut record = retro::collect_snapshot(
                        &cfg,
                        retro::Frame {
                            now: now as f64 / 1000.0,
                            window_secs: 86400,
                            end_exclusive: false,
                        },
                        &sources,
                        None,
                        None,
                    )
                    .map_err(|e| e.to_string())?;
                    if cfg.gate_tickets.is_empty() {
                        for id in [
                            "path_remaining",
                            "path_closed",
                            "path_closed_unverified",
                            "gates_remaining",
                        ] {
                            if let Some(metric) = record.metrics.get_mut(id) {
                                metric.status = retro::Status::Unavailable;
                                metric.value = None;
                                metric.reason = Some("gate tickets not configured".into());
                            }
                        }
                    }
                    // Evidence locators stay in source stores; this cache serves metrics only.
                    if let Some(dept) = record.department.as_mut() {
                        dept.observations.clear();
                        for m in dept.measurements.values_mut() {
                            m.evidence_refs.clear();
                        }
                    }
                    let all_prs = cfg
                        .repos
                        .iter()
                        .map(|repo| github.pull_requests(repo, now as f64 / 1000.0 - 86400.0))
                        .collect::<Result<Vec<_>, _>>()
                        .map(|rows| rows.into_iter().flatten().collect::<Vec<_>>());
                    let pr_stats = all_prs
                        .as_ref()
                        .ok()
                        .map(|prs| super::pr_stats::calculate(prs, now as f64 / 1000.0));
                    let review_stats = (|| -> Result<super::reviews::ReviewStats, String> {
                        let client =
                            crate::LinearGraphQlSource::from_env().map_err(|e| e.to_string())?;
                        let prs = all_prs.as_ref().map_err(Clone::clone)?;
                        let allowed: BTreeSet<_> = prs
                            .iter()
                            .map(|p| format!("{}#{}", p.repo, p.number))
                            .collect();
                        let created = prs
                            .iter()
                            .map(|p| (format!("{}#{}", p.repo, p.number), p.created_at))
                            .collect();
                        let mut comments = Vec::new();
                        for repo in &cfg.repos {
                            comments.extend(client.fetch_marker_comments(
                                "<!-- reviewer ",
                                &format!("repo={repo} "),
                                Some(now as f64 / 1000.0 - 86400.0),
                            )?);
                            comments.extend(client.fetch_marker_comments(
                                "<!-- reviewer-dispositions ",
                                &format!("repo={repo} "),
                                Some(now as f64 / 1000.0 - 86400.0),
                            )?);
                        }
                        super::reviews::calculate(
                            &comments,
                            &allowed,
                            &created,
                            now as f64 / 1000.0,
                        )
                    })()
                    .map_err(|error| eprintln!("review evidence for {}: {error}", scope.key()))
                    .ok();
                    let members = selected
                        .into_iter()
                        .map(|i| {
                            let leaf = summary.leaf_ids.contains(&i.id);
                            (
                                i.id.0,
                                Member {
                                    ticket: i.identifier,
                                    state: i.state_type,
                                    leaf,
                                },
                            )
                        })
                        .collect();
                    Ok(TrackerResult {
                        members,
                        record,
                        build_execution_seconds_24h,
                        ready_issues,
                        blocked_issues,
                        query_identity,
                        resolved_selectors,
                        pr_stats,
                        review_stats,
                        github_at_ms: github.observed_at(),
                    })
                })();
                (scope.key(), result)
            })
            .collect();
        TrackerBatch {
            at_ms: source_sync_ms,
            results,
        }
    }
    /// Commits a completed tracker refresh without holding a network-facing lock.
    pub fn apply_tracker(&mut self, batch: TrackerBatch) -> Result<(), Error> {
        for (key, result) in batch.results {
            let Some(state) = self.state.scopes.get_mut(&key) else {
                continue;
            };
            match result {
                Ok(result) => {
                    let Some(scope) = self.config.scopes.iter().find(|s| s.key() == key) else {
                        continue;
                    };
                    if state.query_identity.as_ref() != Some(&result.query_identity)
                        || state.selectors.as_ref() != Some(&scope.selectors)
                    {
                        state.effort_points.clear();
                        state.samples.clear();
                        state.stages.clear();
                        state.lifecycle_ok = false;
                        state.coverage_started_ms = None;
                        state.comparable_since_ms = batch.at_ms;
                    }
                    state.query_identity = Some(result.query_identity);
                    state.resolved_selectors = Some(result.resolved_selectors);
                    update_members(state, &result.members, &scope.selectors);
                    state.build_execution_seconds_24h = result.build_execution_seconds_24h;
                    state.ready_issues = result.ready_issues;
                    state.blocked_issues = result.blocked_issues;
                    state.record = Some(result.record);
                    state.pr_stats = result.pr_stats;
                    state.review_stats = result.review_stats;
                    state.github_observed_at_ms = result.github_at_ms;
                    if state.pr_stats.is_some() {
                        state.pr_success_at_ms = result.github_at_ms.min(batch.at_ms);
                    }
                    if state.review_stats.is_some() {
                        state.review_success_at_ms = result.github_at_ms.min(batch.at_ms);
                    }
                    state.tracker_at_ms = batch.at_ms;
                    state.tracker_ok = true;
                    state.last_error = None;
                }
                Err(reason) => {
                    state.tracker_ok = false;
                    state.last_error = Some(reason);
                }
            }
        }
        self.journal.checkpoint(&self.state, &[], batch.at_ms)?;
        Ok(())
    }
    /// Renders cached metrics only; performs no source calls.
    pub fn metrics(&self, now: i64) -> Result<String, Error> {
        super::render(&self.config, &self.state, self.journal.bytes()?, now)
    }
    /// Returns immutable configuration for worker setup.
    pub fn config(&self) -> &Config {
        &self.config
    }
}
fn update_members(
    state: &mut ScopeState,
    new: &BTreeMap<String, Member>,
    selectors: &super::Selectors,
) {
    let baseline = state.selectors.is_some();
    if baseline && state.selectors.as_ref() != Some(selectors) {
        state.effort_points.clear();
        state.samples.clear();
        state.stages.clear();
        state.lifecycle_ok = false;
        state.coverage_started_ms = None;
    }
    if baseline {
        for (id, member) in new {
            match state.members.get(id) {
                None => *state.totals.entry("scope_added".into()).or_default() += 1.0,
                Some(old) if old.state != member.state => {
                    let key = if member.state == WorkflowStateType::Completed && member.leaf {
                        "reported_completions"
                    } else if member.state == WorkflowStateType::Canceled {
                        "canceled"
                    } else if old.state == WorkflowStateType::Completed
                        && member.state != WorkflowStateType::Duplicate
                    {
                        "reopened"
                    } else {
                        "state_changes"
                    };
                    *state.totals.entry(key.into()).or_default() += 1.0;
                }
                _ => {}
            }
            if state
                .members
                .get(id)
                .is_some_and(|old| old.leaf && !member.leaf)
                && !matches!(
                    member.state,
                    WorkflowStateType::Canceled | WorkflowStateType::Duplicate
                )
            {
                *state
                    .totals
                    .entry("split_or_reparented".into())
                    .or_default() += 1.0;
            }
        }
        *state.totals.entry("scope_removed".into()).or_default() += state
            .members
            .keys()
            .filter(|id| !new.contains_key(*id))
            .count() as f64;
        if state.selectors.as_ref() != Some(selectors) {
            *state.totals.entry("scope_revisions".into()).or_default() += 1.0;
        }
    }
    state.members = new.clone();
    state.selectors = Some(selectors.clone());
}
// A cache fallback is useful to interactive readers but cannot certify current
// tracker populations or refresh the exporter's last-success timestamp.
fn tracker_workspace(source: &dyn LinearSource, since: f64) -> Result<WorkspaceSnapshot, String> {
    let snapshot = source.workspace(since)?;
    verified_sync_ms(source.cache_freshness())?;
    Ok(snapshot)
}

fn verified_sync_ms(freshness: Option<crate::CacheFreshness>) -> Result<i64, String> {
    freshness
        .filter(|f| !f.stale)
        .and_then(|f| f.last_successful_sync)
        .filter(|at| *at > 0)
        .and_then(|at| at.checked_mul(1000))
        .ok_or_else(|| "tracker cache is stale or has no verified sync provenance".into())
}

struct NoAgents;
impl AgentSource for NoAgents {
    fn report(&self, _: &[String]) -> Result<crate::agents::AgentsReport, String> {
        Err("use session-qualified activity source".into())
    }
}
struct SelectedLinear<'a> {
    inner: &'a dyn LinearSource,
    snapshot: WorkspaceSnapshot,
}
impl LinearSource for SelectedLinear<'_> {
    fn department_evidence(
        &self,
        scope: &retro::department::Scope,
        _since: f64,
    ) -> retro::department::Evidence {
        // Current WIP requires only current facts. Historical claims/movement
        // retain unavailable provenance until their authoritative producers exist.
        retro::department::workspace_evidence(&self.snapshot, scope)
    }
    fn workspace(&self, _: f64) -> Result<WorkspaceSnapshot, String> {
        Ok(self.snapshot.clone())
    }
    fn project_open_count(&self, project: &str) -> Result<usize, String> {
        Ok(self
            .snapshot
            .issues
            .iter()
            .filter(|i| {
                i.project_id.as_ref().is_some_and(|p| {
                    p.0 == project
                        || self.snapshot.projects.iter().any(|native| {
                            native.id == *p && native.name.eq_ignore_ascii_case(project)
                        })
                }) && !matches!(
                    i.state_type,
                    WorkflowStateType::Completed
                        | WorkflowStateType::Canceled
                        | WorkflowStateType::Duplicate
                )
            })
            .count())
    }
    fn state_history(&self, id: &str) -> Result<Vec<retro::sources::StateChange>, String> {
        self.inner.state_history(id)
    }
    fn state_histories(
        &self,
        ids: &[String],
    ) -> Vec<Result<Vec<retro::sources::StateChange>, String>> {
        self.inner.state_histories(ids)
    }
    fn marker_lines(&self, prefix: &str, needle: &str) -> Result<Vec<String>, String> {
        self.inner.marker_lines(prefix, needle)
    }
    fn open_issues_with_label(
        &self,
        label: &str,
    ) -> Result<Vec<retro::sources::LabeledIssue>, String> {
        let ids: BTreeSet<_> = self
            .snapshot
            .issues
            .iter()
            .map(|i| i.identifier.as_str())
            .collect();
        self.inner.open_issues_with_label(label).map(|v| {
            v.into_iter()
                .filter(|i| ids.contains(i.identifier.as_str()))
                .collect()
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;
    #[trace("FR-033-AC-5")]
    #[test]
    fn stale_backoff_snapshots_cannot_refresh_tracker_success() {
        let freshness = |stale, sync| crate::CacheFreshness {
            last_successful_sync: sync,
            last_full_reconciliation: Some(100),
            stale,
            age_seconds: Some(30),
            rate_limit_until: Some(200),
            failure_reason: None,
        };
        assert_eq!(
            verified_sync_ms(Some(freshness(false, Some(123)))).unwrap(),
            123000
        );
        assert!(verified_sync_ms(Some(freshness(true, Some(123)))).is_err());
        assert!(verified_sync_ms(Some(freshness(false, None))).is_err());
        assert!(verified_sync_ms(Some(freshness(false, Some(i64::MAX)))).is_err());
        assert!(verified_sync_ms(None).is_err());
    }
    #[trace("FR-033-AC-6")]
    #[test]
    fn removing_or_canceling_work_does_not_create_delivery() {
        let selectors = super::super::Selectors {
            team: Some("QSL".into()),
            ..Default::default()
        };
        let mut state = ScopeState::default();
        let member = |state| Member {
            ticket: "QSL-12".into(),
            state,
            leaf: true,
        };
        update_members(
            &mut state,
            &BTreeMap::from([("id".into(), member(WorkflowStateType::Started))]),
            &selectors,
        );
        assert!(state.totals.is_empty());
        update_members(
            &mut state,
            &BTreeMap::from([("id".into(), member(WorkflowStateType::Canceled))]),
            &selectors,
        );
        assert_eq!(state.totals["canceled"], 1.0);
        update_members(&mut state, &BTreeMap::new(), &selectors);
        assert_eq!(state.totals["scope_removed"], 1.0);
        assert!(!state.totals.contains_key("completed"));
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    use ix_trace_rs::trace;
    #[trace("FR-033-AC-6")]
    #[test]
    fn changing_selectors_invalidates_comparable_effort_history() {
        let old = super::super::Selectors {
            team: Some("QSL".into()),
            ..Default::default()
        };
        let mut state = ScopeState {
            selectors: Some(old),
            effort_points: vec![(1000, 10.0, 30.0)],
            lifecycle_ok: true,
            ..Default::default()
        };
        let new = super::super::Selectors {
            team: Some("SWM".into()),
            ..Default::default()
        };
        update_members(&mut state, &BTreeMap::new(), &new);
        assert!(state.effort_points.is_empty());
        assert!(!state.lifecycle_ok);
        assert_eq!(state.totals["scope_revisions"], 1.0);
    }
}
