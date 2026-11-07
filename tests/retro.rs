// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Acceptance tests for `ix-board retro snapshot` (FR-024, FR-025, FR-026). Every source is a
//! fixture behind the traits in `ix_board::retro::sources`; no test touches Linear, GitHub,
//! herdr or a relay. Stores live under the system temporary directory, outside any repository.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ix_board::agents::{AgentRow, AgentsReport, GitAges, Verdict};
use ix_board::retro::catalog::{Metric, RecordKind, definition};
use ix_board::retro::delta::{ChangeState, DeltaReport};
use ix_board::retro::sources::{
    AgentSource, CheckState, GateLine, GateLogSource, GithubSource, LabeledIssue, LinearSource,
    LocalSource, MainCommit, Page, ProcessRow, PullRequest, PullRequestInfo, RelaySource, Sources,
    StateChange, read_all_pages,
};
use ix_board::retro::{
    CATALOG, Record, RetroConfig, RetroError, Status, Store, StoreError, parse_window,
    run_backfill, run_delta, run_snapshot, unavailable_notice,
};
use ix_board::{
    Issue, IssueId, Project, ProjectId, TeamKey, WorkflowStateType as S, WorkspaceSnapshot,
};
use tempfile::TempDir;

const NOW: f64 = 1_794_052_800.0; // 2026-11-07T12:00:00Z
const HOUR: f64 = 3600.0;
const DAY: u64 = 86_400;

fn ago(hours: f64) -> f64 {
    NOW - hours * HOUR
}

fn iso(epoch: f64) -> String {
    let secs = epoch as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let doe = shifted - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// ---- fixture sources -------------------------------------------------------------------

#[derive(Default)]
struct Rig {
    workspace: Option<Result<WorkspaceSnapshot, String>>,
    project_counts: BTreeMap<String, usize>,
    histories: BTreeMap<String, Result<Vec<StateChange>, String>>,
    markers: Option<Result<Vec<String>, String>>,
    labeled: Option<Result<Vec<LabeledIssue>, String>>,
    prs: Option<Result<Vec<PullRequest>, String>>,
    merged: BTreeMap<(String, u64), bool>,
    infos: BTreeMap<(String, u64), PullRequestInfo>,
    commits: Option<Result<Vec<MainCommit>, String>>,
    agents: Option<Result<AgentsReport, String>>,
    gate_lines: Option<Result<Vec<GateLine>, String>>,
    relay: BTreeMap<String, Result<Vec<i64>, String>>,
    worktrees: Option<Result<u64, String>>,
    bytes: Option<Result<u64, String>>,
    processes: Option<Result<Vec<ProcessRow>, String>>,
    calls: RefCell<Vec<String>>,
    since: RefCell<Vec<f64>>,
}

impl Rig {
    fn log(&self, call: impl Into<String>) {
        self.calls.borrow_mut().push(call.into());
    }

    fn count(&self, call: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c.as_str() == call)
            .count()
    }

    fn sources(&self) -> Sources<'_> {
        Sources {
            linear: self,
            github: self,
            agents: self,
            gate_log: self,
            relay: self,
            local: self,
        }
    }
}

fn unset<T: Clone>(value: &Option<Result<T, String>>, what: &str) -> Result<T, String> {
    value
        .clone()
        .unwrap_or_else(|| Err(format!("{what} fixture not set")))
}

impl LinearSource for Rig {
    fn workspace(&self, since: f64) -> Result<WorkspaceSnapshot, String> {
        self.log("workspace");
        self.since.borrow_mut().push(since);
        unset(&self.workspace, "workspace")
    }
    fn project_open_count(&self, project: &str) -> Result<usize, String> {
        self.log("project_open_count");
        self.project_counts
            .get(project)
            .copied()
            .ok_or_else(|| "no chunk".to_owned())
    }
    fn state_histories(&self, issue_ids: &[String]) -> Vec<Result<Vec<StateChange>, String>> {
        self.log(format!("batch:{}", issue_ids.len()));
        issue_ids.iter().map(|id| self.state_history(id)).collect()
    }
    fn state_history(&self, issue_id: &str) -> Result<Vec<StateChange>, String> {
        self.log(format!("state_history:{issue_id}"));
        self.histories
            .get(issue_id)
            .cloned()
            .unwrap_or(Ok(Vec::new()))
    }
    fn marker_lines(&self, prefix: &str, needle: &str) -> Result<Vec<String>, String> {
        self.log("marker_lines");
        Ok(unset(&self.markers, "markers")?
            .into_iter()
            // The source filters whole comment bodies, as Linear does, and hands back only the
            // first line of each comment that passes.
            .filter(|body| body.starts_with(prefix) && body.contains(needle))
            .map(|body| ix_board::retro::metrics::first_line(&body).to_owned())
            .collect())
    }
    fn open_issues_with_label(&self, _label: &str) -> Result<Vec<LabeledIssue>, String> {
        self.log("open_issues_with_label");
        unset(&self.labeled, "labeled issues")
    }
}

impl GithubSource for Rig {
    fn pull_requests(&self, repo: &str, _since: f64) -> Result<Vec<PullRequest>, String> {
        self.log(format!("pull_requests:{repo}"));
        unset(&self.prs, "pull requests")
    }
    fn pull_request(&self, repo: &str, number: u64) -> Result<PullRequestInfo, String> {
        self.log(format!("pull_request:{repo}#{number}"));
        let key = (repo.to_owned(), number);
        if let Some(info) = self.infos.get(&key) {
            return Ok(*info);
        }
        if let Some(Ok(listing)) = &self.prs
            && let Some(pr) = listing
                .iter()
                .find(|pr| pr.repo == repo && pr.number == number)
        {
            return Ok(PullRequestInfo {
                merged: pr.merged_at.is_some(),
                updated_at: pr.updated_at,
            });
        }
        match self.merged.get(&key) {
            Some(merged) => Ok(PullRequestInfo {
                merged: *merged,
                updated_at: 0.0,
            }),
            None => Err("unknown pull request".to_owned()),
        }
    }
    fn main_commits(
        &self,
        _repo: &str,
        _branch: &str,
        _since: f64,
    ) -> Result<Vec<MainCommit>, String> {
        self.log("main_commits");
        unset(&self.commits, "commits")
    }
}

impl AgentSource for Rig {
    fn report(&self, _teams: &[String]) -> Result<AgentsReport, String> {
        self.log("agents");
        unset(&self.agents, "agents")
    }
}

impl GateLogSource for Rig {
    fn lines(&self, _path: &Path) -> Result<Vec<GateLine>, String> {
        self.log("gate_log");
        unset(&self.gate_lines, "gate log")
    }
}

impl RelaySource for Rig {
    fn pending_sent_at_ms(&self, address: &str) -> Result<Vec<i64>, String> {
        self.log("relay");
        self.relay
            .get(address)
            .cloned()
            .unwrap_or_else(|| Err("no queue".to_owned()))
    }
}

impl LocalSource for Rig {
    fn prunable_worktrees(&self, _dir: &Path) -> Result<u64, String> {
        self.log("prunable_worktrees");
        unset(&self.worktrees, "worktrees")
    }
    fn disk_bytes(&self, _dir: &Path) -> Result<u64, String> {
        self.log("disk_bytes");
        unset(&self.bytes, "bytes")
    }
    fn processes(&self) -> Result<Vec<ProcessRow>, String> {
        self.log("processes");
        unset(&self.processes, "processes")
    }
}

// ---- fixture data ----------------------------------------------------------------------

fn issue(
    identifier: &str,
    state: S,
    project: Option<&str>,
    created_h: f64,
    updated_h: f64,
) -> Issue {
    Issue {
        id: IssueId(format!("id-{identifier}")),
        identifier: identifier.to_owned(),
        title: identifier.to_owned(),
        description: None,
        state: format!("{state:?}"),
        state_name: format!("{state:?}"),
        state_type: state,
        parent_id: None,
        team_key: TeamKey("SWM".to_owned()),
        assignee_id: None,
        assignee_name: None,
        project_id: project.map(|id| ProjectId(id.to_owned())),
        project_milestone_id: Some("m1".to_owned()),
        priority: 0.0,
        sort_order: 0.0,
        blocked_by: Vec::new(),
        created_at: Some(iso(ago(created_h))),
        updated_at: Some(iso(ago(updated_h))),
        attachment_urls: Vec::new(),
        attachments_complete: true,
    }
}

fn blocked_by(mut issue: Issue, blockers: &[&str]) -> Issue {
    issue.blocked_by = blockers
        .iter()
        .map(|b| IssueId(format!("id-{b}")))
        .collect();
    issue
}

fn attached(mut issue: Issue, number: u64) -> Issue {
    issue
        .attachment_urls
        .push(format!("https://github.com/agent-ix/x/pull/{number}"));
    issue
}

fn no_milestone(mut issue: Issue) -> Issue {
    issue.project_milestone_id = None;
    issue
}

fn project(id: &str, name: &str) -> Project {
    Project {
        id: ProjectId(id.to_owned()),
        identifier: None,
        name: name.to_owned(),
        updated_at: None,
        description: String::new(),
        content: None,
        status: "Started".to_owned(),
        priority: 0,
        sort_order: 0.0,
        target_date: None,
        team_keys: vec![TeamKey("SWM".to_owned())],
        milestones: Vec::new(),
        initiatives: Vec::new(),
    }
}

fn change(hours_ago: f64, from: S, to: S) -> StateChange {
    StateChange {
        at: ago(hours_ago),
        from: Some(from),
        to: Some(to),
    }
}

/// The default world; the comments give each issue's role.
fn issues() -> Vec<Issue> {
    let mut other = issue("OTH-1", S::Unstarted, None, 10.0, 10.0);
    other.team_key = TeamKey("OTH".to_owned());
    vec![
        // gate
        blocked_by(
            issue("SWM-1", S::Started, Some("p1"), 500.0, 3.0),
            &["SWM-2", "SWM-3"],
        ),
        // path, completed in window, merged PR attached
        attached(
            blocked_by(
                issue("SWM-2", S::Completed, Some("p1"), 300.0, 5.0),
                &["SWM-4"],
            ),
            10,
        ),
        // path, open, attached PR updated recently
        attached(
            no_milestone(issue("SWM-3", S::Started, Some("p1"), 300.0, 60.0)),
            11,
        ),
        // path, completed in window, no PR attached
        issue("SWM-4", S::Completed, Some("p1"), 300.0, 10.0),
        // new in window, open
        issue("SWM-5", S::Unstarted, Some("p1"), 2.0, 2.0),
        // new in window, completed in window, not on the path
        issue("SWM-6", S::Completed, Some("p1"), 20.0, 3.0),
        // started 100h ago, no project, no PR: stale
        no_milestone(issue("SWM-7", S::Started, None, 200.0, 100.0)),
        // completed before the window
        issue("SWM-8", S::Completed, Some("p1"), 200.0, 100.0),
        // reopened in the window
        issue("SWM-9", S::Started, Some("p1"), 300.0, 4.0),
        other,
    ]
}

fn histories() -> BTreeMap<String, Result<Vec<StateChange>, String>> {
    let table: Vec<(&str, Vec<StateChange>)> = vec![
        ("SWM-1", vec![change(3.0, S::Unstarted, S::Started)]),
        ("SWM-2", vec![change(5.0, S::Started, S::Completed)]),
        ("SWM-3", vec![change(60.0, S::Unstarted, S::Started)]),
        ("SWM-4", vec![change(10.0, S::Started, S::Completed)]),
        ("SWM-6", vec![change(3.0, S::Started, S::Completed)]),
        ("SWM-7", vec![change(100.0, S::Unstarted, S::Started)]),
        ("SWM-8", vec![change(100.0, S::Started, S::Completed)]),
        ("SWM-9", vec![change(4.0, S::Completed, S::Started)]),
    ];
    table
        .into_iter()
        .map(|(id, h)| (format!("id-{id}"), Ok(h)))
        .collect()
}

fn pr(number: u64, open: bool) -> PullRequest {
    PullRequest {
        repo: "agent-ix/x".to_owned(),
        number,
        open,
        draft: false,
        created_at: ago(30.0),
        merged_at: None,
        updated_at: ago(1.0),
        additions: 10,
        deletions: 10,
        review_decision: None,
        force_pushes: Some(0),
    }
}

fn merged_pr(number: u64, cycle_hours: f64, merged_hours_ago: f64) -> PullRequest {
    PullRequest {
        merged_at: Some(ago(merged_hours_ago)),
        created_at: ago(merged_hours_ago) - cycle_hours * HOUR,
        updated_at: ago(merged_hours_ago),
        ..pr(number, false)
    }
}

fn default_prs() -> Vec<PullRequest> {
    vec![
        merged_pr(101, 1.0, 20.0),
        merged_pr(102, 2.0, 18.0),
        merged_pr(103, 3.0, 12.0),
        merged_pr(104, 100.0, 2.0),
        PullRequest {
            review_decision: Some("APPROVED".to_owned()),
            force_pushes: Some(2),
            ..pr(11, true)
        },
    ]
}

fn agent_row(verdict: Verdict, last_turn: Option<u64>) -> AgentRow {
    AgentRow {
        team: "SWM".to_owned(),
        machine: "local".to_owned(),
        pane_id: "w1:p1".to_owned(),
        agent: "claude".to_owned(),
        title: String::new(),
        cwd: String::new(),
        herdr_status: String::new(),
        verdict,
        partial: false,
        last_turn_age_secs: last_turn,
        subagent_age_secs: None,
        live_subagents: 0,
        processes: Vec::new(),
        git: GitAges::default(),
        excerpt: None,
        pane_tail: None,
        next_ready: None,
        notes: Vec::new(),
    }
}

fn agents_report(rows: Vec<AgentRow>) -> AgentsReport {
    AgentsReport {
        teams: vec!["SWM".to_owned()],
        idle_after_secs: 1200,
        agents: rows,
        missing_teams: Vec::new(),
        errors: Vec::new(),
        ready_queue_error: None,
    }
}

fn base_rig() -> Rig {
    Rig {
        workspace: Some(Ok(WorkspaceSnapshot {
            projects: vec![project("p1", "Alpha")],
            issues: issues(),
            fetched_at: 0,
        })),
        project_counts: BTreeMap::from([("Alpha".to_owned(), 4)]),
        histories: histories(),
        markers: Some(Ok(vec![
            "<!-- reviewer repo=agent-ix/x quoin=quoin 0.28.1 id=SR-1 method=code-review pr=x#101 -->".to_owned(),
            "<!-- reviewer repo=agent-ix/x id=SR-2 method=code-review pr=x#11 -->".to_owned(),
            "<!-- reviewer-dispositions repo=agent-ix/x id=SR-1 round=1 -->".to_owned(),
            "<!-- reviewer-dispositions repo=agent-ix/x id=SR-1 round=2 -->".to_owned(),
            "<!-- reviewer-dispositions repo=agent-ix/x id=SR-2 round=1 -->".to_owned(),
        ])),
        labeled: Some(Ok(Vec::new())),
        prs: Some(Ok(default_prs())),
        merged: BTreeMap::from([(("agent-ix/x".to_owned(), 10), true)]),
        commits: Some(Ok(Vec::new())),
        agents: Some(Ok(agents_report(vec![
            agent_row(Verdict::Working, Some(60)),
            agent_row(Verdict::Working, Some(120)),
            agent_row(Verdict::Idle, Some(1800)),
        ]))),
        ..Rig::default()
    }
}

const CONFIG: &str = r#"{
  "team": "SWM", "linear_teams": ["SWM"], "projects": ["Alpha"],
  "gate_tickets": ["SWM-1"], "repos": ["agent-ix/x"], "herdr_teams": ["SWM"],
  "thresholds": {"pr_idle_hours": 24, "pr_oversize_lines": 1000, "in_progress_stale_hours": 48}
}"#;

fn config_with(extra: &str) -> RetroConfig {
    let mut value: serde_json::Value = serde_json::from_str(CONFIG).unwrap();
    let extra: serde_json::Value = serde_json::from_str(extra).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    RetroConfig::parse(Path::new("retro.json"), &value.to_string()).unwrap()
}

fn config() -> RetroConfig {
    config_with("{}")
}

fn store_in(dir: &TempDir) -> Store {
    Store::resolve("SWM", Some(dir.path().into()), None).unwrap()
}

fn snapshot_with(rig: &Rig, config: &RetroConfig, window: u64) -> (Record, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let record = run_snapshot(config, window, NOW, &rig.sources(), &store_in(&dir)).unwrap();
    (record, dir)
}

fn snapshot(rig: &Rig) -> Record {
    snapshot_with(rig, &config(), DAY).0
}

fn value(record: &Record, id: &str) -> Option<f64> {
    let metric = &record.metrics[id];
    assert_eq!(metric.status == Status::Ok, metric.value.is_some(), "{id}");
    metric.value
}

fn reason<'a>(record: &'a Record, id: &str) -> &'a str {
    let metric = &record.metrics[id];
    assert_eq!(
        metric.status,
        Status::Unavailable,
        "{id} should be unavailable"
    );
    assert_eq!(metric.value, None, "{id} must not report a number");
    metric.reason.as_deref().unwrap()
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn git_init(dir: &Path) {
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(dir)
        .status()
        .unwrap();
    assert!(status.success());
}

// ---- FR-024 ----------------------------------------------------------------------------

/// Trace: FR-024-AC-1
#[test]
fn snapshot_appends_one_record_with_every_catalog_id() {
    let rig = base_rig();
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let record = run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap();
    assert_eq!(lines(store.path()).len(), 1);
    let ids: Vec<_> = record.metrics.keys().cloned().collect();
    let mut expected: Vec<_> = CATALOG.iter().map(|m| m.id.to_owned()).collect();
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(value(&record, "path_remaining"), Some(1.0));
    assert_eq!(value(&record, "prs_open"), Some(1.0));
    assert_eq!(value(&record, "fix_rounds_total"), Some(3.0));
    assert_eq!(reason(&record, "main_red_incidents"), "not configured");
    assert_eq!(record.kind, ix_board::retro::RecordKind::Snapshot);
    assert_eq!(record.taken_at, "2026-11-07T12:00:00Z");
    assert_eq!(record.window, "1d");
}

/// Trace: FR-024-AC-2
#[test]
fn path_counts_only_open_transitive_blockers_of_gates() {
    let record = snapshot(&base_rig());
    // SWM-2 and SWM-4 are completed, SWM-3 is open; SWM-5 and SWM-9 block no gate.
    assert_eq!(value(&record, "path_remaining"), Some(1.0));
    // SWM-2 and SWM-4 completed inside the window; SWM-8 completed 100h ago.
    assert_eq!(value(&record, "path_closed"), Some(2.0));
}

/// Trace: FR-024-AC-2
#[test]
fn path_remaining_is_unavailable_when_the_blocker_expansion_is_partial() {
    let mut rig = base_rig();
    let mut snapshot_data = rig.workspace.clone().unwrap().unwrap();
    snapshot_data.issues[2]
        .blocked_by
        .push(IssueId("id-missing".to_owned()));
    rig.workspace = Some(Ok(snapshot_data));
    let record = snapshot(&rig);
    for id in [
        "path_remaining",
        "path_closed",
        "net_closed_minus_new",
        "path_closed_unverified",
        "done_reopened",
    ] {
        assert!(reason(&record, id).contains("truncated or partial"), "{id}");
    }
    assert_eq!(value(&record, "intake_new"), Some(2.0));
}

/// Trace: FR-024-AC-3
#[test]
fn net_closed_minus_new_counts_path_issues_once_and_intake_only_in_the_window() {
    let record = snapshot(&base_rig());
    // New in the window: SWM-5, SWM-6. Completed in the window: SWM-2, SWM-4 (path), SWM-6.
    assert_eq!(value(&record, "intake_new"), Some(2.0));
    assert_eq!(value(&record, "net_closed_minus_new"), Some(1.0));

    let mut rig = base_rig();
    let mut data = rig.workspace.clone().unwrap().unwrap();
    for number in [10, 11] {
        data.issues.push(issue(
            &format!("SWM-{number}0"),
            S::Unstarted,
            Some("p1"),
            1.0,
            1.0,
        ));
    }
    rig.workspace = Some(Ok(data));
    let record = snapshot(&rig);
    assert_eq!(value(&record, "intake_new"), Some(4.0));
    assert_eq!(value(&record, "net_closed_minus_new"), Some(-1.0));
}

/// Trace: FR-024-AC-4
#[test]
fn merge_rate_and_cycle_percentiles_use_nearest_rank() {
    let record = snapshot(&base_rig());
    assert_eq!(value(&record, "prs_merged_per_hour"), Some(4.0 / 24.0));
    // Cycles 1, 2, 3 and 100 hours: rank ceil(0.5*4)=2 and ceil(0.9*4)=4.
    assert_eq!(value(&record, "pr_cycle_median_hours"), Some(2.0));
    assert_eq!(value(&record, "pr_cycle_p90_hours"), Some(100.0));
}

/// Trace: FR-024-AC-5
#[test]
fn pull_request_state_metrics_straddle_each_threshold() {
    let mut rig = base_rig();
    let open = |number, updated_h: f64, lines: u64| PullRequest {
        updated_at: ago(updated_h),
        additions: lines,
        deletions: 0,
        ..pr(number, true)
    };
    rig.prs = Some(Ok(vec![
        open(1, 25.0, 10),
        open(2, 24.0, 10),
        open(3, 23.0, 1000),
        open(4, 1.0, 1001),
        PullRequest {
            draft: true,
            review_decision: Some("APPROVED".to_owned()),
            ..open(5, 1.0, 10)
        },
        PullRequest {
            review_decision: Some("APPROVED".to_owned()),
            ..open(6, 1.0, 10)
        },
        PullRequest {
            review_decision: Some("REVIEW_REQUIRED".to_owned()),
            ..open(7, 1.0, 10)
        },
        merged_pr(8, 1.0, 5.0),
    ]));
    let record = snapshot(&rig);
    assert_eq!(value(&record, "prs_open"), Some(7.0));
    assert_eq!(
        value(&record, "prs_idle"),
        Some(2.0),
        "25h and exactly 24h are idle"
    );
    assert_eq!(
        value(&record, "prs_oversize"),
        Some(1.0),
        "exactly 1000 is not above"
    );
    assert_eq!(
        value(&record, "prs_ready_unmerged"),
        Some(1.0),
        "draft excluded"
    );
}

/// Trace: FR-024-AC-6
#[test]
fn fix_rounds_read_raw_comment_bodies_through_the_real_marker_parser() {
    let reviewer = "<!-- reviewer repo=agent-ix/x id=SR-1 pr=x#11 -->";
    let disposition = |round: u32, id: &str| {
        format!("<!-- reviewer-dispositions repo=agent-ix/x id={id} round={round} -->")
    };
    // Raw bodies that all pass the source's prefix and substring filter.
    let mut rig = base_rig();
    rig.markers = Some(Ok(vec![
        format!("{reviewer}\nfindings"),
        format!("{}\nbody", disposition(1, "SR-1")),
        // The marker text is on a later line, the first line is another marker without an id.
        format!(
            "<!-- reviewer-dispositions notes -->\n{}",
            disposition(9, "SR-1")
        ),
        // A human comment that starts with marker text and carries more text on the line.
        format!("{} I disagree", disposition(7, "SR-1")),
        // A disposition whose id matches no reviewer marker.
        format!("{}\nbody", disposition(5, "SR-404")),
        // A disposition of another repository with the same id.
        "<!-- reviewer-dispositions repo=agent-ix/y id=SR-1 round=30 -->\nbody".to_owned(),
    ]));
    let record = snapshot(&rig);
    assert_eq!(value(&record, "fix_rounds_max"), Some(1.0));
    assert_eq!(value(&record, "fix_rounds_total"), Some(1.0));
}

/// Trace: FR-024-AC-7
#[test]
fn force_pushes_reopened_and_main_red_follow_the_fixture() {
    let mut rig = base_rig();
    rig.commits = Some(Ok(vec![
        MainCommit {
            committed_at: ago(20.0),
            state: CheckState::Pass,
        },
        MainCommit {
            committed_at: ago(15.0),
            state: CheckState::Fail,
        },
        MainCommit {
            committed_at: ago(10.0),
            state: CheckState::Pass,
        },
    ]));
    let record = snapshot_with(
        &rig,
        &config_with(r#"{"main_check": {"repo": "agent-ix/x", "branch": "main"}}"#),
        DAY,
    )
    .0;
    assert_eq!(value(&record, "head_force_pushes_max"), Some(2.0));
    assert_eq!(value(&record, "done_reopened"), Some(1.0));
    assert_eq!(value(&record, "main_red_incidents"), Some(1.0));
    let record = snapshot(&base_rig());
    assert_eq!(reason(&record, "main_red_incidents"), "not configured");
    assert_eq!(reason(&record, "main_red_minutes"), "not configured");
}

/// Trace: FR-024-AC-8
#[test]
fn scope_metrics_and_stale_issues_ignore_recently_updated_pull_requests() {
    let mut rig = base_rig();
    rig.project_counts.insert("Alpha".to_owned(), 7);
    let record = snapshot(&rig);
    // Open SWM issues without a project: SWM-7. Without a milestone: SWM-3 and SWM-7.
    assert_eq!(value(&record, "issues_without_project"), Some(1.0));
    assert_eq!(value(&record, "issues_without_milestone"), Some(2.0));
    // Snapshot counts 4 open project issues (SWM-1, 3, 5, 9); the chunk read says 7.
    assert_eq!(value(&record, "source_count_disagreement"), Some(3.0));
    // SWM-7 is stale; SWM-3 changed 60h ago but its attached PR #11 updated 1h ago.
    assert_eq!(value(&record, "in_progress_stale"), Some(1.0));
}

/// Trace: FR-024-AC-9
#[test]
fn agent_metrics_match_rows_and_an_empty_list_is_an_empty_population() {
    let record = snapshot(&base_rig());
    assert_eq!(value(&record, "agents_working_share"), Some(2.0 / 3.0));
    assert_eq!(value(&record, "agents_idle_minutes_max"), Some(30.0));
    let mut rig = base_rig();
    rig.agents = Some(Ok(agents_report(Vec::new())));
    let record = snapshot(&rig);
    assert_eq!(reason(&record, "agents_working_share"), "empty population");
    assert_eq!(
        reason(&record, "agents_idle_minutes_max"),
        "empty population"
    );
}

/// Trace: FR-024-AC-10
#[test]
fn local_metrics_read_a_real_directory_with_a_prunable_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_init(&repo);
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["commit", "-q", "--allow-empty", "-m", "init"]);
    let worktree = dir.path().join("wt");
    git(&["worktree", "add", "-q", worktree.to_str().unwrap()]);
    std::fs::remove_dir_all(&worktree).unwrap();
    std::fs::write(repo.join("data.bin"), [0_u8; 100]).unwrap();

    let local = ix_board::retro::live::SystemLocal;
    let rig = base_rig();
    let sources = Sources {
        local: &local,
        ..rig.sources()
    };
    let cfg = config_with(&format!(
        r#"{{"local_paths": [{:?}]}}"#,
        repo.to_str().unwrap()
    ));
    let store_dir = tempfile::tempdir().unwrap();
    let record = run_snapshot(&cfg, DAY, NOW, &sources, &store_in(&store_dir)).unwrap();
    assert_eq!(value(&record, "worktrees_prunable"), Some(1.0));
    assert!(value(&record, "disk_bytes").unwrap() >= 100.0);

    let record = snapshot(&base_rig());
    assert_eq!(reason(&record, "worktrees_prunable"), "not configured");
    assert_eq!(reason(&record, "disk_bytes"), "not configured");
}

fn gate_line(
    pr: &str,
    repin: bool,
    requested_h: f64,
    acquired_h: f64,
    finished_h: f64,
) -> GateLine {
    GateLine {
        pr: pr.to_owned(),
        repin,
        requested_lock_at: ago(requested_h),
        acquired_lock_at: ago(acquired_h),
        started_at: ago(acquired_h),
        finished_at: ago(finished_h),
    }
}

const ALL_OPTIONAL: &str = r#"{
  "gate_log": "gates.jsonl", "slot_process": "qsl-slot", "orphan_slot_hours": 2,
  "relay_leads": ["a:b:c"], "message_stale_minutes": 10, "decision_label": "Decision"
}"#;

/// Trace: FR-024-AC-11
#[test]
fn gate_log_orphans_relay_and_decisions_match_their_fixtures() {
    let mut rig = base_rig();
    rig.gate_lines = Some(Ok(vec![
        gate_line("1", false, 5.0, 4.5, 4.0),
        gate_line("1", false, 3.0, 2.0, 1.0),
        gate_line("2", false, 3.0, 3.0, 2.0),
        gate_line("2", true, 2.0, 1.5, 1.0),
        gate_line("3", false, 60.0, 59.0, 58.0),
    ]));
    rig.processes = Some(Ok(vec![
        ProcessRow {
            command: "bash qsl-slot run".to_owned(),
            parent_alive: false,
            age_secs: 3 * 3600,
        },
        ProcessRow {
            command: "bash qsl-slot run".to_owned(),
            parent_alive: true,
            age_secs: 3 * 3600,
        },
        ProcessRow {
            command: "bash qsl-slot run".to_owned(),
            parent_alive: false,
            age_secs: 3600,
        },
        ProcessRow {
            command: "sleep".to_owned(),
            parent_alive: false,
            age_secs: 9 * 3600,
        },
    ]));
    rig.relay.insert(
        "a:b:c".to_owned(),
        Ok(vec![
            (ago(1.0) * 1000.0) as i64,
            (NOW * 1000.0) as i64 - 60_000,
        ]),
    );
    rig.labeled = Some(Ok(vec![
        LabeledIssue {
            identifier: "SWM-1".to_owned(),
            created_at: ago(48.0),
        },
        LabeledIssue {
            identifier: "SWM-2".to_owned(),
            created_at: ago(5.0),
        },
    ]));
    let record = snapshot_with(&rig, &config_with(ALL_OPTIONAL), DAY).0;
    assert_eq!(value(&record, "full_gates_max"), Some(2.0));
    assert_eq!(value(&record, "repins_total"), Some(1.0));
    // Waits: 30, 60, 0, 30 minutes inside the window; the 60h-old line is outside it.
    assert_eq!(value(&record, "build_lock_wait_minutes"), Some(120.0));
    assert_eq!(value(&record, "orphan_slots"), Some(1.0));
    assert_eq!(value(&record, "messages_undelivered"), Some(1.0));
    assert_eq!(value(&record, "decisions_open"), Some(2.0));
    assert_eq!(value(&record, "decision_oldest_hours"), Some(48.0));

    let record = snapshot(&base_rig());
    for id in [
        "full_gates_max",
        "repins_total",
        "build_lock_wait_minutes",
        "orphan_slots",
        "messages_undelivered",
        "decisions_open",
        "decision_oldest_hours",
    ] {
        assert_eq!(reason(&record, id), "not configured", "{id}");
    }
}

/// Trace: FR-024-AC-11
#[test]
fn the_gate_log_file_reader_parses_jsonl_and_refuses_bad_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gates.jsonl");
    let row = |kind: &str| {
        format!(
            r#"{{"pr":"7","kind":"{kind}","requested_lock_at":"2026-11-07T10:00:00Z","acquired_lock_at":"2026-11-07T10:05:00Z","started_at":"2026-11-07T10:05:00Z","finished_at":"2026-11-07T10:30:00Z"}}"#
        )
    };
    std::fs::write(&path, format!("{}\n\n{}\n", row("gate"), row("repin"))).unwrap();
    let parsed = ix_board::retro::live::FileGateLog.lines(&path).unwrap();
    assert_eq!(parsed.len(), 2);
    assert!(parsed[1].repin);
    assert_eq!(
        parsed[0].acquired_lock_at - parsed[0].requested_lock_at,
        300.0
    );
    std::fs::write(&path, format!("{}\nnot json\n", row("gate"))).unwrap();
    let error = ix_board::retro::live::FileGateLog.lines(&path).unwrap_err();
    assert!(error.contains("line 2"), "{error}");
}

/// Trace: FR-024-AC-15
#[test]
fn main_red_counts_one_incident_per_pass_to_fail_run_and_still_failing_to_now() {
    let cfg = config_with(r#"{"main_check": {"repo": "agent-ix/x", "branch": "main"}}"#);
    let mut rig = base_rig();
    rig.commits = Some(Ok(vec![
        MainCommit {
            committed_at: ago(20.0),
            state: CheckState::Pass,
        },
        MainCommit {
            committed_at: ago(15.0),
            state: CheckState::Fail,
        },
        MainCommit {
            committed_at: ago(10.0),
            state: CheckState::Pass,
        },
    ]));
    let record = snapshot_with(&rig, &cfg, DAY).0;
    assert_eq!(value(&record, "main_red_incidents"), Some(1.0));
    assert_eq!(value(&record, "main_red_minutes"), Some(300.0));

    rig.commits = Some(Ok(vec![
        MainCommit {
            committed_at: ago(20.0),
            state: CheckState::Pass,
        },
        MainCommit {
            committed_at: ago(5.0),
            state: CheckState::Fail,
        },
        MainCommit {
            committed_at: ago(4.0),
            state: CheckState::Fail,
        },
    ]));
    let record = snapshot_with(&rig, &cfg, DAY).0;
    assert_eq!(value(&record, "main_red_incidents"), Some(1.0));
    assert_eq!(
        value(&record, "main_red_minutes"),
        Some(300.0),
        "to the snapshot time"
    );
}

/// Trace: FR-024-AC-16
#[test]
fn gates_remaining_and_unverified_completions() {
    let record = snapshot(&base_rig());
    assert_eq!(value(&record, "gates_remaining"), Some(1.0));
    // SWM-2 completed with merged PR #10; SWM-4 completed with no PR attached.
    assert_eq!(value(&record, "path_closed_unverified"), Some(1.0));
    let mut rig = base_rig();
    rig.merged.insert(("agent-ix/x".to_owned(), 10), false);
    assert_eq!(value(&snapshot(&rig), "path_closed_unverified"), Some(2.0));
}

/// Trace: FR-024-AC-17
#[test]
fn configuration_errors_name_the_file_and_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retro.json");
    let missing = RetroConfig::load(&path).unwrap_err().to_string();
    assert!(
        missing.contains("retro.json") && missing.contains("cannot read"),
        "{missing}"
    );
    std::fs::write(&path, "{not json").unwrap();
    assert!(
        RetroConfig::load(&path)
            .unwrap_err()
            .to_string()
            .contains("retro.json")
    );
    for field in ["team", "gate_tickets", "projects", "repos"] {
        let mut value: serde_json::Value = serde_json::from_str(CONFIG).unwrap();
        value.as_object_mut().unwrap().remove(field);
        std::fs::write(&path, value.to_string()).unwrap();
        let message = RetroConfig::load(&path).unwrap_err().to_string();
        assert!(
            message.contains("retro.json") && message.contains(field),
            "{message}"
        );
    }
}

/// Trace: FR-024-AC-17
#[test]
fn the_command_exits_non_zero_and_appends_nothing_on_a_bad_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("retro.json");
    std::fs::write(&config, r#"{"team": "SWM", "projects": [], "repos": []}"#).unwrap();
    let home = dir.path().join("home");
    let output = assert_cmd::Command::cargo_bin("ix-board")
        .unwrap()
        .env("TEAM_RETRO_HOME", &home)
        .args(["retro", "snapshot", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("retro.json") && stderr.contains("gate_tickets"),
        "{stderr}"
    );
    assert!(!home.exists(), "nothing is created");
}

/// Trace: FR-024-AC-18
#[test]
fn absent_thresholds_and_half_of_a_pair_are_not_configured() {
    let mut rig = base_rig();
    rig.processes = Some(Ok(Vec::new()));
    rig.relay.insert("a:b:c".to_owned(), Ok(Vec::new()));
    let bare = RetroConfig::parse(
        Path::new("retro.json"),
        r#"{"team":"SWM","projects":["Alpha"],"gate_tickets":["SWM-1"],"repos":["agent-ix/x"],
            "slot_process":"qsl-slot","relay_leads":["a:b:c"]}"#,
    )
    .unwrap();
    let record = snapshot_with(&rig, &bare, DAY).0;
    for id in [
        "prs_idle",
        "prs_oversize",
        "in_progress_stale",
        "orphan_slots",
        "messages_undelivered",
    ] {
        assert_eq!(reason(&record, id), "not configured", "{id}");
    }
    assert_eq!(
        value(&record, "prs_open"),
        Some(1.0),
        "metrics without a threshold still compute"
    );
    // Orphan age is strictly above the limit.
    rig.processes = Some(Ok(vec![
        ProcessRow {
            command: "qsl-slot".to_owned(),
            parent_alive: false,
            age_secs: 7200,
        },
        ProcessRow {
            command: "qsl-slot".to_owned(),
            parent_alive: false,
            age_secs: 7201,
        },
    ]));
    let both = config_with(r#"{"slot_process":"qsl-slot","orphan_slot_hours":2}"#);
    assert_eq!(
        value(&snapshot_with(&rig, &both, DAY).0, "orphan_slots"),
        Some(1.0)
    );
}

/// Trace: FR-024-AC-11
#[test]
fn an_unregistered_relay_address_makes_the_metric_unavailable() {
    let mut rig = base_rig();
    rig.relay.insert(
        "a:b:c".to_owned(),
        Err("no registered lead matches a:b:c".to_owned()),
    );
    let cfg = config_with(r#"{"relay_leads":["a:b:c"],"message_stale_minutes":5}"#);
    let record = snapshot_with(&rig, &cfg, DAY).0;
    assert!(reason(&record, "messages_undelivered").contains("a:b:c"));
}

/// Trace: FR-024-AC-12
#[test]
fn a_snapshot_only_reads_and_leaves_a_repository_untouched() {
    let repo = tempfile::tempdir().unwrap();
    git_init(repo.path());
    std::fs::write(repo.path().join("file.txt"), "content").unwrap();
    let listing = |root: &Path| -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    stack.push(entry.path());
                }
                out.push((entry.path(), meta.len(), meta.modified().unwrap()));
            }
        }
        out.sort();
        out
    };
    let before = listing(repo.path());
    let local = ix_board::retro::live::SystemLocal;
    let mut rig = base_rig();
    rig.gate_lines = Some(Ok(Vec::new()));
    rig.processes = Some(Ok(Vec::new()));
    rig.relay.insert("a:b:c".to_owned(), Ok(Vec::new()));
    rig.labeled = Some(Ok(Vec::new()));
    rig.commits = Some(Ok(Vec::new()));
    let sources = Sources {
        local: &local,
        ..rig.sources()
    };
    let cfg = config_with(&format!(
        r#"{{"local_paths": [{:?}], "main_check": {{"repo": "agent-ix/x", "branch": "main"}},
            "gate_log": "g", "slot_process": "s", "orphan_slot_hours": 1,
            "relay_leads": ["a:b:c"], "message_stale_minutes": 1, "decision_label": "D"}}"#,
        repo.path().to_str().unwrap()
    ));
    let store_dir = tempfile::tempdir().unwrap();
    run_snapshot(&cfg, DAY, NOW, &sources, &store_in(&store_dir)).unwrap();
    assert_eq!(listing(repo.path()), before, "no repository write");
    // The sources expose reads only; the recorded calls are exactly those reads.
    const READS: [&str; 12] = [
        "workspace",
        "project_open_count",
        "marker_lines",
        "open_issues_with_label",
        "pull_requests:agent-ix/x",
        "pull_request:agent-ix/x#10",
        "main_commits",
        "agents",
        "gate_log",
        "relay",
        "processes",
        "prunable_worktrees",
    ];
    for call in rig.calls.borrow().iter() {
        assert!(
            READS.contains(&call.as_str())
                || call.starts_with("state_history:")
                || call.starts_with("batch:")
                || call.starts_with("pull_request:"),
            "unexpected call {call}"
        );
    }
}

/// Trace: FR-024-AC-13
#[test]
fn each_source_is_read_once_per_run() {
    let rig = base_rig();
    snapshot(&rig);
    assert_eq!(rig.count("workspace"), 1);
    assert_eq!(rig.count("pull_requests:agent-ix/x"), 1);
    assert!(rig.count("marker_lines") >= 1);
    assert_eq!(rig.count("agents"), 1);
    let history: Vec<_> = rig
        .calls
        .borrow()
        .iter()
        .filter(|call| call.starts_with("state_history:"))
        .cloned()
        .collect();
    let mut unique = history.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(history.len(), unique.len(), "one history read per issue");
}

/// Trace: FR-024-AC-13
#[test]
fn a_pass_that_stops_before_the_last_page_is_an_error_not_a_shorter_list() {
    let mut served = 0;
    let result = read_all_pages(|_| {
        served += 1;
        if served == 1 {
            Ok(Page {
                items: vec![1, 2],
                has_next: true,
                cursor: Some("c".to_owned()),
            })
        } else {
            Err("boom".to_owned())
        }
    });
    assert!(result.unwrap_err().contains("stopped before the last page"));
    let result = read_all_pages(|_| {
        Ok(Page {
            items: vec![1],
            has_next: true,
            cursor: None,
        })
    });
    assert!(result.unwrap_err().contains("stopped before the last page"));
    let result = read_all_pages(|cursor| {
        Ok(Page {
            items: vec![1],
            has_next: cursor.is_none(),
            cursor: Some("next".to_owned()),
        })
    });
    assert_eq!(result.unwrap(), vec![1, 1]);
}

/// Trace: FR-024-AC-14
#[test]
fn json_exposes_every_metric_field_and_text_prints_one_line_each() {
    let record = snapshot(&base_rig());
    let json = serde_json::to_value(&record).unwrap();
    let metrics = json["metrics"].as_object().unwrap();
    assert_eq!(metrics.len(), CATALOG.len());
    for (id, metric) in metrics {
        for field in [
            "id",
            "value",
            "unit",
            "direction",
            "status",
            "source",
            "query",
        ] {
            assert!(metric.get(field).is_some(), "{id} lacks {field}");
        }
        assert_eq!(metric["id"], id.as_str());
    }
    assert_eq!(metrics["prs_open"]["direction"], "neutral");
    assert_eq!(metrics["path_remaining"]["direction"], "down");
    assert_eq!(
        ix_board::retro::render_text(&record).lines().count(),
        CATALOG.len()
    );
}

/// A pull request merged before the window but updated inside it, as the live listing returns.
fn merged_before_window() -> PullRequest {
    PullRequest {
        updated_at: ago(1.0),
        force_pushes: Some(50),
        ..merged_pr(500, 1.0, 90.0)
    }
}

/// Trace: FR-024-AC-6
#[test]
fn fix_rounds_and_force_pushes_exclude_pull_requests_outside_the_window() {
    let mut rig = base_rig();
    let mut listing = default_prs();
    listing.push(merged_before_window());
    rig.prs = Some(Ok(listing));
    rig.markers = Some(Ok(vec![
        "<!-- reviewer repo=agent-ix/x id=SR-1 pr=x#11 -->".to_owned(),
        "<!-- reviewer-dispositions repo=agent-ix/x id=SR-1 round=2 -->".to_owned(),
        // Merged before the window: in the listing, but neither open nor merged inside it.
        "<!-- reviewer repo=agent-ix/x id=SR-2 pr=x#500 -->".to_owned(),
        "<!-- reviewer-dispositions repo=agent-ix/x id=SR-2 round=40 -->".to_owned(),
        // Another repository, and a repository that is not configured.
        "<!-- reviewer repo=agent-ix/y id=SR-3 pr=y#11 -->".to_owned(),
        "<!-- reviewer-dispositions repo=agent-ix/y id=SR-3 round=30 -->".to_owned(),
        // Same id as this team's review, disposed in another repository.
        "<!-- reviewer-dispositions repo=agent-ix/y id=SR-1 round=20 -->".to_owned(),
    ]));
    let record = snapshot(&rig);
    assert_eq!(value(&record, "fix_rounds_max"), Some(2.0));
    assert_eq!(value(&record, "fix_rounds_total"), Some(2.0));
    assert_eq!(
        value(&record, "head_force_pushes_max"),
        Some(2.0),
        "the 50-force-push pull request merged before the window is excluded"
    );
    rig.prs = Some(Err("down".to_owned()));
    let record = snapshot(&rig);
    assert!(reason(&record, "fix_rounds_total").contains("GitHub"));
}

/// Trace: FR-024-AC-13
#[test]
fn histories_are_read_in_one_batch_for_issues_updated_since_the_window_opened() {
    let rig = base_rig();
    snapshot(&rig);
    let batches: Vec<_> = rig
        .calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("batch:"))
        .cloned()
        .collect();
    assert!(batches.len() <= 2, "{batches:?}");
    // SWM-8 and SWM-7 were last updated 100h ago, before the 24h window and the 48h cutoff.
    for old in ["id-SWM-8", "id-SWM-7"] {
        assert_eq!(rig.count(&format!("state_history:{old}")), 0, "{old}");
    }
}

/// Trace: FR-024-AC-11
#[test]
fn relay_addresses_resolve_to_registered_session_queues_and_settings_follow_precedence() {
    use ix_board::retro::live::{
        RedisTarget, RelaySettings, parse_redis_url, queue_names_for_address, relay_settings,
    };
    let registration = |machine: &str, workspace: &str, tab: &str| serde_json::json!({"machine": machine, "place": {"workspace": workspace, "tab": tab}});
    let registrations = vec![
        (
            "session/6c/aa".to_owned(),
            registration("luna", "IR", "plan"),
        ),
        (
            "session/6c/bb".to_owned(),
            registration("luna", "IR", "plan"),
        ),
        (
            "session/64/cc".to_owned(),
            registration("draco", "IR", "plan"),
        ),
    ];
    assert_eq!(
        queue_names_for_address("luna:IR:plan", &registrations).unwrap(),
        ["session/6c/aa", "session/6c/bb"]
    );
    // An unregistered or malformed address is an error, never an empty queue.
    for bad in ["luna:IR:code", "peter", "luna:IR", "a:b:c:d", ""] {
        assert!(
            queue_names_for_address(bad, &registrations).is_err(),
            "{bad}"
        );
    }

    let file = "url = \"redis://u:pw@10.0.0.1:6380\"\nprefix = \"p:\"\n";
    assert_eq!(
        relay_settings(None, None, Some(file)),
        RelaySettings {
            url: "redis://u:pw@10.0.0.1:6380".to_owned(),
            prefix: "p:".to_owned()
        }
    );
    let env = relay_settings(
        Some("redis://h".to_owned()),
        Some("e:".to_owned()),
        Some(file),
    );
    assert_eq!((env.url.as_str(), env.prefix.as_str()), ("redis://h", "e:"));
    assert_eq!(relay_settings(None, None, None).prefix, "ixr:v3:");
    assert_eq!(
        parse_redis_url("redis://draco:secret@10.99.0.1:6380").unwrap(),
        RedisTarget {
            host: "10.99.0.1".to_owned(),
            port: 6380,
            user: Some("draco".to_owned()),
            password: Some("secret".to_owned()),
            db: None,
        }
    );
    assert_eq!(parse_redis_url("redis://h:1/3").unwrap().db, Some(3));
    assert!(parse_redis_url("redis://h/x").is_err());
    assert_eq!(parse_redis_url("redis://127.0.0.1").unwrap().port, 6379);
    assert!(parse_redis_url("http://x").is_err());
}

/// Trace: FR-024-AC-2
#[test]
fn path_closed_ignores_a_completion_before_the_window() {
    let mut rig = base_rig();
    let mut data = rig.workspace.clone().unwrap().unwrap();
    data.issues[0]
        .blocked_by
        .push(IssueId("id-SWM-12".to_owned()));
    // On the path, updated inside the window, but completed 48h ago.
    data.issues
        .push(issue("SWM-12", S::Completed, Some("p1"), 300.0, 2.0));
    rig.workspace = Some(Ok(data));
    rig.histories.insert(
        "id-SWM-12".to_owned(),
        Ok(vec![change(48.0, S::Started, S::Completed)]),
    );
    let record = snapshot(&rig);
    assert_eq!(value(&record, "path_closed"), Some(2.0));
    assert_eq!(value(&record, "path_remaining"), Some(1.0));
}

/// Trace: FR-024-AC-7
#[test]
fn main_red_skips_unchecked_commits_and_ignores_a_leading_failing_run() {
    let cfg = config_with(r#"{"main_check": {"repo": "agent-ix/x", "branch": "main"}}"#);
    let at = |hours: f64, state| MainCommit {
        committed_at: ago(hours),
        state,
    };
    let mut rig = base_rig();
    rig.commits = Some(Ok(vec![
        at(21.0, CheckState::Fail),
        at(20.0, CheckState::Fail),
        at(18.0, CheckState::Pass),
        at(17.0, CheckState::Pending),
        at(16.0, CheckState::Fail),
        at(15.0, CheckState::Pending),
        at(14.0, CheckState::Pass),
    ]));
    let record = snapshot_with(&rig, &cfg, DAY).0;
    assert_eq!(value(&record, "main_red_incidents"), Some(1.0));
    assert_eq!(value(&record, "main_red_minutes"), Some(120.0));
    assert!(
        record.metrics["main_red_incidents"]
            .query
            .contains("agent-ix/x@main")
    );
}

/// Trace: FR-024-AC-8
#[test]
fn a_stale_issue_reads_its_attached_pull_request_from_that_pull_requests_repository() {
    let attach = |rig: &mut Rig| {
        let mut data = rig.workspace.clone().unwrap().unwrap();
        let stale = data
            .issues
            .iter_mut()
            .find(|i| i.identifier == "SWM-7")
            .unwrap();
        stale
            .attachment_urls
            .push("https://github.com/agent-ix/other/pull/5".to_owned());
        rig.workspace = Some(Ok(data));
    };
    let mut rig = base_rig();
    attach(&mut rig);
    let key = ("agent-ix/other".to_owned(), 5);
    // Another repository, closed without merging, updated an hour ago: not stale.
    rig.infos.insert(
        key.clone(),
        PullRequestInfo {
            merged: false,
            updated_at: ago(1.0),
        },
    );
    assert_eq!(value(&snapshot(&rig), "in_progress_stale"), Some(0.0));
    // The same pull request untouched for 200h: stale.
    rig.infos.insert(
        key,
        PullRequestInfo {
            merged: false,
            updated_at: ago(200.0),
        },
    );
    assert_eq!(value(&snapshot(&rig), "in_progress_stale"), Some(1.0));
    // A repository GitHub cannot read: unavailable, never a false stale.
    rig.infos.clear();
    let record = snapshot(&rig);
    assert!(reason(&record, "in_progress_stale").contains("agent-ix/other#5"));
}

/// Trace: FR-024-AC-9
#[test]
fn herdr_metric_queries_record_the_idle_threshold() {
    let record = snapshot(&base_rig());
    for id in ["agents_working_share", "agents_idle_minutes_max"] {
        assert!(record.metrics[id].query.contains("idle_after=20m"), "{id}");
    }
}

/// Trace: FR-024-AC-7
#[test]
fn force_push_metric_query_states_its_counting_rule() {
    let record = snapshot(&base_rig());
    let query = &record.metrics["head_force_pushes_max"].query;
    assert!(
        query.contains("force-push events") && !query.contains("lower bound"),
        "{query}"
    );
    assert_eq!(record.metrics["head_force_pushes_max"].unit, "force_pushes");
    assert!(!record.metrics.contains_key("head_shas_per_pr_max"));
}

/// Trace: FR-024-AC-18
#[test]
fn a_threshold_too_large_to_measure_is_unavailable_with_a_reason() {
    let mut rig = base_rig();
    rig.processes = Some(Ok(Vec::new()));
    let cfg = config_with(&format!(
        r#"{{"slot_process":"s","orphan_slot_hours":{max},"thresholds":{{"pr_idle_hours":{max},"pr_oversize_lines":1,"in_progress_stale_hours":{max}}}}}"#,
        max = u64::MAX
    ));
    let record = snapshot_with(&rig, &cfg, DAY).0;
    for id in ["orphan_slots", "prs_idle", "in_progress_stale"] {
        assert!(reason(&record, id).contains("too large"), "{id}");
    }
}

/// Trace: FR-024-AC-13
#[test]
fn a_cached_snapshot_is_current_only_if_recent_updates_match() {
    use ix_board::retro::live::snapshot_is_current;
    let snap = rig_snapshot();
    let updated = snap.issues[0].updated_at.clone().unwrap();
    let id = snap.issues[0].id.0.clone();
    assert!(snapshot_is_current(&snap, &[(id.clone(), updated)]));
    assert!(snapshot_is_current(&snap, &[]));
    assert!(!snapshot_is_current(
        &snap,
        &[(id, "2099-01-01T00:00:00Z".to_owned())]
    ));
    assert!(!snapshot_is_current(
        &snap,
        &[("new-issue".to_owned(), "2026-11-07T11:00:00Z".to_owned())]
    ));
}

fn rig_snapshot() -> WorkspaceSnapshot {
    base_rig().workspace.unwrap().unwrap()
}

/// Trace: FR-024-AC-13
#[test]
fn the_live_listing_stops_at_the_cutoff_only_when_a_page_ends_before_it() {
    use ix_board::retro::live::pull_request_page;
    let node = |number: u64, updated: &str, forced: u64| {
        format!(
            r#"{{"number":{number},"createdAt":"2026-11-01T00:00:00Z","mergedAt":null,"updatedAt":"{updated}","isDraft":false,"state":"MERGED","additions":1,"deletions":2,"reviewDecision":null,"timelineItems":{{"totalCount":{forced}}}}}"#
        )
    };
    let page = |nodes: &[String], has_next: bool| {
        format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"nodes":[{}],"pageInfo":{{"hasNextPage":{has_next},"endCursor":"c"}}}}}}}}}}"#,
            nodes.join(",")
        )
    };
    let cutoff = Some(NOW - 24.0 * HOUR);
    // The last node on the page is older than the cutoff: later pages cannot matter.
    let old = page(
        &[
            node(1, "2026-11-07T11:00:00Z", 2),
            node(2, "2026-10-01T00:00:00Z", 0),
        ],
        true,
    );
    let parsed = pull_request_page(&old, "o/r", cutoff).unwrap();
    assert!(!parsed.has_next);
    assert_eq!(
        parsed.items[0].force_pushes,
        Some(2),
        "the force-push count"
    );
    // Still inside the window: keep reading.
    let recent = page(
        &[
            node(1, "2026-11-07T11:00:00Z", 0),
            node(2, "2026-11-07T10:00:00Z", 0),
        ],
        true,
    );
    assert!(pull_request_page(&recent, "o/r", cutoff).unwrap().has_next);
    // No cutoff (the open listing) reads every page.
    assert!(pull_request_page(&old, "o/r", None).unwrap().has_next);
    // The source's own last page stays last.
    assert!(
        !pull_request_page(&recent.replace("true", "false"), "o/r", cutoff)
            .unwrap()
            .has_next
    );
    // An unknown field in the selection is a decoding error.
    assert!(
        pull_request_page(
            &old.replace("\"number\"", "\"extra\":1,\"number\""),
            "o/r",
            None
        )
        .is_err()
    );
}

/// Trace: FR-024-AC-13
#[test]
fn the_freshness_check_covers_the_window_start_or_the_earlier_stale_cutoff() {
    // Window 24h, stale threshold 48h: the earlier of the two is the stale cutoff.
    let rig = base_rig();
    snapshot_with(&rig, &config(), DAY);
    assert_eq!(*rig.since.borrow(), [NOW - 48.0 * HOUR]);
    // Stale threshold 6h, window 24h: the window start is earlier.
    let rig = base_rig();
    let narrow = config_with(
        r#"{"thresholds": {"pr_idle_hours": 24, "pr_oversize_lines": 1000, "in_progress_stale_hours": 6}}"#,
    );
    snapshot_with(&rig, &narrow, DAY);
    assert_eq!(*rig.since.borrow(), [NOW - DAY as f64]);
}

/// Trace: FR-024-AC-13
#[test]
fn a_cache_stale_only_inside_the_checked_range_is_refetched() {
    use ix_board::retro::live::current_workspace;
    let cached = rig_snapshot();
    let id = cached.issues[0].id.0.clone();
    let seen = RefCell::new(Vec::new());
    // The issue changed after the cache fill, so the fresh read of the range reports a new time.
    let fresh = |since: f64| {
        seen.borrow_mut().push(since);
        Ok(vec![(id.clone(), "2099-01-01T00:00:00Z".to_owned())])
    };
    let mut refreshed = rig_snapshot();
    refreshed.fetched_at = 7;
    let result = current_workspace(NOW - 48.0 * HOUR, cached.clone(), fresh, || {
        Ok(refreshed.clone())
    });
    assert_eq!(result.unwrap().fetched_at, 7, "a stale cache is replaced");
    assert_eq!(
        *seen.borrow(),
        [NOW - 48.0 * HOUR],
        "the range given is the range read"
    );
    // A matching fresh read keeps the cache and does not refetch.
    let same = cached.issues[0].updated_at.clone().unwrap();
    let kept = current_workspace(
        NOW,
        cached.clone(),
        |_| Ok(vec![(id.clone(), same.clone())]),
        || Err("must not refetch".to_owned()),
    );
    assert_eq!(kept.unwrap(), cached);
}

// ---- FR-025 ----------------------------------------------------------------------------

/// Trace: FR-025-AC-1
#[test]
fn store_path_defaults_below_home_and_follows_the_override() {
    let default = Store::resolve("SWM", None, Some("/home/u".into())).unwrap();
    assert_eq!(
        default.path(),
        Path::new("/home/u/.local/state/team-retro/SWM/snapshots.jsonl")
    );
    let set = Store::resolve("SWM", Some("/data/retro".into()), Some("/home/u".into())).unwrap();
    assert_eq!(set.path(), Path::new("/data/retro/SWM/snapshots.jsonl"));
    assert!(Store::resolve("../x", Some("/d".into()), None).is_err());
    assert!(Store::resolve("a/b", Some("/d".into()), None).is_err());
}

/// Trace: FR-025-AC-2
#[test]
fn consecutive_snapshots_append_and_leave_the_first_line_untouched() {
    let rig = base_rig();
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap();
    let first = std::fs::read(store.path()).unwrap();
    run_snapshot(&config(), DAY, NOW + 60.0, &rig.sources(), &store).unwrap();
    let both = std::fs::read(store.path()).unwrap();
    assert!(both.starts_with(&first));
    assert_eq!(lines(store.path()).len(), 2);
}

/// Trace: FR-025-AC-3
#[test]
fn a_store_inside_a_git_work_tree_is_refused_and_nothing_is_created() {
    let rig = base_rig();
    let repo = tempfile::tempdir().unwrap();
    git_init(repo.path());
    let root = repo.path().join("state");
    let store = Store::resolve("SWM", Some(root.clone().into()), None).unwrap();
    let error = run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap_err();
    assert!(
        error.to_string().contains(root.to_str().unwrap()) || error.to_string().contains("state")
    );
    assert!(matches!(
        error,
        ix_board::retro::RetroError::Store(StoreError::InsideGit { .. })
    ));
    assert!(!root.exists());
    assert!(
        rig.calls.borrow().is_empty(),
        "no source is read for a refused path"
    );

    // Through a symbolic link that lives outside the repository.
    let outside = tempfile::tempdir().unwrap();
    let link = outside.path().join("link");
    std::os::unix::fs::symlink(repo.path(), &link).unwrap();
    let store = Store::resolve("SWM", Some(link.clone().into()), None).unwrap();
    let error = run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap_err();
    assert!(matches!(
        error,
        ix_board::retro::RetroError::Store(StoreError::InsideGit { .. })
    ));
    assert_eq!(
        std::fs::read_dir(repo.path()).unwrap().count(),
        1,
        "only .git"
    );

    // A dangling link into the repository is followed as well.
    let dangling = outside.path().join("dangling");
    std::os::unix::fs::symlink(repo.path().join("later"), &dangling).unwrap();
    let store = Store::resolve("SWM", Some(dangling.into()), None).unwrap();
    assert!(store.check_writable().is_err());
}

fn write_record(store: &Store, taken_at: &str, version: u32) {
    write_record_in(store, taken_at, version, "1d");
}

fn write_record_in(store: &Store, taken_at: &str, version: u32, window: &str) {
    let rig = base_rig();
    let mut record = run_snapshot(
        &config(),
        DAY,
        NOW,
        &rig.sources(),
        &store_in(&tempfile::tempdir().unwrap()),
    )
    .unwrap();
    record.taken_at = taken_at.to_owned();
    record.schema_version = version;
    record.window = window.to_owned();
    std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.path())
        .unwrap();
    writeln!(file, "{}", serde_json::to_string(&record).unwrap()).unwrap();
}

/// Trace: FR-025-AC-4
#[test]
fn newer_schema_stops_and_older_schema_is_read_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    write_record(&store, "2026-11-07T10:00:00Z", 0);
    let before = std::fs::read(store.path()).unwrap();
    assert_eq!(store.read_all().unwrap().len(), 1);
    assert_eq!(std::fs::read(store.path()).unwrap(), before);
    write_record(&store, "2026-11-07T11:00:00Z", 99);
    let error = store.read_all().unwrap_err();
    assert!(matches!(
        error,
        StoreError::NewerSchema {
            found: 99,
            line: 2,
            ..
        }
    ));
    assert!(error.to_string().contains("99"));
}

/// Trace: FR-025-AC-5
#[test]
fn every_record_carries_its_fields_and_no_credential() {
    let mut rig = base_rig();
    let mut data = rig.workspace.clone().unwrap().unwrap();
    data.issues[0].description = Some("token lin_api_SENTINEL".to_owned());
    rig.workspace = Some(Ok(data));
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap();
    let line = &lines(store.path())[0];
    let json: serde_json::Value = serde_json::from_str(line).unwrap();
    for field in [
        "schema_version",
        "kind",
        "taken_at",
        "team",
        "window",
        "metrics",
    ] {
        assert!(json.get(field).is_some(), "{field}");
    }
    assert!(!line.contains("SENTINEL"));
}

/// Trace: FR-025-AC-6
#[test]
fn an_unparsable_line_names_its_number_and_a_missing_final_newline_blocks_appends() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    write_record(&store, "2026-11-07T10:00:00Z", 1);
    let mut bytes = std::fs::read(store.path()).unwrap();
    bytes.extend_from_slice(b"{broken\n");
    std::fs::write(store.path(), &bytes).unwrap();
    let error = store.read_all().unwrap_err();
    assert!(matches!(error, StoreError::Line { line: 2, .. }), "{error}");
    assert_eq!(std::fs::read(store.path()).unwrap(), bytes);

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    write_record(&store, "2026-11-07T10:00:00Z", 1);
    let mut bytes = std::fs::read(store.path()).unwrap();
    bytes.pop();
    std::fs::write(store.path(), &bytes).unwrap();
    let rig = base_rig();
    let error = run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap_err();
    assert!(matches!(
        error,
        ix_board::retro::RetroError::Store(StoreError::MissingFinalNewline(_))
    ));
    assert!(error.to_string().contains("snapshots.jsonl"));
    assert_eq!(std::fs::read(store.path()).unwrap(), bytes);
}

/// Trace: FR-025-AC-7
#[test]
fn records_read_back_in_taken_at_order_with_file_order_breaking_ties() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    for (taken, window) in [
        ("2026-11-07T12:00:00Z", "a"),
        ("2026-11-07T09:00:00Z", "b"),
        ("2026-11-07T12:00:00Z", "c"),
        ("2026-11-07T10:30:00Z", "d"),
    ] {
        write_record_in(&store, taken, 1, window);
    }
    let order: Vec<_> = store
        .read_all()
        .unwrap()
        .into_iter()
        .map(|r| r.window)
        .collect();
    assert_eq!(order, ["b", "d", "a", "c"]);
}

// ---- FR-026 ----------------------------------------------------------------------------

/// Trace: FR-026-AC-1
#[test]
fn a_failing_github_source_makes_flow_unavailable_and_leaves_progress_computed() {
    let mut rig = base_rig();
    rig.prs = Some(Err("gh exited 1".to_owned()));
    let record = snapshot(&rig);
    for id in [
        "prs_merged_per_hour",
        "pr_cycle_median_hours",
        "pr_cycle_p90_hours",
        "prs_open",
        "prs_idle",
        "prs_oversize",
        "prs_ready_unmerged",
        "head_force_pushes_max",
    ] {
        assert!(reason(&record, id).contains("GitHub"), "{id}");
    }
    assert_eq!(value(&record, "path_remaining"), Some(1.0));
    assert_eq!(value(&record, "intake_new"), Some(2.0));
}

/// Trace: FR-026-AC-2
#[test]
fn a_listing_that_stops_early_makes_dependents_unavailable_not_smaller() {
    let mut rig = base_rig();
    let mut served = 0;
    rig.prs = Some(read_all_pages(|_| {
        served += 1;
        if served == 1 {
            Ok(Page {
                items: default_prs(),
                has_next: true,
                cursor: Some("c".to_owned()),
            })
        } else {
            Err("connection reset".to_owned())
        }
    }));
    let record = snapshot(&rig);
    assert!(reason(&record, "prs_open").contains("stopped before the last page"));
    assert!(reason(&record, "prs_merged_per_hour").contains("GitHub"));
}

/// Trace: FR-026-AC-3
#[test]
fn a_source_the_configuration_does_not_name_is_not_configured() {
    let record = snapshot(&base_rig());
    for id in [
        "worktrees_prunable",
        "orphan_slots",
        "full_gates_max",
        "messages_undelivered",
        "decisions_open",
    ] {
        assert_eq!(reason(&record, id), "not configured", "{id}");
    }
}

/// Trace: FR-026-AC-4
#[test]
fn window_and_threshold_changes_change_the_query() {
    let rig = base_rig();
    let query = |cfg: &RetroConfig, window: u64, id: &str| {
        snapshot_with(&rig, cfg, window).0.metrics[id].query.clone()
    };
    let base = config();
    assert_ne!(
        query(&base, DAY, "prs_merged_per_hour"),
        query(&base, 2 * DAY, "prs_merged_per_hour")
    );
    assert_ne!(
        query(&base, DAY, "path_closed"),
        query(&base, 3600, "path_closed")
    );
    let looser = config_with(
        r#"{"thresholds": {"pr_idle_hours": 48, "pr_oversize_lines": 1000, "in_progress_stale_hours": 48}}"#,
    );
    assert_ne!(
        query(&base, DAY, "prs_idle"),
        query(&looser, DAY, "prs_idle")
    );
    assert_eq!(
        query(&base, DAY, "prs_oversize"),
        query(&looser, DAY, "prs_oversize")
    );
    assert!(query(&base, DAY, "prs_idle").contains("pr_idle_hours=24"));
}

/// Trace: FR-026-AC-5
#[test]
fn unavailable_metrics_still_append_a_record_and_are_listed_for_stderr() {
    let mut rig = base_rig();
    rig.prs = Some(Err("down".to_owned()));
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let record = run_snapshot(&config(), DAY, NOW, &rig.sources(), &store).unwrap();
    assert_eq!(lines(store.path()).len(), 1);
    let notice = unavailable_notice(&record).unwrap();
    assert!(notice.contains("prs_open") && notice.contains("main_red_incidents"));
    assert!(!notice.contains("path_remaining"));
    assert_eq!(
        unavailable_notice(&Record {
            metrics: BTreeMap::new(),
            ..record
        }),
        None
    );
}

/// Trace: FR-026-AC-6
#[test]
fn a_median_over_no_merges_is_empty_but_a_merge_rate_is_zero() {
    let mut rig = base_rig();
    rig.prs = Some(Ok(vec![pr(11, true)]));
    let record = snapshot(&rig);
    assert_eq!(reason(&record, "pr_cycle_median_hours"), "empty population");
    assert_eq!(reason(&record, "pr_cycle_p90_hours"), "empty population");
    assert_eq!(value(&record, "prs_merged_per_hour"), Some(0.0));
}

#[test]
fn window_text_parses_to_seconds() {
    assert_eq!(parse_window("90s").unwrap(), 90);
    assert_eq!(parse_window("20m").unwrap(), 1200);
    assert_eq!(parse_window("4h").unwrap(), 14_400);
    assert_eq!(parse_window("2d").unwrap(), 172_800);
    for bad in ["", "h", "0h", "3w", "-1h", "1.5h", "24"] {
        assert!(parse_window(bad).is_err(), "{bad}");
    }
}

// ---- retro delta and backfill (FR-027, FR-028) -------------------------------------------

const QUERY: &str = "rule; window=1d";

/// A stored metric: `Some` is `ok` with that value, `None` is unavailable.
fn stored(id: &str, value: Option<f64>, query: &str) -> Metric {
    let entry = definition(id).unwrap();
    Metric {
        id: id.to_owned(),
        status: if value.is_some() {
            Status::Ok
        } else {
            Status::Unavailable
        },
        value,
        unit: entry.unit.to_owned(),
        direction: entry.direction,
        source: "fixture".to_owned(),
        query: query.to_owned(),
        reason: value.is_none().then(|| "fixture".to_owned()),
    }
}

fn record(kind: RecordKind, taken_at: &str, metrics: &[(&str, Option<f64>, &str)]) -> Record {
    Record {
        schema_version: 1,
        kind,
        taken_at: taken_at.to_owned(),
        team: "SWM".to_owned(),
        window: "1d".to_owned(),
        metrics: metrics
            .iter()
            .map(|(id, value, query)| ((*id).to_owned(), stored(id, *value, query)))
            .collect(),
    }
}

fn snap(taken_at: &str, metrics: &[(&str, Option<f64>, &str)]) -> Record {
    record(RecordKind::Snapshot, taken_at, metrics)
}

fn store_with(records: &[Record]) -> (TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    for record in records {
        store.append(record).unwrap();
    }
    (dir, store)
}

fn delta(records: &[Record], extra: &str) -> Result<DeltaReport, RetroError> {
    let (_dir, store) = store_with(records);
    run_delta(&config_with(extra), &store)
}

fn one(report: &DeltaReport, id: &str) -> ix_board::retro::delta::MetricDelta {
    report.metrics.iter().find(|m| m.id == id).unwrap().clone()
}

/// Three snapshots of `prs_idle` (down), `path_closed` (up) and `prs_open` (neutral).
fn trio(idle: [f64; 3]) -> Vec<Record> {
    let times = [
        "2026-11-05T12:00:00Z",
        "2026-11-06T12:00:00Z",
        "2026-11-07T12:00:00Z",
    ];
    (0..3)
        .map(|i| {
            snap(
                times[i],
                &[
                    ("prs_idle", Some(idle[i]), QUERY),
                    ("path_closed", Some([2.0, 5.0, 9.0][i]), QUERY),
                    ("prs_open", Some(idle[i]), QUERY),
                ],
            )
        })
        .collect()
}

/// Trace: FR-027-AC-1
#[test]
fn delta_reports_value_change_since_previous_and_since_the_earliest_record() {
    // A backfill record earlier than every snapshot is the default baseline; a stored order
    // that differs from time order must not matter.
    let early = record(
        RecordKind::Backfill,
        "2026-11-04T23:59:59Z",
        &[("path_closed", Some(1.0), QUERY)],
    );
    let mut records = trio([1.0, 1.0, 1.0]);
    records.reverse();
    records.push(early);
    let report = delta(&records, "{}").unwrap();
    assert_eq!(report.latest.as_deref(), Some("2026-11-07T12:00:00Z"));
    assert_eq!(report.previous.as_deref(), Some("2026-11-06T12:00:00Z"));
    assert_eq!(report.baseline.as_deref(), Some("2026-11-04T23:59:59Z"));
    let closed = one(&report, "path_closed");
    assert_eq!(closed.value, Some(9.0));
    assert_eq!(closed.since_previous.change, Some(4.0));
    assert_eq!(closed.since_baseline.change, Some(8.0));
    // Without the early record the earliest snapshot is the baseline.
    let report = delta(&trio([1.0, 1.0, 1.0]), "{}").unwrap();
    assert_eq!(one(&report, "path_closed").since_baseline.change, Some(7.0));
}

/// Trace: FR-027-AC-2
#[test]
fn a_metric_moving_against_its_direction_twice_in_a_row_is_regressing() {
    let report = delta(&trio([1.0, 2.0, 3.0]), "{}").unwrap();
    assert!(one(&report, "prs_idle").regressing);
    let report = delta(&trio([1.0, 3.0, 2.0]), "{}").unwrap();
    assert!(!one(&report, "prs_idle").regressing);
    let report = delta(&trio([3.0, 3.0, 4.0]), "{}").unwrap();
    assert!(
        !one(&report, "prs_idle").regressing,
        "a flat step is not a move"
    );
    // An `up` metric regresses when it falls twice.
    let falling: Vec<Record> = [5.0, 4.0, 3.0]
        .iter()
        .zip([
            "2026-11-05T12:00:00Z",
            "2026-11-06T12:00:00Z",
            "2026-11-07T12:00:00Z",
        ])
        .map(|(v, t)| snap(t, &[("path_closed", Some(*v), QUERY)]))
        .collect();
    assert!(one(&delta(&falling, "{}").unwrap(), "path_closed").regressing);
    // Two snapshots hold one comparison only.
    let two = trio([1.0, 2.0, 3.0])[1..].to_vec();
    assert!(!one(&delta(&two, "{}").unwrap(), "prs_idle").regressing);
}

/// Trace: FR-027-AC-3
#[test]
fn unavailable_and_query_changed_metrics_report_no_numeric_change() {
    let records = vec![
        snap(
            "2026-11-05T12:00:00Z",
            &[
                ("prs_idle", Some(1.0), "old rule"),
                ("path_closed", None, QUERY),
                ("prs_open", Some(1.0), QUERY),
            ],
        ),
        snap(
            "2026-11-06T12:00:00Z",
            &[
                ("prs_idle", Some(2.0), "new rule"),
                ("path_closed", Some(3.0), QUERY),
                ("prs_open", None, QUERY),
            ],
        ),
        snap(
            "2026-11-07T12:00:00Z",
            &[
                ("prs_idle", Some(5.0), "new rule"),
                ("path_closed", Some(6.0), QUERY),
                ("prs_open", Some(4.0), QUERY),
            ],
        ),
    ];
    let report = delta(&records, "{}").unwrap();
    let idle = one(&report, "prs_idle");
    assert_eq!(idle.since_previous.state, ChangeState::Changed);
    assert_eq!(idle.since_previous.change, Some(3.0));
    assert_eq!(idle.since_baseline.state, ChangeState::QueryChanged);
    assert_eq!(idle.since_baseline.change, None);
    let queries = idle.since_baseline.queries.unwrap();
    assert_eq!(
        (queries.latest.as_str(), queries.reference.as_str()),
        ("new rule", "old rule")
    );
    assert!(
        !idle.regressing,
        "an incomparable earlier step cannot regress"
    );
    // Unavailable on the baseline side.
    let closed = one(&report, "path_closed");
    assert_eq!(closed.since_baseline.state, ChangeState::Unavailable);
    assert_eq!(closed.since_baseline.change, None);
    // Unavailable on the previous side.
    let open = one(&report, "prs_open");
    assert_eq!(open.since_previous.state, ChangeState::Unavailable);
    assert_eq!(open.since_previous.change, None);
    // Unavailable on the latest side.
    let mut latest_down = records.clone();
    latest_down[2]
        .metrics
        .insert("path_closed".to_owned(), stored("path_closed", None, QUERY));
    let closed = one(&delta(&latest_down, "{}").unwrap(), "path_closed");
    assert_eq!(closed.status, Status::Unavailable);
    assert_eq!(closed.since_previous.state, ChangeState::Unavailable);
}

/// Trace: FR-027-AC-4
#[test]
fn a_neutral_metric_shows_changes_and_is_never_regressing() {
    for series in [[1.0, 2.0, 3.0], [3.0, 2.0, 1.0]] {
        let open = one(&delta(&trio(series), "{}").unwrap(), "prs_open");
        assert_eq!(open.direction, ix_board::retro::Direction::Neutral);
        assert!(open.since_previous.change.is_some());
        assert!(!open.regressing);
    }
}

/// Trace: FR-027-AC-5
#[test]
fn one_snapshot_reports_no_delta_and_exits_zero_without_a_score() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    store
        .append(&snap(
            "2026-11-07T12:00:00Z",
            &[("prs_idle", Some(1.0), QUERY)],
        ))
        .unwrap();
    let config = dir.path().join("config.json");
    std::fs::write(&config, CONFIG).unwrap();
    for json in [false, true] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"));
        command.args(["retro", "delta", "--config"]).arg(&config);
        if json {
            command.arg("--json");
        }
        let output = command.env("TEAM_RETRO_HOME", dir.path()).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("no delta"), "{text}");
        assert!(!text.to_lowercase().contains("score"), "{text}");
    }
    // A full delta carries no combined score either.
    let report = delta(&trio([1.0, 2.0, 3.0]), "{}").unwrap();
    let encoded = serde_json::to_string(&report).unwrap().to_lowercase();
    assert!(!encoded.contains("score") && !encoded.contains("health"));
    assert!(delta(&[], "{}").unwrap().message.is_some());
}

/// Trace: FR-027-AC-6
#[test]
fn delta_succeeds_from_the_store_alone_with_no_source_reachable() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    for record in trio([1.0, 2.0, 3.0]) {
        store.append(&record).unwrap();
    }
    let config = dir.path().join("config.json");
    std::fs::write(&config, CONFIG).unwrap();
    // No credentials, no PATH (so no gh, herdr, redis-cli) and no network-capable home.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(["retro", "delta", "--json", "--config"])
        .arg(&config)
        .env_clear()
        .env("TEAM_RETRO_HOME", dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["latest"], "2026-11-07T12:00:00Z");
    let idle = report["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "prs_idle")
        .unwrap();
    assert_eq!(idle["regressing"], true);
}

fn day(date: &str) -> f64 {
    ix_board::retro::parse_day(date).unwrap()
}

fn backfill_range(
    rig: &Rig,
    store: &Store,
    (since, until): (&str, &str),
    window: u64,
) -> Result<ix_board::retro::BackfillReport, RetroError> {
    run_backfill(
        &config(),
        window,
        (day(since), day(until)),
        NOW,
        (rig, rig),
        store,
    )
}

/// Trace: FR-027-AC-7
#[test]
fn a_backfill_after_a_snapshot_keeps_the_snapshot_latest_and_can_be_the_baseline() {
    let (_dir, store) = store_with(&[
        snap("2026-11-06T12:00:00Z", &[("path_closed", Some(1.0), QUERY)]),
        snap("2026-11-07T12:00:00Z", &[("path_closed", Some(4.0), QUERY)]),
    ]);
    let rig = base_rig();
    // Backfill days that fall before, and none of them inside, the snapshots.
    let done = backfill_range(&rig, &store, ("2026-11-01", "2026-11-03"), DAY).unwrap();
    assert_eq!(done.appended.len(), 3);
    let report = run_delta(&config(), &store).unwrap();
    assert_eq!(report.latest.as_deref(), Some("2026-11-07T12:00:00Z"));
    assert_eq!(report.previous.as_deref(), Some("2026-11-06T12:00:00Z"));
    assert_eq!(report.baseline.as_deref(), Some("2026-11-01T23:59:59Z"));
    assert_eq!(report.baseline_kind, Some(RecordKind::Backfill));
    // A backfill record named in the configuration is the baseline too.
    let named = run_delta(
        &config_with(r#"{"baseline_taken_at": "2026-11-03T23:59:59Z"}"#),
        &store,
    )
    .unwrap();
    assert_eq!(named.baseline.as_deref(), Some("2026-11-03T23:59:59Z"));
    assert_eq!(named.baseline_kind, Some(RecordKind::Backfill));
}

/// Trace: FR-027-AC-8
#[test]
fn a_baseline_matching_no_record_stops_with_an_error_naming_it() {
    let error = delta(
        &trio([1.0, 2.0, 3.0]),
        r#"{"baseline_taken_at": "2026-11-05T12:00:01Z"}"#,
    )
    .unwrap_err();
    assert!(matches!(error, RetroError::BaselineNotFound(_)));
    assert!(
        error.to_string().contains("2026-11-05T12:00:01Z"),
        "{error}"
    );
    // The exact time matches.
    let report = delta(
        &trio([1.0, 2.0, 3.0]),
        r#"{"baseline_taken_at": "2026-11-05T12:00:00Z"}"#,
    )
    .unwrap();
    assert_eq!(report.baseline.as_deref(), Some("2026-11-05T12:00:00Z"));
}

/// Trace: FR-028-AC-1
#[test]
fn a_three_day_range_appends_three_records_at_the_end_of_each_utc_day() {
    let (_dir, store) = store_with(&[]);
    let report = backfill_range(&base_rig(), &store, ("2026-11-02", "2026-11-04"), DAY).unwrap();
    let stored = store.read_all().unwrap();
    assert_eq!(report.appended.len(), 3);
    assert_eq!(
        stored
            .iter()
            .map(|r| r.taken_at.as_str())
            .collect::<Vec<_>>(),
        [
            "2026-11-02T23:59:59Z",
            "2026-11-03T23:59:59Z",
            "2026-11-04T23:59:59Z"
        ]
    );
    assert!(
        stored
            .iter()
            .all(|r| r.kind == RecordKind::Backfill && r.window == "1d")
    );
    // The default window is the snapshot default; a different window is recorded.
    let other =
        backfill_range(&base_rig(), &store, ("2026-11-05", "2026-11-05"), 6 * 3600).unwrap();
    assert_eq!(other.appended[0].window, "6h");
    // A range ending today is refused, since the day has not ended.
    assert!(matches!(
        backfill_range(&base_rig(), &store, ("2026-11-06", "2026-11-07"), DAY),
        Err(RetroError::FutureDay(_))
    ));
    assert!(matches!(
        backfill_range(&base_rig(), &store, ("2026-11-06", "2026-11-05"), DAY),
        Err(RetroError::EmptyRange)
    ));
}

/// Trace: FR-028-AC-2
#[test]
fn running_the_same_range_twice_appends_nothing_and_reports_each_skipped_day() {
    let (_dir, store) = store_with(&[]);
    let rig = base_rig();
    backfill_range(&rig, &store, ("2026-11-02", "2026-11-04"), DAY).unwrap();
    let before = lines(store.path());
    let again = Rig {
        // Sources that fail loudly if the second run reads them.
        workspace: Some(Err("must not be read".to_owned())),
        prs: Some(Err("must not be read".to_owned())),
        ..Rig::default()
    };
    let second = backfill_range(&again, &store, ("2026-11-02", "2026-11-04"), DAY).unwrap();
    assert!(second.appended.is_empty());
    assert_eq!(
        second.skipped,
        [
            "2026-11-02T23:59:59Z",
            "2026-11-03T23:59:59Z",
            "2026-11-04T23:59:59Z"
        ]
    );
    assert_eq!(lines(store.path()), before);
    assert!(again.calls.borrow().is_empty());
    // A partly new range fills only the missing days.
    let wider = backfill_range(&rig, &store, ("2026-11-03", "2026-11-05"), DAY).unwrap();
    assert_eq!(wider.skipped.len(), 2);
    assert_eq!(wider.appended.len(), 1);
    assert_eq!(wider.appended[0].taken_at, "2026-11-05T23:59:59Z");
}

/// Trace: FR-028-AC-3
#[test]
fn live_state_metrics_are_not_reconstructible_in_every_backfill_record() {
    let (_dir, store) = store_with(&[]);
    let report = backfill_range(&base_rig(), &store, ("2026-11-02", "2026-11-04"), DAY).unwrap();
    const KEPT: [&str; 7] = [
        "prs_merged_per_hour",
        "pr_cycle_median_hours",
        "pr_cycle_p90_hours",
        "path_closed",
        "intake_new",
        "net_closed_minus_new",
        "done_reopened",
    ];
    for record in &report.appended {
        assert_eq!(record.metrics.len(), CATALOG.len());
        for entry in CATALOG.iter().filter(|e| !KEPT.contains(&e.id)) {
            assert_eq!(
                reason(record, entry.id),
                "not reconstructible",
                "{} on {}",
                entry.id,
                record.taken_at
            );
        }
    }
    for id in [
        "path_remaining",
        "agents_working_share",
        "in_progress_stale",
        "prs_open",
    ] {
        assert_eq!(reason(&report.appended[0], id), "not reconstructible");
    }
}

/// Trace: FR-028-AC-4
#[test]
fn a_range_containing_a_snapshot_is_refused_naming_it_and_appends_nothing() {
    let snapshot_at = "2026-11-03T12:00:00Z";
    let (_dir, store) = store_with(&[snap(snapshot_at, &[("path_closed", Some(1.0), QUERY)])]);
    let before = lines(store.path());
    let rig = base_rig();
    let error = backfill_range(&rig, &store, ("2026-11-02", "2026-11-04"), DAY).unwrap_err();
    assert!(matches!(error, RetroError::SnapshotInRange(_)));
    assert!(error.to_string().contains(snapshot_at), "{error}");
    assert_eq!(lines(store.path()), before);
    assert!(
        rig.calls.borrow().is_empty(),
        "no source read for a refused range"
    );
    // The days either side of the snapshot's day are fine.
    assert!(backfill_range(&rig, &store, ("2026-11-02", "2026-11-02"), DAY).is_ok());
    assert!(backfill_range(&rig, &store, ("2026-11-04", "2026-11-04"), DAY).is_ok());
    // A snapshot exactly at the next midnight lies outside the last day.
    let (_dir2, edge) = store_with(&[snap("2026-11-03T00:00:00Z", &[])]);
    assert!(backfill_range(&rig, &edge, ("2026-11-02", "2026-11-02"), DAY).is_ok());
}

fn at(date: &str, hours_after_midnight: f64) -> f64 {
    day(date) + hours_after_midnight * HOUR
}

fn merged_at(number: u64, merged: f64, cycle_hours: f64) -> PullRequest {
    PullRequest {
        merged_at: Some(merged),
        created_at: merged - cycle_hours * HOUR,
        updated_at: merged,
        ..pr(number, false)
    }
}

/// Issue created and last updated at absolute times.
fn issue_at(id: &str, state: S, project: Option<&str>, created: f64, updated: f64) -> Issue {
    let mut issue = issue(id, state, project, 0.0, 0.0);
    issue.created_at = Some(iso(created));
    issue.updated_at = Some(iso(updated));
    issue
}

fn change_at(at: f64, from: S, to: S) -> StateChange {
    StateChange {
        at,
        from: Some(from),
        to: Some(to),
    }
}

/// Trace: FR-028-AC-5
#[test]
fn pull_request_metrics_equal_the_known_values_of_the_trailing_window() {
    let end = at("2026-11-03", 24.0) - 1.0;
    let mut rig = base_rig();
    rig.prs = Some(Ok(vec![
        merged_at(1, end - 1.0 * HOUR, 1.0),
        merged_at(2, end - 5.0 * HOUR, 3.0),
        merged_at(3, end - 10.0 * HOUR, 5.0),
        // First second of the day is inside; the last second of the day before is not.
        merged_at(4, at("2026-11-03", 0.0), 10.0),
        merged_at(5, at("2026-11-03", 0.0) - 2.0, 1000.0),
        // The first second of the next day belongs to the next window.
        merged_at(6, end + 1.0, 1000.0),
        pr(7, true),
    ]));
    let (_dir, store) = store_with(&[]);
    let report = backfill_range(&rig, &store, ("2026-11-03", "2026-11-03"), DAY).unwrap();
    let record = &report.appended[0];
    assert_eq!(value(record, "prs_merged_per_hour"), Some(4.0 / 24.0));
    assert_eq!(value(record, "pr_cycle_median_hours"), Some(3.0));
    assert_eq!(value(record, "pr_cycle_p90_hours"), Some(10.0));
    // Pull requests are read once for the whole range, never open-ended.
    assert_eq!(rig.count("pull_requests:agent-ix/x"), 1);
}

/// Trace: FR-028-AC-5
#[test]
fn issue_metrics_are_reconstructed_from_creation_and_state_history() {
    let d = "2026-11-03";
    let mut rig = base_rig();
    let later = at("2026-11-06", 0.0);
    rig.workspace = Some(Ok(WorkspaceSnapshot {
        projects: vec![project("p1", "Alpha")],
        issues: vec![
            blocked_by(
                issue_at(
                    "SWM-1",
                    S::Started,
                    Some("p1"),
                    at("2026-10-01", 0.0),
                    later,
                ),
                &["SWM-2"],
            ),
            // path issue completed inside the day
            issue_at(
                "SWM-2",
                S::Completed,
                Some("p1"),
                at("2026-10-01", 0.0),
                later,
            ),
            // created inside the day, still open
            issue_at("SWM-3", S::Started, Some("p1"), at(d, 3.0), later),
            // completed inside the day, not on the path
            issue_at(
                "SWM-7",
                S::Completed,
                Some("p1"),
                at("2026-10-02", 0.0),
                later,
            ),
            // completed on the next day: outside
            issue_at(
                "SWM-4",
                S::Completed,
                Some("p1"),
                at("2026-10-02", 0.0),
                later,
            ),
            // created the day before: outside intake
            issue_at(
                "SWM-6",
                S::Unstarted,
                Some("p1"),
                at("2026-11-02", 23.0),
                later,
            ),
            // reopened inside the day
            issue_at(
                "SWM-5",
                S::Started,
                Some("p1"),
                at("2026-10-02", 0.0),
                later,
            ),
        ],
        fetched_at: 0,
    }));
    rig.histories = BTreeMap::from([
        (
            "id-SWM-2",
            vec![change_at(at(d, 5.0), S::Started, S::Completed)],
        ),
        (
            "id-SWM-7",
            vec![change_at(at(d, 9.0), S::Started, S::Completed)],
        ),
        (
            "id-SWM-4",
            vec![change_at(at("2026-11-04", 0.0), S::Started, S::Completed)],
        ),
        (
            "id-SWM-5",
            vec![change_at(at(d, 12.0), S::Completed, S::Started)],
        ),
    ])
    .into_iter()
    .map(|(k, v)| (k.to_owned(), Ok(v)))
    .collect();
    let (_dir, store) = store_with(&[]);
    let report = backfill_range(&rig, &store, (d, d), DAY).unwrap();
    let record = &report.appended[0];
    assert_eq!(value(record, "path_closed"), Some(1.0));
    assert_eq!(value(record, "intake_new"), Some(1.0));
    assert_eq!(value(record, "net_closed_minus_new"), Some(1.0));
    assert_eq!(value(record, "done_reopened"), Some(1.0));
    // An unreadable history makes the dependent metrics unavailable, never smaller.
    rig.histories
        .insert("id-SWM-7".to_owned(), Err("page 3 failed".to_owned()));
    let (_dir2, store2) = store_with(&[]);
    let record = &backfill_range(&rig, &store2, (d, d), DAY).unwrap().appended[0];
    assert!(reason(record, "net_closed_minus_new").contains("SWM-7"));
    assert!(reason(record, "done_reopened").contains("SWM-7"));
    assert_eq!(value(record, "path_closed"), Some(1.0));
    // An incomplete pull-request read makes the flow metrics unavailable.
    rig.prs = Some(Err("stopped before the last page (page 2)".to_owned()));
    let (_dir3, store3) = store_with(&[]);
    let record = &backfill_range(&rig, &store3, (d, d), DAY).unwrap().appended[0];
    assert!(reason(record, "prs_merged_per_hour").contains("last page"));
}

/// Trace: FR-028-AC-6
#[test]
fn a_day_without_merged_pull_requests_has_a_zero_rate_and_empty_cycle_metrics() {
    let mut rig = base_rig();
    // Merges exist, but only on the neighbouring days.
    rig.prs = Some(Ok(vec![
        merged_at(1, at("2026-11-02", 23.0), 2.0),
        merged_at(2, at("2026-11-04", 1.0), 2.0),
        pr(3, true),
    ]));
    let (_dir, store) = store_with(&[]);
    let report = backfill_range(&rig, &store, ("2026-11-03", "2026-11-03"), DAY).unwrap();
    let record = &report.appended[0];
    assert_eq!(value(record, "prs_merged_per_hour"), Some(0.0));
    assert_eq!(reason(record, "pr_cycle_median_hours"), "empty population");
    assert_eq!(reason(record, "pr_cycle_p90_hours"), "empty population");
}

/// Trace: FR-028-AC-7
#[test]
fn backfill_queries_carry_the_window_and_differ_from_a_snapshot_of_another_window() {
    let (_dir, store) = store_with(&[]);
    let rig = base_rig();
    let backfilled = backfill_range(&rig, &store, ("2026-11-03", "2026-11-03"), 6 * 3600).unwrap();
    for id in [
        "prs_merged_per_hour",
        "path_closed",
        "intake_new",
        "done_reopened",
    ] {
        assert!(
            backfilled.appended[0].metrics[id]
                .query
                .contains("window=6h"),
            "{id}"
        );
    }
    for (index, now) in [NOW, NOW + HOUR].into_iter().enumerate() {
        let record = run_snapshot(&config(), DAY, now, &rig.sources(), &store).unwrap();
        assert!(
            record.metrics["prs_merged_per_hour"]
                .query
                .contains("window=1d"),
            "{index}"
        );
    }
    let report = run_delta(&config(), &store).unwrap();
    assert_eq!(report.baseline_kind, Some(RecordKind::Backfill));
    let rate = one(&report, "prs_merged_per_hour");
    assert_eq!(rate.since_baseline.state, ChangeState::QueryChanged);
    assert_eq!(rate.since_baseline.change, None);
    let queries = rate.since_baseline.queries.unwrap();
    assert!(queries.reference.contains("window=6h") && queries.latest.contains("window=1d"));
    assert_eq!(rate.since_previous.state, ChangeState::Changed);
}
