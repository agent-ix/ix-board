// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Cross-team blocker graph expansion, with cycle-safe paths.

use crate::program_review::ProgramScope;
use crate::{Error, Issue, IssueId, ProjectId, WorkflowStateType, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error as ThisError;

const PATH_LIMIT: usize = 2_000;

#[derive(Debug, ThisError)]
pub enum ChainError {
    #[error("issue not found: {0}")]
    IssueNotFound(String),
    #[error("project in program scope not found: {0}")]
    ProjectNotFound(String),
    #[error("invalid chain timestamp: {0}")]
    Timestamp(String),
    #[error(transparent)]
    Linear(#[from] Error),
}

impl From<ChainError> for Error {
    fn from(error: ChainError) -> Self {
        Self::ProgramReview {
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChainNode {
    pub id: IssueId,
    pub identifier: String,
    pub title: String,
    pub state: String,
    pub team: String,
    pub owner_lane: Option<String>,
    pub assignee_id: Option<String>,
    pub assignee_name: Option<String>,
    pub age_days: Option<i64>,
    pub blocked_by: Vec<String>,
    pub root_blocker: bool,
    pub unowned_root_blocker: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChainPath {
    pub start: String,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChainReport {
    pub starts: Vec<String>,
    pub nodes: Vec<ChainNode>,
    pub paths: Vec<ChainPath>,
    pub root_blockers: Vec<String>,
    pub unowned_root_blockers: Vec<String>,
    pub cycles: Vec<Vec<String>>,
    pub warnings: Vec<String>,
}

/// Resolve an exact issue identifier or issue UUID.
pub fn resolve_issue<'a>(
    snapshot: &'a WorkspaceSnapshot,
    selector: &str,
) -> Result<&'a Issue, ChainError> {
    snapshot
        .issues
        .iter()
        .find(|issue| issue.identifier.eq_ignore_ascii_case(selector) || issue.id.0 == selector)
        .ok_or_else(|| ChainError::IssueNotFound(selector.to_owned()))
}

/// Resolve all project issues explicitly listed in a program scope.
pub fn open_scope_issues<'a>(
    snapshot: &'a WorkspaceSnapshot,
    scope: &ProgramScope,
) -> Result<Vec<&'a Issue>, ChainError> {
    let mut project_ids = BTreeSet::<ProjectId>::new();
    for placement in &scope.projects {
        let projects = snapshot
            .projects
            .iter()
            .filter(|project| project.name.eq_ignore_ascii_case(&placement.name))
            .collect::<Vec<_>>();
        match projects.as_slice() {
            [project] => {
                project_ids.insert(project.id.clone());
            }
            [] => return Err(ChainError::ProjectNotFound(placement.name.clone())),
            _ => return Err(ChainError::ProjectNotFound(placement.name.clone())),
        }
    }
    let mut issues = snapshot
        .issues
        .iter()
        .filter(|issue| {
            issue
                .project_id
                .as_ref()
                .is_some_and(|id| project_ids.contains(id))
                && !is_terminal(issue.state_type)
        })
        .collect::<Vec<_>>();
    issues.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    Ok(issues)
}

/// Expand dependency chains for one or more Linear issue IDs.
pub fn evaluate_chain(
    snapshot: &WorkspaceSnapshot,
    start_ids: &[IssueId],
    scope: Option<&ProgramScope>,
    now_epoch: f64,
) -> Result<ChainReport, ChainError> {
    let by_id = snapshot
        .issues
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect::<BTreeMap<_, _>>();
    let mut starts = start_ids.to_vec();
    starts.sort_by(|left, right| {
        by_id
            .get(left)
            .map(|issue| issue.identifier.as_str())
            .cmp(&by_id.get(right).map(|issue| issue.identifier.as_str()))
    });
    starts.dedup();
    for start in &starts {
        if !by_id.contains_key(start) {
            return Err(ChainError::IssueNotFound(start.0.clone()));
        }
    }

    let mut discovered = BTreeSet::new();
    let mut edges = BTreeMap::<IssueId, Vec<IssueId>>::new();
    let mut unresolved = BTreeSet::new();
    let mut queue: VecDeque<_> = starts.iter().cloned().collect();
    let mut warnings = Vec::new();
    while let Some(issue_id) = queue.pop_front() {
        if !discovered.insert(issue_id.clone()) {
            continue;
        }
        let Some(issue) = by_id.get(&issue_id).copied() else {
            continue;
        };
        let mut blockers = Vec::new();
        for blocker_id in &issue.blocked_by {
            if by_id.contains_key(blocker_id) {
                blockers.push(blocker_id.clone());
                queue.push_back(blocker_id.clone());
            } else {
                unresolved.insert(issue_id.clone());
                warnings.push(format!(
                    "blocker {} referenced by {} is unavailable in the Linear snapshot",
                    blocker_id.0, issue.identifier
                ));
            }
        }
        blockers.sort_by(|left, right| by_id[left].identifier.cmp(&by_id[right].identifier));
        blockers.dedup();
        edges.insert(issue_id, blockers);
    }

    let mut root_ids = discovered
        .iter()
        .filter(|id| edges.get(*id).is_some_and(Vec::is_empty) && !unresolved.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    root_ids.sort_by(|left, right| by_id[left].identifier.cmp(&by_id[right].identifier));
    let lane_map = lane_map(scope);
    let node_by_id = discovered
        .iter()
        .map(|id| {
            let issue = by_id[id];
            let lane = lane_map
                .get(&issue.team_key.0.to_ascii_lowercase())
                .cloned();
            let age_days = issue
                .created_at
                .as_deref()
                .map(|timestamp| elapsed_days(timestamp, now_epoch))
                .transpose()?;
            let is_root = root_ids.contains(id);
            let unowned = is_root && (issue.assignee_id.is_none() || lane.is_none());
            let blockers = edges[id]
                .iter()
                .filter_map(|blocker| by_id.get(blocker).map(|issue| issue.identifier.clone()))
                .collect();
            Ok::<_, ChainError>((
                id.clone(),
                ChainNode {
                    id: id.clone(),
                    identifier: issue.identifier.clone(),
                    title: issue.title.clone(),
                    state: issue.state_name.clone(),
                    team: issue.team_key.0.clone(),
                    owner_lane: lane,
                    assignee_id: issue.assignee_id.clone(),
                    assignee_name: issue.assignee_name.clone(),
                    age_days,
                    blocked_by: blockers,
                    root_blocker: is_root,
                    unowned_root_blocker: unowned,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, ChainError>>()?;

    let mut path_ids = BTreeSet::<Vec<IssueId>>::new();
    let mut cycles_by_id = BTreeSet::<Vec<IssueId>>::new();
    for start in &starts {
        let mut path = Vec::new();
        let mut active = Vec::new();
        walk_paths(
            start,
            &edges,
            &unresolved,
            &mut path,
            &mut active,
            &mut path_ids,
            &mut cycles_by_id,
            &mut warnings,
        );
        if path_ids.len() >= PATH_LIMIT {
            warnings.push(format!(
                "path output limited to {PATH_LIMIT} distinct chains"
            ));
            break;
        }
    }
    let paths = path_ids
        .into_iter()
        .map(|ids| ChainPath {
            start: node_by_id[&ids[0]].identifier.clone(),
            issues: ids
                .iter()
                .map(|id| node_by_id[id].identifier.clone())
                .collect(),
        })
        .collect();
    let cycles = cycles_by_id
        .into_iter()
        .map(|ids| {
            ids.iter()
                .map(|id| node_by_id[id].identifier.clone())
                .collect()
        })
        .collect();
    let mut nodes = node_by_id.into_values().collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    let mut root_blockers = root_ids
        .iter()
        .map(|id| by_id[id].identifier.clone())
        .collect::<Vec<_>>();
    root_blockers.sort();
    let mut unowned_root_blockers = root_ids
        .iter()
        .filter(|id| node_by_id_contains_unowned(&nodes, id))
        .map(|id| by_id[id].identifier.clone())
        .collect::<Vec<_>>();
    unowned_root_blockers.sort();
    warnings.sort();
    warnings.dedup();

    Ok(ChainReport {
        starts: starts
            .iter()
            .map(|id| by_id[id].identifier.clone())
            .collect(),
        nodes,
        paths,
        root_blockers,
        unowned_root_blockers,
        cycles,
        warnings,
    })
}

fn node_by_id_contains_unowned(nodes: &[ChainNode], id: &IssueId) -> bool {
    nodes
        .iter()
        .find(|node| node.id == *id)
        .is_some_and(|node| node.unowned_root_blocker)
}

fn lane_map(scope: Option<&ProgramScope>) -> BTreeMap<String, String> {
    scope
        .into_iter()
        .flat_map(|scope| scope.lanes.iter())
        .filter_map(|lane| {
            let parts = lane.address.split(':').collect::<Vec<_>>();
            (parts.len() == 3).then(|| (parts[1].to_ascii_lowercase(), lane.address.clone()))
        })
        .collect()
}

fn walk_paths(
    current: &IssueId,
    edges: &BTreeMap<IssueId, Vec<IssueId>>,
    unresolved: &BTreeSet<IssueId>,
    path: &mut Vec<IssueId>,
    active: &mut Vec<IssueId>,
    paths: &mut BTreeSet<Vec<IssueId>>,
    cycles: &mut BTreeSet<Vec<IssueId>>,
    warnings: &mut Vec<String>,
) {
    if active.len() >= edges.len().max(1) {
        return;
    }
    path.push(current.clone());
    active.push(current.clone());
    let blockers = edges.get(current).map(Vec::as_slice).unwrap_or_default();
    if blockers.is_empty() || unresolved.contains(current) {
        paths.insert(path.clone());
    } else {
        for blocker in blockers {
            if let Some(cycle_start) = active.iter().position(|id| id == blocker) {
                let mut cycle = active[cycle_start..].to_vec();
                cycle.push(blocker.clone());
                cycles.insert(canonical_cycle(cycle));
                continue;
            }
            if paths.len() >= PATH_LIMIT {
                warnings.push(format!(
                    "path output limited to {PATH_LIMIT} distinct chains"
                ));
                break;
            }
            walk_paths(
                blocker, edges, unresolved, path, active, paths, cycles, warnings,
            );
        }
    }
    active.pop();
    path.pop();
}

fn canonical_cycle(cycle: Vec<IssueId>) -> Vec<IssueId> {
    let Some((_, body)) = cycle.split_last() else {
        return cycle;
    };
    if body.is_empty() {
        return cycle;
    }
    let mut best = body.to_vec();
    for offset in 1..body.len() {
        let rotated = body[offset..]
            .iter()
            .chain(body[..offset].iter())
            .cloned()
            .collect::<Vec<_>>();
        if rotated < best {
            best = rotated;
        }
    }
    best.push(best[0].clone());
    best
}

fn is_terminal(state_type: WorkflowStateType) -> bool {
    matches!(
        state_type,
        WorkflowStateType::Completed | WorkflowStateType::Canceled | WorkflowStateType::Duplicate
    )
}

fn elapsed_days(timestamp: &str, now_epoch: f64) -> Result<i64, ChainError> {
    let epoch = crate::drift::timestamp_epoch(timestamp)
        .map_err(|_| ChainError::Timestamp(timestamp.to_owned()))?;
    Ok(((now_epoch - epoch) / 86_400.0).floor().max(0.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Milestone, Project, TeamKey};

    fn scope() -> ProgramScope {
        toml::from_str(
            r#"
program = "Review"

[[projects]]
name = "Review Project"
primary_initiative = "Review"

[[lanes]]
address = "terra:QSL:code"
owns = "QSL work"

[[lanes]]
address = "draco:IR:code"
owns = "IR work"

[[lanes]]
address = "terra:ECZ:code"
owns = "ECZ work"
"#,
        )
        .expect("recorded program scope")
    }

    fn snapshot() -> WorkspaceSnapshot {
        let issues: Vec<Issue> =
            serde_json::from_str(include_str!("../tests/fixtures/chain/linear-issues.json"))
                .expect("recorded Linear issue fixture");
        WorkspaceSnapshot {
            projects: vec![Project {
                id: ProjectId("project-chain".to_owned()),
                identifier: Some("SWM-P1".to_owned()),
                name: "Review Project".to_owned(),
                description: String::new(),
                content: None,
                updated_at: None,
                status: "Started".to_owned(),
                priority: 0,
                sort_order: 0.0,
                target_date: None,
                team_keys: vec![TeamKey("SWM".to_owned())],
                milestones: Vec::<Milestone>::new(),
                initiatives: Vec::new(),
            }],
            issues,
            fetched_at: 0,
        }
    }

    fn now() -> f64 {
        crate::drift::timestamp_epoch("2026-10-21T00:00:00Z").expect("fixed test time")
    }

    #[test]
    fn fixture_expands_cross_team_chain_and_prioritizes_unowned_root() {
        let snapshot = snapshot();
        let scope = scope();
        let report = evaluate_chain(
            &snapshot,
            &[IssueId("issue-qsl-1".to_owned())],
            Some(&scope),
            now(),
        )
        .expect("evaluate cross-team dependency chain");

        assert_eq!(report.paths.len(), 1);
        assert_eq!(report.paths[0].issues, ["QSL-1", "IR-2", "ECZ-3"]);
        assert_eq!(report.root_blockers, ["ECZ-3"]);
        assert_eq!(report.unowned_root_blockers, ["ECZ-3"]);
        let root = report
            .nodes
            .iter()
            .find(|node| node.identifier == "ECZ-3")
            .expect("root node");
        assert_eq!(root.team, "ECZ");
        assert_eq!(root.owner_lane.as_deref(), Some("terra:ECZ:code"));
        assert_eq!(root.age_days, Some(41));
        let start = report
            .nodes
            .iter()
            .find(|node| node.identifier == "QSL-1")
            .expect("start node");
        assert_eq!(start.assignee_name.as_deref(), Some("Quill Dev"));
    }

    #[test]
    fn fixture_reports_dependency_cycles_without_recursing_forever() {
        let snapshot = snapshot();
        let report = evaluate_chain(&snapshot, &[IssueId("issue-qsl-4".to_owned())], None, now())
            .expect("evaluate cycle");
        assert_eq!(report.cycles, [vec!["QSL-4", "QSL-5", "QSL-4"]]);
        assert!(report.root_blockers.is_empty());
        assert!(report.paths.is_empty());
    }

    #[test]
    fn a_program_scope_selects_open_tickets_from_its_projects() {
        let snapshot = snapshot();
        let issues = open_scope_issues(&snapshot, &scope()).expect("scope issues");
        assert_eq!(issues.len(), 5);
        assert!(
            issues
                .iter()
                .all(|issue| issue.state_type != WorkflowStateType::Completed)
        );
    }
}
