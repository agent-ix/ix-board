// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Named-selection membership, outcomes and batch summaries (FR-010 and FR-012).

use ix_board::{
    BatchState, CatalogEvidence, CountAvailability, Error, FocusSelection, Initiative,
    InitiativeLink, Issue, IssueId, ListKind, MemberOutcome, Milestone, NamedBatch,
    NamedFocusRequest, NamedMember, NamedSelection, PartialReason, Project, ProjectId, ProjectRow,
    ProjectStatusCategory, ReadService, ScopeSelection, SelectionState, SelectionSummary, TeamKey,
    UnknownFact, WorkflowStateType, WorkspaceSnapshot, WorkspaceSource, evaluate_named_batch,
    select_scope,
};
use ix_trace_rs::trace;
use std::collections::BTreeSet;

fn project(id: &str, initiatives: &[&str]) -> Project {
    Project {
        id: ProjectId(id.into()),
        identifier: Some(format!("PRJ-{id}")),
        name: format!("Project {id}"),
        updated_at: None,
        description: String::new(),
        content: None,
        status: "Started".into(),
        status_name: None,
        status_category: ProjectStatusCategory::Started,
        priority: 2,
        sort_order: 0.0,
        target_date: None,
        team_keys: vec![TeamKey("SWM".into())],
        milestones: vec![],
        initiatives: initiatives
            .iter()
            .map(|id| Initiative {
                id: (*id).into(),
                identifier: format!("I-{id}"),
                name: format!("Initiative {id}"),
            })
            .collect(),
    }
}

fn with_milestone(mut project: Project, id: &str, name: &str) -> Project {
    project.milestones.push(Milestone {
        id: id.into(),
        name: name.into(),
        description: None,
        sort_order: 1.0,
        target_date: None,
    });
    project
}

fn issue(id: &str, project: Option<&str>, kind: WorkflowStateType, stage: &str) -> Issue {
    Issue {
        id: IssueId(id.into()),
        identifier: id.into(),
        title: id.into(),
        description: None,
        state: stage.into(),
        state_name: stage.into(),
        state_type: kind,
        parent_id: None,
        team_key: TeamKey(id.split('-').next().unwrap_or(id).into()),
        assignee_id: None,
        assignee_name: None,
        project_id: project.map(|id| ProjectId(id.into())),
        project_milestone_id: None,
        priority: 2.0,
        sort_order: 0.0,
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
        attachment_urls: vec![],
        attachments_complete: true,
    }
}

/// An open issue in a spec state FR-001 can offer, so it is ready unless blocked.
fn open(id: &str, project: &str) -> Issue {
    issue(id, Some(project), WorkflowStateType::Unstarted, "Specced")
}

/// A started issue; its state is not an FR-001 new-work state, so it is never ready.
fn started(id: &str, project: &str) -> Issue {
    issue(id, Some(project), WorkflowStateType::Started, "In Progress")
}

fn done(id: &str, project: &str) -> Issue {
    issue(id, Some(project), WorkflowStateType::Completed, "Done")
}

fn child_of(mut issue: Issue, parent: &str) -> Issue {
    issue.parent_id = Some(IssueId(parent.into()));
    issue
}

fn in_milestone(mut issue: Issue, milestone: &str) -> Issue {
    issue.project_milestone_id = Some(milestone.into());
    issue
}

fn blocked_by(mut issue: Issue, blockers: &[&str]) -> Issue {
    issue.blocked_by = blockers.iter().map(|id| IssueId((*id).into())).collect();
    issue
}

fn snapshot(projects: Vec<Project>, issues: Vec<Issue>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        projects,
        issues,
        fetched_at: 1_000,
    }
}

fn init(id: &str) -> NamedMember {
    NamedMember::Initiative { id: id.into() }
}

fn proj(id: &str) -> NamedMember {
    NamedMember::Project { id: id.into() }
}

fn mile(id: &str, project: &str) -> NamedMember {
    NamedMember::Milestone {
        id: id.into(),
        project_id: project.into(),
    }
}

fn tick(id: &str) -> NamedMember {
    NamedMember::Issue { id: id.into() }
}

fn selection(team: &str, include: Vec<NamedMember>, exclude: Vec<NamedMember>) -> NamedSelection {
    NamedSelection::new(TeamKey(team.into()), include, exclude)
}

fn request(key: &str, selection: NamedSelection) -> NamedFocusRequest {
    NamedFocusRequest {
        key: key.into(),
        selection,
    }
}

fn evaluate(snapshot: &WorkspaceSnapshot, requests: Vec<NamedFocusRequest>) -> NamedBatch {
    evaluate_named_batch(snapshot, &CatalogEvidence::default(), &requests)
        .expect("the batch evaluates")
}

fn one(snapshot: &WorkspaceSnapshot, selection: NamedSelection) -> SelectionSummary {
    evaluate(snapshot, vec![request("k", selection)])
        .selections
        .remove(0)
}

fn leaves(summary: &SelectionSummary) -> BTreeSet<String> {
    summary.leaf_ids.iter().map(|id| id.0.clone()).collect()
}

fn want(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn standalone<'a>(summary: &'a SelectionSummary, id: &str) -> &'a ProjectRow {
    summary
        .projects
        .iter()
        .find(|row| row.project_id.0 == id)
        .expect("a standalone row for the project")
}

fn outcome(summary: &SelectionSummary, list: ListKind, member: &NamedMember) -> MemberOutcome {
    summary
        .members
        .iter()
        .find(|report| report.list == list && report.member == *member)
        .expect("a report for the member")
        .outcome
}

/// Initiative `ini` holds `p1` (SWM-1, SWM-2) and `p2` (SWM-3, SWM-4).
fn two_project_initiative() -> WorkspaceSnapshot {
    snapshot(
        vec![project("p1", &["ini"]), project("p2", &["ini"])],
        vec![
            open("SWM-1", "p1"),
            open("SWM-2", "p1"),
            open("SWM-3", "p2"),
            open("SWM-4", "p2"),
        ],
    )
}

#[trace("FR-010-AC-9")]
#[test]
fn exclusions_remove_by_closure_and_win_over_inclusion_and_reversal_restores() {
    let snapshot = two_project_initiative();
    let include = vec![init("ini"), tick("SWM-3")];
    let excluding = one(
        &snapshot,
        selection("SWM", include.clone(), vec![proj("p2"), tick("SWM-2")]),
    );
    // P1's leaves remain; the P1 leaf excluded by ticket and the P2 leaf that the
    // same selection also includes by ticket are both absent.
    assert_eq!(leaves(&excluding), want(&["SWM-1"]));
    assert_eq!(excluding.state, SelectionState::Complete);
    let restored = one(&snapshot, selection("SWM", include, vec![]));
    assert_eq!(
        leaves(&restored),
        want(&["SWM-1", "SWM-2", "SWM-3", "SWM-4"])
    );
}

#[trace("FR-010-AC-10")]
#[test]
fn each_exclusion_kind_removes_its_own_closure_within_the_team() {
    let mut other_team = open("AGE-1", "p1");
    other_team.team_key = TeamKey("AGE".into());
    let snapshot = snapshot(
        vec![
            with_milestone(project("p1", &["ini"]), "m1", "Beta"),
            project("p2", &["ini"]),
        ],
        vec![
            in_milestone(open("SWM-1", "p1"), "m1"),
            open("SWM-2", "p1"),
            open("SWM-3", "p2"),
            open("SWM-10", "p1"),
            child_of(open("SWM-11", "p2"), "SWM-10"),
            child_of(open("SWM-12", "p1"), "SWM-10"),
            other_team,
        ],
    );
    let both = || vec![proj("p1"), proj("p2")];
    let check =
        |exclude: Vec<NamedMember>| leaves(&one(&snapshot, selection("SWM", both(), exclude)));
    assert_eq!(
        check(vec![]),
        want(&["SWM-1", "SWM-2", "SWM-3", "SWM-11", "SWM-12"])
    );
    assert_eq!(check(vec![init("ini")]), want(&[]));
    assert_eq!(check(vec![proj("p1")]), want(&["SWM-3", "SWM-11"]));
    assert_eq!(
        check(vec![mile("m1", "p1")]),
        want(&["SWM-2", "SWM-3", "SWM-11", "SWM-12"])
    );
    // A ticket exclusion removes its descendants, including those in another project.
    assert_eq!(
        check(vec![tick("SWM-10")]),
        want(&["SWM-1", "SWM-2", "SWM-3"])
    );
    // An exclusion is limited to the selection's team: the other team's ticket is
    // reported, removes nothing, and leaves the selection incomplete.
    let limited = one(&snapshot, selection("SWM", both(), vec![tick("AGE-1")]));
    assert_eq!(
        leaves(&limited),
        want(&["SWM-1", "SWM-2", "SWM-3", "SWM-11", "SWM-12"])
    );
    assert_eq!(
        outcome(&limited, ListKind::Exclude, &tick("AGE-1")),
        MemberOutcome::WrongTeam
    );
    assert_eq!(limited.state, SelectionState::Incomplete);
}

#[trace("FR-010-AC-11")]
#[test]
fn exclusion_is_local_to_its_selection_and_same_team_selections_union_once() {
    let snapshot = snapshot(
        vec![project("p1", &[]), project("p2", &[])],
        vec![
            open("SWM-1", "p1"),
            open("SWM-2", "p1"),
            open("SWM-3", "p2"),
        ],
    );
    let batch = evaluate(
        &snapshot,
        vec![
            request("a", selection("SWM", vec![proj("p1")], vec![])),
            request("b", selection("SWM", vec![proj("p1")], vec![tick("SWM-2")])),
        ],
    );
    assert_eq!(leaves(&batch.selections[0]), want(&["SWM-1", "SWM-2"]));
    assert_eq!(leaves(&batch.selections[1]), want(&["SWM-1"]));
    assert_eq!(batch.teams.len(), 1);
    assert_eq!(batch.teams[0].counts.total, 2, "SWM-2 is counted once");
    // B's exclusion of P1 removes nothing from A.
    let batch = evaluate(
        &snapshot,
        vec![
            request("a", selection("SWM", vec![proj("p1")], vec![])),
            request("b", selection("SWM", vec![proj("p2")], vec![proj("p1")])),
        ],
    );
    assert_eq!(leaves(&batch.selections[0]), want(&["SWM-1", "SWM-2"]));
    assert_eq!(leaves(&batch.selections[1]), want(&["SWM-3"]));
    let union = batch.teams[0]
        .leaf_ids
        .iter()
        .map(|id| id.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(union, ["SWM-1", "SWM-2", "SWM-3"]);
}

#[trace("FR-010-AC-12")]
#[test]
fn parents_stay_non_leaves_whatever_children_are_excluded() {
    let snapshot = snapshot(
        vec![project("p1", &[])],
        vec![
            open("SWM-1", "p1"),
            child_of(open("SWM-2", "p1"), "SWM-1"),
            child_of(open("SWM-3", "p1"), "SWM-1"),
        ],
    );
    let run = |include: Vec<NamedMember>, exclude: Vec<NamedMember>| {
        leaves(&one(&snapshot, selection("SWM", include, exclude)))
    };
    // Excluding one child never promotes the parent to a leaf.
    assert_eq!(run(vec![proj("p1")], vec![tick("SWM-2")]), want(&["SWM-3"]));
    // Excluding the parent removes both children although the selection names them.
    assert_eq!(
        run(vec![tick("SWM-2"), tick("SWM-3")], vec![tick("SWM-1")]),
        want(&[])
    );
    // A selected ticket whose descendants are all excluded is not turned into a leaf.
    assert_eq!(
        run(vec![tick("SWM-1")], vec![tick("SWM-2"), tick("SWM-3")]),
        want(&[])
    );
}

struct Fixture(WorkspaceSnapshot);

impl WorkspaceSource for Fixture {
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        Ok(self.0.clone())
    }
}

#[trace("FR-010-AC-13", "FR-012-AC-17")]
#[test]
fn explicit_empty_selection_and_no_active_selection_are_distinct_states() {
    let snapshot = two_project_initiative();
    for team in ["SWM", "NOPE"] {
        for exclude in [vec![], vec![proj("p1")]] {
            let summary = one(&snapshot, selection(team, vec![], exclude));
            assert_eq!(summary.state, SelectionState::Empty);
            assert!(summary.leaf_ids.is_empty());
            assert_eq!(summary.counts.total, 0);
            assert_eq!(summary.counts_availability, CountAvailability::Complete);
            assert!(summary.projects.is_empty() && summary.initiatives.is_empty());
        }
    }
    let none = evaluate(&snapshot, vec![]);
    assert_eq!(none.state, BatchState::None);
    assert!(none.selections.is_empty() && none.teams.is_empty());
    // A legacy ScopeSelection with only a team still selects the whole team.
    let whole = select_scope(
        &snapshot,
        &ScopeSelection {
            team: Some(TeamKey("SWM".into())),
            ..ScopeSelection::default()
        },
    )
    .expect("team selection");
    assert_eq!(whole.leaf_ids.len(), 4);
    // An empty inline FocusSelection is still rejected.
    let cache = tempfile::tempdir().expect("cache dir");
    let mut service =
        ReadService::open(Fixture(snapshot), &cache.path().join("c.sqlite3")).expect("service");
    assert!(matches!(
        service.focus_status(&FocusSelection::default()),
        Err(Error::EmptyFocus)
    ));
}

#[trace("FR-010-AC-14")]
#[test]
fn dynamic_membership_follows_each_snapshot_while_the_stored_bytes_stay_fixed() {
    let chosen = selection("SWM", vec![init("ini")], vec![]);
    let bytes = chosen.encode().expect("encodes");
    let with_p2 = two_project_initiative();
    let mut without_p2 = two_project_initiative();
    without_p2.projects[1].initiatives.clear();
    let joined = leaves(&one(&with_p2, chosen.clone()));
    let left = leaves(&one(&without_p2, chosen.clone()));
    assert_eq!(joined, want(&["SWM-1", "SWM-2", "SWM-3", "SWM-4"]));
    assert_eq!(left, want(&["SWM-1", "SWM-2"]));
    assert_eq!(leaves(&one(&with_p2, chosen.clone())), joined);
    assert_eq!(chosen.encode().expect("encodes"), bytes);
    assert!(
        !bytes.contains("SWM-"),
        "only ticket members may name issues: {bytes}"
    );
}

#[trace("FR-010-AC-15", "FR-010-AC-16", "FR-010-AC-17", "FR-010-AC-18")]
#[test]
fn member_outcomes_need_authoritative_evidence_to_be_missing() {
    let mut other_team = open("AGE-5", "p1");
    other_team.team_key = TeamKey("AGE".into());
    let snapshot = snapshot(
        vec![
            with_milestone(project("p1", &["ini"]), "m1", "Beta"),
            with_milestone(project("p2", &[]), "m2", "Beta"),
        ],
        vec![
            in_milestone(open("SWM-1", "p1"), "m1"),
            in_milestone(open("SWM-3", "p2"), "m2"),
            other_team,
        ],
    );
    let include = vec![
        proj("p1"),
        tick("AGE-5"),
        mile("m1", "p2"),
        proj("gone"),
        tick("SWM-404"),
        init("nope"),
        mile("m-nope", "p1"),
    ];
    let summary = one(&snapshot, selection("SWM", include.clone(), vec![]));
    let expected = [
        (proj("p1"), MemberOutcome::Resolved),
        (tick("AGE-5"), MemberOutcome::WrongTeam),
        (mile("m1", "p2"), MemberOutcome::ProjectMismatch),
        (proj("gone"), MemberOutcome::Unavailable),
        (tick("SWM-404"), MemberOutcome::Unavailable),
        (init("nope"), MemberOutcome::Unavailable),
        (mile("m-nope", "p1"), MemberOutcome::Unavailable),
    ];
    for (member, wanted) in &expected {
        assert_eq!(outcome(&summary, ListKind::Include, member), *wanted);
    }
    // A mismatched milestone selects nothing and its same-named twin is not selected;
    // the other-team ticket and the project's other-team work are fenced out.
    assert_eq!(leaves(&summary), want(&["SWM-1"]));
    assert_eq!(summary.state, SelectionState::Incomplete);
    assert_eq!(summary.counts_availability, CountAvailability::Partial);
    let listed = summary
        .members
        .iter()
        .map(|report| (report.list, report.member.clone()))
        .collect::<Vec<_>>();
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(listed, sorted, "outcomes are listed in deterministic order");

    // With enumeration evidence for every kind, the absent records are proven missing,
    // and no outcome changes for the records that are present.
    let evidence = CatalogEvidence {
        initiatives: true,
        projects: true,
        milestones: true,
        tickets: true,
        initiative_links: false,
    };
    let proven = evaluate_named_batch(
        &snapshot,
        &evidence,
        &[request("k", selection("SWM", include, vec![]))],
    )
    .expect("evaluates")
    .selections
    .remove(0);
    for member in [
        proj("gone"),
        tick("SWM-404"),
        init("nope"),
        mile("m-nope", "p1"),
    ] {
        assert_eq!(
            outcome(&proven, ListKind::Include, &member),
            MemberOutcome::Missing
        );
    }
    assert_eq!(
        outcome(&proven, ListKind::Include, &tick("AGE-5")),
        MemberOutcome::WrongTeam
    );
    assert_eq!(
        outcome(&proven, ListKind::Include, &mile("m1", "p2")),
        MemberOutcome::ProjectMismatch
    );

    // An unresolved exclusion removes nothing, is reported, and leaves the selection
    // incomplete rather than silently ignored.
    let excluded = one(
        &snapshot,
        selection("SWM", vec![proj("p1")], vec![proj("gone")]),
    );
    assert_eq!(leaves(&excluded), want(&["SWM-1"]));
    assert_eq!(
        outcome(&excluded, ListKind::Exclude, &proj("gone")),
        MemberOutcome::Unavailable
    );
    assert_eq!(excluded.state, SelectionState::Incomplete);
}

#[trace("FR-010-AC-17")]
#[test]
fn a_team_that_appears_nowhere_is_unavailable_not_empty_or_healthy() {
    let snapshot = two_project_initiative();
    let batch = evaluate(
        &snapshot,
        vec![request("k", selection("ZZZ", vec![proj("p1")], vec![]))],
    );
    let summary = &batch.selections[0];
    assert_eq!(summary.state, SelectionState::Unavailable);
    assert!(!summary.team_known);
    assert_eq!(summary.counts_availability, CountAvailability::Unavailable);
    assert_eq!(batch.teams[0].state, SelectionState::Unavailable);
    // The legacy selector still refuses an unknown team.
    assert!(matches!(
        select_scope(
            &snapshot,
            &ScopeSelection {
                team: Some(TeamKey("ZZZ".into())),
                ..ScopeSelection::default()
            }
        ),
        Err(Error::ScopeSelection { kind: "team", .. })
    ));
}

#[trace("FR-010-AC-19", "FR-012-AC-15")]
#[test]
fn the_team_boundary_counts_only_the_selected_team_and_says_so() {
    let mut foreign = open("AGE-1", "p1");
    foreign.team_key = TeamKey("AGE".into());
    let snapshot = snapshot(
        vec![project("p1", &["ini"])],
        vec![open("SWM-1", "p1"), foreign],
    );
    let summary = one(&snapshot, selection("SWM", vec![init("ini")], vec![]));
    assert_eq!(leaves(&summary), want(&["SWM-1"]));
    assert_eq!(
        summary.initiatives[0].partial,
        [PartialReason::TeamBoundary]
    );
    assert_eq!(
        summary.initiatives[0].projects[0].partial,
        [PartialReason::TeamBoundary]
    );
    assert_eq!(
        outcome(
            &one(&snapshot, selection("SWM", vec![tick("AGE-1")], vec![])),
            ListKind::Include,
            &tick("AGE-1")
        ),
        MemberOutcome::WrongTeam
    );
}

#[trace("FR-012-AC-11", "FR-012-AC-14")]
#[test]
fn one_batch_is_one_snapshot_ordered_by_key_with_teams_kept_separate() {
    let mut age = open("AGE-1", "p2");
    age.team_key = TeamKey("AGE".into());
    let mut age_two = open("AGE-2", "p2");
    age_two.team_key = TeamKey("AGE".into());
    let mut p2 = project("p2", &[]);
    p2.team_keys = vec![TeamKey("AGE".into())];
    let first = WorkspaceSnapshot {
        fetched_at: 4_242,
        ..snapshot(
            vec![project("p1", &[]), p2],
            vec![open("SWM-1", "p1"), open("SWM-2", "p1"), age, age_two],
        )
    };
    let requests = || {
        vec![
            request("focus-1@rev-3", selection("SWM", vec![proj("p1")], vec![])),
            request(
                "focus-0@rev-1",
                selection("SWM", vec![tick("SWM-1")], vec![]),
            ),
            request("focus-2@rev-9", selection("AGE", vec![proj("p2")], vec![])),
        ]
    };
    let batch = evaluate(&first, requests());
    assert_eq!(batch.fetched_at, 4_242);
    let keys = batch
        .selections
        .iter()
        .map(|s| s.key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["focus-0@rev-1", "focus-1@rev-3", "focus-2@rev-9"]);
    assert_eq!(
        evaluate(&first, requests()),
        batch,
        "identical input, identical output"
    );
    let teams = batch
        .teams
        .iter()
        .map(|t| (t.team.0.as_str(), t.counts.total))
        .collect::<Vec<_>>();
    assert_eq!(
        teams,
        [("AGE", 2), ("SWM", 2)],
        "no cross-team total or union"
    );
    assert!(
        batch.teams[0]
            .leaf_ids
            .iter()
            .all(|id| id.0.starts_with("AGE"))
    );
    // A changed snapshot gives results that all come from that one snapshot.
    let mut second = first.clone();
    second.fetched_at = 9_999;
    second.issues.retain(|issue| issue.id.0 != "SWM-2");
    let changed = evaluate(&second, requests());
    assert_eq!(changed.fetched_at, 9_999);
    assert_eq!(changed.teams[1].counts.total, 1);
    assert!(matches!(
        evaluate_named_batch(
            &first,
            &CatalogEvidence::default(),
            &[
                request("dup", selection("SWM", vec![], vec![])),
                request("dup", selection("SWM", vec![], vec![])),
            ]
        ),
        Err(Error::DuplicateSelectionKey { .. })
    ));
}

#[trace("FR-012-AC-12")]
#[test]
fn counts_equal_the_existing_leaf_readiness_and_blocker_results() {
    let snapshot = snapshot(
        vec![project("p1", &[])],
        vec![
            open("SWM-1", "p1"),
            started("SWM-2", "p1"),
            done("SWM-3", "p1"),
            issue("SWM-4", Some("p1"), WorkflowStateType::Canceled, "Canceled"),
            issue(
                "SWM-5",
                Some("p1"),
                WorkflowStateType::Duplicate,
                "Duplicate",
            ),
            open("SWM-6", "p1"),
            child_of(open("SWM-7", "p1"), "SWM-6"),
            blocked_by(open("SWM-8", "p1"), &["SWM-2"]),
        ],
    );
    let all = one(&snapshot, selection("SWM", vec![proj("p1")], vec![]));
    let legacy = select_scope(
        &snapshot,
        &ScopeSelection {
            projects: vec!["p1".into()],
            ..ScopeSelection::default()
        },
    )
    .expect("legacy scope");
    assert_eq!(
        all.leaf_ids.iter().cloned().collect::<BTreeSet<_>>(),
        legacy.leaf_ids
    );
    assert_eq!(all.counts.total, legacy.leaf_ids.len());
    assert_eq!(all.counts.done, legacy.done);
    assert_eq!(all.counts.remaining_by_stage, legacy.remaining_by_stage);
    let ready_queue = ix_board::ready_from_snapshot(&snapshot, "SWM").expect("ready queue");
    let ready_in_leaves = ready_queue
        .iter()
        .filter(|item| all.leaf_ids.iter().any(|id| id.0 == item.identifier))
        .count();
    assert_eq!(all.counts.ready, ready_in_leaves);
    assert_eq!(all.counts.ready, 2, "SWM-1 and SWM-7; SWM-8 is blocked");
    assert_eq!(all.counts.blocked, 1, "only SWM-8 has an open prerequisite");
    // A second selection over the same project reports its own effective set.
    let narrower = one(
        &snapshot,
        selection("SWM", vec![proj("p1")], vec![tick("SWM-8"), tick("SWM-3")]),
    );
    assert_eq!(narrower.counts.total, all.counts.total - 2);
    assert_eq!(narrower.counts.done, 0);
    assert_eq!(narrower.counts.blocked, 0);
    assert_eq!(all.counts.done, 1);
}

#[trace("FR-012-AC-13")]
#[test]
fn shared_rows_keep_their_own_counts_and_no_total_is_a_sum_of_overlaps() {
    let snapshot = snapshot(
        vec![project("p1", &["ini1", "ini2"]), project("p2", &[])],
        vec![open("SWM-1", "p1"), open("SWM-2", "p2")],
    );
    let batch = evaluate(
        &snapshot,
        vec![
            request(
                "a",
                selection("SWM", vec![init("ini1"), init("ini2")], vec![]),
            ),
            request("b", selection("SWM", vec![proj("p1"), proj("p2")], vec![])),
        ],
    );
    let a = &batch.selections[0];
    assert_eq!(a.initiatives.len(), 2);
    let placements = a
        .initiatives
        .iter()
        .map(|row| &row.projects[0])
        .collect::<Vec<_>>();
    assert!(placements.iter().all(|row| row.shared));
    assert_eq!(placements[0].counts, placements[1].counts);
    assert_eq!(placements[0].counts.total, 1);
    assert_eq!(
        a.counts.total, 1,
        "the leaf under two initiatives counts once"
    );
    assert!(a.initiatives.iter().all(|row| !row.shared));
    let b = &batch.selections[1];
    assert!(standalone(b, "p1").shared);
    assert_eq!(standalone(b, "p1").counts, placements[0].counts);
    assert!(!standalone(b, "p2").shared);
    assert_eq!(batch.teams[0].counts.total, 2, "SWM-1 once, SWM-2 once");
}

#[trace("FR-012-AC-15")]
#[test]
fn partial_reasons_are_reported_alone_and_together_with_direct_ticket_rows() {
    let mut foreign = open("AGE-1", "p1");
    foreign.team_key = TeamKey("AGE".into());
    let snapshot = snapshot(
        vec![
            with_milestone(project("p1", &["ini"]), "m1", "Beta"),
            project("p2", &[]),
        ],
        vec![
            in_milestone(open("SWM-1", "p1"), "m1"),
            open("SWM-2", "p1"),
            foreign,
            open("SWM-3", "p2"),
            open("SWM-4", "p2"),
        ],
    );
    let partial = |include: Vec<NamedMember>, exclude: Vec<NamedMember>, id: &str| {
        standalone(&one(&snapshot, selection("SWM", include, exclude)), id)
            .partial
            .clone()
    };
    assert_eq!(
        partial(vec![mile("m1", "p1")], vec![], "p1"),
        [PartialReason::MilestoneOnly]
    );
    assert_eq!(
        partial(vec![tick("SWM-2")], vec![], "p1"),
        [PartialReason::TicketOnly]
    );
    assert_eq!(
        partial(vec![proj("p2")], vec![tick("SWM-4")], "p2"),
        [PartialReason::Exclusion]
    );
    assert_eq!(
        partial(vec![proj("p1")], vec![], "p1"),
        [PartialReason::TeamBoundary]
    );
    assert_eq!(
        partial(vec![proj("p1")], vec![tick("SWM-2")], "p1"),
        [PartialReason::Exclusion, PartialReason::TeamBoundary]
    );
    assert_eq!(
        partial(vec![mile("m1", "p1"), tick("SWM-2")], vec![], "p1"),
        [PartialReason::MilestoneOnly, PartialReason::TicketOnly]
    );
    // Initiative rows carry the same reasons for their projects.
    let by_initiative = one(
        &snapshot,
        selection("SWM", vec![init("ini")], vec![tick("SWM-2")]),
    );
    assert_eq!(
        by_initiative.initiatives[0].partial,
        [PartialReason::Exclusion, PartialReason::TeamBoundary]
    );
    // A ticket-only selection lists its ticket as a direct row, never as a project or
    // team selection; including the project removes the direct row.
    let ticket_only = one(&snapshot, selection("SWM", vec![tick("SWM-2")], vec![]));
    assert_eq!(leaves(&ticket_only), want(&["SWM-2"]));
    assert_eq!(ticket_only.tickets.len(), 1);
    assert_eq!(ticket_only.tickets[0].identifier, "SWM-2");
    assert_eq!(ticket_only.tickets[0].counts.total, 1);
    let covered = one(
        &snapshot,
        selection("SWM", vec![proj("p1"), tick("SWM-2")], vec![]),
    );
    assert!(covered.tickets.is_empty());
}

#[trace("FR-012-AC-16")]
#[test]
fn an_excluded_prerequisite_blocks_and_stays_in_the_closure_without_being_counted() {
    let mut foreign = started("AGE-9", "p2");
    foreign.team_key = TeamKey("AGE".into());
    let snapshot = snapshot(
        vec![project("p1", &[]), project("p2", &[])],
        vec![
            blocked_by(open("SWM-1", "p1"), &["SWM-2", "SWM-3", "AGE-9"]),
            started("SWM-2", "p1"),
            started("SWM-3", "p2"),
            foreign,
            blocked_by(open("SWM-5", "p1"), &["SWM-6"]),
            done("SWM-6", "p2"),
        ],
    );
    let summary = one(
        &snapshot,
        selection("SWM", vec![proj("p1")], vec![tick("SWM-2"), proj("p2")]),
    );
    assert_eq!(leaves(&summary), want(&["SWM-1", "SWM-5"]));
    assert_eq!(summary.counts.total, 2, "prerequisites are not members");
    assert_eq!(summary.counts.blocked, 1);
    // SWM-5 is ready: excluding or keeping a completed prerequisite changes nothing.
    assert_eq!(summary.counts.ready, 1);
    let closure = summary
        .blocker_layers
        .iter()
        .flatten()
        .map(|blocker| blocker.identifier.as_str())
        .collect::<BTreeSet<_>>();
    for prerequisite in ["SWM-2", "SWM-3", "AGE-9", "SWM-6"] {
        assert!(
            closure.contains(prerequisite),
            "{prerequisite} stays as evidence"
        );
    }
    let excluded_done = one(
        &snapshot,
        selection("SWM", vec![proj("p1"), proj("p2")], vec![tick("SWM-6")]),
    );
    assert_eq!(excluded_done.counts.ready, 1, "SWM-5 stays ready");
    assert_eq!(excluded_done.counts.blocked, 1, "only SWM-1 is blocked");
}

#[trace("FR-012-AC-18")]
#[test]
fn an_unknown_status_category_is_kept_and_never_reported_healthy() {
    let mut unknown = project("p2", &[]);
    unknown.status_category = ProjectStatusCategory::Unknown;
    let snapshot = snapshot(
        vec![project("p1", &[]), unknown],
        vec![open("SWM-1", "p1"), open("SWM-2", "p2")],
    );
    let batch = evaluate(
        &snapshot,
        vec![request(
            "k",
            selection("SWM", vec![proj("p1"), proj("p2")], vec![]),
        )],
    );
    let summary = &batch.selections[0];
    let row = standalone(summary, "p2");
    assert_eq!(row.status_category, ProjectStatusCategory::Unknown);
    assert!(
        row.unknown_facts
            .contains(&UnknownFact::ProjectStatusCategory)
    );
    assert!(
        !standalone(summary, "p1")
            .unknown_facts
            .contains(&UnknownFact::ProjectStatusCategory)
    );
    assert!(
        summary
            .unknown_facts
            .contains(&UnknownFact::ProjectStatusCategory)
    );
    // The union's provenance names the selection that depends on the unknown fact.
    assert_eq!(batch.teams[0].causes.len(), 1);
    assert_eq!(batch.teams[0].causes[0].key, "k");
}

#[trace("FR-012-AC-19")]
#[test]
fn snapshot_errors_fail_the_whole_batch_with_the_existing_error() {
    let healthy = selection("SWM", vec![proj("p2")], vec![]);
    let broken = || selection("SWM", vec![proj("p1")], vec![]);
    let run = |issues: Vec<Issue>| {
        let mut all = vec![open("SWM-9", "p2")];
        all.extend(issues);
        evaluate_named_batch(
            &snapshot(vec![project("p1", &[]), project("p2", &[])], all),
            &CatalogEvidence::default(),
            &[request("good", healthy.clone()), request("bad", broken())],
        )
    };
    assert!(matches!(
        run(vec![
            child_of(open("SWM-1", "p1"), "SWM-2"),
            child_of(open("SWM-2", "p1"), "SWM-1"),
        ]),
        Err(Error::ParentCycle { .. })
    ));
    assert!(matches!(
        run(vec![child_of(open("SWM-1", "p1"), "SWM-404")]),
        Err(Error::MissingParent { .. })
    ));
    assert!(matches!(
        run(vec![blocked_by(open("SWM-1", "p1"), &["SWM-404"])]),
        Err(Error::MissingDependency { .. })
    ));
    assert!(matches!(
        run(vec![
            blocked_by(open("SWM-1", "p1"), &["SWM-2"]),
            blocked_by(open("SWM-2", "p1"), &["SWM-1"]),
        ]),
        Err(Error::DependencyCycle { .. })
    ));
}

#[trace("FR-012-AC-21")]
#[test]
fn a_team_union_is_only_as_known_as_its_selections() {
    let snapshot = snapshot(
        vec![project("p1", &[]), project("p2", &[])],
        vec![open("SWM-1", "p1"), open("SWM-2", "p2")],
    );
    let complete = || selection("SWM", vec![proj("p1")], vec![]);
    let incomplete = || selection("SWM", vec![proj("p2")], vec![proj("gone")]);
    let empty = || selection("SWM", vec![], vec![]);
    let unavailable = || selection("SWM", vec![proj("gone")], vec![]);
    // Initiative links are established, so only the selections' own members add causes.
    let evidence = CatalogEvidence {
        initiative_links: true,
        ..CatalogEvidence::default()
    };
    let union_of = |selections: Vec<NamedSelection>| {
        let requests = selections
            .into_iter()
            .enumerate()
            .map(|(index, chosen)| request(&format!("s{index}"), chosen))
            .collect::<Vec<_>>();
        evaluate_named_batch(&snapshot, &evidence, &requests).expect("batch evaluates")
    };
    let mixed = union_of(vec![complete(), incomplete()]);
    let team = &mixed.teams[0];
    assert_eq!(mixed.selections[0].state, SelectionState::Complete);
    assert_eq!(team.state, SelectionState::Incomplete);
    assert_eq!(team.counts_availability, CountAvailability::Partial);
    assert_eq!(team.causes.len(), 1);
    assert_eq!(team.causes[0].key, "s1");
    assert_eq!(team.causes[0].members[0].list, ListKind::Exclude);
    assert_eq!(
        team.causes[0].members[0].outcome,
        MemberOutcome::Unavailable
    );
    assert_eq!(
        union_of(vec![empty(), empty()]).teams[0].state,
        SelectionState::Empty
    );
    assert_eq!(
        union_of(vec![complete(), empty()]).teams[0].state,
        SelectionState::Complete
    );
    assert_eq!(
        union_of(vec![incomplete(), empty()]).teams[0].state,
        SelectionState::Incomplete
    );
    assert_eq!(
        union_of(vec![unavailable(), empty()]).teams[0].state,
        SelectionState::Unavailable
    );
    // Another team with only complete selections stays complete.
    let mut age = open("AGE-1", "p1");
    age.team_key = TeamKey("AGE".into());
    let two_teams = evaluate(
        &{
            let mut with_age = snapshot.clone();
            with_age.issues.push(age);
            with_age
        },
        vec![
            request("a", incomplete()),
            request("b", selection("AGE", vec![tick("AGE-1")], vec![])),
        ],
    );
    assert_eq!(two_teams.teams[0].state, SelectionState::Complete);
    assert_eq!(two_teams.teams[1].state, SelectionState::Incomplete);
}

#[trace("FR-012-AC-22")]
#[test]
fn standalone_projects_report_a_link_status_and_honor_exclusions() {
    let mut empty_project = project("p2", &[]);
    empty_project.team_keys = vec![TeamKey("SWM".into())];
    let snapshot = snapshot(
        vec![project("p1", &[]), empty_project],
        vec![open("SWM-1", "p1")],
    );
    let included = selection("SWM", vec![proj("p1")], vec![]);
    let known = CatalogEvidence {
        initiative_links: true,
        ..CatalogEvidence::default()
    };
    let with = |evidence: &CatalogEvidence, selection: NamedSelection| {
        evaluate_named_batch(&snapshot, evidence, &[request("k", selection)])
            .expect("evaluates")
            .selections
            .remove(0)
    };
    let none_selected = with(&known, included.clone());
    let row = standalone(&none_selected, "p1");
    assert_eq!(row.initiative_link, Some(InitiativeLink::NoneSelected));
    assert_eq!(row.counts.total, 1);
    assert!(
        none_selected.tickets.is_empty(),
        "never counted as a direct ticket"
    );
    let unavailable = with(&CatalogEvidence::default(), included);
    let row = standalone(&unavailable, "p1");
    assert_eq!(row.initiative_link, Some(InitiativeLink::Unavailable));
    assert!(row.unknown_facts.contains(&UnknownFact::InitiativeLinks));
    assert_eq!(row.counts.total, 1);
    // Excluded and included together: no standalone row.
    let both = with(&known, selection("SWM", vec![proj("p1")], vec![proj("p1")]));
    assert!(both.projects.is_empty() && both.initiatives.is_empty());
    // A directly included, known-empty project keeps its row with zero counts.
    let known_empty = with(&known, selection("SWM", vec![proj("p2")], vec![]));
    assert_eq!(standalone(&known_empty, "p2").counts.total, 0);
    // Direct-ticket placement is unchanged.
    let ticket = with(&known, selection("SWM", vec![tick("SWM-1")], vec![]));
    assert_eq!(ticket.tickets.len(), 1);
    assert_eq!(
        standalone(&ticket, "p1").partial,
        [PartialReason::TicketOnly]
    );
}

#[trace("FR-010-AC-20")]
#[test]
fn legacy_scope_selection_is_unchanged_beside_the_named_selection() {
    let snapshot = two_project_initiative();
    let legacy = select_scope(
        &snapshot,
        &ScopeSelection {
            initiatives: vec!["ini".into()],
            ..ScopeSelection::default()
        },
    )
    .expect("legacy scope");
    assert_eq!(legacy.issue_ids.len(), 4);
    assert_eq!(legacy.leaf_ids.len(), 4);
    assert_eq!(legacy.project_ids.len(), 2);
}

#[trace("FR-010-AC-17", "FR-010-AC-18", "FR-012-AC-17", "FR-012-AC-21")]
#[test]
fn unavailable_initiative_links_contaminate_initiative_includes_and_exclusions() {
    // Initiative `ini` is known to hold P1. P2 also holds team work, and nothing in the
    // snapshot says whether P2 is linked to `ini` as well.
    let snapshot = snapshot(
        vec![project("p1", &["ini"]), project("p2", &[])],
        vec![open("SWM-1", "p1"), open("SWM-2", "p2")],
    );
    let unknown = CatalogEvidence::default();
    let established = CatalogEvidence {
        initiative_links: true,
        ..CatalogEvidence::default()
    };
    let batch = |evidence: &CatalogEvidence, selections: Vec<NamedSelection>| {
        let requests = selections
            .into_iter()
            .enumerate()
            .map(|(index, chosen)| request(&format!("s{index}"), chosen))
            .collect::<Vec<_>>();
        evaluate_named_batch(&snapshot, evidence, &requests).expect("batch evaluates")
    };
    let including = || selection("SWM", vec![init("ini")], vec![]);
    let excluding = || selection("SWM", vec![proj("p1"), proj("p2")], vec![init("ini")]);
    let companion = || selection("SWM", vec![proj("p1")], vec![]);

    // Include: the known leaf stays, but P2's links could hide more leaves of `ini`.
    let hidden = batch(&unknown, vec![including(), companion()]);
    let (included, plain) = (&hidden.selections[0], &hidden.selections[1]);
    assert_eq!(leaves(included), want(&["SWM-1"]), "known leaves are kept");
    assert_eq!(included.initiatives.len(), 1, "the known placement is kept");
    assert_eq!(included.state, SelectionState::Incomplete);
    assert_eq!(included.counts_availability, CountAvailability::Partial);
    assert!(
        included
            .unknown_facts
            .contains(&UnknownFact::InitiativeLinks)
    );
    // The companion selection names no initiative, so its own result stays complete.
    assert_eq!(plain.state, SelectionState::Complete);
    assert_eq!(plain.counts_availability, CountAvailability::Complete);
    // The team union depends on the uncertain selection and names it as a cause.
    let team = &hidden.teams[0];
    assert_eq!(team.state, SelectionState::Incomplete);
    assert_eq!(team.counts_availability, CountAvailability::Partial);
    let cause = team
        .causes
        .iter()
        .find(|cause| cause.key == "s0")
        .expect("the initiative selection is a cause");
    assert_eq!(cause.state, SelectionState::Incomplete);
    assert!(cause.unknown_facts.contains(&UnknownFact::InitiativeLinks));

    // Exclude: P2 may belong to `ini` and so may already be excluded.
    let hidden = batch(&unknown, vec![excluding(), companion()]);
    let excluded = &hidden.selections[0];
    assert_eq!(leaves(excluded), want(&["SWM-2"]), "known leaves are kept");
    assert_eq!(excluded.state, SelectionState::Incomplete);
    assert_eq!(excluded.counts_availability, CountAvailability::Partial);
    assert!(
        excluded
            .unknown_facts
            .contains(&UnknownFact::InitiativeLinks)
    );
    assert_eq!(hidden.selections[1].state, SelectionState::Complete);
    assert_eq!(hidden.teams[0].state, SelectionState::Incomplete);
    assert!(hidden.teams[0].causes.iter().any(|cause| cause.key == "s0"));

    // With the links established the same selections are complete and no cause remains.
    let known = batch(&established, vec![including(), excluding(), companion()]);
    assert!(
        known
            .selections
            .iter()
            .all(|s| s.state == SelectionState::Complete && s.unknown_facts.is_empty())
    );
    assert_eq!(known.teams[0].state, SelectionState::Complete);
    assert!(known.teams[0].causes.is_empty());
    assert_eq!(leaves(&known.selections[0]), want(&["SWM-1"]));
    assert_eq!(leaves(&known.selections[1]), want(&["SWM-2"]));
}
