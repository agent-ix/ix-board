// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only source traits behind which every retro reader sits (FR-024).
//!
//! Every method only reads. Errors are plain reason strings: the caller turns them into
//! `unavailable` metrics (FR-026) and never into a zero.

use std::path::Path;

use crate::agents::AgentsReport;
use crate::{WorkflowStateType, WorkspaceSnapshot};

/// Largest number of pages one paginated pass may read.
pub const PAGE_LIMIT: usize = 1_000;

/// One page of a paginated listing.
#[derive(Clone, Debug)]
pub struct Page<T> {
    /// Items on this page.
    pub items: Vec<T>,
    /// True when the source reports another page.
    pub has_next: bool,
    /// Cursor naming the next page.
    pub cursor: Option<String>,
}

/// Reads every page once. A pass that cannot reach the last page is an error, never a
/// shorter list.
pub fn read_all_pages<T>(
    mut fetch: impl FnMut(Option<&str>) -> Result<Page<T>, String>,
) -> Result<Vec<T>, String> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    for number in 1..=PAGE_LIMIT {
        let page = fetch(cursor.as_deref())
            .map_err(|error| format!("stopped before the last page (page {number}): {error}"))?;
        items.extend(page.items);
        if !page.has_next {
            return Ok(items);
        }
        cursor = Some(page.cursor.ok_or_else(|| {
            format!("stopped before the last page (page {number} named no next cursor)")
        })?);
    }
    Err(format!(
        "stopped before the last page ({PAGE_LIMIT}-page limit)"
    ))
}

/// One workflow-state change of an issue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateChange {
    /// Unix seconds of the change.
    pub at: f64,
    /// Category left, when known.
    pub from: Option<WorkflowStateType>,
    /// Category entered, when known.
    pub to: Option<WorkflowStateType>,
}

/// An open issue carrying the decision label.
#[derive(Clone, Debug, PartialEq)]
pub struct LabeledIssue {
    /// Issue identifier.
    pub identifier: String,
    /// Unix seconds the issue was created.
    pub created_at: f64,
}

/// Linear reads.
pub trait LinearSource {
    /// The complete workspace snapshot, read in one paginated pass.
    fn workspace(&self) -> Result<WorkspaceSnapshot, String>;
    /// The open ticket count the project-chunk read reports for a project name.
    fn project_open_count(&self, project: &str) -> Result<usize, String>;
    /// The complete state history of one issue.
    fn state_history(&self, issue_id: &str) -> Result<Vec<StateChange>, String>;
    /// First lines of the comments that begin with a `reviewer` marker.
    fn reviewer_marker_lines(&self) -> Result<Vec<String>, String>;
    /// Open issues carrying a label.
    fn open_issues_with_label(&self, label: &str) -> Result<Vec<LabeledIssue>, String>;
}

/// A pull request with the fields the catalog reads.
#[derive(Clone, Debug, PartialEq)]
pub struct PullRequest {
    /// `owner/name`.
    pub repo: String,
    /// Pull request number.
    pub number: u64,
    /// True while open.
    pub open: bool,
    /// True when a draft.
    pub draft: bool,
    /// Unix seconds of creation.
    pub created_at: f64,
    /// Unix seconds of merge.
    pub merged_at: Option<f64>,
    /// Unix seconds of the last update.
    pub updated_at: f64,
    /// Added lines.
    pub additions: u64,
    /// Deleted lines.
    pub deletions: u64,
    /// GitHub review decision, such as `APPROVED`.
    pub review_decision: Option<String>,
    /// Distinct head commits from the paginated timeline; `None` when it was incomplete.
    pub head_shas: Option<u64>,
}

/// Check state of one commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckState {
    /// Every check run passed.
    Pass,
    /// A check run failed.
    Fail,
    /// Checks have not settled or none exist.
    Pending,
}

/// A commit on the main-check branch with its check state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MainCommit {
    /// Unix seconds of the commit.
    pub committed_at: f64,
    /// Check state.
    pub state: CheckState,
}

/// GitHub reads.
pub trait GithubSource {
    /// Every open pull request of `repo`, plus merged ones updated at or after `since`,
    /// read in one paginated pass.
    fn pull_requests(&self, repo: &str, since: f64) -> Result<Vec<PullRequest>, String>;
    /// Whether one pull request is merged.
    fn is_merged(&self, repo: &str, number: u64) -> Result<bool, String>;
    /// Check state of every commit on `branch` committed at or after `since`.
    fn main_commits(&self, repo: &str, branch: &str, since: f64)
    -> Result<Vec<MainCommit>, String>;
}

/// Herdr agent activity.
pub trait AgentSource {
    /// The FR-018 report for the workspace labels.
    fn report(&self, teams: &[String]) -> Result<AgentsReport, String>;
}

/// One line of the gate log.
#[derive(Clone, Debug, PartialEq)]
pub struct GateLine {
    /// Pull request the run belongs to.
    pub pr: String,
    /// True for a `repin` line, false for a `gate` line.
    pub repin: bool,
    /// Unix seconds the lock was requested.
    pub requested_lock_at: f64,
    /// Unix seconds the lock was acquired.
    pub acquired_lock_at: f64,
    /// Unix seconds the run started.
    pub started_at: f64,
    /// Unix seconds the run finished.
    pub finished_at: f64,
}

/// Gate log reads.
pub trait GateLogSource {
    /// Every line of the JSONL gate log.
    fn lines(&self, path: &Path) -> Result<Vec<GateLine>, String>;
}

/// ix-relay queue reads.
pub trait RelaySource {
    /// Send times, in Unix milliseconds, of the messages waiting for `address`, without
    /// dequeuing them.
    fn pending_sent_at_ms(&self, address: &str) -> Result<Vec<i64>, String>;
}

/// One row of the local process table.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessRow {
    /// Command line.
    pub command: String,
    /// True while the parent process is alive.
    pub parent_alive: bool,
    /// Seconds since the process started.
    pub age_secs: u64,
}

/// Local git, disk and process reads.
pub trait LocalSource {
    /// Prunable worktrees git reports for a directory.
    fn prunable_worktrees(&self, dir: &Path) -> Result<u64, String>;
    /// Total bytes below a directory.
    fn disk_bytes(&self, dir: &Path) -> Result<u64, String>;
    /// The local process table.
    fn processes(&self) -> Result<Vec<ProcessRow>, String>;
}

/// Every source one snapshot reads.
pub struct Sources<'a> {
    /// Linear reader.
    pub linear: &'a dyn LinearSource,
    /// GitHub reader.
    pub github: &'a dyn GithubSource,
    /// Herdr reader.
    pub agents: &'a dyn AgentSource,
    /// Gate log reader.
    pub gate_log: &'a dyn GateLogSource,
    /// Relay queue reader.
    pub relay: &'a dyn RelaySource,
    /// Local reader.
    pub local: &'a dyn LocalSource,
}
