// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Live lane-to-ticket pull-request evidence for stale-PR actions (SWM-282).

use super::LanePr;
use crate::{LinearGraphQlSource, PullRequestStatus, ReadService};
use ix_trace_rs::implements;
use relay_core::assignment::AssignmentList;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

trait PullRequestFacts {
    fn issues(&mut self, tickets: &[String]) -> Result<Vec<crate::Issue>, String>;
    fn activity(&self, url: &str) -> Result<PullRequestActivity, String>;
    fn search_open_pull_requests(
        &self,
        _tickets: &[String],
    ) -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PullRequestActivity {
    last_activity_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PrUnavailable {
    pub lane: Option<String>,
    pub ticket: Option<String>,
    pub pr: Option<String>,
    pub source_basis: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct PrRead {
    pub items: Vec<LanePr>,
    pub unavailable: Vec<PrUnavailable>,
    pub available_from_ms: Option<i64>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct WorkEventsPage {
    available_from_ms: i64,
    has_more: bool,
    next_cursor: Option<String>,
    truncated: bool,
    events: Vec<WorkEvent>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct WorkEvent {
    #[serde(default)]
    at_ms: i64,
    binding: WorkEventBinding,
    change: WorkEventChange,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct WorkEventBinding {
    address: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct WorkEventChange {
    agent: Option<WorkEventAgent>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct WorkEventAgent {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    name: String,
    ticket: String,
    #[serde(default)]
    status: String,
}

struct LivePullRequestFacts {
    timeout: Duration,
    repositories: Vec<String>,
    github_token: Option<String>,
    api_root: String,
}

impl PullRequestFacts for LivePullRequestFacts {
    #[implements("FR-041-AC-25")]
    fn issues(&mut self, tickets: &[String]) -> Result<Vec<crate::Issue>, String> {
        let source = LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
        let mut service = ReadService::open_default(source).map_err(|error| error.to_string())?;
        let batch = service.issues(tickets).map_err(|error| error.to_string())?;
        // Keep found issues when one requested ticket is missing. The caller compares each
        // requested ticket with returned issue identifiers and retains per-ticket gaps.
        Ok(batch.issues)
    }

    #[implements("FR-041-AC-25")]
    fn activity(&self, url: &str) -> Result<PullRequestActivity, String> {
        let api_url = github_api_url_at(url, "https://api.github.com")?;
        let token = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"));
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        github_activity(&client, &api_url, token.as_deref().ok())
    }

    #[implements("FR-041-AC-25")]
    fn search_open_pull_requests(
        &self,
        tickets: &[String],
    ) -> Result<Vec<(String, String)>, String> {
        if self.repositories.is_empty() {
            return Err("github_repository_scope_unconfigured".to_owned());
        }
        if self.github_token.is_none() {
            return Err("github_token_unconfigured".to_owned());
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        let mut matches = BTreeMap::<(String, String), ()>::new();
        for repository in &self.repositories {
            let (owner, name) = repository
                .split_once('/')
                .ok_or_else(|| "invalid github_repositories entry".to_owned())?;
            let mut complete = false;
            for page in 1..=100 {
                let url = format!(
                    "{}/repos/{owner}/{name}/pulls?state=open&per_page=100&page={page}",
                    self.api_root.trim_end_matches('/')
                );
                let pull_requests: Vec<serde_json::Value> =
                    github_get(&client, &url, self.github_token.as_deref())?;
                for pull_request in &pull_requests {
                    let title = pull_request
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let branch = pull_request
                        .pointer("/head/ref")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let Some(url) = pull_request
                        .get("html_url")
                        .and_then(serde_json::Value::as_str)
                    else {
                        return Err("GitHub pull request omitted html_url".to_owned());
                    };
                    for ticket in tickets {
                        if contains_ticket(title, ticket) || contains_ticket(branch, ticket) {
                            matches.insert((ticket_key(ticket), url.to_owned()), ());
                        }
                    }
                }
                if pull_requests.len() < 100 {
                    complete = true;
                    break;
                }
            }
            if !complete {
                return Err(format!(
                    "open pull request scan exceeded bound for {repository}"
                ));
            }
        }
        Ok(matches.into_keys().collect())
    }
}

#[implements("FR-041-AC-25")]
fn github_activity(
    client: &reqwest::blocking::Client,
    api_url: &str,
    token: Option<&str>,
) -> Result<PullRequestActivity, String> {
    let facts: serde_json::Value = github_get(client, api_url, token)?;
    let head_sha = facts
        .pointer("/head/sha")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "GitHub pull request response omitted head.sha".to_owned())?;
    let commits_url = github_commit_api_url(api_url, head_sha)?;
    let commit: serde_json::Value = github_get(client, &commits_url, token)?;
    let commit_ms = parse_github_time(commit.pointer("/commit/committer/date"))?;
    let mut review_ms = None;
    for page in 1..=100 {
        let reviews_url = format!("{api_url}/reviews?per_page=100&page={page}");
        let reviews: serde_json::Value = github_get(client, &reviews_url, token)?;
        let reviews = reviews
            .as_array()
            .ok_or_else(|| "GitHub reviews response was not an array".to_owned())?;
        for submitted in reviews
            .iter()
            .filter_map(|review| parse_github_time(review.get("submitted_at")).ok())
        {
            review_ms = Some(review_ms.map_or(submitted, |latest: i64| latest.max(submitted)));
        }
        if reviews.len() < 100 {
            break;
        }
        if page == 100 {
            return Err("GitHub review activity exceeded the 100-page bound".to_owned());
        }
    }
    for page in 1..=100 {
        let comments_url = format!("{api_url}/comments?per_page=100&page={page}");
        let comments: serde_json::Value = github_get(client, &comments_url, token)?;
        let comments = comments
            .as_array()
            .ok_or_else(|| "GitHub review comments response was not an array".to_owned())?;
        for activity in comments
            .iter()
            .flat_map(|comment| [comment.get("created_at"), comment.get("updated_at")])
            .filter_map(|value| parse_github_time(value).ok())
        {
            review_ms = Some(review_ms.map_or(activity, |latest: i64| latest.max(activity)));
        }
        if comments.len() < 100 {
            break;
        }
        if page == 100 {
            return Err("GitHub review comments exceeded the 100-page bound".to_owned());
        }
    }
    Ok(PullRequestActivity {
        last_activity_ms: review_ms.map_or(commit_ms, |time| time.max(commit_ms)),
    })
}

#[implements("FR-041-AC-25")]
fn github_commit_api_url(pull_request_api_url: &str, sha: &str) -> Result<String, String> {
    let (repository, pull_number) = pull_request_api_url
        .split_once("/pulls/")
        .ok_or_else(|| "malformed GitHub pull request API URL".to_owned())?;
    if pull_number.is_empty()
        || !pull_number
            .chars()
            .all(|character| character.is_ascii_digit())
        || sha.is_empty()
        || !sha.chars().all(|character| character.is_ascii_hexdigit())
    {
        return Err("malformed GitHub pull request or head SHA".to_owned());
    }
    Ok(format!("{repository}/commits/{sha}"))
}

#[implements("FR-041-AC-25")]
fn github_get<T: serde::de::DeserializeOwned>(
    client: &reqwest::blocking::Client,
    url: &str,
    token: Option<&str>,
) -> Result<T, String> {
    let mut request = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "ix-board (https://github.com/agent-ix/ix-board)",
        )
        .header("Accept", "application/vnd.github+json");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())
}

#[implements("FR-041-AC-25")]
fn parse_github_time(value: Option<&serde_json::Value>) -> Result<i64, String> {
    let value = value
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "GitHub activity response omitted timestamp".to_owned())?;
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|time| time.timestamp_millis())
        .map_err(|error| error.to_string())
}

#[implements("FR-041-AC-25")]
fn github_api_url_at(url: &str, api_root: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .ok_or_else(|| "unsupported pull request URL".to_owned())?;
    let parts = rest.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" || parts[3].parse::<u64>().is_err() {
        return Err("malformed pull request URL".to_owned());
    }
    Ok(format!(
        "{}/repos/{}/{}/pulls/{}",
        api_root.trim_end_matches('/'),
        parts[0],
        parts[1],
        parts[3]
    ))
}

#[implements("FR-041-AC-25")]
#[implements("FR-041-AC-25")]
fn contains_ticket(text: &str, identifier: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    let identifier = ticket_key(identifier);
    upper.match_indices(&identifier).any(|(start, matched)| {
        let end = start + matched.len();
        let before_ok = upper[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric());
        let after_ok = upper[end..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric());
        before_ok && after_ok
    })
}

#[implements("FR-041-AC-25")]
fn ticket_key(id: &str) -> String {
    id.rsplit('/')
        .next()
        .unwrap_or(id)
        .trim()
        .to_ascii_uppercase()
}

#[implements("FR-041-AC-25")]
fn ledger_assignments(path: &Path) -> Result<Vec<(String, String)>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("open fleet ledger read-only: {error}"))?;
    let mut statement = connection
        .prepare("SELECT lane,ticket FROM assignment_history WHERE replaced_at IS NULL AND sent_at IS NOT NULL")
        .map_err(|error| format!("read fleet assignment history: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .map_err(|error| format!("read fleet assignment history: {error}"))?;
    let mut output = Vec::new();
    for row in rows {
        let (lane, tickets) =
            row.map_err(|error| format!("decode fleet assignment history: {error}"))?;
        for ticket in tickets
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|ticket| !ticket.is_empty())
        {
            output.push((lane.to_lowercase(), ticket_key(ticket)));
        }
    }
    Ok(output)
}

#[implements("FR-041-AC-25")]
fn agent_msg_events_page(
    executable: &Path,
    limit: usize,
    cursor: Option<&str>,
) -> Result<WorkEventsPage, String> {
    let mut command = std::process::Command::new(executable);
    command.args(["work", "events", "--limit", &limit.to_string()]);
    if let Some(cursor) = cursor {
        command.args(["--after", cursor]);
    }
    let output = command
        .output()
        .map_err(|error| format!("run agent-msg work events: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "agent-msg work events exited {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("decode agent-msg work events: {error}"))
}

#[implements("FR-041-AC-25")]
fn read_work_events(freshness_ms: i64) -> Result<(Vec<(String, String)>, i64), String> {
    let executable = std::env::var_os("IX_BOARD_AGENT_MSG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("agent-msg"));
    collect_work_events(freshness_ms, |limit, cursor| {
        agent_msg_events_page(&executable, limit, cursor)
    })
}

#[implements("FR-041-AC-25")]
fn collect_work_events(
    freshness_ms: i64,
    mut read_page: impl FnMut(usize, Option<&str>) -> Result<WorkEventsPage, String>,
) -> Result<(Vec<(String, String)>, i64), String> {
    const PAGE_SIZE: usize = 200;
    const MAX_PAGES: usize = 100;
    let mut cursor = None;
    let mut first_available = None;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "system clock predates Unix epoch".to_owned())?
        .as_millis() as i64;
    let mut latest = BTreeMap::<(String, String), (i64, String, String)>::new();
    for _ in 0..MAX_PAGES {
        let page = read_page(PAGE_SIZE, cursor.as_deref())?;
        match first_available {
            Some(available) if available != page.available_from_ms => {
                return Err(
                    "agent-msg work events coverage boundary changed during pagination".to_owned(),
                );
            }
            None => first_available = Some(page.available_from_ms),
            _ => {}
        }
        for event in page.events {
            if let Some(agent) = event.change.agent {
                let lane = event.binding.address.trim().to_lowercase();
                let identity = agent
                    .agent_id
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(|| {
                        if agent.name.trim().is_empty() {
                            agent.ticket.clone()
                        } else {
                            agent.name.clone()
                        }
                    });
                if !lane.is_empty()
                    && !agent.ticket.trim().is_empty()
                    && !identity.trim().is_empty()
                {
                    let key = (lane, identity);
                    let candidate = (
                        event.at_ms,
                        ticket_key(&agent.ticket),
                        agent.status.to_lowercase(),
                    );
                    if latest
                        .get(&key)
                        .is_none_or(|current| candidate.0 >= current.0)
                    {
                        latest.insert(key, candidate);
                    }
                }
            }
        }
        if page.truncated {
            return Err("agent-msg work events reports truncated coverage".to_owned());
        }
        if !page.has_more {
            let mapping = latest
                .into_iter()
                .filter_map(|((lane, _), (at_ms, ticket, status))| {
                    (status == "running"
                        && at_ms <= now_ms
                        && now_ms.saturating_sub(at_ms) <= freshness_ms)
                        .then_some((lane, ticket))
                })
                .collect();
            return first_available
                .map(|boundary| (mapping, boundary))
                .ok_or_else(|| "agent-msg work events returned no page".to_owned());
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Err("agent-msg work events has_more without next_cursor".to_owned());
        }
    }
    Err(format!("agent-msg work events exceeded {MAX_PAGES} pages"))
}

#[implements("FR-041-AC-25")]
fn read_lane_prs(
    assignments: &BTreeMap<String, Vec<String>>,
    facts: &mut dyn PullRequestFacts,
) -> Result<PrRead, String> {
    let mut ticket_lanes = assignments.clone();
    for lanes in ticket_lanes.values_mut() {
        lanes.sort();
        lanes.dedup();
    }
    let all_tickets = ticket_lanes.keys().cloned().collect::<Vec<_>>();
    let tickets = all_tickets.iter().take(500).cloned().collect::<Vec<_>>();
    let mut initial_unavailable = all_tickets
        .iter()
        .skip(500)
        .map(|ticket| PrUnavailable {
            lane: ticket_lanes
                .get(ticket)
                .and_then(|lanes| lanes.first())
                .cloned(),
            ticket: Some(ticket.clone()),
            pr: None,
            source_basis: "linear_issue_and_pr_links".to_owned(),
            reason: "ticket omitted because live PR reader reached its 500-ticket bound".to_owned(),
        })
        .collect::<Vec<_>>();
    if tickets.is_empty() {
        return Ok(PrRead {
            items: Vec::new(),
            unavailable: vec![PrUnavailable {
                lane: None,
                ticket: None,
                pr: None,
                source_basis:
                    "relay_assignment_lists+fleet_ledger_assignment_history+agent_msg_work_events"
                        .to_owned(),
                reason: "no ticket-to-lane mapping available".to_owned(),
            }],
            available_from_ms: None,
        });
    }
    let mut issues = Vec::new();
    let mut unavailable = std::mem::take(&mut initial_unavailable);
    for chunk in tickets.chunks(50) {
        match facts.issues(chunk) {
            Ok(mut batch) => issues.append(&mut batch),
            Err(error) => {
                for ticket in chunk {
                    unavailable.push(PrUnavailable {
                        lane: ticket_lanes
                            .get(ticket)
                            .and_then(|lanes| lanes.first())
                            .cloned(),
                        ticket: Some(ticket.clone()),
                        pr: None,
                        source_basis: "linear_issue_and_pr_links".to_owned(),
                        reason: format!("Linear read failed: {error}"),
                    });
                }
            }
        }
    }
    let returned = issues
        .iter()
        .map(|issue| ticket_key(&issue.identifier))
        .collect::<std::collections::BTreeSet<_>>();
    for ticket in &tickets {
        if !returned.contains(ticket) {
            unavailable.push(PrUnavailable {
                lane: ticket_lanes
                    .get(ticket)
                    .and_then(|lanes| lanes.first())
                    .cloned(),
                ticket: Some(ticket.clone()),
                pr: None,
                source_basis: "linear_issue_and_pr_links".to_owned(),
                reason: "Linear returned no ticket".to_owned(),
            });
        }
    }
    let mut result = PrRead {
        items: Vec::new(),
        unavailable,
        available_from_ms: None,
    };
    for issue in issues {
        if !issue.attachments_complete {
            result.unavailable.push(PrUnavailable {
                lane: ticket_lanes
                    .get(&ticket_key(&issue.identifier))
                    .and_then(|lanes| lanes.first())
                    .cloned(),
                ticket: Some(issue.identifier.clone()),
                pr: None,
                source_basis: "linear_issue_and_pr_links".to_owned(),
                reason: "Linear attachments incomplete".to_owned(),
            });
            continue;
        }
        let Some(lanes) = ticket_lanes.get(&ticket_key(&issue.identifier)) else {
            result.unavailable.push(PrUnavailable {
                lane: None,
                ticket: Some(issue.identifier.clone()),
                pr: None,
                source_basis: "linear_issue_and_pr_links".to_owned(),
                reason: "Linear returned an issue without a lane mapping".to_owned(),
            });
            continue;
        };
        for pull_request in issue.pull_requests.iter().filter(|pr| {
            matches!(
                pr.status,
                PullRequestStatus::Open | PullRequestStatus::Draft
            )
        }) {
            let activity = match facts.activity(&pull_request.url) {
                Ok(activity) => activity,
                Err(error) => {
                    for lane in lanes {
                        result.unavailable.push(PrUnavailable {
                            lane: Some(lane.clone()),
                            ticket: Some(issue.identifier.clone()),
                            pr: Some(pull_request.url.clone()),
                            source_basis: "github_pull_request_head_and_reviews".to_owned(),
                            reason: error.clone(),
                        });
                    }
                    continue;
                }
            };
            // A Linear attachment is the authoritative ticket-to-PR edge. Title and branch
            // text are not required to repeat the ticket identifier.
            for lane in lanes {
                result.items.push(LanePr {
                    lane: lane.clone(),
                    pr: pull_request.url.clone(),
                    last_activity_ms: activity.last_activity_ms,
                });
            }
        }
    }
    match facts.search_open_pull_requests(&tickets) {
        Ok(pull_requests) => {
            for (ticket, pr) in pull_requests {
                let key = ticket_key(&ticket);
                let Some(lanes) = ticket_lanes.get(&key) else {
                    continue;
                };
                match facts.activity(&pr) {
                    Ok(activity) => {
                        for lane in lanes {
                            if !result
                                .items
                                .iter()
                                .any(|item| item.lane.eq_ignore_ascii_case(lane) && item.pr == pr)
                            {
                                result.items.push(LanePr {
                                    lane: lane.clone(),
                                    pr: pr.clone(),
                                    last_activity_ms: activity.last_activity_ms,
                                });
                            }
                        }
                    }
                    Err(error) => {
                        for lane in lanes {
                            result.unavailable.push(PrUnavailable {
                                lane: Some(lane.clone()),
                                ticket: Some(ticket.clone()),
                                pr: Some(pr.clone()),
                                source_basis: "github_title_branch_ticket_search".to_owned(),
                                reason: error.clone(),
                            });
                        }
                    }
                }
            }
        }
        Err(error) if error == "github_repository_scope_unconfigured" => {
            result.unavailable.push(PrUnavailable {
                lane: None,
                ticket: None,
                pr: None,
                source_basis: "github_repository_title_branch_scan".to_owned(),
                reason: error,
            });
        }
        Err(error) => {
            for ticket in &tickets {
                for lane in ticket_lanes.get(ticket).into_iter().flatten() {
                    result.unavailable.push(PrUnavailable {
                        lane: Some(lane.clone()),
                        ticket: Some(ticket.clone()),
                        pr: None,
                        source_basis: "github_repository_title_branch_scan".to_owned(),
                        reason: error.clone(),
                    });
                }
            }
        }
    }
    Ok(result)
}

#[implements("FR-041-AC-25")]
pub(super) fn read_live(
    timeout: Duration,
    config: &crate::fleet::FleetConfig,
    current_lanes: &[(String, String)],
) -> Result<PrRead, String> {
    let mut source_unavailable = Vec::new();
    let assignments = match read_current_assignments(timeout) {
        Ok(assignments) => assignments,
        Err(error) => {
            source_unavailable.push(PrUnavailable {
                lane: None,
                ticket: None,
                pr: None,
                source_basis: "relay_assignment_lists".to_owned(),
                reason: error,
            });
            Vec::new()
        }
    };
    let mut assignment_map = BTreeMap::<String, Vec<String>>::new();
    for list in assignments {
        for ticket in list.tickets {
            assignment_map
                .entry(ticket_key(&ticket.id))
                .or_default()
                .push(list.lane.to_lowercase());
        }
    }
    match crate::fleet::ledger::resolve_path(config) {
        Ok(path) => match ledger_assignments(&path) {
            Ok(history) => {
                for (lane, ticket) in history {
                    assignment_map.entry(ticket).or_default().push(lane);
                }
            }
            Err(error) => source_unavailable.push(PrUnavailable {
                lane: None,
                ticket: None,
                pr: None,
                source_basis: "fleet_ledger_assignment_history".to_owned(),
                reason: error,
            }),
        },
        Err(error) => source_unavailable.push(PrUnavailable {
            lane: None,
            ticket: None,
            pr: None,
            source_basis: "fleet_ledger_assignment_history".to_owned(),
            reason: error.to_string(),
        }),
    }
    let mut available_from_ms = None;
    let current_lanes = current_lanes
        .iter()
        .map(|(_, lane)| lane.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    match read_work_events(i64::try_from(config.saturation_window.as_millis()).unwrap_or(i64::MAX))
    {
        Ok((events, available_from)) => {
            available_from_ms = Some(available_from);
            for (lane, ticket) in events {
                if current_lanes.contains(&lane.to_lowercase()) {
                    assignment_map.entry(ticket).or_default().push(lane);
                }
            }
        }
        Err(error) => source_unavailable.push(PrUnavailable {
            lane: None,
            ticket: None,
            pr: None,
            source_basis: "agent_msg_work_events".to_owned(),
            reason: error,
        }),
    }
    let mut read = read_lane_prs(
        &assignment_map,
        &mut LivePullRequestFacts {
            timeout,
            repositories: config.github_repositories.clone(),
            github_token: std::env::var("GITHUB_TOKEN")
                .or_else(|_| std::env::var("GH_TOKEN"))
                .ok(),
            api_root: "https://api.github.com".to_owned(),
        },
    )?;
    read.unavailable.extend(source_unavailable);
    read.available_from_ms = available_from_ms;
    Ok(read)
}

#[implements("FR-041-AC-25")]
fn read_current_assignments(timeout: Duration) -> Result<Vec<AssignmentList>, String> {
    let settings = relay_core::config::load_core(relay_core::config::CoreFlags::default())
        .map_err(|error| error.to_string())?;
    let mut relay = relay_core::Relay::connect(&settings.url.value, timeout)
        .map_err(|error| format!("relay connect: {error}"))?
        .with_prefix(settings.prefix.value.clone());
    relay
        .assignments()
        .map_err(|error| format!("relay assignments: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Issue;
    use crate::{IssueId, ProjectId, TeamKey, WorkflowStateType};
    use ix_trace_rs::trace;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    struct Facts(Vec<Issue>);

    impl PullRequestFacts for Facts {
        fn issues(&mut self, _: &[String]) -> Result<Vec<Issue>, String> {
            Ok(self.0.clone())
        }
        fn activity(&self, url: &str) -> Result<PullRequestActivity, String> {
            if url.ends_with("/999") {
                return Err("mock GitHub item unavailable".to_owned());
            }
            Ok(PullRequestActivity {
                last_activity_ms: 12_345,
            })
        }
    }

    struct RepositoryScanUnavailable;

    impl PullRequestFacts for RepositoryScanUnavailable {
        fn issues(&mut self, _: &[String]) -> Result<Vec<Issue>, String> {
            Ok(Vec::new())
        }

        fn activity(&self, _: &str) -> Result<PullRequestActivity, String> {
            unreachable!("there are no PRs without issue data")
        }

        fn search_open_pull_requests(&self, _: &[String]) -> Result<Vec<(String, String)>, String> {
            Err("GitHub pull request scan failed".to_owned())
        }
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn missing_lane_ticket_mapping_remains_unavailable() {
        let result = read_lane_prs(&BTreeMap::new(), &mut Facts(Vec::new())).unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.unavailable.len(), 1);
        assert!(result.unavailable[0].lane.is_none());
        assert!(result.unavailable[0].pr.is_none());
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn repository_scope_failure_is_attached_to_each_mapped_lane() {
        let assignments =
            BTreeMap::from([("QSL-1".to_owned(), vec!["draco:QSL:builder".to_owned()])]);
        let read = read_lane_prs(&assignments, &mut RepositoryScanUnavailable).unwrap();
        assert!(read.unavailable.iter().any(|unavailable| {
            unavailable.lane.as_deref() == Some("draco:QSL:builder")
                && unavailable.pr.is_none()
                && unavailable.source_basis == "github_repository_title_branch_scan"
        }));
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn production_repository_scan_matches_ticket_in_pull_request_branch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let size = stream.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..size]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let response = serde_json::json!([{
                "html_url": "https://github.com/agent-ix/ix-board/pull/102",
                "title": "feature without ticket in title",
                "head": {"ref": "feature/SWM-252-capacity"}
            }])
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
            String::from_utf8(request).unwrap()
        });
        let facts = LivePullRequestFacts {
            timeout: Duration::from_secs(2),
            repositories: vec!["agent-ix/ix-board".to_owned()],
            github_token: Some("test-token".to_owned()),
            api_root: format!("http://{address}"),
        };
        let matches = facts
            .search_open_pull_requests(&["SWM-252".to_owned()])
            .unwrap();
        assert_eq!(
            matches,
            vec![(
                "SWM-252".to_owned(),
                "https://github.com/agent-ix/ix-board/pull/102".to_owned()
            )]
        );
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /repos/agent-ix/ix-board/pulls?state=open"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("user-agent: ix-board")
        );
        assert!(request.contains("authorization: Bearer test-token"));
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn assigned_lane_open_pr_becomes_live_stale_pr_evidence() {
        let mut issue = crate::Issue {
            id: IssueId("uuid".to_owned()),
            identifier: "SWM-252".to_owned(),
            title: "test".to_owned(),
            description: None,
            state: "Started".to_owned(),
            state_name: "In Progress".to_owned(),
            state_type: WorkflowStateType::Started,
            parent_id: None,
            team_key: TeamKey("SWM".to_owned()),
            assignee_id: None,
            assignee_name: None,
            project_id: Some(ProjectId("project".to_owned())),
            project_milestone_id: None,
            priority: 0.0,
            sort_order: 0.0,
            blocked_by: Vec::new(),
            created_at: None,
            updated_at: None,
            attachment_urls: vec!["https://github.com/agent-ix/ix-board/pull/102".to_owned()],
            attachments_complete: true,
            started_at: None,
            completed_at: None,
            pull_requests: vec![crate::PullRequestFact {
                url: "https://github.com/agent-ix/ix-board/pull/102".to_owned(),
                repo: "agent-ix/ix-board".to_owned(),
                number: 102,
                status: PullRequestStatus::Open,
                draft: false,
                created_at: None,
                merged_at: None,
            }],
        };
        issue.pull_requests.push(crate::PullRequestFact {
            url: "https://github.com/agent-ix/ix-board/pull/999".to_owned(),
            repo: "agent-ix/ix-board".to_owned(),
            number: 999,
            status: PullRequestStatus::Open,
            draft: false,
            created_at: None,
            merged_at: None,
        });
        let assignments = BTreeMap::from([
            (
                "SWM-252".to_owned(),
                vec!["draco:workspace:build".to_owned()],
            ),
            (
                "SWM-999".to_owned(),
                vec!["terra:workspace:build".to_owned()],
            ),
        ]);
        let prs = read_lane_prs(&assignments, &mut Facts(vec![issue])).unwrap();
        assert_eq!(prs.items.len(), 1);
        assert_eq!(prs.items[0].lane, "draco:workspace:build");
        assert_eq!(prs.items[0].last_activity_ms, 12_345);
        assert_eq!(prs.unavailable.len(), 2);
        assert!(
            prs.unavailable
                .iter()
                .any(|item| item.ticket.as_deref() == Some("SWM-999"))
        );
        assert!(prs.unavailable.iter().any(
            |item| item.pr.as_deref() == Some("https://github.com/agent-ix/ix-board/pull/999")
        ));
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn production_github_reader_sends_user_agent_and_uses_head_and_review_activity() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 1024];
                loop {
                    let size = stream.read(&mut chunk).unwrap();
                    if size == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..size]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request_text = String::from_utf8(request).unwrap();
                let path = request_text
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_owned();
                assert!(
                    request_text
                        .to_ascii_lowercase()
                        .contains("user-agent: ix-board (https://github.com/agent-ix/ix-board)")
                );
                let body = if path.contains("/commits/") {
                    r#"{"commit":{"committer":{"date":"2026-11-25T10:00:00Z"}}}"#
                } else if path.contains("/comments?") {
                    r#"[{"created_at":"2026-11-25T12:00:00Z","updated_at":"2026-11-25T12:00:00Z"}]"#
                } else if path.contains("/reviews?") {
                    r#"[{"submitted_at":"2026-11-25T11:00:00Z"}]"#
                } else {
                    r#"{"head":{"sha":"abc123"},"updated_at":"2099-01-01T00:00:00Z"}"#
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                requests.push(path);
            }
            requests
        });
        let client = reqwest::blocking::Client::builder().build().unwrap();
        let activity = github_activity(
            &client,
            &format!("http://{address}/repos/agent-ix/ix-board/pulls/102"),
            None,
        )
        .unwrap();
        assert_eq!(
            activity.last_activity_ms,
            chrono::DateTime::parse_from_rfc3339("2026-11-25T12:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
        assert_eq!(
            server.join().unwrap(),
            vec![
                "/repos/agent-ix/ix-board/pulls/102",
                "/repos/agent-ix/ix-board/commits/abc123",
                "/repos/agent-ix/ix-board/pulls/102/reviews?per_page=100&page=1",
                "/repos/agent-ix/ix-board/pulls/102/comments?per_page=100&page=1",
            ]
        );
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn github_urls_are_converted_to_api_urls() {
        assert_eq!(
            github_api_url_at(
                "https://github.com/agent-ix/ix-board/pull/102",
                "https://api.example.test"
            ),
            Ok("https://api.example.test/repos/agent-ix/ix-board/pulls/102".to_owned()),
        );
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn work_event_reader_pages_lane_ticket_mapping_and_preserves_coverage_boundary() {
        let mut cursors = Vec::new();
        let pages = [
            serde_json::json!({
                "available_from_ms": 1700000000000_i64,
                "has_more": true,
                "next_cursor": "cursor-1",
                "truncated": false,
                "events": [{
                    "at_ms": 1700000001000_i64,
                    "binding": {"machine":"draco", "pane":"p1", "session":"session-1", "address":"draco:QSL:arch"},
                    "change": {"kind":"agent", "agent":{"ticket":"QSL-2128", "role":"coder", "status":"running", "reported_at_ms":1700000001000_i64}}
                }]
            }),
            serde_json::json!({
                "available_from_ms": 1700000000000_i64,
                "has_more": false,
                "next_cursor": null,
                "truncated": false,
                "events": [{
                    "at_ms": 1700000002000_i64,
                    "binding": {"machine":"terra", "pane":"p2", "session":"session-2", "address":"terra:QSL:review"},
                    "change": {"kind":"agent", "agent":{"ticket":"SWM-281", "role":"reviewer", "status":"running", "reported_at_ms":1700000002000_i64}}
                }]
            }),
        ];
        let mut index = 0;
        let (mapping, boundary) = collect_work_events(i64::MAX, |limit, cursor| {
            assert_eq!(limit, 200);
            cursors.push(cursor.map(str::to_owned));
            let page = pages[index].clone();
            index += 1;
            serde_json::from_value(page).map_err(|error| error.to_string())
        })
        .unwrap();
        assert_eq!(boundary, 1700000000000);
        assert_eq!(
            mapping,
            vec![
                ("draco:qsl:arch".to_owned(), "QSL-2128".to_owned()),
                ("terra:qsl:review".to_owned(), "SWM-281".to_owned()),
            ]
        );
        assert_eq!(cursors, vec![None, Some("cursor-1".to_owned())]);
    }

    #[trace("FR-041-AC-25")]
    #[test]
    fn work_event_lookback_keeps_mapping_inside_the_saturation_window() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let page_for = |at_ms| {
            serde_json::json!({
                "available_from_ms": at_ms - 1,
                "has_more": false,
                "next_cursor": null,
                "truncated": false,
                "events": [{
                    "at_ms": at_ms,
                    "binding": {"address":"draco:QSL:arch"},
                    "change": {"agent": {
                        "ticket":"QSL-2128", "name":"coder", "status":"running"
                    }}
                }]
            })
        };
        let within_window = now_ms - 10 * 60 * 1000;
        let (recent, _) = collect_work_events(24 * 60 * 60 * 1000, |_, _| {
            serde_json::from_value(page_for(within_window)).map_err(|error| error.to_string())
        })
        .unwrap();
        assert_eq!(recent.len(), 1);

        let outside_window = now_ms - 25 * 60 * 60 * 1000;
        let (old, _) = collect_work_events(24 * 60 * 60 * 1000, |_, _| {
            serde_json::from_value(page_for(outside_window)).map_err(|error| error.to_string())
        })
        .unwrap();
        assert!(old.is_empty());
    }
}
