// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Exporter-owned GitHub admission, persisted response cache and bounded queries.
use super::Config;
use super::github_http::Http;
use crate::retro::sources::{GithubSource, MainCommit, PullRequest, PullRequestInfo};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const TTL_MS: i64 = 900_000;
const HOUR_MS: i64 = 3_600_000;
pub(super) const MAX_BODY: usize = 1024 * 1024;
const MAX_REQUESTS: i64 = 40;
// Fixed selections expose at most 50 PR nodes and two one-item timeline
// connections per PR (or 25 flat linked PR aliases). Reserve 25 points even
// though GitHub's documented connection calculation yields a few points.
const RESERVED_COST: i64 = 25;
const MAX_COST: i64 = 1000;
const MAX_PAGES: usize = 4;
const MAX_OLD: usize = 50;
const MAX_REPOS: usize = 32;
pub(super) const MAX_QUERY: usize = 16 * 1024;
const CACHE_BYTES: i64 = 4 * 1024 * 1024;

#[derive(serde::Serialize)]
pub(super) struct Request {
    pub(super) query: String,
    pub(super) variables: BTreeMap<String, String>,
    #[serde(skip)]
    pub(super) conditional: bool,
}
impl Request {
    pub(super) fn encoded(&self) -> Result<Vec<u8>, String> {
        let body = serde_json::to_vec(self).map_err(|_| "invalid GitHub request")?;
        if body.len() > MAX_QUERY {
            return Err("GitHub serialized query byte limit exceeded".into());
        }
        Ok(body)
    }
}

/// Executes one bounded, quota-admitted GraphQL query through the shared GitHub
/// journal. Callers must include a `repository` field in the returned `data`.
/// The query and variables are part of the shared cache key; response validation,
/// timeout, rolling-hour admission and cooldown are handled by `Guard`.
pub(crate) fn guarded_graphql(
    state_path: &std::path::Path,
    max_bytes: u64,
    query: &str,
    variables: BTreeMap<String, String>,
) -> Result<serde_json::Value, String> {
    if query.is_empty() || query.len() > MAX_QUERY || variables.len() > 16 {
        return Err("GitHub query exceeds bounded request shape".into());
    }
    let request = Request {
        query: query.to_owned(),
        variables,
        conditional: false,
    };
    let guard = Guard::open_storage(state_path, max_bytes, Http::local()?)?;
    let cached = guard.request(&request, |body, _| graph_body(body).map(|_| ()))?;
    serde_json::from_str(&cached.body).map_err(|_| "GitHub response is not valid JSON".into())
}
pub(super) struct Response {
    pub(super) status: u16,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: String,
}
pub(super) trait Transport {
    fn namespace(&self) -> &str;
    fn send(&self, request: &Request, etag: Option<&str>) -> Result<Response, String>;
}
trait Clock: Send + Sync {
    fn now_ms(&self) -> Result<i64, String>;
}
struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> Result<i64, String> {
        super::now_ms().map_err(|_| "GitHub clock unavailable".into())
    }
}
struct Cached {
    body: String,
    at: i64,
}
struct Guard<T> {
    db: RefCell<Connection>,
    transport: T,
    clock: Arc<dyn Clock>,
    cache_bytes: i64,
}
impl<T: Transport> Guard<T> {
    fn open(config: &Config, transport: T) -> Result<Self, String> {
        Self::with_storage(
            &config.state_path,
            config.max_bytes,
            transport,
            Arc::new(SystemClock),
        )
    }
    fn open_storage(
        state_path: &std::path::Path,
        max_bytes: u64,
        transport: T,
    ) -> Result<Self, String> {
        Self::with_storage(state_path, max_bytes, transport, Arc::new(SystemClock))
    }
    #[cfg(test)]
    fn with_clock(config: &Config, transport: T, clock: Arc<dyn Clock>) -> Result<Self, String> {
        Self::with_storage(&config.state_path, config.max_bytes, transport, clock)
    }
    fn with_storage(
        state_path: &std::path::Path,
        max_bytes: u64,
        transport: T,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, String> {
        if transport.namespace().is_empty() || transport.namespace().len() > 256 {
            return Err("GitHub credential namespace unavailable".into());
        }
        // Use the journal itself: two workers and restarted processes share admission,
        // response leases and the configured SQLite page ceiling, with no new quota file.
        let db = Connection::open(state_path).map_err(|e| e.to_string())?;
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
        if pages.saturating_mul(page_size) > max_bytes {
            return Err("GitHub shared journal exceeds storage ceiling".into());
        }
        db.pragma_update(None, "max_page_count", max_bytes / page_size)
            .map_err(|e| e.to_string())?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS github_gate(id INTEGER PRIMARY KEY CHECK(id=1), cooldown INTEGER NOT NULL, lease INTEGER NOT NULL); INSERT OR IGNORE INTO github_gate VALUES(1,0,0); CREATE TABLE IF NOT EXISTS github_clock(id INTEGER PRIMARY KEY CHECK(id=1), at INTEGER NOT NULL); INSERT OR IGNORE INTO github_clock VALUES(1,0); CREATE TABLE IF NOT EXISTS github_sends(at INTEGER NOT NULL, cost INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS github_responses(key TEXT PRIMARY KEY, body TEXT NOT NULL, etag TEXT, at INTEGER NOT NULL);").map_err(|e| e.to_string())?;
        db.execute("UPDATE github_clock SET at=max(at,coalesce((SELECT max(at) FROM github_sends),0),coalesce((SELECT max(at) FROM github_responses),0)) WHERE id=1", []).map_err(|e| e.to_string())?;
        Ok(Self {
            db: RefCell::new(db),
            transport,
            clock,
            cache_bytes: i64::try_from(max_bytes / 8)
                .map_err(|_| "cache budget overflow")?
                .min(CACHE_BYTES),
        })
    }
    fn request(
        &self,
        request: &Request,
        validate: impl Fn(&str, i64) -> Result<(), String>,
    ) -> Result<Cached, String> {
        let encoded = request.encoded()?;
        let key = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(self.transport.namespace(), &encoded, request.conditional))
                    .map_err(|e| e.to_string())?
            )
        );
        let mut db = self.db.borrow_mut();
        let now = self.clock.now_ms()?;
        // Retention also advances during cooldowns and cache-only cycles.
        db.execute(
            "DELETE FROM github_responses WHERE at < ?1",
            [now.saturating_sub(7 * 86_400_000)],
        )
        .map_err(|e| e.to_string())?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        // Sample after acquiring admission, never from the collection frame.
        let now = self.clock.now_ms()?;
        let last_clock: i64 = tx
            .query_row("SELECT at FROM github_clock WHERE id=1", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if now <= 0 || now < last_clock {
            return Err("GitHub clock precedes persisted admission/receipt".into());
        }
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
            validate(body, *at)?;
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
        tx.execute("UPDATE github_clock SET at=?1 WHERE id=1", [now])
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
        let (received, response) = match self.clock.now_ms() {
            Ok(at) if at >= now => (at, response),
            _ => (now, Err("GitHub response clock moved backwards".into())),
        };
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute("UPDATE github_gate SET lease=0 WHERE id=1", [])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE github_clock SET at=max(at,?1) WHERE id=1",
            [received],
        )
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
                    let until = backoff(&response, received).max(graph_reset);
                    tx.execute("UPDATE github_gate SET cooldown=max(cooldown,?1) WHERE id=1", [until]).map_err(|e| e.to_string())?;
                    Err("GitHub server rate limit; cooldown persisted".into())
                } else if response.status == 304 && request.conditional {
                    match &old {
                        Some((body, _, _)) => validate(body, received).map(|()| Cached { body: body.clone(), at: received }),
                        None => Err("GitHub 304 without cached representation".into()),
                    }
                } else if response.status == 200 && response.body.len() <= MAX_BODY {
                    validate(&response.body, received).map(|()| Cached { body: response.body.clone(), at: received })
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
                [received.saturating_add(HOUR_MS)],
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
    let mut variables =
        BTreeMap::from([("owner".into(), owner.into()), ("name".into(), name.into())]);
    if let Some(after) = after {
        if after.len() > 1024 {
            return Err("GitHub cursor byte limit exceeded".into());
        }
        variables.insert("after".into(), after.into());
    }
    // GraphQL POST has no conditional GET representation. REST requests, when
    // admitted by this transport, use conditional=true and retain ETags on 304.
    Ok(Request {
        query: query.into(),
        variables,
        conditional: false,
    })
}

type PrRows = Result<Vec<PullRequest>, String>;
type InfoRows = Result<BTreeMap<u64, PullRequestInfo>, String>;

pub(super) struct SelectedGithub<T = Http> {
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
        Self::with_guard(
            Http::local().and_then(|transport| Guard::open(config, transport)),
            allowed,
            complete,
            now,
        )
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
    fn available(&self) -> Result<(), String> {
        let guard = self.guard.as_ref().map_err(Clone::clone)?;
        let now = guard.clock.now_ms()?;
        let last_clock: i64 = guard
            .db
            .borrow()
            .query_row("SELECT at FROM github_clock WHERE id=1", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if now < last_clock {
            return Err("GitHub clock precedes persisted receipt".into());
        }
        let cooldown: i64 = guard
            .db
            .borrow()
            .query_row("SELECT cooldown FROM github_gate WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        if cooldown > now {
            return Err("GitHub persisted cooldown active".into());
        }
        Ok(())
    }
    fn read(
        &self,
        request: &Request,
        validate: impl Fn(&str, i64) -> Result<(), String>,
    ) -> Result<Cached, String> {
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
            .request(request, validate)?;
        self.oldest_at.set(if self.oldest_at.get() == 0 {
            cached.at
        } else {
            self.oldest_at.get().min(cached.at)
        });
        Ok(cached)
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
            let body = self.read(&request, |body, at| {
                validate_page(body, repo, since, states, at)
            })?;
            let page =
                crate::retro::live::pull_request_page(&graph_body(&body.body)?, repo, since)?;
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
                    .map(|n| format!("p{n}: pullRequest(number: {n}) {{ state createdAt mergedAt closedAt updatedAt }}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let query = format!(
                    "query($owner:String!,$name:String!) {{ rateLimit {{ cost remaining resetAt }} repository(owner:$owner,name:$name) {{ {fields} }} }}"
                );
                let request = graph_request(&query, repo, None)?;
                let body = self.read(&request, |body, at| {
                    parse_infos(body, chunk, at).map(|_| ())
                })?;
                rows.extend(parse_infos(&body.body, chunk, body.at)?);
            }
            Ok(rows)
        })();
        self.infos.borrow_mut().insert(repo.into(), result.clone());
        result
    }
}
fn validate_page(
    body: &str,
    repo: &str,
    since: Option<f64>,
    states: &str,
    at: i64,
) -> Result<(), String> {
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
    for node in nodes {
        let mut projection = serde_json::Map::new();
        for field in ["state", "createdAt", "mergedAt", "closedAt", "updatedAt"] {
            projection.insert(
                field.into(),
                node.get(field)
                    .ok_or("GitHub PR status field missing")?
                    .clone(),
            );
        }
        let info: RawInfo = serde_json::from_value(projection.into()).map_err(|e| e.to_string())?;
        info.validate(at)?;
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
    state: PrState,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "mergedAt", deserialize_with = "required_nullable_time")]
    merged_at: Option<String>,
    #[serde(rename = "closedAt", deserialize_with = "required_nullable_time")]
    closed_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PrState {
    Open,
    Closed,
    Merged,
}
fn required_nullable_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(value) => Ok(Some(value)),
        _ => Err(serde::de::Error::custom(
            "native timestamp must be a string or explicit null",
        )),
    }
}
impl RawInfo {
    fn validate(&self, at: i64) -> Result<(), String> {
        let created = epoch(&self.created_at)?;
        let updated = epoch(&self.updated_at)?;
        let merged = self.merged_at.as_deref().map(epoch).transpose()?;
        let closed = self.closed_at.as_deref().map(epoch).transpose()?;
        if created < 0.0
            || created > updated
            || updated > at.saturating_add(30_000) as f64 / 1000.0
            || merged.is_some_and(|t| t < created || t > updated)
            || closed.is_some_and(|t| t < created || t > updated)
        {
            return Err("GitHub native PR timestamps are inconsistent/future".into());
        }
        let consistent = match self.state {
            PrState::Open => merged.is_none() && closed.is_none(),
            PrState::Closed => merged.is_none() && closed.is_some(),
            PrState::Merged => matches!((merged,closed), (Some(m),Some(c)) if m <= c),
        };
        if !consistent {
            return Err("GitHub native PR state and merge/close evidence disagree".into());
        }
        Ok(())
    }
}
fn epoch(text: &str) -> Result<f64, String> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|t| t.timestamp_millis() as f64 / 1000.0)
        .map_err(|e| e.to_string())
}
fn parse_infos(
    body: &str,
    numbers: &[u64],
    at: i64,
) -> Result<BTreeMap<u64, PullRequestInfo>, String> {
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
            info.validate(at)?;
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
        self.available()?;
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
        self.available()?;
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
    use ix_trace_rs::trace;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    type Handler = dyn Fn(&Request, Option<&str>) -> Result<Response, String> + Send + Sync;
    #[derive(Clone)]
    struct Fake {
        count: Arc<AtomicUsize>,
        handler: Arc<Handler>,
        clock: Arc<TestClock>,
        namespace: String,
    }
    impl Fake {
        fn new(
            handler: impl Fn(&Request, Option<&str>) -> Result<Response, String> + Send + Sync + 'static,
        ) -> Self {
            Self {
                count: Arc::new(AtomicUsize::new(0)),
                handler: Arc::new(handler),
                clock: Arc::new(TestClock(std::sync::atomic::AtomicI64::new(1000))),
                namespace: "offline-principal".into(),
            }
        }
        fn count(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }
    }
    impl Transport for Fake {
        fn namespace(&self) -> &str {
            &self.namespace
        }
        fn send(&self, request: &Request, etag: Option<&str>) -> Result<Response, String> {
            self.count.fetch_add(1, Ordering::SeqCst);
            (self.handler)(request, etag)
        }
    }
    struct TestClock(std::sync::atomic::AtomicI64);
    impl TestClock {
        fn set(&self, at: i64) {
            self.0.store(at, Ordering::SeqCst);
        }
    }
    impl Clock for TestClock {
        fn now_ms(&self) -> Result<i64, String> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }
    fn open(config: &Config, fake: Fake) -> Result<Guard<Fake>, String> {
        let clock = fake.clock.clone();
        Guard::with_clock(config, fake, clock)
    }
    impl Guard<Fake> {
        fn request_at(
            &self,
            request: &Request,
            at: i64,
            validate: impl Fn(&str, i64) -> Result<(), String>,
        ) -> Result<Cached, String> {
            self.transport.clock.set(at);
            self.request(request, validate)
        }
    }
    fn source(
        guard: Result<Guard<Fake>, String>,
        allowed: BTreeSet<(String, u64)>,
        complete: bool,
        at: i64,
    ) -> SelectedGithub<Fake> {
        if let Ok(g) = &guard {
            g.transport.clock.set(at);
        }
        SelectedGithub::with_guard(guard, allowed, complete, at)
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
            let query = &request.query;
            assert!(query.len() < MAX_QUERY);
            let repository = if query.contains("pullRequests(") {
                serde_json::json!({"pullRequests":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}})
            } else {
                let mut fields = serde_json::Map::new();
                for number in query.split("pullRequest(number: ").skip(1) {
                    let n: u64 = number.split(')').next().unwrap().parse().unwrap();
                    fields.insert(format!("p{n}"),serde_json::json!({"state":"MERGED","createdAt":"1970-01-01T00:00:00Z","mergedAt":"1970-01-01T00:00:00Z","closedAt":"1970-01-01T00:00:00Z","updatedAt":"1970-01-01T00:00:00Z"}));
                }
                assert!(fields.len() <= 25);
                serde_json::Value::Object(fields)
            };
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":3,"remaining":4000,"resetAt":"2030-01-01T00:00:00Z"},"repository":repository}}).to_string()))
        })
    }
    fn request(key: usize, conditional: bool) -> Request {
        Request {
            query: format!("fixture-{key}"),
            variables: BTreeMap::new(),
            conditional,
        }
    }
    fn valid_json(body: &str, _at: i64) -> Result<(), String> {
        serde_json::from_str::<serde_json::Value>(body)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
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
            let source = source(
                open(&cfg, transport.clone()),
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

    #[trace("FR-033-AC-12")]
    #[test]
    fn hard_budget_survives_restart_and_no_probe_or_send_is_admitted() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| Ok(response("{}".into())));
        let guard = open(&cfg, fake.clone()).unwrap();
        for n in 0..40 {
            guard
                .request_at(&request(n, false), 1000, valid_json)
                .unwrap();
        }
        drop(guard);
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(41, false), 1001, valid_json)
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

    #[trace("FR-033-AC-12")]
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
        let other = open(&cfg, fake.clone()).unwrap();
        let cfg2 = config(dir.path());
        let fake2 = fake.clone();
        let worker = std::thread::spawn(move || {
            open(&cfg2, fake2)
                .unwrap()
                .request_at(&request(1, false), 1000, valid_json)
                .map(|c| c.at)
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            other
                .request_at(&request(1, false), 1000, valid_json)
                .is_err()
        );
        assert!(
            other
                .request_at(&request(2, false), 1000, valid_json)
                .is_err()
        );
        assert_eq!(fake.count(), 1);
        release_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), 1000);
        assert_eq!(
            other
                .request_at(&request(1, false), 1001, valid_json)
                .unwrap()
                .at,
            1000
        );
        assert_eq!(fake.count(), 1);
    }

    #[trace("FR-033-AC-13")]
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
        let guard = open(&cfg, fake.clone()).unwrap();
        assert_eq!(
            guard
                .request_at(&request(1, true), 1000, valid_json)
                .unwrap()
                .at,
            1000
        );
        assert_eq!(
            guard
                .request_at(&request(1, true), 1001, valid_json)
                .unwrap()
                .at,
            1000
        );
        assert_eq!(
            guard
                .request_at(&request(1, true), 1000 + TTL_MS, valid_json)
                .unwrap()
                .at,
            1000 + TTL_MS
        );
        drop(guard);
        let guard = open(&cfg, fake.clone()).unwrap();
        assert_eq!(
            guard
                .request_at(&request(1, true), 1000 + 2 * TTL_MS, valid_json)
                .unwrap()
                .body,
            "{}"
        );
        assert_eq!(fake.count(), 3);
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
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
            let guard = open(&cfg, fake.clone()).unwrap();
            assert!(
                guard
                    .request_at(&request(0, false), 1000, valid_json)
                    .is_err()
            );
            drop(guard);
            let guard = open(&cfg, fake.clone()).unwrap();
            assert!(
                guard
                    .request_at(&request(1, false), 9_500_000, valid_json)
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
    }

    #[trace("FR-033-AC-13")]
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
            let source = source(
                open(&cfg, fake.clone()),
                [("agent-ix/one".into(), 1)].into(),
                true,
                1000,
            );
            assert!(source.pull_requests("agent-ix/one", 0.0).is_err());
            assert!(source.pull_request("agent-ix/one", 1).is_err());
            assert_eq!(fake.count(), 1);
        }
        let null = serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"p1":null}}}).to_string();
        assert!(parse_infos(&null, &[1], 1000).unwrap().is_empty());
        assert!(parse_infos(&null, &[2], 1000).is_err());
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn pagination_storage_and_input_bounds_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|request, _| {
            let n = request
                .variables
                .get("after")
                .map(String::as_str)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
                + 1;
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"pullRequests":{"nodes":[],"pageInfo":{"hasNextPage":true,"endCursor":n.to_string()}}}}}).to_string()))
        });
        let source = source(open(&cfg, fake.clone()), BTreeSet::new(), true, 1000);
        assert!(source.pull_requests("agent-ix/one", 0.0).is_err());
        assert_eq!(fake.count(), MAX_PAGES);
        assert!(source.pull_requests("../invalid", 0.0).is_err());
        assert_eq!(fake.count(), MAX_PAGES);
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(
                    &Request {
                        query: "x".repeat(MAX_QUERY + 1),
                        variables: BTreeMap::new(),
                        conditional: false
                    },
                    1000,
                    valid_json
                )
                .is_err()
        );
        assert_eq!(fake.count(), MAX_PAGES);
        let large = Fake::new(|_, _| Ok(response("x".repeat(MAX_BODY + 1))));
        let guard = open(&cfg, large.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(10, false), 1000, |_, _| Ok(()))
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

    #[trace("FR-033-AC-12")]
    #[test]
    fn legacy_expensive_callbacks_are_explicitly_unavailable_and_source_free() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = graph();
        let source = source(open(&cfg, fake.clone()), BTreeSet::new(), true, 1000);
        assert!(source.main_commits("agent-ix/one", "main", 0.0).is_err());
        let config: crate::retro::department::Config = serde_json::from_value(serde_json::json!({"version":1,"policy_id":"fixture","scope":{},"freshness_seconds":{},"clock_tolerance_seconds":30})).unwrap();
        let evidence = source.department_evidence(&config, 0.0);
        assert!(evidence.complete.is_empty());
        assert!(evidence.observed_at.is_none());
        assert!(evidence.observations.is_empty());
        assert_eq!(fake.count(), 0);
    }

    #[trace("FR-033-AC-13")]
    #[test]
    fn persistent_cache_storage_and_retention_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| Ok(response("x".repeat(40_000))));
        let guard = open(&cfg, fake).unwrap();
        for n in 0..150 {
            let now = 1000 + i64::try_from(n / 40).unwrap() * HOUR_MS;
            guard
                .request_at(&request(n, false), now, |_, _| Ok(()))
                .unwrap();
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
                .request_at(&request(200, false), 8 * 86_400_000, |_, _| Ok(()))
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

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn graph_reset_and_missing_conditional_representation_cannot_bypass_admission() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| {
            Ok(response(serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":0,"resetAt":"2030-01-01T00:00:00Z"},"repository":null}}).to_string()))
        });
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(0, false), 1000, graph_body_validator)
                .is_err()
        );
        drop(guard);
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(1, false), 1000 + 2 * HOUR_MS, graph_body_validator)
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
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(0, true), 1000, valid_json)
                .is_err()
        );
        assert!(
            guard
                .request_at(&request(0, true), 1001, valid_json)
                .is_err()
        );
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
    fn graph_body_validator(body: &str, _at: i64) -> Result<(), String> {
        graph_body(body).map(|_| ())
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn native_open_and_merged_rows_resolve_callbacks_without_detail_requests() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|request, _| {
            let query = &request.query;
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
        let source = source(open(&cfg, fake.clone()), allowed, true, at);
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
        source
            .guard
            .as_ref()
            .unwrap()
            .db
            .borrow()
            .execute("UPDATE github_gate SET cooldown=?1", [at + HOUR_MS])
            .unwrap();
        assert!(source.pull_requests("agent-ix/one", 0.0).is_err());
        assert!(source.pull_request("agent-ix/one", 1).is_err());
        assert_eq!(
            fake.count(),
            2,
            "a shared cooldown overrides prior per-cycle memoization"
        );
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn later_requests_use_admission_and_receipt_clocks_instead_of_the_collection_frame() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let mut fake = graph();
        let base = fake.handler.clone();
        let clock = fake.clock.clone();
        fake.handler = Arc::new(move |request, etag| {
            let response = base(request, etag)?;
            clock.set(clock.now_ms()? + 20_000);
            Ok(response)
        });
        let source = source(open(&cfg, fake.clone()), BTreeSet::new(), true, 1000);
        fake.clock.set(1_000_000);
        source.pull_requests("agent-ix/one", 0.0).unwrap();
        let guard = source.guard.as_ref().unwrap();
        let charges: Vec<i64> = guard
            .db
            .borrow()
            .prepare("SELECT at FROM github_sends ORDER BY rowid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(charges, vec![1_000_000, 1_020_000]);
        assert_eq!(source.observed_at(), 1_020_000);
        assert_eq!(fake.count(), 2);
        drop(source);
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(8, false), 1_030_000, valid_json)
                .is_err(),
            "a restarted worker with a clock older than the prior receipt must fail closed"
        );
        assert_eq!(fake.count(), 2);
    }

    #[trace("FR-033-AC-12")]
    #[test]
    fn rolling_hour_keeps_late_charges_and_cooldown_begins_at_response_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let fake = Fake::new(|_, _| Ok(response("{}".into())));
        let guard = open(&cfg, fake.clone()).unwrap();
        for n in 0..40 {
            guard
                .request_at(
                    &request(n, false),
                    1000 + i64::try_from(n).unwrap() * 1000,
                    valid_json,
                )
                .unwrap();
        }
        drop(guard);
        let guard = open(&cfg, fake.clone()).unwrap();
        guard
            .request_at(&request(40, false), HOUR_MS + 2000, valid_json)
            .unwrap();
        guard
            .request_at(&request(41, false), HOUR_MS + 2000, valid_json)
            .unwrap();
        assert!(
            guard
                .request_at(&request(42, false), HOUR_MS + 2000, valid_json)
                .is_err()
        );
        assert_eq!(
            fake.count(),
            42,
            "only the first two charges have aged out of the rolling hour"
        );

        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let mut fake = Fake::new(|_, _| unreachable!());
        let clock = fake.clock.clone();
        fake.handler = Arc::new(move |_, _| {
            clock.set(21_000);
            Ok(Response {
                status: 429,
                headers: [("retry-after".into(), "7200".into())].into(),
                body: String::new(),
            })
        });
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(0, false), 1000, valid_json)
                .is_err()
        );
        let until: i64 = guard
            .db
            .borrow()
            .query_row("SELECT cooldown FROM github_gate", [], |r| r.get(0))
            .unwrap();
        assert_eq!(until, 7_221_000);
        drop(guard);
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(1, false), 7_211_000, valid_json)
                .is_err()
        );
        assert_eq!(fake.count(), 1);
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn backward_response_clock_never_stores_a_fresh_representation() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let mut fake = Fake::new(|_, _| unreachable!());
        let clock = fake.clock.clone();
        fake.handler = Arc::new(move |_, _| {
            clock.set(999);
            Ok(response("{}".into()))
        });
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(1, false), 1000, valid_json)
                .is_err()
        );
        let count: i64 = guard
            .db
            .borrow()
            .query_row("SELECT count(*) FROM github_responses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(
            guard
                .request_at(&request(2, false), 999, valid_json)
                .is_err()
        );
        assert_eq!(fake.count(), 1);
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn credential_namespaces_isolate_private_cache_bodies_and_share_the_hard_budget() {
        use super::super::github_http::tests::{Fixture, reply};
        let server =
            Fixture::start(|n, _| reply(200, "", &format!("{{\"account_observation\":{n}}}")));
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let clock = Arc::new(TestClock(std::sync::atomic::AtomicI64::new(1000)));
        let first = Http::fixture(&server.endpoint, "offline-first-account").unwrap();
        let first_ns = first.namespace().to_owned();
        let guard = Guard::with_clock(&cfg, first, clock.clone()).unwrap();
        assert_eq!(
            guard.request(&request(0, false), valid_json).unwrap().body,
            "{\"account_observation\":1}"
        );
        drop(guard);
        let same = Http::fixture(&server.endpoint, "offline-first-account").unwrap();
        assert_eq!(same.namespace(), first_ns);
        let guard = Guard::with_clock(&cfg, same, clock.clone()).unwrap();
        assert_eq!(
            guard.request(&request(0, false), valid_json).unwrap().body,
            "{\"account_observation\":1}"
        );
        drop(guard);
        let second = Http::fixture(&server.endpoint, "offline-second-account").unwrap();
        assert_ne!(second.namespace(), first_ns);
        let guard = Guard::with_clock(&cfg, second, clock).unwrap();
        assert_eq!(
            guard.request(&request(0, false), valid_json).unwrap().body,
            "{\"account_observation\":2}"
        );
        let seen = server.finish();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].contains("Bearer offline-first-account"));
        assert!(seen[1].contains("Bearer offline-second-account"));
        assert!(Http::fixture("http://127.0.0.1:1/graphql", "").is_err());

        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let mut fake = Fake::new(|_, _| Ok(response("{}".into())));
        fake.namespace.clear();
        assert!(
            open(&cfg, fake.clone()).is_err(),
            "an unknown principal cannot inspect any cached body"
        );
        fake.namespace = "first".into();
        let guard = open(&cfg, fake.clone()).unwrap();
        for n in 0..40 {
            guard
                .request_at(&request(n, false), 1000, valid_json)
                .unwrap();
        }
        drop(guard);
        fake.namespace = "second".into();
        let guard = open(&cfg, fake.clone()).unwrap();
        assert!(
            guard
                .request_at(&request(0, false), 1000, valid_json)
                .is_err()
        );
        assert_eq!(
            fake.count(),
            40,
            "account switches cannot reset shared request accounting"
        );
    }

    #[trace("FR-033-AC-12")]
    #[test]
    fn the_fortieth_actual_http_attempt_cannot_hide_an_extra_redirect_send() {
        use super::super::github_http::tests::{Fixture, reply};
        let server = Fixture::start(|n, _| {
            if n == 40 {
                reply(302, "Location: /again\r\n", "{}")
            } else {
                reply(200, "", "{}")
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let http = Http::fixture(&server.endpoint, "offline-credential").unwrap();
        let clock = Arc::new(TestClock(std::sync::atomic::AtomicI64::new(1000)));
        let guard = Guard::with_clock(&cfg, http, clock).unwrap();
        for n in 0..39 {
            guard.request(&request(n, false), valid_json).unwrap();
        }
        assert!(guard.request(&request(39, false), valid_json).is_err());
        assert!(guard.request(&request(40, false), valid_json).is_err());
        let ledger: i64 = guard
            .db
            .borrow()
            .query_row("SELECT count(*) FROM github_sends", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ledger, 40);
        assert_eq!(
            server.finish().len(),
            40,
            "wire requests, including redirects, must match admitted debits"
        );
    }

    #[trace("FR-033-AC-13")]
    #[test]
    fn required_nullable_native_status_fields_and_timestamp_consistency_are_not_inferred() {
        let at = chrono::DateTime::parse_from_rfc3339("2026-11-22T01:00:00Z")
            .unwrap()
            .timestamp_millis();
        let native = serde_json::json!({"state":"OPEN","createdAt":"2026-11-20T00:00:00Z","updatedAt":"2026-11-22T00:00:00Z","mergedAt":null,"closedAt":null});
        let batch = |node| {
            serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"p1":node}}}).to_string()
        };
        assert!(!parse_infos(&batch(native.clone()), &[1], at).unwrap()[&1].merged);
        for field in ["mergedAt", "closedAt", "state", "createdAt", "updatedAt"] {
            let mut missing = native.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                parse_infos(&batch(missing), &[1], at).is_err(),
                "missing {field} cannot certify a native PR status"
            );
        }
        for (field, value) in [
            ("state", serde_json::json!("MERGED")),
            ("state", serde_json::json!("CLOSED")),
            ("mergedAt", serde_json::json!("2026-11-21T00:00:00Z")),
            ("closedAt", serde_json::json!("2026-11-21T00:00:00Z")),
            ("createdAt", serde_json::json!("2026-11-23T00:00:00Z")),
            ("updatedAt", serde_json::json!("2026-11-23T00:00:00Z")),
        ] {
            let mut invalid = native.clone();
            invalid[field] = value;
            assert!(parse_infos(&batch(invalid), &[1], at).is_err());
        }
        let mut merged = native;
        merged["state"] = serde_json::json!("MERGED");
        merged["mergedAt"] = serde_json::json!("2026-11-22T00:00:00Z");
        merged["closedAt"] = serde_json::json!("2026-11-22T00:00:00Z");
        assert!(parse_infos(&batch(merged.clone()), &[1], at).unwrap()[&1].merged);
        for field in ["mergedAt", "closedAt"] {
            let mut invalid = merged.clone();
            invalid[field] = serde_json::Value::Null;
            invalid["number"] = serde_json::json!(1);
            invalid["isDraft"] = serde_json::json!(false);
            invalid["additions"] = serde_json::json!(0);
            invalid["deletions"] = serde_json::json!(0);
            invalid["reviewDecision"] = serde_json::Value::Null;
            invalid["timelineItems"] = serde_json::json!({"totalCount":0});
            let body=serde_json::json!({"data":{"rateLimit":{"cost":1,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":{"pullRequests":{"nodes":[invalid],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}).to_string();
            assert!(validate_page(&body, "agent-ix/one", None, "[MERGED, CLOSED]", at).is_err());
        }
    }

    #[trace("FR-033-AC-12")]
    #[test]
    fn two_workers_share_a_fresh_lease_while_a_late_request_crosses_the_hour_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let mut fake = Fake::new(|_, _| unreachable!());
        let count = fake.count.clone();
        let clock = fake.clock.clone();
        fake.handler = Arc::new(move |_, _| {
            if count.load(Ordering::SeqCst) == 40 {
                entered_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                clock.set(HOUR_MS + 20_500);
            }
            Ok(response("{}".into()))
        });
        let guard = open(&cfg, fake.clone()).unwrap();
        for n in 0..39 {
            guard
                .request_at(&request(n, false), 1000, valid_json)
                .unwrap();
        }
        drop(guard);
        let other = open(&cfg, fake.clone()).unwrap();
        let cfg2 = config(dir.path());
        let fake2 = fake.clone();
        let worker = std::thread::spawn(move || {
            open(&cfg2, fake2)
                .unwrap()
                .request_at(&request(39, false), HOUR_MS + 500, valid_json)
                .map(|c| c.at)
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let lease: i64 = other
            .db
            .borrow()
            .query_row("SELECT lease FROM github_gate", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lease, HOUR_MS + 90_500);
        assert!(
            other
                .request_at(&request(40, false), HOUR_MS + 1001, valid_json)
                .is_err()
        );
        assert_eq!(fake.count(), 40);
        release_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), HOUR_MS + 20_500);
        other
            .request_at(&request(40, false), HOUR_MS + 20_501, valid_json)
            .unwrap();
        let charges: Vec<i64> = other
            .db
            .borrow()
            .prepare("SELECT at FROM github_sends ORDER BY rowid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(charges, vec![HOUR_MS + 500, HOUR_MS + 20_501]);
        assert_eq!(fake.count(), 41);
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn actual_http_body_nests_variables_for_listing_historical_and_paginated_queries() {
        use super::super::github_http::tests::{Fixture, reply};
        let server = Fixture::start(|_, request| {
            assert!(request.starts_with("POST /graphql HTTP/1.1\r\n"));
            let body = request.split_once("\r\n\r\n").unwrap().1;
            assert!(body.len() <= MAX_QUERY);
            let payload: serde_json::Value = serde_json::from_str(body).unwrap();
            let root = payload.as_object().unwrap();
            assert_eq!(
                root.len(),
                2,
                "only query and variables belong at the GraphQL JSON root"
            );
            let query = root.get("query").unwrap().as_str().unwrap();
            let variables = root.get("variables").unwrap().as_object().unwrap();
            assert_eq!(variables.get("owner").unwrap(), "agent-ix");
            assert_eq!(variables.get("name").unwrap(), "one");
            assert!(query.contains("$owner"));
            assert!(query.contains("$name"));
            let repository = if query.contains("pullRequests(") {
                let (nodes, has_next, cursor) = if query.contains("states: [OPEN]") {
                    let number = if variables.get("after").is_none() {
                        1
                    } else {
                        assert_eq!(variables.get("after").unwrap(), "opaque-page-two");
                        2
                    };
                    let node = serde_json::json!({"number":number,"state":"OPEN","createdAt":"1970-01-01T00:00:00Z","updatedAt":"1970-01-01T00:00:00Z","mergedAt":null,"closedAt":null,"isDraft":false,"additions":3,"deletions":2,"reviewDecision":null,"timelineItems":{"totalCount":0},"reopenEvents":{"totalCount":0}});
                    (
                        vec![node],
                        number == 1,
                        if number == 1 {
                            Some("opaque-page-two")
                        } else {
                            None
                        },
                    )
                } else {
                    assert_eq!(variables.len(), 2);
                    assert!(query.contains("states: [MERGED, CLOSED]"));
                    (Vec::new(), false, None)
                };
                serde_json::json!({"pullRequests":{"nodes":nodes,"pageInfo":{"hasNextPage":has_next,"endCursor":cursor}}})
            } else {
                assert_eq!(variables.len(), 2);
                assert!(query.contains("p3: pullRequest(number: 3)"));
                serde_json::json!({"p3":{"state":"MERGED","createdAt":"1970-01-01T00:00:00Z","updatedAt":"1970-01-01T00:00:00Z","mergedAt":"1970-01-01T00:00:00Z","closedAt":"1970-01-01T00:00:00Z"}})
            };
            let response = serde_json::json!({"data":{"rateLimit":{"cost":3,"remaining":100,"resetAt":"2030-01-01T00:00:00Z"},"repository":repository}}).to_string();
            reply(200, "", &response)
        });
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let http = Http::fixture(&server.endpoint, "offline-credential").unwrap();
        let clock = Arc::new(TestClock(std::sync::atomic::AtomicI64::new(1000)));
        let guard = Guard::with_clock(&cfg, http, clock).unwrap();
        let allowed = (1..=3).map(|n| ("agent-ix/one".into(), n)).collect();
        let source = SelectedGithub::with_guard(Ok(guard), allowed, true, 1000);
        let prs = source.pull_requests("agent-ix/one", 0.0).unwrap();
        assert_eq!(
            prs.iter().map(|pr| pr.number).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(source.pull_request("agent-ix/one", 3).unwrap().merged);
        assert!(!source.pull_request("agent-ix/one", 1).unwrap().merged);
        let seen = server.finish();
        assert_eq!(
            seen.len(),
            4,
            "two open pages, one recent page and one historical batch"
        );
        let pagination: Vec<_> = seen
            .iter()
            .map(|request| {
                let payload: serde_json::Value =
                    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                payload["variables"].get("after").cloned()
            })
            .collect();
        assert_eq!(
            pagination,
            vec![None, Some(serde_json::json!("opaque-page-two")), None, None]
        );
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn final_serialized_graphql_body_limit_includes_envelope_variables_and_json_escaping() {
        use super::super::github_http::tests::{Fixture, reply};
        let server = Fixture::start(|_, request| {
            let body = request.split_once("\r\n\r\n").unwrap().1;
            assert_eq!(body.len(), MAX_QUERY);
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            assert!(value["variables"].is_object());
            reply(200, "", "{}")
        });
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let http = Http::fixture(&server.endpoint, "offline-credential").unwrap();
        let clock = Arc::new(TestClock(std::sync::atomic::AtomicI64::new(1000)));
        let guard = Guard::with_clock(&cfg, http, clock).unwrap();
        let mut request = graph_request("", "agent-ix/one", None).unwrap();
        let overhead = request.encoded().unwrap().len();
        request.query = "x".repeat(MAX_QUERY - overhead);
        assert_eq!(request.encoded().unwrap().len(), MAX_QUERY);
        guard.request(&request, valid_json).unwrap();
        request.variables.insert("after".into(), "page".into());
        assert!(
            guard.request(&request, valid_json).is_err(),
            "variable/envelope bytes must be included before admission"
        );
        request.query = "\"".repeat(MAX_QUERY / 2);
        assert!(
            guard.request(&request, valid_json).is_err(),
            "JSON escaping must be measured after serialization"
        );
        let sends: i64 = guard
            .db
            .borrow()
            .query_row("SELECT count(*) FROM github_sends", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sends, 1);
        assert_eq!(
            server.finish().len(),
            1,
            "oversize final bodies must cause zero HTTP sends"
        );
    }
}
