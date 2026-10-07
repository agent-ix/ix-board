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
    /// Initiative UUIDs, identifiers, or exact names.
    pub initiatives: Vec<String>,
    /// Project UUIDs, identifiers, or exact names.
    pub projects: Vec<String>,
    /// Milestone UUIDs or exact names; selected issues belong to that milestone.
    pub milestones: Vec<String>,
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

/// Resolve one initiative selector to its canonical Linear UUID.
pub(crate) fn resolve_initiative_id(
    snapshot: &WorkspaceSnapshot,
    value: &str,
) -> Result<String, Error> {
    let ids = snapshot
        .projects
        .iter()
        .flat_map(|project| &project.initiatives)
        .filter(|initiative| {
            initiative.id == value
                || initiative.identifier.eq_ignore_ascii_case(value)
                || initiative.name.eq_ignore_ascii_case(value)
        })
        .map(|initiative| initiative.id.clone())
        .collect::<BTreeSet<_>>();
    unique("initiative", value, ids.into_iter().collect())
}

/// Resolve a project selector, preferring its UUID or identifier over a colliding name.
pub(crate) fn resolve_project_id(
    snapshot: &WorkspaceSnapshot,
    value: &str,
) -> Result<ProjectId, Error> {
    let exact = snapshot
        .projects
        .iter()
        .filter(|p| {
            p.id.0.eq_ignore_ascii_case(value)
                || p.identifier
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(value))
        })
        .map(|p| p.id.clone())
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return unique("project", value, exact);
    }
    unique(
        "project",
        value,
        snapshot
            .projects
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(value))
            .map(|p| p.id.clone())
            .collect(),
    )
}

/// Resolve a UUID, globally unique name, or project-qualified name.
pub(crate) fn resolve_milestone_id(
    snapshot: &WorkspaceSnapshot,
    value: &str,
) -> Result<String, Error> {
    resolve_milestone_id_in_projects(snapshot, value, None)
}

/// The same selector grammar restricted to projects already in a focus.
pub(crate) fn resolve_milestone_id_in_projects(
    snapshot: &WorkspaceSnapshot,
    value: &str,
    allowed_projects: Option<&BTreeSet<ProjectId>>,
) -> Result<String, Error> {
    let projects = snapshot
        .projects
        .iter()
        .filter(|p| allowed_projects.is_none_or(|ids| ids.contains(&p.id)))
        .collect::<Vec<_>>();
    let exact = snapshot
        .projects
        .iter()
        .filter(|p| allowed_projects.is_none_or(|ids| ids.contains(&p.id)))
        .flat_map(|p| &p.milestones)
        .filter(|m| m.id.eq_ignore_ascii_case(value))
        .map(|m| m.id.clone())
        .collect::<BTreeSet<_>>();
    if !exact.is_empty() {
        return unique("milestone", value, exact.into_iter().collect());
    }
    let mut names = BTreeMap::new();
    // A slash may belong to either name. Consider the whole name and every
    // split point; deduplicate by canonical milestone ID.
    for project in &projects {
        for milestone in &project.milestones {
            if milestone.name.eq_ignore_ascii_case(value.trim()) {
                names.insert(
                    milestone.id.clone(),
                    format!(
                        "{} ({}) / {} ({})",
                        project.name, project.id.0, milestone.name, milestone.id
                    ),
                );
            }
        }
    }
    for (index, character) in value.char_indices() {
        if character != '/' {
            continue;
        }
        let prefix = value[..index].trim();
        let suffix = value[index + 1..].trim();
        let by_identifier = projects
            .iter()
            .filter(|p| {
                p.id.0.eq_ignore_ascii_case(prefix)
                    || p.identifier
                        .as_deref()
                        .is_some_and(|id| id.eq_ignore_ascii_case(prefix))
            })
            .copied()
            .collect::<Vec<_>>();
        let matches = if by_identifier.is_empty() {
            projects
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case(prefix))
                .copied()
                .collect::<Vec<_>>()
        } else {
            by_identifier
        };
        for project in matches {
            for milestone in &project.milestones {
                if milestone.name.eq_ignore_ascii_case(suffix) {
                    names.insert(
                        milestone.id.clone(),
                        format!(
                            "{} ({}) / {} ({})",
                            project.name, project.id.0, milestone.name, milestone.id
                        ),
                    );
                }
            }
        }
    }
    match names.len() {
        1 => names
            .into_keys()
            .next()
            .ok_or_else(|| Error::ScopeSelection {
                kind: "milestone",
                identifier: value.to_owned(),
                matches: 0,
            }),
        0 => Err(Error::ScopeSelection {
            kind: "milestone",
            identifier: value.to_owned(),
            matches: 0,
        }),
        _ => Err(Error::AmbiguousMilestone {
            selector: value.to_owned(),
            candidates: names.into_values().collect::<Vec<_>>().join("; "),
        }),
    }
}

pub(crate) fn resolve_ticket_id(
    snapshot: &WorkspaceSnapshot,
    value: &str,
) -> Result<IssueId, Error> {
    unique(
        "ticket",
        value,
        snapshot
            .issues
            .iter()
            .filter(|i| {
                i.id.0.eq_ignore_ascii_case(value) || i.identifier.eq_ignore_ascii_case(value)
            })
            .map(|i| i.id.clone())
            .collect(),
    )
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
            && selection.milestones.is_empty()
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
        result
            .project_ids
            .insert(resolve_project_id(snapshot, value)?);
    }
    for value in &selection.initiatives {
        let id = resolve_initiative_id(snapshot, value)?;
        result.project_ids.extend(
            snapshot
                .projects
                .iter()
                .filter(|p| p.initiatives.iter().any(|i| i.id == id))
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
    for value in &selection.milestones {
        let id = resolve_milestone_id(snapshot, value)?;
        // Add the containing project only after full-project expansion above.
        // Moving this earlier would turn a milestone into the whole project.
        for project in &snapshot.projects {
            if project
                .milestones
                .iter()
                .any(|milestone| milestone.id == id)
            {
                result.project_ids.insert(project.id.clone());
            }
        }
        result.issue_ids.extend(
            snapshot
                .issues
                .iter()
                .filter(|issue| issue.project_milestone_id.as_deref() == Some(id.as_str()))
                .map(|issue| issue.id.clone()),
        );
    }
    let mut pending = VecDeque::new();
    for value in &selection.tickets {
        let id = resolve_ticket_id(snapshot, value)?;
        result.issue_ids.insert(id.clone());
        pending.push_back(id);
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
    // Selected tickets contribute their containing projects for status rows,
    // after full-project issue expansion so ticket focus stays partial.
    result.project_ids.extend(
        result
            .issue_ids
            .iter()
            .filter_map(|id| by_id.get(id).and_then(|issue| issue.project_id.clone())),
    );
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
