// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use assert_cmd::Command;
use ix_board::{
    Clock, Error, Issue, IssueId, Milestone, NextStep, Project, ProjectChunk, ProjectId,
    ReadService, SearchKind, TeamKey, WorkspaceSnapshot, WorkspaceSource,
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
}

impl FixtureSource {
    fn new(snapshot: WorkspaceSnapshot) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(snapshot)),
            reads: Arc::new(AtomicUsize::new(0)),
            fail: Arc::new(AtomicBool::new(false)),
        }
    }

    fn replace(&self, snapshot: WorkspaceSnapshot) {
        *self.snapshot.lock().expect("fixture mutex") = snapshot;
    }
}

impl WorkspaceSource for FixtureSource {
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
        id: ProjectId(id.to_owned()),
        identifier: Some(identifier.to_owned()),
        name: format!("Project {identifier}"),
        description: format!("Goal {identifier}"),
        content: Some("Project context".to_owned()),
        status: status.to_owned(),
        priority,
        sort_order,
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
        id: IssueId(id.to_owned()),
        identifier: identifier.to_owned(),
        title: format!("Title {identifier}"),
        description: Some(format!("Description {identifier}")),
        state: spec_state.unwrap_or(state).to_owned(),
        team_key: TeamKey(
            identifier
                .split_once('-')
                .map_or(identifier, |(key, _)| key)
                .to_owned(),
        ),
        project_id: project_id.map(|id| ProjectId(id.to_owned())),
        priority,
        sort_order,
        blocked_by: blocked_by
            .iter()
            .map(|id| IssueId((*id).to_owned()))
            .collect(),
    }
}

fn with_description(mut value: Issue, description: &str) -> Issue {
    value.description = Some(description.to_owned());
    value
}

fn queue_fixture() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
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
}

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
        },
        Milestone {
            id: "m1".to_owned(),
            name: "First".to_owned(),
            description: Some("Milestone description".to_owned()),
            sort_order: 1.0,
        },
    ];
    let mut reads = service(
        FixtureSource::new(snapshot),
        ManualClock::new(2_000),
        &directory,
    );

    let chunk: ProjectChunk = reads.project_chunk("swm-p1").expect("project chunk");
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

#[test]
fn cache_reuses_fresh_snapshot_and_refreshes_after_five_minutes() {
    let directory = TempDir::new().expect("temporary directory");
    let source = FixtureSource::new(queue_fixture());
    let clock = ManualClock::new(10_000);
    let mut reads = service(source.clone(), clock.clone(), &directory);

    let initial = reads.search("Title SWM-1").expect("initial read");
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    source.replace(WorkspaceSnapshot {
        projects: queue_fixture().projects,
        issues: vec![Issue {
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
    clock.set(10_000 + ix_board::CACHE_TTL_SECONDS);
    let refreshed = reads.search("changed").expect("expired cache refresh");
    assert_eq!(refreshed.len(), 1);
    assert_ne!(initial, refreshed);
    assert_eq!(source.reads.load(Ordering::SeqCst), 2);
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

    let invalid = Command::cargo_bin("ix-board")
        .expect("built binary")
        .args(["search", " "])
        .output()
        .expect("empty search");
    assert_eq!(invalid.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("search query must not be empty"));
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
