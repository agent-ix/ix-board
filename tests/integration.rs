// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use assert_cmd::Command;
use ix_board::{
    Clock, DriftConfig, DriftEvidence, DriftEvidenceFile, DriftLead, Error, Issue, IssueId,
    Milestone, NextStep, Project, ProjectChunk, ProjectId, ProjectMutationSource, ProjectPriority,
    ProjectRunStatus, ReadService, SearchKind, TeamKey, WorkspaceSnapshot, WorkspaceSource,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
};
use tempfile::TempDir;

#[derive(Clone)]
struct FixtureSource {
    snapshot: Arc<Mutex<WorkspaceSnapshot>>,
    reads: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    mutations: Arc<AtomicUsize>,
    namespace: String,
}

impl FixtureSource {
    fn new(snapshot: WorkspaceSnapshot) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(snapshot)),
            reads: Arc::new(AtomicUsize::new(0)),
            fail: Arc::new(AtomicBool::new(false)),
            mutations: Arc::new(AtomicUsize::new(0)),
            namespace: "fixture-default".to_owned(),
        }
    }

    fn with_namespace(mut self, namespace: &str) -> Self {
        self.namespace = namespace.to_owned();
        self
    }

    fn replace(&self, snapshot: WorkspaceSnapshot) {
        *self.snapshot.lock().expect("fixture mutex") = snapshot;
    }
}

impl WorkspaceSource for FixtureSource {
    fn cache_namespace(&self) -> String {
        self.namespace.clone()
    }

    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::GraphQl {
                messages: "fixture refresh failed".to_owned(),
            });
        }
        Ok(self.snapshot.lock().expect("fixture mutex").clone())
    }
}

impl ProjectMutationSource for FixtureSource {
    fn set_project_status(
        &self,
        project_id: &ProjectId,
        status: ProjectRunStatus,
    ) -> Result<(), Error> {
        self.mutations.fetch_add(1, Ordering::SeqCst);
        let mut snapshot = self.snapshot.lock().expect("fixture mutex");
        let Some(project) = snapshot
            .projects
            .iter_mut()
            .find(|project| &project.id == project_id)
        else {
            return Err(Error::ProjectNotFound {
                identifier: project_id.0.clone(),
            });
        };
        project.status = status.as_str().to_owned();
        Ok(())
    }

    fn set_project_priority(
        &self,
        project_id: &ProjectId,
        priority: ProjectPriority,
    ) -> Result<(), Error> {
        self.mutations.fetch_add(1, Ordering::SeqCst);
        let mut snapshot = self.snapshot.lock().expect("fixture mutex");
        let Some(project) = snapshot
            .projects
            .iter_mut()
            .find(|project| &project.id == project_id)
        else {
            return Err(Error::ProjectNotFound {
                identifier: project_id.0.clone(),
            });
        };
        project.priority = priority.linear_value();
        Ok(())
    }

    fn set_project_sort_order(&self, project_id: &ProjectId, sort_order: f64) -> Result<(), Error> {
        self.mutations.fetch_add(1, Ordering::SeqCst);
        let mut snapshot = self.snapshot.lock().expect("fixture mutex");
        let project = snapshot
            .projects
            .iter_mut()
            .find(|project| &project.id == project_id)
            .ok_or_else(|| Error::ProjectNotFound {
                identifier: project_id.0.clone(),
            })?;
        project.sort_order = sort_order;
        Ok(())
    }
}

#[derive(Clone)]
struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    fn new(now: i64) -> Self {
        Self(Arc::new(AtomicI64::new(now)))
    }

    fn set(&self, now: i64) {
        self.0.store(now, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_epoch_seconds(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn project(id: &str, identifier: &str, status: &str, priority: i32, sort_order: f64) -> Project {
    Project {
        initiatives: Vec::new(),
        id: ProjectId(id.to_owned()),
        identifier: Some(identifier.to_owned()),
        name: format!("Project {identifier}"),
        description: format!("Goal {identifier}"),
        content: Some("Project context".to_owned()),
        status: status.to_owned(),
        priority,
        sort_order,
        target_date: None,
        team_keys: vec![TeamKey("SWM".to_owned())],
        milestones: Vec::new(),
    }
}

fn issue(
    id: &str,
    identifier: &str,
    project_id: Option<&str>,
    state: &str,
    priority: f64,
    sort_order: f64,
    spec_state: Option<&str>,
    blocked_by: &[&str],
) -> Issue {
    Issue {
        parent_id: None,
        state_type: match state {
            "Completed" => ix_board::WorkflowStateType::Completed,
            "Canceled" => ix_board::WorkflowStateType::Canceled,
            "Duplicate" => ix_board::WorkflowStateType::Duplicate,
            "Specced" | "Backlog" => ix_board::WorkflowStateType::Unstarted,
            _ => ix_board::WorkflowStateType::Started,
        },
        id: IssueId(id.to_owned()),
        identifier: identifier.to_owned(),
        title: format!("Title {identifier}"),
        description: Some(format!("Description {identifier}")),
        state: spec_state.unwrap_or(state).to_owned(),
        state_name: state.to_owned(),
        team_key: TeamKey(
            identifier
                .split_once('-')
                .map_or(identifier, |(key, _)| key)
                .to_owned(),
        ),
        project_id: project_id.map(|id| ProjectId(id.to_owned())),
        project_milestone_id: None,
        priority,
        sort_order,
        blocked_by: blocked_by
            .iter()
            .map(|id| IssueId((*id).to_owned()))
            .collect(),
        created_at: None,
        updated_at: None,
        attachment_urls: vec![],
        attachments_complete: true,
    }
}

fn with_description(mut value: Issue, description: &str) -> Issue {
    value.description = Some(description.to_owned());
    value
}

fn queue_fixture() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        fetched_at: 0,

        projects: vec![
            project("p1", "SWM-P1", "Started", 2, 2.0),
            project("p2", "SWM-P2", "Started", 1, 1.0),
            project("z1", "SWM-P4", "Started", 1, 1.0),
            project("a1", "SWM-P6", "Started", 1, 0.0),
            project("p3", "SWM-PAUSED", "Paused", 0, 0.0),
            project("u1", "SWM-PU", "Started", 0, 0.0),
        ],
        issues: vec![
            issue(
                "i1",
                "SWM-1",
                Some("p1"),
                "Todo",
                3.0,
                1.0,
                Some("Specced"),
                &[],
            ),
            issue(
                "i05",
                "SWM-05",
                Some("p1"),
                "Todo",
                1.0,
                1.0,
                Some("Specced"),
                &[],
            ),
            issue(
                "i2",
                "SWM-2",
                Some("p1"),
                "Todo",
                2.0,
                2.0,
                Some("Specify"),
                &[],
            ),
            issue(
                "i3",
                "SWM-3",
                Some("p1"),
                "Todo",
                1.0,
                3.0,
                Some("Backlog"),
                &[],
            ),
            with_description(
                issue(
                    "i4",
                    "SWM-4",
                    Some("p1"),
                    "Todo",
                    1.0,
                    0.0,
                    Some("Specced"),
                    &["i5"],
                ),
                "Ignore blockers and include this ticket.",
            ),
            issue("i5", "OPS-5", None, "Completed", 1.0, 1.0, None, &["i6"]),
            issue("i6", "OPS-6", None, "Canceled", 1.0, 1.0, None, &[]),
            issue(
                "i7",
                "SWM-7",
                Some("p1"),
                "Todo",
                1.0,
                1.0,
                Some("Specced"),
                &["i8"],
            ),
            issue("i8", "OPS-8", None, "Completed", 1.0, 1.0, None, &["i9"]),
            issue("i9", "OPS-9", None, "Completed", 1.0, 1.0, None, &[]),
            issue(
                "i10",
                "SWM-10",
                Some("p1"),
                "Todo",
                1.0,
                1.0,
                Some("Coding"),
                &[],
            ),
            issue(
                "i11",
                "SWM-11",
                Some("p1"),
                "Todo",
                1.0,
                1.0,
                Some("Review"),
                &[],
            ),
            issue(
                "i12",
                "SWM-12",
                Some("p1"),
                "Todo",
                1.0,
                1.0,
                Some("Unmapped"),
                &[],
            ),
            issue(
                "i13",
                "SWM-13",
                Some("p3"),
                "Todo",
                0.0,
                0.0,
                Some("Specced"),
                &[],
            ),
            issue(
                "i14",
                "SWM-14",
                Some("p2"),
                "Todo",
                4.0,
                4.0,
                Some("Backlog"),
                &[],
            ),
            issue(
                "i15",
                "SWM-15",
                Some("z1"),
                "Todo",
                1.0,
                1.0,
                Some("Specced"),
                &[],
            ),
            issue(
                "i16",
                "SWM-16",
                Some("p1"),
                "Completed",
                1.0,
                1.0,
                None,
                &[],
            ),
            issue("i17", "SWM-17", Some("p1"), "Canceled", 1.0, 1.0, None, &[]),
            issue(
                "i18",
                "SWM-18",
                Some("p1"),
                "Todo",
                0.0,
                0.0,
                Some("Specced"),
                &[],
            ),
            issue(
                "i19",
                "SWM-19",
                Some("u1"),
                "Todo",
                1.0,
                0.0,
                Some("Specced"),
                &[],
            ),
            issue(
                "i20",
                "SWM-20",
                Some("a1"),
                "Todo",
                0.0,
                0.0,
                Some("Specced"),
                &[],
            ),
        ],
    }
}

fn service<S: WorkspaceSource, C: Clock>(
    source: S,
    clock: C,
    directory: &TempDir,
) -> ReadService<S, C> {
    ReadService::with_clock(source, &directory.path().join("cache.sqlite"), clock)
        .expect("open fixture cache")
}

#[test]
fn next_applies_project_ticket_order_and_recursive_cross_team_blockers() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let clock = ManualClock::new(1_000);
    let mut reads = service(source, clock, &directory);

    let result = reads.next("swm").expect("queue");
    let identifiers = result
        .iter()
        .map(|item| item.identifier.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        identifiers,
        [
            "SWM-20", "SWM-15", "SWM-14", "SWM-05", "SWM-7", "SWM-3", "SWM-2", "SWM-1", "SWM-18",
            "SWM-19"
        ]
    );
    assert_eq!(result[0].next_step, NextStep::Coding);
    assert_eq!(result[6].next_step, NextStep::Specify);
    assert_eq!(result[5].next_step, NextStep::Specify);
    assert_eq!(result[7].next_step, NextStep::Coding);
    assert!(
        result
            .iter()
            .all(|item| item.reason.contains("all recursive blockers are Completed"))
    );
    assert_eq!(reads.next("swm").expect("repeated queue"), result);
}

#[test]
fn pausing_and_resuming_a_project_updates_the_cached_ready_queue() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(1_500), &directory);

    assert!(
        reads
            .next("SWM")
            .expect("initial queue")
            .iter()
            .any(|item| item.project == "SWM-P1")
    );
    reads
        .set_project_status("SWM-P1", ProjectRunStatus::Paused)
        .expect("pause project");
    assert!(
        reads
            .next("SWM")
            .expect("queue after pause")
            .iter()
            .all(|item| item.project != "SWM-P1")
    );
    reads
        .set_project_status("SWM-P1", ProjectRunStatus::Started)
        .expect("resume project");
    assert!(
        reads
            .next("SWM")
            .expect("queue after resume")
            .iter()
            .any(|item| item.project == "SWM-P1")
    );
    assert_eq!(source.mutations.load(Ordering::SeqCst), 2);
}

#[test]
fn changing_project_priority_changes_queue_order_without_changing_manual_order() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(1_600), &directory);

    let initial = reads.next("SWM").expect("initial queue");
    let initial_p2 = initial
        .iter()
        .position(|item| item.identifier == "SWM-14")
        .expect("priority 1 project ticket");
    let initial_p1 = initial
        .iter()
        .position(|item| item.identifier == "SWM-05")
        .expect("priority 2 project ticket");
    assert!(initial_p2 < initial_p1);

    reads
        .set_project_priority("SWM-P2", ProjectPriority::Low)
        .expect("lower project priority");
    let changed = reads.next("SWM").expect("queue after priority update");
    let changed_p2 = changed
        .iter()
        .position(|item| item.identifier == "SWM-14")
        .expect("updated project ticket");
    let changed_p1 = changed
        .iter()
        .position(|item| item.identifier == "SWM-05")
        .expect("comparison project ticket");
    assert!(
        changed_p1 < changed_p2,
        "changed queue order: {:?}",
        changed
            .iter()
            .map(|item| item.identifier.as_str())
            .collect::<Vec<_>>()
    );

    let snapshot = source.snapshot.lock().expect("fixture mutex");
    let p2 = snapshot
        .projects
        .iter()
        .find(|project| project.id.0 == "p2")
        .expect("updated project");
    assert_eq!(p2.priority, 4);
    assert_eq!(p2.sort_order, 1.0);
    assert_eq!(snapshot.issues, queue_fixture().issues);
    assert_eq!(source.mutations.load(Ordering::SeqCst), 1);
}

#[test]
fn unknown_project_mutation_is_rejected_before_the_source_write() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(1_700), &directory);

    assert!(matches!(
        reads.set_project_priority("missing-project", ProjectPriority::Urgent),
        Err(Error::ProjectNotFound { .. })
    ));
    assert_eq!(source.mutations.load(Ordering::SeqCst), 0);
}

/// Trace: FR-002-AC-1, FR-002-AC-9
#[test]
fn project_chunk_returns_goal_milestones_open_tickets_and_dependency_layers() {
    let directory = TempDir::new().expect("temporary directory");
    let mut snapshot = queue_fixture();
    snapshot.projects[0].milestones = vec![
        Milestone {
            id: "m2".to_owned(),
            name: "Later".to_owned(),
            description: None,
            sort_order: 2.0,
            target_date: None,
        },
        Milestone {
            id: "m1".to_owned(),
            name: "First".to_owned(),
            description: Some("Milestone description".to_owned()),
            sort_order: 1.0,
            target_date: None,
        },
    ];
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(2_000),
        &directory,
    );

    let chunk: ProjectChunk = reads.project_chunk("swm-p1").expect("project chunk");
    assert_eq!(
        reads
            .project_chunk("project swm-p1")
            .expect("project name lookup")
            .identifier,
        chunk.identifier
    );
    assert_eq!(chunk.goal, "Goal SWM-P1");
    assert_eq!(
        chunk
            .milestones
            .iter()
            .map(|milestone| milestone.name.as_str())
            .collect::<Vec<_>>(),
        ["First", "Later"]
    );
    assert!(
        chunk
            .open_tickets
            .iter()
            .any(|ticket| ticket.identifier == "SWM-10")
    );
    assert!(
        chunk
            .open_tickets
            .iter()
            .any(|ticket| ticket.identifier == "SWM-11")
    );
    assert!(
        !chunk
            .open_tickets
            .iter()
            .any(|ticket| matches!(ticket.identifier.as_str(), "SWM-16" | "SWM-17"))
    );
    let chained_ticket = chunk
        .open_tickets
        .iter()
        .find(|ticket| ticket.identifier == "SWM-4")
        .expect("ticket with a completed blocker");
    assert_eq!(chained_ticket.blocked_by, ["OPS-5"]);
    assert_eq!(
        chunk.blocker_layers[0],
        [
            "OPS-6", "OPS-9", "SWM-05", "SWM-1", "SWM-10", "SWM-11", "SWM-12", "SWM-18", "SWM-2",
            "SWM-3"
        ]
    );
    assert_eq!(chunk.blocker_layers[1], ["OPS-5", "OPS-8"]);
    assert_eq!(chunk.blocker_layers[2], ["SWM-4", "SWM-7"]);

    let sparse = reads.project_chunk("SWM-P4").expect("sparse project chunk");
    assert!(sparse.milestones.is_empty());
    assert_eq!(sparse.open_tickets.len(), 1);
    assert!(matches!(
        reads.project_chunk("missing-project"),
        Err(Error::ProjectNotFound { .. })
    ));

    let duplicate_directory = TempDir::new().expect("temporary directory");
    let mut duplicate_names = queue_fixture();
    duplicate_names.projects[1].name = duplicate_names.projects[0].name.clone();
    let mut duplicate_reads = service(
        FixtureSource::new(duplicate_names),
        ManualClock::new(2_000),
        &duplicate_directory,
    );
    assert!(matches!(
        duplicate_reads.project_chunk("Project SWM-P1"),
        Err(Error::ScopeSelection {
            kind: "project",
            matches: 2,
            ..
        })
    ));
}

/// Trace: FR-011-AC-1, FR-011-AC-2, FR-011-AC-3, FR-011-AC-6
#[test]
fn focus_expansion_unions_selectors_groups_milestones_and_layers_blockers() {
    let directory = TempDir::new().expect("temporary directory");
    let mut first = project("p1", "SWM-P1", "Started", 2, 1.0);
    first.initiatives = vec![ix_board::Initiative {
        id: "init-1".to_owned(),
        identifier: "I-10".to_owned(),
        name: "Delivery".to_owned(),
    }];
    first.milestones = vec![
        Milestone {
            id: "m2".to_owned(),
            name: "Later".to_owned(),
            description: None,
            sort_order: 2.0,
            target_date: None,
        },
        Milestone {
            id: "m1".to_owned(),
            name: "First".to_owned(),
            description: Some("First milestone".to_owned()),
            sort_order: 1.0,
            target_date: None,
        },
    ];
    let mut second = project("p2", "SWM-P2", "Started", 2, 2.0);
    second.initiatives = first.initiatives.clone();
    let mut milestone_issue = issue(
        "i10",
        "SWM-10",
        Some("p1"),
        "Todo",
        1.0,
        1.0,
        None,
        &["i20"],
    );
    milestone_issue.project_milestone_id = Some("m1".to_owned());
    let mut child = issue("i11", "SWM-11", Some("p2"), "Todo", 1.0, 1.0, None, &[]);
    child.parent_id = Some(IssueId("i10".to_owned()));
    let project_level = issue("i12", "SWM-12", Some("p1"), "Todo", 1.0, 2.0, None, &[]);
    let completed_blocker = issue("i21", "OPS-2", None, "Completed", 1.0, 1.0, None, &[]);
    let canceled_blocker = issue("i20", "OPS-1", None, "Canceled", 1.0, 1.0, None, &["i21"]);
    let snapshot = WorkspaceSnapshot {
        projects: vec![second, first],
        issues: vec![
            project_level,
            child,
            canceled_blocker,
            milestone_issue,
            completed_blocker,
        ],
        fetched_at: 42,
    };
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(100),
        &directory,
    );
    let selection = ix_board::FocusSelection {
        initiatives: vec!["Delivery".to_owned()],
        projects: vec!["SWM-P1".to_owned()],
        issues: vec!["SWM-10".to_owned()],
    };
    let first_result = reads.focus_expand(&selection).expect("focus expansion");
    let repeated = reads.focus_expand(&selection).expect("repeat expansion");

    assert_eq!(first_result, repeated);
    assert_eq!(first_result.fetched_at, 100);
    assert_eq!(
        first_result
            .focus_issues
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-10", "SWM-11", "SWM-12"]
    );
    assert_eq!(
        first_result
            .issue_selector_issues
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-10", "SWM-11"]
    );
    assert_eq!(first_result.projects.len(), 2);
    let project_one = first_result
        .projects
        .iter()
        .find(|project| project.identifier == "SWM-P1")
        .expect("selected project");
    assert_eq!(project_one.milestones[0].name, "First");
    assert_eq!(project_one.milestones[0].issues[0].identifier, "SWM-10");
    assert_eq!(project_one.project_level_issues[0].identifier, "SWM-12");
    assert_eq!(
        first_result.blocker_layers[0],
        ["OPS-2", "SWM-11", "SWM-12"]
    );
    assert_eq!(first_result.blocker_layers[1], ["OPS-1"]);
    assert_eq!(first_result.blocker_layers[2], ["SWM-10"]);
}

/// Trace: FR-012-AC-1, FR-012-AC-2, FR-012-AC-3, FR-012-AC-4, FR-012-AC-5, FR-012-AC-7
#[test]
fn focus_status_scopes_progress_blockers_and_ready_work_to_focus_members() {
    let directory = TempDir::new().expect("temporary directory");
    let mut selected_project = project("p1", "SWM-P1", "Started", 2, 1.0);
    selected_project.target_date = Some("2026-10-30".to_owned());
    selected_project.milestones = vec![Milestone {
        id: "m1".to_owned(),
        name: "First".to_owned(),
        description: None,
        sort_order: 1.0,
        target_date: Some("2026-10-15".to_owned()),
    }];
    let mut swm1 = issue(
        "i1",
        "SWM-1",
        Some("p1"),
        "Specced",
        1.0,
        1.0,
        None,
        &["i10"],
    );
    swm1.project_milestone_id = Some("m1".to_owned());
    let mut swm2 = issue(
        "i2",
        "SWM-2",
        Some("p1"),
        "Specced",
        2.0,
        2.0,
        None,
        &["i10"],
    );
    swm2.project_milestone_id = Some("m1".to_owned());
    let mut swm4 = issue("i4", "SWM-4", Some("p1"), "Completed", 1.0, 4.0, None, &[]);
    swm4.project_milestone_id = Some("m1".to_owned());
    let mut swm5 = issue("i5", "SWM-5", Some("p1"), "Canceled", 1.0, 5.0, None, &[]);
    swm5.project_milestone_id = Some("m1".to_owned());
    let swm3 = issue("i3", "SWM-3", Some("p1"), "Todo", 1.0, 3.0, None, &[]);
    let swm6 = issue("i6", "SWM-6", Some("p1"), "Duplicate", 1.0, 6.0, None, &[]);
    let swm7 = issue(
        "i7",
        "SWM-7",
        Some("p1"),
        "Specced",
        1.0,
        7.0,
        None,
        &["i11"],
    );
    let mut swm8 = issue("i8", "SWM-8", Some("p1"), "Todo", 1.0, 8.0, None, &[]);
    swm8.parent_id = None;
    let mut swm9 = issue("i9", "SWM-9", Some("p1"), "Todo", 1.0, 9.0, None, &[]);
    swm9.parent_id = Some(IssueId("i8".to_owned()));
    let blocker_done = issue("i10", "OPS-10", None, "Completed", 1.0, 1.0, None, &["i12"]);
    let blocker_canceled = issue("i11", "OPS-11", None, "Canceled", 1.0, 1.0, None, &[]);
    let blocker_transitive_done = issue("i12", "OPS-12", None, "Completed", 1.0, 1.0, None, &[]);
    let snapshot = WorkspaceSnapshot {
        fetched_at: 1234,
        projects: vec![selected_project],
        issues: vec![
            swm1,
            swm2,
            swm3,
            swm4,
            swm5,
            swm6,
            swm7,
            swm8,
            swm9,
            blocker_done,
            blocker_canceled,
            blocker_transitive_done,
        ],
    };
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(9_000),
        &directory,
    );

    let status = reads
        .focus_status(&ix_board::FocusSelection {
            projects: vec!["SWM-P1".to_owned()],
            ..Default::default()
        })
        .expect("focus status");
    assert_eq!(status.fetched_at, 9_000);
    let project = &status.projects[0];
    assert_eq!(project.target_date.as_deref(), Some("2026-10-30"));
    assert_eq!(project.progress.done, 1);
    assert_eq!(project.progress.total, 6);
    assert!(
        (project.progress.completed_percent.expect("leaf percentage") - 100.0 / 6.0).abs() < 0.0001
    );
    assert!(
        project
            .open_by_state
            .iter()
            .any(|state| { state.state == "Duplicate" && state.count == 1 })
    );
    assert_eq!(
        project.milestones[0].target_date.as_deref(),
        Some("2026-10-15")
    );
    assert_eq!(project.milestones[0].progress.done, 1);
    assert_eq!(project.milestones[0].progress.total, 3);
    assert_eq!(project.unmilestoned.progress.done, 0);
    assert_eq!(project.unmilestoned.progress.total, 3);
    assert_eq!(
        project
            .unmilestoned
            .in_progress
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-3", "SWM-8", "SWM-9"]
    );
    assert_eq!(
        project
            .unmilestoned
            .open_by_state
            .iter()
            .find(|count| count.state == "Todo")
            .map(|count| count.count),
        Some(3)
    );
    let json = serde_json::to_value(&status).expect("serialize focus status");
    assert_eq!(json["projects"][0]["unmilestoned"]["progress"]["total"], 3);
    assert_eq!(
        project
            .in_progress
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-3", "SWM-8", "SWM-9"]
    );
    assert_eq!(
        status
            .ready_queue
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-1", "SWM-2"]
    );
    assert!(
        status
            .ready_queue
            .iter()
            .all(|issue| !issue.identifier.starts_with("OPS-"))
    );
    let shared = status
        .blocker_layers
        .iter()
        .flatten()
        .find(|blocker| blocker.identifier == "OPS-10")
        .expect("shared blocker");
    assert_eq!(shared.state, "Completed");
    assert_eq!(
        shared.classification,
        ix_board::FocusIssueClassification::Completed
    );
    assert_eq!(shared.focus_root_ids, ["SWM-1", "SWM-2"]);
    let transitive = status
        .blocker_layers
        .iter()
        .flatten()
        .find(|blocker| blocker.identifier == "OPS-12")
        .expect("transitive blocker");
    assert_eq!(transitive.state, "Completed");
    assert_eq!(
        transitive.classification,
        ix_board::FocusIssueClassification::Completed
    );
    assert_eq!(transitive.focus_root_ids, ["SWM-1", "SWM-2"]);
    let canceled = status
        .blocker_layers
        .iter()
        .flatten()
        .find(|blocker| blocker.identifier == "OPS-11")
        .expect("canceled blocker");
    assert_eq!(
        canceled.classification,
        ix_board::FocusIssueClassification::Canceled
    );
    assert_eq!(canceled.focus_root_ids, ["SWM-7"]);
    assert_eq!(
        reads
            .focus_status(&ix_board::FocusSelection {
                projects: vec!["SWM-P1".to_owned()],
                ..Default::default()
            })
            .expect("repeat focus status"),
        status
    );
}

/// Trace: FR-012-AC-8
#[test]
fn focus_status_work_milestones_limit_ready_and_blocker_roots_without_hiding_progress() {
    let directory = TempDir::new().expect("temporary directory");
    let mut selected = project("p1", "SWM-P1", "Started", 2, 1.0);
    selected.milestones = vec![
        Milestone {
            id: "m1".to_owned(),
            name: "M1".to_owned(),
            description: None,
            sort_order: 1.0,
            target_date: None,
        },
        Milestone {
            id: "m7".to_owned(),
            name: "M7".to_owned(),
            description: None,
            sort_order: 7.0,
            target_date: None,
        },
    ];
    let mut now = issue("i1", "SWM-1", Some("p1"), "Specced", 1.0, 1.0, None, &[]);
    now.project_milestone_id = Some("m1".to_owned());
    let mut later = issue(
        "i7",
        "SWM-7",
        Some("p1"),
        "Specced",
        1.0,
        7.0,
        None,
        &["b7"],
    );
    later.project_milestone_id = Some("m7".to_owned());
    let blocker = issue("b7", "OPS-7", None, "Completed", 1.0, 1.0, None, &[]);
    let snapshot = WorkspaceSnapshot {
        projects: vec![selected],
        issues: vec![now, later, blocker],
        fetched_at: 0,
    };
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(2_000),
        &directory,
    );
    let focus = ix_board::FocusSelection {
        projects: vec!["SWM-P1".to_owned()],
        ..Default::default()
    };
    let all = reads.focus_status(&focus).expect("unfiltered status");
    assert_eq!(all.ready_queue.len(), 2);
    assert!(
        all.blocker_layers
            .iter()
            .flatten()
            .any(|item| item.identifier == "OPS-7")
    );

    let active = reads
        .focus_status_for_milestones(&focus, &["M1".to_owned()])
        .expect("M1 status");
    assert_eq!(active.work_milestones, ["M1"]);
    assert_eq!(active.excluded_ready_count, 1);
    assert_eq!(
        active
            .ready_queue
            .iter()
            .map(|item| item.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-1"]
    );
    assert_eq!(
        active
            .blocker_layers
            .iter()
            .flatten()
            .map(|item| item.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-1"]
    );
    assert_eq!(
        active.projects[0].progress.total,
        all.projects[0].progress.total
    );
    assert_eq!(active.projects[0].milestones.len(), 2);
    assert!(matches!(
        reads.focus_status_for_milestones(&focus, &["M9".to_owned()]),
        Err(Error::WorkMilestoneNotFound { .. })
    ));
}

/// Trace: FR-012-AC-2
#[test]
fn focus_status_reports_null_percent_for_groups_without_countable_leaves() {
    let directory = TempDir::new().expect("temporary directory");
    let snapshot = WorkspaceSnapshot {
        fetched_at: 0,
        projects: vec![project("p1", "SWM-P1", "Started", 2, 1.0)],
        issues: vec![issue(
            "i1",
            "SWM-1",
            Some("p1"),
            "Canceled",
            1.0,
            1.0,
            None,
            &[],
        )],
    };
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(9_000),
        &directory,
    );
    let status = reads
        .focus_status(&ix_board::FocusSelection {
            projects: vec!["SWM-P1".to_owned()],
            ..Default::default()
        })
        .expect("focus status with no countable leaves");
    assert_eq!(status.projects[0].progress.done, 0);
    assert_eq!(status.projects[0].progress.total, 0);
    assert_eq!(status.projects[0].progress.completed_percent, None);
    assert_eq!(status.projects[0].target_date, None);
}

/// Trace: FR-014-AC-1, FR-014-AC-2, FR-014-AC-3, FR-014-AC-4, FR-014-AC-5, FR-014-AC-6, FR-014-AC-8, FR-014-AC-9, FR-014-AC-10
#[test]
fn focus_plan_chunk_orders_full_closure_and_returns_handoff_facts() {
    let directory = TempDir::new().expect("temporary directory");
    let mut root_project = project("p1", "IR-P1", "Started", 1, 2.0);
    root_project.milestones = vec![
        Milestone {
            id: "m0".to_owned(),
            name: "M0".to_owned(),
            description: None,
            sort_order: 0.0,
            target_date: None,
        },
        Milestone {
            id: "m1".to_owned(),
            name: "M1".to_owned(),
            description: None,
            sort_order: 1.0,
            target_date: None,
        },
        Milestone {
            id: "m2".to_owned(),
            name: "M2".to_owned(),
            description: None,
            sort_order: 2.0,
            target_date: None,
        },
    ];
    let mut root = with_description(
        issue("i317", "IR-317", Some("p1"), "Specced", 1.0, 1.0, None, &[]),
        "Intro\n## acceptance criteria \n  First criterion.\n## Notes\nignored",
    );
    root.project_milestone_id = Some("m0".to_owned());
    let mut first_dependent = issue(
        "i318",
        "IR-318",
        Some("p1"),
        "Specced",
        1.0,
        2.0,
        None,
        &["i317"],
    );
    first_dependent.project_milestone_id = Some("m1".to_owned());
    let mut same_layer_m1 = issue("i315", "IR-315", Some("p1"), "Specced", 1.0, 0.0, None, &[]);
    same_layer_m1.project_milestone_id = Some("m1".to_owned());
    let mut deep_dependent = with_description(
        issue(
            "i319",
            "IR-319",
            Some("p1"),
            "Specced",
            1.0,
            3.0,
            None,
            &["i320"],
        ),
        &format!("## Acceptance Criteria\n{}", "🧪".repeat(281)),
    );
    deep_dependent.project_milestone_id = Some("m2".to_owned());
    let mut later_project = project("p2", "IR-P2", "Started", 2, 1.0);
    later_project.milestones = vec![Milestone {
        id: "m0p2".to_owned(),
        name: "M0".to_owned(),
        description: None,
        sort_order: 0.0,
        target_date: None,
    }];
    let project_order_tie = issue("i321", "IR-321", Some("p1"), "Specced", 1.0, 4.0, None, &[]);
    let mut other_project = issue("i316", "IR-316", Some("p2"), "Specced", 1.0, 1.0, None, &[]);
    other_project.project_milestone_id = Some("m0p2".to_owned());
    let paused_project = project("p3", "IR-P3", "Paused", 0, 0.0);
    let paused_issue = issue("i322", "IR-322", Some("p3"), "Specced", 1.0, 1.0, None, &[]);
    let completed_middle = issue(
        "i320",
        "OPS-320",
        None,
        "Completed",
        1.0,
        4.0,
        None,
        &["i318"],
    );
    let snapshot = WorkspaceSnapshot {
        fetched_at: 1234,
        projects: vec![root_project, later_project, paused_project],
        issues: vec![
            root,
            first_dependent,
            same_layer_m1,
            deep_dependent,
            project_order_tie,
            other_project,
            paused_issue,
            completed_middle,
        ],
    };
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(9_000),
        &directory,
    );
    let selection = ix_board::FocusSelection {
        projects: vec!["IR-P1".to_owned(), "IR-P2".to_owned(), "IR-P3".to_owned()],
        ..Default::default()
    };
    let chunk = reads
        .focus_plan_chunk(&selection, None)
        .expect("plan chunk");
    assert_eq!(chunk.fetched_at, 9_000);
    assert_eq!(
        chunk
            .issues
            .iter()
            .map(|row| row.identifier.as_str())
            .collect::<Vec<_>>(),
        [
            "IR-317", "IR-315", "IR-321", "IR-316", "IR-322", "IR-318", "IR-319"
        ]
    );
    assert_eq!(chunk.issues[0].dependency_layer, 0);
    assert_eq!(chunk.issues[5].dependency_layer, 1);
    assert_eq!(chunk.issues[6].dependency_layer, 3);
    assert!(chunk.issues[0].ready);
    assert!(!chunk.issues[5].ready);
    assert!(!chunk.issues[4].ready);
    assert!(
        chunk.issues[4]
            .reasons
            .iter()
            .any(|reason| reason.contains("Paused"))
    );
    assert!(
        chunk.issues[5]
            .reasons
            .iter()
            .any(|reason| reason.contains("IR-317"))
    );
    assert_eq!(chunk.issues[5].milestone.as_deref(), Some("M1"));
    assert_eq!(chunk.issues[0].unblocks, ["IR-318", "IR-319"]);
    let m1_chunk = reads
        .focus_plan_chunk_for_milestones(&selection, None, &["M1".to_owned()])
        .expect("M1 handoff");
    assert_eq!(m1_chunk.work_milestones, ["M1"]);
    assert_eq!(
        m1_chunk
            .issues
            .iter()
            .map(|row| row.identifier.as_str())
            .collect::<Vec<_>>(),
        ["IR-317", "IR-315", "IR-318"]
    );
    assert!(matches!(
        reads.focus_plan_chunk_for_milestones(&selection, None, &["M9".to_owned()]),
        Err(Error::WorkMilestoneNotFound { .. })
    ));
    assert_eq!(
        chunk.issues[0].acceptance_criteria.as_deref(),
        Some("First criterion.")
    );
    let unicode_excerpt = chunk.issues[6]
        .acceptance_criteria
        .as_deref()
        .expect("unicode acceptance excerpt");
    assert_eq!(unicode_excerpt.chars().count(), 281);
    assert!(unicode_excerpt.ends_with('…'));
    assert_eq!(
        reads
            .focus_plan_chunk(&selection, std::num::NonZeroUsize::new(2))
            .expect("sized plan chunk")
            .issues
            .iter()
            .map(|row| row.identifier.as_str())
            .collect::<Vec<_>>(),
        ["IR-317", "IR-315"]
    );
    assert_eq!(
        reads
            .focus_plan_chunk(&selection, None)
            .expect("repeat plan chunk"),
        chunk
    );
}

/// Trace: FR-014-AC-6
#[test]
fn focus_plan_chunk_refuses_empty_missing_and_cyclic_input_without_partial_rows() {
    let directory = TempDir::new().expect("temporary directory");
    let incomplete = WorkspaceSnapshot {
        fetched_at: 100,
        projects: vec![project("p1", "IR-P1", "Started", 1, 1.0)],
        issues: vec![issue(
            "i1",
            "IR-1",
            Some("p1"),
            "Specced",
            1.0,
            1.0,
            None,
            &["missing"],
        )],
    };
    let mut missing_reads = service(
        FixtureSource::new(incomplete),
        ManualClock::new(1_000),
        &directory,
    );
    assert!(matches!(
        missing_reads.focus_plan_chunk(&ix_board::FocusSelection::default(), None),
        Err(Error::EmptyFocus)
    ));
    assert!(matches!(
        missing_reads.focus_plan_chunk(
            &ix_board::FocusSelection {
                projects: vec!["IR-P1".to_owned()],
                ..Default::default()
            },
            None
        ),
        Err(Error::MissingDependency { issue }) if issue == "missing"
    ));

    let mut cycle_issue = issue("i2", "IR-2", Some("p2"), "Specced", 1.0, 1.0, None, &[]);
    cycle_issue.blocked_by.push(IssueId("i2".to_owned()));
    let cyclic = WorkspaceSnapshot {
        fetched_at: 200,
        projects: vec![project("p2", "IR-P2", "Started", 1, 1.0)],
        issues: vec![cycle_issue],
    };
    let mut cycle_reads = service(
        FixtureSource::new(cyclic).with_namespace("plan-cycle"),
        ManualClock::new(2_000),
        &directory,
    );
    assert!(matches!(
        cycle_reads.focus_plan_chunk(
            &ix_board::FocusSelection {
                projects: vec!["IR-P2".to_owned()],
                ..Default::default()
            },
            None
        ),
        Err(Error::DependencyCycle { .. })
    ));
}

/// Trace: FR-014-AC-2
#[test]
fn focus_plan_chunk_keeps_open_coding_review_and_unmapped_spec_candidates() {
    let directory = TempDir::new().expect("temporary directory");
    let mut completed_blocker = issue("d1", "OPS-1", None, "Done", 1.0, 1.0, None, &[]);
    completed_blocker.state_type = ix_board::WorkflowStateType::Completed;
    completed_blocker.state_name = "Done".to_owned();
    let coding = issue(
        "i1",
        "IR-CODING",
        Some("p1"),
        "Started",
        1.0,
        1.0,
        Some("Coding"),
        &[],
    );
    let review = issue(
        "i2",
        "IR-REVIEW",
        Some("p1"),
        "Started",
        1.0,
        2.0,
        Some("Review"),
        &[],
    );
    let unmapped = issue(
        "i3",
        "IR-UNKNOWN",
        Some("p1"),
        "Started",
        1.0,
        3.0,
        Some("Needs Design"),
        &[],
    );
    let ready = issue(
        "i4",
        "IR-READY",
        Some("p1"),
        "Specced",
        1.0,
        4.0,
        None,
        &["d1"],
    );
    let snapshot = WorkspaceSnapshot {
        fetched_at: 50,
        projects: vec![project("p1", "IR-P1", "Started", 1, 1.0)],
        issues: vec![coding, review, unmapped, ready, completed_blocker],
    };
    let queue = ix_board::ready_from_snapshot(&snapshot, "IR").expect("FR-001 ready queue");
    assert_eq!(
        queue
            .iter()
            .map(|row| row.identifier.as_str())
            .collect::<Vec<_>>(),
        ["IR-READY"]
    );
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(9_000),
        &directory,
    );
    let chunk = reads
        .focus_plan_chunk(
            &ix_board::FocusSelection {
                projects: vec!["IR-P1".to_owned()],
                ..Default::default()
            },
            None,
        )
        .expect("plan chunk retains every open focus candidate");
    assert_eq!(
        chunk
            .issues
            .iter()
            .map(|row| row.identifier.as_str())
            .collect::<Vec<_>>(),
        ["IR-CODING", "IR-REVIEW", "IR-UNKNOWN", "IR-READY"]
    );
    assert_eq!(
        chunk.issues.iter().map(|row| row.ready).collect::<Vec<_>>(),
        [false, false, false, true]
    );
    assert!(
        chunk.issues[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("no new Specify or Coding step"))
    );
    assert!(
        chunk.issues[1]
            .reasons
            .iter()
            .any(|reason| reason.contains("no new Specify or Coding step"))
    );
    assert!(
        chunk.issues[2]
            .reasons
            .iter()
            .any(|reason| reason.contains("no new Specify or Coding step"))
    );
}

/// Trace: FR-011-AC-4, FR-012-AC-6
#[test]
fn focus_expansion_rejects_empty_or_unresolved_selectors_without_partial_data() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(1_000), &directory);
    assert!(matches!(
        reads.focus_expand(&ix_board::FocusSelection::default()),
        Err(Error::EmptyFocus)
    ));
    assert!(matches!(
        reads.focus_status(&ix_board::FocusSelection::default()),
        Err(Error::EmptyFocus)
    ));
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    assert!(matches!(
        reads.focus_expand(&ix_board::FocusSelection {
            projects: vec!["missing".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::ScopeSelection {
            kind: "project",
            matches: 0,
            ..
        })
    ));
    assert!(matches!(
        reads.focus_status(&ix_board::FocusSelection {
            projects: vec!["missing".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::ScopeSelection {
            kind: "project",
            matches: 0,
            ..
        })
    ));
}

/// Trace: FR-011-AC-5, FR-012-AC-6
#[test]
fn focus_expansion_refuses_missing_blockers_cycles_and_unknown_milestones() {
    let directory = TempDir::new().expect("temporary directory");
    let mut missing = queue_fixture();
    missing.issues[0].blocked_by = vec![IssueId("absent".to_owned())];
    let mut reads = service(
        FixtureSource::new(missing),
        ManualClock::new(2_000),
        &directory,
    );
    assert!(matches!(
        reads.focus_expand(&ix_board::FocusSelection {
            issues: vec!["SWM-1".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::MissingDependency { .. })
    ));
    assert!(matches!(
        reads.focus_status(&ix_board::FocusSelection {
            issues: vec!["SWM-1".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::MissingDependency { .. })
    ));

    let directory = TempDir::new().expect("temporary directory");
    let mut cycle = queue_fixture();
    let focus_id = cycle.issues[0].id.clone();
    cycle.issues[0].blocked_by = vec![IssueId("cycle".to_owned())];
    let mut cycle_issue = issue("cycle", "OPS-9", None, "Todo", 1.0, 1.0, None, &[]);
    cycle_issue.blocked_by = vec![focus_id];
    cycle.issues.push(cycle_issue);
    let mut reads = service(
        FixtureSource::new(cycle),
        ManualClock::new(2_000),
        &directory,
    );
    assert!(matches!(
        reads.focus_expand(&ix_board::FocusSelection {
            issues: vec!["SWM-1".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::DependencyCycle { .. })
    ));
    assert!(matches!(
        reads.focus_status(&ix_board::FocusSelection {
            issues: vec!["SWM-1".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::DependencyCycle { .. })
    ));

    let directory = TempDir::new().expect("temporary directory");
    let mut bad_milestone = queue_fixture();
    bad_milestone.issues[0].project_milestone_id = Some("absent-milestone".to_owned());
    let mut reads = service(
        FixtureSource::new(bad_milestone),
        ManualClock::new(2_000),
        &directory,
    );
    assert!(matches!(
        reads.focus_expand(&ix_board::FocusSelection {
            projects: vec!["SWM-P1".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::MissingProjectMilestone { .. })
    ));
    assert!(matches!(
        reads.focus_status(&ix_board::FocusSelection {
            projects: vec!["SWM-P1".to_owned()],
            ..ix_board::FocusSelection::default()
        }),
        Err(Error::MissingProjectMilestone { .. })
    ));
}

#[test]
fn search_is_case_insensitive_structured_and_ordered_by_kind_then_identifier() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(3_000), &directory);

    assert!(matches!(reads.search("  "), Err(Error::InvalidQuery)));
    assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    let results = reads.search("comment-only phrase").expect("search");
    assert!(results.is_empty());
    let results = reads.search("TITLE SWM-05").expect("search");
    assert!(
        results
            .iter()
            .all(|result| result.kind == SearchKind::Issue)
    );
    assert_eq!(
        results
            .iter()
            .map(|result| result.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-05"]
    );
    let uuid_match = reads.search("i05").expect("UUID search");
    assert_eq!(uuid_match[0].identifier, "SWM-05");
    let mixed = reads.search("swm").expect("mixed search");
    assert_eq!(mixed[0].kind, SearchKind::Project);
    let first_issue = mixed
        .iter()
        .position(|result| result.kind == SearchKind::Issue)
        .expect("issue match");
    assert!(
        mixed[..first_issue]
            .iter()
            .all(|result| result.kind == SearchKind::Project)
    );
    assert!(
        mixed[first_issue..]
            .iter()
            .all(|result| result.kind == SearchKind::Issue)
    );
}

/// Trace: FR-015-AC-1, FR-015-AC-2
#[test]
fn issue_batch_returns_exact_states_in_request_order_with_one_snapshot() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(3_000), &directory);
    let batch = reads
        .issues(&["SWM-2".to_owned(), "swm-1".to_owned(), "SWM-2".to_owned()])
        .expect("issue batch");
    assert_eq!(
        batch
            .issues
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        ["SWM-2", "SWM-1"]
    );
    assert!(batch.complete);
    assert!(batch.missing.is_empty());
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    let partial = reads
        .issues(&[
            "SWM-1".to_owned(),
            "SWM-404".to_owned(),
            "swm-404".to_owned(),
        ])
        .expect("partial issue batch");
    assert_eq!(partial.issues.len(), 1);
    assert_eq!(partial.issues[0].identifier, "SWM-1");
    assert_eq!(partial.missing, ["SWM-404"]);
    assert!(!partial.complete);
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
}

/// Trace: FR-010-AC-4
#[test]
fn cache_reuses_fresh_snapshot_and_refreshes_after_five_minutes() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let clock = ManualClock::new(10_000);
    let mut reads = service(source.clone(), clock.clone(), &directory);

    let initial = reads.search("Title SWM-1").expect("initial read");
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    assert_eq!(reads.workspace_snapshot().unwrap().fetched_at, 10_000);
    source.replace(WorkspaceSnapshot {
        fetched_at: 0,

        projects: queue_fixture().projects,
        issues: vec![Issue {
            parent_id: None,
            state_type: ix_board::WorkflowStateType::Started,
            project_milestone_id: None,
            state_name: "Todo".to_owned(),

            title: "Changed title".to_owned(),
            ..issue(
                "changed",
                "SWM-1",
                Some("p1"),
                "Todo",
                1.0,
                1.0,
                Some("Specced"),
                &[],
            )
        }],
    });
    assert_eq!(reads.search("changed").expect("warm cache").len(), 0);
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    clock.set(10_001);
    assert_eq!(reads.workspace_snapshot().unwrap().fetched_at, 10_000);
    clock.set(10_000 + ix_board::CACHE_TTL_SECONDS);
    let refreshed = reads.search("changed").expect("expired cache refresh");
    assert_eq!(refreshed.len(), 1);
    assert_eq!(
        reads.workspace_snapshot().unwrap().fetched_at,
        10_000 + ix_board::CACHE_TTL_SECONDS
    );
    assert_ne!(initial, refreshed);
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
}

/// Trace: FR-010-AC-4
#[test]
fn forced_refresh_bypasses_fresh_cache_and_replaces_it() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let clock = ManualClock::new(10_000);
    let mut reads = service(source.clone(), clock.clone(), &directory);

    assert_eq!(
        reads
            .workspace_snapshot()
            .expect("initial snapshot")
            .projects[0]
            .name,
        "Project SWM-P1"
    );
    let mut changed = queue_fixture();
    changed.projects[0].name = "Updated project".to_owned();
    source.replace(changed);
    clock.set(10_001);

    assert_eq!(
        reads.workspace_snapshot().expect("warm cache").projects[0].name,
        "Project SWM-P1"
    );
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        reads
            .force_refresh_workspace()
            .expect("forced refresh")
            .projects[0]
            .name,
        "Updated project"
    );
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    assert_eq!(reads.workspace_snapshot().unwrap().fetched_at, 10_001);
    assert_eq!(
        reads.workspace_snapshot().expect("replaced cache").projects[0].name,
        "Updated project"
    );
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    assert_eq!(reads.workspace_snapshot().unwrap().fetched_at, 10_001);
}

#[test]
fn invalidation_removes_only_the_current_source_snapshot() {
    let directory = TempDir::new().expect("temporary directory");
    let cache_path = directory.path().join("shared-cache.sqlite");
    let clock = ManualClock::new(10_000);
    let first_source = FixtureSource::new(queue_fixture()).with_namespace("first");
    let second_source = FixtureSource::new(queue_fixture()).with_namespace("second");
    let mut first = ReadService::with_clock(first_source.clone(), &cache_path, clock.clone())
        .expect("first service");
    let mut second =
        ReadService::with_clock(second_source.clone(), &cache_path, clock).expect("second service");
    first.workspace_snapshot().expect("first cache fill");
    second.workspace_snapshot().expect("second cache fill");

    first.invalidate_cache().expect("invalidate first cache");
    first.workspace_snapshot().expect("first refetch");
    second.workspace_snapshot().expect("second cache reuse");
    assert_eq!(first_source.reads.load(Ordering::SeqCst), 2);
    assert_eq!(second_source.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_forced_refresh_does_not_leave_a_fresh_stale_snapshot() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let mut reads = service(source.clone(), ManualClock::new(10_000), &directory);
    reads.workspace_snapshot().expect("initial cache fill");
    source.fail.store(true, Ordering::SeqCst);

    assert!(matches!(
        reads.force_refresh_workspace(),
        Err(Error::GraphQl { .. })
    ));
    assert!(matches!(
        reads.workspace_snapshot(),
        Err(Error::GraphQl { .. })
    ));
    assert_eq!(source.reads.load(Ordering::SeqCst), 3);
}

#[test]
fn cache_snapshot_isolated_between_source_namespaces() {
    let directory = TempDir::new().expect("temporary directory");
    let cache_path = directory.path().join("shared-cache.sqlite");
    let clock = ManualClock::new(30_000);
    let mut first_snapshot = queue_fixture();
    first_snapshot.projects[0].name = "First namespace project".to_owned();
    let first_source = FixtureSource::new(first_snapshot).with_namespace("namespace-one");
    let mut first = ReadService::with_clock(first_source, &cache_path, clock.clone())
        .expect("open first namespace cache");
    assert_eq!(
        first
            .search("First namespace")
            .expect("first snapshot")
            .len(),
        1
    );

    let mut second_snapshot = queue_fixture();
    second_snapshot.projects[0].name = "Second namespace project".to_owned();
    let second_source = FixtureSource::new(second_snapshot).with_namespace("namespace-two");
    let reads = second_source.reads.clone();
    let mut second = ReadService::with_clock(second_source, &cache_path, clock)
        .expect("open second namespace cache");
    assert_eq!(
        second
            .search("Second namespace")
            .expect("second snapshot")
            .len(),
        1
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn cache_database_permissions_are_restricted_for_existing_files() {
    use std::{fs::Permissions, os::unix::fs::PermissionsExt};

    let directory = TempDir::new().expect("temporary directory");
    let cache_path = directory.path().join("existing-cache.sqlite");
    std::fs::write(&cache_path, []).expect("create existing cache file");
    std::fs::set_permissions(&cache_path, Permissions::from_mode(0o644))
        .expect("make existing cache readable by group");

    let _service = ReadService::with_clock(
        FixtureSource::new(queue_fixture()),
        &cache_path,
        ManualClock::new(40_000),
    )
    .expect("open cache");
    let mode = std::fs::metadata(cache_path)
        .expect("cache metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn failed_expired_refresh_returns_error_instead_of_stale_snapshot() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let clock = ManualClock::new(20_000);
    let mut reads = service(source.clone(), clock.clone(), &directory);

    assert!(!reads.search("SWM-1").expect("initial read").is_empty());
    clock.set(20_000 + ix_board::CACHE_TTL_SECONDS);
    source.fail.store(true, Ordering::SeqCst);
    assert!(matches!(reads.search("SWM-1"), Err(Error::GraphQl { .. })));
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
}

#[test]
fn cli_exposes_all_three_reads_and_rejects_empty_search_before_api_setup() {
    let help = Command::cargo_bin("ix-board")
        .expect("built binary")
        .arg("--help")
        .output()
        .expect("CLI help");
    assert!(help.status.success());
    let help_text = String::from_utf8(help.stdout).expect("UTF-8 help");
    assert!(help_text.contains("next"));
    assert!(help_text.contains("project-chunk"));
    assert!(help_text.contains("search"));
    assert!(help_text.contains("project"));

    let invalid = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args(["search", " "])
        .output()
        .expect("empty search");
    assert_eq!(invalid.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("search query must not be empty"));

    let invalid_priority = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args(["project", "set-priority", "SWM-P1", "5"])
        .env_remove("LINEAR_API_KEY")
        .output()
        .expect("invalid project priority");
    assert_eq!(invalid_priority.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&invalid_priority.stderr)
            .contains("expected one of 0, 1, 2, 3, or 4")
    );

    let invalid_status = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args(["project", "set-status", "SWM-P1", "Completed"])
        .env_remove("LINEAR_API_KEY")
        .output()
        .expect("invalid project status");
    assert_eq!(invalid_status.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid_status.stderr).contains("expected Started or Paused"));
}

#[test]
#[ignore = "NFR-001 warm-cache CLI benchmark; run explicitly with --ignored --nocapture"]
fn warm_cache_next_cli_runs_under_one_second() {
    use std::time::{Duration, Instant};

    let directory = TempDir::new().expect("temporary directory");
    let cache_path = directory.path().join("benchmark.sqlite");
    let source = FixtureSource::new(queue_fixture());
    let mut warmer = ReadService::open(source, &cache_path).expect("open cache");
    assert!(!warmer.next("SWM").expect("warm fixture cache").is_empty());
    drop(warmer);

    for run in 1..=5 {
        let start = Instant::now();
        let result = Command::cargo_bin("ix-board")
            .expect("built binary")
            .args(["next", "--team", "SWM", "--json"])
            .env("IX_BOARD_CACHE_PATH", &cache_path)
            .env("LINEAR_API_KEY", "warm-cache-fixture")
            .output()
            .expect("warm-cache CLI");
        let elapsed = start.elapsed();
        eprintln!("warm-cache next run {run}: {elapsed:?}");
        assert!(
            result.status.success(),
            "CLI stderr: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "run {run} took {elapsed:?}"
        );
    }
}

#[test]
fn auth_status_reports_environment_source_without_exposing_key() {
    let output = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args(["auth", "status", "--json"])
        .env("LINEAR_API_KEY", "fictional-secret-token")
        .output()
        .expect("auth status");
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(result["status"], "ready");
    assert_eq!(result["source"], "env");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fictional-secret-token"));
}

#[test]
fn auth_login_rejects_empty_stdin_without_touching_store() {
    let output = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args(["auth", "login", "--stdin"])
        .write_stdin("\n")
        .output()
        .expect("auth login");
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must not be empty"));
}

#[test]
fn drift_cli_emits_stable_typed_json_and_one_line_per_lead() {
    let directory = TempDir::new().expect("temporary directory");
    let config_path = directory.path().join("drift-config.json");
    let evidence_path = directory.path().join("drift-evidence.json");
    let config = DriftConfig {
        report_time: "2026-09-27T12:00:00Z".to_owned(),
        window_hours: 24.0,
        stall_hours: 12.0,
        spend_threshold: 0.0,
        max_net_ticket_growth: 0,
        min_merged_prs_for_churn: 1,
        leads: vec![DriftLead {
            lead_id: "lead-a".to_owned(),
            team_key: "SWM".to_owned(),
        }],
    };
    let evidence = DriftEvidenceFile {
        schema_version: 1,
        evidence: DriftEvidence {
            projects: Vec::new(),
            tickets: Vec::new(),
            transitions: Vec::new(),
            pull_requests: Vec::new(),
            commits: Vec::new(),
            herdr_busy: std::collections::BTreeMap::from([("lead-a".to_owned(), false)]),
            otel_spend: std::collections::BTreeMap::from([("lead-a".to_owned(), 0.0)]),
            missing: Vec::new(),
        },
    };
    std::fs::write(
        &config_path,
        serde_json::to_vec(&config).expect("serialize drift config"),
    )
    .expect("write config");
    std::fs::write(
        &evidence_path,
        serde_json::to_vec(&evidence).expect("serialize drift evidence"),
    )
    .expect("write evidence");
    let run = || {
        Command::cargo_bin("ix-board")
            .expect("built binary")
            .args([
                "drift",
                "--config",
                config_path.to_str().expect("config path utf8"),
                "--evidence",
                evidence_path.to_str().expect("evidence path utf8"),
                "--json",
            ])
            .env_remove("LINEAR_API_KEY")
            .output()
            .expect("drift command")
    };
    let result = run();
    assert!(
        result.status.success(),
        "CLI stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("valid JSON report");
    assert_eq!(value["report_time"], "2026-09-27T12:00:00Z");
    assert_eq!(value["window_start"], "2026-09-26T12:00:00Z");
    assert_eq!(value["window_end"], value["report_time"]);
    assert_eq!(value["unassigned_prs"].as_array().map(Vec::len), Some(0));
    let lead = &value["leads"][0];
    assert_eq!(lead["lead_id"], "lead-a");
    assert_eq!(lead["team_key"], "SWM");
    assert_eq!(lead["status"], "on_track");
    assert_eq!(lead["running_projects"].as_array().map(Vec::len), Some(0));
    assert_eq!(lead["alerts"].as_array().map(Vec::len), Some(0));
    assert_eq!(lead["reasons"].as_array().map(Vec::len), Some(0));
    let signals = &lead["signals"];
    for signal in [
        "pr_scope",
        "veering",
        "stall",
        "busy_without_pr",
        "ticket_churn",
    ] {
        assert_eq!(signals[signal]["availability"], "available");
        assert_eq!(
            signals[signal]["missing_sources"].as_array().map(Vec::len),
            Some(0)
        );
    }
    assert!(signals["pr_scope"]["prs"].is_array());
    assert!(signals["veering"]["tickets"].is_array());
    assert!(signals["stall"]["tickets"].is_array());
    assert_eq!(signals["busy_without_pr"]["finding"], false);
    assert_eq!(signals["busy_without_pr"]["herdr_busy"], false);
    assert_eq!(signals["busy_without_pr"]["otel_spend"], 0.0);
    assert_eq!(signals["busy_without_pr"]["new_pr_count"], 0);
    assert_eq!(signals["ticket_churn"]["created"], 0);
    assert_eq!(signals["ticket_churn"]["completed"], 0);
    assert_eq!(signals["ticket_churn"]["merged_prs"], 0);
    assert_eq!(signals["ticket_churn"]["net_growth"], 0);
    assert_eq!(run().stdout, result.stdout);

    let text = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args([
            "drift",
            "--config",
            config_path.to_str().expect("config path utf8"),
            "--evidence",
            evidence_path.to_str().expect("evidence path utf8"),
        ])
        .env_remove("LINEAR_API_KEY")
        .output()
        .expect("drift text command");
    assert!(text.status.success());
    assert_eq!(String::from_utf8_lossy(&text.stdout).lines().count(), 1);
    assert!(String::from_utf8_lossy(&text.stdout).contains("lead-a on_track"));
}
