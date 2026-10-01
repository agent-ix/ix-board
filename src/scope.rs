// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Explicit work selection and deduplicated leaf-task progress.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{Error, IssueId, ProjectId, TeamKey, WorkflowStateType, WorkspaceSnapshot};

/// Explicit assignment selectors, combined as a union within an optional team boundary.
#[derive(Clone, Debug, Default)]
pub struct ScopeSelection {
    /// Restrict membership to this team; alone selects the whole team.
    pub team: Option<TeamKey>,
    /// Initiative UUIDs or exact names.
    pub initiatives: Vec<String>,
    /// Project UUIDs, identifiers, or exact names.
    pub projects: Vec<String>,
    /// Ticket UUIDs or identifiers, including their descendants.
    pub tickets: Vec<String>,
}

/// Scope membership and progress measured from one complete snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeSummary {
    /// Selected projects in deterministic UUID order.
    pub project_ids: BTreeSet<ProjectId>,
    /// All selected tickets, including parents and terminal tickets.
    pub issue_ids: BTreeSet<IssueId>,
    /// Non-canceled, non-duplicate leaf tasks.
    pub leaf_ids: BTreeSet<IssueId>,
    /// Completed leaf tasks.
    pub done: usize,
    /// Remaining leaf tasks grouped by their workflow stage.
    pub remaining_by_stage: BTreeMap<String, usize>,
}

fn unique<T>(kind: &'static str, identifier: &str, mut values: Vec<T>) -> Result<T, Error> {
    if values.len() == 1 {
        return values.pop().ok_or_else(|| Error::ScopeSelection {
            kind,
            identifier: identifier.to_owned(),
            matches: 0,
        });
    }
    Err(Error::ScopeSelection {
        kind,
        identifier: identifier.to_owned(),
        matches: values.len(),
    })
}

/// Resolves explicit selectors and counts each executable leaf task once.
pub fn select_scope(
    snapshot: &WorkspaceSnapshot,
    selection: &ScopeSelection,
) -> Result<ScopeSummary, Error> {
    let mut result = ScopeSummary::default();
    if let Some(team) = &selection.team {
        let known = snapshot.issues.iter().any(|i| i.team_key == *team)
            || snapshot.projects.iter().any(|p| p.team_keys.contains(team));
        if !known {
            return Err(Error::ScopeSelection {
                kind: "team",
                identifier: team.0.clone(),
                matches: 0,
            });
        }
        if selection.initiatives.is_empty()
            && selection.projects.is_empty()
            && selection.tickets.is_empty()
        {
            result.project_ids.extend(
                snapshot
                    .projects
                    .iter()
                    .filter(|p| p.team_keys.contains(team))
                    .map(|p| p.id.clone()),
            );
            result.issue_ids.extend(
                snapshot
                    .issues
                    .iter()
                    .filter(|i| i.team_key == *team)
                    .map(|i| i.id.clone()),
            );
        }
    }
    for value in &selection.projects {
        let project = unique(
            "project",
            value,
            snapshot
                .projects
                .iter()
                .filter(|p| {
                    p.id.0 == *value
                        || p.name.eq_ignore_ascii_case(value)
                        || p.identifier
                            .as_ref()
                            .is_some_and(|id| id.eq_ignore_ascii_case(value))
                })
                .collect(),
        )?;
        result.project_ids.insert(project.id.clone());
    }
    for value in &selection.initiatives {
        let ids: BTreeSet<_> = snapshot
            .projects
            .iter()
            .flat_map(|p| &p.initiatives)
            .filter(|i| i.id == *value || i.name.eq_ignore_ascii_case(value))
            .map(|i| &i.id)
            .collect();
        let id = unique("initiative", value, ids.into_iter().collect())?;
        result.project_ids.extend(
            snapshot
                .projects
                .iter()
                .filter(|p| p.initiatives.iter().any(|i| &i.id == id))
                .map(|p| p.id.clone()),
        );
    }
    result.issue_ids.extend(
        snapshot
            .issues
            .iter()
            .filter(|i| {
                i.project_id
                    .as_ref()
                    .is_some_and(|p| result.project_ids.contains(p))
            })
            .map(|i| i.id.clone()),
    );
    let mut pending = VecDeque::new();
    for value in &selection.tickets {
        let issue = unique(
            "ticket",
            value,
            snapshot
                .issues
                .iter()
                .filter(|i| i.id.0 == *value || i.identifier.eq_ignore_ascii_case(value))
                .collect(),
        )?;
        result.issue_ids.insert(issue.id.clone());
        pending.push_back(issue.id.clone());
    }
    let mut children: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for issue in &snapshot.issues {
        if let Some(parent) = &issue.parent_id {
            children.entry(parent).or_default().push(&issue.id);
        }
    }
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        for child in children.get(&id).into_iter().flatten() {
            result.issue_ids.insert((*child).clone());
            pending.push_back((*child).clone());
        }
    }
    let by_id: BTreeMap<_, _> = snapshot.issues.iter().map(|i| (&i.id, i)).collect();
    if let Some(team) = &selection.team {
        result
            .issue_ids
            .retain(|id| by_id.get(id).is_some_and(|i| i.team_key == *team));
        result.project_ids.retain(|id| {
            snapshot
                .projects
                .iter()
                .any(|p| p.id == *id && p.team_keys.contains(team))
                || snapshot
                    .issues
                    .iter()
                    .any(|i| result.issue_ids.contains(&i.id) && i.project_id.as_ref() == Some(id))
        });
    }
    let mut checked = BTreeSet::new();
    for issue in snapshot
        .issues
        .iter()
        .filter(|i| result.issue_ids.contains(&i.id))
    {
        let mut seen = BTreeSet::new();
        let mut next = Some(&issue.id);
        while let Some(id) = next {
            if checked.contains(id) {
                break;
            }
            if !seen.insert(id) {
                return Err(Error::ParentCycle {
                    identifier: issue.identifier.clone(),
                });
            }
            let ancestor = by_id.get(id).ok_or_else(|| Error::MissingParent {
                identifier: issue.identifier.clone(),
                parent: (*id).clone(),
            })?;
            next = ancestor.parent_id.as_ref();
        }
        checked.extend(seen);
    }
    let parents: BTreeSet<_> = snapshot
        .issues
        .iter()
        .filter_map(|i| i.parent_id.as_ref())
        .collect();
    for issue in snapshot
        .issues
        .iter()
        .filter(|i| result.issue_ids.contains(&i.id) && !parents.contains(&i.id))
    {
        match issue.state_type {
            WorkflowStateType::Canceled | WorkflowStateType::Duplicate => {}
            WorkflowStateType::Completed => {
                result.leaf_ids.insert(issue.id.clone());
                result.done += 1;
            }
            WorkflowStateType::Triage
            | WorkflowStateType::Backlog
            | WorkflowStateType::Unstarted
            | WorkflowStateType::Started => {
                result.leaf_ids.insert(issue.id.clone());
                *result
                    .remaining_by_stage
                    .entry(issue.state.clone())
                    .or_default() += 1;
            }
        }
    }
    Ok(result)
}
