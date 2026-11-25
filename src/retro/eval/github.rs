// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Bounded GitHub and Prometheus readers plus native-time reducers for FR-047.
use super::{EvalWindow, PROMETHEUS_REQUESTS};
use crate::retro::sources::{GithubSource, MainCommit, PullRequest, PullRequestInfo};
use crate::team_metrics::guarded_graphql;
use ix_trace_rs::implements;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

const MAX_PROM_BODY: usize = 1024 * 1024;
const MAX_GITHUB_ROWS: usize = 100;
static GITHUB_HTTP_REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn request_count() -> u64 {
    GITHUB_HTTP_REQUESTS.load(Ordering::Relaxed)
}

pub(super) fn request_count_for_rules() {
    GITHUB_HTTP_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

/// One pull request with native GitHub timestamps and review state.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestRow {
    pub number: u64,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub closed_at: Option<String>,
    pub is_draft: bool,
    pub additions: u64,
    pub deletions: u64,
    pub review_decision: Option<String>,
    pub author: Option<Actor>,
    pub head_ref_oid: Option<String>,
}

/// GitHub login attached to an authored PR.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct Actor {
    pub login: String,
}

/// Retro's GitHub source; it uses the same guarded GraphQL journal as the exporter.
/// Queries without a guarded reducer fail explicitly instead of invoking `gh` directly.
pub(crate) struct GuardedRetroGithub {
    pub state_path: Option<std::path::PathBuf>,
    pub max_bytes: u64,
}

impl GithubSource for GuardedRetroGithub {
    #[implements("FR-047-AC-1")]
    fn pull_requests(&self, repository: &str, since: f64) -> Result<Vec<PullRequest>, String> {
        let path = self.state_path.as_deref().ok_or_else(|| {
            "shared GitHub guard state path is not configured in eval.json".to_owned()
        })?;
        let cutoff_ms = (since * 1000.0).round() as i64;
        read_pull_requests(path, self.max_bytes, repository).and_then(|rows| {
            rows.into_iter()
                .filter(|row| {
                    row.merged_at.is_none()
                        || timestamp_ms(&row.updated_at).is_some_and(|updated| updated >= cutoff_ms)
                })
                .map(|row| {
                    let created_at = timestamp_epoch(&row.created_at)?;
                    let updated_at = timestamp_epoch(&row.updated_at)?;
                    let merged_at = row.merged_at.as_deref().map(timestamp_epoch).transpose()?;
                    let closed_at = row.closed_at.as_deref().map(timestamp_epoch).transpose()?;
                    Ok(PullRequest {
                        closed_at,
                        reopen_events: None,
                        repo: repository.to_owned(),
                        number: row.number,
                        open: row.merged_at.is_none() && row.closed_at.is_none(),
                        draft: row.is_draft,
                        created_at,
                        merged_at,
                        updated_at,
                        additions: row.additions,
                        deletions: row.deletions,
                        review_decision: row.review_decision,
                        force_pushes: None,
                    })
                })
                .collect()
        })
    }

    #[implements("FR-024-AC-19")]
    fn pull_request(&self, repo: &str, number: u64) -> Result<PullRequestInfo, String> {
        let path = self.state_path.as_deref().ok_or_else(|| {
            "shared GitHub guard state path is not configured in eval.json".to_owned()
        })?;
        let (owner, name) = split_repository(repo)?;
        let query = "query RetroAttachedPullRequest($owner:String!, $name:String!, $number:Int!) { rateLimit { cost remaining resetAt } repository(owner:$owner,name:$name) { pullRequest(number:$number) { mergedAt updatedAt } } }";
        let variables = BTreeMap::from([
            ("owner".into(), owner.into()),
            ("name".into(), name.into()),
            ("number".into(), number.to_string()),
        ]);
        GITHUB_HTTP_REQUESTS.fetch_add(1, Ordering::Relaxed);
        let response = guarded_graphql(path, self.max_bytes, query, variables)?;
        parse_pull_request_info(&response, repo, number)
    }

    fn main_commits(
        &self,
        repo: &str,
        branch: &str,
        _since: f64,
    ) -> Result<Vec<MainCommit>, String> {
        Err(format!(
            "guarded commit query unavailable for {repo}@{branch}"
        ))
    }
}

#[implements("FR-024-AC-19")]
fn parse_pull_request_info(
    response: &serde_json::Value,
    repository: &str,
    number: u64,
) -> Result<PullRequestInfo, String> {
    if response
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(format!(
            "GitHub returned partial PR evidence for {repository}#{number}"
        ));
    }
    let pull_request = response
        .pointer("/data/repository/pullRequest")
        .filter(|value| !value.is_null())
        .ok_or_else(|| format!("GitHub returned no PR for {repository}#{number}"))?;
    let updated_at = pull_request
        .get("updatedAt")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("GitHub returned no valid update time for {repository}#{number}"))
        .and_then(timestamp_epoch)?;
    let merged = pull_request
        .get("mergedAt")
        .and_then(serde_json::Value::as_str)
        .is_some();
    Ok(PullRequestInfo { merged, updated_at })
}

/// Reads one repository's bounded PR connection through the shared team-metrics
/// admission guard. The guard owns timeout, cache, cooldown and quota accounting.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
pub(crate) fn read_pull_requests(
    state_path: &Path,
    max_bytes: u64,
    repository: &str,
) -> Result<Vec<PullRequestRow>, String> {
    let (owner, name) = split_repository(repository)?;
    let query = "query RetroPullRequests($owner:String!, $name:String!) { rateLimit { cost remaining resetAt } repository(owner:$owner,name:$name) { pullRequests(first:100, states:[OPEN,MERGED,CLOSED], orderBy:{field:UPDATED_AT,direction:DESC}) { nodes { number url createdAt updatedAt mergedAt closedAt isDraft additions deletions reviewDecision author { login } headRefOid } pageInfo { hasNextPage } } } }";
    let variables = BTreeMap::from([("owner".into(), owner.into()), ("name".into(), name.into())]);
    GITHUB_HTTP_REQUESTS.fetch_add(1, Ordering::Relaxed);
    let response = guarded_graphql(state_path, max_bytes, query, variables)?;
    if response
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|e| !e.is_empty())
    {
        return Err("GitHub GraphQL returned partial errors".into());
    }
    let connection = response
        .pointer("/data/repository/pullRequests")
        .ok_or_else(|| "GitHub PR response has no repository connection".to_owned())?;
    if connection.pointer("/pageInfo/hasNextPage") != Some(&serde_json::Value::Bool(false)) {
        return Err("GitHub PR response is incomplete beyond the bounded first page".into());
    }
    let nodes = connection
        .get("nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "GitHub PR response has no nodes array".to_owned())?;
    if nodes.len() > MAX_GITHUB_ROWS {
        return Err("GitHub PR response exceeds the bounded page size".into());
    }
    serde_json::from_value(serde_json::Value::Array(nodes.clone()))
        .map_err(|_| "GitHub PR response has an invalid native shape".into())
}

/// Filters PR rows by native merge time, returning unavailable for malformed
/// timestamps instead of silently changing the event window.
#[cfg(test)]
pub(crate) fn merged_in_window<'a>(
    rows: &'a [PullRequestRow],
    window: &EvalWindow,
) -> Result<Vec<&'a PullRequestRow>, String> {
    if window.start_ms >= window.end_ms {
        return Err("GitHub event window is empty or reversed".into());
    }
    let mut selected = Vec::new();
    for row in rows {
        let Some(merged_at) = &row.merged_at else {
            continue;
        };
        let at = timestamp_ms(merged_at)
            .ok_or_else(|| "GitHub PR has an invalid mergedAt timestamp".to_owned())?;
        if at >= window.start_ms && at < window.end_ms {
            selected.push(row);
        }
    }
    Ok(selected)
}

/// Makes a single bounded Prometheus range request and retains its native samples.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
pub(crate) fn read_prometheus_range(
    url: &str,
    expression: &str,
    window: &EvalWindow,
) -> Result<String, String> {
    if expression.is_empty() || expression.len() > 1024 {
        return Err("Prometheus expression exceeds the bounded query limit".into());
    }
    if window.start_ms >= window.end_ms {
        return Err("Prometheus event window is empty or reversed".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Prometheus client unavailable")?;
    let endpoint = format!("{}/api/v1/query_range", url.trim_end_matches('/'));
    PROMETHEUS_REQUESTS.fetch_add(1, Ordering::Relaxed);
    let mut response = client
        .get(endpoint)
        .query(&[
            ("query", expression),
            ("start", &(window.start_ms as f64 / 1000.0).to_string()),
            ("end", &(window.end_ms as f64 / 1000.0).to_string()),
            ("step", "60"),
        ])
        .send()
        .map_err(|_| "Prometheus range request failed".to_owned())?
        .error_for_status()
        .map_err(|_| "Prometheus range response was unsuccessful".to_owned())?;
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_PROM_BODY)
    {
        return Err("Prometheus response exceeds the bounded body size".into());
    }
    let mut body = String::new();
    std::io::Read::take(&mut response, (MAX_PROM_BODY + 1) as u64)
        .read_to_string(&mut body)
        .map_err(|_| "Prometheus response could not be read".to_owned())?;
    if body.len() > MAX_PROM_BODY {
        return Err("Prometheus response exceeds the bounded body size".into());
    }
    Ok(body)
}

/// Parses Prometheus's actual range-vector wire shape into timestamped samples. The exact end
/// sample is retained as an interpolation boundary for time-weighted gauges.
pub(crate) fn prometheus_samples(
    body: &str,
    window: &EvalWindow,
) -> Result<Vec<(i64, f64)>, String> {
    let document: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "Prometheus response is not JSON".to_owned())?;
    if document.get("status").and_then(serde_json::Value::as_str) != Some("success") {
        return Err("Prometheus response status is not success".into());
    }
    let series = document
        .pointer("/data/result")
        .and_then(serde_json::Value::as_array)
        .filter(|rows| !rows.is_empty())
        .ok_or_else(|| "Prometheus range query returned no series".to_owned())?;
    let mut samples = Vec::new();
    for row in series {
        let values = row
            .get("values")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "Prometheus range series has no values array".to_owned())?;
        for value in values {
            let pair = value
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or_else(|| "Prometheus sample has an invalid shape".to_owned())?;
            let at_ms = pair[0]
                .as_f64()
                .map(|seconds| (seconds * 1000.0).round() as i64)
                .ok_or_else(|| "Prometheus sample has no timestamp".to_owned())?;
            if at_ms < window.start_ms || at_ms > window.end_ms {
                continue;
            }
            let sample = pair[1]
                .as_str()
                .and_then(|text| text.parse::<f64>().ok())
                .filter(|number| number.is_finite())
                .ok_or_else(|| "Prometheus sample has no finite value".to_owned())?;
            samples.push((at_ms, sample));
        }
    }
    samples.sort_by_key(|(at, _)| *at);
    if samples.is_empty() {
        return Err("Prometheus range query has no in-window samples".into());
    }
    Ok(samples)
}

pub(super) fn split_repository(repository: &str) -> Result<(&str, &str), String> {
    let mut parts = repository.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("GitHub repository must be owner/name".into());
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    };
    if !valid(owner) || !valid(name) {
        return Err("GitHub repository must be owner/name".into());
    }
    Ok((owner, name))
}

pub(super) fn timestamp_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

fn timestamp_epoch(value: &str) -> Result<f64, String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis() as f64 / 1000.0)
        .ok_or_else(|| "GitHub PR has an invalid timestamp".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[trace("FR-024-AC-19")]
    fn attached_pull_request_state_requires_exact_merged_state_and_valid_timestamp() {
        let merged = serde_json::json!({
            "data": {"repository": {"pullRequest": {
                "mergedAt": "2023-11-14T22:13:20Z",
                "updatedAt": "2023-11-14T22:14:20Z"
            }}}
        });
        let info = parse_pull_request_info(&merged, "agent-ix/ix-board", 42).unwrap();
        assert!(info.merged);
        assert_eq!(info.updated_at, 1_700_000_060.0);

        let open = serde_json::json!({
            "data": {"repository": {"pullRequest": {
                "mergedAt": null,
                "updatedAt": "2023-11-14T22:14:20Z"
            }}}
        });
        assert!(
            !parse_pull_request_info(&open, "agent-ix/ix-board", 42)
                .unwrap()
                .merged
        );
    }
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-047-AC-1")]
    fn github_merge_window_uses_native_merge_time_and_half_open_bounds() {
        let rows: Vec<PullRequestRow> = serde_json::from_str(
            r#"[
                {"number":1,"url":"https://github.com/o/r/pull/1","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z","mergedAt":"2026-01-02T00:00:00Z","closedAt":"2026-01-02T00:00:00Z","isDraft":false,"additions":8,"deletions":2,"reviewDecision":"APPROVED","author":{"login":"a"},"headRefOid":"abc"},
                {"number":2,"url":"https://github.com/o/r/pull/2","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z","mergedAt":null,"closedAt":null,"isDraft":false,"additions":3,"deletions":1,"reviewDecision":null,"author":null,"headRefOid":null}
            ]"#,
        )
        .unwrap();
        let window = EvalWindow {
            start_ms: 1_767_312_000_000,
            end_ms: 1_767_398_400_000,
        };
        let selected = merged_in_window(&rows, &window).unwrap();
        assert_eq!(selected.iter().map(|pr| pr.number).collect::<Vec<_>>(), [1]);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn prometheus_reducer_accepts_native_range_vector_and_filters_window() {
        let body = r#"{"status":"success","data":{"resultType":"matrix","result":[{"metric":{"host":"h1"},"values":[[10,"2"],[20,"4"],[30,"9"]]}]}}"#;
        let window = EvalWindow {
            start_ms: 10_000,
            end_ms: 30_000,
        };
        assert_eq!(
            prometheus_samples(body, &window).unwrap(),
            [(10_000, 2.0), (20_000, 4.0), (30_000, 9.0)]
        );
    }
}
