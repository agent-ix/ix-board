// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Complete-evidence reducers for the role-specific GitHub queries in FR-047.

use super::{EvalWindow, QueryValue, RoleSpec};
use ix_trace_rs::implements;
use std::collections::{BTreeMap, BTreeSet};

const MAX_ROWS_PER_CONNECTION: usize = 100;
const MAX_PR_PAGES: usize = 20;
const MAX_NESTED_PAGES: usize = 20;

/// Linear/transcript joins keyed by the exact repository and PR number. Missing entries are
/// deliberately preserved as unknown so reducers cannot turn unreadable joins into clean passes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PullRequestJoins {
    pub reviewed_heads: Option<BTreeSet<String>>,
    pub gate_required: Option<bool>,
    pub gate_covered_heads: Option<BTreeSet<String>>,
    pub waived_for_gate: Option<bool>,
    pub waived_for_review: Option<bool>,
    pub reviewer_or_dispatch_by_deadline: Option<bool>,
    pub reviewer_or_dispatch_by_window_end: Option<bool>,
    pub ready_merge_waived: Option<bool>,
    pub ticket_started_at_ms: Option<i64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestPage {
    nodes: Vec<PullRequestNode>,
    page_info: PageInfo,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestNode {
    number: u64,
    id: String,
    url: String,
    title: String,
    created_at: String,
    merged_at: Option<String>,
    closed_at: Option<String>,
    is_draft: bool,
    base_ref_oid: Option<String>,
    head_ref_oid: Option<String>,
    head_ref_name: Option<String>,
    timeline_items: Connection<ReadyEvent>,
    reviews: Connection<ReviewNode>,
    commits: Connection<CommitNode>,
    files: Connection<FileNode>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    nodes: Vec<T>,
    page_info: PageInfo,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadyEvent {
    #[serde(rename = "__typename")]
    kind: String,
    created_at: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewNode {
    submitted_at: Option<String>,
    commit: Option<CommitOid>,
}

#[derive(serde::Deserialize)]
struct CommitOid {
    #[serde(default)]
    id: Option<String>,
    oid: String,
    #[serde(default)]
    parents: Option<Connection<CommitParent>>,
}

#[derive(serde::Deserialize)]
struct CommitNode {
    commit: CommitOid,
}

#[derive(serde::Deserialize)]
struct CommitParent {
    oid: String,
}

#[derive(serde::Deserialize)]
struct FileNode {
    path: String,
}

/// Reads one bounded, guarded GitHub query per configured repository and joins the supplied
/// Linear/role evidence. Any truncated PR or nested connection rejects the entire population.
#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
pub fn read_pull_request_evidence(
    state_path: &std::path::Path,
    max_bytes: u64,
    repositories: &[String],
    joins: &BTreeMap<(String, u64), PullRequestJoins>,
    deadline: &crate::Deadline<'_>,
) -> Result<Vec<PullRequestEvidence>, String> {
    if repositories.is_empty() {
        return Err("retro config has no repositories for role-specific GitHub evidence".into());
    }
    let query_first = "query RetroRolePullRequests($owner:String!, $name:String!) { rateLimit { cost remaining resetAt } repository(owner:$owner,name:$name) { pullRequests(first:100, states:[OPEN,MERGED,CLOSED], orderBy:{field:UPDATED_AT,direction:DESC}) { nodes { number id url title createdAt mergedAt closedAt isDraft baseRefOid headRefOid headRefName timelineItems(first:100,itemTypes:[READY_FOR_REVIEW_EVENT,CONVERT_TO_DRAFT_EVENT]) { nodes { __typename ... on ReadyForReviewEvent { createdAt } ... on ConvertToDraftEvent { createdAt } } pageInfo { hasNextPage endCursor } } reviews(first:100) { nodes { submittedAt commit { oid } } pageInfo { hasNextPage endCursor } } commits(first:100) { nodes { commit { id oid parents(first:100) { nodes { oid } pageInfo { hasNextPage endCursor } } } } pageInfo { hasNextPage endCursor } } files(first:100) { nodes { path } pageInfo { hasNextPage endCursor } } } pageInfo { hasNextPage endCursor } } } }";
    let query_next = "query RetroRolePullRequests($owner:String!, $name:String!, $cursor:String!) { rateLimit { cost remaining resetAt } repository(owner:$owner,name:$name) { pullRequests(first:100, after:$cursor, states:[OPEN,MERGED,CLOSED], orderBy:{field:UPDATED_AT,direction:DESC}) { nodes { number id url title createdAt mergedAt closedAt isDraft baseRefOid headRefOid headRefName timelineItems(first:100,itemTypes:[READY_FOR_REVIEW_EVENT,CONVERT_TO_DRAFT_EVENT]) { nodes { __typename ... on ReadyForReviewEvent { createdAt } ... on ConvertToDraftEvent { createdAt } } pageInfo { hasNextPage endCursor } } reviews(first:100) { nodes { submittedAt commit { oid } } pageInfo { hasNextPage endCursor } } commits(first:100) { nodes { commit { id oid parents(first:100) { nodes { oid } pageInfo { hasNextPage endCursor } } } } pageInfo { hasNextPage endCursor } } files(first:100) { nodes { path } pageInfo { hasNextPage endCursor } } } pageInfo { hasNextPage endCursor } } } }";
    let mut evidence = Vec::new();
    for repository in repositories {
        let (owner, name) = super::github::split_repository(repository)?;
        let mut cursor: Option<String> = None;
        let mut nodes = Vec::new();
        let mut complete = false;
        for page_number in 0..MAX_PR_PAGES {
            let mut variables = BTreeMap::from([
                ("owner".into(), owner.to_owned()),
                ("name".into(), name.to_owned()),
            ]);
            let query = if let Some(cursor) = cursor.as_ref() {
                variables.insert("cursor".into(), cursor.clone());
                query_next
            } else {
                query_first
            };
            ensure_request_before_deadline(deadline)?;
            super::github::request_count_for_rules();
            let response =
                crate::team_metrics::guarded_graphql(state_path, max_bytes, query, variables)?;
            if response
                .get("errors")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|errors| !errors.is_empty())
            {
                return Err(format!(
                    "GitHub returned partial role-query errors for {repository}"
                ));
            }
            let connection = response
                .pointer("/data/repository/pullRequests")
                .ok_or_else(|| {
                    format!("GitHub response has no pull-request connection for {repository}")
                })?;
            let page: PullRequestPage =
                serde_json::from_value(connection.clone()).map_err(|_| {
                    format!("GitHub response has an invalid pull-request shape for {repository}")
                })?;
            if page.nodes.len() > MAX_ROWS_PER_CONNECTION {
                return Err(format!(
                    "GitHub pull-request page exceeds the bounded page size for {repository}"
                ));
            }
            nodes.extend(page.nodes);
            if !page.page_info.has_next_page {
                complete = true;
                break;
            }
            cursor = Some(page.page_info.end_cursor.ok_or_else(|| {
                format!("GitHub pull-request page has no cursor for {repository}")
            })?);
            if page_number + 1 == MAX_PR_PAGES {
                break;
            }
        }
        if !complete {
            return Err(format!(
                "GitHub pull-request population exceeds the {MAX_PR_PAGES}-page limit for {repository}"
            ));
        }
        for mut pr in nodes {
            complete_pr_connection(
                state_path,
                max_bytes,
                &pr.id,
                "timelineItems",
                "__typename ... on ReadyForReviewEvent { createdAt } ... on ConvertToDraftEvent { createdAt }",
                "itemTypes:[READY_FOR_REVIEW_EVENT,CONVERT_TO_DRAFT_EVENT]",
                &mut pr.timeline_items,
                deadline,
            )?;
            complete_pr_connection(
                state_path,
                max_bytes,
                &pr.id,
                "reviews",
                "submittedAt commit { oid }",
                "",
                &mut pr.reviews,
                deadline,
            )?;
            complete_pr_connection(
                state_path,
                max_bytes,
                &pr.id,
                "commits",
                "commit { id oid parents(first:100) { nodes { oid } pageInfo { hasNextPage endCursor } } }",
                "",
                &mut pr.commits,
                deadline,
            )?;
            complete_pr_connection(
                state_path,
                max_bytes,
                &pr.id,
                "files",
                "path",
                "",
                &mut pr.files,
                deadline,
            )?;
            for commit in &mut pr.commits.nodes {
                let Some(commit_id) = commit.commit.id.as_deref() else {
                    return Err(format!(
                        "GitHub commit ancestry has no commit identity for {repository}#{}",
                        pr.number
                    ));
                };
                let parents = commit.commit.parents.as_mut().ok_or_else(|| {
                    format!(
                        "GitHub commit ancestry is missing for {repository}#{}",
                        pr.number
                    )
                })?;
                complete_commit_parents(state_path, max_bytes, commit_id, parents, deadline)?;
            }
            let created_at_ms = parse_time(&pr.created_at)?;
            let merged_at_ms = pr.merged_at.as_deref().map(parse_time).transpose()?;
            let closed_at_ms = pr.closed_at.as_deref().map(parse_time).transpose()?;
            let mut ready_events = pr
                .timeline_items
                .nodes
                .iter()
                .filter(|event| event.kind == "ReadyForReviewEvent")
                .map(|event| parse_time(&event.created_at))
                .collect::<Result<Vec<_>, _>>()?;
            ready_events.sort_unstable();
            // The latest ready event is the current review cycle; historical cycles have already
            // been superseded by the newer readiness event.
            let ready_events_complete = true;
            let ready_at_ms = if let Some(ready_at) = ready_events.last().copied() {
                Some(ready_at)
            } else if !pr.is_draft
                && !pr
                    .timeline_items
                    .nodes
                    .iter()
                    .any(|event| event.kind == "ConvertToDraftEvent")
            {
                Some(created_at_ms)
            } else {
                None
            };
            let github_review_heads = pr
                .reviews
                .nodes
                .iter()
                .filter(|review| {
                    review
                        .submitted_at
                        .as_deref()
                        .and_then(|at| parse_time(at).ok())
                        .is_some_and(|at| merged_at_ms.is_none_or(|merged| at <= merged))
                })
                .filter_map(|review| review.commit.as_ref().map(|commit| commit.oid.clone()))
                .collect::<BTreeSet<_>>();
            let mut commit_shas = Vec::new();
            let mut commit_ancestry = BTreeMap::new();
            for commit in pr.commits.nodes {
                let sha = commit.commit.oid;
                let parents = commit.commit.parents.expect("ancestry checked above");
                commit_ancestry.insert(
                    sha.clone(),
                    parents.nodes.into_iter().map(|parent| parent.oid).collect(),
                );
                commit_shas.push(sha);
            }
            let changed_files = pr.files.nodes.into_iter().map(|file| file.path).collect();
            let ticket = ticket_from_title(&pr.title)
                .or_else(|| pr.head_ref_name.as_deref().and_then(ticket_from_title));
            let joined = joins
                .get(&(repository.clone(), pr.number))
                .cloned()
                .unwrap_or_default();
            evidence.push(PullRequestEvidence {
                repository: repository.clone(),
                number: pr.number,
                url: pr.url,
                title: Some(pr.title),
                head_ref_name: pr.head_ref_name,
                base_sha: pr.base_ref_oid,
                head_sha: pr.head_ref_oid.clone(),
                commit_shas: Some(commit_shas),
                commit_ancestry: Some(commit_ancestry),
                github_review_heads: Some(github_review_heads),
                changed_files: Some(changed_files),
                created_at_ms: Some(created_at_ms),
                ready_at_ms,
                ready_events_complete,
                review_events_complete: true,
                merged_at_ms,
                closed_at_ms,
                merged_head: pr.head_ref_oid,
                reviewed_heads: joined.reviewed_heads,
                gate_required: joined.gate_required,
                gate_covered_heads: joined.gate_covered_heads,
                waived_for_gate: joined.waived_for_gate,
                waived_for_review: joined.waived_for_review,
                reviewer_or_dispatch_by_deadline: joined.reviewer_or_dispatch_by_deadline,
                reviewer_or_dispatch_by_window_end: joined.reviewer_or_dispatch_by_window_end,
                ready_merge_waived: joined.ready_merge_waived,
                ticket,
                ticket_started_at_ms: joined.ticket_started_at_ms,
            });
        }
    }
    Ok(evidence)
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
fn complete_pr_connection<T: serde::de::DeserializeOwned>(
    state_path: &std::path::Path,
    max_bytes: u64,
    pull_request_id: &str,
    field: &str,
    node_selection: &str,
    arguments: &str,
    connection: &mut Connection<T>,
    deadline: &crate::Deadline<'_>,
) -> Result<(), String> {
    if connection.nodes.len() > MAX_ROWS_PER_CONNECTION {
        return Err(format!("GitHub {field} connection exceeds its page bound"));
    }
    if connection.page_info.has_next_page && connection.page_info.end_cursor.is_none() {
        return Err(format!("GitHub {field} page has no continuation cursor"));
    }
    let first_query = format!(
        "query RetroPrConnection($id:ID!) {{ rateLimit {{ cost remaining resetAt }} node(id:$id) {{ ... on PullRequest {{ {field}(first:100 {arguments}) {{ nodes {{ {node_selection} }} pageInfo {{ hasNextPage endCursor }} }} }} }} }}"
    );
    let next_query = format!(
        "query RetroPrConnection($id:ID!, $cursor:String!) {{ rateLimit {{ cost remaining resetAt }} node(id:$id) {{ ... on PullRequest {{ {field}(first:100, after:$cursor {arguments}) {{ nodes {{ {node_selection} }} pageInfo {{ hasNextPage endCursor }} }} }} }} }}"
    );
    let mut cursor = connection.page_info.end_cursor.clone();
    let mut complete = !connection.page_info.has_next_page;
    for page_number in 1..MAX_NESTED_PAGES {
        if complete {
            break;
        }
        let mut variables = BTreeMap::from([("id".into(), pull_request_id.to_owned())]);
        let query = if let Some(cursor) = cursor.as_ref() {
            variables.insert("cursor".into(), cursor.clone());
            &next_query
        } else {
            &first_query
        };
        ensure_request_before_deadline(deadline)?;
        super::github::request_count_for_rules();
        let response =
            crate::team_metrics::guarded_graphql(state_path, max_bytes, query, variables)?;
        if response
            .get("errors")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(format!("GitHub returned partial {field} evidence"));
        }
        let page_value = response
            .pointer(&format!("/data/node/{field}"))
            .ok_or_else(|| format!("GitHub response has no {field} connection"))?;
        let page: Connection<T> = serde_json::from_value(page_value.clone())
            .map_err(|_| format!("GitHub returned an invalid {field} connection"))?;
        if page.nodes.len() > MAX_ROWS_PER_CONNECTION {
            return Err(format!("GitHub {field} page exceeds its row bound"));
        }
        connection.nodes.extend(page.nodes);
        complete = !page.page_info.has_next_page;
        cursor = page.page_info.end_cursor;
        if !complete && cursor.is_none() {
            return Err(format!("GitHub {field} page has no continuation cursor"));
        }
        if page_number + 1 == MAX_NESTED_PAGES && !complete {
            break;
        }
    }
    if complete {
        Ok(())
    } else {
        Err(format!(
            "GitHub {field} connection exceeds the {MAX_NESTED_PAGES}-page limit"
        ))
    }
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
fn complete_commit_parents(
    state_path: &std::path::Path,
    max_bytes: u64,
    commit_id: &str,
    connection: &mut Connection<CommitParent>,
    deadline: &crate::Deadline<'_>,
) -> Result<(), String> {
    if connection.nodes.len() > MAX_ROWS_PER_CONNECTION {
        return Err("GitHub commit parent connection exceeds its page bound".into());
    }
    if connection.page_info.has_next_page && connection.page_info.end_cursor.is_none() {
        return Err("GitHub commit parent page has no continuation cursor".into());
    }
    let first_query = "query RetroCommitParents($id:ID!) { rateLimit { cost remaining resetAt } node(id:$id) { ... on Commit { parents(first:100) { nodes { oid } pageInfo { hasNextPage endCursor } } } } }";
    let next_query = "query RetroCommitParents($id:ID!, $cursor:String!) { rateLimit { cost remaining resetAt } node(id:$id) { ... on Commit { parents(first:100, after:$cursor) { nodes { oid } pageInfo { hasNextPage endCursor } } } } }";
    let mut cursor = connection.page_info.end_cursor.clone();
    let mut complete = !connection.page_info.has_next_page;
    for _ in 1..MAX_NESTED_PAGES {
        if complete {
            break;
        }
        let mut variables = BTreeMap::from([("id".into(), commit_id.to_owned())]);
        let query = if let Some(cursor) = cursor.as_ref() {
            variables.insert("cursor".into(), cursor.clone());
            next_query
        } else {
            first_query
        };
        ensure_request_before_deadline(deadline)?;
        super::github::request_count_for_rules();
        let response =
            crate::team_metrics::guarded_graphql(state_path, max_bytes, query, variables)?;
        if response
            .get("errors")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err("GitHub returned partial commit-parent evidence".into());
        }
        let page_value = response
            .pointer("/data/node/parents")
            .ok_or_else(|| "GitHub response has no commit parent connection".to_owned())?;
        let page: Connection<CommitParent> = serde_json::from_value(page_value.clone())
            .map_err(|_| "GitHub returned an invalid commit parent connection".to_owned())?;
        if page.nodes.len() > MAX_ROWS_PER_CONNECTION {
            return Err("GitHub commit parent page exceeds its row bound".into());
        }
        connection.nodes.extend(page.nodes);
        complete = !page.page_info.has_next_page;
        cursor = page.page_info.end_cursor;
        if !complete && cursor.is_none() {
            return Err("GitHub commit parent page has no continuation cursor".into());
        }
    }
    if complete {
        Ok(())
    } else {
        Err(format!(
            "GitHub commit ancestry exceeds the {MAX_NESTED_PAGES}-page limit"
        ))
    }
}

#[implements("FR-047-AC-9")]
fn ensure_request_before_deadline(deadline: &crate::Deadline<'_>) -> Result<(), String> {
    match deadline.remaining() {
        Ok(remaining) if remaining > std::time::Duration::from_secs(10) => Ok(()),
        Ok(remaining) => Err(format!(
            "retro evaluation has only {remaining:?} left; refusing a GitHub request with a 10-second transport timeout"
        )),
        Err(error) => Err(format!("retro evaluation deadline elapsed: {error}")),
    }
}

#[implements("FR-047-AC-1")]
fn parse_time(value: &str) -> Result<i64, String> {
    super::github::timestamp_ms(value)
        .ok_or_else(|| "GitHub response contains an invalid event timestamp".to_owned())
}

#[implements("FR-047-AC-1")]
fn ticket_from_title(title: &str) -> Option<String> {
    for token in
        title.split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
    {
        let Some((prefix, suffix)) = token.split_once('-') else {
            continue;
        };
        if !prefix.is_empty()
            && prefix
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
            && !suffix.is_empty()
            && suffix.chars().all(|ch| ch.is_ascii_digit())
        {
            return Some(token.to_owned());
        }
    }
    None
}

/// One PR plus the explicitly joined evidence needed by deterministic role queries.
/// `None` means that the required evidence was not read or could not be attributed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PullRequestEvidence {
    pub repository: String,
    pub number: u64,
    pub url: String,
    pub title: Option<String>,
    pub head_ref_name: Option<String>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub commit_shas: Option<Vec<String>>,
    pub commit_ancestry: Option<BTreeMap<String, BTreeSet<String>>>,
    pub changed_files: Option<BTreeSet<String>>,
    pub created_at_ms: Option<i64>,
    pub ready_at_ms: Option<i64>,
    pub ready_events_complete: bool,
    pub review_events_complete: bool,
    pub merged_at_ms: Option<i64>,
    pub closed_at_ms: Option<i64>,
    pub merged_head: Option<String>,
    pub github_review_heads: Option<BTreeSet<String>>,
    pub reviewed_heads: Option<BTreeSet<String>>,
    pub gate_required: Option<bool>,
    pub gate_covered_heads: Option<BTreeSet<String>>,
    pub waived_for_gate: Option<bool>,
    pub waived_for_review: Option<bool>,
    pub reviewer_or_dispatch_by_deadline: Option<bool>,
    pub reviewer_or_dispatch_by_window_end: Option<bool>,
    pub ready_merge_waived: Option<bool>,
    pub ticket: Option<String>,
    pub ticket_started_at_ms: Option<i64>,
}

/// Reduces the registered role-specific GitHub queries from a complete, already bounded source
/// population. Callers must set each optional field only from complete, provenance-checked reads.
#[implements("FR-047-AC-1")]
pub fn adapters(
    specs: &[RoleSpec],
    rows: Result<&[PullRequestEvidence], &str>,
    window: &EvalWindow,
    review_age_limit_hours: u64,
) -> BTreeMap<String, QueryValue> {
    const REVIEW_UNDISPATCHED: &str = "github.review_ready_prs_undispatched";
    const MERGES_UNREVIEWED: &str = "github.merges_beyond_reviewed_change";
    const MERGES_UNGATED: &str = "github.merges_without_covering_gate";
    const READY_OLD: &str = "github.ready_prs_unmerged_over_hour";
    const TICKET_START: &str = "github.ticket_start_to_pr_open_median_hours";
    let wanted = specs
        .iter()
        .flat_map(|spec| {
            spec.rules
                .iter()
                .filter(|rule| rule.method == "deterministic")
                .map(|rule| rule.query_or_rubric.as_str())
                .chain(spec.metrics.iter().map(|metric| metric.query.as_str()))
        })
        .filter(|query| {
            matches!(
                *query,
                REVIEW_UNDISPATCHED | MERGES_UNREVIEWED | MERGES_UNGATED | READY_OLD | TICKET_START
            )
        })
        .collect::<BTreeSet<_>>();
    if wanted.is_empty() {
        return BTreeMap::new();
    }
    let rows = match rows {
        Ok(rows) => rows,
        Err(reason) => {
            return wanted
                .into_iter()
                .map(|query| (query.to_owned(), unavailable(query, reason)))
                .collect();
        }
    };
    let mut out = BTreeMap::new();
    for query in wanted {
        let value = match query {
            REVIEW_UNDISPATCHED => reduce_review_undispatched(rows, window, review_age_limit_hours),
            MERGES_UNREVIEWED => reduce_merges_beyond_reviewed(rows, window),
            MERGES_UNGATED => reduce_merges_without_gate(rows, window),
            READY_OLD => reduce_ready_over_hour(rows, window),
            TICKET_START => reduce_ticket_start_median(rows, window),
            _ => unreachable!("query was filtered to the registry"),
        };
        out.insert(query.to_owned(), value);
    }
    out
}

#[implements("FR-047-AC-1")]
fn reduce_review_undispatched(
    rows: &[PullRequestEvidence],
    window: &EvalWindow,
    review_age_limit_hours: u64,
) -> QueryValue {
    let mut count = 0_u64;
    let mut evidence = Vec::new();
    for pr in rows {
        if !pr.ready_events_complete {
            return unavailable(
                "github.review_ready_prs_undispatched",
                "pull-request ready-for-review event history is incomplete",
            );
        }
        let Some(ready_at) = pr.ready_at_ms else {
            continue;
        };
        if pr
            .closed_at_ms
            .is_some_and(|closed_at| closed_at < window.end_ms)
            && pr.merged_at_ms.is_none()
        {
            continue;
        }
        let deadline = ready_at.saturating_add(
            review_age_limit_hours
                .saturating_mul(3_600_000)
                .min(i64::MAX as u64) as i64,
        );
        if deadline >= window.end_ms {
            continue;
        }
        let covered = if deadline < window.start_ms {
            pr.reviewer_or_dispatch_by_window_end
        } else {
            pr.reviewer_or_dispatch_by_deadline
        };
        let Some(covered) = covered else {
            return unavailable(
                "github.review_ready_prs_undispatched",
                "reviewer and lead dispatch evidence is incomplete for a ready pull request",
            );
        };
        if !covered {
            count += 1;
            evidence.push(pr_ref(pr));
        }
    }
    value(count as f64, evidence)
}

#[implements("FR-047-AC-1")]
fn reduce_merges_beyond_reviewed(rows: &[PullRequestEvidence], window: &EvalWindow) -> QueryValue {
    let mut count = 0_u64;
    let mut evidence = Vec::new();
    let merged = match merged_in_window(rows, window, "github.merges_beyond_reviewed_change") {
        Ok(merged) => merged,
        Err(error) => return error,
    };
    for pr in merged {
        if !pr.review_events_complete || pr.github_review_heads.is_none() {
            return unavailable(
                "github.merges_beyond_reviewed_change",
                "GitHub review-event history is incomplete",
            );
        }
        let (Some(head), Some(reviewed)) = (&pr.merged_head, &pr.reviewed_heads) else {
            return unavailable(
                "github.merges_beyond_reviewed_change",
                "merged head or complete review coverage is unavailable",
            );
        };
        if !reviewed.contains(head) {
            count += 1;
            evidence.push(pr_ref(pr));
        }
    }
    value(count as f64, evidence)
}

#[implements("FR-047-AC-1")]
fn reduce_merges_without_gate(rows: &[PullRequestEvidence], window: &EvalWindow) -> QueryValue {
    let mut count = 0_u64;
    let mut evidence = Vec::new();
    let merged = match merged_in_window(rows, window, "github.merges_without_covering_gate") {
        Ok(merged) => merged,
        Err(error) => return error,
    };
    for pr in merged {
        let Some(required) = pr.gate_required else {
            return unavailable(
                "github.merges_without_covering_gate",
                "changed-file gate classification is unavailable",
            );
        };
        if !required {
            continue;
        }
        let Some(waived) = pr.waived_for_gate else {
            return unavailable(
                "github.merges_without_covering_gate",
                "LEAD-R11 gate waiver evidence is unavailable",
            );
        };
        if waived {
            continue;
        }
        let (Some(head), Some(covered_heads)) = (&pr.merged_head, &pr.gate_covered_heads) else {
            return unavailable(
                "github.merges_without_covering_gate",
                "merged head or complete gate coverage is unavailable",
            );
        };
        if !covered_heads.contains(head) {
            count += 1;
            evidence.push(pr_ref(pr));
        }
    }
    value(count as f64, evidence)
}

#[implements("FR-047-AC-1")]
fn reduce_ready_over_hour(rows: &[PullRequestEvidence], window: &EvalWindow) -> QueryValue {
    let mut count = 0_u64;
    let mut evidence = Vec::new();
    for pr in rows {
        if !pr.ready_events_complete {
            return unavailable(
                "github.ready_prs_unmerged_over_hour",
                "pull-request ready-for-review event history is incomplete",
            );
        }
        let Some(ready_at) = pr.ready_at_ms else {
            continue;
        };
        if ready_at < window.start_ms || ready_at >= window.end_ms {
            continue;
        }
        if pr
            .closed_at_ms
            .is_some_and(|closed_at| closed_at < window.end_ms)
            && pr.merged_at_ms.is_none()
        {
            continue;
        }
        let Some(waived) = pr.ready_merge_waived else {
            return unavailable(
                "github.ready_prs_unmerged_over_hour",
                "ready time or waiver evidence is incomplete",
            );
        };
        if waived {
            continue;
        }
        let end = pr.merged_at_ms.unwrap_or(window.end_ms);
        if end.saturating_sub(ready_at) > 3_600_000 {
            count += 1;
            evidence.push(pr_ref(pr));
        }
    }
    value(count as f64, evidence)
}

#[implements("FR-047-AC-1")]
fn reduce_ticket_start_median(rows: &[PullRequestEvidence], window: &EvalWindow) -> QueryValue {
    let mut durations = Vec::new();
    let mut evidence = Vec::new();
    for pr in rows {
        let Some(created) = pr.created_at_ms else {
            return unavailable(
                "github.ticket_start_to_pr_open_median_hours",
                "pull-request creation time is unavailable",
            );
        };
        if created < window.start_ms || created >= window.end_ms || pr.ticket.is_none() {
            continue;
        }
        let Some(started) = pr.ticket_started_at_ms else {
            return unavailable(
                "github.ticket_start_to_pr_open_median_hours",
                "ticket started-state history is unavailable for a pull request with a ticket",
            );
        };
        if created < started {
            continue;
        }
        durations.push((created - started) as f64 / 3_600_000.0);
        evidence.push(pr_ref(pr));
    }
    if durations.is_empty() {
        return unavailable(
            "github.ticket_start_to_pr_open_median_hours",
            "no qualifying ticket-linked pull requests exist in the window",
        );
    }
    durations.sort_by(f64::total_cmp);
    let middle = durations.len() / 2;
    let median = if durations.len().is_multiple_of(2) {
        (durations[middle - 1] + durations[middle]) / 2.0
    } else {
        durations[middle]
    };
    value(median, evidence)
}

#[implements("FR-047-AC-1")]
fn merged_in_window<'a>(
    rows: &'a [PullRequestEvidence],
    window: &EvalWindow,
    query: &str,
) -> Result<Vec<&'a PullRequestEvidence>, QueryValue> {
    let mut selected = Vec::new();
    for pr in rows {
        let Some(merged) = pr.merged_at_ms else {
            continue;
        };
        if merged >= window.start_ms && merged < window.end_ms {
            if pr.merged_head.as_deref().is_none_or(str::is_empty) {
                return Err(unavailable(query, "merged head SHA is missing"));
            }
            selected.push(pr);
        }
    }
    Ok(selected)
}

#[implements("FR-047-AC-1")]
fn pr_ref(pr: &PullRequestEvidence) -> String {
    format!("github:{}#{}", pr.repository, pr.number)
}

#[implements("FR-047-AC-1")]
fn value(value: f64, evidence_refs: Vec<String>) -> QueryValue {
    QueryValue::Value {
        value,
        evidence_refs,
    }
}

#[implements("FR-047-AC-1")]
fn unavailable(query: &str, reason: &str) -> QueryValue {
    QueryValue::Unavailable {
        reason: format!("{query}: {reason}"),
        evidence_refs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{PullRequestEvidence, adapters};
    use crate::retro::eval::{
        EvalWindow, EvaluationRule, ObjectiveMetricSpec, QueryValue, RoleSpec,
    };
    use ix_trace_rs::trace;
    use std::collections::{BTreeMap, BTreeSet};

    fn spec(queries: &[&str]) -> RoleSpec {
        RoleSpec {
            role: "code-lead".into(),
            metrics: Vec::<ObjectiveMetricSpec>::new(),
            rules: queries
                .iter()
                .map(|query| EvaluationRule {
                    id: (*query).into(),
                    rule: "fixture".into(),
                    evidence_source: "github".into(),
                    trigger: "fixture".into(),
                    method: "deterministic".into(),
                    query_or_rubric: (*query).into(),
                    comparator: "=".into(),
                    target: 0.0,
                    unit: "pull_requests".into(),
                })
                .collect(),
        }
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn registered_github_reducers_count_only_proven_complete_evidence() {
        let window = EvalWindow {
            start_ms: 100,
            end_ms: 10_000_000,
        };
        let mut pr = PullRequestEvidence {
            repository: "agent-ix/ix-board".into(),
            number: 42,
            url: "https://github.com/agent-ix/ix-board/pull/42".into(),
            created_at_ms: Some(2_000_000),
            ready_at_ms: Some(100),
            ready_events_complete: true,
            review_events_complete: true,
            merged_at_ms: Some(4_000_000),
            closed_at_ms: Some(4_000_000),
            merged_head: Some("merge-head".into()),
            github_review_heads: Some(BTreeSet::new()),
            commit_shas: Some(Vec::new()),
            commit_ancestry: Some(BTreeMap::new()),
            changed_files: Some(BTreeSet::new()),
            ..Default::default()
        };
        pr.reviewed_heads = Some(BTreeSet::from(["reviewed-head".into()]));
        pr.gate_required = Some(true);
        pr.gate_covered_heads = Some(BTreeSet::from(["gate-head".into()]));
        pr.waived_for_gate = Some(false);
        pr.waived_for_review = Some(false);
        pr.reviewer_or_dispatch_by_deadline = Some(false);
        pr.ready_merge_waived = Some(false);
        pr.ticket = Some("SWM-221".into());
        pr.ticket_started_at_ms = Some(1_000_000);
        let queries = [
            "github.review_ready_prs_undispatched",
            "github.merges_beyond_reviewed_change",
            "github.merges_without_covering_gate",
            "github.ready_prs_unmerged_over_hour",
        ];
        let output = adapters(&[spec(&queries)], Ok(&[pr.clone()]), &window, 1);
        for query in queries {
            assert!(matches!(
                output[query],
                QueryValue::Value { value: 1.0, .. }
            ));
        }
        pr.reviewed_heads = None;
        let output = adapters(
            &[spec(&["github.merges_beyond_reviewed_change"])],
            Ok(&[pr]),
            &window,
            1,
        );
        assert!(matches!(
            output["github.merges_beyond_reviewed_change"],
            QueryValue::Unavailable { .. }
        ));
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn ticket_start_metric_uses_median_and_unknown_history_stays_unavailable() {
        let window = EvalWindow {
            start_ms: 0,
            end_ms: 10_000_000,
        };
        let row = |number, created_at_ms, ticket_started_at_ms| PullRequestEvidence {
            repository: "agent-ix/ix-board".into(),
            number,
            created_at_ms: Some(created_at_ms),
            ticket: Some(format!("SWM-{number}")),
            ticket_started_at_ms: Some(ticket_started_at_ms),
            ..Default::default()
        };
        let mut metric_spec = spec(&[]);
        metric_spec.metrics.push(ObjectiveMetricSpec {
            id: "CODE-M01".into(),
            query: "github.ticket_start_to_pr_open_median_hours".into(),
            unit: "hours".into(),
            target: 4.0,
            direction: "lower".into(),
        });
        let output = adapters(
            &[metric_spec.clone()],
            Ok(&[row(1, 7_200_000, 0), row(2, 7_200_000, 3_600_000)]),
            &window,
            1,
        );
        assert!(matches!(
            output["github.ticket_start_to_pr_open_median_hours"],
            QueryValue::Value { value: 1.5, .. }
        ));
        let output = adapters(
            &[metric_spec],
            Ok(&[PullRequestEvidence {
                ticket: Some("SWM-1".into()),
                created_at_ms: Some(1_000),
                ..Default::default()
            }]),
            &window,
            1,
        );
        assert!(matches!(
            output["github.ticket_start_to_pr_open_median_hours"],
            QueryValue::Unavailable { .. }
        ));
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn older_uncovered_ready_pr_is_counted_again_using_window_end_join() {
        let window = EvalWindow {
            start_ms: 5_000_000,
            end_ms: 10_000_000,
        };
        let row = PullRequestEvidence {
            ready_at_ms: Some(100),
            ready_events_complete: true,
            reviewer_or_dispatch_by_window_end: Some(false),
            ..Default::default()
        };
        let output = adapters(
            &[spec(&["github.review_ready_prs_undispatched"])],
            Ok(&[row]),
            &window,
            1,
        );
        assert!(matches!(
            output["github.review_ready_prs_undispatched"],
            QueryValue::Value { value: 1.0, .. }
        ));
    }
}
