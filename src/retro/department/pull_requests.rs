// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Native GitHub projections, complete pagination, and attributed dependency-diff classification.
//! The HTTP port receives an explicit jq projection; live and fixture readers decode the same
//! strict payload, excluding unrelated upstream fields and private review bodies.
use super::{Attribution, Config, Fact, Observation, PrFact};
use crate::retro::sources::{PAGE_LIMIT, PullRequest};
use serde::Deserialize;

/// Release of the deterministic Cargo dependency revision classifier.
pub const DIFF_CLASSIFIER: &str = "cargo-revisions-v1";
/// jq projection for a native PR detail response.
pub const PR_DETAILS: &str = "{base_sha:.base.sha,head_sha:.head.sha,requested_reviewers:(.requested_reviewers|length),requested_teams:(.requested_teams|length)}";
/// jq projection for review pages; bodies and author identities are excluded.
pub const PR_REVIEWS: &str = "map({id,state,submitted_at})";
/// jq projection for timeline pages; unrelated event-specific fields are excluded.
pub const PR_TIMELINE: &str = "map({id,event,created_at})";
/// jq projection for changed-file pages; contents are fetched separately at exact revisions.
pub const PR_FILES: &str = "map({filename,status})";
/// Sentinel selecting an unencoded raw GitHub contents response.
pub const RAW_CONTENT: &str = "raw";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Details {
    base_sha: String,
    head_sha: String,
    requested_reviewers: u64,
    requested_teams: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    id: u64,
    state: String,
    submitted_at: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timeline {
    id: Option<u64>,
    event: Option<String>,
    created_at: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    filename: String,
    status: String,
}

/// Normalizes real GitHub detail/history/file responses without guessing review readiness.
/// Independent unavailable fields preserve every other successfully read attribute.
pub fn collect_pull_request(
    config: &Config,
    pr: &PullRequest,
    observed_at: &str,
    mut get: impl FnMut(&str, &str) -> Result<String, String>,
) -> Observation {
    let id = format!("{}#{}", pr.repo, pr.number);
    let endpoint = format!("repos/{}/pulls/{}", pr.repo, pr.number);
    let mut fact = PrFact {
        provenance: Default::default(),
        repo: pr.repo.clone(),
        open: pr.open,
        draft: pr.draft,
        merged_at: pr.merged_at.map(crate::drift::format_timestamp),
        review_ready_at: None,
        assigned_reviewers: None,
        qualifying_reviews: None,
        repin_only: None,
        fix_rounds: None,
        full_gates: None,
    };
    let details = get(&endpoint, PR_DETAILS)
        .and_then(|body| serde_json::from_str::<Details>(&body).map_err(|e| e.to_string()))
        .ok();
    if let Some(details) = &details {
        fact.assigned_reviewers = details
            .requested_reviewers
            .checked_add(details.requested_teams);
    }
    if let Ok(reviews) = pages::<Review>(&format!("{endpoint}/reviews"), PR_REVIEWS, &mut get)
        && !config.parameters.qualifying_review_states.is_empty()
    {
        let mut seen = std::collections::BTreeSet::new();
        fact.qualifying_reviews = u64::try_from(
            reviews
                .iter()
                .filter(|review| {
                    seen.insert(review.id)
                        && review.submitted_at.as_deref().is_some_and(valid_timestamp)
                        && config
                            .parameters
                            .qualifying_review_states
                            .contains(&review.state)
                })
                .count(),
        )
        .ok();
    }
    if let Ok(events) = pages::<Timeline>(
        &format!("repos/{}/issues/{}/timeline", pr.repo, pr.number),
        PR_TIMELINE,
        &mut get,
    ) {
        let mut ready: Option<(f64, String)> = None;
        for event in events {
            if event.id.is_some()
                && event.event.as_deref() == Some("ready_for_review")
                && let Some(at) = event.created_at
                && let Ok(epoch) = crate::drift::timestamp_epoch(&at)
                && ready.as_ref().is_none_or(|(old, _)| epoch > *old)
            {
                ready = Some((epoch, at));
            }
        }
        fact.review_ready_at = ready.map(|(_, at)| at);
    }
    let mut attribution = None;
    if config.parameters.diff_classifier.as_deref() == Some(DIFF_CLASSIFIER)
        && let Some(details) = details
        && valid_sha(&details.base_sha)
        && valid_sha(&details.head_sha)
        && let Ok(files) = pages::<File>(&format!("{endpoint}/files"), PR_FILES, &mut get)
    {
        let classified = classify_diff(&pr.repo, &details, &files, &mut get);
        if let Ok(repin_only) = classified {
            fact.repin_only = Some(repin_only);
            attribution = Some(Attribution {
                actor: "ix-board".into(),
                classifier_version: DIFF_CLASSIFIER.into(),
                offset_start: 0,
                offset_end: u64::try_from(files.len()).unwrap_or(u64::MAX),
            });
        }
    }
    Observation {
        event_id: format!(
            "github:pull:{id}:{}",
            crate::drift::format_timestamp(pr.updated_at)
        ),
        entity_id: id,
        event_at: Some(crate::drift::format_timestamp(pr.updated_at)),
        observed_at: Some(observed_at.into()),
        sequence: None,
        scope: config.scope.clone(),
        complete: true,
        source_ref: format!("https://github.com/{}/pull/{}", pr.repo, pr.number),
        attribution,
        fact: Fact::PullRequest(fact),
    }
}
fn valid_timestamp(at: &str) -> bool {
    crate::drift::timestamp_epoch(at).is_ok()
}
fn valid_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit())
}
fn pages<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    projection: &str,
    get: &mut impl FnMut(&str, &str) -> Result<String, String>,
) -> Result<Vec<T>, String> {
    let mut all = Vec::new();
    for page in 1..=PAGE_LIMIT {
        let body = get(&format!("{endpoint}?per_page=100&page={page}"), projection)?;
        let rows: Vec<T> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if rows.len() > 100 {
            return Err("invalid native page size".into());
        }
        let last = rows.len() < 100;
        all.extend(rows);
        if last {
            return Ok(all);
        }
    }
    Err("native history stopped before final page".into())
}
fn classify_diff(
    repo: &str,
    details: &Details,
    files: &[File],
    get: &mut impl FnMut(&str, &str) -> Result<String, String>,
) -> Result<bool, String> {
    if files.is_empty() {
        return Ok(false);
    }
    let mut changed = false;
    for file in files {
        let path = &file.filename;
        if !path.split('/').all(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        }) {
            return Err("invalid native changed-file path".into());
        }
        let name = path.rsplit('/').next().ok_or("missing changed filename")?;
        if !matches!(name, "Cargo.toml" | "Cargo.lock") {
            return Ok(false);
        }
        if file.status != "modified" {
            return Err(
                "dependency file addition/removal requires original resolution evidence".into(),
            );
        }
        let before = get(
            &format!("repos/{repo}/contents/{path}?ref={}", details.base_sha),
            RAW_CONTENT,
        )?;
        let after = get(
            &format!("repos/{repo}/contents/{path}?ref={}", details.head_sha),
            RAW_CONTENT,
        )?;
        let mut before: toml::Value = toml::from_str(&before).map_err(|e| e.to_string())?;
        let mut after: toml::Value = toml::from_str(&after).map_err(|e| e.to_string())?;
        if before == after {
            continue;
        }
        changed = true;
        if name == "Cargo.toml" {
            strip_manifest_revisions(&mut before);
            strip_manifest_revisions(&mut after);
        } else {
            strip_lock_resolution(&mut before)?;
            strip_lock_resolution(&mut after)?;
        }
        if before != after {
            return Ok(false);
        }
    }
    Ok(changed)
}
fn strip_dependency_tables(value: &mut toml::Value) {
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dependencies) = value.get_mut(key).and_then(toml::Value::as_table_mut) {
            for dependency in dependencies.values_mut() {
                if let Some(table) = dependency.as_table_mut() {
                    for field in ["rev", "branch", "tag", "version"] {
                        table.remove(field);
                    }
                } else if dependency.is_str() {
                    *dependency = toml::Value::String(String::new());
                }
            }
        }
    }
}
fn strip_manifest_revisions(value: &mut toml::Value) {
    strip_dependency_tables(value);
    if let Some(workspace) = value.get_mut("workspace") {
        strip_dependency_tables(workspace);
    }
    if let Some(targets) = value.get_mut("target").and_then(toml::Value::as_table_mut) {
        for target in targets.values_mut() {
            strip_dependency_tables(target);
        }
    }
}
fn strip_lock_resolution(value: &mut toml::Value) -> Result<(), String> {
    let packages = value
        .get_mut("package")
        .and_then(toml::Value::as_array_mut)
        .ok_or("Cargo.lock.package missing")?;
    // Source-bearing packages are dependency resolutions. Workspace package versions remain
    // substantive, so a release-version edit cannot masquerade as a dependency repin.
    packages.retain(|package| package.get("source").is_none());
    for package in packages {
        if let Some(table) = package.as_table_mut() {
            table.remove("dependencies");
        }
    }
    Ok(())
}
