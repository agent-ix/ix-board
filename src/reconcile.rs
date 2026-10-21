// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only reconciliation between Linear issue state and GitHub pull requests.

use crate::program_review::ProgramScope;
use crate::{Error, Issue, Project, ProjectId, WorkflowStateType, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};
use thiserror::Error as ThisError;

pub const DEFAULT_STALE_DAYS: u32 = 14;

#[derive(Debug, ThisError)]
pub enum ReconcileError {
    #[error("GitHub {operation} failed for {repository}: {message}")]
    Github {
        repository: String,
        operation: &'static str,
        message: String,
    },
    #[error("invalid timestamp in reconciliation evidence: {0}")]
    Timestamp(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Linear(#[from] Error),
}

impl From<ReconcileError> for Error {
    fn from(error: ReconcileError) -> Self {
        Self::ProgramReview {
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcilePullRequest {
    pub number: u64,
    pub url: String,
    pub state: String,
    #[serde(rename = "isDraft")]
    pub is_draft: bool,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "mergedAt")]
    pub merged_at: Option<String>,
    #[serde(rename = "headRefName")]
    pub head_ref_name: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileTicketFinding {
    pub identifier: String,
    pub title: String,
    pub state: String,
    pub age_days: Option<i64>,
    pub pull_requests: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcilePullRequestFinding {
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub state: String,
    pub age_days: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileFindings {
    pub merged_pr_open_ticket: Vec<ReconcileTicketFinding>,
    pub done_without_merged_pr: Vec<ReconcileTicketFinding>,
    pub in_progress_without_pr_or_branch: Vec<ReconcileTicketFinding>,
    pub stale_open_pr: Vec<ReconcilePullRequestFinding>,
    pub draft_pr_closed_ticket: Vec<ReconcilePullRequestFinding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileReport {
    pub project: String,
    pub project_id: ProjectId,
    pub stale_days: u32,
    pub searched_repositories: Vec<String>,
    pub warnings: Vec<String>,
    pub findings: ReconcileFindings,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileEvidence {
    pub pull_requests: BTreeMap<String, Vec<ReconcilePullRequest>>,
    pub branches: BTreeMap<String, Vec<String>>,
}

/// Resolve a Linear project by exact identifier or case-insensitive exact name.
pub fn resolve_project<'a>(
    snapshot: &'a WorkspaceSnapshot,
    selector: &str,
) -> Result<&'a Project, ReconcileError> {
    let matches = snapshot
        .projects
        .iter()
        .filter(|project| {
            project.identifier.as_deref() == Some(selector)
                || project.name.eq_ignore_ascii_case(selector)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [project] => Ok(project),
        [] => Err(ReconcileError::Invalid(format!(
            "project not found: {selector}"
        ))),
        _ => Err(ReconcileError::Invalid(format!(
            "project selector is ambiguous: {selector}"
        ))),
    }
}

/// Collect current PR and branch records through the authenticated GitHub CLI.
pub fn collect_github_evidence(
    repositories: impl IntoIterator<Item = String>,
) -> Result<ReconcileEvidence, ReconcileError> {
    let repositories = repositories.into_iter().collect::<BTreeSet<_>>();
    let mut evidence = ReconcileEvidence::default();
    for repository in repositories {
        let prs_output = Command::new("gh")
            .args([
                "pr",
                "list",
                "--repo",
                &repository,
                "--state",
                "all",
                "--limit",
                "1000",
                "--json",
                "number,url,state,isDraft,updatedAt,mergedAt,headRefName,title",
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                github_error(&repository, "pull request listing", error.to_string())
            })?;
        if !prs_output.status.success() {
            return Err(github_error(
                &repository,
                "pull request listing",
                String::from_utf8_lossy(&prs_output.stderr)
                    .trim()
                    .to_owned(),
            ));
        }
        let pull_requests = serde_json::from_slice(&prs_output.stdout).map_err(|error| {
            github_error(&repository, "pull request decoding", error.to_string())
        })?;

        let branches_output = Command::new("gh")
            .args([
                "api",
                "--paginate",
                &format!("repos/{repository}/branches"),
                "--jq",
                ".[].name",
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| github_error(&repository, "branch listing", error.to_string()))?;
        if !branches_output.status.success() {
            return Err(github_error(
                &repository,
                "branch listing",
                String::from_utf8_lossy(&branches_output.stderr)
                    .trim()
                    .to_owned(),
            ));
        }
        let branch_names = String::from_utf8_lossy(&branches_output.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        evidence
            .pull_requests
            .insert(repository.clone(), pull_requests);
        evidence.branches.insert(repository, branch_names);
    }
    Ok(evidence)
}

fn github_error(repository: &str, operation: &'static str, message: String) -> ReconcileError {
    ReconcileError::Github {
        repository: repository.to_owned(),
        operation,
        message,
    }
}

/// Build grouped findings from complete Linear and GitHub records.
pub fn evaluate_reconcile(
    project: &Project,
    issues: &[Issue],
    evidence: &ReconcileEvidence,
    repositories: Vec<String>,
    stale_days: u32,
    now_epoch: f64,
) -> Result<ReconcileReport, ReconcileError> {
    let project_issues = issues
        .iter()
        .filter(|issue| issue.project_id.as_ref() == Some(&project.id))
        .collect::<Vec<_>>();
    let mut findings = ReconcileFindings::default();
    for issue in &project_issues {
        let ticket_prs = all_prs_for_issue(issue, evidence);
        let merged = ticket_prs
            .iter()
            .filter(|(_, pr)| pr.merged_at.is_some())
            .collect::<Vec<_>>();
        let pull_request_urls = ticket_prs
            .iter()
            .map(|(_, pr)| pr.url.clone())
            .collect::<Vec<_>>();
        let issue_is_done = issue.state_type == WorkflowStateType::Completed;

        if !ticket_prs.is_empty()
            && merged.len() == ticket_prs.len()
            && !is_terminal(issue.state_type)
        {
            findings.merged_pr_open_ticket.push(ReconcileTicketFinding {
                identifier: issue.identifier.clone(),
                title: issue.title.clone(),
                state: issue.state_name.clone(),
                age_days: None,
                pull_requests: merged.iter().map(|(_, pr)| pr.url.clone()).collect(),
            });
        }
        if issue_is_done && merged.is_empty() && issue.attachments_complete {
            findings
                .done_without_merged_pr
                .push(ReconcileTicketFinding {
                    identifier: issue.identifier.clone(),
                    title: issue.title.clone(),
                    state: issue.state_name.clone(),
                    age_days: None,
                    pull_requests: pull_request_urls,
                });
        }

        for (repository, pr) in &ticket_prs {
            let age_days = elapsed_days(&pr.updated_at, now_epoch)?;
            if pr.state.eq_ignore_ascii_case("open") && age_days as f64 >= f64::from(stale_days) {
                findings.stale_open_pr.push(ReconcilePullRequestFinding {
                    identifier: issue.identifier.clone(),
                    title: issue.title.clone(),
                    url: pr.url.clone(),
                    state: format!("OPEN in {repository}"),
                    age_days,
                });
            }
            if is_terminal(issue.state_type) && pr.is_draft && pr.state.eq_ignore_ascii_case("open")
            {
                findings
                    .draft_pr_closed_ticket
                    .push(ReconcilePullRequestFinding {
                        identifier: issue.identifier.clone(),
                        title: issue.title.clone(),
                        url: pr.url.clone(),
                        state: format!("OPEN draft in {repository}"),
                        age_days,
                    });
            }
        }

        if is_coding_or_in_progress(issue)
            && ticket_prs.is_empty()
            && issue.attachments_complete
            && !repositories.is_empty()
            && !has_matching_branch(issue, &repositories, evidence)
        {
            let age_days = issue
                .updated_at
                .as_deref()
                .or(issue.created_at.as_deref())
                .map(|timestamp| elapsed_days(timestamp, now_epoch))
                .transpose()?;
            if age_days.is_some_and(|age| age as f64 > f64::from(stale_days)) {
                findings
                    .in_progress_without_pr_or_branch
                    .push(ReconcileTicketFinding {
                        identifier: issue.identifier.clone(),
                        title: issue.title.clone(),
                        state: issue.state_name.clone(),
                        age_days,
                        pull_requests: Vec::new(),
                    });
            }
        }
    }

    for values in [
        &mut findings.merged_pr_open_ticket,
        &mut findings.done_without_merged_pr,
        &mut findings.in_progress_without_pr_or_branch,
    ] {
        values.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    }
    findings.stale_open_pr.sort_by(|left, right| {
        left.url
            .cmp(&right.url)
            .then(left.identifier.cmp(&right.identifier))
    });
    findings.draft_pr_closed_ticket.sort_by(|left, right| {
        left.url
            .cmp(&right.url)
            .then(left.identifier.cmp(&right.identifier))
    });

    Ok(ReconcileReport {
        project: project
            .identifier
            .clone()
            .unwrap_or_else(|| project.name.clone()),
        project_id: project.id.clone(),
        stale_days,
        searched_repositories: repositories,
        warnings: {
            let mut warnings = Vec::new();
            if evidence.branches.is_empty() {
                warnings.push("no repositories available for branch search; pass --scope when the project has no linked PRs".to_owned());
            }
            if project_issues
                .iter()
                .any(|issue| !issue.attachments_complete)
            {
                warnings.push("one or more Linear issues have incomplete attachment data; absence-of-PR findings may be omitted".to_owned());
            }
            warnings
        },
        findings,
    })
}

fn all_prs_for_issue<'a>(
    issue: &Issue,
    evidence: &'a ReconcileEvidence,
) -> Vec<(&'a str, &'a ReconcilePullRequest)> {
    let attached = issue
        .attachment_urls
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    for (repository, pull_requests) in &evidence.pull_requests {
        for pull_request in pull_requests {
            let linked = attached.contains(pull_request.url.as_str());
            let ticket_match = contains_identifier(&pull_request.head_ref_name, &issue.identifier)
                || contains_identifier(&pull_request.title, &issue.identifier);
            if linked || ticket_match {
                result.push((repository.as_str(), pull_request));
            }
        }
    }
    result
}

fn has_matching_branch(
    issue: &Issue,
    repositories: &[String],
    evidence: &ReconcileEvidence,
) -> bool {
    repositories.iter().any(|repository| {
        evidence.branches.get(repository).is_some_and(|branches| {
            branches
                .iter()
                .any(|branch| contains_identifier(branch, &issue.identifier))
        })
    })
}

fn contains_identifier(value: &str, identifier: &str) -> bool {
    let value = value.to_ascii_lowercase();
    let identifier = identifier.to_ascii_lowercase();
    value.match_indices(&identifier).any(|(start, _)| {
        let end = start + identifier.len();
        let left_boundary = start == 0
            || !value[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric());
        let right_boundary = end == value.len()
            || !value[end..]
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric());
        left_boundary && right_boundary
    })
}

fn is_terminal(state_type: WorkflowStateType) -> bool {
    matches!(
        state_type,
        WorkflowStateType::Completed | WorkflowStateType::Canceled | WorkflowStateType::Duplicate
    )
}

fn is_coding_or_in_progress(issue: &Issue) -> bool {
    issue.state_type == WorkflowStateType::Started
        && [issue.state_name.as_str(), issue.state.as_str()]
            .iter()
            .any(|state| {
                state.eq_ignore_ascii_case("coding") || state.eq_ignore_ascii_case("in progress")
            })
}

fn elapsed_days(timestamp: &str, now_epoch: f64) -> Result<i64, ReconcileError> {
    let epoch = crate::drift::timestamp_epoch(timestamp)
        .map_err(|_| ReconcileError::Timestamp(timestamp.to_owned()))?;
    Ok(((now_epoch - epoch) / 86_400.0).floor().max(0.0) as i64)
}

/// Scope repositories are used for branch search in addition to repositories attached to PRs.
pub fn repositories_for_scope(scope: Option<&ProgramScope>, issues: &[Issue]) -> BTreeSet<String> {
    let mut repositories = scope
        .into_iter()
        .flat_map(|scope| scope.repos.iter().map(|repository| repository.name.clone()))
        .collect::<BTreeSet<_>>();
    repositories.extend(
        issues
            .iter()
            .flat_map(|issue| issue.attachment_urls.iter())
            .filter_map(|url| pull_request_repository(url)),
    );
    repositories
}

fn pull_request_repository(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https" || url.host_str()? != "github.com" {
        return None;
    }
    let parts = url.path_segments()?.collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" || parts[3].parse::<u64>().is_err() {
        return None;
    }
    Some(format!("{}/{}", parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectId, TeamKey};

    fn project() -> Project {
        Project {
            id: ProjectId("project-reconcile".to_owned()),
            identifier: Some("SWM".to_owned()),
            name: "Program Review".to_owned(),
            description: String::new(),
            content: None,
            updated_at: None,
            status: "Started".to_owned(),
            priority: 0,
            sort_order: 0.0,
            target_date: None,
            team_keys: vec![TeamKey("SWM".to_owned())],
            milestones: Vec::new(),
            initiatives: Vec::new(),
        }
    }

    fn recorded_evidence() -> (Vec<Issue>, ReconcileEvidence) {
        let issues = serde_json::from_str(include_str!(
            "../tests/fixtures/reconcile/linear-project-issues.json"
        ))
        .expect("recorded Linear project issues");
        let pull_requests = serde_json::from_str(include_str!(
            "../tests/fixtures/reconcile/github-pull-requests.json"
        ))
        .expect("recorded gh pull request output");
        let branches = serde_json::from_str(include_str!(
            "../tests/fixtures/reconcile/github-branches.json"
        ))
        .expect("recorded gh branch output");
        (
            issues,
            ReconcileEvidence {
                pull_requests: BTreeMap::from([(
                    "agent-ix/reconcile-demo".to_owned(),
                    pull_requests,
                )]),
                branches: BTreeMap::from([("agent-ix/reconcile-demo".to_owned(), branches)]),
            },
        )
    }

    fn report(evidence: &ReconcileEvidence) -> ReconcileReport {
        let (issues, _) = recorded_evidence();
        let now = crate::drift::timestamp_epoch("2026-10-21T00:00:00Z").unwrap();
        evaluate_reconcile(
            &project(),
            &issues,
            evidence,
            vec!["agent-ix/reconcile-demo".to_owned()],
            DEFAULT_STALE_DAYS,
            now,
        )
        .unwrap()
    }

    #[test]
    fn recorded_linear_and_github_fixtures_cover_every_reconcile_finding() {
        let (issues, evidence) = recorded_evidence();
        assert_eq!(issues.len(), 5);
        let report = report(&evidence);
        assert_eq!(report.findings.merged_pr_open_ticket.len(), 1);
        assert_eq!(
            report.findings.merged_pr_open_ticket[0].identifier,
            "SWM-101"
        );
        assert_eq!(report.findings.done_without_merged_pr.len(), 2);
        assert_eq!(
            report
                .findings
                .done_without_merged_pr
                .iter()
                .map(|finding| finding.identifier.as_str())
                .collect::<Vec<_>>(),
            ["SWM-102", "SWM-105"]
        );
        assert_eq!(report.findings.in_progress_without_pr_or_branch.len(), 1);
        assert_eq!(
            report.findings.in_progress_without_pr_or_branch[0].identifier,
            "SWM-103"
        );
        assert_eq!(report.findings.stale_open_pr.len(), 1);
        assert_eq!(report.findings.stale_open_pr[0].identifier, "SWM-104");
        assert_eq!(report.findings.draft_pr_closed_ticket.len(), 1);
        assert_eq!(
            report.findings.draft_pr_closed_ticket[0].identifier,
            "SWM-105"
        );
    }

    #[test]
    fn matching_branch_suppresses_the_no_pr_or_branch_finding() {
        let (_, mut evidence) = recorded_evidence();
        evidence
            .branches
            .get_mut("agent-ix/reconcile-demo")
            .unwrap()
            .push("feature/SWM-103-work".to_owned());
        let report = report(&evidence);
        assert!(report.findings.in_progress_without_pr_or_branch.is_empty());
    }

    #[test]
    fn started_states_other_than_coding_or_in_progress_are_not_flagged() {
        let (mut issues, evidence) = recorded_evidence();
        let issue = issues
            .iter_mut()
            .find(|issue| issue.identifier == "SWM-103")
            .unwrap();
        issue.state_name = "In Review".to_owned();
        let now = crate::drift::timestamp_epoch("2026-10-21T00:00:00Z").unwrap();
        let report = evaluate_reconcile(
            &project(),
            &issues,
            &evidence,
            vec!["agent-ix/reconcile-demo".to_owned()],
            DEFAULT_STALE_DAYS,
            now,
        )
        .unwrap();
        assert!(report.findings.in_progress_without_pr_or_branch.is_empty());
    }

    #[test]
    fn project_scope_repositories_include_repos_from_linked_pr_attachments() {
        let (issues, _) = recorded_evidence();
        assert_eq!(
            repositories_for_scope(None, &issues),
            BTreeSet::from(["agent-ix/reconcile-demo".to_owned()])
        );
    }
}
