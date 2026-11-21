// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Attributed dev-team review findings from native tracker comments, never prose inference.
use crate::{
    MarkerComment,
    retro::metrics::{first_line, marker_fields},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewStats {
    pub values: BTreeMap<String, f64>,
}
pub(crate) const NAMES: &[&str] = &[
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
];

// These are projections of the externally owned dev-team marker payload. Extra
// finding metadata (excerpts, paths, scope and model text) is deliberately ignored.
#[derive(Deserialize)]
struct ReviewData {
    clean: bool,
    findings: Vec<Finding>,
}
#[derive(Deserialize)]
struct Finding {
    fnd: String,
    severity: String,
}
#[derive(Deserialize)]
struct DispositionData {
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
pub(crate) fn calculate(
    comments: &[MarkerComment],
    allowed: &BTreeSet<String>,
    created: &BTreeMap<String, f64>,
    now: f64,
) -> Result<ReviewStats, String> {
    let mut reviews: BTreeMap<(String, String), Review> = BTreeMap::new();
    let mut outcomes: BTreeMap<(String, String, String), (u64, f64, Disposition)> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    // Resolve attributed review artifacts before their dispositions, regardless
    // of source ordering. Unrelated artifacts cannot invalidate this scope.
    let mut ordered: Vec<_> = comments.iter().collect();
    ordered.sort_by_key(|c| {
        marker_fields(first_line(&c.body)).is_none_or(|(kind, _)| kind != "reviewer")
    });
    for comment in ordered {
        let id = comment
            .id
            .as_ref()
            .ok_or("review comment has no native identity")?;
        if !seen.insert(id) {
            continue;
        }
        let at = crate::drift::timestamp_epoch(
            comment
                .created_at
                .as_deref()
                .ok_or("review comment has no source timestamp")?,
        )
        .map_err(|e| e.to_string())?;
        if at <= now - 86400.0 || at > now {
            continue;
        }
        let Some((kind, fields)) = marker_fields(first_line(&comment.body)) else {
            return Err("unsupported review marker".into());
        };
        let repo = *fields
            .get("repo")
            .ok_or("review marker has no repository")?;
        let artifact = *fields
            .get("id")
            .ok_or("review marker has no artifact identity")?;
        if kind == "reviewer" {
            let pr = *fields
                .get("pr")
                .ok_or("review marker has no PR attribution")?;
            let full = if pr.starts_with(&format!("{repo}#")) {
                pr.to_owned()
            } else {
                let short = repo.rsplit('/').next().ok_or("repository identity")?;
                let number = pr
                    .strip_prefix(&format!("{short}#"))
                    .ok_or("PR/repository attribution mismatch")?;
                format!("{repo}#{number}")
            };
            if !allowed.contains(&full) {
                continue;
            }
            let data: ReviewData = yaml(&comment.body)?;
            if data.clean != data.findings.is_empty() {
                return Err("review clean flag contradicts findings".into());
            }
            let mut findings = BTreeMap::new();
            for finding in data.findings {
                if !matches!(finding.severity.as_str(), "high" | "medium" | "low")
                    || findings.insert(finding.fnd, finding.severity).is_some()
                {
                    return Err("duplicate finding or unsupported severity".into());
                }
            }
            if reviews
                .insert(
                    (repo.into(), artifact.into()),
                    Review {
                        pr: full,
                        at,
                        clean: data.clean,
                        findings,
                    },
                )
                .is_some()
            {
                return Err("duplicate review artifact identity".into());
            }
        } else if kind == "reviewer-dispositions" {
            if !reviews.contains_key(&(repo.into(), artifact.into())) {
                continue;
            }
            let round = fields
                .get("round")
                .ok_or("disposition round missing")?
                .parse::<u64>()
                .map_err(|_| "disposition round invalid")?;
            let data: DispositionData = yaml(&comment.body)?;
            for d in data.dispositions {
                if !matches!(
                    d.outcome.as_str(),
                    "fixed" | "rejected" | "deferred" | "accepted-no-change" | "still-open"
                ) {
                    return Err("unsupported finding disposition".into());
                }
                let key = (repo.into(), artifact.into(), d.fnd.clone());
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
    let (mut pending, mut rejected) = (0usize, 0usize);
    for ((repo, id), review) in &reviews {
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
    let mut values = BTreeMap::new();
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
    if classified > 0 {
        values.insert(
            "review_confirmed_finding_pr_share_24h".into(),
            prs.values().filter(|p| p.1 && p.2).count() as f64 / classified as f64,
        );
    }
    let mut delays: Vec<_> = prs
        .iter()
        .filter_map(|(pr, row)| {
            created
                .get(pr)
                .filter(|at| **at > now - 86400.0 && **at <= row.3)
                .map(|at| row.3 - at)
        })
        .collect();
    delays.sort_by(f64::total_cmp);
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
    fn comment(id: &str, seconds: f64, marker: &str, data: &str) -> MarkerComment {
        MarkerComment {
            id: Some(id.into()),
            created_at: Some(crate::drift::format_timestamp(seconds)),
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
    /// Trace: FR-032-AC-10
    #[test]
    fn pending_findings_are_not_fabricated_as_confirmed_or_clean() {
        let v = calculate(&[review()], &allowed(), &BTreeMap::new(), 100000.0)
            .unwrap()
            .values;
        assert_eq!(v["review_recorded_prs_24h"], 1.0);
        assert_eq!(v["review_findings_pending_24h"], 1.0);
        assert_eq!(v["review_classifiable_prs_24h"], 0.0);
        assert!(!v.contains_key("review_confirmed_finding_pr_share_24h"));
    }
    /// Trace: FR-032-AC-10
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
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(v["review_recorded_prs_24h"], 1.0);
        assert_eq!(v["review_findings_rejected_24h"], 1.0);
        assert_eq!(v["review_findings_confirmed_high_24h"], 0.0);
        assert_eq!(v["review_confirmed_finding_pr_share_24h"], 0.0);
    }
    /// Trace: FR-032-AC-10
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
            100000.0,
        )
        .unwrap()
        .values;
        assert_eq!(v["review_findings_confirmed_high_24h"], 1.0);
        assert_eq!(v["review_confirmed_finding_pr_share_24h"], 1.0);
        assert_eq!(v["new_pr_first_review_median_seconds_24h"], 1000.0);
    }
    /// Trace: FR-032-AC-10
    #[test]
    fn missing_or_contradictory_structured_evidence_is_unavailable() {
        let mut r = review();
        r.body =
            "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 -->\nHigh severity defect in prose"
                .into();
        assert!(calculate(&[r], &allowed(), &BTreeMap::new(), 100000.0).is_err());
        let bad = comment(
            "bad",
            99000.0,
            "<!-- reviewer repo=owner/repo id=SR-1 pr=repo#1 -->",
            "clean: true\nfindings:\n  - fnd: FND-1\n    severity: high",
        );
        assert!(calculate(&[bad], &allowed(), &BTreeMap::new(), 100000.0).is_err());
    }
}
