// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Native tracker history and attachment joins, without prose-derived completion or activity.
use super::{Config, Evidence, Fact, Observation, SourceStamp};
use crate::WorkflowStateType;
use crate::retro::sources::{GithubSource, StateChange};

/// Enriches a native issue row with its latest genuine Started entry from complete history.
/// Failed histories and ambiguous equal-time transitions remain unavailable.
pub fn enrich_claim_history(
    row: &mut Observation,
    history: Result<&[StateChange], String>,
    observed_at: Option<&str>,
) {
    let entity = row.entity_id.clone();
    let Fact::Issue(issue) = &mut row.fact else {
        return;
    };
    let claim = history.and_then(|history| {
        let latest = history
            .iter()
            .filter(|c| {
                c.to == Some(WorkflowStateType::Started)
                    && c.from != Some(WorkflowStateType::Started)
            })
            .max_by(|a, b| a.at.total_cmp(&b.at));
        let Some(latest) = latest else {
            return Ok(None);
        };
        if !latest.at.is_finite() || history.iter().filter(|c| c.at == latest.at).count() != 1 {
            return Err("ambiguous native claim-entry ordering".into());
        }
        Ok(Some(crate::drift::format_timestamp(latest.at)))
    });
    issue.claim_at = claim.as_ref().ok().and_then(Clone::clone);
    issue.provenance.insert(
        "claim_at".into(),
        SourceStamp {
            source: "linear".into(),
            observed_at: observed_at.map(str::to_owned),
            complete: claim.is_ok(),
            reason: claim.err(),
            evidence_refs: vec![format!("linear:issue:{entity}:history")],
        },
    );
}

/// Resolves actual structured PR attachments; incomplete lists or failed reads stay unknown.
pub fn enrich_merged_attachments(
    config: &Config,
    batches: &mut [Evidence],
    github: &dyn GithubSource,
) {
    for batch in batches {
        for row in &mut batch.observations {
            let Fact::Issue(issue) = &mut row.fact else {
                continue;
            };
            let selected = (config.scope.teams.is_empty()
                || config.scope.teams.iter().any(|s| s.id == issue.team))
                && (config.scope.projects.is_empty()
                    || issue
                        .project
                        .as_ref()
                        .is_some_and(|id| config.scope.projects.iter().any(|s| &s.id == id)));
            if !selected {
                continue;
            }
            let result = (|| {
                if !issue.attachments_complete {
                    return Err("incomplete native issue attachment list".into());
                }
                let mut merged = false;
                for url in &issue.attachment_urls {
                    if let Some((repo, number)) = crate::retro::metrics::pull_request_ref(url) {
                        merged |= github.pull_request(&repo, number)?.merged;
                    }
                }
                Ok::<bool, String>(merged)
            })();
            issue.merged_pr = result.as_ref().ok().copied();
            issue.provenance.insert(
                "merged_pr".into(),
                SourceStamp {
                    source: batch.source.clone(),
                    observed_at: batch.observed_at.clone(),
                    complete: result.is_ok(),
                    reason: result.err(),
                    evidence_refs: issue.attachment_urls.clone(),
                },
            );
        }
    }
}

/// Reuses reviewer marker counting with full repository identity and complete-empty semantics.
/// Malformed attributed rows remain unknown for their possible PRs; old metric formulas stay intact.
pub fn enrich_reviewer_rounds(batches: &mut [Evidence], lines: &[String], repositories: &[String]) {
    use std::collections::{BTreeMap, BTreeSet};
    for repository in repositories {
        let allowed: BTreeSet<String> = batches
            .iter()
            .flat_map(|b| &b.observations)
            .filter_map(|o| match &o.fact {
                Fact::PullRequest(p) if &p.repo == repository => {
                    o.entity_id.rsplit_once('/').map(|(_, key)| key.into())
                }
                _ => None,
            })
            .collect();
        let rounds = crate::retro::metrics::fix_rounds_by_pr(
            lines,
            &allowed,
            std::slice::from_ref(repository),
        );
        let mut missing: BTreeSet<String> = BTreeSet::new();
        let mut reviews: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for line in lines {
            let Some((kind, fields)) = crate::retro::metrics::marker_fields(line) else {
                continue;
            };
            if fields.get("repo").is_some_and(|repo| *repo != repository) {
                continue;
            }
            if kind == "reviewer"
                && let Some(pr) = fields.get("pr").filter(|pr| allowed.contains(**pr))
            {
                match (fields.get("repo"), fields.get("id")) {
                    (Some(_), Some(id)) => {
                        reviews
                            .entry((*id).into())
                            .or_default()
                            .insert((*pr).into());
                    }
                    _ => {
                        missing.insert((*pr).into());
                    }
                }
            }
        }
        for line in lines {
            let Some((kind, fields)) = crate::retro::metrics::marker_fields(line) else {
                continue;
            };
            if kind != "reviewer-dispositions"
                || fields.get("repo").is_some_and(|repo| *repo != repository)
            {
                continue;
            }
            if let Some(id) = fields.get("id")
                && let Some(prs) = reviews.get(*id)
                && (!fields.contains_key("repo")
                    || fields
                        .get("round")
                        .and_then(|r| r.parse::<u64>().ok())
                        .is_none())
            {
                missing.extend(prs.iter().cloned());
            }
        }
        for row in batches.iter_mut().flat_map(|b| &mut b.observations) {
            if let Fact::PullRequest(p) = &mut row.fact
                && &p.repo == repository
                && let Some((_, key)) = row.entity_id.rsplit_once('/')
            {
                p.fix_rounds =
                    (!missing.contains(key)).then(|| rounds.get(key).copied().unwrap_or(0));
            }
        }
    }
}
