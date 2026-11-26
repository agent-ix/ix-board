// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::github::SelectedGithub;
use super::history::CommandHistory;
use super::timeline::{ExecutionSample, LinearStageAge, Member};
use super::{Config, Error, Event, History, Journal, ScopeState, State};
use crate::fleet::live::{PromHttp, PromQuery};
use crate::retro::sources::{AgentSource, GithubSource, LinearSource};
use crate::{Issue, WorkflowStateType, WorkspaceSnapshot, retro};
use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// A completed tracker refresh, passed across a bounded worker channel.
pub struct TrackerBatch {
    at_ms: i64,
    linear_requests: u64,
    results: Vec<(String, Result<TrackerResult, String>)>,
}
struct TrackerResult {
    members: BTreeMap<String, Member>,
    record: Option<retro::Record>,
    record_reason: Option<String>,
    pr_stats_reason: Option<String>,
    review_stats_reason: Option<String>,
    closures_daily_reason: Option<String>,
    merged_prs_daily_reason: Option<String>,
    closures_daily_coverage: f64,
    merged_prs_daily_coverage: f64,
    build_execution_seconds_24h: Option<f64>,
    ready_issues: Option<usize>,
    blocked_issues: Option<usize>,
    wip_active: Option<usize>,
    query_identity: String,
    resolved_selectors: super::Selectors,
    pr_stats: Option<super::pr_stats::PrStats>,
    review_stats: Option<super::reviews::ReviewStats>,
    closures_daily: Option<Vec<usize>>,
    merged_prs_daily: Option<Vec<usize>>,
    linear_stage_ages: Vec<LinearStageAge>,
    effort_range: Option<super::timeline::EffortRange>,
    effort_range_reason: Option<String>,
    github_repo_coverage: f64,
    github_at_ms: i64,
}

struct DailyClosureResult {
    values: Vec<usize>,
    coverage: f64,
    reason: Option<String>,
}

const REVIEW_MARKER_PAGE_BUDGET: usize = 1;
const REVIEW_MARKER_STORE_LIMIT: usize = 16 * 1024 * 1024;
const STAGE_HISTORY_ISSUE_BUDGET: usize = 2;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewMarkerSync {
    #[serde(default)]
    channels: BTreeMap<String, ReviewMarkerChannel>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewMarkerChannel {
    #[serde(default)]
    backfill_complete: bool,
    #[serde(default)]
    scan_started_ms: Option<i64>,
    #[serde(default)]
    scan_since_ms: Option<i64>,
    #[serde(default)]
    next_cursor: Option<String>,
    #[serde(default)]
    watermark_ms: i64,
    #[serde(default)]
    comments: BTreeMap<String, crate::MarkerComment>,
}

struct ReviewMarkerRefresh {
    comments: Vec<crate::MarkerComment>,
    complete: bool,
    requests: u64,
    reason: Option<String>,
}

#[implements("FR-033-AC-17")]
fn sync_review_markers(
    client: &crate::LinearGraphQlSource,
    state_path: &std::path::Path,
    scope: &str,
    repos: &[String],
    now_ms: i64,
) -> ReviewMarkerRefresh {
    let mut sync = load_review_marker_sync(state_path);
    let mut requests = 0u64;
    let mut reasons = Vec::new();
    let mut remaining = REVIEW_MARKER_PAGE_BUDGET;
    for repo in repos {
        for (prefix, channel_name) in [
            ("<!-- reviewer ", "reviewer"),
            ("<!-- reviewer-dispositions ", "dispositions"),
        ] {
            if remaining == 0 {
                break;
            }
            let key = format!("{scope}:{repo}:{channel_name}");
            let mut channel = sync.channels.get(&key).cloned().unwrap_or_default();
            if channel.scan_started_ms.is_none() {
                channel.scan_started_ms = Some(now_ms);
                channel.scan_since_ms = channel
                    .backfill_complete
                    .then_some(channel.watermark_ms.saturating_sub(2_000));
                channel.next_cursor = None;
            }
            while remaining > 0 {
                let page = client.fetch_marker_comment_page(
                    prefix,
                    &format!("repo={repo}"),
                    channel.scan_since_ms.map(|ms| ms as f64 / 1000.0),
                    channel.next_cursor.as_deref(),
                );
                requests = requests.saturating_add(1);
                remaining -= 1;
                match page {
                    Ok(page) => {
                        for comment in page.items {
                            if let Some(id) = comment.id.clone() {
                                let mut compact = comment;
                                compact.body = compact_marker_body(&compact.body);
                                channel.comments.insert(id, compact);
                            }
                        }
                        if page.has_next {
                            channel.next_cursor = page.cursor;
                            sync.channels.insert(key.clone(), channel.clone());
                            if let Err(error) = save_review_marker_sync(state_path, &sync) {
                                reasons.push(error);
                                break;
                            }
                        } else {
                            channel.backfill_complete = true;
                            channel.watermark_ms = channel
                                .scan_started_ms
                                .unwrap_or(now_ms)
                                .max(channel.watermark_ms);
                            channel.scan_started_ms = None;
                            channel.scan_since_ms = None;
                            channel.next_cursor = None;
                            sync.channels.insert(key.clone(), channel.clone());
                            if let Err(error) = save_review_marker_sync(state_path, &sync) {
                                reasons.push(error);
                                break;
                            }
                            break;
                        }
                    }
                    Err(error) => {
                        // The cursor was checkpointed after the prior successful page.
                        sync.channels.insert(key.clone(), channel.clone());
                        let _ = save_review_marker_sync(state_path, &sync);
                        reasons.push(error);
                        break;
                    }
                }
            }
        }
    }
    let complete = sync.channels.len() == repos.len() * 2
        && sync
            .channels
            .values()
            .all(|channel| channel.backfill_complete);
    let comments = sync
        .channels
        .values()
        .flat_map(|channel| channel.comments.values().cloned())
        .collect();
    ReviewMarkerRefresh {
        comments,
        complete,
        requests,
        reason: (!reasons.is_empty()).then(|| reasons.join("; ")),
    }
}

#[implements("FR-033-AC-17")]
fn compact_marker_body(body: &str) -> String {
    let mut compact = body.lines().next().unwrap_or_default().to_owned();
    let mut in_block = false;
    let mut fence_width = 0usize;
    for line in body.lines().skip(1) {
        let trimmed = line.trim_start();
        if !in_block {
            let ticks = trimmed.bytes().take_while(|byte| *byte == b'`').count();
            if ticks >= 3 && matches!(trimmed[ticks..].trim(), "yaml" | "yml") {
                in_block = true;
                fence_width = ticks;
                compact.push('\n');
                compact.push_str(line);
                compact.push('\n');
            }
        } else {
            compact.push_str(line);
            compact.push('\n');
            let trimmed = line.trim_start();
            let ticks = trimmed.bytes().take_while(|byte| *byte == b'`').count();
            if ticks >= fence_width && trimmed[ticks..].trim().is_empty() {
                break;
            }
        }
        if compact.len() > 65_536 {
            return body.lines().next().unwrap_or_default().to_owned();
        }
    }
    compact
}

fn load_review_marker_sync(state_path: &std::path::Path) -> ReviewMarkerSync {
    let path = review_marker_path(state_path);
    std::fs::read(&path)
        .ok()
        .filter(|bytes| bytes.len() <= REVIEW_MARKER_STORE_LIMIT)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_review_marker_sync(
    state_path: &std::path::Path,
    sync: &ReviewMarkerSync,
) -> Result<(), String> {
    let path = review_marker_path(state_path);
    let bytes = serde_json::to_vec(sync).map_err(|error| error.to_string())?;
    if bytes.len() > REVIEW_MARKER_STORE_LIMIT {
        return Err("review marker cursor store exceeds 16 MiB".into());
    }
    let temporary = path.with_extension("markers.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn review_marker_path(state_path: &std::path::Path) -> PathBuf {
    state_path.with_extension("review-markers.json")
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
        let execution_observed_at = super::now_ms().unwrap_or(now);
        for scope in &self.config.scopes {
            let key = scope.key();
            let status = scope.selectors.team.as_deref().and_then(|team| {
                read_prometheus_status(&BTreeSet::from([team.to_owned()]))
                    .map_err(|error| eprintln!("Prometheus lane status for {team}: {error}"))
                    .ok()
            });
            let team = scope.selectors.team.as_deref().unwrap_or_default();
            let lane_counts = status
                .as_ref()
                .map(|status| classify_team_lanes(status, team, execution_observed_at));
            if let Some(state) = self.state.scopes.get_mut(&key) {
                let (expected, observed, working) = lane_counts.unwrap_or_default();
                state.execution_population_expected = expected;
                state.execution_population_observed = observed;
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
        let complete = result
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
    #[implements("FR-033-AC-16", "FR-033-AC-17", "FR-033-AC-18")]
    pub fn tracker(config: &Config, now: i64) -> TrackerBatch {
        let source = retro::live::linear_or_unreadable();
        let linear_before = source.request_count().unwrap_or(0);
        let workspace = tracker_workspace(source.as_ref(), now as f64 / 1000.0 - 86400.0);
        // The native workspace reader stamps completed observations. Anchor
        // calculations after that read, rather than mistaking read duration
        // for source clock skew against the poll's starting instant.
        let now = super::now_ms().unwrap_or(now).max(now);
        let source_sync_ms = verified_sync_ms(source.cache_freshness()).unwrap_or(now);
        let mut marker_requests = 0u64;
        let results = config
            .scopes
            .iter()
            .map(|scope| {
                let result = (|| {
                    let workspace = workspace.as_ref().map_err(Clone::clone)?;
                    let selection = scope.selectors.selection();
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
                    let closure_candidates = gate_path_issues(
                        workspace,
                        &summary.issue_ids,
                        &cfg.gate_tickets,
                        scope.selectors.team.as_deref().unwrap_or_default(),
                        now as f64 / 1000.0 - 7.0 * 86400.0,
                    );
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
                    let wip_active = selected
                        .iter()
                        .filter(|issue| {
                            crate::issue_is_open(issue)
                                && issue.state_type == WorkflowStateType::Started
                        })
                        .count();
                    let attachments_complete = selected.iter().all(|i| i.attachments_complete);
                    let allowed: BTreeSet<(String, u64)> = selected
                        .iter()
                        .flat_map(|i| &i.attachment_urls)
                        .filter_map(|u| retro::metrics::pull_request_ref(u))
                        .collect();
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
                    let closure_prs = closure_candidates
                        .iter()
                        .flat_map(|issue| &issue.attachment_urls)
                        .filter_map(|url| retro::metrics::pull_request_ref(url))
                        .collect();
                    let github = SelectedGithub::new_with_priority(
                        config,
                        allowed,
                        closure_prs,
                        attachments_complete,
                        now,
                    );
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
                    use sha2::Digest;
                    let query_identity = format!(
                        "{:x}",
                        sha2::Sha256::digest(serde_json::to_vec(&cfg).map_err(|e| e.to_string())?)
                    );
                    let (record, record_reason) = latest_retro_record(&cfg.team, now);
                    let mut all_prs = Vec::new();
                    let mut successful_repos = 0usize;
                    let mut successful_repo_names = BTreeSet::new();
                    let mut github_errors = Vec::new();
                    for repo in &cfg.repos {
                        match github.pull_requests(repo, now as f64 / 1000.0 - 604800.0) {
                            Ok(rows) => {
                                successful_repos += 1;
                                successful_repo_names.insert(repo.clone());
                                all_prs.extend(rows);
                            }
                            Err(error) => {
                                eprintln!("GitHub evidence for {repo}: {error}");
                                github_errors.push(format!("{repo}: {error}"));
                            }
                        }
                    }
                    let github_repo_coverage = if cfg.repos.is_empty() {
                        0.0
                    } else {
                        successful_repos as f64 / cfg.repos.len() as f64
                    };
                    let all_prs = (successful_repos > 0).then_some(all_prs);
                    let pr_stats = all_prs
                        .as_ref()
                        .filter(|_| github.observed_at() > 0)
                        .map(|prs| super::pr_stats::calculate(prs, now as f64 / 1000.0));
                    let (closures_daily, mut closures_daily_reason, closures_daily_coverage) =
                        match daily_verified_linear_closures(
                            source.as_ref(),
                            &closure_candidates,
                            &github,
                            &cfg.repos,
                            &successful_repo_names,
                            &verified_closure_history_path()?,
                            now as f64 / 1000.0,
                        ) {
                            Ok(result) if result.coverage > 0.0 => {
                                (Some(result.values), result.reason, result.coverage)
                            }
                            Ok(result) => (
                                None,
                                result.reason.or_else(|| {
                                    Some(
                                        "no configured repository or issue history is covered"
                                            .to_owned(),
                                    )
                                }),
                                0.0,
                            ),
                            Err(reason) => (None, Some(reason), 0.0),
                        };
                    if !github_errors.is_empty() {
                        let reason = github_errors.join("; ");
                        closures_daily_reason = Some(match closures_daily_reason {
                            Some(existing) => format!("{existing}; {reason}"),
                            None => reason,
                        });
                    }
                    let pr_stats_reason = pr_stats.is_none().then(|| {
                        if !github_errors.is_empty() {
                            github_errors.join("; ")
                        } else {
                            "GitHub PR poll has no fresh observation".to_owned()
                        }
                    });
                    let review_result =
                        (|| -> Result<(super::reviews::ReviewStats, Option<String>), String> {
                            let client = crate::LinearGraphQlSource::from_env()
                                .map_err(|e| e.to_string())?;
                            let prs = all_prs
                                .as_ref()
                                .ok_or_else(|| "GitHub PR population unavailable".to_owned())?;
                            let allowed: BTreeSet<_> = prs
                                .iter()
                                .map(|p| format!("{}#{}", p.repo, p.number))
                                .collect();
                            let created = first_review_population(prs);
                            let refreshed = sync_review_markers(
                                &client,
                                &config.state_path,
                                &scope.key(),
                                &cfg.repos,
                                now,
                            );
                            marker_requests = marker_requests.saturating_add(refreshed.requests);
                            let stats = super::reviews::calculate(
                                &refreshed.comments,
                                &allowed,
                                &created,
                                &prs.iter()
                                    .filter(|pr| pr.open && !pr.draft)
                                    .map(|pr| format!("{}#{}", pr.repo, pr.number))
                                    .collect(),
                                now as f64 / 1000.0,
                            )?;
                            Ok((
                                stats,
                                refreshed.reason.or_else(|| {
                                    (!refreshed.complete)
                                        .then(|| "review marker backfill is incomplete".to_owned())
                                }),
                            ))
                        })();
                    let mut review_stats_reason = review_result.as_ref().err().cloned();
                    if let Some(error) = &review_stats_reason {
                        eprintln!("review evidence for {}: {error}", scope.key());
                    }
                    let (review_stats, marker_reason) = review_result
                        .ok()
                        .map(|(stats, reason)| (Some(stats), reason))
                        .unwrap_or((None, None));
                    if marker_reason.is_some() {
                        review_stats_reason = marker_reason;
                    }
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
                    let linear_stage_ages = linear_stage_ages_incremental(
                        source.as_ref(),
                        workspace,
                        &summary.issue_ids,
                        &config.state_path,
                        now,
                    );
                    let (effort_range, effort_range_reason) = scope
                        .selectors
                        .team
                        .as_deref()
                        .map(|team| match prometheus_effort_range(team, now) {
                            Ok(range) => (Some(range), None),
                            Err(error) => (None, Some(error)),
                        })
                        .unwrap_or((None, Some("team selector is missing".into())));
                    Ok(TrackerResult {
                        members,
                        record,
                        record_reason,
                        build_execution_seconds_24h,
                        ready_issues,
                        blocked_issues,
                        wip_active: Some(wip_active),
                        query_identity,
                        resolved_selectors,
                        pr_stats,
                        review_stats,
                        closures_daily,
                        closures_daily_reason,
                        closures_daily_coverage,
                        merged_prs_daily: all_prs
                            .as_ref()
                            .map(|prs| daily_github_merges(prs, now as f64 / 1000.0)),
                        merged_prs_daily_reason: pr_stats_reason.clone(),
                        merged_prs_daily_coverage: github_repo_coverage,
                        pr_stats_reason,
                        review_stats_reason,
                        linear_stage_ages,
                        effort_range,
                        effort_range_reason,
                        github_repo_coverage,
                        github_at_ms: github.observed_at(),
                    })
                })();
                (scope.key(), result)
            })
            .collect();
        TrackerBatch {
            at_ms: source_sync_ms,
            linear_requests: source
                .request_count()
                .unwrap_or(linear_before)
                .saturating_sub(linear_before)
                .saturating_add(marker_requests),
            results,
        }
    }
    /// Commits a completed tracker refresh without holding a network-facing lock.
    pub fn apply_tracker(&mut self, batch: TrackerBatch) -> Result<(), Error> {
        self.state.linear_requests_last_refresh = batch.linear_requests;
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
                        state.comparable_since_ms = batch.at_ms;
                    }
                    state.query_identity = Some(result.query_identity);
                    state.resolved_selectors = Some(result.resolved_selectors);
                    update_members(state, &result.members, &scope.selectors);
                    state.build_execution_seconds_24h = result.build_execution_seconds_24h;
                    state.ready_issues = result.ready_issues;
                    state.blocked_issues = result.blocked_issues;
                    state.wip_active = result.wip_active;
                    state.record = result.record;
                    state.record_reason = result.record_reason;
                    state.pr_stats_reason = result.pr_stats_reason;
                    state.review_stats_reason = result.review_stats_reason;
                    state.closures_daily_reason = result.closures_daily_reason;
                    state.merged_prs_daily_reason = result.merged_prs_daily_reason;
                    state.closures_daily_coverage = result.closures_daily_coverage;
                    state.merged_prs_daily_coverage = result.merged_prs_daily_coverage;
                    state.pr_stats = result.pr_stats;
                    state.review_stats = result.review_stats;
                    state.closures_daily = result.closures_daily;
                    state.merged_prs_daily = result.merged_prs_daily;
                    state.linear_stage_ages = result.linear_stage_ages;
                    state.effort_range = result.effort_range;
                    state.effort_range_reason = result.effort_range_reason;
                    state.github_observed_at_ms = result.github_at_ms;
                    state.github_repo_coverage = result.github_repo_coverage;
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

#[derive(Clone, Debug, Default)]
pub(super) struct PrometheusLeadStatus {
    pub(super) lane_status: Vec<PrometheusSample>,
}

#[derive(Clone, Debug)]
pub(super) struct PrometheusSample {
    pub(super) labels: BTreeMap<String, String>,
    pub(super) timestamp_seconds: f64,
    pub(super) value: f64,
}

#[implements("FR-033-AC-16")]
fn read_prometheus_status(teams: &BTreeSet<String>) -> Result<PrometheusLeadStatus, String> {
    let base = std::env::var("IX_PROMETHEUS_URL")
        .unwrap_or_else(|_| "http://prometheus.luna.ix".to_owned());
    let client = PromHttp::new(&base, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let lane_filter = teams
        .iter()
        .map(|team| format!(".*:{}:.*", regex_escape(team)))
        .collect::<Vec<_>>()
        .join("|");
    let lane_query = format!("ix_fleet_lane_state_ratio{{lane=~\"{lane_filter}\"}}");
    let values = client.query(&lane_query).map_err(|e| e.to_string())?;
    let timestamps = client
        .query(&format!("timestamp({lane_query})"))
        .map_err(|e| e.to_string())?;
    let lane_status = join_prometheus_samples(&values, &timestamps)?;
    Ok(PrometheusLeadStatus { lane_status })
}

#[implements("FR-033-AC-16")]
pub(super) fn join_prometheus_samples(
    values_body: &str,
    timestamps_body: &str,
) -> Result<Vec<PrometheusSample>, String> {
    let values = parse_prometheus_samples(values_body)?;
    let timestamps = parse_prometheus_samples(timestamps_body)?
        .into_iter()
        .map(|sample| (sample.labels, sample.value))
        .collect::<BTreeMap<_, _>>();
    values
        .into_iter()
        .map(|mut sample| {
            sample.timestamp_seconds = *timestamps
                .get(&sample.labels)
                .ok_or_else(|| "Prometheus timestamp query omitted a lane series".to_owned())?;
            Ok(sample)
        })
        .collect()
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
                timestamp_seconds: evaluation_seconds,
                value,
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
pub(super) fn classify_team_lanes(
    status: &PrometheusLeadStatus,
    team: &str,
    now_ms: i64,
) -> (usize, usize, usize) {
    let mut latest = BTreeMap::<String, (f64, BTreeSet<String>, bool)>::new();
    for sample in &status.lane_status {
        let Some(lane) = sample.labels.get("lane") else {
            continue;
        };
        let Some((_, workspace, _)) = lane.split_once(':').and_then(|(host, rest)| {
            rest.split_once(':')
                .map(|(workspace, tab)| (host, workspace, tab))
        }) else {
            continue;
        };
        if !workspace.eq_ignore_ascii_case(team) {
            continue;
        }
        let key = lane.to_ascii_lowercase();
        let Some(state) = sample.labels.get("state") else {
            continue;
        };
        if sample.value <= 0.0 {
            continue;
        }
        match latest.get_mut(&key) {
            Some((timestamp, states, fresh)) if sample.timestamp_seconds > *timestamp => {
                *timestamp = sample.timestamp_seconds;
                states.clear();
                states.insert(state.clone());
                *fresh = status_is_fresh(sample, now_ms);
            }
            Some((timestamp, states, fresh)) if sample.timestamp_seconds == *timestamp => {
                states.insert(state.clone());
                *fresh |= status_is_fresh(sample, now_ms);
            }
            Some(_) => {}
            None => {
                latest.insert(
                    key,
                    (
                        sample.timestamp_seconds,
                        BTreeSet::from([state.clone()]),
                        status_is_fresh(sample, now_ms),
                    ),
                );
            }
        }
    }
    let expected = latest.len();
    let mut observed = 0;
    let mut working = 0;
    for (_, states, fresh) in latest.into_values() {
        if fresh && states.len() == 1 {
            observed += 1;
            working += usize::from(states.contains("working"));
        }
    }
    (expected, observed, working)
}

#[implements("FR-033-AC-17")]
fn first_review_population(prs: &[retro::sources::PullRequest]) -> BTreeMap<String, f64> {
    prs.iter()
        .filter(|pr| !pr.draft)
        .map(|pr| (format!("{}#{}", pr.repo, pr.number), pr.created_at))
        .collect()
}

/// Counts verified path closures by UTC day from retained Linear histories and configured repos.
#[implements("FR-033-AC-18")]
fn verified_closure_history_path() -> Result<PathBuf, String> {
    let cache_path = match std::env::var_os("IX_BOARD_CACHE_PATH") {
        Some(path) => PathBuf::from(path),
        None => crate::default_cache_path().map_err(|error| error.to_string())?,
    };
    Ok(cache_path.with_extension("verified-closure-histories.json"))
}

#[implements("FR-033-AC-18")]
fn daily_verified_linear_closures(
    source: &dyn LinearSource,
    issues: &[Issue],
    github: &dyn GithubSource,
    repos: &[String],
    successful_repos: &BTreeSet<String>,
    cache_path: &std::path::Path,
    now: f64,
) -> Result<DailyClosureResult, String> {
    let mut cache: BTreeMap<String, ClosureHistoryCache> = std::fs::read(cache_path)
        .ok()
        .filter(|bytes| bytes.len() <= REVIEW_MARKER_STORE_LIMIT)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let missing = issues
        .iter()
        .filter(|issue| {
            cache.get(&issue.id.0).is_none_or(|cached| {
                issue.updated_at.is_none() || cached.updated_at != issue.updated_at
            })
        })
        .collect::<Vec<_>>();
    let fetched = if missing.is_empty() {
        Vec::new()
    } else {
        source.state_histories(
            &missing
                .iter()
                .map(|issue| issue.id.0.clone())
                .collect::<Vec<_>>(),
        )
    };
    if fetched.len() != missing.len() {
        return Err("Linear returned a different number of path histories than requested".into());
    }
    let mut history_errors = Vec::new();
    for (issue, history) in missing.into_iter().zip(fetched) {
        match history {
            Ok(history) => {
                cache.insert(
                    issue.id.0.clone(),
                    ClosureHistoryCache {
                        updated_at: issue.updated_at.clone(),
                        history,
                    },
                );
            }
            Err(error) => history_errors.push(format!("{}: {error}", issue.identifier)),
        }
    }
    let histories = issues
        .iter()
        .map(|issue| {
            cache
                .get(&issue.id.0)
                .filter(|cached| cached.updated_at == issue.updated_at)
                .map(|cached| cached.history.clone())
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&cache).map_err(|error| error.to_string())?;
    if bytes.len() > REVIEW_MARKER_STORE_LIMIT {
        return Err("verified closure history cache exceeds 16 MiB".into());
    }
    let temporary = cache_path.with_extension("closure-histories.json.tmp");
    if let Some(parent) = temporary.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(temporary, cache_path).map_err(|error| error.to_string())?;
    let configured: BTreeSet<_> = repos.iter().map(String::as_str).collect();
    let mut refs = BTreeSet::new();
    let mut covered_issues = Vec::new();
    let mut completed_histories = Vec::new();
    for (issue, history) in issues.iter().zip(histories) {
        let Some(history) = history else {
            continue;
        };
        if !issue.attachments_complete {
            history_errors.push(format!(
                "{}: Linear PR attachments incomplete",
                issue.identifier
            ));
            continue;
        }
        refs.extend(
            issue
                .attachment_urls
                .iter()
                .filter_map(|url| retro::metrics::pull_request_ref(url))
                .filter(|(repo, _)| configured.contains(repo.as_str())),
        );
        covered_issues.push(issue.clone());
        completed_histories.push(history);
    }
    let mut merged_by_repo = BTreeMap::<String, BTreeSet<u64>>::new();
    let mut failed_repos = BTreeSet::new();
    let mut github_errors = Vec::new();
    for (repo, number) in refs {
        if !successful_repos.contains(&repo) {
            failed_repos.insert(repo.clone());
            continue;
        }
        match github.pull_request(&repo, number) {
            Ok(info) if info.merged => {
                merged_by_repo.entry(repo).or_default().insert(number);
            }
            Ok(_) => {}
            Err(error) => {
                failed_repos.insert(repo.clone());
                github_errors.push(format!("{repo}#{number}: {error}"));
            }
        }
    }
    let complete_repos = successful_repos
        .iter()
        .filter(|repo| !failed_repos.contains(*repo))
        .collect::<BTreeSet<_>>();
    let repo_coverage = if configured.is_empty() {
        0.0
    } else {
        complete_repos.len() as f64 / configured.len() as f64
    };
    let history_coverage = if issues.is_empty() {
        1.0
    } else {
        covered_issues.len() as f64 / issues.len() as f64
    };
    let coverage = repo_coverage.min(history_coverage).clamp(0.0, 1.0);
    let effective_repos = complete_repos
        .iter()
        .map(|repo| repo.as_str())
        .collect::<BTreeSet<_>>();
    let merged = merged_by_repo
        .into_iter()
        .filter(|(repo, _)| effective_repos.contains(repo.as_str()))
        .flat_map(|(repo, numbers)| {
            numbers
                .into_iter()
                .map(move |number| (repo.clone(), number))
        })
        .collect::<BTreeSet<_>>();
    let values = verified_daily_closure_counts(
        &covered_issues,
        &completed_histories,
        &merged,
        &effective_repos,
        now,
    )?;
    let reason = (!history_errors.is_empty() || !github_errors.is_empty()).then(|| {
        history_errors
            .into_iter()
            .chain(github_errors)
            .collect::<Vec<_>>()
            .join("; ")
    });
    Ok(DailyClosureResult {
        values,
        coverage,
        reason,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClosureHistoryCache {
    updated_at: Option<String>,
    history: Vec<retro::sources::StateChange>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StageHistoryStore {
    #[serde(default)]
    last_issue_id: Option<String>,
    #[serde(default)]
    entries: BTreeMap<String, StageHistoryCache>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StageHistoryCache {
    updated_at: String,
    history: Vec<retro::sources::StateChange>,
}

#[implements("FR-033-AC-18")]
fn verified_daily_closure_counts(
    issues: &[Issue],
    histories: &[Vec<retro::sources::StateChange>],
    merged_prs: &BTreeSet<(String, u64)>,
    repos: &BTreeSet<&str>,
    now: f64,
) -> Result<Vec<usize>, String> {
    if histories.len() != issues.len() {
        return Err("Linear returned a different number of path histories than requested".into());
    }
    let day_now = (now / 86400.0).floor() as i64;
    let mut counts = vec![0usize; 7];
    let mut seen = BTreeSet::new();
    for (issue, history) in issues.iter().zip(histories) {
        if !issue.attachments_complete {
            return Err(format!(
                "Linear PR attachments incomplete for {}",
                issue.identifier
            ));
        }
        let refs: BTreeSet<_> = issue
            .attachment_urls
            .iter()
            .filter_map(|url| retro::metrics::pull_request_ref(url))
            .filter(|(repo, _)| repos.contains(repo.as_str()))
            .collect();
        let verified = refs.iter().any(|reference| merged_prs.contains(reference));
        if !verified {
            continue;
        }
        for change in history {
            if change.to != Some(WorkflowStateType::Completed)
                || change.from == Some(WorkflowStateType::Completed)
                || !change.at.is_finite()
                || change.at > now
            {
                continue;
            }
            let days_ago = day_now - (change.at / 86400.0).floor() as i64;
            if !(0..7).contains(&days_ago) {
                continue;
            }
            let event = (issue.id.0.clone(), change.at.to_bits());
            if seen.insert(event)
                && let Ok(index) = usize::try_from(days_ago)
                && let Some(count) = counts.get_mut(index)
            {
                *count += 1;
            }
        }
    }
    Ok(counts)
}

#[implements("FR-033-AC-18")]
fn gate_path_issues(
    workspace: &WorkspaceSnapshot,
    selected_ids: &BTreeSet<crate::IssueId>,
    gates: &[String],
    team: &str,
    updated_since: f64,
) -> Vec<Issue> {
    let by_id: BTreeMap<_, _> = workspace
        .issues
        .iter()
        .filter(|issue| {
            selected_ids.contains(&issue.id) && issue.team_key.0.eq_ignore_ascii_case(team)
        })
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let mut stack: Vec<_> = gates
        .iter()
        .filter_map(|gate| {
            by_id
                .values()
                .find(|issue| issue.identifier.eq_ignore_ascii_case(gate))
                .map(|issue| issue.id.clone())
        })
        .collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) || visited.len() > 256 {
            continue;
        }
        if let Some(issue) = by_id.get(&id) {
            stack.extend(
                issue
                    .blocked_by
                    .iter()
                    .filter(|blocker| by_id.contains_key(*blocker))
                    .cloned(),
            );
        }
    }
    visited
        .into_iter()
        .filter_map(|id| by_id.get(&id).copied())
        .filter(|issue| {
            issue
                .updated_at
                .as_deref()
                .and_then(|at| crate::drift::timestamp_epoch(at).ok())
                .is_some_and(|at| at >= updated_since)
        })
        .cloned()
        .collect()
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

#[implements("FR-033-AC-16")]
fn linear_stage_ages_incremental(
    source: &dyn LinearSource,
    workspace: &WorkspaceSnapshot,
    selected_ids: &BTreeSet<crate::IssueId>,
    state_path: &std::path::Path,
    now_ms: i64,
) -> Vec<LinearStageAge> {
    let cache_path = state_path.with_extension("stage-histories.json");
    let mut cache: StageHistoryStore = std::fs::read(&cache_path)
        .ok()
        .filter(|bytes| bytes.len() <= REVIEW_MARKER_STORE_LIMIT)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let issues = workspace
        .issues
        .iter()
        .filter(|issue| {
            selected_ids.contains(&issue.id)
                && crate::issue_is_open(issue)
                && issue.state_type == WorkflowStateType::Started
        })
        .collect::<Vec<_>>();
    let mut changed = issues
        .iter()
        .filter(|issue| {
            issue.updated_at.as_ref().is_some_and(|updated_at| {
                cache
                    .entries
                    .get(&issue.id.0)
                    .is_none_or(|entry| &entry.updated_at != updated_at)
            })
        })
        .collect::<Vec<_>>();
    changed.sort_by_key(|issue| issue.id.0.clone());
    if let Some(cursor) = cache.last_issue_id.as_deref() {
        let split = changed.partition_point(|issue| issue.id.0.as_str() <= cursor);
        changed.rotate_left(split);
    }
    let selected = changed
        .into_iter()
        .take(STAGE_HISTORY_ISSUE_BUDGET)
        .collect::<Vec<_>>();
    if !selected.is_empty() {
        let ids = selected
            .iter()
            .map(|issue| issue.id.0.clone())
            .collect::<Vec<_>>();
        let histories = source.state_histories(&ids);
        for (issue, history) in selected.iter().zip(histories) {
            cache.last_issue_id = Some(issue.id.0.clone());
            if let (Some(updated_at), Ok(history)) = (&issue.updated_at, history) {
                cache.entries.insert(
                    issue.id.0.clone(),
                    StageHistoryCache {
                        updated_at: updated_at.clone(),
                        history,
                    },
                );
            }
        }
        if let Ok(bytes) = serde_json::to_vec(&cache)
            && bytes.len() <= REVIEW_MARKER_STORE_LIMIT
        {
            let temporary = cache_path.with_extension("stage-histories.json.tmp");
            if let Some(parent) = temporary.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::File::create(&temporary)
                && file.write_all(&bytes).is_ok()
                && file.sync_all().is_ok()
            {
                let _ = std::fs::rename(temporary, cache_path);
            }
        }
    }
    linear_stage_ages_from_histories(workspace, selected_ids, &cache.entries, now_ms)
}

#[implements("FR-033-AC-16")]
fn linear_stage_ages_from_histories(
    workspace: &WorkspaceSnapshot,
    selected_ids: &BTreeSet<crate::IssueId>,
    history_cache: &BTreeMap<String, StageHistoryCache>,
    now_ms: i64,
) -> Vec<LinearStageAge> {
    let now = now_ms as f64 / 1000.0;
    let issues = workspace
        .issues
        .iter()
        .filter(|issue| {
            selected_ids.contains(&issue.id)
                && crate::issue_is_open(issue)
                && issue.state_type == WorkflowStateType::Started
        })
        .collect::<Vec<_>>();
    let mut populations = BTreeMap::<String, (usize, Vec<f64>)>::new();
    for issue in issues {
        let stage = workflow_stage_label(&issue.state_name);
        let entry = populations.entry(stage).or_default();
        entry.0 += 1;
        if let Some(updated_at) = &issue.updated_at
            && let Some(history) = history_cache.get(&issue.id.0)
            && &history.updated_at == updated_at
            && let Some(started_at) = history
                .history
                .iter()
                .filter(|change| {
                    change.to_name.as_deref() == Some(issue.state_name.as_str())
                        && change.to == Some(WorkflowStateType::Started)
                        && change.at.is_finite()
                        && change.at <= now
                })
                .max_by(|left, right| left.at.total_cmp(&right.at))
                .map(|change| change.at)
        {
            entry.1.push((now - started_at).max(0.0));
        }
    }
    populations
        .into_iter()
        .map(|(stage, (expected, mut ages))| {
            ages.sort_by(f64::total_cmp);
            LinearStageAge {
                stage,
                oldest_seconds: ages.last().copied().unwrap_or_default(),
                coverage: ages.len() as f64 / expected.max(1) as f64,
                available: ages.len() as f64 / expected.max(1) as f64 >= 0.5,
            }
        })
        .collect()
}

#[implements("FR-033-AC-16")]
fn workflow_stage_label(name: &str) -> String {
    let mut label = String::new();
    let mut underscore = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            label.push(character);
            underscore = false;
        } else if !label.is_empty() && !underscore {
            label.push('_');
            underscore = true;
        }
    }
    label.trim_end_matches('_').to_owned()
}

#[implements("FR-033-AC-5", "FR-033-AC-16")]
fn latest_retro_record(team: &str, now_ms: i64) -> (Option<retro::Record>, Option<String>) {
    let store = match retro::store::Store::from_env(team) {
        Ok(store) => store,
        Err(error) => return (None, Some(error.to_string())),
    };
    let records = match store.read_all() {
        Ok(records) => records,
        Err(error) => return (None, Some(error.to_string())),
    };
    let latest = records
        .into_iter()
        .filter(|record| {
            record.team.eq_ignore_ascii_case(team)
                && record.kind == retro::RecordKind::Snapshot
                && record.window == "1d"
        })
        .filter_map(|record| {
            let taken_at = crate::drift::timestamp_epoch(&record.taken_at).ok()?;
            Some((taken_at, record))
        })
        .max_by(|left, right| left.0.total_cmp(&right.0));
    let Some((taken_at, record)) = latest else {
        return (
            None,
            Some("no 1d retro snapshot is stored for this team".into()),
        );
    };
    let age = now_ms as f64 / 1000.0 - taken_at;
    if !(0.0..=900.0).contains(&age) {
        return (
            None,
            Some("latest 1d retro snapshot is stale or from the future".into()),
        );
    }
    (Some(record), None)
}

#[implements("FR-033-AC-15")]
fn prometheus_effort_range(
    team: &str,
    now_ms: i64,
) -> Result<super::timeline::EffortRange, String> {
    let base = std::env::var("IX_PROMETHEUS_URL")
        .unwrap_or_else(|_| "http://prometheus.luna.ix".to_owned());
    let client = PromHttp::new(&base, Duration::from_secs(20)).map_err(|e| e.to_string())?;
    let end = now_ms as f64 / 1000.0;
    let start = end - 86400.0;
    let filter = regex_escape(team);
    let selector = format!("ix_fleet_lane_state_ratio{{lane=~\".*:{filter}:.*\"}}");
    let current_values = client.query(&selector).map_err(|e| e.to_string())?;
    let current_timestamps = client
        .query(&format!("timestamp({selector})"))
        .map_err(|e| e.to_string())?;
    let expected_hosts = join_prometheus_samples(&current_values, &current_timestamps)?
        .into_iter()
        .filter(|sample| status_is_fresh(sample, now_ms))
        .filter_map(|sample| {
            sample.labels.get("host").cloned().or_else(|| {
                sample
                    .labels
                    .get("lane")
                    .and_then(|lane| lane.split(':').next())
                    .map(str::to_owned)
            })
        })
        .collect::<BTreeSet<_>>();
    let timestamps = client
        .query_range(&format!("timestamp({selector})"), start, end, 60)
        .map_err(|e| e.to_string())?;
    let values = client
        .query_range(&selector, start, end, 60)
        .map_err(|e| e.to_string())?;
    parse_effort_range(&timestamps, &values, &expected_hosts, start, end)
}

#[implements("FR-033-AC-15")]
pub(super) fn parse_effort_range(
    timestamp_body: &str,
    value_body: &str,
    expected_hosts: &BTreeSet<String>,
    start_seconds: f64,
    end_seconds: f64,
) -> Result<super::timeline::EffortRange, String> {
    fn rows(body: &str) -> Result<Vec<serde_json::Value>, String> {
        let value: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
        if value.get("status").and_then(serde_json::Value::as_str) != Some("success") {
            return Err("Prometheus range query did not succeed".into());
        }
        value
            .pointer("/data/result")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .ok_or_else(|| "Prometheus range response omitted result rows".to_owned())
    }
    let timestamp_rows = rows(timestamp_body)?;
    let value_rows = rows(value_body)?;
    let first_slot = (start_seconds / 60.0).ceil() as i64;
    let last_slot = (end_seconds / 60.0).floor() as i64;
    let slot_count = usize::try_from(last_slot.saturating_sub(first_slot))
        .map_err(|_| "Prometheus range has invalid sample count")?;
    if slot_count == 0 || slot_count > 1440 {
        return Err("Prometheus range has invalid one-minute coverage grid".into());
    }
    let mut fresh_states = BTreeMap::<(String, String, i64), BTreeSet<String>>::new();
    let mut active_states = BTreeMap::<(String, String, i64), BTreeSet<String>>::new();
    for row in &timestamp_rows {
        let labels = row
            .get("metric")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| "Prometheus range row omitted labels".to_owned())?;
        let label = |key: &str| {
            labels
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let lane = label("lane").ok_or_else(|| "Prometheus range row omitted lane".to_owned())?;
        let state =
            label("state").ok_or_else(|| "Prometheus range row omitted state".to_owned())?;
        let host = label("host")
            .or_else(|| lane.split(':').next().map(str::to_owned))
            .ok_or_else(|| "Prometheus range row omitted host".to_owned())?;
        let values = row
            .get("values")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "Prometheus range row omitted values".to_owned())?;
        for pair in values {
            let Some(pair) = pair.as_array().filter(|pair| pair.len() == 2) else {
                continue;
            };
            let Some(evaluated) = pair[0].as_f64() else {
                continue;
            };
            let source_at = pair[1]
                .as_f64()
                .or_else(|| pair[1].as_str().and_then(|s| s.parse::<f64>().ok()));
            let Some(source_at) = source_at.filter(|at| at.is_finite() && *at > 0.0) else {
                continue;
            };
            let slot = (evaluated / 60.0).round() as i64;
            if slot < first_slot || slot >= last_slot || evaluated - source_at > 90.0 {
                continue;
            }
            fresh_states
                .entry((host.clone(), lane.clone(), slot))
                .or_default()
                .insert(state.clone());
        }
    }
    if expected_hosts.is_empty() {
        return Err("no current QSL host inventory from Prometheus lane states".into());
    }
    for row in &value_rows {
        let labels = row
            .get("metric")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| "Prometheus working range row omitted labels".to_owned())?;
        let label = |key: &str| {
            labels
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let lane =
            label("lane").ok_or_else(|| "Prometheus state range row omitted lane".to_owned())?;
        let state =
            label("state").ok_or_else(|| "Prometheus state range row omitted state".to_owned())?;
        let host = label("host")
            .or_else(|| lane.split(':').next().map(str::to_owned))
            .ok_or_else(|| "Prometheus working range row omitted host".to_owned())?;
        let values = row
            .get("values")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "Prometheus state range row omitted values".to_owned())?;
        for pair in values {
            let Some(pair) = pair.as_array().filter(|pair| pair.len() == 2) else {
                continue;
            };
            let Some(evaluated) = pair[0].as_f64() else {
                continue;
            };
            let value = pair[1]
                .as_f64()
                .or_else(|| pair[1].as_str().and_then(|s| s.parse::<f64>().ok()));
            let Some(value) = value.filter(|value| value.is_finite()) else {
                continue;
            };
            let slot = (evaluated / 60.0).round() as i64;
            if slot >= first_slot && slot < last_slot && value > 0.0 {
                active_states
                    .entry((host.clone(), lane.clone(), slot))
                    .or_default()
                    .insert(state.clone());
            }
        }
    }
    let mut covered_host_slots = 0usize;
    let mut working_lane_seconds = 0.0;
    let mut covered_hosts_by_slot = BTreeMap::<i64, BTreeSet<String>>::new();
    for slot in first_slot..last_slot {
        for (key @ (host, lane, observed_slot), states) in &fresh_states {
            if *observed_slot != slot {
                continue;
            }
            let Some(active) = active_states.get(key) else {
                continue;
            };
            if active.len() != 1 || states.is_disjoint(active) {
                continue;
            }
            covered_hosts_by_slot
                .entry(slot)
                .or_default()
                .insert(host.clone());
            if active.contains("working") {
                working_lane_seconds += 60.0;
            }
            let _ = lane;
        }
        covered_host_slots += covered_hosts_by_slot.get(&slot).map_or(0, BTreeSet::len);
    }
    let coverage = covered_host_slots as f64 / (expected_hosts.len() * slot_count) as f64;
    Ok(super::timeline::EffortRange {
        working_lane_seconds,
        coverage: coverage.clamp(0.0, 1.0),
    })
}

#[implements("FR-033-AC-6")]
fn update_members(
    state: &mut ScopeState,
    new: &BTreeMap<String, Member>,
    selectors: &super::Selectors,
) {
    let baseline = state.selectors.is_some();
    if baseline && state.selectors.as_ref() != Some(selectors) {
        state.comparable_since_ms = state.tracker_at_ms;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;

    struct ClosureTestLinear {
        history: Vec<retro::sources::StateChange>,
    }

    impl retro::sources::LinearSource for ClosureTestLinear {
        fn workspace(&self, _since: f64) -> Result<WorkspaceSnapshot, String> {
            Err("not used".into())
        }

        fn project_open_count(&self, _project: &str) -> Result<usize, String> {
            Err("not used".into())
        }

        fn state_history(
            &self,
            _issue_id: &str,
        ) -> Result<Vec<retro::sources::StateChange>, String> {
            Ok(self.history.clone())
        }

        fn marker_lines(&self, _prefix: &str, _needle: &str) -> Result<Vec<String>, String> {
            Err("not used".into())
        }

        fn open_issues_with_label(
            &self,
            _label: &str,
        ) -> Result<Vec<retro::sources::LabeledIssue>, String> {
            Err("not used".into())
        }
    }

    struct ClosureTestGithub;

    impl retro::sources::GithubSource for ClosureTestGithub {
        fn pull_requests(
            &self,
            _repo: &str,
            _since: f64,
        ) -> Result<Vec<retro::sources::PullRequest>, String> {
            Ok(Vec::new())
        }

        fn pull_request(
            &self,
            repo: &str,
            _number: u64,
        ) -> Result<retro::sources::PullRequestInfo, String> {
            if repo == "owner/bad" {
                return Err("repository unavailable".into());
            }
            Ok(retro::sources::PullRequestInfo {
                merged: true,
                updated_at: 100_000.0,
            })
        }

        fn main_commits(
            &self,
            _repo: &str,
            _branch: &str,
            _since: f64,
        ) -> Result<Vec<retro::sources::MainCommit>, String> {
            Ok(Vec::new())
        }
    }

    #[trace("FR-033-AC-17")]
    #[test]
    fn first_review_population_excludes_draft_pull_requests() {
        let pull_request = |number, draft| retro::sources::PullRequest {
            closed_at: None,
            reopen_events: None,
            repo: "owner/repo".into(),
            number,
            open: true,
            draft,
            created_at: 10.0,
            merged_at: None,
            updated_at: 10.0,
            additions: 0,
            deletions: 0,
            review_decision: None,
            force_pushes: None,
        };
        let created = first_review_population(&[
            pull_request(1, false),
            pull_request(2, true),
            pull_request(3, false),
        ]);
        assert_eq!(
            created.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["owner/repo#1".into(), "owner/repo#3".into()])
        );
    }

    #[trace("FR-033-AC-18")]
    #[test]
    fn daily_closures_keep_successful_repositories_when_one_verification_fails() {
        let directory = tempfile::tempdir().unwrap();
        let issue = |id: &str, identifier: &str, repo: &str, number: u64| {
            serde_json::from_value::<Issue>(serde_json::json!({
                "id": id,
                "identifier": identifier,
                "title": "closed path",
                "description": null,
                "state": "Done",
                "state_name": "Done",
                "state_type": "completed",
                "parent_id": null,
                "team_key": "QSL",
                "assignee_id": null,
                "assignee_name": null,
                "project_id": null,
                "project_milestone_id": null,
                "priority": 0,
                "sort_order": 0,
                "blocked_by": [],
                "created_at": "2026-11-20T00:00:00Z",
                "updated_at": "2026-11-25T00:00:00Z",
                "attachment_urls": [format!("https://github.com/{repo}/pull/{number}")],
                "attachments_complete": true,
                "started_at": null,
                "completed_at": "2026-11-25T00:00:00Z",
                "pull_requests": []
            }))
            .unwrap()
        };
        let issues = vec![
            issue("good", "QSL-1", "owner/good", 1),
            issue("bad", "QSL-2", "owner/bad", 2),
        ];
        let linear = ClosureTestLinear {
            history: vec![retro::sources::StateChange {
                at: 99_900.0,
                from: Some(WorkflowStateType::Started),
                to: Some(WorkflowStateType::Completed),
                to_name: Some("Done".into()),
            }],
        };
        let result = daily_verified_linear_closures(
            &linear,
            &issues,
            &ClosureTestGithub,
            &["owner/good".into(), "owner/bad".into()],
            &BTreeSet::from(["owner/good".into(), "owner/bad".into()]),
            &directory.path().join("closure-history.json"),
            100_000.0,
        )
        .unwrap();
        assert_eq!(result.values[0], 1);
        assert_eq!(result.coverage, 0.5);
        assert!(result.reason.unwrap().contains("owner/bad#2"));
    }

    #[trace("FR-033-AC-16")]
    #[test]
    fn linear_stage_age_only_emits_started_workflow_states_with_measurements() {
        let issue = |id: &str,
                     identifier: &str,
                     state_name: &str,
                     state_type: &str,
                     started_at: Option<&str>| {
            serde_json::from_value::<Issue>(serde_json::json!({
                "id":id,"identifier":identifier,"title":"example","description":null,
                "state":"Started","state_name":state_name,"state_type":state_type,
                "parent_id":null,"team_key":"QSL","assignee_id":null,"assignee_name":null,
                "project_id":null,"project_milestone_id":null,"priority":0,"sort_order":0,
                "blocked_by":[],"created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-25T20:01:00Z",
                "attachment_urls":[],"attachments_complete":true,"started_at":started_at,
                "completed_at":null,"pull_requests":[]
            }))
            .unwrap()
        };
        let workspace = WorkspaceSnapshot {
            issues: vec![
                issue(
                    "1",
                    "QSL-1",
                    "In Progress",
                    "started",
                    Some("2026-11-25T20:00:00Z"),
                ),
                issue("2", "QSL-2", "In Progress", "started", None),
                issue(
                    "3",
                    "QSL-3",
                    "Ready for Review",
                    "unstarted",
                    Some("2026-11-25T00:00:00Z"),
                ),
            ],
            ..Default::default()
        };
        let history_cache = BTreeMap::from([(
            "1".to_owned(),
            StageHistoryCache {
                updated_at: "2026-11-25T20:01:00Z".into(),
                history: vec![retro::sources::StateChange {
                    at: crate::drift::timestamp_epoch("2026-11-25T20:00:00Z").unwrap(),
                    from: Some(WorkflowStateType::Unstarted),
                    to: Some(WorkflowStateType::Started),
                    to_name: Some("In Progress".into()),
                }],
            },
        )]);
        let ages = linear_stage_ages_from_histories(
            &workspace,
            &BTreeSet::from([
                crate::IssueId("1".into()),
                crate::IssueId("2".into()),
                crate::IssueId("3".into()),
            ]),
            &history_cache,
            crate::drift::timestamp_epoch("2026-11-25T21:00:00Z").unwrap() as i64 * 1000,
        );
        assert_eq!(ages.len(), 1);
        assert_eq!(ages[0].stage, "in_progress");
        assert_eq!(ages[0].oldest_seconds, 3600.0);
        assert_eq!(ages[0].coverage, 0.5);
        assert!(ages[0].available);
    }
    #[trace("FR-033-AC-18")]
    #[test]
    fn daily_closures_count_only_merged_pr_verified_path_events() {
        use crate::retro::sources::StateChange;
        let issue = |id: &str, number: Option<u64>| {
            let attachment_urls = number.map_or_else(Vec::new, |number| {
                vec![format!("https://github.com/agent-ix/repo/pull/{number}")]
            });
            serde_json::from_value::<Issue>(serde_json::json!({
                "id":id,"identifier":format!("QSL-{id}"),"title":"example","description":null,
                "state":"Completed","state_name":"Done","state_type":"completed",
                "parent_id":null,"team_key":"QSL","assignee_id":null,"assignee_name":null,
                "project_id":null,"project_milestone_id":null,"priority":0,"sort_order":0,
                "blocked_by":[],"created_at":"2026-11-01T00:00:00Z","updated_at":null,
                "attachment_urls":attachment_urls,"attachments_complete":true,"started_at":null,
                "completed_at":null,"pull_requests":[]
            }))
            .unwrap()
        };
        let event = StateChange {
            at: 180_000.0,
            from: Some(WorkflowStateType::Started),
            to: Some(WorkflowStateType::Completed),
            to_name: Some("Done".into()),
        };
        let issues = vec![issue("1", Some(1)), issue("2", None), issue("3", Some(2))];
        let histories = vec![vec![event.clone()], vec![event.clone()], vec![event]];
        let merged = BTreeSet::from([("agent-ix/repo".into(), 1)]);
        let counts = verified_daily_closure_counts(
            &issues,
            &histories,
            &merged,
            &BTreeSet::from(["agent-ix/repo"]),
            200_000.0,
        )
        .unwrap();
        assert_eq!(counts, vec![1, 0, 0, 0, 0, 0, 0]);
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
                linear_requests: 0,
                results: vec![(
                    "fixture:work".into(),
                    Ok(TrackerResult {
                        members: BTreeMap::new(),
                        record: Some(record),
                        record_reason: None,
                        pr_stats_reason: None,
                        review_stats_reason: None,
                        closures_daily_reason: None,
                        merged_prs_daily_reason: None,
                        closures_daily_coverage: 0.0,
                        merged_prs_daily_coverage: 0.0,
                        build_execution_seconds_24h: None,
                        ready_issues: None,
                        blocked_issues: None,
                        wip_active: None,
                        query_identity: "fixture".into(),
                        resolved_selectors: super::super::Selectors::default(),
                        pr_stats: available.then(|| super::super::pr_stats::calculate(&[], 0.0)),
                        review_stats: None,
                        closures_daily: None,
                        merged_prs_daily: None,
                        linear_stage_ages: Vec::new(),
                        effort_range: None,
                        effort_range_reason: None,
                        github_repo_coverage: 1.0,
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
    #[trace("FR-033-AC-17")]
    #[test]
    fn review_marker_cursor_and_payload_survive_restart_without_prose() {
        let directory = tempfile::tempdir().unwrap();
        let state_path = directory.path().join("exporter.sqlite");
        let mut sync = ReviewMarkerSync::default();
        sync.channels.insert(
            "fsl:agent-ix/repo:reviewer".into(),
            ReviewMarkerChannel {
                backfill_complete: false,
                scan_started_ms: Some(1000),
                scan_since_ms: None,
                next_cursor: Some("opaque-cursor".into()),
                watermark_ms: 0,
                comments: BTreeMap::from([(
                    "comment-id".into(),
                    crate::MarkerComment {
                        id: Some("comment-id".into()),
                        created_at: Some("1970-01-01T00:00:01Z".into()),
                        updated_at: Some("1970-01-01T00:00:01Z".into()),
                        body: compact_marker_body("<!-- reviewer repo=agent-ix/repo -->\nprivate prose\n```yaml\nclean: true\nfindings: []\n```\nmore prose"),
                    },
                )]),
            },
        );
        save_review_marker_sync(&state_path, &sync).unwrap();
        let restored = load_review_marker_sync(&state_path);
        let channel = &restored.channels["fsl:agent-ix/repo:reviewer"];
        assert_eq!(channel.next_cursor.as_deref(), Some("opaque-cursor"));
        assert!(channel.comments["comment-id"].body.contains("clean: true"));
        assert!(
            !channel.comments["comment-id"]
                .body
                .contains("private prose")
        );
        assert!(!channel.comments["comment-id"].body.contains("more prose"));
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    use ix_trace_rs::trace;

    #[trace("FR-033-AC-6")]
    #[test]
    fn changing_selectors_preserves_retained_effort_history() {
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
        assert_eq!(state.effort_points, vec![(1000, 10.0, 30.0)]);
        assert!(state.lifecycle_ok);
        assert_eq!(state.totals["scope_revisions"], 1.0);
    }
}
