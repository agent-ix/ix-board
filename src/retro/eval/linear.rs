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

fn reviewer_marker<'a>(
    comment: &'a MarkerComment,
) -> Result<(String, String, BTreeMap<&'a str, &'a str>), String> {
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

fn normalize_pr_key(repo: &str, pr: &str) -> Result<String, String> {
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
        let (without_review, evidence) =
            merged_prs_without_review_count(&[pr.clone()], &[comment.clone()], &window())
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
