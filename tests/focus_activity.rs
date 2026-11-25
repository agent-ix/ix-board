// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use ix_board::{
    Clock, Error, FocusActivityKind, FocusSelection, FocusStatus, Issue, IssueId, Milestone,
    Project, ProjectId, PullRequestFact, PullRequestStatus, ReadService, TeamKey,
    WorkflowStateType, WorkspaceSnapshot, WorkspaceSource,
};
use tempfile::TempDir;

/// 2026-11-25T12:00:00Z.
const NOW: i64 = 1_795_608_000;

struct FixedClock;

impl Clock for FixedClock {
    fn now_epoch_seconds(&self) -> i64 {
        NOW
    }
}

struct FixtureSource(WorkspaceSnapshot);

impl WorkspaceSource for FixtureSource {
    fn cache_namespace(&self) -> String {
        "focus-activity".to_owned()
    }

    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        Ok(self.0.clone())
    }
}

fn rfc3339(hours_ago: f64) -> String {
    let seconds = NOW - (hours_ago * 3600.0) as i64;
    chrono::DateTime::from_timestamp(seconds, 0)
        .expect("timestamp in range")
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn project(milestones: Vec<Milestone>) -> Project {
    Project {
        initiatives: Vec::new(),
        id: ProjectId("p1".to_owned()),
        identifier: Some("SWM-P1".to_owned()),
        name: "Delivery".to_owned(),
        updated_at: None,
        description: String::new(),
        content: None,
        status: "Started".to_owned(),
        status_name: None,
        status_category: Default::default(),
        priority: 2,
        sort_order: 1.0,
        target_date: None,
        team_keys: vec![TeamKey("SWM".to_owned())],
        milestones,
    }
}

fn milestone() -> Milestone {
    Milestone {
        id: "m1".to_owned(),
        name: "First".to_owned(),
        description: None,
        sort_order: 1.0,
        target_date: None,
    }
}

fn issue(number: usize, completed_hours_ago: Option<f64>) -> Issue {
    let completed = completed_hours_ago.is_some();
    Issue {
        id: IssueId(format!("i{number}")),
        identifier: format!("SWM-{number}"),
        title: format!("Title {number}"),
        description: None,
        state: if completed {
            "Completed"
        } else {
            "In Progress"
        }
        .to_owned(),
        state_name: if completed { "Done" } else { "In Progress" }.to_owned(),
        state_type: if completed {
            WorkflowStateType::Completed
        } else {
            WorkflowStateType::Started
        },
        parent_id: None,
        team_key: TeamKey("SWM".to_owned()),
        assignee_id: None,
        assignee_name: Some("Ada".to_owned()),
        project_id: Some(ProjectId("p1".to_owned())),
        project_milestone_id: None,
        priority: 1.0,
        sort_order: number as f64,
        blocked_by: Vec::new(),
        created_at: None,
        updated_at: None,
        attachment_urls: Vec::new(),
        attachments_complete: true,
        started_at: Some(rfc3339(100.0)),
        completed_at: completed_hours_ago.map(rfc3339),
        pull_requests: Vec::new(),
    }
}

fn pull_request(
    number: u64,
    merged_hours_ago: Option<f64>,
    opened_hours_ago: f64,
) -> PullRequestFact {
    PullRequestFact {
        url: format!("https://github.com/agent-ix/repo/pull/{number}"),
        repo: "repo".to_owned(),
        number,
        status: if merged_hours_ago.is_some() {
            PullRequestStatus::Merged
        } else {
            PullRequestStatus::Open
        },
        draft: false,
        created_at: Some(rfc3339(opened_hours_ago)),
        merged_at: merged_hours_ago.map(rfc3339),
    }
}

fn status_with(
    projects: Vec<Project>,
    issues: Vec<Issue>,
    closed_since_seconds: u64,
) -> FocusStatus {
    let directory = TempDir::new().expect("temporary directory");
    let snapshot = WorkspaceSnapshot {
        fetched_at: NOW,
        projects,
        issues,
    };
    let mut service = ReadService::with_clock(
        FixtureSource(snapshot),
        &directory.path().join("cache.sqlite"),
        FixedClock,
    )
    .expect("open service");
    service
        .focus_status_windowed(
            &FocusSelection {
                projects: vec!["SWM-P1".to_owned()],
                ..Default::default()
            },
            &[],
            closed_since_seconds,
        )
        .expect("focus status")
}

const HOURS_48: u64 = 48 * 3600;

/// Trace: FR-045-AC-1, FR-045-AC-2
#[test]
fn recently_closed_keeps_completed_issues_inside_the_window_newest_first() {
    let mut in_milestone = issue(2, Some(30.0));
    in_milestone.project_milestone_id = Some("m1".to_owned());
    let issues = vec![
        issue(1, Some(47.0)),
        in_milestone,
        issue(3, Some(1.0)),
        issue(4, Some(49.0)),
        issue(5, None),
        {
            let mut canceled = issue(6, Some(2.0));
            canceled.state_type = WorkflowStateType::Canceled;
            canceled
        },
    ];
    let status = status_with(vec![project(vec![milestone()])], issues, HOURS_48);
    let project = &status.projects[0];
    let ids: Vec<_> = project
        .recently_closed
        .iter()
        .map(|issue| issue.identifier.as_str())
        .collect();
    assert_eq!(ids, ["SWM-3", "SWM-2", "SWM-1"]);
    let milestone: Vec<_> = project.milestones[0]
        .recently_closed
        .iter()
        .map(|issue| issue.identifier.as_str())
        .collect();
    assert_eq!(milestone, ["SWM-2"]);
    let unmilestoned: Vec<_> = project
        .unmilestoned
        .recently_closed
        .iter()
        .map(|issue| issue.identifier.as_str())
        .collect();
    assert_eq!(unmilestoned, ["SWM-3", "SWM-1"]);

    let narrow = status_with(
        vec![project_fixture()],
        vec![issue(1, Some(47.0)), issue(3, Some(1.0))],
        2 * 3600,
    );
    assert_eq!(narrow.projects[0].recently_closed.len(), 1);
}

fn project_fixture() -> Project {
    project(Vec::new())
}

/// Trace: FR-045-AC-3
#[test]
fn recently_closed_is_capped_at_fifty_and_activity_at_two_hundred() {
    let issues: Vec<_> = (1..=100)
        .map(|number| {
            let hours = 1.0 + number as f64 * 0.1;
            let mut issue = issue(number, Some(hours));
            issue.pull_requests = vec![pull_request(number as u64, Some(hours), hours + 1.0)];
            issue
        })
        .collect();
    let status = status_with(vec![project_fixture()], issues, HOURS_48);
    let closed = &status.projects[0].recently_closed;
    assert_eq!(closed.len(), 50);
    assert_eq!(closed[0].identifier, "SWM-1");
    assert_eq!(closed[49].identifier, "SWM-50");
    assert_eq!(status.activity.len(), 200);
    assert!(
        status
            .activity
            .windows(2)
            .all(|pair| pair[0].at >= pair[1].at)
    );
}

/// Trace: FR-045-AC-4, FR-045-AC-5
#[test]
fn activity_emits_merged_opened_and_closed_once_each_inside_the_window() {
    let mut done = issue(1, Some(3.0));
    let merged = pull_request(7, Some(4.0), 10.0);
    done.pull_requests = vec![merged.clone(), merged, pull_request(8, None, 60.0)];
    let mut open = issue(2, None);
    open.pull_requests = vec![
        pull_request(9, None, 2.0),
        pull_request(10, Some(70.0), 80.0),
    ];
    let status = status_with(vec![project_fixture()], vec![done, open], HOURS_48);

    let summary: Vec<_> = status
        .activity
        .iter()
        .map(|event| {
            (
                event.kind,
                event.identifier.as_str(),
                event.pr.as_ref().map(|pr| pr.number),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (FocusActivityKind::Opened, "SWM-2", Some(9)),
            (FocusActivityKind::Closed, "SWM-1", None),
            (FocusActivityKind::Merged, "SWM-1", Some(7)),
            (FocusActivityKind::Opened, "SWM-1", Some(7)),
        ]
    );
    assert!(
        status
            .activity
            .iter()
            .all(|event| event.project == "Delivery")
    );
    assert_eq!(status.activity[2].pr.as_ref().unwrap().repo, "repo");
}

/// Trace: FR-045-AC-6
#[test]
fn focus_issues_carry_assignee_lifecycle_times_and_pull_requests_as_json() {
    let mut done = issue(1, Some(3.0));
    done.pull_requests = vec![pull_request(7, Some(4.0), 10.0)];
    let status = status_with(
        vec![project_fixture()],
        vec![done, issue(2, None)],
        HOURS_48,
    );
    let value = serde_json::to_value(&status).expect("status json");
    let closed = &value["projects"][0]["recently_closed"][0];
    assert_eq!(closed["assignee"], "Ada");
    assert_eq!(closed["completed_at"], rfc3339(3.0));
    assert_eq!(closed["started_at"], rfc3339(100.0));
    let pr = &closed["prs"][0];
    assert_eq!(pr["status"], "merged");
    assert_eq!(pr["draft"], false);
    assert_eq!(pr["merged_at"], rfc3339(4.0));
    assert_eq!(pr["repo"], "repo");
    let in_progress = &value["projects"][0]["in_progress"][0];
    assert_eq!(in_progress["prs"], serde_json::json!([]));
    assert_eq!(in_progress["completed_at"], serde_json::Value::Null);
}

/// Trace: FR-045-AC-1
#[test]
fn a_timestamp_slightly_ahead_of_the_clock_is_still_inside_the_window() {
    let status = status_with(
        vec![project_fixture()],
        vec![issue(1, Some(-0.02)), issue(2, Some(-1.0))],
        HOURS_48,
    );
    let ids: Vec<_> = status.projects[0]
        .recently_closed
        .iter()
        .map(|issue| issue.identifier.as_str())
        .collect();
    assert_eq!(ids, ["SWM-1"]);
}
