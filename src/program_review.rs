// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Program-level scope files and a live initiative/project roadmap.

use crate::{Error, Initiative, Issue, Project, WorkflowStateType, WorkspaceSnapshot};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use thiserror::Error as ThisError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramScope {
    pub program: String,
    #[serde(default)]
    pub initiatives: Vec<NamedRef>,
    #[serde(default)]
    pub projects: Vec<ProjectPlacement>,
    #[serde(default)]
    pub repos: Vec<RepositoryRef>,
    #[serde(default)]
    pub lanes: Vec<LaneRef>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
}

const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

fn github_client(timeout: Duration) -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent("ix-board")
        .timeout(timeout)
        .build()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NamedRef {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPlacement {
    pub name: String,
    pub primary_initiative: String,
    #[serde(default)]
    pub also_in: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryRef {
    /// GitHub owner/repository path, such as `agent-ix/quire`.
    pub name: String,
    /// A one-line explanation of the repository's role in this program.
    pub purpose: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaneRef {
    /// Relay address in `server:workspace:tab` form.
    pub address: String,
    /// What this lane owns in the program.
    pub owns: String,
}

#[derive(Debug, ThisError)]
pub enum ProgramReviewError {
    #[error("could not read program scope: {0}")]
    Read(#[from] std::io::Error),
    #[error("invalid program scope TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid program scope: {0}")]
    InvalidScope(String),
    #[error("GitHub repository check failed for {repository}: {source}")]
    Github {
        repository: String,
        source: reqwest::Error,
    },
    #[error(
        "cannot verify GitHub repository {repository}: unauthenticated GitHub returned 404 (private or missing repository)"
    )]
    UnverifiableRepository { repository: String },
    #[error(transparent)]
    Linear(#[from] Error),
}

impl From<ProgramReviewError> for Error {
    fn from(error: ProgramReviewError) -> Self {
        Self::ProgramReview {
            message: error.to_string(),
        }
    }
}

impl ProgramScope {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProgramReviewError> {
        let scope: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        scope.validate()?;
        Ok(scope)
    }

    fn validate(&self) -> Result<(), ProgramReviewError> {
        if self.program.trim().is_empty() {
            return Err(ProgramReviewError::InvalidScope(
                "program name must not be empty".to_owned(),
            ));
        }
        let mut seen_projects = BTreeSet::new();
        for project in &self.projects {
            if project.name.trim().is_empty() || project.primary_initiative.trim().is_empty() {
                return Err(ProgramReviewError::InvalidScope(
                    "each project needs a name and primary_initiative".to_owned(),
                ));
            }
            if !seen_projects.insert(project.name.to_lowercase()) {
                return Err(ProgramReviewError::InvalidScope(format!(
                    "project {} is placed more than once",
                    project.name
                )));
            }
        }
        for repo in &self.repos {
            if !valid_repo_path(&repo.name) || repo.purpose.trim().is_empty() {
                return Err(ProgramReviewError::InvalidScope(format!(
                    "repository {:?} needs a valid owner/repository path and one-line purpose",
                    repo.name
                )));
            }
        }
        for lane in &self.lanes {
            let parts: Vec<&str> = lane.address.split(':').collect();
            if parts.len() != 3
                || parts.iter().any(|part| part.trim().is_empty())
                || lane.owns.trim().is_empty()
            {
                return Err(ProgramReviewError::InvalidScope(format!(
                    "lane {:?} needs server:workspace:tab and an ownership description",
                    lane.address
                )));
            }
        }
        Ok(())
    }
}

fn valid_repo_path(path: &str) -> bool {
    let mut parts = path.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && [owner, repo].iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeCheckReport {
    pub missing_initiatives: Vec<String>,
    pub missing_projects: Vec<String>,
    pub missing_repositories: Vec<String>,
    pub unplaced_projects: Vec<String>,
}

impl ScopeCheckReport {
    pub fn is_valid(&self) -> bool {
        self.missing_initiatives.is_empty()
            && self.missing_projects.is_empty()
            && self.missing_repositories.is_empty()
            && self.unplaced_projects.is_empty()
    }
}

/// Compare a scope file with live Linear and GitHub catalogs.
pub fn check_scope(
    scope: &ProgramScope,
    initiatives: &[Initiative],
    workspace: &WorkspaceSnapshot,
) -> Result<ScopeCheckReport, ProgramReviewError> {
    let mut report = validate_scope_catalog(scope, initiatives, &workspace.projects);
    let client =
        github_client(GITHUB_REQUEST_TIMEOUT).map_err(|source| ProgramReviewError::Github {
            repository: "<client initialization>".to_owned(),
            source,
        })?;
    let token = selected_github_token(
        std::env::var("GITHUB_TOKEN").ok(),
        std::env::var("GH_TOKEN").ok(),
        github_cli_token,
    );
    for repo in &scope.repos {
        let response = request_github_repository(
            &client,
            &format!("https://api.github.com/repos/{}", repo.name),
            &repo.name,
            token.as_deref(),
        )?;
        if repository_is_missing(response.status().as_u16(), token.is_some(), &repo.name)? {
            report.missing_repositories.push(repo.name.clone());
        }
    }
    report.missing_repositories.sort();
    Ok(report)
}

fn github_repository_request(
    client: &Client,
    url: &str,
    token: Option<&str>,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let request = client.get(url);
    let request = match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    };
    request.send()
}

fn request_github_repository(
    client: &Client,
    url: &str,
    repository: &str,
    token: Option<&str>,
) -> Result<reqwest::blocking::Response, ProgramReviewError> {
    github_repository_request(client, url, token).map_err(|source| ProgramReviewError::Github {
        repository: repository.to_owned(),
        source,
    })
}

fn selected_github_token(
    github_token: Option<String>,
    gh_token: Option<String>,
    gh_cli_token: impl FnOnce() -> Option<String>,
) -> Option<String> {
    github_token
        .filter(|token| !token.trim().is_empty())
        .or_else(|| gh_token.filter(|token| !token.trim().is_empty()))
        .or_else(gh_cli_token)
        .filter(|token| !token.trim().is_empty())
}

fn repository_is_missing(
    status: u16,
    authenticated: bool,
    repository: &str,
) -> Result<bool, ProgramReviewError> {
    match status {
        200..=299 => Ok(false),
        404 if authenticated => Ok(true),
        404 => Err(ProgramReviewError::UnverifiableRepository {
            repository: repository.to_owned(),
        }),
        _ => Err(ProgramReviewError::InvalidScope(format!(
            "GitHub returned HTTP {status} while checking repository {repository}"
        ))),
    }
}

fn github_cli_token() -> Option<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

/// Deterministic catalog validation, used for both live checks and recorded fixtures.
pub fn validate_scope_catalog(
    scope: &ProgramScope,
    initiatives: &[Initiative],
    projects: &[Project],
) -> ScopeCheckReport {
    let mut required_initiatives: Vec<String> =
        scope.initiatives.iter().map(|i| i.name.clone()).collect();
    required_initiatives.extend(scope.projects.iter().flat_map(|project| {
        std::iter::once(project.primary_initiative.clone()).chain(project.also_in.iter().cloned())
    }));
    required_initiatives.sort_by_key(|name| name.to_lowercase());
    required_initiatives.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let scoped_initiatives = required_initiatives.clone();

    let missing_initiatives = required_initiatives
        .into_iter()
        .filter(|wanted| {
            !initiatives
                .iter()
                .any(|known| same_name(&known.name, wanted))
        })
        .collect();
    let missing_projects = scope
        .projects
        .iter()
        .filter(|wanted| {
            !projects
                .iter()
                .any(|known| same_name(&known.name, &wanted.name))
        })
        .map(|item| item.name.clone())
        .collect();

    let placed: BTreeSet<String> = scope
        .projects
        .iter()
        .map(|project| project.name.to_lowercase())
        .collect();
    let unplaced_projects = projects
        .iter()
        .filter(|project| {
            project.initiatives.iter().any(|initiative| {
                scoped_initiatives
                    .iter()
                    .any(|listed| same_name(listed, &initiative.name))
            })
        })
        .filter(|project| !placed.contains(&project.name.to_lowercase()))
        .map(|project| project.name.clone())
        .collect();
    let mut report = ScopeCheckReport {
        missing_initiatives,
        missing_projects,
        missing_repositories: Vec::new(),
        unplaced_projects,
    };
    report.unplaced_projects.sort();
    report
}

fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramRoadmap {
    pub initiatives: Vec<RoadmapInitiative>,
    pub unplaced_projects: Vec<RoadmapUnplacedProject>,
    pub initiatives_without_projects: Vec<String>,
    pub projects_in_multiple_initiatives: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoadmapInitiative {
    pub name: String,
    pub projects: Vec<RoadmapProject>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoadmapProject {
    pub name: String,
    pub status: String,
    pub progress_percent: u8,
    pub open: usize,
    pub done: usize,
    pub canceled: usize,
    pub last_activity: Option<String>,
    pub also_in: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoadmapUnplacedProject {
    pub name: String,
    pub status: String,
    pub progress_percent: u8,
    pub open: usize,
    pub done: usize,
    pub canceled: usize,
    pub last_activity: Option<String>,
}

/// Build one row per project from a live workspace snapshot. Scope placement
/// names the primary initiative; remaining memberships are rendered as `also_in`.
pub fn build_roadmap(
    initiatives: &[Initiative],
    workspace: &WorkspaceSnapshot,
    scope: Option<&ProgramScope>,
) -> ProgramRoadmap {
    let mut initiative_by_id: BTreeMap<String, String> = initiatives
        .iter()
        .map(|item| (item.id.clone(), item.name.clone()))
        .collect();
    for project in &workspace.projects {
        for member in &project.initiatives {
            initiative_by_id
                .entry(member.id.clone())
                .or_insert_with(|| member.name.clone());
        }
    }
    let selected_initiatives: Vec<&Initiative> = match scope {
        Some(scope) => {
            let mut names: Vec<String> = scope
                .initiatives
                .iter()
                .map(|item| item.name.clone())
                .collect();
            names.extend(scope.projects.iter().flat_map(|project| {
                std::iter::once(project.primary_initiative.clone())
                    .chain(project.also_in.iter().cloned())
            }));
            initiatives
                .iter()
                .filter(|initiative| names.iter().any(|name| same_name(name, &initiative.name)))
                .collect()
        }
        None => initiatives.iter().collect(),
    };

    let mut result = ProgramRoadmap {
        initiatives: Vec::new(),
        unplaced_projects: Vec::new(),
        initiatives_without_projects: Vec::new(),
        projects_in_multiple_initiatives: Vec::new(),
    };
    let mut assigned = BTreeSet::<String>::new();
    for initiative in selected_initiatives {
        let mut rows = Vec::new();
        for project in &workspace.projects {
            if !assigned.insert(project.id.0.clone()) {
                continue;
            }
            let declared = scope.and_then(|file| {
                file.projects
                    .iter()
                    .find(|item| same_name(&item.name, &project.name))
            });
            if scope.is_some() && declared.is_none() {
                assigned.remove(&project.id.0);
                continue;
            }
            let mut memberships: Vec<String> = project
                .initiatives
                .iter()
                .filter_map(|item| initiative_by_id.get(&item.id).cloned())
                .collect();
            memberships.sort_by_key(|name| name.to_lowercase());
            memberships.dedup_by(|a, b| same_name(a, b));
            let primary = declared
                .map(|item| item.primary_initiative.as_str())
                .or_else(|| memberships.first().map(String::as_str));
            if !primary.is_some_and(|name| same_name(name, &initiative.name)) {
                assigned.remove(&project.id.0);
                continue;
            }
            let also_in = declared
                .map(|item| item.also_in.clone())
                .unwrap_or_else(|| {
                    memberships
                        .iter()
                        .filter(|name| !same_name(name, &initiative.name))
                        .cloned()
                        .collect()
                });
            if memberships.len() > 1 {
                result
                    .projects_in_multiple_initiatives
                    .push(project.name.clone());
            }
            rows.push(roadmap_project(project, &workspace.issues, also_in));
        }
        rows.sort_by_key(|item| item.name.to_lowercase());
        if rows.is_empty() {
            result
                .initiatives_without_projects
                .push(initiative.name.clone());
        }
        result.initiatives.push(RoadmapInitiative {
            name: initiative.name.clone(),
            projects: rows,
        });
    }
    for project in &workspace.projects {
        if assigned.contains(&project.id.0) {
            continue;
        }
        let included = scope.is_none_or(|file| {
            file.projects
                .iter()
                .any(|item| same_name(&item.name, &project.name))
        });
        if project.initiatives.is_empty() && included {
            result
                .unplaced_projects
                .push(roadmap_unplaced(project, &workspace.issues));
        }
    }
    result
        .initiatives
        .sort_by_key(|item| item.name.to_lowercase());
    result
        .unplaced_projects
        .sort_by_key(|item| item.name.to_lowercase());
    result
        .initiatives_without_projects
        .sort_by_key(|item| item.to_lowercase());
    result
        .projects_in_multiple_initiatives
        .sort_by_key(|item| item.to_lowercase());
    result.projects_in_multiple_initiatives.dedup();
    result
}

fn issue_counts(project: &Project, issues: &[Issue]) -> (usize, usize, usize, Option<String>) {
    let mut open = 0;
    let mut done = 0;
    let mut canceled = 0;
    let mut last_activity = project.updated_at.clone();
    for issue in issues
        .iter()
        .filter(|issue| issue.project_id.as_ref() == Some(&project.id))
    {
        match issue.state_type {
            WorkflowStateType::Completed => done += 1,
            WorkflowStateType::Canceled | WorkflowStateType::Duplicate => canceled += 1,
            _ => open += 1,
        }
        if issue.updated_at.as_ref().is_some_and(|updated| {
            last_activity
                .as_ref()
                .is_none_or(|current| updated > current)
        }) {
            last_activity.clone_from(&issue.updated_at);
        }
    }
    (open, done, canceled, last_activity)
}

fn progress(open: usize, done: usize) -> u8 {
    let total = open + done;
    let value = done
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or_default()
        .min(100);
    u8::try_from(value).unwrap_or(u8::MAX)
}

fn roadmap_project(project: &Project, issues: &[Issue], also_in: Vec<String>) -> RoadmapProject {
    let (open, done, canceled, last_activity) = issue_counts(project, issues);
    RoadmapProject {
        name: project.name.clone(),
        status: project.status.clone(),
        progress_percent: progress(open, done),
        open,
        done,
        canceled,
        last_activity,
        also_in,
    }
}

fn roadmap_unplaced(project: &Project, issues: &[Issue]) -> RoadmapUnplacedProject {
    let (open, done, canceled, last_activity) = issue_counts(project, issues);
    RoadmapUnplacedProject {
        name: project.name.clone(),
        status: project.status.clone(),
        progress_percent: progress(open, done),
        open,
        done,
        canceled,
        last_activity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IssueId, ProjectId, TeamKey};

    fn scope() -> ProgramScope {
        toml::from_str(
            r#"
program = "QSL"
out_of_scope = ["Sapho"]

[[initiatives]]
name = "QSL Core"

[[projects]]
name = "Compiler"
primary_initiative = "QSL Core"
also_in = []

[[repos]]
name = "agent-ix/quire"
purpose = "QSpec tooling"

[[lanes]]
address = "terra:QSL:program-review"
owns = "program review"
"#,
        )
        .unwrap()
    }

    #[test]
    fn empty_environment_token_falls_back_to_the_next_authenticated_source() {
        assert_eq!(
            selected_github_token(Some("  ".to_owned()), Some("gh-token".to_owned()), || {
                panic!("GH_TOKEN should win before gh auth token")
            })
            .as_deref(),
            Some("gh-token")
        );
        assert_eq!(
            selected_github_token(Some(String::new()), Some(" ".to_owned()), || {
                Some("gh-cli-token".to_owned())
            })
            .as_deref(),
            Some("gh-cli-token")
        );
    }

    #[test]
    /// Trace: FR-019-AC-2
    fn unauthenticated_not_found_is_unverifiable_and_names_the_repository() {
        let error = repository_is_missing(404, false, "agent-ix/private-repo").unwrap_err();
        let diagnostic = error.to_string();
        assert!(diagnostic.contains("agent-ix/private-repo"));
        assert!(diagnostic.contains("private or missing"));
        assert!(repository_is_missing(404, true, "agent-ix/private-repo").unwrap());
        assert!(!repository_is_missing(200, false, "agent-ix/public-repo").unwrap());
    }

    fn initiative(name: &str, id: &str) -> Initiative {
        Initiative {
            id: id.into(),
            identifier: format!("I-{id}"),
            name: name.into(),
        }
    }

    fn project(name: &str, memberships: Vec<Initiative>) -> Project {
        Project {
            id: ProjectId(name.into()),
            identifier: None,
            name: name.into(),
            description: String::new(),
            content: None,
            updated_at: Some("2026-10-20T00:00:00Z".into()),
            status: "Started".into(),
            priority: 0,
            sort_order: 0.0,
            target_date: None,
            team_keys: vec![TeamKey("QSL".into())],
            milestones: Vec::new(),
            initiatives: memberships,
        }
    }

    #[test]
    /// Trace: FR-019-AC-1
    fn parses_scope_and_rejects_stored_tallies() {
        let parsed = scope();
        parsed.validate().unwrap();
        assert_eq!(parsed.repos[0].purpose, "QSpec tooling");
        assert!(
            toml::from_str::<ProgramScope>(
                r#"program="bad"
count=17"#
            )
            .is_err()
        );
    }

    #[test]
    /// Trace: FR-019-AC-1
    fn scope_loader_reads_the_documented_toml_format() {
        let input = r#"program = "QSL"
[[initiatives]]
name = "QSL Core"
[[projects]]
name = "Compiler"
primary_initiative = "QSL Core"
[[repos]]
name = "agent-ix/quire"
purpose = "Spec tooling"
[[lanes]]
address = "terra:QSL:program-review"
owns = "program review"
"#;
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), input).unwrap();
        let loaded = ProgramScope::load(file.path()).unwrap();
        assert_eq!(loaded.program, "QSL");
        assert_eq!(loaded.projects[0].primary_initiative, "QSL Core");
    }

    #[test]
    /// Trace: FR-019-AC-2
    fn catalog_check_reports_missing_projects_and_unplaced_projects() {
        let scope = scope();
        let initiatives = vec![initiative("QSL Core", "2")];
        let projects = vec![project("Runtime", vec![initiative("QSL Core", "2")])];
        let report = validate_scope_catalog(&scope, &initiatives, &projects);
        assert_eq!(report.missing_projects, ["Compiler"]);
        assert_eq!(report.unplaced_projects, ["Runtime"]);
        assert!(!report.is_valid());
    }

    #[test]
    /// Trace: FR-020-AC-1, FR-020-AC-2
    fn roadmap_places_shared_project_once_and_uses_live_issue_counts() {
        let core = initiative("QSL Core", "2");
        let ecosystem = initiative("QSL Ecosystem", "4");
        let project = project("Compiler", vec![core.clone(), ecosystem.clone()]);
        let issues = vec![
            Issue {
                id: IssueId("1".into()),
                identifier: "QSL-1".into(),
                title: "done".into(),
                description: None,
                state: "Done".into(),
                state_name: "Done".into(),
                state_type: WorkflowStateType::Completed,
                parent_id: None,
                team_key: TeamKey("QSL".into()),
                assignee_id: None,
                project_id: Some(project.id.clone()),
                project_milestone_id: None,
                priority: 0.0,
                sort_order: 0.0,
                blocked_by: Vec::new(),
                created_at: None,
                updated_at: Some("2026-10-21T00:00:00Z".into()),
                attachment_urls: Vec::new(),
                attachments_complete: true,
            },
            Issue {
                id: IssueId("2".into()),
                identifier: "QSL-2".into(),
                title: "open".into(),
                description: None,
                state: "Started".into(),
                state_name: "Started".into(),
                state_type: WorkflowStateType::Started,
                parent_id: None,
                team_key: TeamKey("QSL".into()),
                assignee_id: None,
                project_id: Some(project.id.clone()),
                project_milestone_id: None,
                priority: 0.0,
                sort_order: 0.0,
                blocked_by: Vec::new(),
                created_at: None,
                updated_at: None,
                attachment_urls: Vec::new(),
                attachments_complete: true,
            },
        ];
        let workspace = WorkspaceSnapshot {
            projects: vec![project],
            issues,
            fetched_at: 0,
        };
        let report = build_roadmap(&[ecosystem, core], &workspace, None);
        let listed = report
            .initiatives
            .iter()
            .flat_map(|initiative| &initiative.projects)
            .collect::<Vec<_>>();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].progress_percent, 50);
        assert_eq!(listed[0].open, 1);
        assert_eq!(listed[0].done, 1);
        assert_eq!(listed[0].also_in, ["QSL Ecosystem"]);
        assert_eq!(
            listed[0].last_activity.as_deref(),
            Some("2026-10-21T00:00:00Z")
        );
    }

    #[test]
    /// Trace: FR-020-AC-3, FR-020-AC-4
    fn scoped_roadmap_reports_empty_and_unplaced_members_in_stable_order() {
        let core = initiative("QSL Core", "2");
        let ecosystem = initiative("QSL Ecosystem", "4");
        let zulu = project("Zulu", vec![core.clone(), ecosystem.clone()]);
        let alpha = project("Alpha", Vec::new());
        let mut file = scope();
        file.projects.push(ProjectPlacement {
            name: "Zulu".into(),
            primary_initiative: "QSL Core".into(),
            also_in: vec!["QSL Ecosystem".into()],
        });
        file.projects.push(ProjectPlacement {
            name: "Alpha".into(),
            primary_initiative: "QSL Core".into(),
            also_in: Vec::new(),
        });
        file.initiatives.push(NamedRef {
            name: "QSL Empty".into(),
        });
        let initiatives = [initiative("QSL Empty", "8"), ecosystem, core];
        let workspace = WorkspaceSnapshot {
            projects: vec![zulu, alpha],
            issues: Vec::new(),
            fetched_at: 0,
        };
        let report = build_roadmap(&initiatives, &workspace, Some(&file));
        assert_eq!(
            report
                .initiatives
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["QSL Core", "QSL Ecosystem", "QSL Empty"]
        );
        assert_eq!(
            report.initiatives_without_projects,
            ["QSL Ecosystem", "QSL Empty"]
        );
        assert!(report.unplaced_projects.is_empty());
        assert_eq!(report.projects_in_multiple_initiatives, ["Zulu"]);
        let unscoped = build_roadmap(&initiatives, &workspace, None);
        assert_eq!(unscoped.unplaced_projects[0].name, "Alpha");
    }

    #[test]
    /// Trace: FR-019-AC-3
    fn scope_repository_probe_times_out() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;

        assert!(!GITHUB_REQUEST_TIMEOUT.is_zero());
        assert!(GITHUB_REQUEST_TIMEOUT <= Duration::from_secs(30));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_ready_tx, request_ready_rx) = mpsc::channel();
        let (respond_tx, respond_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request);
            request_ready_tx.send(()).unwrap();
            respond_rx.recv().unwrap();
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        });
        let client = github_client(Duration::from_millis(30)).unwrap();
        let (result_tx, result_rx) = mpsc::channel();
        let request_url = format!("http://{address}/repos/agent-ix/quire");
        let client_thread = thread::spawn(move || {
            let result = request_github_repository(&client, &request_url, "agent-ix/quire", None);
            let result = match result {
                Err(error) => (
                    error.to_string(),
                    matches!(error, ProgramReviewError::Github { source, .. } if source.is_timeout()),
                ),
                Ok(_) => ("request unexpectedly succeeded".to_owned(), false),
            };
            result_tx.send(result).unwrap();
        });
        request_ready_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("server accepted the repository request");
        let (diagnostic, timed_out) = result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("client completed the bounded request");
        respond_tx.send(()).unwrap();
        client_thread.join().unwrap();
        server.join().unwrap();
        assert!(diagnostic.contains("agent-ix/quire"));
        assert!(timed_out, "expected request timeout, got: {diagnostic}");
    }
}
