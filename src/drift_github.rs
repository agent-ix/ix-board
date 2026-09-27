// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! GitHub PR event collection with explicit search-page completeness.

use std::collections::BTreeMap;
use std::process::Command;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::{DriftConfig, DriftPullRequest, MissingEvidence};

const QUERY: &str = "query($q:String!,$after:String){search(query:$q,type:ISSUE,first:100,after:$after){issueCount pageInfo{hasNextPage endCursor} nodes{... on PullRequest{number url title headRefName createdAt mergedAt repository{nameWithOwner}}}}}";
const SEARCH_LIMIT: usize = 1000;

#[derive(Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
    errors: Option<Vec<GraphQlError>>,
}
#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}
#[derive(Deserialize)]
struct GraphQlData {
    search: SearchPage,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPage {
    issue_count: usize,
    page_info: PageInfo,
    nodes: Vec<Option<GithubPr>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubPr {
    number: u64,
    url: String,
    title: String,
    head_ref_name: String,
    created_at: String,
    merged_at: Option<String>,
    repository: GithubRepository,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubRepository {
    name_with_owner: String,
}

/// Queries PRs created in the activity lookback and PRs merged in the report window.
/// Returns every received record plus missing-source reasons for any incomplete query.
pub fn collect_prs(
    organization: &str,
    config: &DriftConfig,
) -> (Vec<DriftPullRequest>, Vec<MissingEvidence>) {
    let mut records = BTreeMap::new();
    let mut missing = Vec::new();
    if organization.is_empty()
        || organization.len() > 39
        || !organization
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        missing.push(MissingEvidence {
            source: "github.prs".into(),
            reason: "invalid GitHub organization name".into(),
        });
        return (Vec::new(), missing);
    }
    let report_time = match DateTime::parse_from_rfc3339(&config.report_time) {
        Ok(time) => time.with_timezone(&Utc),
        Err(error) => {
            missing.push(MissingEvidence {
                source: "github.prs".into(),
                reason: format!("invalid report time: {error}"),
            });
            return (Vec::new(), missing);
        }
    };
    let activity_hours = config.window_hours.max(config.stall_hours);
    let Some(activity_duration) = hours_duration(activity_hours) else {
        missing.push(MissingEvidence {
            source: "github.prs".into(),
            reason: "activity lookback is outside the supported duration range".into(),
        });
        return (Vec::new(), missing);
    };
    let Some(window_duration) = hours_duration(config.window_hours) else {
        missing.push(MissingEvidence {
            source: "github.prs".into(),
            reason: "report window is outside the supported duration range".into(),
        });
        return (Vec::new(), missing);
    };
    let Some(activity_start) = report_time.checked_sub_signed(activity_duration) else {
        missing.push(MissingEvidence {
            source: "github.prs".into(),
            reason: "activity start is outside the supported timestamp range".into(),
        });
        return (Vec::new(), missing);
    };
    let Some(window_start) = report_time.checked_sub_signed(window_duration) else {
        missing.push(MissingEvidence {
            source: "github.prs".into(),
            reason: "window start is outside the supported timestamp range".into(),
        });
        return (Vec::new(), missing);
    };
    for (kind, since) in [("created", activity_start), ("merged", window_start)] {
        let search = format!(
            "org:{organization} is:pr {kind}:>={}",
            since.format("%Y-%m-%d")
        );
        let mut cursor: Option<String> = None;
        let mut received = 0usize;
        loop {
            let mut command = Command::new("gh");
            command.args([
                "api",
                "graphql",
                "-f",
                &format!("query={QUERY}"),
                "-f",
                &format!("q={search}"),
            ]);
            if let Some(after) = &cursor {
                command.args(["-f", &format!("after={after}")]);
            }
            let output = match command.output() {
                Ok(output) => output,
                Err(error) => {
                    missing.push(gap(kind, format!("gh execution failed: {error}")));
                    break;
                }
            };
            if !output.status.success() {
                missing.push(gap(
                    kind,
                    format!("gh exited with status {}", output.status),
                ));
                break;
            }
            let response: GraphQlResponse = match serde_json::from_slice(&output.stdout) {
                Ok(response) => response,
                Err(error) => {
                    missing.push(gap(kind, format!("response decode failed: {error}")));
                    break;
                }
            };
            if let Some(errors) = response.errors {
                missing.extend(errors.into_iter().map(|error| gap(kind, error.message)));
                break;
            }
            let Some(data) = response.data else {
                missing.push(gap(kind, "response lacked data".into()));
                break;
            };
            let page = data.search;
            for node in page.nodes {
                let Some(pr) = node else {
                    missing.push(gap(kind, "null search result".into()));
                    continue;
                };
                records.insert(
                    (pr.repository.name_with_owner.clone(), pr.number),
                    DriftPullRequest {
                        repository: pr.repository.name_with_owner,
                        number: pr.number,
                        url: pr.url,
                        title: pr.title,
                        branch: pr.head_ref_name,
                        created_at: pr.created_at,
                        merged_at: pr.merged_at,
                        event_ids: Vec::new(),
                    },
                );
                received += 1;
            }
            if !page.page_info.has_next_page {
                break;
            }
            if received >= SEARCH_LIMIT || page.issue_count > SEARCH_LIMIT {
                missing.push(gap(
                    kind,
                    format!("search exceeds the {SEARCH_LIMIT}-result pagination limit"),
                ));
                break;
            }
            let Some(after) = page.page_info.end_cursor else {
                missing.push(gap(kind, "unresolved next-page cursor".into()));
                break;
            };
            cursor = Some(after);
        }
    }
    (records.into_values().collect(), missing)
}

fn gap(kind: &str, reason: String) -> MissingEvidence {
    MissingEvidence {
        source: "github.prs".into(),
        reason: format!("{kind} query: {reason}"),
    }
}

fn hours_duration(hours: f64) -> Option<Duration> {
    if !hours.is_finite() || hours <= 0.0 || hours * 3_600_000.0 > i64::MAX as f64 {
        return None;
    }
    Duration::try_milliseconds((hours * 3_600_000.0).ceil() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_pr_page_decodes_repository_and_merge_event() {
        let response:GraphQlResponse=serde_json::from_str(r#"{"data":{"search":{"issueCount":1,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"number":7,"url":"https://github.com/agent-ix/ix-board/pull/7","title":"SWM-2","headRefName":"swm-2-code","createdAt":"2026-09-26T00:00:00Z","mergedAt":"2026-09-27T00:00:00Z","repository":{"nameWithOwner":"agent-ix/ix-board"}}]}}}"#).unwrap();
        let pr = response
            .data
            .unwrap()
            .search
            .nodes
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(pr.repository.name_with_owner, "agent-ix/ix-board");
        assert_eq!(pr.merged_at.as_deref(), Some("2026-09-27T00:00:00Z"));
    }
}
