// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Acceptance tests for explicit session work selection (FR-010).

use ix_board::{
    Initiative, Issue, IssueId, Project, ProjectId, ScopeSelection, TeamKey, WorkflowStateType,
    WorkspaceSnapshot, select_scope,
};

fn project(id: &str, name: &str) -> Project {
    Project {
        id: ProjectId(id.into()),
        identifier: None,
        name: name.into(),
        description: String::new(),
        content: None,
        status: "Started".into(),
        priority: 2,
        sort_order: 0.0,
        team_keys: vec![TeamKey("SWM".into())],
        milestones: vec![],
        initiatives: vec![Initiative {
            id: "initiative".into(),
            name: "Delivery".into(),
        }],
    }
}

fn issue(id: &str, parent: Option<&str>, category: WorkflowStateType) -> Issue {
    Issue {
        id: IssueId(id.into()),
        identifier: id.into(),
        title: id.into(),
        description: None,
        state: "Review".into(),
        state_type: category,
        parent_id: parent.map(|id| IssueId(id.into())),
        team_key: TeamKey("SWM".into()),
        project_id: Some(ProjectId("p".into())),
        project_milestone_id: None,
        priority: 2.0,
        sort_order: 0.0,
        blocked_by: vec![],
    }
}

/// Trace: FR-010-AC-1, FR-010-AC-2
#[test]
fn tc_010_01_overlapping_selectors_count_leaves_once_and_exclude_terminal_nonwork() {
    let snapshot = WorkspaceSnapshot {
        projects: vec![project("p", "Product")],
        issues: vec![
            issue("SWM-1", None, WorkflowStateType::Completed),
            issue("SWM-2", Some("SWM-1"), WorkflowStateType::Completed),
            issue("SWM-3", None, WorkflowStateType::Started),
            issue("SWM-4", None, WorkflowStateType::Canceled),
            issue("SWM-5", None, WorkflowStateType::Duplicate),
        ],
        fetched_at: 10,
    };
    let selection = ScopeSelection {
        projects: vec!["Product".into()],
        initiatives: vec!["Delivery".into()],
        tickets: vec!["SWM-1".into()],
        ..ScopeSelection::default()
    };
    let scope = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(scope.issue_ids.len(), 5);
    assert_eq!(
        scope.leaf_ids,
        [IssueId("SWM-2".into()), IssueId("SWM-3".into())].into()
    );
    assert_eq!(scope.done, 1);
    assert_eq!(scope.remaining_by_stage.get("Review"), Some(&1));
    let subtree = select_scope(
        &snapshot,
        &ScopeSelection {
            tickets: vec!["SWM-1".into()],
            ..ScopeSelection::default()
        },
    )
    .unwrap();
    assert_eq!(subtree.leaf_ids, [IssueId("SWM-2".into())].into());
}

/// Trace: FR-010-AC-3
#[test]
fn tc_010_02_unknown_ambiguous_and_cyclic_scope_is_refused() {
    let mut snapshot = WorkspaceSnapshot {
        projects: vec![project("p", "Product"), project("q", "Product")],
        issues: vec![],
        fetched_at: 0,
    };
    for name in ["Missing", "Product"] {
        assert!(matches!(
            select_scope(
                &snapshot,
                &ScopeSelection {
                    projects: vec![name.into()],
                    ..ScopeSelection::default()
                }
            ),
            Err(ix_board::Error::ScopeSelection { .. })
        ));
    }
    snapshot.issues = vec![
        issue("SWM-1", Some("SWM-2"), WorkflowStateType::Started),
        issue("SWM-2", Some("SWM-1"), WorkflowStateType::Started),
    ];
    assert!(matches!(
        select_scope(
            &snapshot,
            &ScopeSelection {
                tickets: vec!["SWM-1".into()],
                ..ScopeSelection::default()
            }
        ),
        Err(ix_board::Error::ParentCycle { .. })
    ));
    snapshot.issues.pop();
    assert!(matches!(
        select_scope(
            &snapshot,
            &ScopeSelection {
                tickets: vec!["SWM-1".into()],
                ..ScopeSelection::default()
            }
        ),
        Err(ix_board::Error::MissingParent { .. })
    ));
}

/// Trace: FR-010-AC-5
#[test]
fn team_boundary_includes_unprojected_work_and_intersects_project_focus() {
    let mut foreign = issue("AGE-1", Some("SWM-1"), WorkflowStateType::Started);
    foreign.team_key = TeamKey("AGE".into());
    let mut unprojected = issue("SWM-3", None, WorkflowStateType::Started);
    unprojected.project_id = None;
    let mut unrelated = project("q", "Other team");
    unrelated.team_keys = vec![TeamKey("AGE".into())];
    let snapshot = WorkspaceSnapshot {
        projects: vec![project("p", "Product"), unrelated],
        issues: vec![
            issue("SWM-1", None, WorkflowStateType::Completed),
            issue("SWM-2", None, WorkflowStateType::Completed),
            foreign,
            unprojected,
        ],
        fetched_at: 0,
    };
    let mut selection = ScopeSelection {
        team: Some(TeamKey("SWM".into())),
        ..ScopeSelection::default()
    };
    let all = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(all.issue_ids.len(), 3);
    assert_eq!(all.project_ids, [ProjectId("p".into())].into());
    // Global ancestry prevents a parent with a foreign child becoming a leaf.
    assert_eq!(
        all.leaf_ids,
        [IssueId("SWM-2".into()), IssueId("SWM-3".into())].into()
    );
    assert_eq!(all.done, 1);
    selection.projects = vec!["Product".into()];
    let focused = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(focused.issue_ids.len(), 2);
    assert_eq!(focused.leaf_ids, [IssueId("SWM-2".into())].into());
    selection.projects.clear();
    selection.initiatives = vec!["Delivery".into()];
    assert_eq!(
        select_scope(&snapshot, &selection).unwrap().issue_ids,
        focused.issue_ids
    );
    selection.initiatives.clear();
    selection.tickets = vec!["SWM-1".into()];
    let subtree = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(subtree.issue_ids, [IssueId("SWM-1".into())].into());
    assert!(subtree.leaf_ids.is_empty());
    selection.team = Some(TeamKey("MISSING".into()));
    assert!(matches!(
        select_scope(&snapshot, &selection),
        Err(ix_board::Error::ScopeSelection {
            kind: "team",
            matches: 0,
            ..
        })
    ));
}

/// Trace: FR-010-AC-6
#[test]
fn project_focus_does_not_expand_into_descendants_in_other_projects() {
    let mut moved_child = issue("SWM-2", Some("SWM-3"), WorkflowStateType::Started);
    moved_child.project_id = Some(ProjectId("q".into()));
    let mut other = project("q", "Other");
    other.initiatives.clear();
    let snapshot = WorkspaceSnapshot {
        projects: vec![project("p", "Product"), other],
        issues: vec![
            issue("SWM-1", None, WorkflowStateType::Started),
            issue("SWM-3", Some("SWM-1"), WorkflowStateType::Started),
            moved_child,
        ],
        fetched_at: 0,
    };
    let mut selection = ScopeSelection {
        projects: vec!["Product".into()],
        team: Some(TeamKey("SWM".into())),
        ..Default::default()
    };
    let focused = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(
        focused.issue_ids,
        [IssueId("SWM-1".into()), IssueId("SWM-3".into())].into()
    );
    assert_eq!(focused.project_ids, [ProjectId("p".into())].into());
    assert!(focused.leaf_ids.is_empty());
    selection.projects.clear();
    selection.initiatives = vec!["Delivery".into()];
    assert_eq!(
        select_scope(&snapshot, &selection).unwrap().issue_ids,
        focused.issue_ids
    );
    selection.initiatives.clear();
    selection.projects = vec!["Product".into()];
    selection.tickets = vec!["SWM-1".into()];
    let subtree = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(subtree.issue_ids.len(), 3);
    assert_eq!(subtree.leaf_ids, [IssueId("SWM-2".into())].into());
    selection.tickets.clear();
    selection.projects.clear();
    let whole_team = select_scope(&snapshot, &selection).unwrap();
    assert_eq!(whole_team.issue_ids.len(), 3);
    assert_eq!(whole_team.leaf_ids, subtree.leaf_ids);
}
