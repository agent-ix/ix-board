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
