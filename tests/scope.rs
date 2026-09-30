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
