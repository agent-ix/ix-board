// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! External consumers retain the pre-summary roadmap construction API.

use ix_board::linear_summary::{InitiativeSummary, ProjectSummary, SummarySnapshot};
use ix_board::program_review::{
    RoadmapInitiative, RoadmapProject, RoadmapUnplacedProject, build_roadmap, build_summary_roadmap,
};
use ix_board::{Initiative, Project, ProjectId, ProjectStatusCategory, WorkspaceSnapshot};

/// Trace: FR-020-AC-1, FR-020-AC-2
#[test]
fn unchanged_external_struct_literals_and_legacy_builder_remain_supported() {
    let project = RoadmapProject {
        name: "Project".to_owned(),
        status: "Started".to_owned(),
        progress_percent: 25,
        open: 3,
        done: 1,
        canceled: 0,
        last_activity: None,
        also_in: Vec::new(),
    };
    let initiative = RoadmapInitiative {
        name: "Program".to_owned(),
        projects: vec![project],
    };
    let unplaced = RoadmapUnplacedProject {
        name: "Unplaced".to_owned(),
        status: "Planned".to_owned(),
        progress_percent: 0,
        open: 0,
        done: 0,
        canceled: 0,
        last_activity: None,
    };
    assert_eq!(initiative.projects[0].progress_percent, 25);
    assert_eq!(unplaced.name, "Unplaced");
    let roadmap = build_roadmap(&[], &WorkspaceSnapshot::default(), None);
    assert!(roadmap.initiatives.is_empty());
    assert!(roadmap.unplaced_projects.is_empty());
}

fn native_project(
    id: &str,
    status: &str,
    progress: f64,
    initiative: Option<&Initiative>,
) -> ProjectSummary {
    ProjectSummary {
        project: Project {
            id: ProjectId(id.to_owned()),
            identifier: None,
            name: "Duplicate".to_owned(),
            updated_at: Some(format!("{id}-updated")),
            description: String::new(),
            content: None,
            status: status.to_owned(),
            status_name: Some(status.to_owned()),
            status_category: ProjectStatusCategory::Planned,
            priority: 0,
            sort_order: 0.0,
            target_date: None,
            team_keys: Vec::new(),
            milestones: Vec::new(),
            initiatives: initiative.into_iter().cloned().collect(),
        },
        progress: Some(progress),
        updated_at: format!("{id}-updated"),
        archived: false,
        trashed: false,
    }
}

/// Trace: FR-020-AC-1, FR-020-AC-2, FR-020-AC-4
#[test]
fn native_rows_join_duplicate_project_names_by_stable_id_for_placed_and_unplaced_rows() {
    let initiative = Initiative {
        id: "program-id".to_owned(),
        identifier: String::new(),
        name: "Program".to_owned(),
    };
    let summary = SummarySnapshot {
        // Reverse ID order exercises sorting rows together with their private identities.
        projects: vec![
            native_project("placed-b", "Placed B", 0.75, Some(&initiative)),
            native_project("placed-a", "Placed A", 0.25, Some(&initiative)),
            native_project("unplaced-b", "Unplaced B", 0.8, None),
            native_project("unplaced-a", "Unplaced A", 0.2, None),
        ],
        initiatives: vec![InitiativeSummary {
            initiative,
            status: "Active".to_owned(),
            health: Some("atRisk".to_owned()),
            updated_at: "program-updated".to_owned(),
            archived: false,
            trashed: false,
            project_ids: vec![
                ProjectId("placed-a".to_owned()),
                ProjectId("placed-b".to_owned()),
            ],
        }],
    };
    let roadmap = build_summary_roadmap(&summary, None, "observed");
    let placed = &roadmap.initiatives[0].projects;
    assert_eq!(placed.len(), 2);
    assert_eq!(placed[0].status, "Placed A");
    assert_eq!(placed[0].progress_percent, Some(25.0));
    assert_eq!(
        placed[0].project_updated_at.as_deref(),
        Some("placed-a-updated")
    );
    assert_eq!(placed[1].status, "Placed B");
    assert_eq!(placed[1].progress_percent, Some(75.0));
    assert_eq!(
        placed[1].project_updated_at.as_deref(),
        Some("placed-b-updated")
    );
    let unplaced = &roadmap.unplaced_projects;
    assert_eq!(unplaced.len(), 2);
    assert_eq!(unplaced[0].status, "Unplaced A");
    assert_eq!(unplaced[0].progress_percent, Some(20.0));
    assert_eq!(
        unplaced[0].project_updated_at.as_deref(),
        Some("unplaced-a-updated")
    );
    assert_eq!(unplaced[1].status, "Unplaced B");
    assert_eq!(unplaced[1].progress_percent, Some(80.0));
    assert_eq!(
        unplaced[1].project_updated_at.as_deref(),
        Some("unplaced-b-updated")
    );
}
