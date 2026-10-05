// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic hygiene findings and focus snapshot differences.

use crate::{Issue, IssueId, ProjectId, WorkflowStateType, WorkspaceSnapshot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum number of title pairs considered before match thresholds.
pub const TITLE_PAIR_LIMIT: usize = 100_000;

/// Whether a report group has complete evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupStatus {
    /// Every required source was available.
    Available,
    /// One or more required sources were unavailable.
    Unavailable,
}

/// Availability and findings for one hygiene check.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HygieneGroup<T> {
    /// Whether this check had complete evidence.
    pub availability: GroupStatus,
    /// Reasons the check could not be complete.
    pub reasons: Vec<String>,
    /// Findings proven by the available records.
    pub findings: Vec<T>,
}

/// Issue with unassigned project or milestone scope.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissingScopeFinding {
    /// Stable Linear issue ID.
    pub issue_id: IssueId,
    /// Fields absent from the issue.
    pub missing_fields: Vec<String>,
}

/// Overdue project milestone with its remaining open issues.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverdueMilestoneFinding {
    /// Stable project ID.
    pub project_id: ProjectId,
    /// Milestone ID.
    pub milestone_id: String,
    /// Milestone display name.
    pub name: String,
    /// Target date from Linear.
    pub target_date: String,
    /// Sorted open issue IDs.
    pub open_issue_ids: Vec<IssueId>,
}

/// Issue older than the configured activity threshold.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaleIssueFinding {
    /// Stable Linear issue ID.
    pub issue_id: IssueId,
    /// Last updated timestamp.
    pub updated_at: String,
    /// Whole elapsed days.
    pub elapsed_days: i64,
}

/// Connected component of similar issue titles.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateTitleFinding {
    /// Component issue IDs, sorted by stable ID.
    pub issue_ids: Vec<IssueId>,
    /// Original title strings in issue ID order.
    pub titles: Vec<String>,
}

/// A linked pull request whose state disagrees with its issue.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PullRequestFinding {
    /// Stable Linear issue ID.
    pub issue_id: IssueId,
    /// GitHub repository in owner/name form.
    pub repository: String,
    /// GitHub pull request number.
    pub number: u64,
    /// Structured Linear attachment URL.
    pub url: String,
    /// Structured mismatch classification.
    pub kind: PullRequestMismatchKind,
    /// Current GitHub pull request state.
    pub state: String,
    /// Merge timestamp if present.
    pub merged_at: Option<String>,
}

/// Human-readable Linear identity for an issue referenced by a hygiene finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueReference {
    /// Stable Linear issue identifier.
    pub identifier: String,
    /// Current issue title.
    pub title: String,
}

/// PR and issue state mismatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestMismatchKind {
    /// Issue is complete while its pull request is open.
    DoneWithOpenPr,
    /// Issue remains open although its pull request merged.
    OpenWithMergedPr,
}

impl PullRequestMismatchKind {
    /// Stable finding identifier for terminal output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DoneWithOpenPr => "done_with_open_pr",
            Self::OpenWithMergedPr => "open_with_merged_pr",
        }
    }
}

/// Focus membership transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FocusMembershipChange {
    /// Issue entered the current focus.
    EnteredFocus,
    /// Issue left the current focus.
    LeftFocus,
}

/// A previous or current focus issue record used by the diff.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusBaselineIssue {
    /// Stable Linear issue ID.
    pub id: IssueId,
    /// Whether it belonged to the focus at this report.
    pub in_focus: bool,
    /// Workflow display name.
    pub state: String,
    /// Workflow category.
    pub state_type: WorkflowStateType,
    /// Project ID, when assigned.
    pub project_id: Option<ProjectId>,
    /// Milestone ID, when assigned.
    pub milestone_id: Option<String>,
    /// Sorted blocker IDs.
    pub blocked_by: Vec<IssueId>,
}

/// Successful report state stored for one workspace and canonical focus.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusBaseline {
    /// RFC 3339 UTC time of the prior successful report.
    pub report_time: String,
    /// Complete records for all issues in the prior focus.
    pub issues: Vec<FocusBaselineIssue>,
}

/// One kind of change for a focus issue.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FocusChange {
    /// The issue entered or left focus.
    Membership {
        issue_id: IssueId,
        change: FocusMembershipChange,
    },
    /// An issue's workflow state changed.
    State {
        issue_id: IssueId,
        old: String,
        old_category: WorkflowStateType,
        current: String,
        current_category: WorkflowStateType,
    },
    /// An issue's project changed.
    Project {
        issue_id: IssueId,
        old: Option<ProjectId>,
        current: Option<ProjectId>,
    },
    /// An issue's milestone changed.
    Milestone {
        issue_id: IssueId,
        old: Option<String>,
        current: Option<String>,
    },
    /// An issue's blockers changed.
    Blockers {
        issue_id: IssueId,
        old: Vec<IssueId>,
        current: Vec<IssueId>,
    },
    /// An issue was created after the previous report.
    Created { issue_id: IssueId },
    /// An issue entered a completed or canceled state.
    Closed { issue_id: IssueId },
}

/// Calculated hygiene groups plus their current per-focus baseline.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FocusHygieneReport {
    /// RFC 3339 UTC report time.
    pub report_time: String,
    /// Canonical selectors encoded as kind/ID pairs.
    pub canonical_focus: Vec<String>,
    /// Stale threshold in days.
    pub stale_days: u32,
    /// Human-readable identities keyed by the UUIDs referenced in findings.
    pub issue_refs: BTreeMap<IssueId, IssueReference>,
    /// Issues missing project or milestone assignment.
    pub missing_scope: HygieneGroup<MissingScopeFinding>,
    /// Overdue milestones with open focus issues.
    pub overdue_milestones: HygieneGroup<OverdueMilestoneFinding>,
    /// Linked pull requests with state mismatches or incomplete lookup evidence.
    pub pull_requests: HygieneGroup<PullRequestFinding>,
    /// Stale open issues.
    pub stale_issues: HygieneGroup<StaleIssueFinding>,
    /// Similar title components.
    pub near_duplicate_titles: HygieneGroup<DuplicateTitleFinding>,
    /// Changes since the previous successful report.
    pub diff: HygieneGroup<FocusChange>,
    /// Current snapshot to persist after a complete Linear diff.
    #[serde(skip)]
    pub baseline: Vec<FocusBaselineIssue>,
    #[serde(skip)]
    pub(crate) baseline_advance: bool,
}

/// Failure to build a complete hygiene report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HygieneError {
    /// The deterministic title candidate cap was exceeded.
    ResourceLimit { limit: usize },
    /// Required focus record data was unavailable.
    IncompleteSnapshot { issue: String },
    /// The report timestamp or threshold was invalid.
    InvalidTime,
}

/// Build a deterministic report from a complete Linear snapshot and optional successful baseline.
pub fn build_report(
    snapshot: &WorkspaceSnapshot,
    focused_ids: &BTreeSet<IssueId>,
    canonical_focus: Vec<String>,
    report_time: &str,
    stale_days: u32,
    prior: Option<(&str, &[FocusBaselineIssue])>,
    prior_unavailable: Option<String>,
) -> Result<FocusHygieneReport, HygieneError> {
    let Some(report_epoch) = timestamp_epoch(report_time) else {
        return Err(HygieneError::InvalidTime);
    };
    if stale_days == 0 {
        return Err(HygieneError::InvalidTime);
    }
    let issues: BTreeMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let focused = focused_ids
        .iter()
        .map(|id| {
            issues
                .get(id)
                .copied()
                .ok_or_else(|| HygieneError::IncompleteSnapshot {
                    issue: id.0.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let today = report_time.get(..10).ok_or(HygieneError::InvalidTime)?;

    let missing_scope = focused
        .iter()
        .filter_map(|issue| {
            let mut missing_fields = Vec::new();
            if issue.project_id.is_none() {
                missing_fields.push("project".to_owned());
            }
            if issue.project_milestone_id.is_none() {
                missing_fields.push("milestone".to_owned());
            }
            (!missing_fields.is_empty()).then(|| MissingScopeFinding {
                issue_id: issue.id.clone(),
                missing_fields,
            })
        })
        .collect();

    let mut overdue = Vec::new();
    for project in &snapshot.projects {
        for milestone in &project.milestones {
            let Some(target_date) = milestone.target_date.as_deref() else {
                continue;
            };
            if target_date >= today {
                continue;
            }
            let mut open_issue_ids = focused
                .iter()
                .filter(|issue| {
                    issue.project_id.as_ref() == Some(&project.id)
                        && issue.project_milestone_id.as_deref() == Some(milestone.id.as_str())
                        && !terminal(issue)
                })
                .map(|issue| issue.id.clone())
                .collect::<Vec<_>>();
            open_issue_ids.sort();
            if !open_issue_ids.is_empty() {
                overdue.push(OverdueMilestoneFinding {
                    project_id: project.id.clone(),
                    milestone_id: milestone.id.clone(),
                    name: milestone.name.clone(),
                    target_date: target_date.to_owned(),
                    open_issue_ids,
                });
            }
        }
    }
    overdue.sort_by(|left, right| {
        left.project_id
            .cmp(&right.project_id)
            .then(left.milestone_id.cmp(&right.milestone_id))
    });

    let threshold = i64::from(stale_days)
        .checked_mul(86_400)
        .ok_or(HygieneError::InvalidTime)?;
    let mut stale = Vec::new();
    let mut stale_reasons = Vec::new();
    for issue in focused.iter().filter(|issue| !terminal(issue)) {
        let Some(updated_at) = issue.updated_at.as_deref() else {
            stale_reasons.push(format!("updatedAt unavailable for {}", issue.identifier));
            continue;
        };
        let Some(updated_epoch) = timestamp_epoch(updated_at) else {
            stale_reasons.push(format!("updatedAt invalid for {}", issue.identifier));
            continue;
        };
        let elapsed = report_epoch.saturating_sub(updated_epoch);
        if elapsed > threshold {
            stale.push(StaleIssueFinding {
                issue_id: issue.id.clone(),
                updated_at: updated_at.to_owned(),
                elapsed_days: elapsed / 86_400,
            });
        }
    }

    let duplicates = duplicate_groups(&focused)?;
    let current_baseline = focused
        .iter()
        .map(|issue| baseline_issue(issue, true))
        .collect::<Vec<_>>();
    let diff_result = if prior_unavailable.is_some() {
        Ok(Vec::new())
    } else if let Some((prior_time, prior_issues)) = prior {
        compare_baselines(
            snapshot,
            focused_ids,
            prior_time,
            prior_issues,
            &current_baseline,
        )
    } else {
        Ok(Vec::new())
    };
    let diff_reasons = diff_result
        .as_ref()
        .err()
        .map(|error| vec![format!("{error:?}")])
        .unwrap_or_default();
    let diff = diff_result.unwrap_or_default();
    let created_time_unavailable = current_baseline
        .iter()
        .filter(|current| {
            prior.is_none_or(|(_, records)| !records.iter().any(|old| old.id == current.id))
        })
        .any(|current| {
            issues
                .get(&current.id)
                .and_then(|issue| issue.created_at.as_deref())
                .and_then(timestamp_epoch)
                .is_none()
        });
    let mut diff_reasons = prior_unavailable
        .into_iter()
        .chain(diff_reasons)
        .collect::<Vec<_>>();
    if created_time_unavailable {
        diff_reasons.push("createdAt unavailable or invalid for a newly focused issue".to_owned());
    }
    let baseline_advance = stale_reasons.is_empty() && diff_reasons.is_empty();
    let mut report = FocusHygieneReport {
        report_time: report_time.to_owned(),
        canonical_focus,
        stale_days,
        issue_refs: BTreeMap::new(),
        missing_scope: available(missing_scope),
        overdue_milestones: available(overdue),
        pull_requests: pull_request_group(&focused),
        stale_issues: group_with_reasons(stale, stale_reasons),
        near_duplicate_titles: available(duplicates),
        diff: if !diff_reasons.is_empty() {
            group_with_reasons(diff, diff_reasons)
        } else if prior.is_some() {
            available(diff)
        } else {
            HygieneGroup {
                availability: GroupStatus::Unavailable,
                reasons: vec!["no_prior_report".to_owned()],
                findings: Vec::new(),
            }
        },
        baseline: current_baseline,
        baseline_advance,
    };
    let mut referenced = BTreeSet::new();
    referenced.extend(
        report
            .missing_scope
            .findings
            .iter()
            .map(|finding| finding.issue_id.clone()),
    );
    referenced.extend(
        report
            .overdue_milestones
            .findings
            .iter()
            .flat_map(|finding| finding.open_issue_ids.iter().cloned()),
    );
    referenced.extend(
        report
            .pull_requests
            .findings
            .iter()
            .map(|finding| finding.issue_id.clone()),
    );
    referenced.extend(
        report
            .stale_issues
            .findings
            .iter()
            .map(|finding| finding.issue_id.clone()),
    );
    referenced.extend(
        report
            .near_duplicate_titles
            .findings
            .iter()
            .flat_map(|finding| finding.issue_ids.iter().cloned()),
    );
    referenced.extend(
        report
            .diff
            .findings
            .iter()
            .map(|finding| change_issue_id(finding).clone()),
    );
    report.issue_refs = referenced
        .into_iter()
        .filter_map(|id| {
            issues.get(&id).map(|issue| {
                (
                    id,
                    IssueReference {
                        identifier: issue.identifier.clone(),
                        title: issue.title.clone(),
                    },
                )
            })
        })
        .collect();
    Ok(report)
}

fn group_with_reasons<T>(findings: Vec<T>, reasons: Vec<String>) -> HygieneGroup<T> {
    HygieneGroup {
        availability: if reasons.is_empty() {
            GroupStatus::Available
        } else {
            GroupStatus::Unavailable
        },
        reasons,
        findings,
    }
}

fn pull_request_group(issues: &[&Issue]) -> HygieneGroup<PullRequestFinding> {
    let (linked, reasons) = linked_pull_requests(issues);
    if linked.is_empty() {
        return pull_request_group_result(Vec::new(), reasons, |_, _| Ok(None));
    }
    let token = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"));
    let Some(token) = token.ok().filter(|token| !token.trim().is_empty()) else {
        let unique = linked
            .iter()
            .map(|link| (&link.repository, link.number))
            .collect::<BTreeSet<_>>()
            .len();
        let mut reasons = reasons;
        reasons.push(format!(
            "GITHUB_TOKEN or GH_TOKEN is required to check {unique} linked pull requests"
        ));
        return group_with_reasons(Vec::new(), reasons);
    };
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return pull_request_group_result(linked, reasons, |_, _| {
                Err(format!("GitHub client unavailable: {error}"))
            });
        }
    };
    pull_request_group_with(issues, |repository, number| {
        let (owner, name) = repository
            .split_once('/')
            .ok_or_else(|| "invalid repository in pull request URL".to_owned())?;
        fetch_github_pull_request(
            &client,
            "https://api.github.com/graphql",
            &token,
            owner,
            name,
            number,
        )
    })
}

struct LinkedPullRequest<'a> {
    issue: &'a Issue,
    repository: String,
    number: u64,
    url: String,
}

fn linked_pull_requests<'a>(issues: &[&'a Issue]) -> (Vec<LinkedPullRequest<'a>>, Vec<String>) {
    let mut linked = Vec::new();
    let mut reasons = Vec::new();
    for issue in issues {
        if !issue.attachments_complete {
            reasons.push(format!("attachments incomplete for {}", issue.identifier));
        }
        for url in &issue.attachment_urls {
            match parse_pull_url(url) {
                Some((repository, number)) => linked.push(LinkedPullRequest {
                    issue,
                    repository,
                    number,
                    url: url.clone(),
                }),
                None => reasons.push(format!(
                    "unrecognized pull request URL on {}: {url}",
                    issue.identifier
                )),
            }
        }
    }
    (linked, reasons)
}

fn pull_request_group_with(
    issues: &[&Issue],
    lookup: impl FnMut(&str, u64) -> Result<Option<GithubPullRequest>, String>,
) -> HygieneGroup<PullRequestFinding> {
    let (linked, reasons) = linked_pull_requests(issues);
    pull_request_group_result(linked, reasons, lookup)
}

fn pull_request_group_result(
    linked: Vec<LinkedPullRequest<'_>>,
    mut reasons: Vec<String>,
    mut lookup: impl FnMut(&str, u64) -> Result<Option<GithubPullRequest>, String>,
) -> HygieneGroup<PullRequestFinding> {
    let mut findings = Vec::new();
    let mut lookup_cache = BTreeMap::new();
    for link in linked {
        let record = match lookup_cache
            .entry((link.repository.clone(), link.number))
            .or_insert_with(|| lookup(&link.repository, link.number))
            .clone()
        {
            Ok(record) => record,
            Err(reason) => {
                reasons.push(format!(
                    "GitHub lookup failed for {}#{}: {reason}",
                    link.repository, link.number
                ));
                None
            }
        };
        let Some(record) = record else {
            if !reasons
                .iter()
                .any(|reason| reason.contains(&format!("{}#{}", link.repository, link.number)))
            {
                reasons.push(format!(
                    "GitHub pull request unavailable: {}#{}",
                    link.repository, link.number
                ));
            }
            continue;
        };
        if let Some(kind) = pr_mismatch_kind(link.issue, &record.state, record.merged_at.as_deref())
        {
            findings.push(PullRequestFinding {
                issue_id: link.issue.id.clone(),
                repository: link.repository,
                number: link.number,
                url: link.url,
                kind,
                state: record.state,
                merged_at: record.merged_at,
            });
        }
    }
    findings.sort_by(|left, right| {
        left.repository
            .cmp(&right.repository)
            .then(left.number.cmp(&right.number))
            .then(left.issue_id.cmp(&right.issue_id))
    });
    let mut seen_reasons = BTreeSet::new();
    reasons.retain(|reason| seen_reasons.insert(reason.clone()));
    HygieneGroup {
        availability: if reasons.is_empty() {
            GroupStatus::Available
        } else {
            GroupStatus::Unavailable
        },
        reasons,
        findings,
    }
}

fn fetch_github_pull_request(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    token: &str,
    owner: &str,
    name: &str,
    number: u64,
) -> Result<Option<GithubPullRequest>, String> {
    let request = GithubGraphQlRequest {
        query: "query PullRequestState($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { state mergedAt } } }",
        variables: GithubPullRequestVariables {
            owner,
            name,
            number,
        },
    };
    let response = client
        .post(endpoint)
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "ix-board")
        .json(&request)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(|response| response.json::<GithubGraphQlResponse>())
        .map_err(|error| error.to_string())?;
    decode_github_pull_request(response)
}

fn decode_github_pull_request(
    response: GithubGraphQlResponse,
) -> Result<Option<GithubPullRequest>, String> {
    if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
        return Err(errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; "));
    }
    Ok(response
        .data
        .and_then(|data| data.repository)
        .and_then(|repository| repository.pull_request))
}

fn pr_mismatch_kind(
    issue: &Issue,
    state: &str,
    merged_at: Option<&str>,
) -> Option<PullRequestMismatchKind> {
    if issue.state_type == WorkflowStateType::Completed && state == "OPEN" {
        Some(PullRequestMismatchKind::DoneWithOpenPr)
    } else if !terminal(issue) && merged_at.is_some() {
        Some(PullRequestMismatchKind::OpenWithMergedPr)
    } else {
        None
    }
}

fn parse_pull_url(value: &str) -> Option<(String, u64)> {
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https" || url.host_str()? != "github.com" {
        return None;
    }
    let parts = url.path_segments()?.collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" {
        return None;
    }
    Some((format!("{}/{}", parts[0], parts[1]), parts[3].parse().ok()?))
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct GithubGraphQlRequest<'a> {
    query: &'static str,
    variables: GithubPullRequestVariables<'a>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct GithubPullRequestVariables<'a> {
    owner: &'a str,
    name: &'a str,
    number: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubGraphQlResponse {
    data: Option<GithubResponseData>,
    errors: Option<Vec<GithubError>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubResponseData {
    repository: Option<GithubRepository>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubRepository {
    #[serde(rename = "pullRequest")]
    pull_request: Option<GithubPullRequest>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubPullRequest {
    state: String,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubError {
    message: String,
}

fn available<T>(findings: Vec<T>) -> HygieneGroup<T> {
    HygieneGroup {
        availability: GroupStatus::Available,
        reasons: Vec::new(),
        findings,
    }
}

fn terminal(issue: &Issue) -> bool {
    matches!(
        issue.state_type,
        WorkflowStateType::Completed | WorkflowStateType::Canceled
    )
}

fn baseline_issue(issue: &Issue, in_focus: bool) -> FocusBaselineIssue {
    let mut blocked_by = issue.blocked_by.clone();
    blocked_by.sort();
    blocked_by.dedup();
    FocusBaselineIssue {
        id: issue.id.clone(),
        in_focus,
        state: issue.state_name.clone(),
        state_type: issue.state_type,
        project_id: issue.project_id.clone(),
        milestone_id: issue.project_milestone_id.clone(),
        blocked_by,
    }
}

fn compare_baselines(
    snapshot: &WorkspaceSnapshot,
    focused_ids: &BTreeSet<IssueId>,
    prior_time: &str,
    prior: &[FocusBaselineIssue],
    current_focus: &[FocusBaselineIssue],
) -> Result<Vec<FocusChange>, HygieneError> {
    if timestamp_epoch(prior_time).is_none() {
        return Err(HygieneError::InvalidTime);
    }
    let old: BTreeMap<_, _> = prior
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let all_ids = old
        .keys()
        .cloned()
        .chain(focused_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let records: BTreeMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let current: BTreeMap<_, _> = current_focus
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let mut changes = Vec::new();
    for id in all_ids {
        let old_issue = old.get(&id).copied();
        let issue = records
            .get(&id)
            .copied()
            .ok_or_else(|| HygieneError::IncompleteSnapshot {
                issue: id.0.clone(),
            })?;
        let current_issue = current
            .get(&id)
            .copied()
            .cloned()
            .unwrap_or_else(|| baseline_issue(issue, false));
        match old_issue {
            None => {
                if current_issue.in_focus {
                    changes.push(FocusChange::Membership {
                        issue_id: id.clone(),
                        change: FocusMembershipChange::EnteredFocus,
                    });
                }
                let created_after_baseline = issue
                    .created_at
                    .as_deref()
                    .and_then(timestamp_epoch)
                    .zip(timestamp_epoch(prior_time))
                    .is_some_and(|(created, baseline)| created > baseline);
                if created_after_baseline {
                    changes.push(FocusChange::Created {
                        issue_id: id.clone(),
                    });
                    if terminal(issue) {
                        changes.push(FocusChange::Closed {
                            issue_id: id.clone(),
                        });
                    }
                }
            }
            Some(old_issue) => {
                if old_issue.in_focus != current_issue.in_focus {
                    changes.push(FocusChange::Membership {
                        issue_id: id.clone(),
                        change: if current_issue.in_focus {
                            FocusMembershipChange::EnteredFocus
                        } else {
                            FocusMembershipChange::LeftFocus
                        },
                    });
                }
                if old_issue.state != current_issue.state
                    || old_issue.state_type != current_issue.state_type
                {
                    changes.push(FocusChange::State {
                        issue_id: id.clone(),
                        old: old_issue.state.clone(),
                        old_category: old_issue.state_type,
                        current: current_issue.state.clone(),
                        current_category: current_issue.state_type,
                    });
                }
                if old_issue.project_id != current_issue.project_id {
                    changes.push(FocusChange::Project {
                        issue_id: id.clone(),
                        old: old_issue.project_id.clone(),
                        current: current_issue.project_id.clone(),
                    });
                }
                if old_issue.milestone_id != current_issue.milestone_id {
                    changes.push(FocusChange::Milestone {
                        issue_id: id.clone(),
                        old: old_issue.milestone_id.clone(),
                        current: current_issue.milestone_id.clone(),
                    });
                }
                if old_issue.blocked_by != current_issue.blocked_by {
                    changes.push(FocusChange::Blockers {
                        issue_id: id.clone(),
                        old: old_issue.blocked_by.clone(),
                        current: current_issue.blocked_by.clone(),
                    });
                }
                if !matches!(
                    old_issue.state_type,
                    WorkflowStateType::Completed | WorkflowStateType::Canceled
                ) && terminal(issue)
                {
                    changes.push(FocusChange::Closed {
                        issue_id: id.clone(),
                    });
                }
            }
        }
    }
    changes.sort_by(|left, right| {
        change_rank(left)
            .cmp(&change_rank(right))
            .then_with(|| change_issue_id(left).cmp(change_issue_id(right)))
    });
    Ok(changes)
}

fn change_rank(change: &FocusChange) -> u8 {
    match change {
        FocusChange::Blockers { .. } => 0,
        FocusChange::Closed { .. } => 1,
        FocusChange::Created { .. } => 2,
        FocusChange::Membership { .. } => 3,
        FocusChange::Milestone { .. } => 4,
        FocusChange::Project { .. } => 5,
        FocusChange::State { .. } => 6,
    }
}

fn change_issue_id(change: &FocusChange) -> &IssueId {
    match change {
        FocusChange::Membership { issue_id, .. }
        | FocusChange::State { issue_id, .. }
        | FocusChange::Project { issue_id, .. }
        | FocusChange::Milestone { issue_id, .. }
        | FocusChange::Blockers { issue_id, .. }
        | FocusChange::Created { issue_id }
        | FocusChange::Closed { issue_id } => issue_id,
    }
}

fn duplicate_groups(issues: &[&Issue]) -> Result<Vec<DuplicateTitleFinding>, HygieneError> {
    const STOP: &[&str] = &[
        "a",
        "an",
        "as",
        "at",
        "by",
        "for",
        "from",
        "in",
        "into",
        "of",
        "on",
        "or",
        "the",
        "to",
        "with",
        "agent",
        "antipattern",
        "doc",
        "docs",
        "instructions",
        "removal",
        "slated",
        "tracking",
    ];
    let mut postings = BTreeMap::<String, Vec<IssueId>>::new();
    let mut tokens_by_id = BTreeMap::<IssueId, BTreeSet<String>>::new();
    let mut titles = BTreeMap::<IssueId, String>::new();
    for issue in issues {
        let tokens = issue
            .title
            .to_lowercase()
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|token| !token.is_empty() && !STOP.contains(token))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        for token in &tokens {
            postings
                .entry(token.clone())
                .or_default()
                .push(issue.id.clone());
        }
        tokens_by_id.insert(issue.id.clone(), tokens);
        titles.insert(issue.id.clone(), issue.title.clone());
    }
    let mut shared = BTreeMap::<(IssueId, IssueId), usize>::new();
    for ids in postings.values_mut() {
        ids.sort();
        ids.dedup();
        for (index, left) in ids.iter().enumerate() {
            for right in ids.iter().skip(index + 1) {
                let pair = (left.clone(), right.clone());
                if !shared.contains_key(&pair) && shared.len() == TITLE_PAIR_LIMIT {
                    return Err(HygieneError::ResourceLimit {
                        limit: TITLE_PAIR_LIMIT,
                    });
                }
                *shared.entry(pair).or_default() += 1;
            }
        }
    }
    let mut adjacency = BTreeMap::<IssueId, BTreeSet<IssueId>>::new();
    for ((left, right), count) in shared {
        let left_count = tokens_by_id.get(&left).map_or(0, BTreeSet::len);
        let right_count = tokens_by_id.get(&right).map_or(0, BTreeSet::len);
        let smaller = left_count.min(right_count);
        if count >= 3 && smaller >= 4 && count >= smaller.saturating_sub(smaller / 4) {
            adjacency
                .entry(left.clone())
                .or_default()
                .insert(right.clone());
            adjacency.entry(right).or_default().insert(left);
        }
    }
    let mut visited = BTreeSet::new();
    let mut groups = Vec::new();
    for start in adjacency.keys() {
        if !visited.insert(start.clone()) {
            continue;
        }
        let mut stack = vec![start.clone()];
        let mut component = Vec::new();
        while let Some(id) = stack.pop() {
            component.push(id.clone());
            for neighbor in adjacency.get(&id).into_iter().flatten() {
                if visited.insert(neighbor.clone()) {
                    stack.push(neighbor.clone());
                }
            }
        }
        component.sort();
        let component_titles = component
            .iter()
            .filter_map(|id| titles.get(id).cloned())
            .collect();
        groups.push(DuplicateTitleFinding {
            issue_ids: component,
            titles: component_titles,
        });
    }
    groups.sort_by(|left, right| left.issue_ids.cmp(&right.issue_ids));
    Ok(groups)
}

fn timestamp_epoch(timestamp: &str) -> Option<i64> {
    if !timestamp.is_ascii()
        || timestamp.len() < 20
        || timestamp.as_bytes().get(4) != Some(&b'-')
        || timestamp.as_bytes().get(7) != Some(&b'-')
        || timestamp.as_bytes().get(10) != Some(&b'T')
        || timestamp.as_bytes().get(13) != Some(&b':')
        || timestamp.as_bytes().get(16) != Some(&b':')
    {
        return None;
    }
    let year = timestamp.get(0..4)?.parse::<i64>().ok()?;
    let month = timestamp.get(5..7)?.parse::<i64>().ok()?;
    let day = timestamp.get(8..10)?.parse::<i64>().ok()?;
    let hour = timestamp.get(11..13)?.parse::<i64>().ok()?;
    let minute = timestamp.get(14..16)?.parse::<i64>().ok()?;
    let second = timestamp.get(17..19)?.parse::<i64>().ok()?;
    if year == 0 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month {
        return None;
    }

    let bytes = timestamp.as_bytes();
    let mut position = 19;
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let fraction_start = position;
        while position < bytes.len() && bytes[position].is_ascii_digit() {
            position += 1;
        }
        if position == fraction_start {
            return None;
        }
    }
    let offset = match bytes.get(position) {
        Some(b'Z') if position + 1 == bytes.len() => 0,
        Some(b'+' | b'-')
            if position + 6 == bytes.len() && bytes.get(position + 3) == Some(&b':') =>
        {
            let hours = timestamp
                .get(position + 1..position + 3)?
                .parse::<i64>()
                .ok()?;
            let minutes = timestamp
                .get(position + 4..position + 6)?
                .parse::<i64>()
                .ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let sign = if bytes[position] == b'+' { 1 } else { -1 };
            sign * (hours * 3_600 + minutes * 60)
        }
        _ => return None,
    };

    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146097 + day_of_era - 719468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Initiative, Milestone, Project, TeamKey};

    fn issue(id: &str, title: &str, state_type: WorkflowStateType) -> Issue {
        Issue {
            id: IssueId(id.to_owned()),
            identifier: id.to_owned(),
            title: title.to_owned(),
            description: None,
            state: "Todo".to_owned(),
            state_name: "Todo".to_owned(),
            state_type,
            parent_id: None,
            team_key: TeamKey("IR".to_owned()),
            project_id: Some(ProjectId("p1".to_owned())),
            project_milestone_id: Some("m1".to_owned()),
            priority: 2.0,
            sort_order: 0.0,
            blocked_by: Vec::new(),
            created_at: Some("2026-01-01T00:00:00Z".to_owned()),
            updated_at: Some("2026-01-01T00:00:00Z".to_owned()),
            attachment_urls: Vec::new(),
            attachments_complete: true,
        }
    }

    fn workspace_snapshot(issues: Vec<Issue>) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            projects: vec![Project {
                id: ProjectId("p1".to_owned()),
                identifier: None,
                name: "P1".to_owned(),
                description: String::new(),
                content: None,
                status: "Started".to_owned(),
                priority: 0,
                sort_order: 0.0,
                target_date: None,
                team_keys: vec![TeamKey("IR".to_owned())],
                initiatives: vec![Initiative {
                    id: "in1".to_owned(),
                    name: "I1".to_owned(),
                }],
                milestones: vec![Milestone {
                    id: "m1".to_owned(),
                    name: "M1".to_owned(),
                    description: None,
                    sort_order: 0.0,
                    target_date: Some("2026-01-30".to_owned()),
                }],
            }],
            issues,
            fetched_at: 1,
        }
    }

    fn baseline(issue: &Issue, in_focus: bool) -> FocusBaselineIssue {
        baseline_issue(issue, in_focus)
    }

    /// Trace: FR-013-AC-1, FR-013-AC-2, FR-013-AC-12
    #[test]
    fn tc_013_01_missing_scope_is_independent_and_overdue_needs_open_work() {
        let mut no_project = issue("IR-1", "one", WorkflowStateType::Completed);
        no_project.project_id = None;
        no_project.project_milestone_id = None;
        let open = issue("IR-2", "two", WorkflowStateType::Started);
        let mut snapshot = workspace_snapshot(vec![no_project.clone(), open.clone()]);
        let ids = [no_project.id.clone(), open.id.clone()].into();
        let report = build_report(
            &snapshot,
            &ids,
            vec!["project:p1".into()],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            report.missing_scope.findings[0].missing_fields,
            ["project", "milestone"]
        );
        assert_eq!(
            report.overdue_milestones.findings[0].open_issue_ids,
            vec![open.id.clone()]
        );
        assert_eq!(
            report.issue_refs.get(&no_project.id),
            Some(&IssueReference {
                identifier: "IR-1".to_owned(),
                title: "one".to_owned(),
            })
        );
        assert_eq!(
            report.issue_refs.get(&open.id),
            Some(&IssueReference {
                identifier: "IR-2".to_owned(),
                title: "two".to_owned(),
            })
        );
        let json = serde_json::to_value(&report).expect("serialize hygiene report");
        assert_eq!(json["issue_refs"]["IR-2"]["title"], "two");
        snapshot.projects[0].milestones[0].target_date = Some("2026-02-01".to_owned());
        let due_today = build_report(
            &snapshot,
            &ids,
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert!(due_today.overdue_milestones.findings.is_empty());
        snapshot.projects[0].milestones[0].target_date = None;
        let no_date = build_report(
            &snapshot,
            &ids,
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert!(no_date.overdue_milestones.findings.is_empty());
        snapshot.projects[0].milestones[0].target_date = Some("2026-01-30".to_owned());
        let completed = issue("IR-3", "done", WorkflowStateType::Completed);
        let terminal_only = WorkspaceSnapshot {
            projects: snapshot.projects.clone(),
            issues: vec![completed.clone()],
            fetched_at: snapshot.fetched_at,
        };
        let terminal_report = build_report(
            &terminal_only,
            &[completed.id.clone()].into(),
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert!(terminal_report.overdue_milestones.findings.is_empty());
    }

    /// Trace: FR-013-AC-4
    #[test]
    fn tc_013_02_stale_boundary_is_strict_and_terminal_work_is_excluded() {
        let mut exact = issue("IR-1", "one", WorkflowStateType::Started);
        exact.updated_at = Some("2026-01-02T00:00:00Z".to_owned());
        let mut old = issue("IR-2", "two", WorkflowStateType::Started);
        old.updated_at = Some("2026-01-01T23:59:59Z".to_owned());
        let mut done = issue("IR-3", "three", WorkflowStateType::Completed);
        done.updated_at = Some("2020-01-01T00:00:00Z".to_owned());
        let snapshot = workspace_snapshot(vec![exact.clone(), old.clone(), done.clone()]);
        let ids = [exact.id, old.id.clone(), done.id].into();
        let report = build_report(
            &snapshot,
            &ids,
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            report
                .stale_issues
                .findings
                .iter()
                .map(|finding| finding.issue_id.clone())
                .collect::<Vec<_>>(),
            vec![old.id]
        );
        assert!(report.issue_refs.contains_key(&IssueId("IR-2".to_owned())));
    }

    /// Trace: FR-013-AC-5, FR-013-AC-10, FR-013-AC-11, FR-013-AC-12
    #[test]
    fn tc_013_03_duplicate_component_threshold_and_pair_cap_are_deterministic() {
        let related = [
            issue(
                "IR-1",
                "Mark hash/digest/pin antipattern as slated for removal",
                WorkflowStateType::Completed,
            ),
            issue(
                "IR-2",
                "Mark hash/digest/pin as slated for removal in agent docs",
                WorkflowStateType::Canceled,
            ),
            issue(
                "IR-3",
                "Document hash/digest/pin tracking as slated for removal",
                WorkflowStateType::Started,
            ),
        ];
        let groups = duplicate_groups(&related.iter().collect::<Vec<_>>()).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].issue_ids,
            vec![
                IssueId("IR-1".into()),
                IssueId("IR-2".into()),
                IssueId("IR-3".into())
            ]
        );
        let all = workspace_snapshot(vec![
            related[0].clone(),
            related[1].clone(),
            related[2].clone(),
        ]);
        let focused_ids = [related[0].id.clone(), related[1].id.clone()].into();
        let focused_report = build_report(
            &all,
            &focused_ids,
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            focused_report.near_duplicate_titles.findings[0]
                .issue_ids
                .len(),
            2
        );
        assert!(focused_report.issue_refs.contains_key(&related[0].id));
        assert!(focused_report.issue_refs.contains_key(&related[1].id));
        assert!(!focused_report.issue_refs.contains_key(&related[2].id));
        let below = issue("IR-4", "hash digest token", WorkflowStateType::Started);
        assert!(duplicate_groups(&[&related[0], &below]).unwrap().is_empty());
        let five_tokens = issue(
            "IR-5",
            "mark hash digest pin alpha",
            WorkflowStateType::Started,
        );
        let low_overlap = issue(
            "IR-6",
            "mark hash digest beta gamma",
            WorkflowStateType::Started,
        );
        assert!(
            duplicate_groups(&[&five_tokens, &low_overlap])
                .unwrap()
                .is_empty()
        );
        for (count, expected_error) in [(447, false), (448, true)] {
            let rows = (0..count)
                .map(|index| issue(&format!("IR-{index}"), "shared", WorkflowStateType::Started))
                .collect::<Vec<_>>();
            let references = rows.iter().collect::<Vec<_>>();
            let result = duplicate_groups(&references);
            assert_eq!(
                matches!(
                    result,
                    Err(HygieneError::ResourceLimit {
                        limit: TITLE_PAIR_LIMIT
                    })
                ),
                expected_error
            );
            if !expected_error {
                assert!(result.unwrap().is_empty());
            }
        }
    }

    /// Trace: FR-013-AC-3
    #[test]
    fn tc_013_04_pr_urls_and_state_mismatches_are_classified_without_closed_false_positive() {
        assert_eq!(
            parse_pull_url("https://github.com/agent-ix/ix-board/pull/17"),
            Some(("agent-ix/ix-board".to_owned(), 17))
        );
        assert!(parse_pull_url("https://github.com/agent-ix/ix-board/issues/17").is_none());
        let done = issue("IR-1", "done", WorkflowStateType::Completed);
        let open = issue("IR-2", "open", WorkflowStateType::Started);
        assert!(matches!(
            pr_mismatch_kind(&done, "OPEN", None),
            Some(PullRequestMismatchKind::DoneWithOpenPr)
        ));
        assert_eq!(
            pr_mismatch_kind(&open, "CLOSED", Some("2026-02-01T00:00:00Z")),
            Some(PullRequestMismatchKind::OpenWithMergedPr)
        );
        assert_eq!(pr_mismatch_kind(&open, "CLOSED", None), None);
    }

    /// Trace: FR-013-AC-3
    #[test]
    fn tc_013_09_github_graphql_pr_response_fixtures_and_query_scope() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        fn fixture(body: &'static str) -> (String, thread::JoinHandle<Vec<u8>>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let handle = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..count]);
                    let header_end = request
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .map(|position| position + 4);
                    let complete = header_end.is_some_and(|header_end| {
                        let header = String::from_utf8_lossy(&request[..header_end]);
                        let content_length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        request.len() >= header_end + content_length
                    });
                    if count == 0 || complete {
                        break;
                    }
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
                request
            });
            (format!("http://{address}/graphql"), handle)
        }

        let client = reqwest::blocking::Client::new();
        let (endpoint, server) =
            fixture(r#"{"data":{"repository":{"pullRequest":{"state":"OPEN","mergedAt":null}}}}"#);
        let response =
            fetch_github_pull_request(&client, &endpoint, "token", "agent-ix", "ix-board", 17)
                .unwrap()
                .unwrap();
        assert_eq!(response.state, "OPEN");
        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(request.contains("pullRequest(number"));
        assert!(!request.contains("issues(") && !request.contains("/issues/"));

        let (endpoint, server) = fixture(r#"{"data":{"repository":{"pullRequest":null}}}"#);
        assert!(
            fetch_github_pull_request(&client, &endpoint, "token", "agent-ix", "ix-board", 19)
                .unwrap()
                .is_none()
        );
        let _ = server.join().unwrap();

        let (endpoint, server) = fixture(r#"{"errors":[{"message":"denied"}]}"#);
        assert!(
            fetch_github_pull_request(&client, &endpoint, "token", "agent-ix", "ix-board", 20)
                .is_err()
        );
        let _ = server.join().unwrap();
    }

    /// Trace: FR-013-AC-3
    #[test]
    fn tc_013_12_report_pr_group_attributes_each_link_and_keeps_proven_findings() {
        let mut done = issue("IR-1", "done", WorkflowStateType::Completed);
        done.attachment_urls = vec!["https://github.com/agent-ix/ix-board/pull/17".to_owned()];
        let mut open = issue("IR-2", "open", WorkflowStateType::Started);
        open.attachment_urls = vec!["https://github.com/agent-ix/ix-board/pull/18".to_owned()];
        let mut missing = issue("IR-3", "missing PR record", WorkflowStateType::Completed);
        missing.attachment_urls = vec!["https://github.com/agent-ix/ix-board/pull/19".to_owned()];
        let issues = [&done, &open, &missing];
        let fixtures = BTreeMap::from([
            (
                ("agent-ix/ix-board".to_owned(), 17),
                r#"{"data":{"repository":{"pullRequest":{"state":"OPEN","mergedAt":null}}}}"#,
            ),
            (
                ("agent-ix/ix-board".to_owned(), 18),
                r#"{"data":{"repository":{"pullRequest":{"state":"CLOSED","mergedAt":"2026-02-01T00:00:00Z"}}}}"#,
            ),
            (
                ("agent-ix/ix-board".to_owned(), 19),
                r#"{"data":{"repository":{"pullRequest":null}}}"#,
            ),
        ]);
        let report = pull_request_group_with(&issues, |repository, number| {
            let key = (repository.to_owned(), number);
            let fixture = fixtures
                .get(&key)
                .ok_or_else(|| format!("missing fixture for {repository}#{number}"))?;
            let response = serde_json::from_str::<GithubGraphQlResponse>(fixture)
                .map_err(|error| error.to_string())?;
            decode_github_pull_request(response)
        });
        assert_eq!(report.availability, GroupStatus::Unavailable);
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains("agent-ix/ix-board#19"))
        );
        assert_eq!(report.findings.len(), 2);
        assert!(report.findings.iter().any(|finding| {
            finding.issue_id == done.id
                && finding.number == 17
                && finding.kind == PullRequestMismatchKind::DoneWithOpenPr
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.issue_id == open.id
                && finding.number == 18
                && finding.kind == PullRequestMismatchKind::OpenWithMergedPr
        }));
    }

    /// Trace: FR-013-AC-13
    #[test]
    fn repeated_pr_links_share_one_lookup_and_one_unavailable_reason() {
        let mut first = issue("IR-1", "first", WorkflowStateType::Started);
        let mut second = issue("IR-2", "second", WorkflowStateType::Started);
        let url = "https://github.com/agent-ix/ix-board/pull/20".to_owned();
        first.attachment_urls = vec![url.clone()];
        second.attachment_urls = vec![url];
        let mut calls = 0;
        let group = pull_request_group_with(&[&first, &second], |_, _| {
            calls += 1;
            Err("unavailable".to_owned())
        });
        assert_eq!(calls, 1);
        assert_eq!(group.reasons.len(), 1);
        assert_eq!(group.availability, GroupStatus::Unavailable);
    }

    /// Trace: FR-013-AC-3, FR-013-AC-7, FR-013-AC-8
    #[test]
    fn tc_013_10_attachment_incompleteness_is_limited_to_the_pr_group() {
        let focused = issue("IR-1", "focused", WorkflowStateType::Started);
        let mut unrelated = issue("PLAT-1", "unrelated", WorkflowStateType::Started);
        unrelated.attachments_complete = false;
        let snapshot = workspace_snapshot(vec![focused.clone(), unrelated]);
        let report = build_report(
            &snapshot,
            &[focused.id.clone()].into(),
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert_eq!(report.pull_requests.availability, GroupStatus::Available);
        assert!(report.baseline_advance);

        let mut incomplete_focus = focused.clone();
        incomplete_focus.attachments_complete = false;
        let incomplete_snapshot = workspace_snapshot(vec![incomplete_focus.clone()]);
        let incomplete_report = build_report(
            &incomplete_snapshot,
            &[incomplete_focus.id.clone()].into(),
            vec![],
            "2026-02-02T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            incomplete_report.pull_requests.availability,
            GroupStatus::Unavailable
        );
        assert!(incomplete_report.baseline_advance);
    }

    /// Trace: FR-013-AC-6, FR-013-AC-9
    #[test]
    fn tc_013_05_prior_only_issue_reports_left_focus_and_changed_fields() {
        let mut old = issue("IR-1", "old", WorkflowStateType::Started);
        old.blocked_by = vec![IssueId("IR-2".into())];
        let mut current = old.clone();
        current.project_id = Some(ProjectId("p2".into()));
        current.project_milestone_id = None;
        current.blocked_by.clear();
        current.state_name = "Done".to_owned();
        current.state_type = WorkflowStateType::Completed;
        let snapshot = workspace_snapshot(vec![current.clone()]);
        let changes = compare_baselines(
            &snapshot,
            &BTreeSet::new(),
            "2026-01-01T00:00:00Z",
            &[baseline(&old, true)],
            &[],
        )
        .unwrap();
        assert_eq!(
            changes.iter().map(change_rank).collect::<Vec<_>>(),
            vec![0, 1, 3, 4, 5, 6]
        );
        assert!(changes.iter().any(|change| matches!(
            change,
            FocusChange::Membership {
                change: FocusMembershipChange::LeftFocus,
                ..
            }
        )));
        assert!(changes.iter().any(|change| matches!(change, FocusChange::Project { current: Some(ProjectId(id)), .. } if id == "p2")));
        assert!(changes.iter().any(|change| matches!(
            change,
            FocusChange::State {
                current_category: WorkflowStateType::Completed,
                ..
            }
        )));
        assert!(
            changes
                .iter()
                .any(|change| matches!(change, FocusChange::Milestone { current: None, .. }))
        );
        assert!(changes.iter().any(
            |change| matches!(change, FocusChange::Blockers { current, .. } if current.is_empty())
        ));

        let mut category_only = issue("IR-3", "same label", WorkflowStateType::Started);
        let old_category = baseline(&category_only, true);
        category_only.state_type = WorkflowStateType::Completed;
        let category_snapshot = workspace_snapshot(vec![category_only.clone()]);
        let category_changes = compare_baselines(
            &category_snapshot,
            &[category_only.id.clone()].into(),
            "2026-01-01T00:00:00Z",
            &[old_category],
            &[baseline(&category_only, true)],
        )
        .unwrap();
        assert!(category_changes.iter().any(|change| matches!(
            change,
            FocusChange::State {
                old,
                old_category: WorkflowStateType::Started,
                current,
                current_category: WorkflowStateType::Completed,
                ..
            } if old == current
        )));
    }

    /// Trace: FR-013-AC-6
    #[test]
    fn tc_013_06_created_and_closed_are_both_reported_once() {
        let mut new_done = issue("IR-1", "new done", WorkflowStateType::Completed);
        new_done.created_at = Some("2026-01-02T00:00:00Z".to_owned());
        let snapshot = workspace_snapshot(vec![new_done.clone()]);
        let changes = compare_baselines(
            &snapshot,
            &[new_done.id.clone()].into(),
            "2026-01-01T00:00:00Z",
            &[],
            &[baseline(&new_done, true)],
        )
        .unwrap();
        assert_eq!(
            changes
                .iter()
                .filter(|change| matches!(change, FocusChange::Created { .. }))
                .count(),
            1
        );
        assert_eq!(
            changes
                .iter()
                .filter(|change| matches!(change, FocusChange::Closed { .. }))
                .count(),
            1
        );
        assert_eq!(
            changes
                .iter()
                .filter(|change| matches!(
                    change,
                    FocusChange::Membership {
                        change: FocusMembershipChange::EnteredFocus,
                        ..
                    }
                ))
                .count(),
            1
        );
    }

    /// Trace: FR-013-AC-7, FR-013-AC-8, FR-013-AC-9, FR-013-AC-12
    #[test]
    fn tc_013_07_incomplete_diff_preserves_baseline_and_complete_output_is_stable() {
        let current = issue("IR-2", "current", WorkflowStateType::Started);
        let snapshot = workspace_snapshot(vec![current.clone()]);
        let ids = [current.id.clone()].into();
        let initial = build_report(
            &snapshot,
            &ids,
            vec!["project:p1".into()],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert!(matches!(
            initial.diff.availability,
            GroupStatus::Unavailable
        ));
        assert!(initial.baseline_advance);
        let prior_only = issue("IR-1", "moved out", WorkflowStateType::Started);
        let incomplete = build_report(
            &snapshot,
            &ids,
            vec!["project:p1".into()],
            "2026-02-02T00:00:00Z",
            30,
            Some(("2026-02-01T00:00:00Z", &[baseline(&prior_only, true)])),
            None,
        )
        .unwrap();
        assert!(matches!(
            incomplete.diff.availability,
            GroupStatus::Unavailable
        ));
        assert!(!incomplete.baseline_advance);
        let complete_snapshot = workspace_snapshot(vec![current.clone(), prior_only.clone()]);
        let complete = build_report(
            &complete_snapshot,
            &ids,
            vec!["project:p1".into()],
            "2026-02-02T00:00:00Z",
            30,
            Some(("2026-02-01T00:00:00Z", &[baseline(&prior_only, true)])),
            None,
        )
        .unwrap();
        assert!(complete.diff.findings.iter().any(|finding| {
            matches!(finding, FocusChange::Membership { issue_id, change: FocusMembershipChange::LeftFocus } if issue_id == &prior_only.id)
        }));
        assert_eq!(
            complete
                .issue_refs
                .get(&prior_only.id)
                .map(|reference| reference.title.as_str()),
            Some("moved out")
        );
        let first_copy = serde_json::to_vec(&initial).unwrap();
        let second_copy = serde_json::to_vec(
            &build_report(
                &snapshot,
                &ids,
                vec!["project:p1".into()],
                "2026-02-01T00:00:00Z",
                30,
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(first_copy, second_copy);
    }

    /// Trace: FR-013-AC-7, FR-013-AC-8
    #[test]
    fn tc_013_11_invalid_created_at_for_new_focus_blocks_diff_and_baseline() {
        let mut current = issue(
            "IR-3",
            "new with invalid timestamp",
            WorkflowStateType::Started,
        );
        current.created_at = Some("2026-02-30T00:00:00Z".to_owned());
        current.updated_at = Some("2026-02-02T00:00:00Z".to_owned());
        let snapshot = workspace_snapshot(vec![current.clone()]);
        let report = build_report(
            &snapshot,
            &[current.id.clone()].into(),
            vec!["issue:IR-3".to_owned()],
            "2026-02-02T00:00:00Z",
            30,
            Some(("2026-02-01T00:00:00Z", &[])),
            None,
        )
        .unwrap();
        assert_eq!(report.diff.availability, GroupStatus::Unavailable);
        assert!(
            report
                .diff
                .reasons
                .iter()
                .any(|reason| reason.contains("createdAt"))
        );
        assert!(!report.baseline_advance);
    }

    /// Trace: FR-013-AC-7, FR-013-AC-8
    #[test]
    fn tc_013_08_missing_activity_timestamp_makes_group_unavailable_and_preserves_baseline() {
        assert!(timestamp_epoch("2026-09-27T14:00:00+02:00").is_some());
        assert!(timestamp_epoch("2026-02-30T00:00:00Z").is_none());
        assert!(timestamp_epoch("2026-09-27T14:00:00+25:00").is_none());
        assert!(timestamp_epoch("2026-09-27T14:00:00").is_none());
        let mut current = issue("IR-1", "missing timestamp", WorkflowStateType::Started);
        current.updated_at = None;
        let snapshot = workspace_snapshot(vec![current.clone()]);
        let report = build_report(
            &snapshot,
            &[current.id.clone()].into(),
            vec![],
            "2026-02-01T00:00:00Z",
            30,
            None,
            None,
        )
        .unwrap();
        assert!(matches!(
            report.stale_issues.availability,
            GroupStatus::Unavailable
        ));
        assert!(!report.baseline_advance);
    }
}
