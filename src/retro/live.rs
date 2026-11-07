// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The real readers behind the traits in [`super::sources`].
//!
//! Each one only reads: Linear queries, `gh api` GET calls, `herdr` reads, `git worktree list`,
//! `ps`, a file read and `redis-cli LRANGE` (which does not dequeue).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;

use super::sources::{
    AgentSource, CheckState, GateLine, GateLogSource, GithubSource, LabeledIssue, LinearSource,
    LocalSource, MainCommit, Page, ProcessRow, PullRequest, RelaySource, StateChange,
    read_all_pages,
};
use crate::agents::{AgentsReport, build_report, system_hosts};
use crate::{LinearGraphQlSource, ReadService, WorkspaceSnapshot};

/// Most state-history reads in flight at once.
const HISTORY_CONCURRENCY: usize = 6;

/// Runs `read` over `inputs` with at most [`HISTORY_CONCURRENCY`] reads in flight, keeping the
/// order of `inputs`. A read that never ran is an error, not a missing value.
fn parallel_map<I: Sync, T: Send + Clone>(
    inputs: &[I],
    read: impl Fn(&I) -> Result<T, String> + Sync,
) -> Vec<Result<T, String>> {
    let next = AtomicUsize::new(0);
    let slots = Mutex::new(vec![None; inputs.len()]);
    std::thread::scope(|scope| {
        for _ in 0..HISTORY_CONCURRENCY.min(inputs.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(input) = inputs.get(index) else {
                        break;
                    };
                    let result = read(input);
                    if let Ok(mut slots) = slots.lock() {
                        slots[index] = Some(result);
                    }
                }
            });
        }
    });
    let slots = slots.into_inner().unwrap_or_default();
    (0..inputs.len())
        .map(|index| {
            slots
                .get(index)
                .cloned()
                .flatten()
                .unwrap_or_else(|| Err("read did not complete".to_owned()))
        })
        .collect()
}

/// Linear through the GraphQL client. The workspace snapshot and the project-chunk read share
/// one cached read service (FR-004), so a warm run fetches the workspace from the cache.
pub struct LiveLinear {
    source: LinearGraphQlSource,
    service: Mutex<ReadService<LinearGraphQlSource>>,
}

impl LiveLinear {
    /// Builds the reader from the saved or environment credential.
    pub fn from_env() -> Result<Self, crate::Error> {
        Ok(Self {
            source: LinearGraphQlSource::from_env()?,
            service: Mutex::new(ReadService::open_default(LinearGraphQlSource::from_env()?)?),
        })
    }

    fn service(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ReadService<LinearGraphQlSource>>, String> {
        self.service
            .lock()
            .map_err(|_| "read service lock poisoned".to_owned())
    }
}

impl LinearSource for LiveLinear {
    fn workspace(&self) -> Result<WorkspaceSnapshot, String> {
        self.service()?
            .workspace_snapshot()
            .map_err(|error| error.to_string())
    }

    fn project_open_count(&self, project: &str) -> Result<usize, String> {
        self.service()?
            .project_chunk(project)
            .map(|chunk| chunk.open_tickets.len())
            .map_err(|error| error.to_string())
    }

    fn state_history(&self, issue_id: &str) -> Result<Vec<StateChange>, String> {
        self.source.fetch_state_changes(issue_id)
    }

    fn state_histories(&self, issue_ids: &[String]) -> Vec<Result<Vec<StateChange>, String>> {
        parallel_map(issue_ids, |id| self.source.fetch_state_changes(id))
    }

    fn marker_lines(&self, prefix: &str, needle: &str) -> Result<Vec<String>, String> {
        self.source.fetch_marker_lines(prefix, needle)
    }

    fn marker_lines_batch(&self, queries: &[(String, String)]) -> Vec<Result<Vec<String>, String>> {
        parallel_map(queries, |(prefix, needle)| {
            self.source.fetch_marker_lines(prefix, needle)
        })
    }

    fn open_issues_with_label(&self, label: &str) -> Result<Vec<LabeledIssue>, String> {
        self.source.fetch_open_issues_with_label(label)
    }
}

fn run(program: &str, args: &[String]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("{program}: {error}"))
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn epoch(text: &str) -> Result<f64, String> {
    crate::drift::timestamp_epoch(text).map_err(|error| error.to_string())
}

/// GitHub through the authenticated `gh` CLI, GET and GraphQL queries only.
pub struct LiveGithub;

const PULL_REQUEST_QUERY: &str = "query($owner: String!, $name: String!, $after: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(first: 50, states: __STATES__, after: $after,
                 orderBy: {field: UPDATED_AT, direction: DESC}) {
      nodes {
        number createdAt mergedAt updatedAt isDraft state additions deletions reviewDecision
        timelineItems(first: 100, itemTypes: [PULL_REQUEST_COMMIT, HEAD_REF_FORCE_PUSHED_EVENT]) {
          nodes {
            __typename
            ... on PullRequestCommit { commit { oid } }
            ... on HeadRefForcePushedEvent { afterCommit { oid } }
          }
          pageInfo { hasNextPage }
        }
      }
      pageInfo { hasNextPage endCursor }
    }
  }
}";

#[derive(Deserialize)]
struct GraphQlData {
    data: RepositoryData,
}

#[derive(Deserialize)]
struct RepositoryData {
    repository: Option<PullRequestsData>,
}

#[derive(Deserialize)]
struct PullRequestsData {
    #[serde(rename = "pullRequests")]
    pull_requests: PullRequestConnection,
}

#[derive(Deserialize)]
struct PullRequestConnection {
    nodes: Vec<RawPullRequest>,
    #[serde(rename = "pageInfo")]
    page_info: RawPageInfo,
}

#[derive(Deserialize)]
struct RawPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor", default)]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct RawPullRequest {
    number: u64,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    state: String,
    additions: u64,
    deletions: u64,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    #[serde(rename = "timelineItems")]
    timeline: Timeline,
}

#[derive(Deserialize)]
struct Timeline {
    nodes: Vec<TimelineNode>,
    #[serde(rename = "pageInfo")]
    page_info: TimelinePageInfo,
}

#[derive(Deserialize)]
struct TimelinePageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

#[derive(Deserialize)]
struct TimelineNode {
    commit: Option<Oid>,
    #[serde(rename = "afterCommit")]
    after_commit: Option<Oid>,
}

#[derive(Deserialize)]
struct Oid {
    oid: String,
}

impl LiveGithub {
    fn list(
        &self,
        repo: &str,
        states: &str,
        since: Option<f64>,
    ) -> Result<Vec<PullRequest>, String> {
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| format!("repository {repo:?} is not owner/name"))?;
        let query = PULL_REQUEST_QUERY.replace("__STATES__", states);
        let raw = read_all_pages(|after| {
            let mut args = strings(&["api", "graphql", "-f"]);
            args.push(format!("query={query}"));
            args.extend(["-f".to_owned(), format!("owner={owner}")]);
            args.extend(["-f".to_owned(), format!("name={name}")]);
            if let Some(cursor) = after {
                args.extend(["-f".to_owned(), format!("after={cursor}")]);
            }
            let body: GraphQlData =
                serde_json::from_str(&run("gh", &args)?).map_err(|error| error.to_string())?;
            let connection = body
                .data
                .repository
                .ok_or_else(|| format!("repository {repo} not found"))?
                .pull_requests;
            // Newest-updated first: past the cutoff nothing older can matter.
            let reached_cutoff = since.is_some_and(|cutoff| {
                connection
                    .nodes
                    .last()
                    .and_then(|pr| epoch(&pr.updated_at).ok())
                    .is_some_and(|updated| updated < cutoff)
            });
            Ok(Page {
                items: connection.nodes,
                has_next: connection.page_info.has_next_page && !reached_cutoff,
                cursor: connection.page_info.end_cursor,
            })
        })?;
        raw.into_iter()
            .map(|pr| {
                let mut heads = std::collections::BTreeSet::new();
                for node in &pr.timeline.nodes {
                    heads.extend(
                        node.commit
                            .iter()
                            .chain(&node.after_commit)
                            .map(|c| c.oid.clone()),
                    );
                }
                Ok(PullRequest {
                    repo: repo.to_owned(),
                    number: pr.number,
                    open: pr.state == "OPEN",
                    draft: pr.is_draft,
                    created_at: epoch(&pr.created_at)?,
                    merged_at: pr.merged_at.as_deref().map(epoch).transpose()?,
                    updated_at: epoch(&pr.updated_at)?,
                    additions: pr.additions,
                    deletions: pr.deletions,
                    review_decision: pr.review_decision,
                    head_shas: (!pr.timeline.page_info.has_next_page).then_some(heads.len() as u64),
                })
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct RestCommit {
    sha: String,
    commit: RestCommitBody,
}

#[derive(Deserialize)]
struct RestCommitBody {
    committer: RestCommitter,
}

#[derive(Deserialize)]
struct RestCommitter {
    date: String,
}

#[derive(Deserialize)]
struct CheckRuns {
    check_runs: Vec<CheckRun>,
}

#[derive(Deserialize)]
struct CheckRun {
    status: String,
    conclusion: Option<String>,
}

fn check_state(runs: &[CheckRun]) -> CheckState {
    let failed = runs.iter().any(|run| {
        matches!(
            run.conclusion.as_deref(),
            Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
        )
    });
    if failed {
        CheckState::Fail
    } else if runs.is_empty() || runs.iter().any(|run| run.status != "completed") {
        CheckState::Pending
    } else {
        CheckState::Pass
    }
}

impl GithubSource for LiveGithub {
    fn pull_requests(&self, repo: &str, since: f64) -> Result<Vec<PullRequest>, String> {
        let mut all = self.list(repo, "[OPEN]", None)?;
        all.extend(self.list(repo, "[MERGED]", Some(since))?);
        Ok(all)
    }

    fn is_merged(&self, repo: &str, number: u64) -> Result<bool, String> {
        let text = run(
            "gh",
            &strings(&[
                "api",
                "--method",
                "GET",
                &format!("repos/{repo}/pulls/{number}"),
                "--jq",
                ".merged_at != null",
            ]),
        )?;
        Ok(text.trim() == "true")
    }

    fn main_commits(
        &self,
        repo: &str,
        branch: &str,
        since: f64,
    ) -> Result<Vec<MainCommit>, String> {
        let since_text = crate::drift::format_timestamp(since.floor());
        let text = run(
            "gh",
            &strings(&[
                "api",
                "--paginate",
                "--slurp",
                "--method",
                "GET",
                &format!("repos/{repo}/commits"),
                "-f",
                &format!("sha={branch}"),
                "-f",
                &format!("since={since_text}"),
                "-F",
                "per_page=100",
            ]),
        )?;
        let pages: Vec<Vec<RestCommit>> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let mut commits = Vec::new();
        for commit in pages.into_iter().flatten() {
            let text = run(
                "gh",
                &strings(&[
                    "api",
                    "--paginate",
                    "--slurp",
                    "--method",
                    "GET",
                    &format!("repos/{repo}/commits/{}/check-runs", commit.sha),
                    "-F",
                    "per_page=100",
                ]),
            )?;
            let pages: Vec<CheckRuns> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let runs = pages
                .into_iter()
                .flat_map(|page| page.check_runs)
                .collect::<Vec<_>>();
            commits.push(MainCommit {
                committed_at: epoch(&commit.commit.committer.date)?,
                state: check_state(&runs),
            });
        }
        Ok(commits)
    }
}

/// Longest the herdr read may take before the agent metrics are unavailable. An unreachable
/// machine otherwise holds the whole snapshot for the detector's two 60-second calls.
const AGENTS_DEADLINE: std::time::Duration = std::time::Duration::from_secs(25);

type AgentsResult = Result<AgentsReport, String>;

/// The herdr read, started in the background so it overlaps the other sources.
pub struct LiveAgents {
    receiver: Mutex<Option<std::sync::mpsc::Receiver<AgentsResult>>>,
    started: std::time::Instant,
}

impl LiveAgents {
    /// Starts reading the agents of `teams` now; [`AgentSource::report`] collects the result.
    pub fn start(teams: Vec<String>) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        if teams.is_empty() {
            return Self {
                receiver: Mutex::new(None),
                started: std::time::Instant::now(),
            };
        }
        std::thread::spawn(move || {
            let report = system_hosts()
                .map_err(|error| error.to_string())
                .map(|hosts| {
                    let mut no_queue = |_: &str| Ok(None);
                    build_report(&hosts, &teams, 20 * 60, &mut no_queue)
                });
            let _ = sender.send(report);
        });
        Self {
            receiver: Mutex::new(Some(receiver)),
            started: std::time::Instant::now(),
        }
    }
}

impl AgentSource for LiveAgents {
    fn report(&self, _teams: &[String]) -> Result<AgentsReport, String> {
        let receiver = self
            .receiver
            .lock()
            .map_err(|_| "agents lock poisoned".to_owned())?
            .take()
            .ok_or_else(|| "the herdr read was already collected".to_owned())?;
        let left = AGENTS_DEADLINE.saturating_sub(self.started.elapsed());
        receiver.recv_timeout(left).map_err(|_| {
            format!(
                "the herdr read did not finish within {}s",
                AGENTS_DEADLINE.as_secs()
            )
        })?
    }
}

/// The team gate script's JSONL log.
pub struct FileGateLog;

fn field_epoch(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    match value.get(key) {
        Some(serde_json::Value::String(text)) => epoch(text),
        Some(serde_json::Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| format!("{key} is not a number")),
        _ => Err(format!("{key} is missing")),
    }
}

impl GateLogSource for FileGateLog {
    fn lines(&self, path: &Path) -> Result<Vec<GateLine>, String> {
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let mut lines = Vec::new();
        for (index, line) in text
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
        {
            let at = |message: String| format!("line {}: {message}", index + 1);
            let value: serde_json::Value =
                serde_json::from_str(line).map_err(|e| at(e.to_string()))?;
            let pr = match value.get("pr") {
                Some(serde_json::Value::String(text)) => text.clone(),
                Some(serde_json::Value::Number(number)) => number.to_string(),
                _ => return Err(at("pr is missing".to_owned())),
            };
            let repin = match value.get("kind").and_then(serde_json::Value::as_str) {
                Some("gate") => false,
                Some("repin") => true,
                other => return Err(at(format!("kind {other:?} is not gate or repin"))),
            };
            let (requested, acquired) = (
                field_epoch(&value, "requested_lock_at").map_err(&at)?,
                field_epoch(&value, "acquired_lock_at").map_err(&at)?,
            );
            if acquired < requested {
                return Err(at("acquired_lock_at precedes requested_lock_at".to_owned()));
            }
            lines.push(GateLine {
                pr,
                repin,
                requested_lock_at: requested,
                acquired_lock_at: acquired,
                started_at: field_epoch(&value, "started_at").map_err(&at)?,
                finished_at: field_epoch(&value, "finished_at").map_err(&at)?,
            });
        }
        Ok(lines)
    }
}

/// Where the relay bus lives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelaySettings {
    /// Redis URL of the bus.
    pub url: String,
    /// Key prefix.
    pub prefix: String,
}

/// Resolves the bus URL and prefix: the environment beats `~/.ix/relay/config.toml`, which beats
/// the relay's own defaults.
pub fn relay_settings(
    env_url: Option<String>,
    env_prefix: Option<String>,
    config_text: Option<&str>,
) -> RelaySettings {
    let file: Option<toml::Table> = config_text.and_then(|text| text.parse().ok());
    let from_file = |key: &str| {
        file.as_ref()
            .and_then(|table| table.get(key))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    RelaySettings {
        url: env_url
            .or_else(|| from_file("url"))
            .unwrap_or_else(|| "redis://127.0.0.1:6379".to_owned()),
        prefix: env_prefix
            .or_else(|| from_file("prefix"))
            .unwrap_or_else(|| "ixr:v3:".to_owned()),
    }
}

/// The parts of a `redis://[user[:password]@]host[:port][/db]` URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisTarget {
    /// Host name.
    pub host: String,
    /// Port.
    pub port: u16,
    /// ACL user, when the URL names one.
    pub user: Option<String>,
    /// Password, when the URL carries one.
    pub password: Option<String>,
}

/// Splits a Redis URL so the password can be passed in the environment, not on the command line.
pub fn parse_redis_url(url: &str) -> Result<RedisTarget, String> {
    let rest = url
        .strip_prefix("redis://")
        .ok_or_else(|| "relay URL must start with redis://".to_owned())?;
    let rest = rest.split('/').next().unwrap_or(rest);
    let (credentials, host_port) = rest
        .rsplit_once('@')
        .map_or((None, rest), |(c, h)| (Some(c), h));
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse()
                .map_err(|_| "relay URL port is not a number".to_owned())?,
        ),
        None => (host_port, 6379),
    };
    let (user, password) = match credentials.map(|c| c.split_once(':')) {
        Some(Some((user, password))) => (
            (!user.is_empty()).then(|| user.to_owned()),
            Some(password.to_owned()),
        ),
        Some(None) => (None, credentials.map(str::to_owned)),
        None => (None, None),
    };
    Ok(RedisTarget {
        host: host.to_owned(),
        port,
        user,
        password,
    })
}

/// The inbox names an address reads from. A lead address is `server:workspace:tab` and matches
/// registrations by machine, workspace and tab; the bus queues are keyed by the registered
/// session name, not by the address. Text without a colon is taken as the queue name itself.
pub fn queue_names_for_address(
    address: &str,
    registrations: &[(String, serde_json::Value)],
) -> Vec<String> {
    let parts = address.split(':').collect::<Vec<_>>();
    let [server, workspace, tab] = parts.as_slice() else {
        return vec![address.to_owned()];
    };
    let matches = |value: &serde_json::Value, key: &str, wanted: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|got| got.eq_ignore_ascii_case(wanted))
    };
    registrations
        .iter()
        .filter(|(_, registration)| {
            let place = registration
                .get("place")
                .unwrap_or(&serde_json::Value::Null);
            matches(registration, "machine", server)
                && matches(place, "workspace", workspace)
                && matches(place, "tab", tab)
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// The ix-relay inbox lists, read with `HGETALL` on the registry and `LRANGE` on the inbox
/// lists, so nothing is dequeued or written.
///
/// The bus URL and prefix come from `IX_RELAY_URL` and `IX_RELAY_PREFIX`, then
/// `~/.ix/relay/config.toml`, then the relay defaults. The password goes to `redis-cli` in
/// `REDISCLI_AUTH`, never on the command line.
pub struct RedisCliRelay;

#[derive(Deserialize)]
struct RelayMessage {
    sent_at_ms: i64,
}

fn redis(target: &RedisTarget, args: &[String]) -> Result<String, String> {
    let mut command = Command::new("redis-cli");
    command.args([
        "-h",
        &target.host,
        "-p",
        &target.port.to_string(),
        "--no-auth-warning",
        "--raw",
    ]);
    if let Some(user) = &target.user {
        command.args(["--user", user]);
    }
    if let Some(password) = &target.password {
        command.env("REDISCLI_AUTH", password);
    }
    let output = command
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("redis-cli: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "redis-cli failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    // redis-cli reports server errors on stdout with exit status zero.
    if text.starts_with("ERR ") || text.starts_with("NOAUTH") || text.starts_with("WRONGPASS") {
        return Err(format!("redis: {}", text.trim()));
    }
    Ok(text)
}

impl RelaySource for RedisCliRelay {
    fn pending_sent_at_ms(&self, address: &str) -> Result<Vec<i64>, String> {
        let config = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".ix/relay/config.toml"))
            .and_then(|path| std::fs::read_to_string(path).ok());
        let settings = relay_settings(
            std::env::var("IX_RELAY_URL").ok(),
            std::env::var("IX_RELAY_PREFIX").ok(),
            config.as_deref(),
        );
        let target = parse_redis_url(&settings.url)?;
        let names = if address.contains(':') {
            let flat = redis(
                &target,
                &strings(&["HGETALL", &format!("{}agents", settings.prefix)]),
            )?;
            let mut rows = flat.lines();
            let mut registrations = Vec::new();
            while let (Some(name), Some(json)) = (rows.next(), rows.next()) {
                if let Ok(value) = serde_json::from_str(json) {
                    registrations.push((name.to_owned(), value));
                }
            }
            let names = queue_names_for_address(address, &registrations);
            if names.is_empty() {
                return Err(format!("no registered lead matches {address}"));
            }
            names
        } else {
            vec![address.to_owned()]
        };
        let mut sent = Vec::new();
        for name in names {
            let text = redis(
                &target,
                &strings(&[
                    "LRANGE",
                    &format!("{}inbox:{name}", settings.prefix),
                    "0",
                    "-1",
                ]),
            )?;
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                sent.push(
                    serde_json::from_str::<RelayMessage>(line)
                        .map(|message| message.sent_at_ms)
                        .map_err(|error| format!("queue entry does not parse: {error}"))?,
                );
            }
        }
        Ok(sent)
    }
}

/// Local git, disk and process reads.
pub struct SystemLocal;

fn git_roots(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if dir.join(".git").exists() {
        return Ok(vec![dir.to_owned()]);
    }
    let mut roots = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.join(".git").exists() {
            roots.push(path);
        }
    }
    roots.sort();
    Ok(roots)
}

fn directory_bytes(dir: &Path) -> Result<u64, String> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            total += directory_bytes(&entry.path())?;
        } else if kind.is_file() {
            total += entry.metadata().map_err(|error| error.to_string())?.len();
        }
    }
    Ok(total)
}

/// Parses `ps` elapsed time: `[[dd-]hh:]mm:ss`.
fn parse_elapsed(text: &str) -> Option<u64> {
    let (days, rest) = text
        .split_once('-')
        .map_or((0, text), |(d, r)| (d.parse().unwrap_or(0), r));
    let parts = rest
        .split(':')
        .map(|part| part.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let seconds = match parts.as_slice() {
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(days * 86_400 + seconds)
}

impl LocalSource for SystemLocal {
    fn prunable_worktrees(&self, dir: &Path) -> Result<u64, String> {
        let mut prunable = 0;
        for root in git_roots(dir)? {
            let text = run(
                "git",
                &[
                    "-C".to_owned(),
                    root.display().to_string(),
                    "worktree".to_owned(),
                    "list".to_owned(),
                    "--porcelain".to_owned(),
                ],
            )?;
            prunable += text
                .lines()
                .filter(|line| line.starts_with("prunable"))
                .count() as u64;
        }
        Ok(prunable)
    }

    fn disk_bytes(&self, dir: &Path) -> Result<u64, String> {
        directory_bytes(dir)
    }

    fn processes(&self) -> Result<Vec<ProcessRow>, String> {
        let text = run("ps", &strings(&["-axo", "pid=,ppid=,etime=,command="]))?;
        let rows = text
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let pid: u64 = fields.next()?.parse().ok()?;
                let ppid: u64 = fields.next()?.parse().ok()?;
                let age = parse_elapsed(fields.next()?)?;
                let command = fields.collect::<Vec<_>>().join(" ");
                Some((pid, ppid, age, command))
            })
            .collect::<Vec<_>>();
        let alive = rows
            .iter()
            .map(|row| row.0)
            .collect::<std::collections::BTreeSet<_>>();
        Ok(rows
            .into_iter()
            .map(|(_, ppid, age_secs, command)| ProcessRow {
                command,
                parent_alive: ppid > 1 && alive.contains(&ppid),
                age_secs,
            })
            .collect())
    }
}

/// A Linear source that could not be built; every read reports why.
struct UnreadableLinear(String);

impl LinearSource for UnreadableLinear {
    fn workspace(&self) -> Result<WorkspaceSnapshot, String> {
        Err(self.0.clone())
    }

    fn project_open_count(&self, _project: &str) -> Result<usize, String> {
        Err(self.0.clone())
    }

    fn state_history(&self, _issue_id: &str) -> Result<Vec<StateChange>, String> {
        Err(self.0.clone())
    }

    fn marker_lines(&self, _prefix: &str, _needle: &str) -> Result<Vec<String>, String> {
        Err(self.0.clone())
    }

    fn open_issues_with_label(&self, _label: &str) -> Result<Vec<LabeledIssue>, String> {
        Err(self.0.clone())
    }
}

/// The live Linear reader, or one that reports why it could not be built, so a missing
/// credential makes Linear metrics unavailable instead of stopping the record.
pub fn linear_or_unreadable() -> Box<dyn LinearSource> {
    match LiveLinear::from_env() {
        Ok(linear) => Box::new(linear),
        Err(error) => Box::new(UnreadableLinear(error.to_string())),
    }
}
