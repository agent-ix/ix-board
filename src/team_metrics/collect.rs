// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::github::SelectedGithub;
use super::history::CommandHistory;
use super::timeline::{ExecutionSample, LinearStageAge, Member};
use super::{Config, Error, Event, History, Journal, ScopeState, State};
use crate::fleet::live::{PromHttp, PromQuery};
use crate::retro::sources::{AgentSource, GithubSource, LinearSource};
use crate::{Issue, WorkflowStateType, WorkspaceSnapshot, retro};
use ix_trace_rs::{implements, trace};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    review_source_fresh: bool,
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
const REVIEW_MARKER_STORE_LIMIT: usize = 4 * 1024 * 1024;
const REVIEW_MARKER_RETENTION_MS: i64 = 90 * 86_400_000;
const DAILY_CLOSURE_HISTORY_BUDGET: usize = 50;
const STAGE_HISTORY_ISSUE_BUDGET: usize = 4;
const MIN_STAGE_AGE_COVERAGE: f64 = 0.25;
const PROMETHEUS_CLOCK_SKEW_MS: i64 = 5_000;
const RETRO_SNAPSHOT_MAX_AGE_SECONDS: f64 = 25.0 * 60.0 * 60.0;
const REVIEW_MARKER_IMPORT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewMarkerSync {
    #[serde(default)]
    channels: BTreeMap<String, ReviewMarkerChannel>,
    #[serde(default)]
    next_by_scope: BTreeMap<String, usize>,
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
    coverage: f64,
    requests: u64,
    reason: Option<String>,
}

#[implements("FR-033-AC-17")]
fn marker_channel_rotation(channel_count: usize, cursor: usize) -> Option<(usize, usize)> {
    (channel_count > 0).then(|| {
        let current = cursor % channel_count;
        (current, (current + 1) % channel_count)
    })
}

#[cfg(test)]
#[trace("FR-033-AC-17")]
#[test]
fn marker_channel_rotation_visits_every_channel_before_repeating() {
    let mut cursor = 0;
    let visited = (0..4)
        .map(|_| {
            let (current, next) = marker_channel_rotation(3, cursor).unwrap();
            cursor = next;
            current
        })
        .collect::<Vec<_>>();
    assert_eq!(visited, [0, 1, 2, 0]);
    assert_eq!(marker_channel_rotation(0, 0), None);
}

#[trace("FR-033-AC-17")]
#[test]
fn marker_sync_rotates_across_repositories_and_record_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let repos = vec!["owner/a".into(), "owner/b".into(), "owner/c".into()];
    let mut calls = 0;
    for _ in 0..6 {
        let result = sync_review_markers_with_fetch(
            dir.path(),
            "qsl:completion",
            &repos,
            10_000,
            |_, _, _, _| {
                calls += 1;
                Ok(crate::retro::sources::Page {
                    items: Vec::new(),
                    has_next: false,
                    cursor: None,
                })
            },
        );
        assert_eq!(result.requests, REVIEW_MARKER_PAGE_BUDGET as u64);
    }
    let restored = load_review_marker_sync(dir.path());
    assert_eq!(calls, 6 * REVIEW_MARKER_PAGE_BUDGET);
    assert_eq!(restored.channels.len(), repos.len() * 2);
    assert!(
        restored
            .channels
            .values()
            .all(|channel| channel.backfill_complete)
    );
}

#[test]
fn marker_sync_keeps_two_scope_cursors_coverage_and_comments_separate() {
    let dir = tempfile::tempdir().unwrap();
    let qsl_repos = (0..3).map(|i| format!("owner/qsl-{i}")).collect::<Vec<_>>();
    let swm_repos = (0..4).map(|i| format!("owner/swm-{i}")).collect::<Vec<_>>();
    let mut qsl_seen = BTreeSet::new();
    let mut swm_seen = BTreeSet::new();
    let mut qsl_result = None;
    let mut swm_result = None;

    for _ in 0..8 {
        if qsl_seen.len() < 6 {
            qsl_result = Some(sync_review_markers_with_fetch(
                dir.path(),
                "qsl:completion",
                &qsl_repos,
                10_000,
                |prefix, needle, _, _| {
                    qsl_seen.insert(format!("{prefix}:{needle}"));
                    Ok(crate::retro::sources::Page {
                        items: vec![crate::MarkerComment {
                            id: Some(format!("qsl:{}", needle.trim_start_matches("repo="))),
                            created_at: Some(crate::drift::format_timestamp(9.0)),
                            updated_at: Some(crate::drift::format_timestamp(9.0)),
                            body: format!(
                                "{prefix} repo={} id=SR-1 -->",
                                needle.trim_start_matches("repo=")
                            ),
                        }],
                        has_next: false,
                        cursor: None,
                    })
                },
            ));
        }
        if swm_seen.len() < 8 {
            swm_result = Some(sync_review_markers_with_fetch(
                dir.path(),
                "swarm-tools:department-monitoring",
                &swm_repos,
                10_000,
                |prefix, needle, _, _| {
                    swm_seen.insert(format!("{prefix}:{needle}"));
                    Ok(crate::retro::sources::Page {
                        items: vec![crate::MarkerComment {
                            id: Some(format!("swm:{}", needle.trim_start_matches("repo="))),
                            created_at: Some(crate::drift::format_timestamp(9.0)),
                            updated_at: Some(crate::drift::format_timestamp(9.0)),
                            body: format!(
                                "{prefix} repo={} id=SR-1 -->",
                                needle.trim_start_matches("repo=")
                            ),
                        }],
                        has_next: false,
                        cursor: None,
                    })
                },
            ));
        }
    }

    let qsl_result = qsl_result.unwrap();
    let swm_result = swm_result.unwrap();
    assert_eq!(qsl_seen.len(), 6);
    assert_eq!(swm_seen.len(), 8);
    assert!(qsl_result.complete);
    assert!(swm_result.complete);
    assert_eq!(qsl_result.coverage, 1.0);
    assert_eq!(swm_result.coverage, 1.0);
    assert!(qsl_result.comments.iter().all(|comment| {
        comment
            .id
            .as_deref()
            .is_some_and(|id| id.starts_with("qsl:"))
    }));
    assert!(swm_result.comments.iter().all(|comment| {
        comment
            .id
            .as_deref()
            .is_some_and(|id| id.starts_with("swm:"))
    }));
}

#[trace("FR-033-AC-17")]
#[test]
fn marker_sync_persists_cursor_when_a_later_page_fails() {
    let dir = tempfile::tempdir().unwrap();
    let repos = vec!["owner/a".into()];
    let mut fetch = |_: &str, _: &str, _: Option<f64>, after: Option<&str>| {
        if after.is_some() {
            return Err("temporary source failure".into());
        }
        Ok(crate::retro::sources::Page {
            items: Vec::new(),
            has_next: true,
            cursor: Some("resume-here".into()),
        })
    };
    for _ in 0..2 {
        sync_review_markers_with_fetch(dir.path(), "qsl:completion", &repos, 10_000, &mut fetch);
    }
    let result =
        sync_review_markers_with_fetch(dir.path(), "qsl:completion", &repos, 10_000, &mut fetch);
    assert!(result.reason.is_some());
    let restored = load_review_marker_sync(dir.path());
    assert_eq!(
        restored.channels["qsl:completion:owner/a:reviewer"]
            .next_cursor
            .as_deref(),
        Some("resume-here")
    );
}

#[implements("FR-033-AC-17")]
fn sync_review_markers(
    client: &crate::LinearGraphQlSource,
    state_path: &std::path::Path,
    scope: &str,
    repos: &[String],
    now_ms: i64,
) -> ReviewMarkerRefresh {
    sync_review_markers_with_fetch(
        state_path,
        scope,
        repos,
        now_ms,
        |prefix, needle, since, after| {
            client.fetch_marker_comment_page(prefix, needle, since, after)
        },
    )
}

#[implements("FR-033-AC-17")]
fn sync_review_markers_with_fetch(
    state_path: &std::path::Path,
    scope: &str,
    repos: &[String],
    now_ms: i64,
    mut fetch: impl FnMut(
        &str,
        &str,
        Option<f64>,
        Option<&str>,
    ) -> Result<crate::retro::sources::Page<crate::MarkerComment>, String>,
) -> ReviewMarkerRefresh {
    let mut sync = load_review_marker_sync(state_path);
    let retention_floor = now_ms.saturating_sub(REVIEW_MARKER_RETENTION_MS);
    prune_review_marker_age(&mut sync, retention_floor);
    for channel in sync.channels.values_mut() {
        if !channel.backfill_complete && channel.scan_since_ms.is_none() {
            channel.scan_started_ms = Some(now_ms);
            channel.scan_since_ms = Some(retention_floor);
            channel.next_cursor = None;
        }
    }
    let mut requests = 0u64;
    let mut reasons = Vec::new();
    let mut channels = repos
        .iter()
        .flat_map(|repo| {
            [
                ("<!-- reviewer ", "reviewer"),
                ("<!-- reviewer-dispositions ", "dispositions"),
            ]
            .into_iter()
            .map(move |(prefix, name)| (format!("{scope}:{repo}:{name}"), prefix, repo))
        })
        .collect::<Vec<_>>();
    let own = channels
        .iter()
        .map(|(key, ..)| key.clone())
        .collect::<BTreeSet<_>>();
    let cursor = sync.next_by_scope.get(scope).copied().unwrap_or(0);
    if let Some((rotate, next)) = marker_channel_rotation(channels.len(), cursor) {
        channels.rotate_left(rotate);
        sync.next_by_scope.insert(scope.to_owned(), next);
    }
    for (key, prefix, repo) in channels.into_iter().take(REVIEW_MARKER_PAGE_BUDGET) {
        let mut channel = sync.channels.get(&key).cloned().unwrap_or_default();
        if channel.scan_started_ms.is_none() {
            channel.scan_started_ms = Some(now_ms);
            channel.scan_since_ms = Some(if channel.backfill_complete {
                channel
                    .watermark_ms
                    .saturating_sub(2_000)
                    .max(retention_floor)
            } else {
                retention_floor
            });
            channel.next_cursor = None;
        }
        while requests < REVIEW_MARKER_PAGE_BUDGET as u64 {
            let page = fetch(
                prefix,
                &format!("repo={repo}"),
                channel.scan_since_ms.map(|ms| ms as f64 / 1000.0),
                channel.next_cursor.as_deref(),
            );
            requests = requests.saturating_add(1);
            match page {
                Ok(page) => {
                    for comment in page.items {
                        if let Some(id) = comment.id.clone() {
                            let mut compact = comment;
                            compact.body = compact_marker_body(&compact.body);
                            channel.comments.insert(id, compact);
                        }
                    }
                    prune_marker_channel_age(
                        &mut channel,
                        now_ms.saturating_sub(REVIEW_MARKER_RETENTION_MS),
                    );
                    if page.has_next {
                        channel.next_cursor = page.cursor;
                        sync.channels.insert(key.clone(), channel.clone());
                        if let Err(error) = save_review_marker_sync(state_path, &mut sync) {
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
                        if let Err(error) = save_review_marker_sync(state_path, &mut sync) {
                            reasons.push(error);
                            break;
                        }
                        break;
                    }
                }
                Err(error) => {
                    // The cursor was checkpointed after the prior successful page.
                    sync.channels.insert(key.clone(), channel.clone());
                    let _ = save_review_marker_sync(state_path, &mut sync);
                    reasons.push(error);
                    break;
                }
            }
        }
    }
    let done = own
        .iter()
        .filter(|key| sync.channels.get(*key).is_some_and(|c| c.backfill_complete))
        .count();
    let complete = done == own.len();
    let coverage = if own.is_empty() {
        1.0
    } else {
        done as f64 / own.len() as f64
    };
    let comments = own
        .iter()
        .filter_map(|key| sync.channels.get(key))
        .flat_map(|channel| channel.comments.values().cloned())
        .collect();
    ReviewMarkerRefresh {
        comments,
        complete,
        coverage,
        requests,
        reason: (!reasons.is_empty()).then(|| reasons.join("; ")),
    }
}

#[implements("FR-033-AC-17")]
fn compact_marker_body(body: &str) -> String {
    super::reviews::compact_marker_body(body)
}

fn load_review_marker_sync(state_path: &std::path::Path) -> ReviewMarkerSync {
    let path = review_marker_path(state_path);
    let Ok(bytes) = std::fs::read(&path) else {
        return ReviewMarkerSync::default();
    };
    if bytes.len() > REVIEW_MARKER_IMPORT_LIMIT {
        return ReviewMarkerSync::default();
    }
    let Ok(mut sync) = serde_json::from_slice::<ReviewMarkerSync>(&bytes) else {
        return ReviewMarkerSync::default();
    };
    for channel in sync.channels.values_mut() {
        for comment in channel.comments.values_mut() {
            comment.body = compact_marker_body(&comment.body);
        }
    }
    sync
}

#[implements("FR-033-AC-17")]
fn prune_review_marker_age(sync: &mut ReviewMarkerSync, cutoff_ms: i64) {
    for channel in sync.channels.values_mut() {
        prune_marker_channel_age(channel, cutoff_ms);
    }
}

#[implements("FR-033-AC-17")]
fn prune_marker_channel_age(channel: &mut ReviewMarkerChannel, cutoff_ms: i64) {
    channel.comments.retain(|_, comment| {
        comment
            .updated_at
            .as_deref()
            .or(comment.created_at.as_deref())
            .and_then(|at| crate::drift::timestamp_epoch(at).ok())
            .and_then(|at| (at * 1000.0).is_finite().then_some((at * 1000.0) as i64))
            .is_none_or(|at| at >= cutoff_ms)
    });
}

#[implements("FR-033-AC-17")]
fn prune_oldest_marker_comments(
    sync: &mut ReviewMarkerSync,
    max_bytes: usize,
) -> Result<(), String> {
    loop {
        let bytes = serde_json::to_vec(sync).map_err(|error| error.to_string())?;
        if bytes.len() <= max_bytes {
            return Ok(());
        }
        let oldest = sync
            .channels
            .iter()
            .flat_map(|(channel_key, channel)| {
                channel.comments.iter().map(move |(comment_id, comment)| {
                    let at = comment
                        .updated_at
                        .as_deref()
                        .or(comment.created_at.as_deref())
                        .and_then(|at| crate::drift::timestamp_epoch(at).ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    (channel_key.clone(), comment_id.clone(), at)
                })
            })
            .min_by(|left, right| left.2.total_cmp(&right.2));
        let Some((channel_key, comment_id, _)) = oldest else {
            return Err("review marker channel metadata exceeds store limit".into());
        };
        if let Some(channel) = sync.channels.get_mut(&channel_key) {
            channel.comments.remove(&comment_id);
        }
    }
}

fn save_review_marker_sync(
    state_path: &std::path::Path,
    sync: &mut ReviewMarkerSync,
) -> Result<(), String> {
    let path = review_marker_path(state_path);
    prune_oldest_marker_comments(sync, REVIEW_MARKER_STORE_LIMIT)?;
    let bytes = serde_json::to_vec(sync).map_err(|error| error.to_string())?;
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
        let prometheus_url = prometheus_endpoint(&self.config).ok();
        for scope in &self.config.scopes {
            let key = scope.key();
            let status = scope.selectors.team.as_deref().and_then(|team| {
                prometheus_url
                    .as_deref()
                    .ok_or_else(|| "Prometheus URL is not configured".to_owned())
                    .and_then(|url| read_prometheus_status(url, &BTreeSet::from([team.to_owned()])))
                    .map_err(|error| eprintln!("Prometheus lane status for {team}: {error}"))
                    .ok()
            });
            // The returned Prometheus samples are timestamped while the remote reads run.
            // Compare them against a clock captured after those reads so fresh samples are
            // not rejected as future-dated by request latency.
            let execution_observed_at = super::now_ms().unwrap_or(now);
            let team = scope.selectors.team.as_deref().unwrap_or_default();
            let lane_counts = status
                .as_ref()
                .map(|status| classify_team_lanes(status, team, execution_observed_at));
            if let Some((expected, observed, working)) = lane_counts {
                eprintln!(
                    "Prometheus pane status for team {team}: expected={expected} observed={observed} working={working}"
                );
            }
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
        let tracker_started = Instant::now();
        let source = retro::live::linear_or_unreadable();
        let linear_before = source.request_count().unwrap_or(0);
        eprintln!("tracker refresh started");
        let workspace_started = Instant::now();
        let workspace = tracker_workspace(source.as_ref(), now as f64 / 1000.0 - 86400.0);
        eprintln!(
            "tracker workspace finished in {:.2}s; Linear requests={}",
            workspace_started.elapsed().as_secs_f64(),
            source
                .request_count()
                .unwrap_or(linear_before)
                .saturating_sub(linear_before)
        );
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
                    let (closure_candidates, gate_path_error) = match gate_path_issues(
                        workspace,
                        &summary.issue_ids,
                        &cfg.gate_tickets,
                        scope.selectors.team.as_deref().unwrap_or_default(),
                        now as f64 / 1000.0 - 7.0 * 86400.0,
                    ) {
                        Ok(issues) => (issues, None),
                        Err(error) => (Vec::new(), Some(error)),
                    };
                    if let Some(error) = gate_path_error {
                        eprintln!("gate path PR attribution unavailable: {error}");
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
                    let mut repo_results = Vec::with_capacity(cfg.repos.len());
                    for (repo_index, repo) in cfg.repos.iter().enumerate() {
                        let repo_started = Instant::now();
                        let result =
                            github.pull_requests(repo, now as f64 / 1000.0 - 7.0 * 86_400.0);
                        if let Err(error) = &result {
                            eprintln!("GitHub evidence for {repo}: {error}");
                        }
                        eprintln!(
                            "tracker GitHub repository {} finished in {:.2}s",
                            repo_index + 1,
                            repo_started.elapsed().as_secs_f64()
                        );
                        repo_results.push((repo.clone(), result));
                    }
                    let (all_prs, successful_repo_names, github_errors) =
                        aggregate_repository_pull_requests(repo_results);
                    let successful_repos = successful_repo_names.len();
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
                    let daily_candidates = daily_closure_candidates(&selected, now as f64 / 1000.0);
                    let closure_started = Instant::now();
                    let closure_requests_before = source.request_count().unwrap_or(0);
                    let (closures_daily, mut closures_daily_reason, closures_daily_coverage) =
                        match daily_linear_closures(
                            source.as_ref(),
                            &daily_candidates,
                            &verified_closure_history_path(&scope.key())?,
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
                    eprintln!(
                        "tracker daily Linear histories finished in {:.2}s; requests={}",
                        closure_started.elapsed().as_secs_f64(),
                        source
                            .request_count()
                            .unwrap_or(closure_requests_before)
                            .saturating_sub(closure_requests_before)
                    );
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
                        (|| -> Result<(super::reviews::ReviewStats, Option<String>, bool), String> {
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
                            eprintln!(
                                "review marker sync finished: requests={} comments={} coverage={:.3} complete={} reason_present={}",
                                refreshed.requests,
                                refreshed.comments.len(),
                                refreshed.coverage,
                                refreshed.complete,
                                refreshed.reason.is_some()
                            );
                            marker_requests = marker_requests.saturating_add(refreshed.requests);
                            let mut stats = super::reviews::calculate(
                                &refreshed.comments,
                                &allowed,
                                &created,
                                &prs.iter()
                                    .filter(|pr| pr.open && !pr.draft)
                                    .map(|pr| format!("{}#{}", pr.repo, pr.number))
                                    .collect(),
                                now as f64 / 1000.0,
                            )?;
                            stats.values.insert(
                                "review_marker_backfill_coverage".into(),
                                refreshed.coverage,
                            );
                            stats.values.insert(
                                "review_marker_backfill_complete".into(),
                                f64::from(refreshed.complete),
                            );
                            let source_fresh = refreshed.reason.is_none() && refreshed.requests > 0;
                            Ok((stats, refreshed.reason, source_fresh))
                        })();
                    let mut review_stats_reason = review_result.as_ref().err().cloned();
                    if let Some(error) = &review_stats_reason {
                        eprintln!("review evidence for {}: {error}", scope.key());
                    }
                    let (review_stats, marker_reason, review_source_fresh) = review_result
                        .ok()
                        .map(|(stats, reason, fresh)| (Some(stats), reason, fresh))
                        .unwrap_or((None, None, false));
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
                    let stage_started = Instant::now();
                    let stage_requests_before = source.request_count().unwrap_or(0);
                    let linear_stage_ages = linear_stage_ages_incremental(
                        source.as_ref(),
                        workspace,
                        &summary.issue_ids,
                        &config.state_path,
                        now,
                    );
                    eprintln!(
                        "tracker stage Linear histories finished in {:.2}s; requests={}",
                        stage_started.elapsed().as_secs_f64(),
                        source
                            .request_count()
                            .unwrap_or(stage_requests_before)
                            .saturating_sub(stage_requests_before)
                    );
                    let (effort_range, effort_range_reason) = scope
                        .selectors
                        .team
                        .as_deref()
                        .map(|team| {
                            prometheus_endpoint(config)
                                .and_then(|url| prometheus_effort_range(&url, team, now))
                        })
                        .map(|result| match result {
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
                        review_source_fresh,
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
        eprintln!(
            "tracker refresh finished in {:.2}s; Linear requests={}",
            tracker_started.elapsed().as_secs_f64(),
            source
                .request_count()
                .unwrap_or(linear_before)
                .saturating_sub(linear_before)
        );
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
                    let review_complete =
                        result.review_stats.is_some() && result.review_source_fresh;
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
                    if review_complete && result.github_at_ms > 0 {
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
fn prometheus_endpoint(config: &Config) -> Result<String, String> {
    config
        .prometheus_url
        .clone()
        .or_else(|| std::env::var("IX_PROMETHEUS_URL").ok())
        .ok_or_else(|| "Prometheus URL is not configured".into())
}

#[implements("FR-033-AC-16")]
fn read_prometheus_status(
    base: &str,
    teams: &BTreeSet<String>,
) -> Result<PrometheusLeadStatus, String> {
    let client = PromHttp::new(base, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let team_filter = teams
        .iter()
        .map(|team| regex_escape(team))
        .collect::<Vec<_>>()
        .join("|");
    let info_query = format!("herdr_pane_info_ratio{{workspace_label=~\"^({team_filter})$\"}}");
    let info_values = client.query(&info_query).map_err(|e| e.to_string())?;
    let info_timestamps = client
        .query(&format!("timestamp({info_query})"))
        .map_err(|e| e.to_string())?;
    let info = join_prometheus_samples(&info_values, &info_timestamps)?;
    let status_query = format!(
        "herdr_pane_status_ratio * on(machine,pane) group_left(workspace_label,tab_label) {info_query}"
    );
    let status_values = client.query(&status_query).map_err(|e| e.to_string())?;
    let status_timestamps = client
        .query("timestamp(herdr_pane_status_ratio)")
        .map_err(|e| e.to_string())?;
    let mut pane_status = join_pane_status_timestamps(&status_values, &status_timestamps)?;
    let present = pane_status
        .iter()
        .filter_map(|sample| pane_lane_key(&sample.labels).ok())
        .collect::<BTreeSet<_>>();
    for mut sample in info {
        let key = pane_lane_key(&sample.labels)?;
        if present.contains(&key) {
            continue;
        }
        sample.labels.insert("lane".into(), key);
        sample.labels.insert("state".into(), "__missing__".into());
        pane_status.push(sample);
    }
    for sample in &mut pane_status {
        let lane = pane_lane_key(&sample.labels)?;
        let status = sample
            .labels
            .remove("status")
            .unwrap_or_else(|| "__missing__".into());
        sample.labels.insert("lane".into(), lane);
        sample.labels.insert("state".into(), status);
    }
    let lane_status = pane_status;
    Ok(PrometheusLeadStatus { lane_status })
}

pub(super) fn join_pane_status_timestamps(
    status_values: &str,
    timestamp_values: &str,
) -> Result<Vec<PrometheusSample>, String> {
    let mut values = parse_prometheus_samples(status_values)?;
    let timestamps = parse_prometheus_samples(timestamp_values)?
        .into_iter()
        .filter_map(|sample| {
            Some((
                (
                    sample.labels.get("machine")?.clone(),
                    sample.labels.get("pane")?.clone(),
                    sample.labels.get("status")?.clone(),
                ),
                sample.value,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    for sample in &mut values {
        let key = (
            sample
                .labels
                .get("machine")
                .ok_or("Prometheus pane status omitted machine")?
                .clone(),
            sample
                .labels
                .get("pane")
                .ok_or("Prometheus pane status omitted pane")?
                .clone(),
            sample
                .labels
                .get("status")
                .ok_or("Prometheus pane status omitted status")?
                .clone(),
        );
        sample.timestamp_seconds = *timestamps
            .get(&key)
            .ok_or("Prometheus pane status timestamp is unavailable")?;
    }
    Ok(values)
}

#[implements("FR-033-AC-16")]
fn pane_lane_key(labels: &BTreeMap<String, String>) -> Result<String, String> {
    let machine = labels
        .get("machine")
        .ok_or_else(|| "Prometheus pane row omitted machine".to_owned())?;
    let pane = labels
        .get("pane")
        .ok_or_else(|| "Prometheus pane row omitted pane".to_owned())?;
    let team = labels
        .get("workspace_label")
        .ok_or_else(|| "Prometheus pane row omitted workspace label".to_owned())?;
    Ok(format!("{machine}:{team}:{pane}"))
}

#[implements("FR-033-AC-16")]
pub(super) fn join_prometheus_samples(
    values_body: &str,
    timestamps_body: &str,
) -> Result<Vec<PrometheusSample>, String> {
    let values = parse_prometheus_samples(values_body)?;
    let timestamps = parse_prometheus_samples(timestamps_body)?
        .into_iter()
        .map(|mut sample| {
            sample.labels.remove("__name__");
            (sample.labels, sample.value)
        })
        .collect::<BTreeMap<_, _>>();
    values
        .into_iter()
        .map(|mut sample| {
            sample.labels.remove("__name__");
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
    (-PROMETHEUS_CLOCK_SKEW_MS..=90_000).contains(&age)
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
    let mut fresh_conflicts = 0;
    let mut stale = 0;
    if let Some((sample_seconds, _, _)) = latest.values().next() {
        let sample_ms = (sample_seconds * 1000.0) as i64;
        eprintln!(
            "Prometheus freshness clock: now_ms={now_ms} sample_ms={sample_ms} age_ms={}",
            now_ms.saturating_sub(sample_ms)
        );
    }
    for (_, states, fresh) in latest.into_values() {
        if fresh && states.len() == 1 && !states.contains("__missing__") {
            observed += 1;
            working += usize::from(states.contains("working"));
        } else if !fresh {
            stale += 1;
        } else {
            fresh_conflicts += 1;
        }
    }
    eprintln!(
        "Prometheus classification details: fresh_single={} fresh_conflict={} stale={}",
        observed, fresh_conflicts, stale
    );
    (expected, observed, working)
}

#[implements("FR-033-AC-13")]
fn aggregate_repository_pull_requests(
    results: Vec<(String, Result<Vec<retro::sources::PullRequest>, String>)>,
) -> (
    Vec<retro::sources::PullRequest>,
    BTreeSet<String>,
    Vec<String>,
) {
    let mut rows = Vec::new();
    let mut covered = BTreeSet::new();
    let mut errors = Vec::new();
    for (repo, result) in results {
        match result {
            Ok(mut repository_rows) => {
                covered.insert(repo);
                rows.append(&mut repository_rows);
            }
            Err(error) => errors.push(format!("{repo}: {error}")),
        }
    }
    (rows, covered, errors)
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
fn verified_closure_history_path(scope: &str) -> Result<PathBuf, String> {
    let cache_path = match std::env::var_os("IX_BOARD_CACHE_PATH") {
        Some(path) => PathBuf::from(path),
        None => crate::default_cache_path().map_err(|error| error.to_string())?,
    };
    Ok(scoped_closure_history_path(&cache_path, scope))
}

fn scoped_closure_history_path(cache_path: &std::path::Path, scope: &str) -> PathBuf {
    cache_path.with_extension(format!(
        "closure-window-v2-{}.json",
        scope.replace(':', "-")
    ))
}

#[implements("FR-033-AC-18")]
fn daily_linear_closures(
    source: &dyn LinearSource,
    issues: &[Issue],
    cache_path: &std::path::Path,
    now: f64,
) -> Result<DailyClosureResult, String> {
    let mut cache: ClosureHistoryStore = std::fs::read(cache_path)
        .ok()
        .filter(|bytes| bytes.len() <= REVIEW_MARKER_STORE_LIMIT)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let selected = issues
        .iter()
        .map(|issue| issue.id.0.clone())
        .collect::<BTreeSet<_>>();
    cache.entries.retain(|id, _| selected.contains(id));
    let mut missing = issues
        .iter()
        .filter(|issue| {
            cache.entries.get(&issue.id.0).is_none_or(|cached| {
                issue.updated_at.is_none() || cached.updated_at != issue.updated_at
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    missing.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    if let Some(cursor) = cache.next_issue_id.as_deref() {
        let split = missing.partition_point(|issue| issue.id.0.as_str() <= cursor);
        missing.rotate_left(split);
    }
    missing.truncate(DAILY_CLOSURE_HISTORY_BUDGET);
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
        return Err(
            "Linear returned a different number of selected-issue histories than requested".into(),
        );
    }
    let mut history_errors = Vec::new();
    for (issue, history) in missing.into_iter().zip(fetched) {
        cache.next_issue_id = Some(issue.id.0.clone());
        match history {
            Ok(history) => {
                cache.entries.insert(
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
                .entries
                .get(&issue.id.0)
                .filter(|cached| cached.updated_at == issue.updated_at)
                .map(|cached| cached.history.clone())
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&cache).map_err(|error| error.to_string())?;
    if bytes.len() > REVIEW_MARKER_STORE_LIMIT {
        return Err("daily closure history cache exceeds 4 MiB".into());
    }
    let temporary = cache_path.with_extension("closure-histories.json.tmp");
    if let Some(parent) = temporary.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(temporary, cache_path).map_err(|error| error.to_string())?;
    let mut covered_issues = Vec::new();
    let mut completed_histories = Vec::new();
    for (issue, history) in issues.iter().zip(histories) {
        let Some(history) = history else {
            continue;
        };
        covered_issues.push(issue.clone());
        completed_histories.push(history);
    }
    let history_coverage = if issues.is_empty() {
        1.0
    } else {
        covered_issues.len() as f64 / issues.len() as f64
    };
    let coverage = history_coverage.clamp(0.0, 1.0);
    let values = daily_linear_closure_counts(&covered_issues, &completed_histories, now)?;
    let reason = (!history_errors.is_empty()).then(|| history_errors.join("; "));
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
struct ClosureHistoryStore {
    #[serde(default)]
    next_issue_id: Option<String>,
    #[serde(default)]
    entries: BTreeMap<String, ClosureHistoryCache>,
}

#[implements("FR-033-AC-18")]
fn daily_closure_candidates(issues: &[Issue], now: f64) -> Vec<Issue> {
    let since = now - 7.0 * 86_400.0;
    issues
        .iter()
        .filter(|issue| {
            issue
                .updated_at
                .as_deref()
                .and_then(|updated| crate::drift::timestamp_epoch(updated).ok())
                .is_none_or(|updated| updated >= since)
        })
        .cloned()
        .collect()
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
fn daily_linear_closure_counts(
    issues: &[Issue],
    histories: &[Vec<retro::sources::StateChange>],
    now: f64,
) -> Result<Vec<usize>, String> {
    if histories.len() != issues.len() {
        return Err(
            "Linear returned a different number of selected-issue histories than requested".into(),
        );
    }
    let day_now = (now / 86400.0).floor() as i64;
    let mut counts = vec![0usize; 7];
    let mut seen = BTreeSet::new();
    for (issue, history) in issues.iter().zip(histories) {
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
) -> Result<Vec<Issue>, String> {
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
        if !visited.insert(id.clone()) {
            continue;
        }
        if visited.len() > 256 {
            return Err("gate path expansion exceeded 256 selected issues".into());
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
    Ok(visited
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
        .collect())
}

/// Counts GitHub merge timestamps by UTC day for the trailing seven calendar days.
#[implements("FR-033-AC-18")]
fn daily_github_merges(prs: &[crate::retro::sources::PullRequest], now: f64) -> Vec<usize> {
    let day_now = (now / 86400.0).floor() as i64;
    let mut counts = vec![0usize; 7];
    let mut seen = BTreeSet::new();
    for pr in prs {
        if !seen.insert((pr.repo.as_str(), pr.number)) {
            continue;
        }
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
                available: ages.len() as f64 / expected.max(1) as f64 >= MIN_STAGE_AGE_COVERAGE,
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
    if !retro_snapshot_is_fresh(age) {
        return (
            None,
            Some("latest 1d retro snapshot is stale or from the future".into()),
        );
    }
    (Some(record), None)
}

#[implements("FR-033-AC-5")]
fn retro_snapshot_is_fresh(age_seconds: f64) -> bool {
    (0.0..=RETRO_SNAPSHOT_MAX_AGE_SECONDS).contains(&age_seconds)
}

#[implements("FR-033-AC-15")]
fn prometheus_effort_range(
    base: &str,
    team: &str,
    now_ms: i64,
) -> Result<super::timeline::EffortRange, String> {
    let client = PromHttp::new(base, Duration::from_secs(20)).map_err(|e| e.to_string())?;
    let end = now_ms as f64 / 1000.0;
    let start = end - 86400.0;
    let filter = regex_escape(team);
    let inventory = format!("herdr_pane_info_ratio{{workspace_label=~\"^({filter})$\"}}");
    let inventory_values = client.query(&inventory).map_err(|e| e.to_string())?;
    let inventory_timestamps = client
        .query(&format!("timestamp({inventory})"))
        .map_err(|e| e.to_string())?;
    let expected_hosts =
        expected_effort_hosts(&inventory_values, &inventory_timestamps, team, now_ms)?;
    if expected_hosts.is_empty() {
        return Err("no fresh team host inventory is available".into());
    }
    let selector = format!("ix_fleet_lane_state_ratio{{lane=~\".*:{filter}:.*\"}}");
    let timestamps = client
        .query_range(&format!("timestamp({selector})"), start, end, 60)
        .map_err(|e| e.to_string())?;
    let values = client
        .query_range(&selector, start, end, 60)
        .map_err(|e| e.to_string())?;
    parse_effort_range(&timestamps, &values, &expected_hosts, start, end)
}

#[implements("FR-033-AC-15")]
fn expected_effort_hosts(
    inventory_values: &str,
    inventory_timestamps: &str,
    team: &str,
    now_ms: i64,
) -> Result<BTreeSet<String>, String> {
    Ok(
        join_prometheus_samples(inventory_values, inventory_timestamps)?
            .into_iter()
            .filter(|sample| {
                status_is_fresh(sample, now_ms)
                    && sample
                        .labels
                        .get("workspace_label")
                        .is_some_and(|workspace| workspace.eq_ignore_ascii_case(team))
            })
            .filter_map(|sample| sample.labels.get("machine").cloned())
            .collect(),
    )
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
    let mut expected_hosts = expected_hosts.clone();
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
        expected_hosts.insert(host.clone());
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
            let key = (host.clone(), lane.clone(), slot);
            if slot >= first_slot
                && slot < last_slot
                && value > 0.0
                && fresh_states
                    .get(&key)
                    .is_some_and(|fresh| fresh.contains(&state))
            {
                active_states.entry(key).or_default().insert(state.clone());
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
        fail_issue_id: Option<&'static str>,
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
            issue_id: &str,
        ) -> Result<Vec<retro::sources::StateChange>, String> {
            if self.fail_issue_id == Some(issue_id) {
                return Err("history unavailable".into());
            }
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

    #[trace("FR-033-AC-13")]
    #[test]
    fn partial_repository_population_keeps_covered_rows_and_errors() {
        let pull_request = |repo: &str, number| retro::sources::PullRequest {
            repo: repo.into(),
            number,
            open: true,
            draft: false,
            created_at: 1_000.0,
            merged_at: None,
            closed_at: None,
            updated_at: 1_000.0,
            additions: 1,
            deletions: 1,
            review_decision: None,
            reopen_events: Some(0),
            force_pushes: Some(0),
        };
        let (rows, covered, errors) = aggregate_repository_pull_requests(vec![
            (
                "owner/covered".into(),
                Ok(vec![pull_request("owner/covered", 7)]),
            ),
            ("owner/unavailable".into(), Err("source timeout".into())),
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].repo, "owner/covered");
        assert_eq!(covered, BTreeSet::from(["owner/covered".into()]));
        assert_eq!(errors, ["owner/unavailable: source timeout"]);
    }

    #[trace("FR-033-AC-18")]
    #[test]
    fn daily_closure_history_reads_only_issues_updated_in_window() {
        let issue = |id: &str, updated_at: Option<&str>| {
            serde_json::from_value::<Issue>(serde_json::json!({
                "id":id,"identifier":format!("QSL-{id}"),"title":"example","description":null,
                "state":"Started","state_name":"In Progress","state_type":"started",
                "parent_id":null,"team_key":"QSL","assignee_id":null,"assignee_name":null,
                "project_id":null,"project_milestone_id":null,"priority":0,"sort_order":0,
                "blocked_by":[],"created_at":"2026-11-01T00:00:00Z","updated_at":updated_at,
                "attachment_urls":[],"attachments_complete":true,"started_at":null,
                "completed_at":null,"pull_requests":[]
            }))
            .unwrap()
        };
        let now = crate::drift::timestamp_epoch("2026-11-26T02:10:00Z").unwrap();
        let candidates = daily_closure_candidates(
            &[
                issue("recent", Some("2026-11-25T00:00:00Z")),
                issue("old", Some("2026-11-18T00:00:00Z")),
                issue("unknown", None),
            ],
            now,
        );
        assert_eq!(
            candidates
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["QSL-recent", "QSL-unknown"])
        );
    }

    #[test]
    fn daily_closure_histories_keep_two_scope_caches_independent() {
        let directory = tempfile::tempdir().unwrap();
        let issue = |id: &str, key: &str| {
            serde_json::from_value::<Issue>(serde_json::json!({
                "id": id,
                "identifier": format!("{key}-{id}"),
                "title": "closed path",
                "description": null,
                "state": "Done",
                "state_name": "Done",
                "state_type": "completed",
                "parent_id": null,
                "team_key": key,
                "assignee_id": null,
                "assignee_name": null,
                "project_id": null,
                "project_milestone_id": null,
                "priority": 0,
                "sort_order": 0,
                "blocked_by": [],
                "created_at": "2026-11-20T00:00:00Z",
                "updated_at": "2026-11-25T00:00:00Z",
                "attachment_urls": [],
                "attachments_complete": true,
                "started_at": null,
                "completed_at": null,
                "pull_requests": []
            }))
            .unwrap()
        };
        let base = directory.path().join("linear.sqlite3");
        let qsl_path = scoped_closure_history_path(&base, "fsl:completion");
        let swm_path = scoped_closure_history_path(&base, "swarm-tools:department-monitoring");
        let qsl_issues = vec![issue("q1", "QSL"), issue("q2", "QSL")];
        let swm_issues = vec![issue("s1", "SWM"), issue("s2", "SWM"), issue("s3", "SWM")];
        let source = ClosureTestLinear {
            history: Vec::new(),
            fail_issue_id: None,
        };

        let qsl_first = daily_linear_closures(&source, &qsl_issues, &qsl_path, 200_000.0).unwrap();
        let swm = daily_linear_closures(&source, &swm_issues, &swm_path, 200_000.0).unwrap();
        let qsl_again = daily_linear_closures(&source, &qsl_issues, &qsl_path, 200_000.0).unwrap();
        let qsl_cache: ClosureHistoryStore =
            serde_json::from_slice(&std::fs::read(qsl_path).unwrap()).unwrap();
        let swm_cache: ClosureHistoryStore =
            serde_json::from_slice(&std::fs::read(swm_path).unwrap()).unwrap();

        assert_eq!(qsl_first.coverage, 1.0);
        assert_eq!(swm.coverage, 1.0);
        assert_eq!(qsl_again.coverage, 1.0);
        assert_eq!(qsl_cache.entries.len(), 2);
        assert_eq!(swm_cache.entries.len(), 3);
    }

    #[trace("FR-033-AC-18")]
    #[test]
    fn seven_day_merge_window_includes_day_four_and_deduplicates_pull_requests() {
        let now = 10.0 * 86_400.0 + 1_000.0;
        let merged = |repo: &str, number, days_ago| retro::sources::PullRequest {
            repo: repo.into(),
            number,
            open: false,
            draft: false,
            created_at: now - 100_000.0,
            merged_at: Some(now - days_ago * 86_400.0),
            closed_at: Some(now - days_ago * 86_400.0),
            updated_at: now,
            additions: 1,
            deletions: 1,
            review_decision: None,
            reopen_events: Some(0),
            force_pushes: Some(0),
        };
        let day_four = merged("owner/repo", 7, 4.0);
        let counts = daily_github_merges(
            &[day_four.clone(), day_four, merged("owner/repo", 8, 0.0)],
            now,
        );
        assert_eq!(counts.len(), 7);
        assert_eq!(counts[0], 1);
        assert_eq!(counts[4], 1);
        assert_eq!(counts.iter().sum::<usize>(), 2);
    }

    #[trace("FR-033-AC-18")]
    #[test]
    fn daily_closures_keep_read_histories_when_one_selected_issue_history_fails() {
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
            fail_issue_id: Some("bad"),
        };
        let result = daily_linear_closures(
            &linear,
            &issues,
            &directory.path().join("closure-history.json"),
            100_000.0,
        )
        .unwrap();
        assert_eq!(result.values[0], 1);
        assert_eq!(result.coverage, 0.5);
        assert!(
            result
                .reason
                .unwrap()
                .contains("QSL-2: history unavailable")
        );
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

        let mut partial_workspace = workspace.clone();
        partial_workspace
            .issues
            .push(issue("4", "QSL-4", "In Progress", "started", None));
        partial_workspace
            .issues
            .push(issue("5", "QSL-5", "In Progress", "started", None));
        partial_workspace
            .issues
            .push(issue("6", "QSL-6", "In Progress", "started", None));
        let partial_ages = linear_stage_ages_from_histories(
            &partial_workspace,
            &BTreeSet::from([
                crate::IssueId("1".into()),
                crate::IssueId("2".into()),
                crate::IssueId("3".into()),
                crate::IssueId("4".into()),
                crate::IssueId("5".into()),
                crate::IssueId("6".into()),
            ]),
            &history_cache,
            crate::drift::timestamp_epoch("2026-11-25T21:00:00Z").unwrap() as i64 * 1000,
        );
        assert_eq!(partial_ages[0].coverage, 0.2);
        assert!(!partial_ages[0].available);
    }
    #[trace("FR-033-AC-18")]
    #[test]
    fn daily_closures_count_every_selected_completed_state_transition() {
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
        let counts = daily_linear_closure_counts(&issues, &histories, 200_000.0).unwrap();
        assert_eq!(counts, vec![3, 0, 0, 0, 0, 0, 0]);
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
                        review_source_fresh: false,
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
    fn daily_retro_snapshots_stay_fresh_until_the_next_scheduled_run() {
        assert!(retro_snapshot_is_fresh(24.0 * 60.0 * 60.0));
        assert!(retro_snapshot_is_fresh(RETRO_SNAPSHOT_MAX_AGE_SECONDS));
        assert!(!retro_snapshot_is_fresh(
            RETRO_SNAPSHOT_MAX_AGE_SECONDS + 1.0
        ));
        assert!(!retro_snapshot_is_fresh(-1.0));
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
        save_review_marker_sync(&state_path, &mut sync).unwrap();
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

    #[trace("FR-033-AC-17")]
    #[test]
    fn marker_backfill_starts_at_the_retention_floor_and_store_prunes_oldest_rows() {
        let directory = tempfile::tempdir().unwrap();
        let state_path = directory.path().join("exporter.sqlite");
        let mut captured_since = None;
        let result = sync_review_markers_with_fetch(
            directory.path(),
            "qsl:completion",
            &["owner/repo".into()],
            200 * 86_400_000,
            |_, _, since, _| {
                captured_since = since;
                Ok(crate::retro::sources::Page {
                    items: Vec::new(),
                    has_next: true,
                    cursor: Some("resume".into()),
                })
            },
        );
        assert_eq!(result.requests, 1);
        assert_eq!(
            captured_since,
            Some((200 * 86_400_000 - REVIEW_MARKER_RETENTION_MS) as f64 / 1000.0)
        );
        let mut sync = load_review_marker_sync(directory.path());
        let channel = sync.channels.values_mut().next().unwrap();
        for n in 0..20 {
            channel.comments.insert(
                format!("comment-{n:02}"),
                crate::MarkerComment {
                    id: Some(format!("comment-{n:02}")),
                    created_at: Some(crate::drift::format_timestamp(n as f64)),
                    updated_at: Some(crate::drift::format_timestamp(n as f64)),
                    body: format!(
                        "<!-- reviewer repo=owner/repo id=SR-{n} -->\n{}",
                        "x".repeat(120)
                    ),
                },
            );
        }
        prune_oldest_marker_comments(&mut sync, 2_000).unwrap();
        let channel = sync.channels.values().next().unwrap();
        assert!(!channel.comments.contains_key("comment-00"));
        assert!(channel.comments.len() < 20);
        save_review_marker_sync(&state_path, &mut sync).unwrap();
        assert!(
            std::fs::metadata(review_marker_path(&state_path))
                .unwrap()
                .len()
                <= 2_000
        );
    }

    #[trace("FR-033-AC-15")]
    #[test]
    fn effort_host_inventory_includes_hosts_without_current_lane_rows() {
        let values = serde_json::json!({
            "status":"success",
            "data":{"result":[
                {"metric":{"machine":"draco","workspace_label":"QSL"},"value":[1000,"1"]},
                {"metric":{"machine":"luna","workspace_label":"QSL"},"value":[1000,"1"]}
            ]}
        })
        .to_string();
        let timestamps = serde_json::json!({
            "status":"success",
            "data":{"result":[
                {"metric":{"machine":"draco","workspace_label":"QSL"},"value":[1000,"1000"]},
                {"metric":{"machine":"luna","workspace_label":"QSL"},"value":[1000,"1000"]}
            ]}
        })
        .to_string();
        let hosts = expected_effort_hosts(&values, &timestamps, "QSL", 1_000_000).unwrap();
        assert_eq!(hosts, BTreeSet::from(["draco".into(), "luna".into()]));
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
