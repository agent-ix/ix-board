// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Attributed dev-team review findings from native tracker comments, never prose inference.
use crate::{
    MarkerComment,
    retro::metrics::{first_line, marker_fields},
};
use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewStats {
    pub values: BTreeMap<String, f64>,
}
pub(crate) const NAMES: &[&str] = &[
    "pr_changes_requested_share",
    "review_recorded_prs_24h",
    "review_classifiable_prs_24h",
    "review_classification_share",
    "review_prs_with_findings_share_24h",
    "review_confirmed_finding_pr_share_24h",
    "review_findings_confirmed_high_24h",
    "review_findings_confirmed_medium_24h",
    "review_findings_confirmed_low_24h",
    "review_findings_confirmed_unknown_24h",
    "review_findings_pending_24h",
    "review_findings_rejected_24h",
    "new_pr_first_review_median_seconds_24h",
    "new_pr_first_review_p90_seconds_24h",
    "new_pr_first_review_count_24h",
    "review_records_skipped_24h",
];

// These are projections of the externally owned dev-team marker payload. Extra
// finding metadata (excerpts, paths, scope and model text) is deliberately ignored.
#[derive(Deserialize)]
struct ReviewData {
    clean: bool,
    findings: Vec<Finding>,
    #[serde(default)]
    mergeable: Option<bool>,
}
#[derive(Deserialize)]
struct Finding {
    fnd: String,
    severity: String,
}
#[derive(Deserialize)]
struct DispositionData {
    #[serde(default)]
    mergeable: Option<bool>,
    dispositions: Vec<Disposition>,
}
#[derive(Clone, Deserialize)]
struct Disposition {
    fnd: String,
    outcome: String,
}
#[derive(Clone)]
struct Review {
    pr: String,
    at: f64,
    clean: bool,
    findings: BTreeMap<String, String>,
}

fn yaml<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, String> {
    if body.len() > 262144 {
        return Err("review record exceeds 256KiB".into());
    }
    let mut block = String::new();
    let mut active = false;
    let mut width = 0;
    let mut decoded = Vec::new();
    for line in body.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let text = line.trim_start_matches(' ');
        if indent <= 3 && text.starts_with("```") {
            let ticks = text.bytes().take_while(|b| *b == b'`').count();
            let tag = text.get(ticks..).unwrap_or("").trim();
            if !active && matches!(tag, "yaml" | "yml") {
                active = true;
                width = ticks;
                block.clear();
                continue;
            }
            if active && ticks >= width && tag.is_empty() {
                if let Ok(value) = serde_yaml::from_str::<T>(&block) {
                    decoded.push(value);
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
        return Err("review record requires exactly one supported structured YAML block".into());
    }
    decoded
        .pop()
        .ok_or_else(|| "review record missing data".into())
}

/// Measures only PRs with explicit review records created in the selected day.
/// Unresolved findings are pending; dispositions distinguish confirmed from rejected.
#[implements("FR-033-AC-10", "FR-033-AC-17")]
pub(crate) fn calculate(
    comments: &[MarkerComment],
    allowed: &BTreeSet<String>,
    created: &BTreeMap<String, f64>,
    open_prs: &BTreeSet<String>,
    now: f64,
) -> Result<ReviewStats, String> {
    let mut reviews: BTreeMap<(String, String), Review> = BTreeMap::new();
    let mut outcomes: BTreeMap<(String, String, String), (u64, f64, Disposition)> = BTreeMap::new();
    let mut latest_records = BTreeMap::<String, (f64, Option<bool>)>::new();
    let mut seen = BTreeSet::new();
    let mut skipped = 0usize;
    // Resolve attributed review artifacts before their dispositions, regardless
    // of source ordering. Unrelated artifacts cannot invalidate this scope.
    let mut ordered: Vec<_> = comments.iter().collect();
    ordered.sort_by_key(|c| {
        marker_fields(first_line(&c.body)).is_none_or(|(kind, _)| kind != "reviewer")
    });
    for comment in ordered {
        let Some(id) = comment.id.as_ref() else {
            skipped += 1;
            continue;
        };
        if !seen.insert(id) {
            skipped += 1;
            continue;
        }
        let Some(created_at) = comment.created_at.as_deref() else {
            skipped += 1;
            continue;
        };
        let Ok(at) = crate::drift::timestamp_epoch(created_at) else {
            skipped += 1;
            continue;
        };
        if at > now {
            continue;
        }
        let Some((kind, fields)) = marker_fields(first_line(&comment.body)) else {
            skipped += 1;
            continue;
        };
        let (Some(repo), Some(artifact)) = (fields.get("repo"), fields.get("id")) else {
            skipped += 1;
            continue;
        };
        if kind == "reviewer" {
            let Some(pr) = fields.get("pr") else {
                skipped += 1;
                continue;
            };
            let full = if pr.starts_with(&format!("{repo}#")) {
                (*pr).to_owned()
            } else {
                let Some(short) = repo.rsplit('/').next() else {
                    skipped += 1;
                    continue;
                };
                let Some(number) = pr.strip_prefix(&format!("{short}#")) else {
                    skipped += 1;
                    continue;
                };
                format!("{repo}#{number}")
            };
            if !allowed.contains(&full) {
                continue;
            }
            let Ok(data) = yaml::<ReviewData>(&comment.body) else {
                skipped += 1;
                continue;
            };
            if data.clean != data.findings.is_empty() {
                skipped += 1;
                continue;
            }
            let mut findings = BTreeMap::new();
            let mut valid = true;
            for finding in data.findings {
                if !matches!(finding.severity.as_str(), "high" | "medium" | "low")
                    || findings.insert(finding.fnd, finding.severity).is_some()
                {
                    valid = false;
                    break;
                }
            }
            let key = (repo.to_string(), artifact.to_string());
            if !valid || reviews.contains_key(&key) {
                skipped += 1;
                continue;
            }
            reviews.insert(
                key,
                Review {
                    pr: full.clone(),
                    at,
                    clean: data.clean,
                    findings,
                },
            );
            if latest_records
                .get(&full)
                .is_none_or(|(old_at, _)| at > *old_at)
            {
                latest_records.insert(full, (at, data.mergeable));
            }
        } else if kind == "reviewer-dispositions" {
            let artifact_key = ((*repo).to_string(), (*artifact).to_string());
            let Some(review) = reviews.get(&artifact_key) else {
                continue;
            };
            let pr = review.pr.clone();
            let Some(round) = fields.get("round").and_then(|r| r.parse::<u64>().ok()) else {
                skipped += 1;
                continue;
            };
            let Ok(data) = yaml::<DispositionData>(&comment.body) else {
                skipped += 1;
                continue;
            };
            if data.dispositions.iter().any(|d| {
                !matches!(
                    d.outcome.as_str(),
                    "fixed" | "rejected" | "deferred" | "accepted-no-change" | "still-open"
                )
            }) {
                skipped += 1;
                continue;
            }
            if latest_records
                .get(&pr)
                .is_none_or(|(old_at, _)| at > *old_at)
            {
                latest_records.insert(pr, (at, data.mergeable));
            }
            for d in data.dispositions {
                let key = ((*repo).to_string(), (*artifact).to_string(), d.fnd.clone());
                if outcomes
                    .get(&key)
                    .is_none_or(|old| (round, at) > (old.0, old.1))
                {
                    outcomes.insert(key, (round, at, d));
                }
            }
        }
    }
    let mut prs: BTreeMap<String, (bool, bool, bool, f64)> = BTreeMap::new();
    let mut confirmed: BTreeMap<String, usize> = BTreeMap::from([
        ("high".into(), 0),
        ("medium".into(), 0),
        ("low".into(), 0),
        ("unknown".into(), 0),
    ]);
    let mut latest_reviews: BTreeMap<String, &Review> = BTreeMap::new();
    let (mut pending, mut rejected) = (0usize, 0usize);
    for ((repo, id), review) in &reviews {
        if latest_reviews
            .get(&review.pr)
            .is_none_or(|old| review.at > old.at)
        {
            latest_reviews.insert(review.pr.clone(), review);
        }
        if review.at <= now - 86400.0 {
            continue;
        }
        let row = prs
            .entry(review.pr.clone())
            .or_insert((false, false, true, review.at));
        row.0 |= !review.clean;
        row.3 = row.3.min(review.at);
        let mut findings = review.findings.clone();
        for key in outcomes.keys().filter(|(r, a, _)| r == repo && a == id) {
            findings
                .entry(key.2.clone())
                .or_insert_with(|| "unknown".into());
        }
        row.0 |= !findings.is_empty();
        for (fnd, severity) in findings {
            match outcomes.get(&(repo.clone(), id.clone(), fnd)) {
                Some((_, _, d)) if d.outcome == "rejected" => rejected += 1,
                Some(_) => {
                    row.1 = true;
                    *confirmed.entry(severity).or_default() += 1;
                }
                None => {
                    pending += 1;
                    row.2 = false;
                }
            }
        }
    }
    let mut latest_finding_state = BTreeMap::<(String, String), (u64, f64, bool)>::new();
    for ((repo, artifact), review) in &reviews {
        for fnd in review.findings.keys() {
            let disposition = outcomes.get(&(repo.clone(), artifact.clone(), fnd.clone()));
            let (round, at, unresolved) = match disposition {
                Some((round, at, disposition)) => (
                    *round,
                    *at,
                    matches!(disposition.outcome.as_str(), "still-open" | "deferred"),
                ),
                None => (0, review.at, true),
            };
            let key = (review.pr.clone(), fnd.clone());
            if latest_finding_state
                .get(&key)
                .is_none_or(|old| (at, round) > (old.1, old.0))
            {
                latest_finding_state.insert(key, (round, at, unresolved));
            }
        }
    }
    let unresolved_prs = latest_finding_state
        .into_iter()
        .filter_map(|((pr, _), (_, _, unresolved))| unresolved.then_some(pr))
        .collect::<BTreeSet<_>>();
    let mut values = BTreeMap::new();
    values.insert("review_records_skipped_24h".into(), skipped as f64);
    let classified = prs.values().filter(|p| p.2).count();
    values.insert("review_recorded_prs_24h".into(), prs.len() as f64);
    values.insert("review_classifiable_prs_24h".into(), classified as f64);
    values.insert("review_findings_pending_24h".into(), pending as f64);
    values.insert("review_findings_rejected_24h".into(), rejected as f64);
    for (severity, n) in confirmed {
        values.insert(
            format!("review_findings_confirmed_{severity}_24h"),
            n as f64,
        );
    }
    if !prs.is_empty() {
        values.insert(
            "review_classification_share".into(),
            classified as f64 / prs.len() as f64,
        );
        values.insert(
            "review_prs_with_findings_share_24h".into(),
            prs.values().filter(|p| p.0).count() as f64 / prs.len() as f64,
        );
    }
    let reviewed_open: Vec<_> = open_prs
        .iter()
        .filter_map(|pr| latest_reviews.get(pr).copied())
        .collect();
    let open_review_coverage = if open_prs.is_empty() {
        1.0
    } else {
        reviewed_open.len() as f64 / open_prs.len() as f64
    };
    values.insert("pr_changes_requested_coverage".into(), open_review_coverage);
    if !reviewed_open.is_empty() {
        let requested = reviewed_open
            .iter()
            .filter(|review| {
                latest_records
                    .get(&review.pr)
                    .is_some_and(|(_, mergeable)| *mergeable == Some(false))
                    || unresolved_prs.contains(&review.pr)
            })
            .count();
        values.insert(
            "pr_changes_requested_share".into(),
            requested as f64 / reviewed_open.len() as f64,
        );
    }
    if classified > 0 {
        values.insert(
            "review_confirmed_finding_pr_share_24h".into(),
            prs.values().filter(|p| p.1 && p.2).count() as f64 / classified as f64,
        );
    }
    let eligible_new_prs = created
        .values()
        .filter(|at| **at > now - 86400.0 && **at <= now)
        .count();
    let mut delays: Vec<_> = created
        .iter()
        .filter(|(_, at)| **at > now - 86400.0 && **at <= now)
        .filter_map(|(pr, at)| {
            prs.get(pr)
                .filter(|row| row.3 >= *at)
                .map(|row| row.3 - *at)
        })
        .collect();
    delays.sort_by(f64::total_cmp);
    let first_review_coverage = if eligible_new_prs == 0 {
        1.0
    } else {
        delays.len() as f64 / eligible_new_prs as f64
    };
    values.insert(
        "new_pr_first_review_coverage_24h".into(),
        first_review_coverage,
    );
    values.insert("new_pr_first_review_count_24h".into(), delays.len() as f64);
    if !delays.is_empty() {
        for (name, fraction) in [
            ("new_pr_first_review_median_seconds_24h", 0.5),
            ("new_pr_first_review_p90_seconds_24h", 0.9),
        ] {
            let rank = (delays.len() as f64 * fraction).ceil() as usize;
            if let Some(v) = delays.get(rank.saturating_sub(1)) {
                values.insert(name.into(), *v);
            }
        }
    }
    Ok(ReviewStats { values })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;
    fn comment(id: &str, seconds: f64, marker: &str, data: &str) -> MarkerComment {
        MarkerComment {
            id: Some(id.into()),
            created_at: Some(crate::drift::format_timestamp(seconds)),
            updated_at: Some(crate::drift::format_timestamp(seconds)),
            body: format!("{marker}\n\n```yaml\n{data}\n```\n"),
        }
    }
    fn allowed() -> BTreeSet<String> {
        BTreeSet::from(["owner/repo#1".into()])
    }
    fn review() -> MarkerComment {
        comment(
            "review",
            99000.0,
            "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 method=rust-review run=a -->",
            "clean: false\nfindings:\n  - fnd: FND-1\n    severity: high\n    finding: private prose is not counted",
        )
    }
    #[trace("FR-033-AC-10")]
    #[test]
    fn pending_findings_are_not_fabricated_as_confirmed_or_clean() {
        let v = calculate(
            &[review()],
            &allowed(),
            &BTreeMap::new(),
            &allowed(),
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(v["review_recorded_prs_24h"], 1.0);
        assert_eq!(v["review_findings_pending_24h"], 1.0);
        assert_eq!(v["review_classifiable_prs_24h"], 0.0);
        assert_eq!(v["pr_changes_requested_coverage"], 1.0);
        assert_eq!(v["pr_changes_requested_share"], 1.0);
        assert_eq!(v["review_records_skipped_24h"], 0.0);
        assert!(!v.contains_key("review_confirmed_finding_pr_share_24h"));
    }
    #[trace("FR-033-AC-10")]
    #[test]
    fn latest_disposition_supersedes_earlier_outcomes_and_native_ids_deduplicate() {
        let fixed = comment(
            "fixed",
            99100.0,
            "<!-- reviewer-dispositions repo=owner/repo id=SR-1 round=1 run=b -->",
            "dispositions:\n  - fnd: FND-1\n    outcome: fixed",
        );
        let rejected = comment(
            "rejected",
            99200.0,
            "<!-- reviewer-dispositions repo=owner/repo id=SR-1 round=2 run=c -->",
            "dispositions:\n  - fnd: FND-1\n    outcome: rejected\n    reason: false positive",
        );
        let v = calculate(
            &[review(), review(), rejected, fixed],
            &allowed(),
            &BTreeMap::new(),
            &allowed(),
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(v["review_recorded_prs_24h"], 1.0);
        assert_eq!(v["review_findings_rejected_24h"], 1.0);
        assert_eq!(v["review_findings_confirmed_high_24h"], 0.0);
        assert_eq!(v["review_confirmed_finding_pr_share_24h"], 0.0);
    }
    #[trace("FR-033-AC-10")]
    #[test]
    fn confirmed_findings_use_severity_and_new_pr_first_record_time() {
        let fixed = comment(
            "fixed",
            99100.0,
            "<!-- reviewer-dispositions repo=owner/repo id=SR-1 round=1 run=b -->",
            "dispositions:\n  - fnd: FND-1\n    outcome: fixed",
        );
        let v = calculate(
            &[review(), fixed],
            &allowed(),
            &BTreeMap::from([("owner/repo#1".into(), 98000.0)]),
            &allowed(),
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(v["review_findings_confirmed_high_24h"], 1.0);
        assert_eq!(v["review_confirmed_finding_pr_share_24h"], 1.0);
        assert_eq!(v["new_pr_first_review_median_seconds_24h"], 1000.0);
    }
    #[trace("FR-033-AC-10")]
    #[test]
    fn missing_or_contradictory_structured_evidence_is_skipped_and_counted() {
        let mut r = review();
        r.body =
            "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 -->\nHigh severity defect in prose"
                .into();
        let v = calculate(&[r], &allowed(), &BTreeMap::new(), &allowed(), 100000.0)
            .unwrap()
            .values;
        assert_eq!(v["review_records_skipped_24h"], 1.0);
        assert!(!v.contains_key("new_pr_first_review_median_seconds_24h"));
        let bad = comment(
            "bad",
            99000.0,
            "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 -->",
            "clean: true\nfindings:\n  - fnd: FND-1\n    severity: high",
        );
        let v = calculate(&[bad], &allowed(), &BTreeMap::new(), &allowed(), 100000.0)
            .unwrap()
            .values;
        assert_eq!(v["review_records_skipped_24h"], 1.0);
        assert!(!v.contains_key("new_pr_first_review_median_seconds_24h"));
    }
    #[trace("FR-033-AC-10")]
    #[test]
    fn missing_native_comment_id_and_duplicate_review_identity_are_counted_and_skipped() {
        let mut no_id = review();
        no_id.id = None;
        let mut duplicate = review();
        duplicate.id = Some("duplicate-native-id".into());
        let mut another = duplicate.clone();
        another.id = Some("second-native-id".into());
        let v = calculate(
            &[no_id, duplicate, another],
            &allowed(),
            &BTreeMap::new(),
            &allowed(),
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(v["review_records_skipped_24h"], 2.0);
        assert_eq!(v["review_recorded_prs_24h"], 1.0);
        assert_eq!(v["pr_changes_requested_share"], 1.0);
    }

    #[trace("FR-033-AC-17")]
    #[test]
    fn changes_requested_uses_latest_mergeability_not_the_findings_share() {
        let not_mergeable = comment(
            "not-mergeable",
            99000.0,
            "<!-- reviewer repo=owner/repo id=SR-2 pr=repo#1 method=rust-review run=b -->",
            "clean: true\nmergeable: false\nfindings: []",
        );
        let open_prs = BTreeSet::from(["owner/repo#1".into(), "owner/repo#2".into()]);
        let values = calculate(
            &[not_mergeable],
            &allowed(),
            &BTreeMap::new(),
            &open_prs,
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(values["pr_changes_requested_share"], 1.0);
        assert_eq!(values["pr_changes_requested_coverage"], 0.5);
        assert_eq!(values["review_prs_with_findings_share_24h"], 0.0);
    }

    #[test]
    fn later_clean_review_does_not_hide_an_unresolved_older_finding() {
        let older = comment(
            "older",
            98_000.0,
            "<!-- reviewer repo=owner/repo id=SR-older pr=repo#1 method=rust-review run=a -->",
            "clean: false\nfindings:\n  - fnd: FND-1\n    severity: high\nmergeable: true",
        );
        let newer = comment(
            "newer",
            99_000.0,
            "<!-- reviewer repo=owner/repo id=SR-newer pr=repo#1 method=rust-review run=b -->",
            "clean: true\nfindings: []\nmergeable: true",
        );
        let open = allowed();
        let values = calculate(&[older, newer], &open, &BTreeMap::new(), &open, 100_000.0)
            .unwrap()
            .values;
        assert_eq!(values["pr_changes_requested_share"], 1.0);
    }

    #[test]
    fn latest_disposition_mergeability_marks_the_open_pr() {
        let review = comment(
            "review",
            98_000.0,
            "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 method=rust-review run=a -->",
            "clean: true\nfindings: []\nmergeable: true",
        );
        let disposition = comment(
            "disposition",
            99_000.0,
            "<!-- reviewer-dispositions repo=owner/repo id=SR-1 round=1 run=b -->",
            "mergeable: false\ndispositions: []",
        );
        let open = allowed();
        let values = calculate(
            &[review, disposition],
            &open,
            &BTreeMap::new(),
            &open,
            100_000.0,
        )
        .unwrap()
        .values;
        assert_eq!(values["pr_changes_requested_share"], 1.0);
    }

    #[trace("FR-033-AC-17")]
    #[test]
    fn changes_requested_share_remains_visible_with_partial_coverage() {
        let open: BTreeSet<_> = (1..=20)
            .map(|number| format!("owner/repo#{number}"))
            .collect();
        let mut comments = Vec::new();
        let mut created = BTreeMap::new();
        for number in 1..=19 {
            let mergeable = number != 1;
            let pr = format!("owner/repo#{number}");
            created.insert(pr, 90_000.0);
            comments.push(comment(
                &format!("review-{number}"),
                99_000.0,
                &format!("<!-- reviewer repo=owner/repo id=SR-{number} pr=repo#{number} method=rust-review run=a -->"),
                &format!("clean: true\nfindings: []\nmergeable: {mergeable}"),
            ));
        }
        let values = calculate(&comments, &open, &created, &open, 100_000.0)
            .unwrap()
            .values;
        assert_eq!(values["pr_changes_requested_coverage"], 0.95);
        assert_eq!(values["pr_changes_requested_share"], 1.0 / 19.0);
    }

    #[trace("FR-033-AC-17")]
    #[test]
    fn first_review_distribution_is_published_for_partial_coverage() {
        let open = BTreeSet::from(["owner/repo#1".into(), "owner/repo#2".into()]);
        let created = BTreeMap::from([
            ("owner/repo#1".into(), 98_000.0),
            ("owner/repo#2".into(), 98_000.0),
        ]);
        let values = calculate(&[review()], &allowed(), &created, &open, 100_000.0)
            .unwrap()
            .values;
        assert_eq!(values["new_pr_first_review_coverage_24h"], 0.5);
        assert_eq!(values["new_pr_first_review_median_seconds_24h"], 1_000.0);
    }
}
