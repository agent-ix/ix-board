// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Reads over references to unavailable Linear records (FR-046), and the cached leaf
//! counts of the roadmap (FR-020).

use ix_board::linear_summary::{InitiativeSummary, ProjectSummary, SummarySnapshot};
use ix_board::program_review::build_summary_roadmap;
use ix_board::{
    CatalogEvidence, Clock, CountAvailability, Error, FocusExpansion, FocusSelection, FocusStatus,
    Initiative, Issue, IssueId, Milestone, NamedBatch, NamedFocusRequest, NamedMember,
    NamedSelection, Project, ProjectId, ProjectStatusCategory, ReadService, ReferenceWarning,
    ScopeSelection, SelectionState, TeamKey, UnknownFact, WorkflowStateType, WorkspaceSnapshot,
    WorkspaceSource, evaluate_named_batch, select_scope,
};
use ix_trace_rs::trace;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

#[derive(Clone)]
struct Source {
    snapshot: WorkspaceSnapshot,
    reads: Arc<AtomicUsize>,
}

impl WorkspaceSource for Source {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.snapshot.clone())
    }
}

struct Now;

impl Clock for Now {
    fn now_epoch_seconds(&self) -> i64 {
        1_000
    }
}

struct Fixture {
    reads: ReadService<Source, Now>,
    source: Source,
    _directory: TempDir,
}

fn fixture(snapshot: WorkspaceSnapshot) -> Fixture {
    let directory = TempDir::new().expect("temporary directory");
    let source = Source {
        snapshot,
        reads: Arc::new(AtomicUsize::new(0)),
    };
    let reads = ReadService::with_clock(
        source.clone(),
        &directory.path().join("linear.sqlite3"),
        Now,
    )
    .expect("service");
    Fixture {
        reads,
        source,
        _directory: directory,
    }
}

fn initiative() -> Initiative {
    Initiative {
        id: "i1".to_owned(),
        identifier: String::new(),
        name: "Hardening".to_owned(),
    }
}

fn project(id: &str, milestones: &[&str]) -> Project {
    Project {
        id: ProjectId(id.to_owned()),
        identifier: None,
        name: format!("Project {id}"),
        updated_at: None,
        description: String::new(),
        content: None,
        status: "Started".to_owned(),
        status_name: None,
        status_category: ProjectStatusCategory::Started,
        priority: 1,
        sort_order: 0.0,
        target_date: None,
        team_keys: vec![TeamKey("SWM".to_owned())],
        milestones: milestones
            .iter()
            .map(|id| Milestone {
                id: (*id).to_owned(),
                name: format!("Milestone {id}"),
                description: None,
                sort_order: 1.0,
                target_date: None,
            })
            .collect(),
        initiatives: vec![initiative()],
    }
}

/// An open, spec-ready issue whose UUID equals its identifier.
fn issue(id: &str, project: Option<&str>) -> Issue {
    Issue {
        id: IssueId(id.to_owned()),
        identifier: id.to_owned(),
        title: format!("Title {id}"),
        description: None,
        state: "Specced".to_owned(),
        state_name: "Specced".to_owned(),
        state_type: WorkflowStateType::Unstarted,
        parent_id: None,
        team_key: TeamKey("SWM".to_owned()),
        assignee_id: None,
        assignee_name: None,
        project_id: project.map(|id| ProjectId(id.to_owned())),
        project_milestone_id: None,
        priority: 2.0,
        sort_order: 0.0,
        blocked_by: Vec::new(),
        created_at: Some("2026-11-01T00:00:00Z".to_owned()),
        updated_at: Some("2026-11-01T00:00:00Z".to_owned()),
        attachment_urls: Vec::new(),
        attachments_complete: true,
        started_at: None,
        completed_at: None,
        pull_requests: Vec::new(),
    }
}

fn in_milestone(mut issue: Issue, milestone: &str) -> Issue {
    issue.project_milestone_id = Some(milestone.to_owned());
    issue
}

fn done(mut issue: Issue) -> Issue {
    issue.state = "Completed".to_owned();
    issue.state_name = "Done".to_owned();
    issue.state_type = WorkflowStateType::Completed;
    issue
}

fn child_of(mut issue: Issue, parent: &str) -> Issue {
    issue.parent_id = Some(IssueId(parent.to_owned()));
    issue
}

fn blocked_by(mut issue: Issue, blocker: &str) -> Issue {
    issue.blocked_by = vec![IssueId(blocker.to_owned())];
    issue
}

fn snapshot(issues: Vec<Issue>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        projects: vec![project("p1", &["m1"]), project("p2", &[])],
        issues,
        fetched_at: 900,
    }
}

/// Every kind of dangling reference, beside ordinary work.
fn dangling() -> WorkspaceSnapshot {
    snapshot(vec![
        done(in_milestone(issue("SWM-1", Some("p1")), "m1")),
        done(in_milestone(issue("SWM-2", Some("p1")), "gone")),
        issue("SWM-3", Some("p1")),
        // A milestone of another project is not one of this project's.
        in_milestone(issue("SWM-4", Some("p2")), "m1"),
        in_milestone(issue("SWM-5", None), "gone"),
        done(issue("SWM-6", Some("ghost"))),
        child_of(issue("SWM-7", Some("p1")), "archived-parent"),
        blocked_by(issue("SWM-8", Some("p2")), "archived-blocker"),
        blocked_by(issue("SWM-9", Some("p2")), "SWM-8"),
    ])
}

fn projects(ids: &[&str]) -> FocusSelection {
    FocusSelection {
        projects: ids.iter().map(|id| (*id).to_owned()).collect(),
        ..FocusSelection::default()
    }
}

fn tickets(ids: &[&str]) -> FocusSelection {
    FocusSelection {
        issues: ids.iter().map(|id| (*id).to_owned()).collect(),
        ..FocusSelection::default()
    }
}

fn whole_initiative() -> FocusSelection {
    FocusSelection {
        initiatives: vec!["i1".to_owned()],
        ..FocusSelection::default()
    }
}

/// Status counts, project rows plus the outside group, must equal `select_scope`.
fn assert_counts_match_scope(
    snapshot: &WorkspaceSnapshot,
    selection: &FocusSelection,
    status: &FocusStatus,
) {
    let scope = select_scope(
        snapshot,
        &ScopeSelection {
            team: None,
            initiatives: selection.initiatives.clone(),
            projects: selection.projects.clone(),
            milestones: selection.milestones.clone(),
            tickets: selection.issues.clone(),
        },
    )
    .expect("scope");
    let groups = status
        .projects
        .iter()
        .map(|row| row.progress)
        .chain(status.outside_projects.iter().map(|group| group.progress))
        .collect::<Vec<_>>();
    assert_eq!(
        groups.iter().map(|p| p.total).sum::<usize>(),
        scope.leaf_ids.len()
    );
    assert_eq!(groups.iter().map(|p| p.done).sum::<usize>(), scope.done);
    for row in &status.projects {
        let milestones = row.milestones.iter().map(|m| m.progress);
        let split = milestones.chain([row.unmilestoned.progress]);
        assert_eq!(
            split.map(|p| p.total).sum::<usize>(),
            row.progress.total,
            "every leaf of {} is in a milestone or at project level",
            row.identifier
        );
    }
}

fn milestone_warning(issue: &str, milestone: &str, project: Option<&str>) -> ReferenceWarning {
    ReferenceWarning::UnavailableMilestone {
        issue: issue.to_owned(),
        milestone: milestone.to_owned(),
        project: project.map(str::to_owned),
    }
}

fn blocker_warning(issue: &str, blocker: &str) -> ReferenceWarning {
    ReferenceWarning::UnavailableBlocker {
        issue: issue.to_owned(),
        blocker: blocker.to_owned(),
    }
}

fn parent_warning() -> ReferenceWarning {
    ReferenceWarning::UnavailableParent {
        issue: "SWM-7".to_owned(),
        parent: "archived-parent".to_owned(),
    }
}

fn project_level<'a>(expansion: &'a FocusExpansion, project: &str) -> Vec<&'a str> {
    expansion
        .projects
        .iter()
        .find(|row| row.id.0 == project)
        .expect("project row")
        .project_level_issues
        .iter()
        .map(|issue| issue.identifier.as_str())
        .collect()
}

#[trace("FR-046-AC-1")]
#[test]
fn an_unavailable_milestone_counts_at_project_level_in_every_focus_read() {
    let snapshot = dangling();
    let mut fixture = fixture(snapshot.clone());
    let expected = [
        milestone_warning("SWM-2", "gone", Some("p1")),
        milestone_warning("SWM-4", "m1", Some("p2")),
    ];
    for selection in [projects(&["p1", "p2"]), whole_initiative()] {
        let expansion = fixture.reads.focus_expand(&selection).expect("expand");
        assert_eq!(project_level(&expansion, "p1"), ["SWM-2", "SWM-3", "SWM-7"]);
        assert_eq!(project_level(&expansion, "p2"), ["SWM-4", "SWM-8", "SWM-9"]);
        assert_eq!(expansion.focus_issues.len(), 7);

        let status = fixture.reads.focus_status(&selection).expect("status");
        assert_counts_match_scope(&snapshot, &selection, &status);
        let p1 = &status.projects[0];
        assert_eq!((p1.progress.done, p1.progress.total), (2, 4));
        assert_eq!(
            (
                p1.unmilestoned.progress.done,
                p1.unmilestoned.progress.total
            ),
            (1, 3)
        );
        assert!(status.outside_projects.is_none());

        let record = fixture.reads.focus_record(&selection).expect("report");
        assert_eq!(record.status, status);

        let chunk = fixture
            .reads
            .focus_plan_chunk(&selection, None)
            .expect("plan chunk");
        let moved = chunk
            .issues
            .iter()
            .find(|row| row.identifier == "SWM-4")
            .expect("SWM-4 stays in the handoff");
        assert_eq!(moved.milestone, None);

        for warnings in [&expansion.warnings, &status.warnings, &chunk.warnings] {
            for warning in &expected {
                assert!(warnings.contains(warning), "{warnings:?}");
            }
        }
    }
    assert_eq!(
        expected[0].to_string(),
        "issue SWM-2 references unavailable milestone gone in project p1"
    );
}

#[trace("FR-046-AC-2", "FR-046-AC-3", "FR-045-AC-4")]
#[test]
fn issues_outside_every_project_row_are_counted_and_warned() {
    let mut snapshot = dangling();
    // SWM-6 completed inside the 48 hours before the fixture clock.
    snapshot.issues[5].completed_at = Some("1970-01-01T00:15:00Z".to_owned());
    let mut fixture = fixture(snapshot.clone());
    let selection = tickets(&["SWM-3", "SWM-5", "SWM-6"]);
    let status = fixture.reads.focus_status(&selection).expect("status");
    assert_counts_match_scope(&snapshot, &selection, &status);
    let outside = status.outside_projects.as_ref().expect("outside group");
    assert_eq!((outside.progress.done, outside.progress.total), (1, 2));
    assert_eq!(outside.recently_closed.len(), 1);
    assert_eq!(outside.recently_closed[0].identifier, "SWM-6");
    let activity = status
        .activity
        .iter()
        .map(|event| (event.identifier.as_str(), event.project.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(activity, [("SWM-6", "ghost (unavailable)")]);
    assert_eq!(status.projects.len(), 1);
    assert_eq!(
        status.warnings,
        [
            milestone_warning("SWM-5", "gone", None),
            ReferenceWarning::UnavailableProject {
                issue: "SWM-6".to_owned(),
                project: "ghost".to_owned(),
            },
        ]
    );
    let expansion = fixture.reads.focus_expand(&selection).expect("expand");
    assert_eq!(expansion.focus_issues.len(), 3);
}

#[trace("FR-046-AC-3")]
#[test]
fn an_unavailable_project_is_not_ready_and_next_reports_it() {
    let mut fixture = fixture(snapshot(vec![issue("SWM-6", Some("ghost"))]));
    let chunk = fixture
        .reads
        .focus_plan_chunk(&tickets(&["SWM-6"]), None)
        .expect("plan chunk");
    assert!(!chunk.issues[0].ready);
    assert!(
        chunk.issues[0]
            .reasons
            .contains(&"owning project ghost is unavailable".to_owned())
    );
    let (queue, warnings) = fixture.reads.next_with_warnings("SWM").expect("next");
    assert!(queue.is_empty());
    assert_eq!(
        warnings,
        [ReferenceWarning::UnavailableProject {
            issue: "SWM-6".to_owned(),
            project: "ghost".to_owned(),
        }]
    );
}

#[trace("FR-046-AC-4")]
#[test]
fn an_unavailable_parent_leaves_the_issue_a_counted_leaf() {
    let snapshot = dangling();
    let scope = select_scope(
        &snapshot,
        &ScopeSelection {
            tickets: vec!["SWM-7".to_owned()],
            ..ScopeSelection::default()
        },
    )
    .expect("scope tolerates the parent");
    assert_eq!(scope.leaf_ids, [IssueId("SWM-7".to_owned())].into());

    let mut fixture = fixture(snapshot.clone());
    let selection = tickets(&["SWM-7"]);
    let status = fixture.reads.focus_status(&selection).expect("status");
    assert_counts_match_scope(&snapshot, &selection, &status);
    assert_eq!(status.warnings, [parent_warning()]);
    let expansion = fixture.reads.focus_expand(&selection).expect("expand");
    assert_eq!(expansion.warnings, [parent_warning()]);
    let chunk = fixture
        .reads
        .focus_plan_chunk(&selection, None)
        .expect("plan chunk");
    assert_eq!(chunk.warnings, [parent_warning()]);

    let batch = evaluate_named_batch(
        &snapshot,
        &CatalogEvidence::default(),
        &[NamedFocusRequest {
            key: "k".to_owned(),
            selection: NamedSelection::new(
                TeamKey("SWM".to_owned()),
                vec![NamedMember::Issue {
                    id: "SWM-7".to_owned(),
                }],
                Vec::new(),
            ),
        }],
    )
    .expect("batch");
    assert_eq!(batch.selections[0].counts.total, 1);
    assert_eq!(batch.warnings, [parent_warning()]);
}

#[trace("FR-046-AC-5", "FR-002-AC-7")]
#[test]
fn an_unavailable_blocker_blocks_its_dependents_without_failing_any_read() {
    let snapshot = dangling();
    let mut fixture = fixture(snapshot.clone());
    let expected = blocker_warning("SWM-8", "archived-blocker");
    // SWM-9 reaches the unavailable record only through the available SWM-8.
    let selection = tickets(&["SWM-9"]);

    let expansion = fixture.reads.focus_expand(&selection).expect("expand");
    // The unavailable prerequisite holds layer zero, so nothing that waits on it does.
    assert_eq!(
        expansion.blocker_layers,
        [
            vec!["archived-blocker (unavailable)".to_owned()],
            vec!["SWM-8".to_owned()],
            vec!["SWM-9".to_owned()]
        ]
    );
    assert_eq!(expansion.warnings, std::slice::from_ref(&expected));

    let status = fixture.reads.focus_status(&selection).expect("status");
    assert_counts_match_scope(&snapshot, &selection, &status);
    assert!(status.ready_queue.is_empty());
    assert_eq!(status.blocker_layers.len(), 3);
    assert!(status.blocker_layers[0].is_empty());
    assert_eq!(status.blocker_layers[1][0].focus_root_ids, ["SWM-9"]);
    assert_eq!(status.warnings, std::slice::from_ref(&expected));

    let chunk = fixture
        .reads
        .focus_plan_chunk(&tickets(&["SWM-8", "SWM-9"]), None)
        .expect("plan chunk");
    assert!(chunk.issues.iter().all(|row| !row.ready));
    assert!(
        chunk.issues[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("archived-blocker (unavailable)"))
    );
    assert_eq!(chunk.issues[0].unblocks, ["SWM-9"]);
    let layers = chunk
        .issues
        .iter()
        .map(|row| (row.identifier.as_str(), row.dependency_layer))
        .collect::<Vec<_>>();
    assert_eq!(layers, [("SWM-8", 1), ("SWM-9", 2)]);

    let project = fixture.reads.project_chunk("p2").expect("project chunk");
    assert_eq!(
        project.blocker_layers,
        [
            vec![
                "archived-blocker (unavailable)".to_owned(),
                "SWM-4".to_owned()
            ],
            vec!["SWM-8".to_owned()],
            vec!["SWM-9".to_owned()]
        ]
    );
    let blocked_by = project
        .open_tickets
        .iter()
        .map(|ticket| (ticket.identifier.as_str(), ticket.blocked_by.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        blocked_by,
        [
            ("SWM-4", vec![]),
            ("SWM-8", vec!["archived-blocker (unavailable)".to_owned()]),
            ("SWM-9", vec!["SWM-8".to_owned()]),
        ]
    );
    assert_eq!(
        project.warnings,
        [
            milestone_warning("SWM-4", "m1", Some("p2")),
            expected.clone(),
        ]
    );

    let (queue, warnings) = fixture.reads.next_with_warnings("SWM").expect("next");
    let ready = queue
        .iter()
        .map(|row| row.identifier.as_str())
        .collect::<Vec<_>>();
    assert!(ready.contains(&"SWM-3"));
    assert!(!ready.contains(&"SWM-8") && !ready.contains(&"SWM-9"));
    assert!(warnings.contains(&expected));

    let batch = evaluate_named_batch(
        &snapshot,
        &CatalogEvidence::default(),
        &[NamedFocusRequest {
            key: "k".to_owned(),
            selection: NamedSelection::new(
                TeamKey("SWM".to_owned()),
                vec![NamedMember::Project {
                    id: "p2".to_owned(),
                }],
                Vec::new(),
            ),
        }],
    )
    .expect("batch");
    let counts = &batch.selections[0].counts;
    assert_eq!((counts.total, counts.ready, counts.blocked), (3, 1, 2));
    assert!(batch.warnings.contains(&expected));
}

#[trace("FR-046-AC-5", "FR-014-AC-8")]
#[test]
fn a_bounded_plan_chunk_offers_unblocked_work_before_work_behind_an_unavailable_blocker() {
    // The blocked identifier sorts first, so only its layer can put it second.
    let mut fixture = fixture(snapshot(vec![
        blocked_by(issue("SWM-20", Some("p1")), "archived-blocker"),
        issue("SWM-21", Some("p1")),
    ]));
    let chunk = fixture
        .reads
        .focus_plan_chunk(&projects(&["p1"]), std::num::NonZeroUsize::new(1))
        .expect("plan chunk");
    assert_eq!(chunk.issues.len(), 1);
    assert_eq!(chunk.issues[0].identifier, "SWM-21");
    assert!(chunk.issues[0].ready);
}

fn batch_of(snapshot: &WorkspaceSnapshot, members: Vec<(&str, NamedMember)>) -> NamedBatch {
    let requests = members
        .into_iter()
        .map(|(key, member)| NamedFocusRequest {
            key: key.to_owned(),
            selection: NamedSelection::new(TeamKey("SWM".to_owned()), vec![member], Vec::new()),
        })
        .collect::<Vec<_>>();
    let evidence = CatalogEvidence {
        initiative_links: true,
        ..CatalogEvidence::default()
    };
    evaluate_named_batch(snapshot, &evidence, &requests).expect("batch")
}

fn assert_incomplete(batch: &NamedBatch, key: &str) {
    let selection = batch
        .selections
        .iter()
        .find(|selection| selection.key == key)
        .expect("selection");
    assert_eq!(selection.state, SelectionState::Incomplete, "{key}");
    assert_eq!(selection.counts_availability, CountAvailability::Partial);
    assert_eq!(
        selection.unknown_facts,
        [UnknownFact::UnavailableReferences]
    );
    let team = &batch.teams[0];
    assert_eq!(team.state, SelectionState::Incomplete);
    assert_eq!(team.counts_availability, CountAvailability::Partial);
    assert!(team.causes.iter().any(|cause| {
        cause.key == key && cause.unknown_facts == [UnknownFact::UnavailableReferences]
    }));
}

#[trace("FR-046-AC-9", "FR-012-AC-17")]
#[test]
fn a_named_selection_that_depends_on_an_unavailable_record_is_never_complete() {
    let project = |id: &str| NamedMember::Project { id: id.to_owned() };
    let ticket = |id: &str| NamedMember::Issue { id: id.to_owned() };
    for (name, issues) in [
        (
            "blocker",
            vec![blocked_by(issue("SWM-1", Some("p1")), "gone")],
        ),
        ("parent", vec![child_of(issue("SWM-1", Some("p1")), "gone")]),
    ] {
        let batch = batch_of(&snapshot(issues), vec![(name, project("p1"))]);
        assert_incomplete(&batch, name);
    }
    // A selection of a ticket's descendants cannot tell what a missing parent hides.
    let batch = batch_of(
        &snapshot(vec![
            issue("SWM-1", Some("p1")),
            child_of(issue("SWM-2", Some("p2")), "gone"),
        ]),
        vec![("ticket", ticket("SWM-1"))],
    );
    assert_incomplete(&batch, "ticket");
    assert!(batch.warnings.is_empty());

    // The affected selection does not taint another one, and a milestone reference
    // changes no count.
    let batch = batch_of(
        &snapshot(vec![
            in_milestone(issue("SWM-1", Some("p1")), "gone"),
            blocked_by(issue("SWM-2", Some("p2")), "gone"),
        ]),
        vec![("clean", project("p1")), ("dirty", project("p2"))],
    );
    assert_eq!(batch.selections[0].state, SelectionState::Complete);
    assert!(batch.selections[0].unknown_facts.is_empty());
    assert_incomplete(&batch, "dirty");
    assert_eq!(batch.teams[0].causes.len(), 1);
}

#[trace("FR-046-AC-6", "FR-046-AC-1")]
#[test]
fn warning_members_appear_in_json_only_when_there_is_something_to_report() {
    let mut clean = fixture(snapshot(vec![issue("SWM-3", Some("p1"))]));
    let record = clean
        .reads
        .focus_record(&projects(&["p1"]))
        .expect("clean report");
    let json = serde_json::to_value(&record).expect("record JSON");
    for member in ["warnings", "outside_projects"] {
        assert!(json["status"].get(member).is_none(), "{member} is omitted");
    }
    let expansion = clean
        .reads
        .focus_expand(&projects(&["p1"]))
        .expect("clean expansion");
    assert!(
        serde_json::to_value(&expansion)
            .expect("expansion JSON")
            .get("warnings")
            .is_none()
    );

    let mut dirty = fixture(dangling());
    let status = dirty
        .reads
        .focus_status(&tickets(&["SWM-2", "SWM-5"]))
        .expect("status");
    let json = serde_json::to_value(&status).expect("status JSON");
    assert_eq!(
        json["warnings"][0],
        serde_json::json!({
            "kind": "unavailable_milestone",
            "issue": "SWM-2",
            "milestone": "gone",
            "project": "p1"
        })
    );
    assert!(json["warnings"][1]["project"].is_null());
    assert_eq!(json["outside_projects"]["progress"]["total"], 1);
    let decoded: FocusStatus = serde_json::from_value(json).expect("round trip");
    assert_eq!(decoded, status);
}

#[trace("FR-046-AC-7")]
#[test]
fn focus_hygiene_carries_the_same_warnings() {
    let mut fixture = fixture(dangling());
    let selection = whole_initiative();
    let status = fixture.reads.focus_status(&selection).expect("status");
    let report = fixture
        .reads
        .focus_hygiene(&selection, "2026-11-25T00:00:00Z", 30)
        .expect("hygiene");
    assert!(!report.warnings.is_empty());
    assert_eq!(report.warnings, status.warnings);
}

fn summary_project(id: &str, memberships: &[&Initiative]) -> ProjectSummary {
    let mut project = project(id, &[]);
    project.identifier = Some(format!("slug-{id}"));
    project.initiatives = memberships.iter().map(|i| (*i).clone()).collect();
    ProjectSummary {
        project,
        progress: Some(0.5),
        updated_at: "2026-11-20T00:00:00Z".to_owned(),
        archived: false,
        trashed: false,
    }
}

fn summary_initiative(id: &str, name: &str) -> InitiativeSummary {
    InitiativeSummary {
        initiative: Initiative {
            id: id.to_owned(),
            identifier: format!("slug-{id}"),
            name: name.to_owned(),
        },
        status: "Active".to_owned(),
        health: None,
        updated_at: "2026-11-20T00:00:00Z".to_owned(),
        archived: false,
        trashed: false,
        project_ids: Vec::new(),
    }
}

fn summary() -> SummarySnapshot {
    let first = summary_initiative("i1", "Alpha");
    let second = summary_initiative("i2", "Beta");
    SummarySnapshot {
        projects: vec![
            summary_project("p1", &[&first.initiative]),
            // Listed under Alpha, its first membership by name, and also in Beta.
            summary_project("p2", &[&first.initiative, &second.initiative]),
            summary_project("new", &[]),
        ],
        initiatives: vec![first, second],
    }
}

#[trace("FR-020-AC-7")]
#[test]
fn roadmap_entries_carry_ids_slugs_and_complete_initiative_membership() {
    let roadmap = build_summary_roadmap(&summary(), None, "observed");
    let json = serde_json::to_value(&roadmap).expect("roadmap JSON");
    let beta = json["initiatives"]
        .as_array()
        .expect("initiatives")
        .iter()
        .find(|row| row["id"] == "i2")
        .expect("lookup by initiative UUID");
    assert_eq!(beta["identifier"], "slug-i2");
    assert!(beta["projects"].as_array().expect("rows").is_empty());
    assert_eq!(beta["member_project_ids"], serde_json::json!(["p2"]));
    assert_eq!(
        json["initiatives"][0]["member_project_ids"],
        serde_json::json!(["p1", "p2"])
    );
    assert_eq!(json["initiatives"][0]["projects"][1]["id"], "p2");
    assert_eq!(
        json["initiatives"][0]["projects"][1]["identifier"],
        "slug-p2"
    );
    assert_eq!(json["unplaced_projects"][0]["id"], "new");
    assert_eq!(json["unplaced_projects"][0]["identifier"], "slug-new");
}

#[trace("FR-020-AC-8", "FR-020-AC-2")]
#[test]
fn roadmap_leaf_counts_come_from_the_cached_issues_and_match_focus_status() {
    let mut parent = issue("SWM-10", Some("p1"));
    parent.state_type = WorkflowStateType::Started;
    let mut canceled = issue("SWM-12", Some("p1"));
    canceled.state_type = WorkflowStateType::Canceled;
    let mut issues = dangling().issues;
    issues.extend([
        parent,
        done(child_of(issue("SWM-11", Some("p1")), "SWM-10")),
        canceled,
    ]);
    let mut fixture = fixture(snapshot(issues));
    assert!(
        fixture
            .reads
            .cached_workspace_snapshot()
            .expect("empty cache")
            .is_none()
    );
    assert_eq!(fixture.source.reads.load(Ordering::SeqCst), 0);

    let status = fixture
        .reads
        .focus_status(&projects(&["p1", "p2"]))
        .expect("status");
    let detail = fixture
        .reads
        .cached_workspace_snapshot()
        .expect("cached read")
        .expect("cached snapshot");
    assert_eq!(fixture.source.reads.load(Ordering::SeqCst), 1);

    let mut roadmap = build_summary_roadmap(&summary(), None, "observed");
    roadmap.count_from_issues(&detail, "2026-11-25T00:00:00Z");
    let rows = &roadmap.initiatives[0].projects;
    for (row, expected) in rows.iter().zip(&status.projects) {
        assert_eq!(row.id, expected.id.0);
        assert_eq!(row.done, Some(expected.progress.done));
        assert_eq!(row.total, Some(expected.progress.total));
        assert_eq!(
            row.open,
            Some(expected.progress.total - expected.progress.done)
        );
        assert_eq!(
            row.counts_source.as_deref(),
            Some("ix_board_cached_issue_leaf_tasks")
        );
        assert_eq!(
            row.counts_observed_at.as_deref(),
            Some("2026-11-25T00:00:00Z")
        );
        assert!(row.counts_unavailable_reason.is_empty());
    }
    assert_eq!((rows[0].done, rows[0].total), (Some(3), Some(5)));
    assert_eq!(rows[0].canceled, Some(1));
    // A project the detail snapshot does not hold is unavailable, not zero.
    let absent = &roadmap.unplaced_projects[0];
    assert_eq!((absent.done, absent.total), (None, None));
    assert_eq!(
        absent.counts_unavailable_reason,
        "project is not in the cached issue snapshot"
    );

    roadmap.counts_unavailable("no cached issue snapshot");
    let row = &roadmap.initiatives[0].projects[0];
    assert_eq!((row.done, row.total, row.open), (None, None, None));
    assert_eq!(row.counts_source, None);
    assert_eq!(row.counts_unavailable_reason, "no cached issue snapshot");
}
