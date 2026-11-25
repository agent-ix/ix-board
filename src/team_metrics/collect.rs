// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::github::SelectedGithub;
use super::history::CommandHistory;
use super::timeline::{ExecutionSample, Member};
use super::{Config, Error, Event, History, Journal, ScopeState, State};
use crate::fleet::live::{PromHttp, PromQuery};
use crate::retro::sources::{AgentSource, GithubSource, LinearSource};
use crate::{Issue, Project, ProjectStatusCategory, WorkflowStateType, WorkspaceSnapshot, retro};
use ix_trace_rs::implements;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

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
    closures_daily: Option<Vec<usize>>,
    merged_prs_daily: Option<Vec<usize>>,
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
    #[implements("FR-033-AC-16")]
    pub fn activity(&mut self, now: i64) -> Result<(), Error> {
        let mut lead_observations = BTreeMap::new();
        let mut lead_population_complete = true;
        for scope in &mut self.config.scopes {
            if !scope.derive_leads_from_relay {
                continue;
            }
            let Some(team) = scope.selectors.team.as_deref() else {
                lead_population_complete = false;
                continue;
            };
            match discover_relay_leads(team) {
                Ok(leads) if !leads.is_empty() => match observe_live_leads(team, &leads) {
                    Ok(observation) => {
                        let complete = observation.expected > 0 && observation.coverage() >= 0.95;
                        scope.leads = observation.live.clone();
                        lead_population_complete &= complete;
                        lead_observations.insert(scope.key(), observation);
                    }
                    Err(error) => {
                        lead_population_complete = false;
                        eprintln!("Herdr lead observation for {team}: {error}");
                    }
                },
                Ok(_) => {
                    lead_population_complete = false;
                    eprintln!("relay lead directory returned no addresses for {team}");
                }
                Err(error) => {
                    lead_population_complete = false;
                    eprintln!("relay lead directory for {team}: {error}");
                }
            }
        }
        let prometheus_status = read_prometheus_status(
            &self
                .config
                .scopes
                .iter()
                .filter_map(|scope| scope.selectors.team.clone())
                .collect(),
        )
        .map_err(|error| eprintln!("Prometheus pane/lane status: {error}"))
        .ok();
        let execution_observed_at = super::now_ms().unwrap_or(now);
        for scope in &self.config.scopes {
            let key = scope.key();
            let observation = lead_observations.get(&key);
            let expected = if scope.derive_leads_from_relay {
                observation
                    .map(|o| o.expected_addresses.as_slice())
                    .unwrap_or(&[])
            } else {
                scope.leads.as_slice()
            };
            let no_pane = observation.map(|o| &o.no_pane).cloned().unwrap_or_default();
            let state = self.state.scopes.get(&key);
            let (observed, working) = classify_execution_leads(
                expected,
                &no_pane,
                prometheus_status.as_ref(),
                state,
                &[],
                execution_observed_at,
            );
            if let Some(state) = self.state.scopes.get_mut(&key) {
                state.relay_leads_expected = expected.len();
                state.relay_leads_observed = observation.map_or(0, |o| o.observed);
                state.relay_leads_not_running = observation.map_or(0, |o| o.no_pane.len());
                state.execution_observed_count = observed;
                state.execution_working_count = working;
                state.execution_observed_at_ms = execution_observed_at;
            }
        }
        self.journal.checkpoint(&self.state, &[], now)?;
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
        // A missing machine keeps the healthy machines' rows but makes coverage partial.
        let complete = lead_population_complete
            && result
                .as_ref()
                .is_ok_and(|report| report.missing_hosts.is_empty());
        if let Ok(report) = result {
            for missing in &report.missing_hosts {
                eprintln!(
                    "lifecycle evidence: host {} missing: {}",
                    missing.machine, missing.reason
                );
            }
            for row in report.agents.iter().filter(|r| {
                execution_evidence_usable(
                    r.verdict,
                    r.availability.pane.complete,
                    r.availability.transcript.complete,
                )
            }) {
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
    #[implements("FR-033-AC-16", "FR-033-AC-18")]
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
                    let mut selection = scope.selectors.selection();
                    if scope.selectors.current_release_project {
                        let project = current_release_project(
                            workspace,
                            scope.selectors.team.as_deref().unwrap_or_default(),
                        )?;
                        selection.projects.push(project.id.0.clone());
                    }
                    let summary =
                        crate::select_scope(workspace, &selection).map_err(|e| e.to_string())?;
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
                        projects: selection
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
                        current_release_project: scope.selectors.current_release_project,
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
                    let closures_daily =
                        daily_linear_closures(source.as_ref(), &selected, now as f64 / 1000.0);
                    if scope.selectors.current_release_project {
                        cfg.gate_tickets = selected
                            .iter()
                            .filter(|issue| {
                                summary.leaf_ids.contains(&issue.id)
                                    && !matches!(
                                        issue.state_type,
                                        WorkflowStateType::Completed
                                            | WorkflowStateType::Canceled
                                            | WorkflowStateType::Duplicate
                                    )
                            })
                            .map(|issue| issue.identifier.clone())
                            .collect();
                    }
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
                            // Keep blockers outside the selected release available to the
                            // path evaluator; RetroConfig still limits project metrics.
                            issues: workspace.issues.clone(),
                            projects: workspace.projects.clone(),
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
                    let mut all_prs = Vec::new();
                    let mut successful_repos = 0usize;
                    for repo in &cfg.repos {
                        match github.pull_requests(repo, now as f64 / 1000.0 - 604800.0) {
                            Ok(rows) => {
                                successful_repos += 1;
                                all_prs.extend(rows);
                            }
                            Err(error) => eprintln!("GitHub evidence for {repo}: {error}"),
                        }
                    }
                    let all_prs = (!cfg.repos.is_empty()
                        && attachments_complete
                        && successful_repos == cfg.repos.len())
                    .then_some(all_prs);
                    let pr_stats = all_prs
                        .as_ref()
                        .filter(|_| github.observed_at() > 0)
                        .map(|prs| super::pr_stats::calculate(prs, now as f64 / 1000.0));
                    let review_stats = (|| -> Result<super::reviews::ReviewStats, String> {
                        let client =
                            crate::LinearGraphQlSource::from_env().map_err(|e| e.to_string())?;
                        let prs = all_prs
                            .as_ref()
                            .ok_or_else(|| "GitHub PR population unavailable".to_owned())?;
                        let allowed: BTreeSet<_> = prs
                            .iter()
                            .map(|p| format!("{}#{}", p.repo, p.number))
                            .collect();
                        let created = prs
                            .iter()
                            .map(|p| (format!("{}#{}", p.repo, p.number), p.created_at))
                            .collect();
                        let mut comments =
                            client.fetch_marker_comments("<!-- reviewer ", "repo=", None)?;
                        comments.extend(client.fetch_marker_comments(
                            "<!-- reviewer-dispositions ",
                            "repo=",
                            None,
                        )?);
                        super::reviews::calculate(
                            &comments,
                            &allowed,
                            &created,
                            &prs.iter()
                                .filter(|pr| pr.open && !pr.draft)
                                .map(|pr| format!("{}#{}", pr.repo, pr.number))
                                .collect(),
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
                        closures_daily,
                        merged_prs_daily: all_prs
                            .as_ref()
                            .map(|prs| daily_github_merges(prs, now as f64 / 1000.0)),
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
                    state.closures_daily = result.closures_daily;
                    state.merged_prs_daily = result.merged_prs_daily;
                    state.github_observed_at_ms = result.github_at_ms;
                    if state.pr_stats.is_some() && result.github_at_ms > 0 {
                        state.pr_success_at_ms = result.github_at_ms;
                    }
                    if state.review_stats.is_some() && result.github_at_ms > 0 {
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

#[derive(serde::Deserialize)]
struct RelayDirectoryLead {
    address: String,
    workspace_label: String,
}

struct LeadObservation {
    expected: usize,
    observed: usize,
    expected_addresses: Vec<String>,
    no_pane: BTreeSet<String>,
    live: Vec<String>,
}
impl LeadObservation {
    fn coverage(&self) -> f64 {
        self.observed as f64 / self.expected.max(1) as f64
    }
}

/// Returns the registered addresses in the exact Linear team workspace.
#[implements("FR-033-AC-16")]
fn discover_relay_leads(team: &str) -> Result<Vec<String>, String> {
    let output = super::history::read_history(
        "agent-msg",
        &["find", "--json", team],
        std::time::Duration::from_secs(10),
    )
    .map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(&output).map_err(|_| "relay directory is not UTF-8")?;
    let mut leads = BTreeSet::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: RelayDirectoryLead = serde_json::from_str(line)
            .map_err(|_| "relay directory returned an invalid lead row")?;
        if !row.workspace_label.eq_ignore_ascii_case(team) {
            continue;
        }
        if row.address.split(':').count() != 3 || row.address.split(':').any(str::is_empty) {
            return Err("relay directory returned an invalid address".into());
        }
        leads.insert(row.address);
        if leads.len() > 32 {
            return Err("relay directory lead count exceeds configured bound".into());
        }
    }
    Ok(leads.into_iter().collect())
}

/// Classifies relay-directory addresses against complete Herdr snapshots per machine.
#[implements("FR-033-AC-16")]
fn observe_live_leads(team: &str, leads: &[String]) -> Result<LeadObservation, String> {
    let whoami = super::history::read_history(
        "agent-msg",
        &["whoami", "--json"],
        std::time::Duration::from_secs(10),
    )
    .map_err(|error| error.to_string())?;
    let identity: serde_json::Value =
        serde_json::from_slice(&whoami).map_err(|_| "invalid relay identity")?;
    let local_host = identity
        .get("address")
        .and_then(serde_json::Value::as_str)
        .and_then(|address| address.split(':').next())
        .ok_or("relay identity has no server label")?;
    let mut by_host: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for lead in leads {
        let mut parts = lead.split(':');
        let (Some(host), Some(workspace), Some(tab)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err("relay directory returned an invalid address".into());
        };
        if parts.next().is_some() || workspace.is_empty() || tab.is_empty() {
            return Err("relay directory returned an invalid address".into());
        }
        by_host.entry(host).or_default().push(lead);
    }
    let mut observed = 0;
    let mut no_pane = BTreeSet::new();
    let mut live = Vec::new();
    for (host, addresses) in by_host {
        let (program, args): (&str, Vec<&str>) = if host.eq_ignore_ascii_case(local_host) {
            ("herdr", vec!["api", "snapshot"])
        } else {
            ("herdr", vec!["--machine", host, "api", "snapshot"])
        };
        let Ok(output) =
            super::history::read_history(program, &args, std::time::Duration::from_secs(20))
        else {
            continue;
        };
        let Ok(envelope) = serde_json::from_slice::<serde_json::Value>(&output) else {
            continue;
        };
        let Some(snapshot) = envelope
            .get("result")
            .and_then(|result| result.get("snapshot"))
        else {
            continue;
        };
        let Some(workspaces) = snapshot
            .get("workspaces")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let Some(tabs) = snapshot.get("tabs").and_then(serde_json::Value::as_array) else {
            continue;
        };
        let workspace_labels: BTreeMap<_, _> = workspaces
            .iter()
            .filter_map(|workspace| {
                Some((
                    workspace.get("workspace_id")?.as_str()?,
                    workspace.get("label")?.as_str()?,
                ))
            })
            .collect();
        let live_tabs: BTreeSet<_> = tabs
            .iter()
            .filter(|tab| {
                tab.get("pane_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
                    > 0
            })
            .filter_map(|tab| {
                let workspace = workspace_labels.get(tab.get("workspace_id")?.as_str()?)?;
                let tab_label = tab.get("label")?.as_str()?;
                Some((*workspace, tab_label))
            })
            .collect();
        for address in addresses {
            let mut parts = address.split(':');
            let _host = parts.next();
            let workspace = parts.next().unwrap_or_default();
            let tab = parts.next().unwrap_or_default();
            if !workspace.eq_ignore_ascii_case(team) {
                continue;
            }
            observed += 1;
            if live_tabs
                .iter()
                .any(|(w, t)| w.eq_ignore_ascii_case(workspace) && t.eq_ignore_ascii_case(tab))
            {
                live.push((*address).to_owned());
            } else {
                no_pane.insert((*address).to_owned());
            }
        }
    }
    live.sort();
    Ok(LeadObservation {
        expected: leads.len(),
        observed,
        expected_addresses: leads.to_vec(),
        no_pane,
        live,
    })
}

#[derive(Clone, Debug, Default)]
struct PrometheusLeadStatus {
    pane_info: Vec<PrometheusSample>,
    pane_status: Vec<PrometheusSample>,
    lane_status: Vec<PrometheusSample>,
}

#[derive(Clone, Debug)]
struct PrometheusSample {
    labels: BTreeMap<String, String>,
    timestamp_seconds: f64,
    value: f64,
}

#[implements("FR-033-AC-16")]
fn read_prometheus_status(teams: &BTreeSet<String>) -> Result<PrometheusLeadStatus, String> {
    let base = std::env::var("IX_PROMETHEUS_URL")
        .unwrap_or_else(|_| "http://prometheus.luna.ix".to_owned());
    let client = PromHttp::new(&base, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let pane_info_query = "timestamp(herdr_pane_info_ratio)";
    let lane_filter = teams
        .iter()
        .map(|team| format!(".*:{}:.*", regex_escape(team)))
        .collect::<Vec<_>>()
        .join("|");
    let lane_query = format!("timestamp(ix_fleet_lane_state_ratio{{lane=~\"{lane_filter}\"}})");
    Ok(PrometheusLeadStatus {
        pane_info: parse_prometheus_samples(
            &client.query(pane_info_query).map_err(|e| e.to_string())?,
        )?,
        pane_status: parse_prometheus_samples(
            &client
                .query("timestamp(herdr_pane_status_ratio)")
                .map_err(|e| e.to_string())?,
        )?,
        lane_status: parse_prometheus_samples(
            &client.query(&lane_query).map_err(|e| e.to_string())?,
        )?,
    })
}

#[implements("FR-033-AC-16")]
fn regex_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if ".+*?()|[]{}^$\\".contains(ch) {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect()
}

#[implements("FR-033-AC-16")]
fn parse_prometheus_samples(body: &str) -> Result<Vec<PrometheusSample>, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if value.get("status").and_then(serde_json::Value::as_str) != Some("success") {
        return Err("Prometheus query did not succeed".to_owned());
    }
    let rows = value
        .pointer("/data/result")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Prometheus response omitted result rows".to_owned())?;
    rows.iter()
        .map(|row| {
            let labels = row
                .get("metric")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| "Prometheus row omitted labels".to_owned())?
                .iter()
                .map(|(key, value)| {
                    value
                        .as_str()
                        .map(|value| (key.clone(), value.to_owned()))
                        .ok_or_else(|| "Prometheus label was not a string".to_owned())
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let sample = row
                .get("value")
                .and_then(serde_json::Value::as_array)
                .filter(|value| value.len() == 2)
                .ok_or_else(|| "Prometheus row omitted instant value".to_owned())?;
            let parse_number = |value: &serde_json::Value| {
                value
                    .as_f64()
                    .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
            };
            let evaluation_seconds = parse_number(&sample[0])
                .ok_or_else(|| "Prometheus row had invalid sample timestamp".to_owned())?;
            let value = parse_number(&sample[1])
                .ok_or_else(|| "Prometheus row had invalid sample value".to_owned())?;
            if !evaluation_seconds.is_finite() || !value.is_finite() {
                return Err("Prometheus row had non-finite sample".to_owned());
            }
            Ok(PrometheusSample {
                labels,
                timestamp_seconds: value,
                value: 1.0,
            })
        })
        .collect()
}

#[implements("FR-033-AC-16")]
fn status_is_fresh(sample: &PrometheusSample, now_ms: i64) -> bool {
    let Some(sample_ms) = (sample.timestamp_seconds * 1000.0)
        .is_finite()
        .then_some((sample.timestamp_seconds * 1000.0) as i64)
    else {
        return false;
    };
    let age = now_ms.saturating_sub(sample_ms);
    (0..=90_000).contains(&age)
}

#[implements("FR-033-AC-16")]
fn classify_execution_leads(
    expected: &[String],
    no_pane: &BTreeSet<String>,
    prometheus: Option<&PrometheusLeadStatus>,
    state: Option<&ScopeState>,
    samples: &[ExecutionSample],
    now_ms: i64,
) -> (usize, usize) {
    let mut observed = 0;
    let mut working = 0;
    for lead in expected {
        let machine = lead.split(':').next().unwrap_or_default();
        if no_pane.contains(lead)
            && prometheus.is_some_and(|status| host_feed_is_fresh(machine, status, now_ms))
        {
            observed += 1;
            continue;
        }
        if let Some((is_working, _)) = prometheus.and_then(|status| {
            observed_pane_status(lead, status, now_ms)
                .or_else(|| observed_lane_status(lead, status, now_ms))
        }) {
            observed += 1;
            working += usize::from(is_working);
            continue;
        }
        let binding = state.and_then(|scope| scope.bindings.get(&lead.to_lowercase()));
        if let Some(sample) = binding.and_then(|binding| {
            samples.iter().find(|sample| {
                sample.machine == binding.machine
                    && sample.session == binding.session
                    && now_ms.saturating_sub(sample.at_ms).abs() <= 90_000
            })
        }) {
            observed += 1;
            working += usize::from(sample.working);
        }
    }
    (observed, working)
}

#[implements("FR-033-AC-16")]
fn host_feed_is_fresh(machine: &str, status: &PrometheusLeadStatus, now_ms: i64) -> bool {
    status.pane_info.iter().any(|sample| {
        status_is_fresh(sample, now_ms)
            && sample
                .labels
                .get("machine")
                .is_some_and(|host| host.eq_ignore_ascii_case(machine))
    })
}

#[implements("FR-033-AC-16")]
fn observed_pane_status(
    lead: &str,
    status: &PrometheusLeadStatus,
    now_ms: i64,
) -> Option<(bool, i64)> {
    let mut parts = lead.splitn(3, ':');
    let (machine, workspace, tab) = (parts.next()?, parts.next()?, parts.next()?);
    let pane = status
        .pane_info
        .iter()
        .filter(|sample| {
            status_is_fresh(sample, now_ms)
                && sample
                    .labels
                    .get("machine")
                    .is_some_and(|v| v.eq_ignore_ascii_case(machine))
                && sample
                    .labels
                    .get("workspace_label")
                    .is_some_and(|v| v.eq_ignore_ascii_case(workspace))
                && sample
                    .labels
                    .get("tab_label")
                    .is_some_and(|v| v.eq_ignore_ascii_case(tab))
                && sample.value > 0.0
        })
        .find_map(|info| info.labels.get("pane").map(|pane| (info, pane)))?;
    let (info, pane) = pane;
    let matching = status
        .pane_status
        .iter()
        .filter(|sample| {
            status_is_fresh(sample, now_ms)
                && sample.labels.get("machine") == info.labels.get("machine")
                && sample.labels.get("pane") == Some(pane)
                && sample.value > 0.0
        })
        .max_by(|a, b| a.timestamp_seconds.total_cmp(&b.timestamp_seconds))?;
    Some((
        matching
            .labels
            .get("status")
            .is_some_and(|s| s == "working"),
        (matching.timestamp_seconds * 1000.0) as i64,
    ))
}

#[implements("FR-033-AC-16")]
fn observed_lane_status(
    lead: &str,
    status: &PrometheusLeadStatus,
    now_ms: i64,
) -> Option<(bool, i64)> {
    let matching = status
        .lane_status
        .iter()
        .filter(|sample| {
            status_is_fresh(sample, now_ms)
                && sample
                    .labels
                    .get("lane")
                    .is_some_and(|lane| lane.eq_ignore_ascii_case(lead))
                && sample.value > 0.0
        })
        .max_by(|a, b| a.timestamp_seconds.total_cmp(&b.timestamp_seconds))?;
    Some((
        matching
            .labels
            .get("state")
            .is_some_and(|state| state == "working"),
        (matching.timestamp_seconds * 1000.0) as i64,
    ))
}

/// Selects the only started release project for the team; ambiguity fails closed.
#[implements("FR-033-AC-16")]
fn current_release_project<'a>(
    workspace: &'a WorkspaceSnapshot,
    team: &str,
) -> Result<&'a Project, String> {
    let mut candidates = workspace.projects.iter().filter(|project| {
        project.status_category == ProjectStatusCategory::Started
            && project.name.to_ascii_lowercase().contains("release")
            && project
                .team_keys
                .iter()
                .any(|key| key.0.eq_ignore_ascii_case(team))
    });
    let project = candidates
        .next()
        .ok_or_else(|| format!("no started release project found for team {team}"))?;
    if candidates.next().is_some() {
        return Err(format!(
            "multiple started release projects found for team {team}"
        ));
    }
    Ok(project)
}

/// Returns seven UTC-day completion counts only when every selected ticket history is readable.
#[implements("FR-033-AC-18")]
fn daily_linear_closures(
    source: &dyn LinearSource,
    issues: &[Issue],
    now: f64,
) -> Option<Vec<usize>> {
    let ids = issues
        .iter()
        .map(|issue| issue.id.0.clone())
        .collect::<Vec<_>>();
    let histories = source.state_histories(&ids);
    if histories.len() != issues.len() {
        return None;
    }
    let day_now = (now / 86400.0).floor() as i64;
    let mut counts = vec![0usize; 7];
    for history in histories {
        let history = history.ok()?;
        for change in history {
            if change.to != Some(WorkflowStateType::Completed)
                || change.from == Some(WorkflowStateType::Completed)
                || !change.at.is_finite()
                || change.at > now
            {
                continue;
            }
            let days_ago = day_now - (change.at / 86400.0).floor() as i64;
            if (0..7).contains(&days_ago) {
                counts[usize::try_from(days_ago).ok()?] += 1;
            }
        }
    }
    Some(counts)
}

/// Counts GitHub merge timestamps by UTC day for the trailing seven calendar days.
#[implements("FR-033-AC-18")]
fn daily_github_merges(prs: &[crate::retro::sources::PullRequest], now: f64) -> Vec<usize> {
    let day_now = (now / 86400.0).floor() as i64;
    let mut counts = vec![0usize; 7];
    for pr in prs {
        let Some(merged_at) = pr.merged_at.filter(|at| at.is_finite() && *at <= now) else {
            continue;
        };
        let days_ago = day_now - (merged_at / 86400.0).floor() as i64;
        if (0..7).contains(&days_ago)
            && let Ok(index) = usize::try_from(days_ago)
        {
            counts[index] += 1;
        }
    }
    counts
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

fn execution_evidence_usable(
    verdict: crate::agents::Verdict,
    pane_complete: bool,
    transcript_complete: bool,
) -> bool {
    pane_complete && transcript_complete && verdict != crate::agents::Verdict::Unknown
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
    #[trace("FR-033-AC-16")]
    #[test]
    fn current_release_selector_requires_one_started_team_release_project() {
        let project = |id: &str, name: &str, status_category: &str| {
            serde_json::from_value::<Project>(serde_json::json!({
                "id": id,
                "identifier": null,
                "name": name,
                "updated_at": null,
                "description": "",
                "content": null,
                "status": "Started",
                "status_name": null,
                "status_category": status_category,
                "priority": 0,
                "sort_order": 0.0,
                "target_date": null,
                "team_keys": ["QSL"],
                "milestones": [],
                "initiatives": []
            }))
            .unwrap()
        };
        let selected = project("release", "FSL alpha release", "started");
        let planned = project("planned", "QSL next release", "planned");
        let workspace = WorkspaceSnapshot {
            projects: vec![selected.clone(), planned],
            ..Default::default()
        };
        assert_eq!(
            current_release_project(&workspace, "QSL").unwrap().id,
            selected.id
        );

        let ambiguous = WorkspaceSnapshot {
            projects: vec![selected, project("other", "QSL beta release", "started")],
            ..Default::default()
        };
        assert!(current_release_project(&ambiguous, "QSL").is_err());
    }
    #[trace("FR-033-AC-15")]
    #[test]
    fn execution_samples_ignore_unrelated_partial_git_and_process_sources() {
        assert!(super::execution_evidence_usable(
            crate::agents::Verdict::Working,
            true,
            true
        ));
        assert!(!super::execution_evidence_usable(
            crate::agents::Verdict::Unknown,
            true,
            true
        ));
        assert!(!super::execution_evidence_usable(
            crate::agents::Verdict::Working,
            false,
            true
        ));
    }
    #[trace("FR-033-AC-5", "FR-033-AC-13")]
    #[test]
    fn provider_receipt_and_cached_provenance_do_not_inherit_linear_poll_times() {
        let dir = tempfile::tempdir().unwrap();
        let config: Config = serde_json::from_value(serde_json::json!({"version":1,"listen":"127.0.0.1:0","state_path":dir.path().join("state.sqlite"),"scopes":[{"team_id":"fixture","scope_id":"work","selectors":{"team":"SWM"},"leads":["draco:SWM:code"],"herdr_teams":[],"retro_config":"unused"}]})).unwrap();
        let mut collector = Collector::open(config.clone()).unwrap();
        let batch = |linear_at: i64, github_at: i64, available: bool| {
            let record = serde_json::from_value(serde_json::json!({
                "schema_version":1, "kind":"snapshot", "taken_at":"1970-01-01T00:01:40Z",
                "team":"fixture", "window":"24h", "metrics":{}
            }))
            .unwrap();
            TrackerBatch {
                at_ms: linear_at,
                results: vec![(
                    "fixture:work".into(),
                    Ok(TrackerResult {
                        members: BTreeMap::new(),
                        record,
                        build_execution_seconds_24h: None,
                        ready_issues: None,
                        blocked_issues: None,
                        query_identity: "fixture".into(),
                        resolved_selectors: super::super::Selectors::default(),
                        pr_stats: available.then(|| super::super::pr_stats::calculate(&[], 0.0)),
                        review_stats: None,
                        closures_daily: None,
                        merged_prs_daily: None,
                        github_at_ms: github_at,
                    }),
                )],
            }
        };
        collector
            .apply_tracker(batch(1_000_000, 1_020_000, true))
            .unwrap();
        assert_eq!(
            collector.state.scopes["fixture:work"].pr_success_at_ms,
            1_020_000
        );
        collector
            .apply_tracker(batch(1_100_000, 1_020_000, true))
            .unwrap();
        assert_eq!(
            collector.state.scopes["fixture:work"].pr_success_at_ms,
            1_020_000
        );
        collector.apply_tracker(batch(1_200_000, 0, false)).unwrap();
        assert_eq!(
            collector.state.scopes["fixture:work"].pr_success_at_ms,
            1_020_000
        );
        drop(collector);
        let collector = Collector::open(config).unwrap();
        assert_eq!(
            collector.state.scopes["fixture:work"].pr_success_at_ms,
            1_020_000
        );
    }
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

    #[trace("FR-033-AC-16")]
    #[test]
    fn fresh_and_stale_pane_lane_and_host_feeds_classify_lead_coverage() {
        let now_ms = 1_000_000;
        let decoded = parse_prometheus_samples(
            r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{"machine":"luna"},"value":[1000,"999.95"]}]}}"#,
        )
        .unwrap();
        assert_eq!(decoded[0].timestamp_seconds, 999.95);
        let sample = |labels: &[(&str, &str)], timestamp_seconds: f64| PrometheusSample {
            labels: labels
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            timestamp_seconds,
            value: 1.0,
        };
        let status = PrometheusLeadStatus {
            pane_info: vec![
                sample(
                    &[
                        ("machine", "luna"),
                        ("pane", "wZ:p1"),
                        ("workspace_label", "QSL"),
                        ("tab_label", "code"),
                    ],
                    999.95,
                ),
                sample(
                    &[
                        ("machine", "draco"),
                        ("pane", "w1F:p9"),
                        ("workspace_label", "OTHER"),
                        ("tab_label", "shell"),
                    ],
                    999.95,
                ),
                sample(
                    &[
                        ("machine", "terra"),
                        ("pane", "wE:p88"),
                        ("workspace_label", "QSL"),
                        ("tab_label", "stale-info"),
                    ],
                    909.0,
                ),
                sample(
                    &[
                        ("machine", "terra"),
                        ("pane", "wE:p89"),
                        ("workspace_label", "QSL"),
                        ("tab_label", "stale-status"),
                    ],
                    999.95,
                ),
                sample(
                    &[
                        ("machine", "titan"),
                        ("pane", "wB:pZ"),
                        ("workspace_label", "QSL"),
                        ("tab_label", "stale-host"),
                    ],
                    909.0,
                ),
            ],
            pane_status: vec![
                sample(
                    &[
                        ("machine", "luna"),
                        ("pane", "wZ:p1"),
                        ("status", "working"),
                    ],
                    999.95,
                ),
                sample(
                    &[
                        ("machine", "terra"),
                        ("pane", "wE:p88"),
                        ("status", "working"),
                    ],
                    999.95,
                ),
                sample(
                    &[
                        ("machine", "terra"),
                        ("pane", "wE:p89"),
                        ("status", "working"),
                    ],
                    909.0,
                ),
            ],
            lane_status: vec![
                sample(
                    &[
                        ("host", "terra"),
                        ("lane", "terra:QSL:plan"),
                        ("state", "working"),
                    ],
                    999.95,
                ),
                sample(
                    &[
                        ("host", "titan"),
                        ("lane", "titan:QSL:stale-lane"),
                        ("state", "working"),
                    ],
                    909.0,
                ),
            ],
        };
        let expected = vec![
            "luna:QSL:code".to_owned(),
            "terra:QSL:plan".to_owned(),
            "draco:QSL:idle-lead".to_owned(),
            "titan:QSL:missing-feed".to_owned(),
            "terra:QSL:stale-info".to_owned(),
            "terra:QSL:stale-status".to_owned(),
            "titan:QSL:stale-lane".to_owned(),
            "titan:QSL:stale-host".to_owned(),
        ];
        let no_pane = BTreeSet::from([
            "draco:QSL:idle-lead".to_owned(),
            "titan:QSL:missing-feed".to_owned(),
            "titan:QSL:stale-host".to_owned(),
        ]);
        assert_eq!(
            classify_execution_leads(&expected, &no_pane, Some(&status), None, &[], now_ms),
            (3, 2)
        );
    }
}
