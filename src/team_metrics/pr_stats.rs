// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! PR outcome and review measures; production regressions need a separate explicit contract.
use crate::retro::sources::PullRequest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Scalar PR measurements; every exported name is declared in this module.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrStats {
    pub values: BTreeMap<String, f64>,
}
pub(crate) const NAMES: &[&str] = &[
    "prs_closed_unmerged_24h",
    "pr_closed_unmerged_share_24h",
    "prs_changes_requested",
    "prs_review_required",
    "pr_changes_requested_share",
    "prs_reopened_open",
    "pr_size_median_lines",
    "pr_size_p90_lines",
    "pr_size_count",
];
/// Computes current-status and final-closure cohorts without labeling abandonment as a defect.
pub(crate) fn calculate(prs: &[PullRequest], now: f64) -> PrStats {
    let mut values = BTreeMap::new();
    let inside = |at: f64| at > now - 86400.0 && at <= now;
    let mut seen = BTreeSet::new();
    let prs: Vec<_> = prs
        .iter()
        .filter(|p| seen.insert((&p.repo, p.number)))
        .collect();
    let open: Vec<_> = prs.iter().copied().filter(|p| p.open && !p.draft).collect();
    let decisions_complete = open.iter().all(|p| {
        p.review_decision
            .as_deref()
            .is_none_or(|s| matches!(s, "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED"))
    });
    if decisions_complete {
        let requested = open
            .iter()
            .filter(|p| p.review_decision.as_deref() == Some("CHANGES_REQUESTED"))
            .count();
        let approved = open
            .iter()
            .filter(|p| p.review_decision.as_deref() == Some("APPROVED"))
            .count();
        values.insert("prs_changes_requested".into(), requested as f64);
        values.insert(
            "prs_review_required".into(),
            open.iter()
                .filter(|p| p.review_decision.as_deref() == Some("REVIEW_REQUIRED"))
                .count() as f64,
        );
        if requested + approved > 0 {
            values.insert(
                "pr_changes_requested_share".into(),
                requested as f64 / (requested + approved) as f64,
            );
        }
    }
    if prs
        .iter()
        .filter(|p| !p.open && p.merged_at.is_none())
        .all(|p| p.closed_at.is_some())
    {
        let closed = prs
            .iter()
            .filter(|p| !p.open && p.merged_at.is_none() && p.closed_at.is_some_and(inside))
            .count();
        let merged = prs
            .iter()
            .filter(|p| p.merged_at.is_some_and(inside))
            .count();
        values.insert("prs_closed_unmerged_24h".into(), closed as f64);
        if closed + merged > 0 {
            values.insert(
                "pr_closed_unmerged_share_24h".into(),
                closed as f64 / (closed + merged) as f64,
            );
        }
    }
    if open.iter().all(|p| p.reopen_events.is_some()) {
        values.insert(
            "prs_reopened_open".into(),
            open.iter()
                .filter(|p| p.reopen_events.is_some_and(|n| n > 0))
                .count() as f64,
        );
    }
    let mut sizes: Vec<_> = prs
        .iter()
        .filter(|p| p.open || p.merged_at.is_some_and(inside))
        .map(|p| p.additions.checked_add(p.deletions))
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    sizes.sort();
    values.insert("pr_size_count".into(), sizes.len() as f64);
    if !sizes.is_empty() {
        for (name, fraction) in [("pr_size_median_lines", 0.5), ("pr_size_p90_lines", 0.9)] {
            let rank = (sizes.len() as f64 * fraction).ceil() as usize;
            if let Some(value) = sizes.get(rank.saturating_sub(1)) {
                values.insert(name.into(), *value as f64);
            }
        }
    }
    PrStats { values }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pr(n: u64) -> PullRequest {
        PullRequest {
            repo: "owner/repo".into(),
            number: n,
            open: true,
            draft: false,
            created_at: 1000.0,
            merged_at: None,
            closed_at: None,
            reopen_events: Some(0),
            updated_at: 100000.0,
            additions: 10,
            deletions: 5,
            review_decision: None,
            force_pushes: Some(0),
        }
    }
    /// Trace: FR-033-AC-9
    #[test]
    fn closure_and_review_ratios_keep_their_populations_distinct() {
        let mut abandoned = pr(1);
        abandoned.open = false;
        abandoned.closed_at = Some(99000.0);
        let mut merged = pr(2);
        merged.open = false;
        merged.merged_at = Some(99000.0);
        merged.closed_at = Some(99000.0);
        let mut changes = pr(3);
        changes.review_decision = Some("CHANGES_REQUESTED".into());
        changes.reopen_events = Some(2);
        let mut approved = pr(4);
        approved.review_decision = Some("APPROVED".into());
        let values = calculate(
            &[abandoned.clone(), abandoned, merged, changes, approved],
            100000.0,
        )
        .values;
        assert_eq!(values["prs_closed_unmerged_24h"], 1.0);
        assert_eq!(values["pr_closed_unmerged_share_24h"], 0.5);
        assert_eq!(values["pr_changes_requested_share"], 0.5);
        assert_eq!(values["prs_reopened_open"], 1.0);
    }
    /// Trace: FR-033-AC-9
    #[test]
    fn no_decisive_reviews_or_closures_means_unavailable_ratios() {
        let v = calculate(&[pr(1)], 100000.0).values;
        assert!(!v.contains_key("pr_changes_requested_share"));
        assert!(!v.contains_key("pr_closed_unmerged_share_24h"));
    }
}
