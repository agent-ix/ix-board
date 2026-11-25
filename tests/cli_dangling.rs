// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The `ix-board` binary over seeded caches: warning output and diagnostics (FR-046) and
//! the roadmap's cached leaf counts (FR-020). Every network route is closed, so a read
//! that misses a cache fails.

use assert_cmd::Command;
use ix_board::linear_summary::{InitiativeSummary, ProjectSummary, SummarySnapshot};
use ix_board::{
    Clock, Error, Initiative, Issue, IssueId, LinearGraphQlSource, Project, ProjectId,
    ProjectStatusCategory, ReadService, SystemClock, TeamKey, WorkflowStateType, WorkspaceSnapshot,
    WorkspaceSource,
};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use serde_json::Value;
use std::ffi::OsString;
use std::path::PathBuf;
use tempfile::TempDir;

const KEY: &str = "cli-cache-fixture";

fn now() -> i64 {
    SystemClock.now_epoch_seconds()
}

/// The cache partition the binary reads when it runs with [`KEY`].
fn namespace() -> String {
    LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from(KEY)),
        || None,
        "https://api.linear.app/graphql",
    )
    .expect("fixture source")
    .cache_namespace()
}

struct Seed(WorkspaceSnapshot);

impl WorkspaceSource for Seed {
    fn cache_namespace(&self) -> String {
        namespace()
    }

    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        Ok(self.0.clone())
    }
}

struct Cache {
    directory: TempDir,
}

impl Cache {
    fn new() -> Self {
        Self {
            directory: TempDir::new().expect("temporary directory"),
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.path().join("linear.sqlite3")
    }

    fn seed_detail(&self, snapshot: WorkspaceSnapshot) {
        ReadService::open(Seed(snapshot), &self.path())
            .expect("cache")
            .workspace_snapshot()
            .expect("seeded detail snapshot");
    }

    fn seed_summary(&self, summary: &SummarySnapshot) {
        let database = rusqlite::Connection::open(self.path()).expect("cache");
        database
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS summary_snapshot (cache_key TEXT PRIMARY KEY, \
                 payload BLOB NOT NULL, synced_at INTEGER NOT NULL, full_at INTEGER NOT NULL)",
            )
            .expect("summary table");
        database
            .execute(
                "INSERT INTO summary_snapshot VALUES(?1,?2,?3,?3)",
                rusqlite::params![
                    format!("{}:summary:v2", namespace()),
                    serde_json::to_vec(summary).expect("summary payload"),
                    now()
                ],
            )
            .expect("seeded summary");
    }

    fn run(&self, arguments: &[&str]) -> (String, String) {
        let output = Command::cargo_bin("ix-board")
            .expect("built binary")
            .args(arguments)
            .env("IX_BOARD_CACHE_PATH", self.path())
            .env("IX_BOARD_CACHE_TTL_SECONDS", "3600")
            .env("LINEAR_API_KEY", KEY)
            .env("XDG_CONFIG_HOME", self.directory.path())
            .env_remove("IX_BOARD_CONFIG_PATH")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env_remove("NO_PROXY")
            .output()
            .expect("CLI run");
        let text = |bytes| String::from_utf8(bytes).expect("UTF-8 output");
        let (stdout, stderr) = (text(output.stdout), text(output.stderr));
        assert!(output.status.success(), "{arguments:?}: {stderr}");
        (stdout, stderr)
    }
}

fn project() -> Project {
    Project {
        id: ProjectId("p1".to_owned()),
        identifier: None,
        name: "Ledger".to_owned(),
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
        milestones: Vec::new(),
        initiatives: Vec::new(),
    }
}

fn issue(id: &str, blockers: &[&str]) -> Issue {
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
        project_id: Some(ProjectId("p1".to_owned())),
        project_milestone_id: None,
        priority: 2.0,
        sort_order: 0.0,
        blocked_by: blockers
            .iter()
            .map(|id| IssueId((*id).to_owned()))
            .collect(),
        created_at: None,
        updated_at: None,
        attachment_urls: Vec::new(),
        attachments_complete: true,
        started_at: None,
        completed_at: None,
        pull_requests: Vec::new(),
    }
}

fn snapshot(issues: Vec<Issue>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        projects: vec![project()],
        issues,
        fetched_at: now(),
    }
}

/// Warnings whose snapshot order is not their output order, with a blocker listed twice.
fn disordered() -> WorkspaceSnapshot {
    let mut milestone = issue("SWM-9", &[]);
    milestone.project_milestone_id = Some("gone-milestone".to_owned());
    let mut parent = issue("SWM-5", &[]);
    parent.parent_id = Some(IssueId("gone-parent".to_owned()));
    snapshot(vec![
        issue("SWM-8", &["gone-b", "gone-b"]),
        parent,
        issue("SWM-2", &["gone-a"]),
        milestone,
    ])
}

#[trace("FR-046-AC-6")]
#[test]
fn terminal_warnings_are_sorted_and_listed_once() {
    let cache = Cache::new();
    cache.seed_detail(disordered());
    let (stdout, _) = cache.run(&["focus", "status", "--project", "p1"]);
    let (_, warnings) = stdout.split_once("Warnings:\n").expect("warnings section");
    assert_eq!(
        warnings.trim_end().lines().collect::<Vec<_>>(),
        [
            "  issue SWM-9 references unavailable milestone gone-milestone in project p1",
            "  issue SWM-5 references unavailable parent gone-parent",
            "  issue SWM-2 references unavailable blocker gone-a",
            "  issue SWM-8 references unavailable blocker gone-b",
        ]
    );
}

#[trace("FR-046-AC-10")]
#[test]
fn next_keeps_its_json_list_and_reports_warnings_in_the_diagnostics_format() {
    let cache = Cache::new();
    cache.seed_detail(snapshot(vec![
        issue("SWM-1", &[]),
        issue("SWM-2", &["gone"]),
    ]));
    let (stdout, stderr) = cache.run(&[
        "--json",
        "--diagnostics-format",
        "json",
        "next",
        "--team",
        "SWM",
    ]);
    let queue: Value = serde_json::from_str(&stdout).expect("queue JSON");
    assert_eq!(queue.as_array().expect("bare list").len(), 1);
    assert_eq!(queue[0]["identifier"], "SWM-1");
    let records = stderr
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("structured diagnostic"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1, "{stderr}");
    let record = records[0].to_string();
    assert!(record.contains("unavailable_blocker"), "{record}");
    assert!(
        record.contains("issue SWM-2 references unavailable blocker gone"),
        "{record}"
    );
}

fn summary() -> SummarySnapshot {
    let initiative = Initiative {
        id: "i1".to_owned(),
        identifier: "slug-i1".to_owned(),
        name: "Hardening".to_owned(),
    };
    let mut member = project();
    member.identifier = Some("slug-p1".to_owned());
    member.initiatives = vec![initiative.clone()];
    SummarySnapshot {
        projects: vec![ProjectSummary {
            project: member,
            progress: Some(0.5),
            updated_at: "2026-11-20T00:00:00Z".to_owned(),
            archived: false,
            trashed: false,
        }],
        initiatives: vec![InitiativeSummary {
            initiative,
            status: "Active".to_owned(),
            health: None,
            updated_at: "2026-11-20T00:00:00Z".to_owned(),
            archived: false,
            trashed: false,
            project_ids: vec![ProjectId("p1".to_owned())],
        }],
    }
}

fn roadmap_row(cache: &Cache) -> Value {
    let (stdout, _) = cache.run(&["--json", "roadmap"]);
    let report: Value = serde_json::from_str(&stdout).expect("roadmap JSON");
    // A fresh cache is printed without the freshness wrapper.
    let report = report.get("result").unwrap_or(&report);
    report["initiatives"][0]["projects"][0].clone()
}

#[trace("FR-020-AC-8")]
#[test]
fn the_roadmap_command_counts_from_the_cached_snapshot_or_says_why_not() {
    let cache = Cache::new();
    cache.seed_summary(&summary());
    let row = roadmap_row(&cache);
    assert!(row["total"].is_null() && row["done"].is_null());
    assert!(
        row["counts_unavailable_reason"]
            .as_str()
            .expect("reason")
            .starts_with("no cached issue snapshot"),
        "{row}"
    );

    let mut done = issue("SWM-2", &[]);
    done.state_type = WorkflowStateType::Completed;
    cache.seed_detail(snapshot(vec![issue("SWM-1", &[]), done]));
    let row = roadmap_row(&cache);
    assert_eq!((&row["done"], &row["total"]), (&1.into(), &2.into()));
    assert_eq!(row["counts_source"], "ix_board_cached_issue_leaf_tasks");
    let (text, _) = cache.run(&["roadmap"]);
    assert!(text.contains("leaf tasks done 1/2, open 1"), "{text}");

    rusqlite::Connection::open(cache.path())
        .expect("cache")
        .execute("UPDATE workspace_snapshot SET payload=X'00'", [])
        .expect("damaged payload");
    let row = roadmap_row(&cache);
    assert!(row["total"].is_null());
    assert!(
        row["counts_unavailable_reason"]
            .as_str()
            .expect("reason")
            .starts_with("cached issue snapshot is unreadable: "),
        "{row}"
    );
}
