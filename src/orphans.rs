// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only suggestions for in-scope Linear issues that have no project.

use crate::program_review::{ProgramScope, ProjectPlacement};
use crate::{Issue, Project, WorkflowStateType, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum OrphansError {
    #[error("project in program scope not found: {0}")]
    ProjectNotFound(String),
    #[error("project in program scope is ambiguous: {0}")]
    ProjectAmbiguous(String),
    #[error(transparent)]
    Linear(#[from] crate::Error),
}

impl From<OrphansError> for crate::Error {
    fn from(error: OrphansError) -> Self {
        Self::ProgramReview {
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSuggestion {
    pub project: String,
    pub reason: String,
    pub team_mismatch: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrphanFinding {
    pub identifier: String,
    pub title: String,
    pub team: String,
    pub suggestions: Vec<ProjectSuggestion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrphansReport {
    pub scope: String,
    pub open: Vec<OrphanFinding>,
    pub done: Vec<OrphanFinding>,
    pub warnings: Vec<String>,
}

/// Find unprojected open and completed issues for teams represented by the scope's projects.
pub fn find_orphans(
    scope: &ProgramScope,
    snapshot: &WorkspaceSnapshot,
) -> Result<OrphansReport, OrphansError> {
    let mut selected = Vec::new();
    for placement in &scope.projects {
        let matches = snapshot
            .projects
            .iter()
            .filter(|project| project.name.eq_ignore_ascii_case(&placement.name))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [project] => selected.push((*project, placement)),
            [] => return Err(OrphansError::ProjectNotFound(placement.name.clone())),
            _ => return Err(OrphansError::ProjectAmbiguous(placement.name.clone())),
        }
    }
    let scope_teams = selected
        .iter()
        .flat_map(|(project, _)| project.team_keys.iter().map(|key| key.0.clone()))
        .collect::<BTreeSet<_>>();
    let mut open = Vec::new();
    let mut done = Vec::new();
    for issue in snapshot.issues.iter().filter(|issue| {
        issue.project_id.is_none()
            && scope_teams.contains(&issue.team_key.0)
            && !matches!(
                issue.state_type,
                WorkflowStateType::Canceled | WorkflowStateType::Duplicate
            )
    }) {
        let finding = make_finding(issue, &selected);
        if issue.state_type == WorkflowStateType::Completed {
            done.push(finding);
        } else {
            open.push(finding);
        }
    }
    open.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    done.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    let warnings = selected
        .iter()
        .filter(|(project, _)| project.team_keys.is_empty())
        .map(|(project, _)| {
            format!(
                "project {} has no associated teams; its orphan scope cannot be inferred",
                project.name
            )
        })
        .collect();
    Ok(OrphansReport {
        scope: scope.program.clone(),
        open,
        done,
        warnings,
    })
}

fn make_finding(issue: &Issue, projects: &[(&Project, &ProjectPlacement)]) -> OrphanFinding {
    let mut title_matches = projects
        .iter()
        .filter_map(|(project, _)| {
            let prefix = project
                .identifier
                .as_deref()
                .filter(|prefix| starts_with_prefix(&issue.title, prefix))
                .map(str::to_owned)
                .or_else(|| {
                    starts_with_prefix(&issue.title, &project.name).then(|| project.name.clone())
                });
            prefix.map(|prefix| (*project, prefix))
        })
        .collect::<Vec<_>>();
    title_matches.sort_by(|(left, left_prefix), (right, right_prefix)| {
        right_prefix
            .len()
            .cmp(&left_prefix.len())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    let selected: Vec<(&Project, String)> = if let Some((project, prefix)) = title_matches.first() {
        vec![(
            *project,
            format!("title begins with project prefix {prefix:?}"),
        )]
    } else {
        let team_matches = projects
            .iter()
            .filter(|(project, _)| {
                project
                    .team_keys
                    .iter()
                    .any(|team| team.0.eq_ignore_ascii_case(&issue.team_key.0))
            })
            .collect::<Vec<_>>();
        team_matches
            .into_iter()
            .map(|(project, _)| {
                (
                    *project,
                    format!(
                        "project is associated with ticket team {}",
                        issue.team_key.0
                    ),
                )
            })
            .collect()
    };
    let suggestions = selected
        .into_iter()
        .map(|(project, reason)| ProjectSuggestion {
            project: project.name.clone(),
            reason,
            team_mismatch: !project
                .team_keys
                .iter()
                .any(|team| team.0.eq_ignore_ascii_case(&issue.team_key.0)),
        })
        .collect();
    OrphanFinding {
        identifier: issue.identifier.clone(),
        title: issue.title.clone(),
        team: issue.team_key.0.clone(),
        suggestions,
    }
}

fn starts_with_prefix(title: &str, prefix: &str) -> bool {
    let title = title.trim().to_lowercase();
    let prefix = prefix.trim().to_lowercase();
    title.strip_prefix(&prefix).is_some_and(|tail| {
        tail.is_empty()
            || tail
                .chars()
                .next()
                .is_some_and(|character| !character.is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_fixtures_separate_open_done_and_warn_on_team_mismatch() {
        let snapshot: WorkspaceSnapshot =
            serde_json::from_str(include_str!("../tests/fixtures/orphans/workspace.json"))
                .expect("recorded Linear fixture");
        let scope: ProgramScope =
            toml::from_str(include_str!("../tests/fixtures/orphans/scope.toml"))
                .expect("recorded scope fixture");
        let report = find_orphans(&scope, &snapshot).expect("find in-scope orphans");
        assert_eq!(
            report
                .open
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>(),
            ["IR-21", "QSL-11", "QSL-12"]
        );
        assert_eq!(report.done[0].identifier, "IR-22");
        let mismatch = report
            .open
            .iter()
            .find(|issue| issue.identifier == "IR-21")
            .unwrap();
        assert_eq!(mismatch.suggestions[0].project, "QSL Language Spec");
        assert!(mismatch.suggestions[0].team_mismatch);
        assert!(mismatch.suggestions[0].reason.contains("prefix"));
        let team_match = report
            .open
            .iter()
            .find(|issue| issue.identifier == "QSL-12")
            .unwrap();
        assert_eq!(team_match.suggestions[0].project, "QSL Language Spec");
        assert!(!team_match.suggestions[0].team_mismatch);
        assert!(team_match.suggestions[0].reason.contains("team QSL"));
        assert!(!report.open.iter().any(|issue| issue.identifier == "QSL-13"));
    }

    #[test]
    fn missing_scope_project_is_an_error() {
        let snapshot = WorkspaceSnapshot::default();
        let scope: ProgramScope = toml::from_str(
            "program = 'Review'\n[[projects]]\nname = 'Missing'\nprimary_initiative = 'Review'\n",
        )
        .unwrap();
        assert!(matches!(
            find_orphans(&scope, &snapshot),
            Err(OrphansError::ProjectNotFound(name)) if name == "Missing"
        ));
    }
}
