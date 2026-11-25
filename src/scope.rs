// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Explicit work selection and deduplicated leaf-task progress.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::read_bound::Budget;
use crate::{Error, IssueId, Project, ProjectId, TeamKey, WorkflowStateType, WorkspaceSnapshot};

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
    resolve_initiative_id_within(snapshot, value, Budget::unbounded())
}

/// [`resolve_initiative_id`] that stops with [`Error::ReadTimeout`] once `budget` has expired.
pub(crate) fn resolve_initiative_id_within(
    snapshot: &WorkspaceSnapshot,
    value: &str,
    budget: Budget<'_>,
) -> Result<String, Error> {
    let mut ids = BTreeSet::new();
    for project in &snapshot.projects {
        budget.check()?;
        for initiative in &project.initiatives {
            budget.check()?;
            if initiative.id == value
                || initiative.identifier.eq_ignore_ascii_case(value)
                || initiative.name.eq_ignore_ascii_case(value)
            {
                ids.insert(initiative.id.clone());
            }
        }
    }
    unique("initiative", value, ids.into_iter().collect())
}

/// Resolve a project selector, preferring its UUID or identifier over a colliding name.
pub(crate) fn resolve_project_id(
    snapshot: &WorkspaceSnapshot,
    value: &str,
) -> Result<ProjectId, Error> {
    resolve_project_id_within(snapshot, value, Budget::unbounded())
}

/// [`resolve_project_id`] that stops with [`Error::ReadTimeout`] once `budget` has expired.
pub(crate) fn resolve_project_id_within(
    snapshot: &WorkspaceSnapshot,
    value: &str,
    budget: Budget<'_>,
) -> Result<ProjectId, Error> {
    let mut exact = Vec::new();
    for project in &snapshot.projects {
        budget.check()?;
        if project.id.0.eq_ignore_ascii_case(value)
            || project
                .identifier
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case(value))
        {
            exact.push(project.id.clone());
        }
    }
    if !exact.is_empty() {
        return unique("project", value, exact);
    }
    let mut named = Vec::new();
    for project in &snapshot.projects {
        budget.check()?;
        if project.name.eq_ignore_ascii_case(value) {
            named.push(project.id.clone());
        }
    }
    unique("project", value, named)
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
    resolve_milestone_id_within(snapshot, value, allowed_projects, Budget::unbounded())
}

/// [`resolve_milestone_id_in_projects`] that stops with [`Error::ReadTimeout`] once
/// `budget` has expired.
pub(crate) fn resolve_milestone_id_within(
    snapshot: &WorkspaceSnapshot,
    value: &str,
    allowed_projects: Option<&BTreeSet<ProjectId>>,
    budget: Budget<'_>,
) -> Result<String, Error> {
    let mut projects = Vec::new();
    let mut exact = BTreeSet::new();
    for project in &snapshot.projects {
        budget.check()?;
        if allowed_projects.is_none_or(|ids| ids.contains(&project.id)) {
            projects.push(project);
            for milestone in &project.milestones {
                budget.check()?;
                if milestone.id.eq_ignore_ascii_case(value) {
                    exact.insert(milestone.id.clone());
                }
            }
        }
    }
    if !exact.is_empty() {
        return unique("milestone", value, exact.into_iter().collect());
    }
    let mut names = BTreeMap::new();
    // A slash may belong to either name. Consider the whole name and every
    // split point; deduplicate by canonical milestone ID.
    for project in &projects {
        budget.check()?;
        collect_named_milestones(&mut names, project, value.trim(), budget)?;
    }
    for (index, character) in value.char_indices() {
        if character != '/' {
            continue;
        }
        let prefix = value[..index].trim();
        let suffix = value[index + 1..].trim();
        let mut by_identifier = Vec::new();
        let mut by_name = Vec::new();
        for project in &projects {
            budget.check()?;
            if project.id.0.eq_ignore_ascii_case(prefix)
                || project
                    .identifier
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(prefix))
            {
                by_identifier.push(*project);
            }
            if project.name.eq_ignore_ascii_case(prefix) {
                by_name.push(*project);
            }
        }
        let matches = if by_identifier.is_empty() {
            by_name
        } else {
            by_identifier
        };
        for project in matches {
            budget.check()?;
            collect_named_milestones(&mut names, project, suffix, budget)?;
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

/// Records, by milestone ID, each milestone of `project` named `wanted`, with the label
/// used to list an ambiguous selector's candidates. Checks `budget` for every milestone.
fn collect_named_milestones(
    names: &mut BTreeMap<String, String>,
    project: &Project,
    wanted: &str,
    budget: Budget<'_>,
) -> Result<(), Error> {
    for milestone in &project.milestones {
        budget.check()?;
        if milestone.name.eq_ignore_ascii_case(wanted) {
            names.insert(
                milestone.id.clone(),
                format!(
                    "{} ({}) / {} ({})",
                    project.name, project.id.0, milestone.name, milestone.id
                ),
            );
        }
    }
    Ok(())
}

pub(crate) fn resolve_ticket_id(
    snapshot: &WorkspaceSnapshot,
    value: &str,
) -> Result<IssueId, Error> {
    resolve_ticket_id_within(snapshot, value, Budget::unbounded())
}

/// [`resolve_ticket_id`] that stops with [`Error::ReadTimeout`] once `budget` has expired.
pub(crate) fn resolve_ticket_id_within(
    snapshot: &WorkspaceSnapshot,
    value: &str,
    budget: Budget<'_>,
) -> Result<IssueId, Error> {
    let mut matches = Vec::new();
    for issue in &snapshot.issues {
        budget.check()?;
        if issue.id.0.eq_ignore_ascii_case(value) || issue.identifier.eq_ignore_ascii_case(value) {
            matches.push(issue.id.clone());
        }
    }
    unique("ticket", value, matches)
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
    let leaves = classify_leaves(snapshot, &result.issue_ids)?;
    result.leaf_ids = leaves.leaf_ids;
    result.done = leaves.done;
    result.remaining_by_stage = leaves.remaining_by_stage;
    Ok(result)
}

/// Leaf tasks among a set of selected issues, classified with full snapshot ancestry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LeafClassification {
    pub(crate) leaf_ids: BTreeSet<IssueId>,
    pub(crate) done: usize,
    pub(crate) remaining_by_stage: BTreeMap<String, usize>,
}

/// Validates the ancestry of `issue_ids` and classifies their leaf tasks.
///
/// A parent is judged against every issue in the snapshot, never only the selected
/// ones, so excluding a child can never turn its parent into a leaf.
pub(crate) fn classify_leaves(
    snapshot: &WorkspaceSnapshot,
    issue_ids: &BTreeSet<IssueId>,
) -> Result<LeafClassification, Error> {
    classify_leaves_within(snapshot, issue_ids, Budget::unbounded())
}

/// [`classify_leaves`] that stops with [`Error::ReadTimeout`] once `budget` has expired.
pub(crate) fn classify_leaves_within(
    snapshot: &WorkspaceSnapshot,
    issue_ids: &BTreeSet<IssueId>,
    budget: Budget<'_>,
) -> Result<LeafClassification, Error> {
    let by_id: BTreeMap<_, _> = snapshot.issues.iter().map(|i| (&i.id, i)).collect();
    let mut checked = BTreeSet::new();
    for issue in snapshot.issues.iter().filter(|i| issue_ids.contains(&i.id)) {
        let mut seen = BTreeSet::new();
        let mut next = Some(&issue.id);
        while let Some(id) = next {
            budget.check()?;
            if checked.contains(id) {
                break;
            }
            if !seen.insert(id) {
                return Err(Error::ParentCycle {
                    identifier: issue.identifier.clone(),
                });
            }
            // An unavailable parent ends the ancestry here (FR-045); reads that report
            // warnings name it.
            next = by_id
                .get(id)
                .and_then(|ancestor| ancestor.parent_id.as_ref());
        }
        checked.extend(seen);
    }
    let parents: BTreeSet<_> = snapshot
        .issues
        .iter()
        .filter_map(|i| i.parent_id.as_ref())
        .collect();
    let mut result = LeafClassification::default();
    for issue in snapshot
        .issues
        .iter()
        .filter(|i| issue_ids.contains(&i.id) && !parents.contains(&i.id))
    {
        budget.check()?;
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
