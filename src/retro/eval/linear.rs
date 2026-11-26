// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Pure aggregation for Linear review markers used by retro evaluation.

use ix_trace_rs::implements;
use std::collections::{BTreeMap, BTreeSet};

use super::{EvalWindow, QueryValue};
use crate::{MarkerComment, retro::sources::PullRequest};

const REVIEWER_FINDING_FIELDS: &[&str] = &[
    "fnd",
    "severity",
    "confidence",
    "method",
    "check",
    "artifact_id",
    "path",
    "lines",
    "excerpt",
    "finding",
];

/// Parses review and disposition markers into their catalog metrics.
///
/// `comments` must be the complete read-only Linear population for the repositories and event-time
/// span being evaluated. Historical marker payloads outside the requested window are not parsed;
/// this keeps malformed old records from invalidating later windows.
#[implements("FR-047-AC-1")]
pub(super) fn review_marker_metrics(
    comments: &[MarkerComment],
    window: &EvalWindow,
) -> Result<BTreeMap<String, QueryValue>, String> {
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let mut marker_count = 0usize;
    let mut missing_fields = 0usize;
    let mut unparseable = 0usize;
    let mut content_coverage_complete = true;
    let mut rounds = BTreeSet::<(String, u64)>::new();
    let mut findings = BTreeSet::<(String, String, String, String)>::new();
    let mut locationless = BTreeSet::<(String, String, String, String)>::new();
    let mut evidence = BTreeMap::<&str, Vec<String>>::new();
    let mut artifact_prs = BTreeMap::<(String, String), BTreeSet<String>>::new();

    for comment in comments {
        let at = comment_time(comment)?;
        let in_window = at >= start_secs && at < end_secs;
        let first_line = comment.body.lines().next().unwrap_or_default();
        let Some((kind, fields)) = marker_fields(first_line) else {
            if in_window {
                marker_count += 1;
                missing_fields += 1;
                unparseable += 1;
                content_coverage_complete = false;
                add_evidence(&mut evidence, "linear.review_markers", comment)?;
                add_evidence(
                    &mut evidence,
                    "linear.review_markers_missing_fields",
                    comment,
                )?;
                add_evidence(
                    &mut evidence,
                    "linear.review_comments_unparseable_or_incomplete",
                    comment,
                )?;
            }
            continue;
        };

        // Reviewer identity is indexed across history so an in-window disposition can be joined to
        // its review. Payload validation is intentionally deferred until a row is in this window.
        if kind == "reviewer"
            && let (Some(repo), Some(id), Some(pr)) =
                (fields.get("repo"), fields.get("id"), fields.get("pr"))
            && let Ok(pr_key) = normalize_pr_key(repo, pr)
        {
            artifact_prs
                .entry(((*repo).to_owned(), (*id).to_owned()))
                .or_default()
                .insert(pr_key);
        }
        if !in_window {
            continue;
        }

        marker_count += 1;
        add_evidence(&mut evidence, "linear.review_markers", comment)?;
        let required: &[&str] = if kind == "reviewer" {
            &["repo", "id", "method", "pr", "run", "head"]
        } else {
            &["repo", "id", "round", "run", "head", "mergeable"]
        };
        let fields_complete = required.iter().all(|field| {
            fields
                .get(field)
                .is_some_and(|value| !value.trim().is_empty())
        });
        if !fields_complete {
            missing_fields += 1;
            add_evidence(
                &mut evidence,
                "linear.review_markers_missing_fields",
                comment,
            )?;
        }

        let data = parse_yaml_data_block(&comment.body);
        let data_complete = data
            .as_ref()
            .is_ok_and(|value| marker_data_complete(kind, value));
        if !fields_complete || !data_complete {
            unparseable += 1;
            content_coverage_complete = false;
            add_evidence(
                &mut evidence,
                "linear.review_comments_unparseable_or_incomplete",
                comment,
            )?;
        }
        let Some(data) = data.ok().filter(|_| data_complete) else {
            continue;
        };
        if !fields_complete {
            continue;
        }
        let (Some(repo), Some(id)) = (fields.get("repo"), fields.get("id")) else {
            continue;
        };

        if kind == "reviewer" {
            let pr = normalize_pr_key(
                repo,
                fields
                    .get("pr")
                    .ok_or_else(|| "reviewer marker has no PR".to_owned())?,
            )?;
            collect_findings(
                &data,
                repo,
                &pr,
                id,
                comment,
                &mut findings,
                &mut locationless,
                &mut evidence,
            )?;
        } else {
            let Some(prs) = artifact_prs.get(&((*repo).to_owned(), (*id).to_owned())) else {
                return Err(format!(
                    "Linear disposition marker {id} has no matching reviewer marker"
                ));
            };
            if prs.len() != 1 {
                return Err(format!(
                    "Linear disposition marker {id} matches multiple pull requests"
                ));
            }
            let pr = prs
                .first()
                .ok_or_else(|| "Linear disposition has no pull request attribution".to_owned())?;
            let round = fields
                .get("round")
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| "Linear disposition round is invalid".to_owned())?;
            rounds.insert((pr.clone(), round));
            add_evidence(&mut evidence, "linear.disposition_rounds", comment)?;
            collect_findings(
                &data,
                repo,
                pr,
                id,
                comment,
                &mut findings,
                &mut locationless,
                &mut evidence,
            )?;
        }
    }

    for refs in evidence.values_mut() {
        refs.sort();
        refs.dedup();
    }
    let metric = |query: &str, count: usize| QueryValue::Value {
        value: count as f64,
        evidence_refs: evidence.get(query).cloned().unwrap_or_default(),
    };
    let content_metric = |query: &str, count: usize| {
        if content_coverage_complete {
            metric(query, count)
        } else {
            QueryValue::Unavailable {
                reason: "Linear review marker payload coverage is incomplete".into(),
                evidence_refs: evidence.get(query).cloned().unwrap_or_default(),
            }
        }
    };
    Ok(BTreeMap::from([
        (
            "linear.review_markers".into(),
            metric("linear.review_markers", marker_count),
        ),
        (
            "linear.review_markers_missing_fields".into(),
            metric("linear.review_markers_missing_fields", missing_fields),
        ),
        (
            "linear.disposition_rounds".into(),
            content_metric("linear.disposition_rounds", rounds.len()),
        ),
        (
            "linear.review_findings".into(),
            content_metric("linear.review_findings", findings.len()),
        ),
        (
            "linear.findings_without_location_or_scenario".into(),
            content_metric(
                "linear.findings_without_location_or_scenario",
                locationless.len(),
            ),
        ),
        (
            "linear.review_comments_unparseable_or_incomplete".into(),
            metric(
                "linear.review_comments_unparseable_or_incomplete",
                unparseable,
            ),
        ),
    ]))
}

/// Counts PRs reviewed in the window plus PRs merged in the window.
///
/// A marker after a PR's merge cannot count as a review of that PR. Review markers are counted
/// only when their native creation time falls in the window; merges use native merge time.
#[implements("FR-047-AC-1")]
pub(super) fn reviewed_prs_count(
    prs: &[PullRequest],
    comments: &[MarkerComment],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let mut reviewed = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    for comment in comments {
        let at = comment_time(comment)?;
        if at < start_secs || at >= end_secs {
            continue;
        }
        let (_repo, pr, _) = reviewer_marker(comment)?;
        if merged_at(prs, &pr).is_some_and(|merged| at > merged) {
            continue;
        }
        if reviewed.insert(pr) {
            evidence.insert(comment_ref(comment)?);
        }
    }
    for pr in prs {
        if pr
            .merged_at
            .is_some_and(|at| at >= start_secs && at < end_secs)
        {
            let key = format!("{}#{}", pr.repo, pr.number);
            if reviewed.insert(key.clone()) {
                evidence.insert(format!("github-pr:{key}:merged"));
            }
        }
    }
    Ok((reviewed.len(), evidence.into_iter().collect()))
}

/// Counts `(PR, method)` pairs with more than one distinct review run in the selected window.
#[implements("FR-047-AC-1")]
pub(super) fn pr_methods_with_multiple_runs(
    prs: &[PullRequest],
    comments: &[MarkerComment],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let mut methods = BTreeMap::<(String, String), BTreeSet<String>>::new();
    let mut refs_by_method = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for comment in comments {
        let at = comment_time(comment)?;
        if at < start_secs || at >= end_secs {
            continue;
        }
        let (_repo, pr, fields) = reviewer_marker(comment)?;
        if merged_at(prs, &pr).is_some_and(|merged| at > merged) {
            continue;
        }
        let method = fields
            .get("method")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "reviewer marker has no method".to_owned())?;
        let run = fields
            .get("run")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "reviewer marker has no run".to_owned())?;
        let key = (pr, (*method).to_owned());
        methods
            .entry(key.clone())
            .or_default()
            .insert((*run).to_owned());
        refs_by_method
            .entry(key)
            .or_default()
            .insert(comment_ref(comment)?);
    }
    let counted = methods
        .iter()
        .filter(|(_, runs)| runs.len() > 1)
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>();
    let evidence = counted
        .iter()
        .flat_map(|key| refs_by_method.get(*key).into_iter().flatten().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((counted.len(), evidence))
}

/// Counts merged pull requests in the window without a reviewer marker at or before merge time.
#[implements("FR-047-AC-1")]
pub(super) fn merged_prs_without_review_count(
    prs: &[PullRequest],
    comments: &[MarkerComment],
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let start_secs = window.start_ms as f64 / 1000.0;
    let end_secs = window.end_ms as f64 / 1000.0;
    let mut reviewed_by_merge = BTreeSet::new();
    for comment in comments {
        let at = comment_time(comment)?;
        let (_repo, pr, _) = reviewer_marker(comment)?;
        if merged_at(prs, &pr).is_some_and(|merged| at <= merged) {
            reviewed_by_merge.insert(pr);
        }
    }
    let missing = prs
        .iter()
        .filter(|pr| {
            pr.merged_at
                .is_some_and(|at| at >= start_secs && at < end_secs)
        })
        .map(|pr| format!("{}#{}", pr.repo, pr.number))
        .filter(|key| !reviewed_by_merge.contains(key))
        .collect::<BTreeSet<_>>();
    let evidence = missing
        .iter()
        .map(|pr| format!("github-pr:{pr}:merged-without-review-marker"))
        .collect();
    Ok((missing.len(), evidence))
}

fn collect_findings(
    data: &serde_json::Value,
    repo: &str,
    pr: &str,
    artifact: &str,
    comment: &MarkerComment,
    findings: &mut BTreeSet<(String, String, String, String)>,
    locationless: &mut BTreeSet<(String, String, String, String)>,
    evidence: &mut BTreeMap<&'static str, Vec<String>>,
) -> Result<(), String> {
    for finding in data
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            data.get("new_findings")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten(),
        )
    {
        let Some(id) = finding.get("fnd").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if id.trim().is_empty() {
            continue;
        }
        let key = (
            repo.to_owned(),
            pr.to_owned(),
            artifact.to_owned(),
            id.to_owned(),
        );
        findings.insert(key.clone());
        add_evidence(evidence, "linear.review_findings", comment)?;
        let path = finding
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let lines = finding
            .get("lines")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let scenario = finding
            .get("finding")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if (path.trim().is_empty() && lines.trim().is_empty()) || scenario.trim().is_empty() {
            locationless.insert(key);
            add_evidence(
                evidence,
                "linear.findings_without_location_or_scenario",
                comment,
            )?;
        }
    }
    Ok(())
}

fn marker_data_complete(kind: &str, data: &serde_json::Value) -> bool {
    let has_fields = |row: &serde_json::Value, fields: &[&str]| {
        fields.iter().all(|field| row.get(*field).is_some())
    };
    match kind {
        "reviewer" => {
            let Some(clean) = data.get("clean").and_then(serde_json::Value::as_bool) else {
                return false;
            };
            let Some(findings) = data.get("findings").and_then(serde_json::Value::as_array) else {
                return false;
            };
            reviewer_scope_complete(data.get("scope"))
                && findings
                    .iter()
                    .all(|row| has_fields(row, REVIEWER_FINDING_FIELDS))
                && (clean == findings.is_empty())
        }
        "reviewer-dispositions" => {
            let dispositions_complete = data
                .get("dispositions")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|rows| {
                    rows.iter().all(|row| {
                        let Some(outcome) = row.get("outcome").and_then(serde_json::Value::as_str)
                        else {
                            return false;
                        };
                        has_fields(row, &["fnd", "outcome"])
                            && match outcome {
                                "fixed" => row
                                    .get("after_excerpt")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(|value| !value.trim().is_empty()),
                                "rejected" | "deferred" | "accepted-no-change" | "still-open" => {
                                    row.get("reason")
                                        .and_then(serde_json::Value::as_str)
                                        .is_some_and(|value| !value.trim().is_empty())
                                }
                                _ => false,
                            }
                    })
                });
            let new_findings_complete = data.get("new_findings").is_none_or(|rows| {
                rows.as_array().is_some_and(|rows| {
                    rows.iter()
                        .all(|row| has_fields(row, REVIEWER_FINDING_FIELDS))
                })
            });
            dispositions_complete && new_findings_complete
        }
        _ => false,
    }
}

fn reviewer_scope_complete(scope: Option<&serde_json::Value>) -> bool {
    let Some(scope) = scope else {
        return false;
    };
    scope.as_array().is_some_and(|rows| {
        !rows.is_empty()
            && rows.iter().all(|row| {
                ["id", "path", "role", "excerpt"].iter().all(|field| {
                    row.get(*field)
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
            })
    })
}

fn parse_yaml_data_block(body: &str) -> Result<serde_json::Value, String> {
    let mut active = false;
    let mut width = 0usize;
    let mut block = String::new();
    let mut decoded = Vec::new();
    for line in body.lines() {
        let text = line.trim_start_matches(' ');
        let indent = line.len() - text.len();
        if indent <= 3 && text.starts_with("```") {
            let ticks = text.bytes().take_while(|byte| *byte == b'`').count();
            let tag = text.get(ticks..).unwrap_or("").trim();
            if !active && matches!(tag, "yaml" | "yml") {
                active = true;
                width = ticks;
                block.clear();
                continue;
            }
            if active && ticks >= width && tag.is_empty() {
                if let Ok(parsed) = serde_yaml::from_str::<serde_json::Value>(&block) {
                    decoded.push(parsed);
                }
                active = false;
                continue;
            }
        }
        if active {
            block.push_str(line);
            block.push('\n');
        }
    }
    if decoded.len() != 1 {
        return Err("review marker must contain exactly one supported YAML block".into());
    }
    decoded
        .pop()
        .ok_or_else(|| "review marker has no structured data".into())
}

fn marker_fields(line: &str) -> Option<(&str, BTreeMap<&str, &str>)> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .trim_end()
        .strip_suffix("-->")?;
    let mut tokens = inner.split_whitespace();
    let kind = tokens.next()?;
    if !matches!(kind, "reviewer" | "reviewer-dispositions") {
        return None;
    }
    Some((
        kind,
        tokens.filter_map(|token| token.split_once('=')).collect(),
    ))
}

fn reviewer_marker(
    comment: &MarkerComment,
) -> Result<(String, String, BTreeMap<&str, &str>), String> {
    let first_line = comment.body.lines().next().unwrap_or_default();
    let Some(("reviewer", fields)) = marker_fields(first_line) else {
        return Err("Linear returned an unsupported reviewer marker".into());
    };
    let repo = fields
        .get("repo")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "reviewer marker has no repo".to_owned())?;
    let pr = fields
        .get("pr")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "reviewer marker has no PR".to_owned())?;
    Ok(((*repo).to_owned(), normalize_pr_key(repo, pr)?, fields))
}

pub(super) fn normalize_pr_key(repo: &str, pr: &str) -> Result<String, String> {
    let full_prefix = format!("{repo}#");
    let short_repo = repo.rsplit('/').next().unwrap_or(repo);
    let short_prefix = format!("{short_repo}#");
    let number = pr
        .strip_prefix(&full_prefix)
        .or_else(|| pr.strip_prefix(&short_prefix))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|number| *number > 0)
        .ok_or_else(|| "reviewer marker PR does not match its repo".to_owned())?;
    Ok(format!("{repo}#{number}"))
}

fn merged_at(prs: &[PullRequest], key: &str) -> Option<f64> {
    let (repo, number) = key.rsplit_once('#')?;
    let number = number.parse::<u64>().ok()?;
    prs.iter()
        .find(|pr| {
            pr.number == number && (pr.repo == repo || pr.repo.rsplit('/').next() == Some(repo))
        })
        .and_then(|pr| pr.merged_at)
}

/// Reduces the class-A reviewer findings queries from complete Linear marker history.
///
/// `changed_files` is required for `outside_diff_findings_without_failure`; callers must pass
/// `None` when GitHub file coverage was unavailable instead of treating that query as zero.
#[implements("FR-047-AC-1")]
pub(super) fn reviewer_class_a_metrics(
    comments: &[MarkerComment],
    prs: &[PullRequest],
    window: &EvalWindow,
    changed_files: Option<&BTreeMap<String, BTreeSet<String>>>,
    final_fix_records: &[MarkerComment],
) -> BTreeMap<String, QueryValue> {
    let queries = [
        "linear.findings_open_at_merge",
        "linear.fix_round_findings_returned_still_open",
        "linear.findings_without_outcome_after_disposition",
        "linear.mergeable_yes_with_open_finding",
        "linear.outside_diff_findings_without_failure",
        "linear.review_to_clean_median_hours",
    ];
    let unavailable = |reason: &str| QueryValue::Unavailable {
        reason: reason.to_owned(),
        evidence_refs: Vec::new(),
    };

    let mut artifact_prs = BTreeMap::<(String, String), BTreeSet<String>>::new();
    for comment in comments {
        let first_line = comment.body.lines().next().unwrap_or_default();
        let Some(("reviewer", fields)) = marker_fields(first_line) else {
            continue;
        };
        if let (Some(repo), Some(id), Some(pr)) =
            (fields.get("repo"), fields.get("id"), fields.get("pr"))
            && let Ok(pr) = normalize_pr_key(repo, pr)
        {
            artifact_prs
                .entry(((*repo).to_owned(), (*id).to_owned()))
                .or_default()
                .insert(pr);
        }
    }
    let mut parsed = Vec::with_capacity(comments.len());
    for comment in comments {
        let first_line = comment.body.lines().next().unwrap_or_default();
        if (first_line.starts_with("<!-- reviewer")
            || first_line.starts_with("<!-- reviewer-dispositions"))
            && marker_fields(first_line).is_none()
        {
            return queries
                .into_iter()
                .map(|query| {
                    (
                        query.to_owned(),
                        unavailable("Linear review marker population contains a malformed marker"),
                    )
                })
                .collect();
        }
        let Some((kind, fields)) = marker_fields(first_line) else {
            continue;
        };
        let at = match comment_time(comment) {
            Ok(at) => at,
            Err(reason) => {
                return queries
                    .into_iter()
                    .map(|query| (query.to_owned(), unavailable(&reason)))
                    .collect();
            }
        };
        let data = match parse_yaml_data_block(&comment.body) {
            Ok(data) if marker_data_complete(kind, &data) => data,
            _ => {
                return queries
                    .into_iter()
                    .map(|query| {
                        (
                            query.to_owned(),
                            unavailable("Linear review marker population is incomplete"),
                        )
                    })
                    .collect();
            }
        };
        let pr_key = match kind {
            "reviewer" | "reviewer-dispositions" => {
                let Some(repo) = fields.get("repo").copied() else {
                    return queries
                        .into_iter()
                        .map(|query| {
                            (
                                query.to_owned(),
                                unavailable("Linear review marker has no repository"),
                            )
                        })
                        .collect();
                };
                let pr = match fields.get("pr").copied() {
                    Some(pr) => normalize_pr_key(repo, pr),
                    None if kind == "reviewer-dispositions" => {
                        let Some(id) = fields.get("id") else {
                            return queries
                                .into_iter()
                                .map(|query| {
                                    (
                                        query.to_owned(),
                                        unavailable("Linear disposition marker has no id"),
                                    )
                                })
                                .collect();
                        };
                        match artifact_prs.get(&(repo.to_owned(), (*id).to_owned())) {
                            Some(prs) if prs.len() == 1 => prs.iter().next().cloned().ok_or_else(|| "Linear disposition PR binding is empty".to_owned()),
                            _ => Err("Linear disposition marker cannot be bound to exactly one reviewer PR".to_owned()),
                        }
                    }
                    None => Err("Linear reviewer marker has no pull request".to_owned()),
                };
                match pr {
                    Ok(key) => key,
                    Err(reason) => {
                        return queries
                            .into_iter()
                            .map(|query| (query.to_owned(), unavailable(&reason)))
                            .collect();
                    }
                }
            }
            _ => continue,
        };
        let evidence_ref = match comment_ref(comment) {
            Ok(reference) => reference,
            Err(reason) => {
                return queries
                    .into_iter()
                    .map(|query| (query.to_owned(), unavailable(&reason)))
                    .collect();
            }
        };
        parsed.push(ReviewRecord {
            kind,
            pr: pr_key,
            at,
            fields,
            data,
            evidence_ref,
        });
    }

    let start = window.start_ms as f64 / 1000.0;
    let end = window.end_ms as f64 / 1000.0;
    let mut final_fixes = Vec::<(String, String, String, f64, String)>::new();
    for comment in final_fix_records {
        let first_line = comment.body.lines().next().unwrap_or_default();
        if first_line.starts_with("<!-- lead-record") && lead_record_fields(first_line).is_none() {
            return queries
                .into_iter()
                .map(|query| {
                    (
                        query.to_owned(),
                        unavailable("Linear lead-record marker is malformed"),
                    )
                })
                .collect();
        }
        let Some(fields) = lead_record_fields(first_line) else {
            continue;
        };
        if fields.get("kind") != Some(&"final-fix") {
            continue;
        }
        let Some(repo) = fields.get("repo") else {
            return queries
                .into_iter()
                .map(|query| {
                    (
                        query.to_owned(),
                        unavailable("final-fix record has no repository"),
                    )
                })
                .collect();
        };
        let Some(pr) = fields.get("pr") else {
            return queries
                .into_iter()
                .map(|query| {
                    (
                        query.to_owned(),
                        unavailable("final-fix record has no pull request"),
                    )
                })
                .collect();
        };
        let pr = match normalize_pr_key(repo, pr) {
            Ok(pr) => pr,
            Err(reason) => {
                return queries
                    .into_iter()
                    .map(|query| (query.to_owned(), unavailable(&reason)))
                    .collect();
            }
        };
        let Some(findings) = fields.get("fnd") else {
            return queries
                .into_iter()
                .map(|query| {
                    (
                        query.to_owned(),
                        unavailable("final-fix record has no closed findings"),
                    )
                })
                .collect();
        };
        let at = match comment_time(comment) {
            Ok(at) => at,
            Err(reason) => {
                return queries
                    .into_iter()
                    .map(|query| (query.to_owned(), unavailable(&reason)))
                    .collect();
            }
        };
        let evidence_ref = match comment_ref(comment) {
            Ok(reference) => reference,
            Err(reason) => {
                return queries
                    .into_iter()
                    .map(|query| (query.to_owned(), unavailable(&reason)))
                    .collect();
            }
        };
        for finding in findings
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let Some((artifact, finding)) = finding.split_once('/') else {
                return queries
                    .into_iter()
                    .map(|query| {
                        (
                            query.to_owned(),
                            unavailable("final-fix finding identity is malformed"),
                        )
                    })
                    .collect();
            };
            final_fixes.push((
                pr.clone(),
                artifact.to_owned(),
                finding.to_owned(),
                at,
                evidence_ref.clone(),
            ));
        }
    }
    let mut open_at_merge = BTreeSet::<(String, String)>::new();
    let mut open_merge_refs = BTreeSet::new();
    for pr in prs
        .iter()
        .filter(|pr| pr.merged_at.is_some_and(|at| at >= start && at < end))
    {
        let key = format!("{}#{}", pr.repo, pr.number);
        let Some(merge_at) = pr.merged_at else {
            continue;
        };
        let mut findings = BTreeMap::<(String, String), f64>::new();
        let mut outcomes = BTreeMap::<(String, String), (u64, f64, String)>::new();
        for record in parsed
            .iter()
            .filter(|record| record.pr == key && record.at <= merge_at)
        {
            match record.kind {
                "reviewer" => {
                    let Some(id) = record.fields.get("id") else {
                        continue;
                    };
                    for finding in record
                        .data
                        .get("findings")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(fnd) = finding.get("fnd").and_then(serde_json::Value::as_str) {
                            findings
                                .entry(((*id).to_owned(), fnd.to_owned()))
                                .or_insert(record.at);
                        }
                    }
                }
                "reviewer-dispositions" => {
                    let Some(id) = record.fields.get("id") else {
                        continue;
                    };
                    for finding in record
                        .data
                        .get("new_findings")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(fnd) = finding.get("fnd").and_then(serde_json::Value::as_str) {
                            findings
                                .entry(((*id).to_owned(), fnd.to_owned()))
                                .or_insert(record.at);
                        }
                    }
                    let Some(round) = record
                        .fields
                        .get("round")
                        .and_then(|round| round.parse::<u64>().ok())
                    else {
                        return queries
                            .into_iter()
                            .map(|query| {
                                (
                                    query.to_owned(),
                                    unavailable("reviewer disposition round is invalid"),
                                )
                            })
                            .collect();
                    };
                    for disposition in record
                        .data
                        .get("dispositions")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let (Some(fnd), Some(outcome)) = (
                            disposition.get("fnd").and_then(serde_json::Value::as_str),
                            disposition
                                .get("outcome")
                                .and_then(serde_json::Value::as_str),
                        ) {
                            let identity = ((*id).to_owned(), fnd.to_owned());
                            if outcomes.get(&identity).is_none_or(|(prior_round, at, _)| {
                                round > *prior_round || (round == *prior_round && record.at > *at)
                            }) {
                                outcomes.insert(identity, (round, record.at, outcome.to_owned()));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for (finding, first_seen) in findings {
            let disposition_closed = outcomes.get(&finding).is_some_and(|(_, at, outcome)| {
                *at >= first_seen && *at <= merge_at && outcome != "still-open"
            });
            let final_fixed = final_fixes.iter().any(|(fix_pr, artifact, fnd, at, _)| {
                fix_pr == &key && artifact == &finding.0 && fnd == &finding.1 && *at <= merge_at
            });
            let resolved = disposition_closed || final_fixed;
            if !resolved {
                open_at_merge.insert((key.clone(), finding.1));
                open_merge_refs.extend(
                    parsed
                        .iter()
                        .filter(|record| record.pr == key && record.at <= merge_at)
                        .map(|record| record.evidence_ref.clone()),
                );
            }
        }
    }

    let mut round_refs = BTreeSet::new();
    let mut missing_outcomes = BTreeSet::<(String, String, String)>::new();
    let mut mergeable_open = BTreeSet::<(String, String, String)>::new();
    let mut returned_open = BTreeSet::<(String, String, String)>::new();
    let mut review_to_clean = Vec::<(f64, String, Vec<String>)>::new();
    let mut reviews_by_pr = BTreeMap::<String, Vec<&ReviewRecord>>::new();
    let mut rounds = BTreeMap::<(String, String, String), Vec<&ReviewRecord>>::new();
    for record in &parsed {
        if record.kind == "reviewer" {
            reviews_by_pr
                .entry(record.pr.clone())
                .or_default()
                .push(record);
        } else if let Some(id) = record.fields.get("id") {
            rounds
                .entry((
                    record.pr.clone(),
                    (*id).to_owned(),
                    record
                        .fields
                        .get("round")
                        .copied()
                        .unwrap_or_default()
                        .to_owned(),
                ))
                .or_default()
                .push(record);
        }
    }
    for ((pr, _artifact_id, round_id), round_records) in &rounds {
        let Some(first) = round_records
            .iter()
            .map(|record| record.at)
            .min_by(f64::total_cmp)
        else {
            continue;
        };
        let in_window = first >= start && first < end;
        if in_window {
            round_refs.extend(
                round_records
                    .iter()
                    .map(|record| record.evidence_ref.clone()),
            );
        }
        let new_findings = round_records
            .iter()
            .flat_map(|record| {
                record
                    .data
                    .get("new_findings")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter_map(|row| row.get("fnd").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let mut earlier = BTreeSet::<String>::new();
        for record in parsed
            .iter()
            .filter(|record| record.pr == *pr && record.at < first)
        {
            let field = if record.kind == "reviewer" {
                "findings"
            } else if record.kind == "reviewer-dispositions" {
                "new_findings"
            } else {
                continue;
            };
            for finding in record
                .data
                .get(field)
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(fnd) = finding.get("fnd").and_then(serde_json::Value::as_str) {
                    earlier.insert(fnd.to_owned());
                }
            }
        }
        let mut latest_outcome = BTreeMap::<String, String>::new();
        for record in round_records {
            for disposition in record
                .data
                .get("dispositions")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                if let (Some(fnd), Some(outcome)) = (
                    disposition.get("fnd").and_then(serde_json::Value::as_str),
                    disposition
                        .get("outcome")
                        .and_then(serde_json::Value::as_str),
                ) {
                    latest_outcome.insert(fnd.to_owned(), outcome.to_owned());
                    if in_window
                        && outcome == "still-open"
                        && !new_findings.contains(fnd)
                        && prs.iter().any(|pr_row| {
                            format!("{}#{}", pr_row.repo, pr_row.number) == *pr
                                && (pr_row.open
                                    || pr_row.merged_at.is_some_and(|at| at >= start && at < end))
                        })
                    {
                        returned_open.insert((pr.clone(), round_id.clone(), fnd.to_owned()));
                    }
                }
            }
        }
        for fnd in earlier {
            let mut prior_outcome: Option<(u64, f64, String)> = None;
            for record in parsed.iter().filter(|record| {
                record.pr == *pr && record.kind == "reviewer-dispositions" && record.at < first
            }) {
                let round = record
                    .fields
                    .get("round")
                    .and_then(|round| round.parse::<u64>().ok())
                    .unwrap_or_default();
                for row in record
                    .data
                    .get("dispositions")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if row.get("fnd").and_then(serde_json::Value::as_str) != Some(fnd.as_str()) {
                        continue;
                    }
                    let outcome = row
                        .get("outcome")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    if prior_outcome
                        .as_ref()
                        .is_none_or(|(prior_round, prior_at, _)| {
                            round > *prior_round || (round == *prior_round && record.at > *prior_at)
                        })
                    {
                        prior_outcome = Some((round, record.at, outcome));
                    }
                }
            }
            let was_open = prior_outcome
                .as_ref()
                .is_none_or(|(_, _, outcome)| outcome == "still-open");
            let state = latest_outcome.get(&fnd).map(String::as_str);
            if !was_open {
                continue;
            }
            if in_window && !latest_outcome.contains_key(&fnd) {
                missing_outcomes.insert((pr.clone(), round_id.clone(), fnd.clone()));
            }
            if state.is_none_or(|outcome| outcome == "still-open")
                && round_records
                    .iter()
                    .any(|record| record.fields.get("mergeable") == Some(&"yes"))
            {
                mergeable_open.insert((pr.clone(), _artifact_id.clone(), round_id.clone()));
            }
        }
        if round_records
            .iter()
            .any(|record| record.fields.get("mergeable") == Some(&"yes"))
        {
            for fnd in &new_findings {
                if latest_outcome
                    .get(fnd)
                    .is_none_or(|outcome| outcome == "still-open")
                {
                    mergeable_open.insert((pr.clone(), _artifact_id.clone(), round_id.clone()));
                }
            }
        }
    }
    let review_prs = parsed
        .iter()
        .map(|record| record.pr.clone())
        .collect::<BTreeSet<_>>();
    for pr in review_prs {
        let mut times = parsed
            .iter()
            .filter(|record| record.pr == pr && record.at < end)
            .map(|record| record.at)
            .chain(
                final_fixes
                    .iter()
                    .filter(|(fix_pr, _, _, at, _)| fix_pr == &pr && *at < end)
                    .map(|(_, _, _, at, _)| *at),
            )
            .collect::<Vec<_>>();
        times.sort_by(f64::total_cmp);
        times.dedup_by(|left, right| left.total_cmp(right).is_eq());
        let mut findings = BTreeSet::<(String, String)>::new();
        let mut outcomes = BTreeMap::<(String, String), (u64, f64, String)>::new();
        let mut final_fixed = BTreeSet::<(String, String)>::new();
        let mut first_reviewer_at = None;
        let mut last_reviewer_at = None;
        let mut clean_start = None;
        let mut was_clean = false;
        let mut clean_evidence = BTreeSet::new();
        for at in times {
            for record in parsed
                .iter()
                .filter(|record| record.pr == pr && record.at == at)
            {
                match record.kind {
                    "reviewer" => {
                        first_reviewer_at.get_or_insert(at);
                        last_reviewer_at = Some(at);
                        if let Some(id) = record.fields.get("id") {
                            for finding in record
                                .data
                                .get("findings")
                                .and_then(serde_json::Value::as_array)
                                .into_iter()
                                .flatten()
                            {
                                if let Some(fnd) =
                                    finding.get("fnd").and_then(serde_json::Value::as_str)
                                {
                                    findings.insert(((*id).to_owned(), fnd.to_owned()));
                                }
                            }
                        }
                        clean_evidence.insert(record.evidence_ref.clone());
                    }
                    "reviewer-dispositions" => {
                        if let Some(id) = record.fields.get("id") {
                            for finding in record
                                .data
                                .get("new_findings")
                                .and_then(serde_json::Value::as_array)
                                .into_iter()
                                .flatten()
                            {
                                if let Some(fnd) =
                                    finding.get("fnd").and_then(serde_json::Value::as_str)
                                {
                                    findings.insert(((*id).to_owned(), fnd.to_owned()));
                                }
                            }
                        }
                        let round = record
                            .fields
                            .get("round")
                            .and_then(|round| round.parse::<u64>().ok())
                            .unwrap_or_default();
                        for disposition in record
                            .data
                            .get("dispositions")
                            .and_then(serde_json::Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            if let (Some(id), Some(fnd), Some(outcome)) = (
                                record.fields.get("id"),
                                disposition.get("fnd").and_then(serde_json::Value::as_str),
                                disposition
                                    .get("outcome")
                                    .and_then(serde_json::Value::as_str),
                            ) {
                                let identity = ((*id).to_owned(), fnd.to_owned());
                                if outcomes.get(&identity).is_none_or(
                                    |(prior_round, prior_at, _)| {
                                        round > *prior_round
                                            || (round == *prior_round && at > *prior_at)
                                    },
                                ) {
                                    outcomes.insert(identity, (round, at, outcome.to_owned()));
                                }
                            }
                        }
                        clean_evidence.insert(record.evidence_ref.clone());
                    }
                    _ => {}
                }
            }
            for (fix_pr, artifact, fnd, fix_at, evidence_ref) in &final_fixes {
                if fix_pr == &pr && *fix_at == at {
                    final_fixed.insert((artifact.clone(), fnd.clone()));
                    clean_evidence.insert(evidence_ref.clone());
                }
            }
            let clean = last_reviewer_at.is_some()
                && findings.iter().all(|finding| {
                    final_fixed.contains(finding)
                        || outcomes
                            .get(finding)
                            .is_some_and(|(_, _, outcome)| outcome != "still-open")
                });
            if clean && !was_clean {
                clean_start = Some(at);
            }
            if !clean {
                clean_start = None;
            }
            was_clean = clean;
        }
        if was_clean
            && let (Some(first_review), Some(last_review), Some(clean_start)) =
                (first_reviewer_at, last_reviewer_at, clean_start)
        {
            let clean_at = last_review.max(clean_start);
            if clean_at >= start && clean_at < end {
                review_to_clean.push((
                    clean_at - first_review,
                    pr,
                    clean_evidence.into_iter().collect(),
                ));
            }
        }
    }
    let mut outside_diff = BTreeSet::<(String, String)>::new();
    let mut outside_refs = BTreeSet::new();
    let mut outside_unavailable = None;
    if let Some(files_by_pr) = changed_files {
        for record in &parsed {
            if record.kind != "reviewer" || record.at < start || record.at >= end {
                continue;
            }
            let Some(files) = files_by_pr.get(&record.pr) else {
                outside_unavailable = Some(format!(
                    "GitHub changed-file coverage is incomplete for {}",
                    record.pr
                ));
                continue;
            };
            for finding in record
                .data
                .get("findings")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(fnd) = finding.get("fnd").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let path = finding
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let failure = finding
                    .get("outside_diff_failure")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                if !path.is_empty() && !files.contains(path) && !failure {
                    outside_diff.insert((record.pr.clone(), fnd.to_owned()));
                    outside_refs.insert(record.evidence_ref.clone());
                }
            }
        }
    } else {
        outside_unavailable = Some("GitHub changed-file source is unavailable".to_owned());
    }
    let median = median(
        review_to_clean
            .iter()
            .map(|(duration, _, _)| *duration / 3600.0)
            .collect(),
    );
    let value = |count: usize, refs: BTreeSet<String>| QueryValue::Value {
        value: count as f64,
        evidence_refs: refs.into_iter().collect(),
    };
    let metric_value = match median {
        Some(value) => QueryValue::Value {
            value,
            evidence_refs: review_to_clean
                .iter()
                .flat_map(|(_, _, references)| references.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        },
        None => QueryValue::Unavailable {
            reason: "no complete clean review event falls in the window".to_owned(),
            evidence_refs: Vec::new(),
        },
    };
    BTreeMap::from([
        (
            "linear.findings_open_at_merge".into(),
            value(open_at_merge.len(), open_merge_refs),
        ),
        (
            "linear.fix_round_findings_returned_still_open".into(),
            value(returned_open.len(), round_refs.clone()),
        ),
        (
            "linear.findings_without_outcome_after_disposition".into(),
            value(missing_outcomes.len(), round_refs.clone()),
        ),
        (
            "linear.mergeable_yes_with_open_finding".into(),
            value(mergeable_open.len(), round_refs),
        ),
        (
            "linear.outside_diff_findings_without_failure".into(),
            outside_unavailable.map_or_else(
                || value(outside_diff.len(), outside_refs),
                |reason| QueryValue::Unavailable {
                    reason,
                    evidence_refs: Vec::new(),
                },
            ),
        ),
        ("linear.review_to_clean_median_hours".into(), metric_value),
    ])
}

struct ReviewRecord<'a> {
    kind: &'a str,
    pr: String,
    at: f64,
    fields: BTreeMap<&'a str, &'a str>,
    data: serde_json::Value,
    evidence_ref: String,
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

/// One complete `lead-record kind=gate` comment with native times.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct GateRecord {
    pub pr: String,
    pub head: String,
    pub exit: i32,
    pub started_at: f64,
    pub created_at: f64,
    pub evidence_ref: String,
}

/// Parses complete gate records, preserving source comment evidence and rejecting malformed rows.
#[implements("FR-047-AC-1")]
pub(super) fn gate_records(comments: &[MarkerComment]) -> Result<Vec<GateRecord>, String> {
    let mut records = Vec::new();
    for comment in comments {
        let first_line = comment.body.lines().next().unwrap_or_default();
        if first_line.starts_with("<!-- lead-record") && lead_record_fields(first_line).is_none() {
            return Err("Linear lead-record marker is malformed".to_owned());
        }
        let Some(fields) = lead_record_fields(first_line) else {
            continue;
        };
        if fields.get("kind") != Some(&"gate") {
            continue;
        }
        let repo = fields.get("repo").ok_or("gate record has no repo")?;
        let pr = fields.get("pr").ok_or("gate record has no PR")?;
        let pr = normalize_pr_key(repo, pr)?;
        let head = fields
            .get("head")
            .filter(|value| !value.trim().is_empty())
            .ok_or("gate record has no head")?;
        let exit = fields
            .get("exit")
            .and_then(|value| value.parse::<i32>().ok())
            .ok_or("gate record has no valid exit status")?;
        let started_at = crate::drift::timestamp_epoch(
            fields
                .get("started")
                .ok_or("gate record has no started time")?,
        )
        .map_err(|error| format!("gate record started time is invalid: {error}"))?;
        let created_at = comment_time(comment)?;
        records.push(GateRecord {
            pr,
            head: (*head).to_owned(),
            exit,
            started_at,
            created_at,
            evidence_ref: comment_ref(comment)?,
        });
    }
    Ok(records)
}

/// Counts gate records started before the PR had a clean review, omitting rule-specific waivers.
#[implements("FR-047-AC-1")]
pub(super) fn gate_runs_before_clean_review(
    records: &[GateRecord],
    reviewer_comments: &[MarkerComment],
    waived_prs: &BTreeSet<String>,
    window: &EvalWindow,
) -> Result<(usize, Vec<String>), String> {
    let start = window.start_ms as f64 / 1000.0;
    let end = window.end_ms as f64 / 1000.0;
    let mut clean_at = BTreeMap::<String, f64>::new();
    for comment in reviewer_comments {
        let Some(("reviewer", fields)) =
            marker_fields(comment.body.lines().next().unwrap_or_default())
        else {
            continue;
        };
        let Some(repo) = fields.get("repo") else {
            continue;
        };
        let Some(pr) = fields.get("pr") else { continue };
        let pr = normalize_pr_key(repo, pr)?;
        let data = parse_yaml_data_block(&comment.body)?;
        if !marker_data_complete("reviewer", &data) {
            return Err("Linear reviewer marker population is incomplete".to_owned());
        }
        if data.get("clean").and_then(serde_json::Value::as_bool) == Some(true) {
            let at = comment_time(comment)?;
            clean_at
                .entry(pr)
                .and_modify(|first| *first = first.min(at))
                .or_insert(at);
        }
    }
    let mut evidence = BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.created_at >= start && record.created_at < end)
    {
        if waived_prs.contains(&record.pr) {
            continue;
        }
        if clean_at
            .get(&record.pr)
            .is_none_or(|clean| *clean > record.started_at)
        {
            evidence.insert(record.evidence_ref.clone());
        }
    }
    Ok((evidence.len(), evidence.into_iter().collect()))
}

/// Counts successful prior-head coverage for gate records when GitHub ancestry is complete.
#[implements("FR-047-AC-1")]
pub(super) fn redundant_gate_runs(
    records: &[GateRecord],
    covered_head_pairs: &BTreeSet<(String, String, String)>,
    waived_prs: &BTreeSet<String>,
    window: &EvalWindow,
) -> (usize, Vec<String>) {
    let start = window.start_ms as f64 / 1000.0;
    let end = window.end_ms as f64 / 1000.0;
    let mut evidence = BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.created_at >= start && record.created_at < end)
    {
        if waived_prs.contains(&record.pr) {
            continue;
        }
        let covered = records.iter().any(|earlier| {
            earlier.pr == record.pr
                && earlier.exit == 0
                && earlier.started_at < record.started_at
                && covered_head_pairs.contains(&(
                    record.pr.clone(),
                    earlier.head.clone(),
                    record.head.clone(),
                ))
        });
        if covered {
            evidence.insert(record.evidence_ref.clone());
        }
    }
    (evidence.len(), evidence.into_iter().collect())
}

/// Builds Linear-derived fields for the GitHub PR reducers.
///
/// Unknown fields stay `None`; the caller must provide complete marker populations before using
/// the returned joins. Transcript dispatch and ticket-start state are outside Linear's evidence.
#[implements("FR-047-AC-1")]
pub(super) fn pull_request_joins(
    comments: &[MarkerComment],
    coverage_complete: bool,
    github_rows: &[super::github_rules::PullRequestEvidence],
    review_age_limit_hours: u64,
    window: &EvalWindow,
) -> Result<BTreeMap<(String, u64), super::github_rules::PullRequestJoins>, String> {
    use super::github_rules::PullRequestJoins;

    let mut joins = BTreeMap::<(String, u64), PullRequestJoins>::new();
    let mut reviewed_heads = BTreeMap::<(String, u64), BTreeSet<String>>::new();
    let mut reviewer_times = BTreeMap::<(String, u64), Vec<f64>>::new();
    let mut gate_heads = BTreeMap::<(String, u64), BTreeSet<String>>::new();
    let mut waiver_rules = BTreeMap::<(String, u64), BTreeSet<String>>::new();
    for comment in comments {
        let first_line = comment.body.lines().next().unwrap_or_default();
        if (first_line.starts_with("<!-- reviewer") && marker_fields(first_line).is_none())
            || (first_line.starts_with("<!-- lead-record")
                && lead_record_fields(first_line).is_none())
        {
            return Err("Linear marker population contains a malformed marker".to_owned());
        }
        if let Some(("reviewer", fields)) = marker_fields(first_line) {
            let (repo, pr, _) = reviewer_marker(comment)?;
            let number = pr
                .rsplit_once('#')
                .and_then(|(_, number)| number.parse::<u64>().ok())
                .ok_or("reviewer marker PR is invalid")?;
            let head = fields
                .get("head")
                .filter(|value| !value.trim().is_empty())
                .ok_or("reviewer marker has no head")?;
            let data = parse_yaml_data_block(&comment.body)?;
            if !marker_data_complete("reviewer", &data) {
                return Err("Linear reviewer marker population is incomplete".to_owned());
            }
            reviewed_heads
                .entry((repo.clone(), number))
                .or_default()
                .insert((*head).to_owned());
            reviewer_times
                .entry((repo.to_owned(), number))
                .or_default()
                .push(comment_time(comment)?);
            continue;
        }
        let Some(fields) = lead_record_fields(first_line) else {
            continue;
        };
        let Some(repo) = fields.get("repo").copied() else {
            continue;
        };
        let Some(pr_value) = fields.get("pr").copied() else {
            continue;
        };
        let key = normalize_pr_key(repo, pr_value)?;
        let number = key
            .rsplit_once('#')
            .and_then(|(_, number)| number.parse::<u64>().ok())
            .ok_or("lead record PR is invalid")?;
        match fields.get("kind").copied() {
            Some("gate") => {
                let head = fields
                    .get("head")
                    .filter(|head| !head.trim().is_empty())
                    .ok_or("gate record has no head")?;
                let exit = fields
                    .get("exit")
                    .and_then(|value| value.parse::<i32>().ok())
                    .ok_or("gate record has no valid exit status")?;
                if exit == 0 {
                    gate_heads
                        .entry((repo.to_owned(), number))
                        .or_default()
                        .insert((*head).to_owned());
                }
            }
            Some("rebase") | Some("final-fix") => {
                let head = fields
                    .get("head")
                    .filter(|head| !head.trim().is_empty())
                    .ok_or("review coverage record has no head")?;
                let successful_rebase = fields.get("kind") != Some(&"rebase")
                    || fields
                        .get("exit")
                        .and_then(|value| value.parse::<i32>().ok())
                        == Some(0);
                if successful_rebase {
                    reviewed_heads
                        .entry((repo.to_owned(), number))
                        .or_default()
                        .insert((*head).to_owned());
                }
            }
            Some("waiver") => {
                let rule = fields
                    .get("covers")
                    .copied()
                    .ok_or("waiver record has no covered rule")?;
                waiver_rules
                    .entry((repo.to_owned(), number))
                    .or_default()
                    .insert(rule.to_owned());
            }
            _ => {}
        }
    }
    let keys = reviewed_heads
        .keys()
        .chain(gate_heads.keys())
        .chain(waiver_rules.keys())
        .cloned()
        .chain(
            github_rows
                .iter()
                .map(|row| (row.repository.clone(), row.number)),
        )
        .collect::<BTreeSet<_>>();
    for key in keys {
        let mut row = PullRequestJoins::default();
        if coverage_complete {
            row.reviewed_heads = Some(reviewed_heads.remove(&key).unwrap_or_default());
            row.gate_covered_heads = Some(gate_heads.get(&key).cloned().unwrap_or_default());
        }
        let waivers = waiver_rules.remove(&key).unwrap_or_default();
        if coverage_complete {
            let has_waiver = |rule: &str| {
                waivers
                    .iter()
                    .any(|waiver| waiver == rule || waiver.starts_with(&format!("{rule}-")))
            };
            row.waived_for_gate = Some(has_waiver("LEAD-R11"));
            row.waived_for_review = Some(has_waiver("LEAD-R09"));
            row.ready_merge_waived = Some(has_waiver("LEAD-R17"));
            if let Some(github_row) = github_rows
                .iter()
                .find(|github_row| (github_row.repository.clone(), github_row.number) == key)
            {
                row.gate_required = github_row.changed_files.as_ref().map(|_| true);
                row.gate_covered_heads = github_row.commit_ancestry.as_ref().map(|ancestry| {
                    let gate_heads = gate_heads.get(&key).cloned().unwrap_or_default();
                    let candidate_heads = ancestry
                        .keys()
                        .cloned()
                        .chain(ancestry.values().flatten().cloned())
                        .collect::<BTreeSet<_>>();
                    candidate_heads
                        .into_iter()
                        .filter(|candidate| {
                            gate_heads
                                .iter()
                                .any(|gate_head| commit_is_ancestor(gate_head, candidate, ancestry))
                        })
                        .collect()
                });
                if let Some(times) = reviewer_times.get(&key) {
                    let end_ms = window.end_ms as f64;
                    if times.iter().any(|at| at * 1000.0 < end_ms) {
                        row.reviewer_or_dispatch_by_window_end = Some(true);
                    }
                    if let Some(ready_at) = github_row.ready_at_ms {
                        let deadline = ready_at.saturating_add(
                            review_age_limit_hours
                                .saturating_mul(3_600_000)
                                .min(i64::MAX as u64) as i64,
                        );
                        if times.iter().any(|at| at * 1000.0 <= deadline as f64) {
                            row.reviewer_or_dispatch_by_deadline = Some(true);
                        }
                    }
                }
            }
        }
        joins.insert(key, row);
    }
    Ok(joins)
}

fn lead_record_fields(line: &str) -> Option<BTreeMap<&str, &str>> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .trim_end()
        .strip_suffix("-->")?;
    let mut tokens = inner.split_whitespace();
    if tokens.next()? != "lead-record" {
        return None;
    }
    Some(tokens.filter_map(|token| token.split_once('=')).collect())
}

fn commit_is_ancestor(
    ancestor: &str,
    descendant: &str,
    ancestry: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    if ancestor == descendant {
        return true;
    }
    let mut pending = vec![descendant.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(commit) = pending.pop() {
        if !visited.insert(commit.clone()) {
            continue;
        }
        for parent in ancestry.get(&commit).into_iter().flatten() {
            if parent == ancestor {
                return true;
            }
            pending.push(parent.clone());
        }
    }
    false
}

fn comment_time(comment: &MarkerComment) -> Result<f64, String> {
    let Some(created_at) = comment.created_at.as_deref() else {
        return Err(
            "Linear marker population contains a comment without a source timestamp".into(),
        );
    };
    crate::drift::timestamp_epoch(created_at)
        .map_err(|error| format!("Linear marker creation time is invalid: {error}"))
}

fn comment_ref(comment: &MarkerComment) -> Result<String, String> {
    comment
        .id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .map(|id| format!("linear-comment:{id}"))
        .ok_or_else(|| "Linear marker is missing its native comment id".to_owned())
}

fn add_evidence(
    evidence: &mut BTreeMap<&'static str, Vec<String>>,
    query: &'static str,
    comment: &MarkerComment,
) -> Result<(), String> {
    evidence
        .entry(query)
        .or_default()
        .push(comment_ref(comment)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;

    fn window() -> EvalWindow {
        EvalWindow {
            start_ms: 0,
            end_ms: 60_000,
        }
    }

    fn reviewer_comment(id: &str, at: f64, body: &str) -> MarkerComment {
        MarkerComment {
            id: Some(id.to_owned()),
            created_at: Some(crate::drift::format_timestamp(at)),
            updated_at: None,
            body: body.to_owned(),
        }
    }

    fn real_reviewer_body(pr: &str, run: &str) -> String {
        format!(
            "<!-- reviewer repo=agent-ix/ix-board visibility=public quoin=0.36.10 module=spec-artifacts-process@v0.29.2 id=SR-6710 method=code-review lang=rust pr={pr} head=abc model=claude-sonnet-5-5 run={run} -->\n\n+++ [reviewer data]\n\n```yaml\nclean: false\nscope:\n  - {{id: FR-047-AC-1, path: spec/functional/FR-047-retro-eval.md, role: examined, excerpt: example}}\nfindings:\n  - fnd: FND-001\n    severity: high\n    confidence: high\n    method: code-review\n    check: code-bug\n    artifact_id: FR-047-AC-1\n    related_ids: []\n    path: src/retro/eval.rs\n    lines: \"100-120\"\n    excerpt: example\n    finding: Example finding.\nbindings: []\n```\n\n+++",
        )
    }

    fn disposition_body(round: u64, run: &str) -> String {
        format!(
            "<!-- reviewer-dispositions repo=agent-ix/ix-board visibility=public quoin=0.36.10 module=spec-artifacts-process@v0.29.2 id=SR-6710 round={round} model=claude-sonnet-5-5 run={run} head=abc mergeable=yes -->\n\n+++ [reviewer data]\n\n```yaml\ndispositions:\n  - fnd: FND-001\n    outcome: fixed\n    after_excerpt: fixed in source\n```\n\n+++"
        )
    }

    fn pull_request(number: u64, merged_at: Option<f64>) -> PullRequest {
        PullRequest {
            closed_at: merged_at,
            reopen_events: Some(0),
            repo: "agent-ix/ix-board".into(),
            number,
            open: merged_at.is_none(),
            draft: false,
            created_at: 0.0,
            merged_at,
            updated_at: 0.0,
            additions: 0,
            deletions: 0,
            review_decision: None,
            force_pushes: Some(0),
        }
    }

    fn clean_reviewer_body(pr: &str) -> String {
        format!(
            "<!-- reviewer repo=agent-ix/ix-board visibility=public quoin=0.36.10 module=spec-artifacts-process@v0.29.2 id=SR-6711 method=code-review lang=rust pr={pr} head=abc model=claude-sonnet-5-5 run=clean-run -->\n\n+++ [reviewer data]\n\n```yaml\nclean: true\nscope:\n  - {{id: FR-047-AC-1, path: spec/functional/FR-047-retro-eval.md, role: examined, excerpt: example}}\nfindings: []\nbindings: []\n```\n\n+++"
        )
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn class_a_review_reducers_use_round_state_and_native_window_times() {
        let reviewer = reviewer_comment("review", 20.0, &real_reviewer_body("ix-board#7", "run-a"));
        let open_disposition_body = disposition_body(1, "run-b").replace(
            "outcome: fixed\n    after_excerpt: fixed in source",
            "outcome: still-open\n    reason: needs another change",
        );
        let disposition = reviewer_comment("disposition", 30.0, &open_disposition_body);
        let clean = reviewer_comment("clean", 50.0, &clean_reviewer_body("ix-board#8"));
        let comments = [reviewer, disposition, clean];

        let prs = [pull_request(7, None)];
        let metrics =
            reviewer_class_a_metrics(&comments, &prs, &window(), Some(&BTreeMap::new()), &[]);

        assert_eq!(
            metrics["linear.findings_without_outcome_after_disposition"],
            QueryValue::Value {
                value: 0.0,
                evidence_refs: vec!["linear-comment:disposition".into()],
            }
        );
        assert_eq!(
            metrics["linear.fix_round_findings_returned_still_open"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:disposition".into()],
            }
        );
        assert_eq!(
            metrics["linear.review_to_clean_median_hours"],
            QueryValue::Value {
                value: 0.0,
                evidence_refs: vec!["linear-comment:clean".into()],
            }
        );

        let merged = [pull_request(7, Some(40.0))];
        let merge_metrics =
            reviewer_class_a_metrics(&comments, &merged, &window(), Some(&BTreeMap::new()), &[]);
        assert_eq!(
            merge_metrics["linear.findings_open_at_merge"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec![
                    "linear-comment:disposition".into(),
                    "linear-comment:review".into()
                ],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn class_a_review_queries_require_complete_changed_file_coverage() {
        let comments = [reviewer_comment(
            "review",
            20.0,
            &real_reviewer_body("ix-board#7", "run-a"),
        )];
        let metrics = reviewer_class_a_metrics(&comments, &[], &window(), None, &[]);

        assert!(matches!(
            &metrics["linear.outside_diff_findings_without_failure"],
            QueryValue::Unavailable { reason, .. } if reason == "GitHub changed-file source is unavailable"
        ));
        let complete_files = BTreeMap::from([("agent-ix/ix-board#7".to_owned(), BTreeSet::new())]);
        let complete_metrics =
            reviewer_class_a_metrics(&comments, &[], &window(), Some(&complete_files), &[]);
        assert_eq!(
            complete_metrics["linear.outside_diff_findings_without_failure"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:review".into()],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn gate_review_order_uses_gate_start_and_excludes_waived_pull_requests() {
        let gate = reviewer_comment(
            "gate",
            40.0,
            "<!-- lead-record repo=agent-ix/ix-board kind=gate pr=ix-board#7 head=abc exit=0 started=1970-01-01T00:00:10Z finished=1970-01-01T00:00:40Z -->",
        );
        let record = gate_records(&[gate]).expect("valid gate row");
        let earlier_clean = reviewer_comment("clean", 20.0, &clean_reviewer_body("ix-board#7"));
        let later_clean = reviewer_comment("later-clean", 35.0, &clean_reviewer_body("ix-board#8"));
        let reviews = [earlier_clean, later_clean];

        let result = gate_runs_before_clean_review(&record, &reviews, &BTreeSet::new(), &window())
            .expect("complete review evidence");
        assert_eq!(result, (1, vec!["linear-comment:gate".to_owned()]));
        let waived = BTreeSet::from(["agent-ix/ix-board#7".to_owned()]);
        assert_eq!(
            gate_runs_before_clean_review(&record, &reviews, &waived, &window())
                .expect("complete waived evidence"),
            (0, Vec::new())
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn linear_joins_keep_complete_heads_waivers_and_review_times() {
        let reviewer = reviewer_comment("review", 20.0, &real_reviewer_body("ix-board#7", "run-a"));
        let gate = reviewer_comment(
            "gate",
            40.0,
            "<!-- lead-record repo=agent-ix/ix-board kind=gate pr=ix-board#7 head=abc exit=0 started=1970-01-01T00:00:10Z finished=1970-01-01T00:00:40Z -->",
        );
        let waiver = reviewer_comment(
            "waiver",
            45.0,
            "<!-- lead-record repo=agent-ix/ix-board kind=waiver pr=ix-board#7 covers=LEAD-R11 ref=policy -->",
        );
        let ancestry = BTreeMap::from([
            ("abc".to_owned(), BTreeSet::from(["base".to_owned()])),
            ("def".to_owned(), BTreeSet::from(["abc".to_owned()])),
        ]);
        let github_row = super::super::github_rules::PullRequestEvidence {
            repository: "agent-ix/ix-board".to_owned(),
            number: 7,
            ready_at_ms: Some(10_000),
            changed_files: Some(BTreeSet::from(["src/lib.rs".to_owned()])),
            commit_ancestry: Some(ancestry),
            ..Default::default()
        };

        let joins =
            pull_request_joins(&[reviewer, gate, waiver], true, &[github_row], 1, &window())
                .expect("complete Linear and GitHub joins");
        let joined = &joins[&(String::from("agent-ix/ix-board"), 7)];
        assert_eq!(
            joined.reviewed_heads,
            Some(BTreeSet::from(["abc".to_owned()]))
        );
        assert_eq!(joined.gate_required, Some(true));
        assert_eq!(
            joined.gate_covered_heads,
            Some(BTreeSet::from(["abc".to_owned(), "def".to_owned()]))
        );
        assert_eq!(joined.waived_for_gate, Some(true));
        assert_eq!(joined.waived_for_review, Some(false));
        assert_eq!(joined.reviewer_or_dispatch_by_deadline, Some(true));
        assert_eq!(joined.reviewer_or_dispatch_by_window_end, Some(true));
        assert_eq!(joined.ticket_started_at_ms, None);
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn live_scope_list_shape_is_complete_and_findings_are_counted() {
        let comments = [
            reviewer_comment("review", 20.0, &real_reviewer_body("ix-board#7", "run-a")),
            reviewer_comment("disposition", 30.0, &disposition_body(1, "run-b")),
        ];
        assert!(marker_data_complete(
            "reviewer",
            &parse_yaml_data_block(&comments[0].body).unwrap()
        ));
        assert!(marker_data_complete(
            "reviewer-dispositions",
            &parse_yaml_data_block(&comments[1].body).unwrap()
        ));
        let metrics = review_marker_metrics(&comments, &window()).expect("valid marker population");
        assert_eq!(
            metrics["linear.review_findings"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:review".into()],
            }
        );
        assert_eq!(
            metrics["linear.disposition_rounds"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:disposition".into()],
            }
        );
        assert_eq!(
            metrics["linear.review_comments_unparseable_or_incomplete"],
            QueryValue::Value {
                value: 0.0,
                evidence_refs: vec![],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn malformed_old_marker_does_not_poison_current_window() {
        let old = reviewer_comment(
            "old",
            -120.0,
            "<!-- reviewer repo=owner/repo id=old pr=wrong#bad -->\nnot a YAML block",
        );
        let current = reviewer_comment("current", 20.0, &real_reviewer_body("ix-board#7", "run-a"));
        assert!(marker_data_complete(
            "reviewer",
            &parse_yaml_data_block(&current.body).unwrap()
        ));
        let metrics =
            review_marker_metrics(&[old, current], &window()).expect("old record is ignored");
        assert_eq!(
            metrics["linear.review_comments_unparseable_or_incomplete"],
            QueryValue::Value {
                value: 0.0,
                evidence_refs: vec![],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn non_list_scope_is_counted_as_an_incomplete_marker() {
        let body = real_reviewer_body("ix-board#7", "run-a")
            .replace("scope:\n  - {id: FR-047-AC-1, path: spec/functional/FR-047-retro-eval.md, role: examined, excerpt: example}", "scope: full");
        let metrics =
            review_marker_metrics(&[reviewer_comment("wrong-shape", 20.0, &body)], &window())
                .expect("the source row itself is readable");
        assert_eq!(
            metrics["linear.review_comments_unparseable_or_incomplete"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:wrong-shape".into()],
            }
        );
        assert_eq!(
            metrics["linear.review_findings"],
            QueryValue::Unavailable {
                reason: "Linear review marker payload coverage is incomplete".into(),
                evidence_refs: vec![],
            }
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn post_merge_marker_does_not_count_as_review() {
        let pr = pull_request(7, Some(10.0));
        let comment = reviewer_comment("late", 20.0, &real_reviewer_body("ix-board#7", "run-a"));
        let (without_review, evidence) = merged_prs_without_review_count(
            std::slice::from_ref(&pr),
            std::slice::from_ref(&comment),
            &window(),
        )
        .expect("complete data");
        assert_eq!(without_review, 1);
        assert_eq!(
            evidence,
            vec!["github-pr:agent-ix/ix-board#7:merged-without-review-marker"]
        );
        let (reviewed, _) = reviewed_prs_count(&[pr], &[comment], &window()).expect("valid data");
        assert_eq!(
            reviewed, 1,
            "the merge itself belongs to the reviewed_prs population"
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn repeated_methods_use_distinct_in_window_runs_before_merge_only() {
        let pr = pull_request(7, Some(55.0));
        let comments = [
            reviewer_comment("run-a", 10.0, &real_reviewer_body("ix-board#7", "run-a")),
            reviewer_comment(
                "run-a-copy",
                20.0,
                &real_reviewer_body("ix-board#7", "run-a"),
            ),
            reviewer_comment("run-b", 30.0, &real_reviewer_body("ix-board#7", "run-b")),
            reviewer_comment("run-late", 56.0, &real_reviewer_body("ix-board#7", "run-c")),
            reviewer_comment(
                "run-after-window",
                80.0,
                &real_reviewer_body("ix-board#7", "run-d"),
            ),
        ];
        let (count, refs) =
            pr_methods_with_multiple_runs(&[pr], &comments, &window()).expect("valid comments");
        assert_eq!(count, 1);
        assert_eq!(
            refs,
            vec![
                "linear-comment:run-a".to_owned(),
                "linear-comment:run-a-copy".to_owned(),
                "linear-comment:run-b".to_owned(),
            ]
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn repeated_methods_ignore_malformed_historical_markers_outside_the_window() {
        let malformed_old = reviewer_comment(
            "old",
            -120.0,
            "<!-- reviewer repo=agent-ix/ix-board id=old pr=not-a-pr -->\nold payload",
        );
        let first = reviewer_comment("run-a", 10.0, &real_reviewer_body("ix-board#7", "run-a"));
        let second = reviewer_comment("run-b", 20.0, &real_reviewer_body("ix-board#7", "run-b"));
        let (count, refs) =
            pr_methods_with_multiple_runs(&[], &[malformed_old, first, second], &window())
                .expect("old unrelated marker is not parsed");
        assert_eq!(count, 1);
        assert_eq!(
            refs,
            vec![
                "linear-comment:run-a".to_owned(),
                "linear-comment:run-b".to_owned(),
            ]
        );
    }

    #[test]
    #[trace("FR-047-AC-1")]
    fn disposition_new_findings_require_location_and_text() {
        let review = reviewer_comment("review", 20.0, &real_reviewer_body("ix-board#7", "run-a"));
        let disposition = reviewer_comment(
            "disposition",
            30.0,
            "<!-- reviewer-dispositions repo=agent-ix/ix-board id=SR-6710 round=1 run=run-b head=abc mergeable=no -->\n\n```yaml\ndispositions: []\nnew_findings:\n  - fnd: FND-002\n    severity: medium\n    confidence: high\n    method: code-review\n    check: code-bug\n    artifact_id: FR-047-AC-1\n    related_ids: []\n    path: ''\n    lines: ''\n    excerpt: example\n    finding: Missing valid coverage.\n```",
        );
        let metrics = review_marker_metrics(&[review, disposition], &window()).expect("valid rows");
        assert_eq!(
            metrics["linear.review_findings"],
            QueryValue::Value {
                value: 2.0,
                evidence_refs: vec![
                    "linear-comment:disposition".into(),
                    "linear-comment:review".into(),
                ],
            }
        );
        assert_eq!(
            metrics["linear.findings_without_location_or_scenario"],
            QueryValue::Value {
                value: 1.0,
                evidence_refs: vec!["linear-comment:disposition".into()],
            }
        );
    }
}
