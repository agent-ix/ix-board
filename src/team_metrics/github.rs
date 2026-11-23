// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Exporter-owned GitHub admission, persisted response cache and bounded queries.
use super::Config;
use crate::retro::sources::{GithubSource, MainCommit, PullRequest, PullRequestInfo};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub(super) const TTL_MS: i64 = 900_000;
const HOUR_MS: i64 = 3_600_000;
const MAX_BODY: usize = 1024 * 1024;
const MAX_REQUESTS: i64 = 40;
const RESERVED_COST: i64 = 25;
const MAX_COST: i64 = 1000;
const MAX_PAGES: usize = 4;
const MAX_OLD: usize = 50;
const MAX_REPOS: usize = 32;
const MAX_QUERY: usize = 16 * 1024;
const CACHE_BYTES: i64 = 4 * 1024 * 1024;

pub(super) struct Request {
    args: Vec<String>,
    conditional: bool,
}
pub(super) struct Response {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}
pub(super) trait Transport {
    fn send(&self, request: &Request, etag: Option<&str>) -> Result<Response, String>;
}
pub(super) struct Gh;
impl Transport for Gh {
    fn send(&self, request: &Request, etag: Option<&str>) -> Result<Response, String> {
        #[cfg(unix)]
        {
            let mut command = std::process::Command::new("gh");
            command.args(["api", "--hostname", "github.com", "--include"]);
            command.args(&request.args);
            if let Some(etag) = etag {
                command.args(["-H", &format!("If-None-Match: {etag}")]);
            }
            let output = super::history::command_output(
                &mut command,
                Duration::from_secs(20),
                MAX_BODY + 32 * 1024,
            )
            .map_err(|e| e.to_string())?;
            // gh exits nonzero for HTTP errors; parse their headers before returning failure.
            let response = parse_http(&output.bytes)?;
            if !output.success && response.status < 400 && response.status != 304 {
                return Err("GitHub command failed without an HTTP error".into());
            }
            Ok(response)
        }
        #[cfg(not(unix))]
        {
            let _ = (request, etag);
            Err("GitHub exporter transport requires Unix/WSL".into())
        }
    }
}
fn parse_http(bytes: &[u8]) -> Result<Response, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "GitHub HTTP response is not UTF-8")?;
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .ok_or("GitHub HTTP headers missing")?;
    if head.len() > 32 * 1024 || body.len() > MAX_BODY {
        return Err("GitHub response byte limit exceeded".into());
    }
    let mut lines = head.lines();
    let status = lines
        .next()
        .filter(|s| s.starts_with("HTTP/"))
        .and_then(|s| s.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|s| (100..=599).contains(s))
        .ok_or("GitHub HTTP status missing")?;
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("invalid GitHub HTTP header")?;
        let name = name.to_ascii_lowercase();
        if headers.insert(name, value.trim().to_owned()).is_some() {
            return Err("duplicate GitHub HTTP header".into());
        }
    }
    Ok(Response {
        status,
        headers,
        body: body.into(),
    })
}
struct Cached {
    body: String,
    at: i64,
}
struct Guard<T> {
    db: RefCell<Connection>,
    transport: T,
    cache_bytes: i64,
}
impl<T: Transport> Guard<T> {
    fn open(config: &Config, transport: T) -> Result<Self, String> {
        // Use the journal itself: two workers and restarted processes share admission,
        // response leases and the configured SQLite page ceiling, with no new quota file.
        let db = Connection::open(&config.state_path).map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_millis(250))
            .map_err(|e| e.to_string())?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 1 {
            return Err("unsupported GitHub shared journal schema".into());
        }
        let page_size: u64 = db
            .pragma_query_value(None, "page_size", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let pages: u64 = db
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if pages.saturating_mul(page_size) > config.max_bytes {
            return Err("GitHub shared journal exceeds storage ceiling".into());
        }
        db.pragma_update(None, "max_page_count", config.max_bytes / page_size)
            .map_err(|e| e.to_string())?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS github_gate(id INTEGER PRIMARY KEY CHECK(id=1), cooldown INTEGER NOT NULL, lease INTEGER NOT NULL); INSERT OR IGNORE INTO github_gate VALUES(1,0,0); CREATE TABLE IF NOT EXISTS github_sends(at INTEGER NOT NULL, cost INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS github_responses(key TEXT PRIMARY KEY, body TEXT NOT NULL, etag TEXT, at INTEGER NOT NULL);").map_err(|e| e.to_string())?;
        Ok(Self {
            db: RefCell::new(db),
            transport,
            cache_bytes: i64::try_from(config.max_bytes / 8)
                .map_err(|_| "cache budget overflow")?
                .min(CACHE_BYTES),
        })
    }
    fn request(
        &self,
        request: &Request,
        now: i64,
        validate: impl Fn(&str) -> Result<(), String>,
    ) -> Result<Cached, String> {
        if request.args.iter().map(String::len).sum::<usize>() > MAX_QUERY {
            return Err("GitHub query byte limit exceeded".into());
        }
        let key = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&request.args).map_err(|e| e.to_string())?)
        );
        let mut db = self.db.borrow_mut();
        // Retention also advances during cooldowns and cache-only cycles.
        db.execute(
            "DELETE FROM github_responses WHERE at < ?1",
            [now.saturating_sub(7 * 86_400_000)],
        )
        .map_err(|e| e.to_string())?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let (cooldown, lease): (i64, i64) = tx
            .query_row(
                "SELECT cooldown,lease FROM github_gate WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        // Cooldown overrides even a recent response: rate-limited sources are unavailable.
        if now < cooldown {
            return Err("GitHub persisted cooldown active".into());
        }
        let old: Option<(String, Option<String>, i64)> = tx
            .query_row(
                "SELECT body,etag,at FROM github_responses WHERE key=?1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((body, _, at)) = &old
            && *at <= now
            && now.saturating_sub(*at) < TTL_MS
        {
            validate(body)?;
            return Ok(Cached {
                body: body.clone(),
                at: *at,
            });
        }
        if lease > now {
            return Err("GitHub shared request already in flight".into());
        }
        tx.execute(
            "DELETE FROM github_sends WHERE at <= ?1",
            [now.saturating_sub(HOUR_MS)],
        )
        .map_err(|e| e.to_string())?;
        let (count, cost): (i64, i64) = tx
            .query_row(
                "SELECT count(*),coalesce(sum(cost),0) FROM github_sends",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if count >= MAX_REQUESTS || cost + RESERVED_COST > MAX_COST {
            return Err("GitHub hard rolling-hour request/cost budget exhausted".into());
        }
        // Commit the pessimistic debit BEFORE sending; a kill cannot refund account load.
        tx.execute(
            "INSERT INTO github_sends VALUES(?1,?2)",
            params![now, RESERVED_COST],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE github_gate SET lease=?1 WHERE id=1",
            [now.saturating_add(90_000)],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        let etag = old
            .as_ref()
            .and_then(|(_, etag, _)| etag.as_deref())
            .filter(|_| request.conditional);
        let response = self.transport.send(request, etag);
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute("UPDATE github_gate SET lease=0 WHERE id=1", [])
            .map_err(|e| e.to_string())?;
        let result = match response {
            Ok(response) => {
                let graph_limit = serde_json::from_str::<GraphResponse>(&response.body)
                    .ok()
                    .filter(|r| r.data.rate.remaining < 25);
                let limited = response.status == 429
                    || response.status == 403
                    || response
                        .headers
                        .get("x-ratelimit-remaining")
                        .is_some_and(|v| v == "0")
                    || graph_limit.is_some();
                if limited {
                    let graph_reset = graph_limit.and_then(|r| chrono::DateTime::parse_from_rfc3339(&r.data.rate.reset_at).ok()).map(|d| d.timestamp_millis()).unwrap_or(0);
                    let until = backoff(&response, now).max(graph_reset);
                    tx.execute("UPDATE github_gate SET cooldown=max(cooldown,?1) WHERE id=1", [until]).map_err(|e| e.to_string())?;
                    Err("GitHub server rate limit; cooldown persisted".into())
                } else if response.status == 304 && request.conditional {
                    match &old {
                        Some((body, _, _)) => validate(body).map(|()| Cached { body: body.clone(), at: now }),
                        None => Err("GitHub 304 without cached representation".into()),
                    }
                } else if response.status == 200 && response.body.len() <= MAX_BODY {
                    validate(&response.body).map(|()| Cached { body: response.body.clone(), at: now })
                } else {
                    Err("GitHub HTTP response unavailable".into())
                }.and_then(|cached| {
                    if i64::try_from(cached.body.len()).map_err(|_| "cache size overflow")? > self.cache_bytes {
                        return Err("GitHub representation exceeds shared cache budget".into());
                    }
                    // Bound response storage independently of the journal's hard file ceiling.
                    tx.execute("DELETE FROM github_responses WHERE at < ?1", [now.saturating_sub(7 * 86_400_000)]).map_err(|e| e.to_string())?;
                    tx.execute("DELETE FROM github_responses WHERE key=?1", [&key]).map_err(|e| e.to_string())?;
                    loop {
                        let (bytes, count): (i64, i64) = tx.query_row("SELECT coalesce(sum(length(CAST(body AS BLOB))),0),count(*) FROM github_responses", [], |r| Ok((r.get(0)?,r.get(1)?))).map_err(|e| e.to_string())?;
                        if bytes + i64::try_from(cached.body.len()).map_err(|_| "cache size overflow")? <= self.cache_bytes && count < 128 { break; }
                        tx.execute("DELETE FROM github_responses WHERE key IN (SELECT key FROM github_responses ORDER BY at,key LIMIT 1)", []).map_err(|e| e.to_string())?;
                    }
                    let etag = response.headers.get("etag").map(String::as_str).or(etag).filter(|v| v.len() <= 1024 && !v.contains(['\r','\n']));
                    tx.execute("INSERT INTO github_responses VALUES(?1,?2,?3,?4)", params![key,cached.body,etag,cached.at]).map_err(|e| e.to_string())?;
                    Ok(cached)
                })
            }
            Err(reason) => Err(reason),
        };
        if result.is_err() {
            // Malformed payloads, transport timeouts and GraphQL errors also suppress
            // retry storms. A longer server cooldown always wins.
            tx.execute(
                "UPDATE github_gate SET cooldown=max(cooldown,?1) WHERE id=1",
                [now.saturating_add(HOUR_MS)],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        result
    }
}
fn backoff(response: &Response, now: i64) -> i64 {
    let reset = response
        .headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.parse::<i64>().ok())
        .and_then(|s| s.checked_mul(1000))
        .unwrap_or(0);
    let retry = response
        .headers
        .get("retry-after")
        .and_then(|v| {
            v.parse::<i64>()
                .ok()
                .filter(|s| *s >= 0)
                .map(|s| now.saturating_add(s.saturating_mul(1000)))
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(v)
                        .ok()
                        .map(|d| d.timestamp_millis())
                })
        })
        .unwrap_or(0);
    reset.max(retry).max(now.saturating_add(HOUR_MS))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphResponse {
    data: GraphData,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphData {
    #[serde(rename = "rateLimit")]
    rate: Rate,
    repository: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rate {
    cost: u64,
    remaining: u64,
    #[serde(rename = "resetAt")]
    reset_at: String,
}
fn graph_body(body: &str) -> Result<String, String> {
    let response: GraphResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    if response.data.rate.cost > u64::try_from(RESERVED_COST).map_err(|_| "cost overflow")?
        || response.data.rate.remaining == 0
        || epoch(&response.data.rate.reset_at).is_err()
    {
        return Err("GitHub GraphQL cost/quota evidence unavailable".into());
    }
    Ok(serde_json::json!({"data":{"repository":response.data.repository}}).to_string())
}
fn graph_request(query: &str, repo: &str, after: Option<&str>) -> Result<Request, String> {
    let mut pieces = repo.split('/');
    let (Some(owner), Some(name), None) = (pieces.next(), pieces.next(), pieces.next()) else {
        return Err("invalid repository".into());
    };
    if [owner, name].iter().any(|s| {
        s.is_empty()
            || s.len() > 100
            || *s == "."
            || *s == ".."
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    }) {
        return Err("invalid repository".into());
    }
    let mut args = vec![
        "--method".into(),
        "POST".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={query}"),
        "-f".into(),
        format!("owner={owner}"),
        "-f".into(),
        format!("name={name}"),
    ];
    if let Some(after) = after {
        if after.len() > 1024 {
            return Err("GitHub cursor byte limit exceeded".into());
        }
        args.extend(["-f".into(), format!("after={after}")]);
    }
    // GraphQL POST has no conditional GET representation. REST requests, when
    // admitted by this transport, use conditional=true and retain ETags on 304.
    Ok(Request {
        args,
        conditional: false,
    })
}

type PrRows = Result<Vec<PullRequest>, String>;
type InfoRows = Result<BTreeMap<u64, PullRequestInfo>, String>;

pub(super) struct SelectedGithub<T = Gh> {
    guard: Result<Guard<T>, String>,
    allowed: BTreeSet<(String, u64)>,
    complete: bool,
    now: i64,
    oldest_at: Cell<i64>,
    start: Instant,
    lists: RefCell<BTreeMap<String, PrRows>>,
    infos: RefCell<BTreeMap<String, InfoRows>>,
}
impl SelectedGithub {
    pub(super) fn new(
        config: &Config,
        allowed: BTreeSet<(String, u64)>,
        complete: bool,
        now: i64,
    ) -> Self {
        Self::with_guard(Guard::open(config, Gh), allowed, complete, now)
    }
}
impl<T: Transport> SelectedGithub<T> {
    fn with_guard(
        guard: Result<Guard<T>, String>,
        allowed: BTreeSet<(String, u64)>,
        complete: bool,
        now: i64,
    ) -> Self {
        Self {
            guard,
            allowed,
            complete,
            now,
            oldest_at: Cell::new(0),
            start: Instant::now(),
            lists: RefCell::new(BTreeMap::new()),
            infos: RefCell::new(BTreeMap::new()),
        }
    }
    pub(super) fn observed_at(&self) -> i64 {
        self.oldest_at.get()
    }
    fn read(
        &self,
        request: &Request,
        validate: impl Fn(&str) -> Result<(), String>,
    ) -> Result<String, String> {
        if self.start.elapsed() >= Duration::from_secs(45) {
            return Err("GitHub collection time budget exhausted".into());
        }
        if self.allowed.len() > 10_000 {
            return Err("GitHub linked population exceeds bound".into());
        }
        let cached = self
            .guard
            .as_ref()
            .map_err(Clone::clone)?
            .request(request, self.now, validate)?;
        self.oldest_at.set(if self.oldest_at.get() == 0 {
            cached.at
        } else {
            self.oldest_at.get().min(cached.at)
        });
        Ok(cached.body)
    }
    fn list(&self, repo: &str, states: &str, since: Option<f64>) -> PrRows {
        let query = crate::retro::live::PULL_REQUEST_QUERY
            .replace("__STATES__", states)
            .replacen(
                "  repository(",
                "  rateLimit { cost remaining resetAt }\n  repository(",
                1,
            );
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut prs = Vec::new();
        let mut previous_update = f64::INFINITY;
        for _ in 0..MAX_PAGES {
            let request = graph_request(&query, repo, cursor.as_deref())?;
            let body = self.read(&request, |body| validate_page(body, repo, since, states))?;
            let page = crate::retro::live::pull_request_page(&graph_body(&body)?, repo, since)?;
            if page.items.len() > 50 || page.items.iter().any(|p| !seen.insert(p.number)) {
                return Err("GitHub duplicate/oversize PR page".into());
            }
            for pr in &page.items {
                if pr.updated_at > previous_update {
                    return Err("GitHub pagination order is inconsistent".into());
                }
                previous_update = pr.updated_at;
            }
            prs.extend(
                page.items
                    .into_iter()
                    .filter(|p| since.is_none_or(|s| p.updated_at >= s)),
            );
            if !page.has_next {
                return Ok(prs);
            }
            let next = page.cursor.ok_or("GitHub next cursor absent")?;
            if cursor.as_ref() == Some(&next) {
                return Err("GitHub pagination did not advance".into());
            }
            cursor = Some(next);
        }
        Err("GitHub repository page budget exhausted; incomplete population".into())
    }
    fn repository_prs(&self, repo: &str) -> PrRows {
        if let Some(rows) = self.lists.borrow().get(repo) {
            return rows.clone();
        }
        if self.lists.borrow().len() >= MAX_REPOS {
            return Err("GitHub repository population exceeds collection bound".into());
        }
        let result = (|| {
            let mut rows = self.list(repo, "[OPEN]", None)?;
            rows.extend(self.list(
                repo,
                "[MERGED, CLOSED]",
                Some(self.now as f64 / 1000.0 - 86_400.0),
            )?);
            Ok(rows)
        })();
        self.lists.borrow_mut().insert(repo.into(), result.clone());
        result
    }
    fn repository_infos(&self, repo: &str) -> InfoRows {
        if let Some(rows) = self.infos.borrow().get(repo) {
            return rows.clone();
        }
        if self.infos.borrow().len() >= MAX_REPOS {
            return Err("GitHub linked repository population exceeds collection bound".into());
        }
        let result = (|| {
            let mut rows = BTreeMap::new();
            // A failed full population never certifies an empty listing. Linked detail
            // can still be resolved independently through its bounded batch query.
            if let Ok(prs) = self.repository_prs(repo) {
                for pr in prs {
                    rows.insert(
                        pr.number,
                        PullRequestInfo {
                            merged: pr.merged_at.is_some(),
                            updated_at: pr.updated_at,
                        },
                    );
                }
            }
            let numbers: Vec<_> = self
                .allowed
                .iter()
                .filter(|(r, n)| r == repo && !rows.contains_key(n))
                .take(MAX_OLD)
                .map(|(_, n)| *n)
                .collect();
            for chunk in numbers.chunks(25) {
                let fields = chunk
                    .iter()
                    .map(|n| format!("p{n}: pullRequest(number: {n}) {{ mergedAt updatedAt }}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let query = format!(
                    "query($owner:String!,$name:String!) {{ rateLimit {{ cost remaining resetAt }} repository(owner:$owner,name:$name) {{ {fields} }} }}"
                );
                let request = graph_request(&query, repo, None)?;
                let body = self.read(&request, |body| parse_infos(body, chunk).map(|_| ()))?;
                rows.extend(parse_infos(&body, chunk)?);
            }
            Ok(rows)
        })();
        self.infos.borrow_mut().insert(repo.into(), result.clone());
        result
    }
}
fn validate_page(body: &str, repo: &str, since: Option<f64>, states: &str) -> Result<(), String> {
    let response: GraphResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let nodes = response
        .data
        .repository
        .get("pullRequests")
        .and_then(|r| r.get("nodes"))
        .and_then(serde_json::Value::as_array)
        .ok_or("GitHub PR nodes unavailable")?;
    if nodes.len() > 50
        || nodes.iter().any(
            |n| match n.get("state").and_then(serde_json::Value::as_str) {
                Some("OPEN") => states != "[OPEN]",
                Some("CLOSED" | "MERGED") => states != "[MERGED, CLOSED]",
                _ => true,
            },
        )
    {
        return Err("GitHub PR page has unknown/incorrect states or too many nodes".into());
    }
    let page = crate::retro::live::pull_request_page(&graph_body(body)?, repo, since)?;
    if page
        .items
        .windows(2)
        .any(|pair| pair[0].updated_at < pair[1].updated_at)
    {
        return Err("GitHub PR page is not ordered by native update time".into());
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInfo {
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}
fn epoch(text: &str) -> Result<f64, String> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|t| t.timestamp_millis() as f64 / 1000.0)
        .map_err(|e| e.to_string())
}
fn parse_infos(body: &str, numbers: &[u64]) -> Result<BTreeMap<u64, PullRequestInfo>, String> {
    graph_body(body)?;
    let response: GraphResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut data: BTreeMap<String, Option<RawInfo>> =
        serde_json::from_value(response.data.repository).map_err(|e| e.to_string())?;
    let mut rows = BTreeMap::new();
    for n in numbers {
        let info = data
            .remove(&format!("p{n}"))
            .ok_or("GitHub linked PR batch field missing")?;
        if let Some(info) = info {
            if let Some(at) = &info.merged_at {
                epoch(at)?;
            }
            rows.insert(
                *n,
                PullRequestInfo {
                    merged: info.merged_at.is_some(),
                    updated_at: epoch(&info.updated_at)?,
                },
            );
        }
    }
    if !data.is_empty() {
        return Err("GitHub linked PR batch has unexpected fields".into());
    }
    Ok(rows)
}
impl<T: Transport> GithubSource for SelectedGithub<T> {
    fn pull_requests(&self, repo: &str, since: f64) -> PrRows {
        if !self.complete {
            return Err("selected issue PR attachments incomplete".into());
        }
        self.repository_prs(repo).map(|rows| {
            rows.into_iter()
                .filter(|p| {
                    (p.open || p.updated_at >= since)
                        && self.allowed.contains(&(p.repo.clone(), p.number))
                })
                .collect()
        })
    }
    fn pull_request(&self, repo: &str, number: u64) -> Result<PullRequestInfo, String> {
        if !self.complete || !self.allowed.contains(&(repo.into(), number)) {
            return Err("linked PR identity/attachment evidence unavailable".into());
        }
        self.repository_infos(repo)?
            .get(&number)
            .copied()
            .ok_or_else(|| {
                "linked PR outside bounded available evidence; no per-PR fallback".into()
            })
    }
    fn main_commits(&self, _: &str, _: &str, _: f64) -> Result<Vec<MainCommit>, String> {
        Err("exporter commit/check collection unavailable within GitHub budget".into())
    }
    fn department_evidence(
        &self,
        _: &crate::retro::department::Config,
        _: f64,
    ) -> crate::retro::department::Evidence {
        crate::retro::department::Evidence {
            source: "github".into(),
            reasons: ["pull_request", "review", "check", "pin"]
                .into_iter()
                .map(|kind| {
                    (
                        kind.into(),
                        "exporter enrichment unavailable within GitHub budget".into(),
                    )
                })
                .collect(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    type Handler = dyn Fn(&Request, Option<&str>) -> Result<Response, String> + Send + Sync;
    #[derive(Clone)]
    struct Fake {
        count: Arc<AtomicUsize>,
        handler: Arc<Handler>,
    }
    impl Fake {
        fn new(
            handler: impl Fn(&Request, Option<&str>) -> Result<Response, String> + Send + Sync + 'static,
        ) -> Self {
            Self {
                count: Arc::new(AtomicUsize::new(0)),
                handler: Arc::new(handler),
            }
        }
        fn count(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }
    }
    impl Transport for Fake {
        fn send(&self, request: &Request, etag: Option<&str>) -> Result<Response, String> {
            self.count.fetch_add(1, Ordering::SeqCst);
            (self.handler)(request, etag)
        }
    }
    fn config(path: &std::path::Path) -> Config {
        serde_json::from_value(serde_json::json!({
            "version":1,"listen":"127.0.0.1:0","state_path":path.join("state.sqlite"),
            "scopes":[]
        }))
        .unwrap()
    }
    fn response(body: String) -> Response {
        Response {
            status: 200,
            headers: BTreeMap::new(),
            body,
        }
    }
    fn graph() -> Fake {
        Fake::new(|request, _| {
            assert!(!request.conditional);
            assert!(request.args.iter().any(|a| a == "graphql"));
            assert!(
                !request.args.iter().any(|a| a.starts_with("repos/")),
                "no per-PR REST request permitted"
            );
            let query = request
                .args
                .iter()
                .find_map(|a| a.strip_prefix("query="))
                .unwrap();
            assert!(query.len() < MAX_QUERY);
            let repository = if query.contains("pullRequests(") {
                serde_json::json!({"pullRequests":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}})
            } else {
                let mut fields = serde_json::Map::new();
                for number in query.split("pullRequest(number: ").skip(1) {
                    let n: u64 = number.split(')').next().unwrap().parse().unwrap();
                    fields.insert(format!("p{n}"),serde_json::json!({"mergedAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}));
                }
                assert!(fields.len() <= 25);
                serde_json::Value::Object(fields)
            };
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":3,"remaining":4000,"resetAt":"2030-01-01T00:00:00Z"},"repository":repository}}).to_string()))
        })
    }
    fn request(key: usize, conditional: bool) -> Request {
        Request {
            args: vec![format!("fixture-{key}")],
            conditional,
        }
    }
    fn valid_json(body: &str) -> Result<(), String> {
        serde_json::from_str::<serde_json::Value>(body)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Trace: FR-033-AC-12, FR-033-AC-13
    #[test]
    fn thousands_of_old_links_have_constant_batched_cost_across_scopes_cycles_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let transport = graph();
        let allowed: BTreeSet<_> = ["agent-ix/one", "agent-ix/two"]
            .into_iter()
            .flat_map(|r| (1..=3000).map(move |n| (r.to_owned(), n)))
            .collect();
        let at = 1_000_000;
        for offset in [0, 1000, 2000, TTL_MS] {
            let source = SelectedGithub::with_guard(
                Guard::open(&cfg, transport.clone()),
                allowed.clone(),
                true,
                at + offset,
            );
            for repo in ["agent-ix/one", "agent-ix/two"] {
                assert!(source.pull_requests(repo, 0.0).unwrap().is_empty());
                for n in 1..=3000 {
                    let info = source.pull_request(repo, n);
                    if n <= 50 {
                        assert!(info.unwrap().merged);
                    } else {
                        assert!(info.is_err(), "missing old PR must stay unavailable");
                    }
                }
            }
            assert_eq!(
                source.observed_at(),
                if offset < TTL_MS { at } else { at + TTL_MS }
            );
            assert_eq!(
                transport.count(),
                if offset < TTL_MS { 8 } else { 16 },
                "cold=8, warm/restart=0, expired unchanged=8 sends for two repositories"
            );
        }
    }

    /// Trace: FR-033-AC-12
    #[test]
    fn hard_budget_survives_restart_and_no_probe_or_send_is_admitted() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| Ok(response("{}".into())));
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        for n in 0..40 {
            guard.request(&request(n, false), 1000, valid_json).unwrap();
        }
        drop(guard);
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request(&request(41, false), 1001, valid_json)
                .is_err()
        );
        assert_eq!(fake.count(), 40);
        let cost: i64 = guard
            .db
            .borrow()
            .query_row("SELECT sum(cost) FROM github_sends", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cost, 1000);
    }

    /// Trace: FR-033-AC-12
    #[test]
    fn concurrent_workers_share_persisted_lease_and_completed_representation() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let fake = Fake::new(move |_, _| {
            entered_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            Ok(response("{}".into()))
        });
        let other = Guard::open(&cfg, fake.clone()).unwrap();
        let cfg2 = config(dir.path());
        let fake2 = fake.clone();
        let worker = std::thread::spawn(move || {
            Guard::open(&cfg2, fake2)
                .unwrap()
                .request(&request(1, false), 1000, valid_json)
                .map(|c| c.at)
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(other.request(&request(1, false), 1000, valid_json).is_err());
        assert!(other.request(&request(2, false), 1000, valid_json).is_err());
        assert_eq!(fake.count(), 1);
        release_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), 1000);
        assert_eq!(
            other
                .request(&request(1, false), 1001, valid_json)
                .unwrap()
                .at,
            1000
        );
        assert_eq!(fake.count(), 1);
    }

    /// Trace: FR-033-AC-13
    #[test]
    fn conditional_304_revalidates_but_warm_cache_does_not_renew_time() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, etag| match etag {
            None => Ok(Response {
                status: 200,
                headers: [("etag".into(), "\"version1\"".into())].into(),
                body: "{}".into(),
            }),
            Some("\"version1\"") => Ok(Response {
                status: 304,
                headers: BTreeMap::new(),
                body: String::new(),
            }),
            _ => panic!("conditional validator changed"),
        });
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert_eq!(
            guard
                .request(&request(1, true), 1000, valid_json)
                .unwrap()
                .at,
            1000
        );
        assert_eq!(
            guard
                .request(&request(1, true), 1001, valid_json)
                .unwrap()
                .at,
            1000
        );
        assert_eq!(
            guard
                .request(&request(1, true), 1000 + TTL_MS, valid_json)
                .unwrap()
                .at,
            1000 + TTL_MS
        );
        drop(guard);
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert_eq!(
            guard
                .request(&request(1, true), 1000 + 2 * TTL_MS, valid_json)
                .unwrap()
                .body,
            "{}"
        );
        assert_eq!(fake.count(), 3);
    }

    /// Trace: FR-033-AC-12, FR-033-AC-13
    #[test]
    fn server_cooldowns_and_errors_remain_unavailable_after_restart() {
        for status in [403, 429, 200] {
            let dir = tempfile::tempdir().unwrap();
            let cfg = config(dir.path());
            let fake = Fake::new(move |_, _| {
                Ok(Response {
                    status,
                    headers: [
                        ("x-ratelimit-remaining".into(), "0".into()),
                        ("x-ratelimit-reset".into(), "9000".into()),
                        ("retry-after".into(), "10000".into()),
                    ]
                    .into(),
                    body: "{}".into(),
                })
            });
            let guard = Guard::open(&cfg, fake.clone()).unwrap();
            assert!(guard.request(&request(0, false), 1000, valid_json).is_err());
            drop(guard);
            let guard = Guard::open(&cfg, fake.clone()).unwrap();
            assert!(
                guard
                    .request(&request(1, false), 9_500_000, valid_json)
                    .is_err()
            );
            assert_eq!(fake.count(), 1);
            let cooldown: i64 = guard
                .db
                .borrow()
                .query_row("SELECT cooldown FROM github_gate", [], |r| r.get(0))
                .unwrap();
            assert_eq!(cooldown, 10_001_000);
        }
        let raw = b"HTTP/2.0 429 Too Many Requests\r\nRetry-After: 10000\r\nX-RateLimit-Remaining: 0\r\n\r\n{\"message\":\"limited\"}";
        let http = parse_http(raw).unwrap();
        assert_eq!(http.status, 429);
        assert_eq!(backoff(&http, 1000), 10_001_000);
    }

    /// Trace: FR-033-AC-13
    #[test]
    fn malformed_partial_null_and_cost_excess_never_certify_evidence() {
        for body in [
            "{}",
            "{\"errors\":[{\"type\":\"RATE_LIMITED\"}]}",
            "{\"data\":{\"repository\":null,\"rateLimit\":{\"cost\":26,\"remaining\":1,\"resetAt\":\"2030-01-01T00:00:00Z\"}}}",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let cfg = config(dir.path());
            let body = body.to_owned();
            let fake = Fake::new(move |_, _| Ok(response(body.clone())));
            let source = SelectedGithub::with_guard(
                Guard::open(&cfg, fake.clone()),
                [("agent-ix/one".into(), 1)].into(),
                true,
                1000,
            );
            assert!(source.pull_requests("agent-ix/one", 0.0).is_err());
            assert!(source.pull_request("agent-ix/one", 1).is_err());
            assert_eq!(fake.count(), 1);
        }
        let null = serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"p1":null}}}).to_string();
        assert!(parse_infos(&null, &[1]).unwrap().is_empty());
        assert!(parse_infos(&null, &[2]).is_err());
    }

    /// Trace: FR-033-AC-12, FR-033-AC-13
    #[test]
    fn pagination_storage_and_input_bounds_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|request, _| {
            let n = request
                .args
                .iter()
                .find_map(|a| a.strip_prefix("after="))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
                + 1;
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"pullRequests":{"nodes":[],"pageInfo":{"hasNextPage":true,"endCursor":n.to_string()}}}}}).to_string()))
        });
        let source = SelectedGithub::with_guard(
            Guard::open(&cfg, fake.clone()),
            BTreeSet::new(),
            true,
            1000,
        );
        assert!(source.pull_requests("agent-ix/one", 0.0).is_err());
        assert_eq!(fake.count(), MAX_PAGES);
        assert!(source.pull_requests("../invalid", 0.0).is_err());
        assert_eq!(fake.count(), MAX_PAGES);
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request(
                    &Request {
                        args: vec!["x".repeat(MAX_QUERY + 1)],
                        conditional: false
                    },
                    1000,
                    valid_json
                )
                .is_err()
        );
        assert_eq!(fake.count(), MAX_PAGES);
        assert!(
            parse_http(format!("HTTP/2.0 200 OK\r\n\r\n{}", "x".repeat(MAX_BODY + 1)).as_bytes())
                .is_err()
        );
        let large = Fake::new(|_, _| Ok(response("x".repeat(MAX_BODY + 1))));
        let guard = Guard::open(&cfg, large.clone()).unwrap();
        assert!(
            guard
                .request(&request(10, false), 1000, |_| Ok(()))
                .is_err()
        );
        assert_eq!(large.count(), 1);
        let cached: i64 = guard
            .db
            .borrow()
            .query_row("SELECT count(*) FROM github_responses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cached, MAX_PAGES as i64);
    }

    /// Trace: FR-033-AC-12
    #[test]
    fn legacy_expensive_callbacks_are_explicitly_unavailable_and_source_free() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = graph();
        let source = SelectedGithub::with_guard(
            Guard::open(&cfg, fake.clone()),
            BTreeSet::new(),
            true,
            1000,
        );
        assert!(source.main_commits("agent-ix/one", "main", 0.0).is_err());
        let config: crate::retro::department::Config = serde_json::from_value(serde_json::json!({"version":1,"policy_id":"fixture","scope":{},"freshness_seconds":{},"clock_tolerance_seconds":30})).unwrap();
        let evidence = source.department_evidence(&config, 0.0);
        assert!(evidence.complete.is_empty());
        assert!(evidence.observed_at.is_none());
        assert!(evidence.observations.is_empty());
        assert_eq!(fake.count(), 0);
    }

    /// Trace: FR-033-AC-13
    #[test]
    fn persistent_cache_storage_and_retention_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| Ok(response("x".repeat(40_000))));
        let guard = Guard::open(&cfg, fake).unwrap();
        for n in 0..150 {
            let now = 1000 + i64::try_from(n / 40).unwrap() * HOUR_MS;
            guard.request(&request(n, false), now, |_| Ok(())).unwrap();
        }
        let (bytes, count): (i64, i64) = guard
            .db
            .borrow()
            .query_row(
                "SELECT sum(length(CAST(body AS BLOB))),count(*) FROM github_responses",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(bytes <= CACHE_BYTES);
        assert!(count < 128);
        assert!(count > 0);
        guard
            .db
            .borrow()
            .execute("UPDATE github_gate SET cooldown=?1", [i64::MAX])
            .unwrap();
        assert!(
            guard
                .request(&request(200, false), 8 * 86_400_000, |_| Ok(()))
                .is_err()
        );
        let count: i64 = guard
            .db
            .borrow()
            .query_row("SELECT count(*) FROM github_responses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "cooldown cycles must still prune seven-day response retention"
        );
    }

    /// Trace: FR-033-AC-12, FR-033-AC-13
    #[test]
    fn graph_reset_and_missing_conditional_representation_cannot_bypass_admission() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| {
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":0,"resetAt":"2030-01-01T00:00:00Z"},"repository":null}}).to_string()))
        });
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request(&request(0, false), 1000, graph_body_validator)
                .is_err()
        );
        drop(guard);
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request(&request(1, false), 1000 + 2 * HOUR_MS, graph_body_validator)
                .is_err()
        );
        assert_eq!(fake.count(), 1);
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| {
            Ok(Response {
                status: 304,
                headers: BTreeMap::new(),
                body: String::new(),
            })
        });
        let guard = Guard::open(&cfg, fake.clone()).unwrap();
        assert!(guard.request(&request(0, true), 1000, valid_json).is_err());
        assert!(guard.request(&request(0, true), 1001, valid_json).is_err());
        assert_eq!(fake.count(), 1);
        let response = Response {
            status: 429,
            headers: [(
                "retry-after".into(),
                "Thu, 01 Jan 1970 02:00:00 +0000".into(),
            )]
            .into(),
            body: String::new(),
        };
        assert_eq!(backoff(&response, 1000), 7_200_000);
    }
    fn graph_body_validator(body: &str) -> Result<(), String> {
        graph_body(body).map(|_| ())
    }

    /// Trace: FR-033-AC-12, FR-033-AC-13
    #[test]
    fn native_open_and_merged_rows_resolve_callbacks_without_detail_requests() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|request, _| {
            let query = request
                .args
                .iter()
                .find_map(|a| a.strip_prefix("query="))
                .unwrap();
            let open = query.contains("states: [OPEN]");
            let node = serde_json::json!({
                "number":if open {1} else {2},"state":if open {"OPEN"} else {"MERGED"},
                "createdAt":"2026-11-20T00:00:00Z","updatedAt":"2026-11-22T00:00:00Z",
                "mergedAt":if open {None} else {Some("2026-11-22T00:00:00Z")},
                "closedAt":if open {None} else {Some("2026-11-22T00:00:00Z")},
                "isDraft":false,"additions":7,"deletions":3,"reviewDecision":"APPROVED",
                "timelineItems":{"totalCount":0},"reopenEvents":{"totalCount":0}
            });
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":3,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"pullRequests":{"nodes":[node],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}).to_string()))
        });
        let at = chrono::DateTime::parse_from_rfc3339("2026-11-22T01:00:00Z")
            .unwrap()
            .timestamp_millis();
        let allowed = [("agent-ix/one".into(), 1), ("agent-ix/one".into(), 2)].into();
        let source = SelectedGithub::with_guard(Guard::open(&cfg, fake.clone()), allowed, true, at);
        let rows = source
            .pull_requests("agent-ix/one", at as f64 / 1000.0 - 86_400.0)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].additions + rows[0].deletions, 10);
        assert!(rows[0].open);
        assert!(!source.pull_request("agent-ix/one", 1).unwrap().merged);
        assert!(source.pull_request("agent-ix/one", 2).unwrap().merged);
        assert_eq!(fake.count(), 2);
        assert_eq!(source.observed_at(), at);
    }
}
