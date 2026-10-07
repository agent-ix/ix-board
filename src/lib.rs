// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic Linear project controls and cached reads for ready work, context, and search.

mod cache;
mod drift;
mod hygiene;
mod linear;
mod model;
mod scope;
pub use scope::{ScopeSelection, ScopeSummary, select_scope};

use cache::SnapshotCache;
pub use cache::{CACHE_TTL_SECONDS, Clock, SystemClock};
pub use drift::{
    AlertKind, Availability, BusyWithoutPrSignal, DRIFT_EVIDENCE_SCHEMA_VERSION, DriftAlert,
    DriftCommit, DriftConfig, DriftError, DriftEvidence, DriftEvidenceFile, DriftLead,
    DriftPullRequest, DriftReport, DriftTicket, HistoricalProject, HistoricalTicket,
    IssueTransition, LeadDriftReport, LeadSignals, LeadStatus, MissingEvidence, PrScopeSignal,
    QueueEvidence, RunningProject, ScopeClassification, ScopedPullRequest, SignalAvailability,
    StallSignal, StalledTicket, TicketChurnSignal, UnassignedPullRequest, UnassignedReason,
    VeeringReason, VeeringSignal, VeeringTicket, evaluate_drift, evaluate_drift_file,
    render_drift_text,
};
pub use hygiene::{
    DuplicateTitleFinding, FocusBaseline, FocusBaselineIssue, FocusChange, FocusHygieneReport,
    FocusMembershipChange, GroupStatus, HygieneError, HygieneGroup, IssueReference,
    MissingScopeFinding, OverdueMilestoneFinding, PullRequestFinding, PullRequestMismatchKind,
    StaleIssueFinding, TITLE_PAIR_LIMIT,
};
pub use linear::LinearGraphQlSource;
pub use model::{
    ChunkIssue, FocusBlocker, FocusExpansion, FocusIssue, FocusIssueClassification, FocusMilestone,
    FocusMilestoneStatus, FocusPlanChunk, FocusPlanIssue, FocusProgress, FocusProject,
    FocusProjectStatus, FocusSelection, FocusStateCount, FocusStatus, FocusUnmilestonedStatus,
    Initiative, Issue, IssueId, Milestone, NextStep, Project, ProjectChunk, ProjectId,
    ProjectPriority, ProjectRunStatus, ReadyItem, SearchKind, SearchResult, TeamKey,
    WorkflowStateType, WorkspaceSnapshot,
};

use model::{compare_f64, issue_is_open};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use thiserror::Error as ThisError;

/// Errors returned by the library's read and cache boundaries.
#[derive(Debug, ThisError)]
pub enum Error {
    /// A referenced ancestor was absent from the complete snapshot.
    #[error("ticket {identifier} has an unavailable parent {parent:?}")]
    MissingParent {
        /// Descendant whose ancestry was incomplete.
        identifier: String,
        /// Missing parent identity.
        parent: IssueId,
    },
    /// A scope selector did not resolve to one authoritative record.
    #[error("scope {kind} {identifier:?} resolved to {matches} records")]
    ScopeSelection {
        /// Selected record kind.
        kind: &'static str,
        /// User supplied identifier.
        identifier: String,
        /// Number of distinct matches.
        matches: usize,
    },
    /// The parent graph contains a cycle and cannot define leaf tasks.
    #[error("issue parent graph contains a cycle at {identifier}")]
    ParentCycle {
        /// Ticket where the cycle was detected.
        identifier: String,
    },
    /// A Linear HTTP request failed.
    #[error("Linear request failed: {0}")]
    Http(#[from] reqwest::Error),
    /// Linear exhausted a request or complexity quota.
    #[error("Linear rate limited; retry in {retry_after:?}")]
    LinearRateLimited {
        /// Minimum cooldown before attempting another source request.
        retry_after: std::time::Duration,
    },
    /// Neither an environment override nor a saved Linear credential exists.
    #[error("Linear API key is missing; run `ix-board auth login` or set LINEAR_API_KEY")]
    MissingApiKey,
    /// The shared credential store could not resolve the Linear key.
    #[error("Linear credential error: {0}")]
    Credential(#[from] ix_cli_kit::secrets::SecretError),
    /// Interactive or stdin credential input failed.
    #[error("could not read Linear API key: {0}")]
    CredentialInput(#[source] std::io::Error),
    /// The entered credential contains no non-whitespace characters.
    #[error("Linear API key must not be empty")]
    EmptyCredentialInput,
    /// The platform did not provide an application cache directory.
    #[error("could not determine the ix-board cache directory")]
    CacheDirectoryUnavailable,
    /// The cache directory could not be created.
    #[error("could not create cache directory: {0}")]
    CacheDirectory(#[source] std::io::Error),
    /// Linear returned GraphQL errors.
    #[error("Linear GraphQL error: {messages}")]
    GraphQl { messages: String },
    /// Linear returned a response without workspace data.
    #[error("the Linear source returned no workspace data")]
    MissingWorkspaceData,
    /// The server exceeded the bounded pagination budget.
    #[error("Linear pagination exceeded the {pages}-page safety limit")]
    PaginationLimit { pages: usize },
    /// A nested GraphQL connection exceeded the supported single page.
    #[error("Linear connection exceeded the supported page size: {resource}")]
    NestedPaginationLimit { resource: String },
    /// A required pagination cursor was absent.
    #[error("Linear indicated another page but returned no cursor")]
    MissingCursor,
    /// The server returned more than one relation page for an issue.
    #[error("issue {issue} has more relations than the supported page size")]
    RelationsLimit { issue: String },
    /// SQLite could not read or write the cache.
    #[error("SQLite cache error: {0}")]
    Cache(#[source] rusqlite::Error),
    /// The cache database permissions could not be restricted.
    #[error("could not restrict cache database permissions: {0}")]
    CachePermissions(#[source] std::io::Error),
    /// A cached timestamp could not be represented safely.
    #[error("cached timestamp overflow")]
    CacheTimestamp,
    /// Cached snapshot bytes could not be decoded.
    #[error("cached snapshot is invalid: {0}")]
    CacheDecode(#[source] serde_json::Error),
    /// A snapshot could not be encoded for cache storage.
    #[error("snapshot could not be encoded: {0}")]
    CacheEncode(#[source] serde_json::Error),
    /// The requested team value was empty.
    #[error("team identifier must not be empty")]
    InvalidTeam,
    /// The requested search query was empty.
    #[error("search query must not be empty")]
    InvalidQuery,
    /// Drift config/evidence input failed to read.
    #[error("could not read drift input {path}: {source}")]
    DriftInputRead {
        /// Input path.
        path: String,
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// Drift config/evidence input failed its strict JSON contract.
    #[error("invalid drift JSON in {path}: {source}")]
    DriftInputDecode {
        /// Input path.
        path: String,
        /// Underlying JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// Drift configuration or evaluation failed.
    #[error(transparent)]
    Drift(#[from] drift::DriftError),
    /// The requested project was not present in the source snapshot.
    #[error("project not found: {identifier}")]
    ProjectNotFound { identifier: String },
    /// A project status token was not one of the supported values.
    #[error("invalid project status '{value}'; expected Started or Paused")]
    InvalidProjectStatus { value: String },
    /// A project priority token was not one of the exact Linear priority values.
    #[error("invalid project priority '{value}'; expected one of 0, 1, 2, 3, or 4")]
    InvalidProjectPriority { value: String },
    /// A manual project order value must be finite.
    #[error("project sort order must be finite")]
    InvalidProjectSortOrder,
    /// No supported status is configured in the current Linear workspace.
    #[error("Linear project status '{status}' is unavailable in this workspace")]
    ProjectStatusUnavailable { status: String },
    /// Multiple supported statuses match the requested status and scope.
    #[error("Linear project status '{status}' is ambiguous in this workspace")]
    ProjectStatusAmbiguous { status: String },
    /// Linear rejected a project update or returned no updated project.
    #[error("Linear rejected the project update for {identifier}")]
    ProjectUpdateRejected { identifier: String },
    /// A referenced issue was absent from the complete snapshot.
    #[error("dependency issue {issue} is missing from the Linear snapshot")]
    MissingDependency { issue: String },
    /// The blocker graph contains a cycle that cannot be layered.
    #[error("dependency graph contains a cycle involving: {issues}")]
    DependencyCycle { issues: String },
    /// No inline initiative, project, or issue selector was supplied.
    #[error("focus requires at least one --initiative, --project, or --issue selector")]
    EmptyFocus,
    /// An issue's required structured focus data was unavailable.
    #[error("focus plan data is incomplete for {issue}: missing {field}")]
    FocusPlanIncomplete {
        /// Issue whose source record was incomplete.
        issue: String,
        /// Missing structured field or related record.
        field: &'static str,
    },
    /// A focus hygiene report exceeded a deterministic resource bound.
    #[error("focus hygiene title candidates exceeded resource limit {limit}")]
    FocusHygieneResourceLimit { limit: usize },
    /// A required source record for the hygiene report was incomplete.
    #[error("focus hygiene record is unavailable: {issue}")]
    FocusHygieneIncomplete { issue: String },
    /// The report timestamp or stale threshold is invalid.
    #[error("focus hygiene requires an RFC 3339 report timestamp and positive stale threshold")]
    InvalidFocusHygieneTime,
    /// An issue references a milestone absent from its selected project's records.
    #[error("issue {issue} references unavailable milestone {milestone} in project {project}")]
    MissingProjectMilestone {
        /// Human-readable issue identifier.
        issue: String,
        /// Missing milestone UUID.
        milestone: String,
        /// Project UUID.
        project: String,
    },
}

/// Injectable source of complete structured workspace snapshots.
pub trait WorkspaceSource: Send + Sync {
    /// Fetches all projects and issues needed for cross-team dependency resolution.
    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error>;

    /// Identifies the source data for cache isolation. Fixture sources share a stable default.
    fn cache_namespace(&self) -> String {
        "fixture-default".to_owned()
    }
}

/// Source capable of updating the Linear project fields ix-board owns.
pub trait ProjectMutationSource: WorkspaceSource {
    /// Sets one project's status to Started or Paused.
    fn set_project_status(
        &self,
        project_id: &ProjectId,
        status: ProjectRunStatus,
    ) -> Result<(), Error>;

    /// Sets one project's priority to a supported Linear priority.
    fn set_project_priority(
        &self,
        project_id: &ProjectId,
        priority: ProjectPriority,
    ) -> Result<(), Error>;

    /// Sets one project's manual order within its priority group.
    fn set_project_sort_order(&self, project_id: &ProjectId, sort_order: f64) -> Result<(), Error>;
}

/// Cached read service shared by the CLI and library consumers.
pub struct ReadService<S, C = SystemClock> {
    source: S,
    cache: SnapshotCache,
    clock: C,
}

impl<S: WorkspaceSource> ReadService<S, SystemClock> {
    /// Opens a service using ix-board's platform cache location, honoring
    /// `IX_BOARD_CACHE_PATH` when it is set.
    pub fn open_default(source: S) -> Result<Self, Error> {
        Self::open(source, &default_cache_path()?)
    }

    /// Opens a service with the system clock and a SQLite cache at cache_path.
    pub fn open(source: S, cache_path: &Path) -> Result<Self, Error> {
        Self::with_clock(source, cache_path, SystemClock)
    }
}

/// Returns ix-board's SQLite cache path, creating the parent directory for the
/// platform default.
///
/// `IX_BOARD_CACHE_PATH` overrides the platform location. Without that override,
/// Linux uses `$XDG_CACHE_HOME/ix-board` or `$HOME/.cache/ix-board`, macOS uses
/// `$HOME/Library/Caches/com.agent-ix.ix-board`, and Windows uses
/// `%LOCALAPPDATA%/agent-ix/ix-board/cache` (falling back to the conventional
/// path under `%USERPROFILE%`). On Unix, an absolute `XDG_CACHE_HOME` is enough
/// even when `HOME` is unset; otherwise `HOME` must be available. This function
/// returns [`Error::CacheDirectoryUnavailable`] rather than guessing a home path
/// when neither standard environment variable is available. The standard library
/// has no portable API for resolving the operating system account database, so
/// callers in stripped environments can provide `XDG_CACHE_HOME` explicitly.
pub fn default_cache_path() -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("IX_BOARD_CACHE_PATH") {
        return Ok(PathBuf::from(path));
    }
    let cache_dir = platform_cache_dir()?;
    std::fs::create_dir_all(&cache_dir).map_err(Error::CacheDirectory)?;
    Ok(cache_dir.join("linear.sqlite3"))
}

#[cfg(target_os = "windows")]
fn platform_cache_dir() -> Result<PathBuf, Error> {
    let local_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join("AppData").join("Local")))
        .ok_or(Error::CacheDirectoryUnavailable)?;
    Ok(local_data.join("agent-ix").join("ix-board").join("cache"))
}

#[cfg(target_os = "macos")]
fn platform_cache_dir() -> Result<PathBuf, Error> {
    Ok(home_dir()
        .ok_or(Error::CacheDirectoryUnavailable)?
        .join("Library")
        .join("Caches")
        .join("com.agent-ix.ix-board"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_cache_dir() -> Result<PathBuf, Error> {
    let xdg_cache_home = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
    unix_cache_dir(home_dir().as_deref(), xdg_cache_home.as_deref())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn unix_cache_dir(home: Option<&Path>, xdg_cache_home: Option<&Path>) -> Result<PathBuf, Error> {
    if let Some(cache_home) = xdg_cache_home.filter(|path| path.is_absolute()) {
        return Ok(cache_home.join("ix-board"));
    }
    home.map(|path| path.join(".cache").join("ix-board"))
        .ok_or(Error::CacheDirectoryUnavailable)
}

#[cfg(all(test, not(any(target_os = "windows", target_os = "macos"))))]
mod cache_path_tests {
    use super::unix_cache_dir;
    use std::path::Path;

    /// Tracing: TC-004-05
    /// ACs: FR-004-AC-5
    #[test]
    fn tc_004_05_absolute_xdg_cache_home_does_not_require_home() {
        assert_eq!(
            unix_cache_dir(None, Some(Path::new("/tmp/cache"))).expect("absolute XDG path"),
            Path::new("/tmp/cache/ix-board")
        );
    }

    /// Tracing: TC-004-06
    /// ACs: FR-004-AC-6
    #[test]
    fn tc_004_06_invalid_xdg_uses_home_and_missing_home_is_unavailable() {
        assert_eq!(
            unix_cache_dir(Some(Path::new("/home/agent")), Some(Path::new("relative")))
                .expect("home fallback"),
            Path::new("/home/agent/.cache/ix-board")
        );
        assert!(matches!(
            unix_cache_dir(None, None),
            Err(super::Error::CacheDirectoryUnavailable)
        ));
    }
}

impl<S: WorkspaceSource, C: Clock> ReadService<S, C> {
    /// Opens a service with an injected clock, primarily useful for deterministic cache tests.
    pub fn with_clock(source: S, cache_path: &Path, clock: C) -> Result<Self, Error> {
        let cache_namespace = source.cache_namespace();
        Ok(Self {
            source,
            cache: SnapshotCache::open(cache_path, &cache_namespace)?,
            clock,
        })
    }

    /// Reads an ordered queue of eligible, not-yet-active tickets for team.
    pub fn next(&mut self, team: &str) -> Result<Vec<ReadyItem>, Error> {
        if team.trim().is_empty() {
            return Err(Error::InvalidTeam);
        }
        let snapshot = self.snapshot()?;
        ready_queue(&snapshot, team)
    }

    /// Returns one project's goal, milestones, open tickets, and dependency layers.
    pub fn project_chunk(&mut self, identifier: &str) -> Result<ProjectChunk, Error> {
        if identifier.trim().is_empty() {
            return Err(Error::ProjectNotFound {
                identifier: identifier.to_owned(),
            });
        }
        let snapshot = self.snapshot()?;
        make_project_chunk(&snapshot, identifier)
    }

    /// Resolves inline initiative, project, and issue selectors into a focus expansion.
    pub fn focus_expand(&mut self, selection: &FocusSelection) -> Result<FocusExpansion, Error> {
        if selection.initiatives.is_empty()
            && selection.projects.is_empty()
            && selection.issues.is_empty()
        {
            return Err(Error::EmptyFocus);
        }
        let snapshot = self.snapshot()?;
        make_focus_expansion(&snapshot, selection)
    }

    /// Returns focus status, complete blocker attribution, and eligible ready work.
    pub fn focus_status(&mut self, selection: &FocusSelection) -> Result<FocusStatus, Error> {
        if selection.initiatives.is_empty()
            && selection.projects.is_empty()
            && selection.issues.is_empty()
        {
            return Err(Error::EmptyFocus);
        }
        let snapshot = self.snapshot()?;
        make_focus_status(&snapshot, selection)
    }

    /// Returns a dependency-ordered handoff of open focus issues, optionally bounded by size.
    pub fn focus_plan_chunk(
        &mut self,
        selection: &FocusSelection,
        size: Option<std::num::NonZeroUsize>,
    ) -> Result<FocusPlanChunk, Error> {
        if selection.initiatives.is_empty()
            && selection.projects.is_empty()
            && selection.issues.is_empty()
        {
            return Err(Error::EmptyFocus);
        }
        let snapshot = self.snapshot()?;
        make_focus_plan_chunk(&snapshot, selection, size)
    }

    /// Returns a deterministic focus hygiene report from the current snapshot.
    pub fn focus_hygiene(
        &mut self,
        selection: &FocusSelection,
        report_time: &str,
        stale_days: u32,
    ) -> Result<FocusHygieneReport, Error> {
        if selection.initiatives.is_empty()
            && selection.projects.is_empty()
            && selection.issues.is_empty()
        {
            return Err(Error::EmptyFocus);
        }
        let snapshot = self.snapshot()?;
        let scope = select_scope(
            &snapshot,
            &ScopeSelection {
                team: None,
                initiatives: selection.initiatives.clone(),
                projects: selection.projects.clone(),
                tickets: selection.issues.clone(),
            },
        )?;
        if scope.issue_ids.is_empty() {
            return Err(Error::EmptyFocus);
        }
        let canonical_focus = canonical_selectors(&snapshot, selection)?;
        let focus_key = serde_json::to_string(&canonical_focus).map_err(Error::CacheEncode)?;
        let (prior, prior_unavailable) = match self.cache.hygiene_baseline(&focus_key) {
            Ok(prior) => (prior, None),
            Err(Error::CacheDecode(_)) => (None, Some("prior baseline is unreadable".to_owned())),
            Err(error) => return Err(error),
        };
        let mut report = hygiene::build_report(
            &snapshot,
            &scope.issue_ids,
            canonical_focus,
            report_time,
            stale_days,
            prior
                .as_ref()
                .map(|baseline| (baseline.report_time.as_str(), baseline.issues.as_slice())),
            prior_unavailable,
        )
        .map_err(|error| match error {
            hygiene::HygieneError::ResourceLimit { limit } => {
                Error::FocusHygieneResourceLimit { limit }
            }
            hygiene::HygieneError::IncompleteSnapshot { issue } => {
                Error::FocusHygieneIncomplete { issue }
            }
            hygiene::HygieneError::InvalidTime => Error::InvalidFocusHygieneTime,
        })?;
        let baseline = FocusBaseline {
            report_time: report.report_time.clone(),
            issues: std::mem::take(&mut report.baseline),
        };
        if report.baseline_advance {
            self.cache.replace_hygiene_baseline(&focus_key, &baseline)?;
        }
        report.baseline = baseline.issues;
        Ok(report)
    }

    /// Searches structured project and issue IDs, names, titles, and descriptions.
    pub fn search(&mut self, query: &str) -> Result<Vec<SearchResult>, Error> {
        if query.trim().is_empty() {
            return Err(Error::InvalidQuery);
        }
        let snapshot = self.snapshot()?;
        Ok(search_snapshot(&snapshot, query))
    }

    /// Returns the cached workspace snapshot, fetching it when the cache has expired.
    pub fn workspace_snapshot(&mut self) -> Result<WorkspaceSnapshot, Error> {
        self.snapshot()
    }

    /// Fetches a new workspace snapshot even when the cache is still fresh.
    /// A failed fetch leaves the cache empty so later reads cannot use stale data.
    pub fn force_refresh_workspace(&mut self) -> Result<WorkspaceSnapshot, Error> {
        self.cache.invalidate()?;
        let mut snapshot = self.source.fetch_workspace()?;
        snapshot.fetched_at = self.clock.now_epoch_seconds();
        self.cache.replace(snapshot.fetched_at, &snapshot)?;
        Ok(snapshot)
    }

    /// Removes this source's cached snapshot before a write or uncertain source state.
    pub fn invalidate_cache(&mut self) -> Result<(), Error> {
        self.cache.invalidate()
    }

    fn snapshot(&mut self) -> Result<WorkspaceSnapshot, Error> {
        let now = self.clock.now_epoch_seconds();
        if let Some(snapshot) = self.cache.fresh(now)? {
            return Ok(snapshot);
        }
        self.force_refresh_workspace()
    }
}

impl<S: ProjectMutationSource, C: Clock> ReadService<S, C> {
    /// Sets a project's status and refreshes the cached queue snapshot.
    pub fn set_project_status(
        &mut self,
        identifier: &str,
        status: ProjectRunStatus,
    ) -> Result<(), Error> {
        let mut snapshot = self.snapshot()?;
        let project_id = project_id(&snapshot, identifier)?;
        self.source.set_project_status(&project_id, status)?;
        let project = snapshot
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| Error::ProjectNotFound {
                identifier: identifier.to_owned(),
            })?;
        project.status = status.as_str().to_owned();
        self.cache
            .replace(self.clock.now_epoch_seconds(), &snapshot)
    }

    /// Sets a project's priority and refreshes the cached queue snapshot.
    pub fn set_project_priority(
        &mut self,
        identifier: &str,
        priority: ProjectPriority,
    ) -> Result<(), Error> {
        let mut snapshot = self.snapshot()?;
        let project_id = project_id(&snapshot, identifier)?;
        self.source.set_project_priority(&project_id, priority)?;
        let project = snapshot
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| Error::ProjectNotFound {
                identifier: identifier.to_owned(),
            })?;
        project.priority = priority.linear_value();
        self.cache
            .replace(self.clock.now_epoch_seconds(), &snapshot)
    }
}

fn project_id(snapshot: &WorkspaceSnapshot, identifier: &str) -> Result<ProjectId, Error> {
    snapshot
        .projects
        .iter()
        .find(|project| {
            project.id.0.eq_ignore_ascii_case(identifier)
                || project
                    .identifier
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(identifier))
        })
        .map(|project| project.id.clone())
        .ok_or_else(|| Error::ProjectNotFound {
            identifier: identifier.to_owned(),
        })
}

fn ready_queue(snapshot: &WorkspaceSnapshot, team: &str) -> Result<Vec<ReadyItem>, Error> {
    ready_queue_for(snapshot, |issue| {
        issue.team_key.0.eq_ignore_ascii_case(team)
    })
}

fn ready_queue_for(
    snapshot: &WorkspaceSnapshot,
    is_member: impl Fn(&Issue) -> bool,
) -> Result<Vec<ReadyItem>, Error> {
    let projects: HashMap<_, _> = snapshot
        .projects
        .iter()
        .map(|project| (&project.id, project))
        .collect();
    let issues: HashMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (&issue.id, issue))
        .collect();
    let mut candidates = Vec::new();
    for issue in &snapshot.issues {
        if !is_member(issue) || !issue_is_open(issue) {
            continue;
        }
        let Some(step) = spec_step(&issue.state) else {
            continue;
        };
        let Some(project_id) = &issue.project_id else {
            continue;
        };
        let Some(project) = projects.get(project_id).copied() else {
            continue;
        };
        if project.status != "Started" || !blockers_completed(issue, &issues)? {
            continue;
        }
        candidates.push((project, issue, step));
    }
    candidates.sort_by(|(project_a, issue_a, _), (project_b, issue_b, _)| {
        compare_project_priority(project_a.priority, project_b.priority)
            .then_with(|| compare_f64(project_a.sort_order, project_b.sort_order))
            .then_with(|| compare_issue_priority(issue_a.priority, issue_b.priority))
            .then_with(|| compare_f64(issue_a.sort_order, issue_b.sort_order))
            .then_with(|| {
                project_a
                    .identifier
                    .as_deref()
                    .unwrap_or(&project_a.id.0)
                    .cmp(project_b.identifier.as_deref().unwrap_or(&project_b.id.0))
            })
            .then_with(|| issue_a.identifier.cmp(&issue_b.identifier))
            .then_with(|| issue_a.id.cmp(&issue_b.id))
    });
    Ok(candidates
        .into_iter()
        .map(|(project, issue, step)| ReadyItem {
            project: project.identifier.clone().unwrap_or_else(|| project.name.clone()),
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            next_step: step,
            reason: format!(
                "project {} is Started; all recursive blockers are Completed; state {} maps to {}; ordered by project priority {}, project order {}, ticket priority {}, ticket order {}",
                project.identifier.as_deref().unwrap_or(&project.name),
                issue.state,
                match step {
                    NextStep::Specify => "Specify",
                    NextStep::Coding => "Coding",
                },
                project.priority,
                project.sort_order,
                issue.priority,
                issue.sort_order
            ),
        })
        .collect())
}

fn compare_project_priority(left: i32, right: i32) -> Ordering {
    (left == 0)
        .cmp(&(right == 0))
        .then_with(|| left.cmp(&right))
}

fn compare_issue_priority(left: f64, right: f64) -> Ordering {
    (left == 0.0)
        .cmp(&(right == 0.0))
        .then_with(|| compare_f64(left, right))
}

fn spec_step(spec_state: &str) -> Option<NextStep> {
    match spec_state {
        "Specced" => Some(NextStep::Coding),
        "Specify" | "Backlog" => Some(NextStep::Specify),
        _ => None,
    }
}

fn blockers_completed(issue: &Issue, issues: &HashMap<&IssueId, &Issue>) -> Result<bool, Error> {
    Ok(unresolved_blockers(issue, issues)?.is_empty())
}

fn unresolved_blockers(
    issue: &Issue,
    issues: &HashMap<&IssueId, &Issue>,
) -> Result<BTreeSet<String>, Error> {
    let mut pending = issue.blocked_by.to_vec();
    let mut visited = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    while let Some(blocker_id) = pending.pop() {
        if !visited.insert(blocker_id.clone()) {
            continue;
        }
        let Some(blocker) = issues.get(&blocker_id) else {
            return Err(Error::MissingDependency {
                issue: blocker_id.0,
            });
        };
        if blocker.state_type != WorkflowStateType::Completed {
            unresolved.insert(blocker.identifier.clone());
        }
        pending.extend(blocker.blocked_by.iter().cloned());
    }
    Ok(unresolved)
}

fn canonical_selectors(
    snapshot: &WorkspaceSnapshot,
    selection: &FocusSelection,
) -> Result<Vec<String>, Error> {
    let mut selectors = BTreeSet::new();
    for value in &selection.initiatives {
        let matches = snapshot
            .projects
            .iter()
            .flat_map(|project| &project.initiatives)
            .filter(|initiative| {
                initiative.id == *value || initiative.name.eq_ignore_ascii_case(value)
            })
            .map(|initiative| initiative.id.clone())
            .collect::<BTreeSet<_>>();
        if matches.len() != 1 {
            return Err(Error::ScopeSelection {
                kind: "initiative",
                identifier: value.clone(),
                matches: matches.len(),
            });
        }
        if let Some(id) = matches.first() {
            selectors.insert(format!("initiative:{id}"));
        }
    }
    for value in &selection.projects {
        let matches = snapshot
            .projects
            .iter()
            .filter(|project| {
                project.id.0 == *value
                    || project.name.eq_ignore_ascii_case(value)
                    || project
                        .identifier
                        .as_ref()
                        .is_some_and(|id| id.eq_ignore_ascii_case(value))
            })
            .map(|project| project.id.0.clone())
            .collect::<BTreeSet<_>>();
        if matches.len() != 1 {
            return Err(Error::ScopeSelection {
                kind: "project",
                identifier: value.clone(),
                matches: matches.len(),
            });
        }
        if let Some(id) = matches.first() {
            selectors.insert(format!("project:{id}"));
        }
    }
    for value in &selection.issues {
        let matches = snapshot
            .issues
            .iter()
            .filter(|issue| issue.id.0 == *value || issue.identifier.eq_ignore_ascii_case(value))
            .map(|issue| issue.id.0.clone())
            .collect::<BTreeSet<_>>();
        if matches.len() != 1 {
            return Err(Error::ScopeSelection {
                kind: "issue",
                identifier: value.clone(),
                matches: matches.len(),
            });
        }
        if let Some(id) = matches.first() {
            selectors.insert(format!("issue:{id}"));
        }
    }
    Ok(selectors.into_iter().collect())
}

fn make_project_chunk(
    snapshot: &WorkspaceSnapshot,
    identifier: &str,
) -> Result<ProjectChunk, Error> {
    let Some(project) = snapshot.projects.iter().find(|project| {
        project.id.0.eq_ignore_ascii_case(identifier)
            || project
                .identifier
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case(identifier))
    }) else {
        return Err(Error::ProjectNotFound {
            identifier: identifier.to_owned(),
        });
    };
    let issues: HashMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (&issue.id, issue))
        .collect();
    let mut project_issues = snapshot
        .issues
        .iter()
        .filter(|issue| issue.project_id.as_ref() == Some(&project.id))
        .collect::<Vec<_>>();
    project_issues.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    let open_tickets = project_issues
        .iter()
        .filter(|issue| issue_is_open(issue))
        .map(|issue| ChunkIssue {
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            description: issue.description.clone(),
            state: issue.state.clone(),
            blocked_by: issue
                .blocked_by
                .iter()
                .filter_map(|id| issues.get(id))
                .map(|dependency| dependency.identifier.clone())
                .collect(),
        })
        .collect::<Vec<_>>();
    let open_project_issues = project_issues
        .iter()
        .copied()
        .filter(|issue| issue_is_open(issue))
        .collect::<Vec<_>>();
    let layers = dependency_layers(snapshot, &open_project_issues)?;
    let mut milestones = project.milestones.clone();
    milestones.sort_by(|left, right| {
        compare_f64(left.sort_order, right.sort_order).then_with(|| left.id.cmp(&right.id))
    });
    Ok(ProjectChunk {
        identifier: project
            .identifier
            .clone()
            .unwrap_or_else(|| project.id.0.clone()),
        name: project.name.clone(),
        goal: project.description.clone(),
        milestones,
        open_tickets,
        blocker_layers: layers,
    })
}

fn make_focus_expansion(
    snapshot: &WorkspaceSnapshot,
    selection: &FocusSelection,
) -> Result<FocusExpansion, Error> {
    if selection.initiatives.is_empty()
        && selection.projects.is_empty()
        && selection.issues.is_empty()
    {
        return Err(Error::EmptyFocus);
    }

    let scope_selection = ScopeSelection {
        team: None,
        initiatives: selection.initiatives.clone(),
        projects: selection.projects.clone(),
        tickets: selection.issues.clone(),
    };
    let scope = select_scope(snapshot, &scope_selection)?;
    let issue_selector_scope = if selection.issues.is_empty() {
        ScopeSummary::default()
    } else {
        select_scope(
            snapshot,
            &ScopeSelection {
                team: None,
                initiatives: Vec::new(),
                projects: Vec::new(),
                tickets: selection.issues.clone(),
            },
        )?
    };
    let issues_by_id: BTreeMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let focus_issues = summarize_issues(&scope.issue_ids, &issues_by_id)?;
    let issue_selector_issues = summarize_issues(&issue_selector_scope.issue_ids, &issues_by_id)?;
    let focus_roots = scope
        .issue_ids
        .iter()
        .filter_map(|id| issues_by_id.get(id).copied())
        .collect::<Vec<_>>();
    let blocker_layers = dependency_layers(snapshot, &focus_roots)?;

    let mut projects = Vec::new();
    for project in snapshot
        .projects
        .iter()
        .filter(|project| scope.project_ids.contains(&project.id))
    {
        let mut milestone_records = project.milestones.iter().collect::<Vec<_>>();
        milestone_records.sort_by(|left, right| {
            compare_f64(left.sort_order, right.sort_order).then_with(|| left.id.cmp(&right.id))
        });
        let mut milestones = milestone_records
            .into_iter()
            .map(|milestone| FocusMilestone {
                id: milestone.id.clone(),
                name: milestone.name.clone(),
                description: milestone.description.clone(),
                issues: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut project_level_issues = Vec::new();
        for issue in snapshot.issues.iter().filter(|issue| {
            scope.issue_ids.contains(&issue.id) && issue.project_id.as_ref() == Some(&project.id)
        }) {
            let summary = focus_issue(issue);
            if let Some(milestone_id) = &issue.project_milestone_id {
                let Some(milestone) = milestones.iter_mut().find(|m| &m.id == milestone_id) else {
                    return Err(Error::MissingProjectMilestone {
                        issue: issue.identifier.clone(),
                        milestone: milestone_id.clone(),
                        project: project.id.0.clone(),
                    });
                };
                milestone.issues.push(summary);
            } else {
                project_level_issues.push(summary);
            }
        }
        for milestone in &mut milestones {
            milestone
                .issues
                .sort_by(|left, right| left.identifier.cmp(&right.identifier));
        }
        project_level_issues.sort_by(|left, right| left.identifier.cmp(&right.identifier));
        projects.push(FocusProject {
            id: project.id.clone(),
            identifier: project
                .identifier
                .clone()
                .unwrap_or_else(|| project.id.0.clone()),
            name: project.name.clone(),
            milestones,
            project_level_issues,
        });
    }
    projects.sort_by(|left, right| {
        left.identifier
            .cmp(&right.identifier)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(FocusExpansion {
        fetched_at: snapshot.fetched_at,
        projects,
        focus_issues,
        issue_selector_issues,
        blocker_layers,
    })
}

fn make_focus_status(
    snapshot: &WorkspaceSnapshot,
    selection: &FocusSelection,
) -> Result<FocusStatus, Error> {
    let expansion = make_focus_expansion(snapshot, selection)?;
    let scope = select_scope(
        snapshot,
        &ScopeSelection {
            team: None,
            initiatives: selection.initiatives.clone(),
            projects: selection.projects.clone(),
            tickets: selection.issues.clone(),
        },
    )?;
    if expansion.focus_issues.is_empty() {
        return Err(Error::EmptyFocus);
    }
    let issues_by_id: BTreeMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (issue.id.clone(), issue))
        .collect();
    let leaf_ids = scope.leaf_ids;
    let mut projects = snapshot
        .projects
        .iter()
        .filter(|project| scope.project_ids.contains(&project.id))
        .map(|project| {
            let member_issues = snapshot
                .issues
                .iter()
                .filter(|issue| {
                    scope.issue_ids.contains(&issue.id)
                        && issue.project_id.as_ref() == Some(&project.id)
                })
                .collect::<Vec<_>>();
            let mut milestones = project.milestones.iter().collect::<Vec<_>>();
            milestones.sort_by(|left, right| {
                compare_f64(left.sort_order, right.sort_order).then_with(|| left.id.cmp(&right.id))
            });
            let milestone_status = milestones
                .into_iter()
                .map(|milestone| {
                    let assigned = member_issues
                        .iter()
                        .copied()
                        .filter(|issue| {
                            issue.project_milestone_id.as_deref() == Some(&milestone.id)
                        })
                        .collect::<Vec<_>>();
                    FocusMilestoneStatus {
                        id: milestone.id.clone(),
                        name: milestone.name.clone(),
                        target_date: milestone.target_date.clone(),
                        open_by_state: open_state_counts(&assigned),
                        in_progress: started_issues(&assigned),
                        progress: leaf_progress(&assigned, &leaf_ids),
                    }
                })
                .collect();
            let without_milestone = member_issues
                .iter()
                .copied()
                .filter(|issue| issue.project_milestone_id.is_none())
                .collect::<Vec<_>>();
            FocusProjectStatus {
                id: project.id.clone(),
                identifier: project
                    .identifier
                    .clone()
                    .unwrap_or_else(|| project.id.0.clone()),
                name: project.name.clone(),
                target_date: project.target_date.clone(),
                open_by_state: open_state_counts(&member_issues),
                in_progress: started_issues(&member_issues),
                progress: leaf_progress(&member_issues, &leaf_ids),
                milestones: milestone_status,
                unmilestoned: FocusUnmilestonedStatus {
                    open_by_state: open_state_counts(&without_milestone),
                    in_progress: started_issues(&without_milestone),
                    progress: leaf_progress(&without_milestone, &leaf_ids),
                },
            }
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        left.identifier
            .cmp(&right.identifier)
            .then_with(|| left.id.cmp(&right.id))
    });

    let focus_roots = scope
        .issue_ids
        .iter()
        .map(|id| {
            issues_by_id
                .get(id)
                .copied()
                .ok_or_else(|| Error::MissingDependency {
                    issue: id.0.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let blocker_layers = focus_blocker_layers(snapshot, &focus_roots)?;
    let focus_ids = scope.issue_ids;
    let ready_queue = ready_queue_for(snapshot, |issue| focus_ids.contains(&issue.id))?;
    Ok(FocusStatus {
        fetched_at: snapshot.fetched_at,
        projects,
        blocker_layers,
        ready_queue,
    })
}

fn make_focus_plan_chunk(
    snapshot: &WorkspaceSnapshot,
    selection: &FocusSelection,
    size: Option<std::num::NonZeroUsize>,
) -> Result<FocusPlanChunk, Error> {
    let scope = select_scope(
        snapshot,
        &ScopeSelection {
            team: None,
            initiatives: selection.initiatives.clone(),
            projects: selection.projects.clone(),
            tickets: selection.issues.clone(),
        },
    )?;
    if scope.issue_ids.is_empty() {
        return Err(Error::EmptyFocus);
    }

    let issues: HashMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (&issue.id, issue))
        .collect();
    let roots = scope
        .issue_ids
        .iter()
        .map(|id| {
            issues
                .get(id)
                .copied()
                .ok_or_else(|| Error::MissingDependency {
                    issue: id.0.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let layers = dependency_layer_ids(snapshot, &roots)?;
    let layer_by_id: BTreeMap<_, _> = layers
        .iter()
        .enumerate()
        .flat_map(|(layer, ids)| ids.iter().map(move |id| (id.clone(), layer)))
        .collect();
    let projects: HashMap<_, _> = snapshot
        .projects
        .iter()
        .map(|project| (&project.id, project))
        .collect();
    let mut candidates = scope
        .issue_ids
        .iter()
        .filter_map(|id| issues.get(id).copied())
        .filter(|issue| {
            !matches!(
                issue.state_type,
                WorkflowStateType::Completed | WorkflowStateType::Canceled
            )
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        let left_layer = layer_by_id.get(&left.id).copied().unwrap_or(usize::MAX);
        let right_layer = layer_by_id.get(&right.id).copied().unwrap_or(usize::MAX);
        left_layer
            .cmp(&right_layer)
            .then_with(|| compare_plan_project_order(left, right, &projects))
            .then_with(|| compare_plan_milestone_order(left, right, &projects))
            .then_with(|| left.identifier.cmp(&right.identifier))
            .then_with(|| left.id.cmp(&right.id))
    });
    let unblocks_by_id = focus_unblocks(&candidates, &issues)?;

    let rows =
        candidates
            .iter()
            .map(|issue| {
                let layer = layer_by_id.get(&issue.id).copied().ok_or_else(|| {
                    Error::FocusPlanIncomplete {
                        issue: issue.identifier.clone(),
                        field: "dependency layer",
                    }
                })?;
                let milestone = plan_milestone(issue, &projects)?;
                let (ready, reasons) = plan_readiness(issue, &projects, &issues)?;
                Ok(FocusPlanIssue {
                    identifier: issue.identifier.clone(),
                    title: issue.title.clone(),
                    milestone,
                    dependency_layer: layer,
                    ready,
                    reasons,
                    unblocks: unblocks_by_id
                        .get(&issue.id)
                        .map(|values| values.iter().cloned().collect())
                        .unwrap_or_default(),
                    acceptance_criteria: acceptance_criteria_excerpt(issue.description.as_deref()),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
    let issues = match size {
        Some(size) => rows.into_iter().take(size.get()).collect(),
        None => rows,
    };
    Ok(FocusPlanChunk {
        fetched_at: snapshot.fetched_at,
        issues,
    })
}

fn compare_plan_project_order(
    left: &Issue,
    right: &Issue,
    projects: &HashMap<&ProjectId, &Project>,
) -> Ordering {
    let left_project = left
        .project_id
        .as_ref()
        .and_then(|id| projects.get(id).copied());
    let right_project = right
        .project_id
        .as_ref()
        .and_then(|id| projects.get(id).copied());
    match (left_project, right_project) {
        (Some(left_project), Some(right_project)) => {
            compare_project_priority(left_project.priority, right_project.priority)
                .then_with(|| compare_f64(left_project.sort_order, right_project.sort_order))
                .then_with(|| {
                    left_project
                        .identifier
                        .as_deref()
                        .unwrap_or(&left_project.id.0)
                        .cmp(
                            right_project
                                .identifier
                                .as_deref()
                                .unwrap_or(&right_project.id.0),
                        )
                })
                .then_with(|| left_project.id.cmp(&right_project.id))
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_plan_milestone_order(
    left: &Issue,
    right: &Issue,
    projects: &HashMap<&ProjectId, &Project>,
) -> Ordering {
    let milestone = |issue: &Issue| {
        issue
            .project_id
            .as_ref()
            .and_then(|project_id| projects.get(project_id).copied())
            .and_then(|project| {
                issue.project_milestone_id.as_deref().and_then(|id| {
                    project
                        .milestones
                        .iter()
                        .find(|milestone| milestone.id == id)
                })
            })
    };
    match (milestone(left), milestone(right)) {
        (Some(left), Some(right)) => {
            compare_f64(left.sort_order, right.sort_order).then_with(|| left.id.cmp(&right.id))
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn plan_milestone(
    issue: &Issue,
    projects: &HashMap<&ProjectId, &Project>,
) -> Result<Option<String>, Error> {
    let Some(milestone_id) = &issue.project_milestone_id else {
        return Ok(None);
    };
    let Some(project_id) = &issue.project_id else {
        return Err(Error::FocusPlanIncomplete {
            issue: issue.identifier.clone(),
            field: "project for milestone",
        });
    };
    let Some(project) = projects.get(project_id).copied() else {
        return Err(Error::FocusPlanIncomplete {
            issue: issue.identifier.clone(),
            field: "project record",
        });
    };
    let Some(milestone) = project.milestones.iter().find(|m| m.id == *milestone_id) else {
        return Err(Error::MissingProjectMilestone {
            issue: issue.identifier.clone(),
            milestone: milestone_id.clone(),
            project: project_id.0.clone(),
        });
    };
    Ok(Some(milestone.name.clone()))
}

fn plan_readiness(
    issue: &Issue,
    projects: &HashMap<&ProjectId, &Project>,
    issues: &HashMap<&IssueId, &Issue>,
) -> Result<(bool, Vec<String>), Error> {
    let mut reasons = Vec::new();
    let state_is_ready = issue_is_open(issue);
    reasons.push(if state_is_ready {
        format!("workflow state {} is eligible", issue.state_name)
    } else {
        format!("workflow state {} is not eligible", issue.state_name)
    });
    let project_running = match &issue.project_id {
        Some(project_id) => {
            let Some(project) = projects.get(project_id).copied() else {
                return Err(Error::FocusPlanIncomplete {
                    issue: issue.identifier.clone(),
                    field: "project record",
                });
            };
            let started = project.status == "Started";
            reasons.push(format!(
                "owning project {} is {}",
                project.identifier.as_deref().unwrap_or(&project.name),
                project.status
            ));
            started
        }
        None => {
            reasons.push("issue has no owning project".to_owned());
            false
        }
    };
    let step = spec_step(&issue.state);
    reasons.push(match step {
        Some(step) => format!("spec state {} maps to {:?}", issue.state, step),
        None => format!(
            "workflow state {} has no new Specify or Coding step (ready states: Backlog, Specify, Specced)",
            issue.state
        ),
    });
    let unresolved = unresolved_blockers(issue, issues)?;
    let blockers_ready = unresolved.is_empty();
    reasons.push(if blockers_ready {
        "all recursive blockers are Completed".to_owned()
    } else {
        format!(
            "recursive blockers are not Completed: {}",
            unresolved.into_iter().collect::<Vec<_>>().join(", ")
        )
    });
    let ready = state_is_ready && project_running && step.is_some() && blockers_ready;
    Ok((ready, reasons))
}

fn focus_unblocks(
    candidates: &[&Issue],
    issues: &HashMap<&IssueId, &Issue>,
) -> Result<BTreeMap<IssueId, BTreeSet<String>>, Error> {
    let focus_ids = candidates
        .iter()
        .map(|issue| issue.id.clone())
        .collect::<BTreeSet<_>>();
    let mut unblocks = BTreeMap::<IssueId, BTreeSet<String>>::new();
    for dependent in candidates {
        let mut pending = dependent.blocked_by.clone();
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let Some(blocker) = issues.get(&id).copied() else {
                return Err(Error::MissingDependency { issue: id.0 });
            };
            if focus_ids.contains(&id) && id != dependent.id {
                unblocks
                    .entry(id.clone())
                    .or_default()
                    .insert(dependent.identifier.clone());
            }
            pending.extend(blocker.blocked_by.iter().cloned());
        }
    }
    Ok(unblocks)
}

fn acceptance_criteria_excerpt(description: Option<&str>) -> Option<String> {
    let lines = description?.lines().collect::<Vec<_>>();
    let headings = markdown_headings(&lines);
    for (heading_index, (_, body_start, title)) in headings.iter().enumerate() {
        if !title.trim().eq_ignore_ascii_case("Acceptance Criteria") {
            continue;
        }
        let body_end = headings
            .get(heading_index + 1)
            .map_or(lines.len(), |(next_heading_line, _, _)| *next_heading_line);
        let body = lines[*body_start..body_end].to_vec().join("\n");
        let body = body.trim();
        if body.is_empty() {
            return None;
        }
        let mut chars = body.chars();
        let excerpt = chars.by_ref().take(280).collect::<String>();
        return Some(if chars.next().is_some() {
            format!("{excerpt}…")
        } else {
            excerpt
        });
    }
    None
}

fn markdown_headings(lines: &[&str]) -> Vec<(usize, usize, String)> {
    let mut headings = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, minimum)) = fence {
            if closing_fence(line, marker, minimum) {
                fence = None;
            }
            index += 1;
            continue;
        }
        if let Some(opening) = opening_fence(line) {
            fence = Some(opening);
            index += 1;
            continue;
        }
        if let Some(title) = atx_heading_title(line) {
            headings.push((index, index + 1, title.to_owned()));
            index += 1;
            continue;
        }
        if index > 0 && is_setext_underline(line) {
            let title_line = lines[index - 1];
            if !title_line.trim().is_empty()
                && headings
                    .last()
                    .is_none_or(|(last_heading_line, _, _)| *last_heading_line != index - 1)
            {
                headings.push((index - 1, index + 1, title_line.trim().to_owned()));
            }
        }
        index += 1;
    }
    headings.sort_by_key(|(heading_line, _, _)| *heading_line);
    headings
}

fn opening_fence(line: &str) -> Option<(u8, usize)> {
    let indentation = line.len() - line.trim_start_matches(' ').len();
    if indentation > 3 {
        return None;
    }
    let content = &line[indentation..];
    let marker = *content.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = content.bytes().take_while(|byte| *byte == marker).count();
    (length >= 3).then_some((marker, length))
}

fn closing_fence(line: &str, marker: u8, minimum: usize) -> bool {
    let indentation = line.len() - line.trim_start_matches(' ').len();
    if indentation > 3 {
        return false;
    }
    let content = &line[indentation..];
    let length = content.bytes().take_while(|byte| *byte == marker).count();
    length >= minimum && content[length..].trim().is_empty()
}

fn is_setext_underline(line: &str) -> bool {
    let indentation = line.len() - line.trim_start_matches(' ').len();
    if indentation > 3 {
        return false;
    }
    let underline = line.trim();
    (underline.bytes().all(|byte| byte == b'=') || underline.bytes().all(|byte| byte == b'-'))
        && !underline.is_empty()
}

fn atx_heading_title(line: &str) -> Option<&str> {
    let indentation = line.len() - line.trim_start_matches(' ').len();
    if indentation > 3 {
        return None;
    }
    let line = &line[indentation..];
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let remainder = &line[hashes..];
    if remainder
        .as_bytes()
        .first()
        .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return None;
    }
    let title = remainder.trim();
    let closing_hashes = title.bytes().rev().take_while(|byte| *byte == b'#').count();
    if closing_hashes > 0 {
        let closing_start = title.len() - closing_hashes;
        if closing_start == 0 || title.as_bytes()[closing_start - 1].is_ascii_whitespace() {
            return Some(title[..closing_start].trim_end());
        }
    }
    Some(title)
}

fn open_state_counts(issues: &[&Issue]) -> Vec<FocusStateCount> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for issue in issues.iter().copied().filter(|issue| {
        !matches!(
            issue.state_type,
            WorkflowStateType::Completed | WorkflowStateType::Canceled
        )
    }) {
        *counts.entry(&issue.state_name).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(state, count)| FocusStateCount {
            state: state.to_owned(),
            count,
        })
        .collect()
}

fn started_issues(issues: &[&Issue]) -> Vec<FocusIssue> {
    let mut rows = issues
        .iter()
        .copied()
        .filter(|issue| issue.state_type == WorkflowStateType::Started)
        .map(|issue| FocusIssue {
            id: issue.id.clone(),
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            state: issue.state_name.clone(),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.identifier
            .cmp(&right.identifier)
            .then_with(|| left.id.cmp(&right.id))
    });
    rows
}

fn leaf_progress(issues: &[&Issue], leaf_ids: &BTreeSet<IssueId>) -> FocusProgress {
    let leaves = issues
        .iter()
        .copied()
        .filter(|issue| leaf_ids.contains(&issue.id))
        .collect::<Vec<_>>();
    let done = leaves
        .iter()
        .filter(|issue| issue.state_type == WorkflowStateType::Completed)
        .count();
    let total = leaves.len();
    FocusProgress {
        done,
        total,
        completed_percent: (total != 0).then(|| (done as f64 / total as f64) * 100.0),
    }
}

fn focus_blocker_layers(
    snapshot: &WorkspaceSnapshot,
    focus_roots: &[&Issue],
) -> Result<Vec<Vec<FocusBlocker>>, Error> {
    let layers = dependency_layer_ids(snapshot, focus_roots)?;
    let issues: HashMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (&issue.id, issue))
        .collect();
    let mut roots_by_blocker = BTreeMap::<IssueId, BTreeSet<String>>::new();
    for root in focus_roots {
        let mut pending = root.blocked_by.clone();
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let blocker = issues
                .get(&id)
                .copied()
                .ok_or_else(|| Error::MissingDependency {
                    issue: id.0.clone(),
                })?;
            roots_by_blocker
                .entry(id)
                .or_default()
                .insert(root.identifier.clone());
            pending.extend(blocker.blocked_by.iter().cloned());
        }
    }
    layers
        .into_iter()
        .map(|layer| {
            layer
                .into_iter()
                .map(|id| {
                    let issue =
                        issues
                            .get(&id)
                            .copied()
                            .ok_or_else(|| Error::MissingDependency {
                                issue: id.0.clone(),
                            })?;
                    let classification = match issue.state_type {
                        WorkflowStateType::Completed => FocusIssueClassification::Completed,
                        WorkflowStateType::Canceled => FocusIssueClassification::Canceled,
                        WorkflowStateType::Triage
                        | WorkflowStateType::Backlog
                        | WorkflowStateType::Unstarted
                        | WorkflowStateType::Started
                        | WorkflowStateType::Duplicate => FocusIssueClassification::Open,
                    };
                    Ok(FocusBlocker {
                        identifier: issue.identifier.clone(),
                        title: issue.title.clone(),
                        state: issue.state_name.clone(),
                        classification,
                        focus_root_ids: roots_by_blocker
                            .get(&id)
                            .into_iter()
                            .flat_map(|roots| roots.iter().cloned())
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .collect()
}

fn summarize_issues(
    ids: &BTreeSet<IssueId>,
    issues: &BTreeMap<IssueId, &Issue>,
) -> Result<Vec<FocusIssue>, Error> {
    let mut summaries = ids
        .iter()
        .map(|id| {
            issues
                .get(id)
                .copied()
                .map(focus_issue)
                .ok_or_else(|| Error::MissingDependency {
                    issue: id.0.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    summaries.sort_by(|left, right| {
        left.identifier
            .cmp(&right.identifier)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(summaries)
}

fn focus_issue(issue: &Issue) -> FocusIssue {
    FocusIssue {
        id: issue.id.clone(),
        identifier: issue.identifier.clone(),
        title: issue.title.clone(),
        state: issue.state.clone(),
    }
}

fn dependency_layers(
    snapshot: &WorkspaceSnapshot,
    project_issues: &[&Issue],
) -> Result<Vec<Vec<String>>, Error> {
    let layers = dependency_layer_ids(snapshot, project_issues)?;
    let issues: HashMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (&issue.id, issue))
        .collect();
    layers
        .into_iter()
        .map(|layer| {
            layer
                .into_iter()
                .map(|id| {
                    issues
                        .get(&id)
                        .map(|issue| issue.identifier.clone())
                        .ok_or_else(|| Error::MissingDependency { issue: id.0 })
                })
                .collect()
        })
        .collect()
}

fn dependency_layer_ids(
    snapshot: &WorkspaceSnapshot,
    project_issues: &[&Issue],
) -> Result<Vec<Vec<IssueId>>, Error> {
    let issues: HashMap<_, _> = snapshot
        .issues
        .iter()
        .map(|issue| (&issue.id, issue))
        .collect();
    let mut closure = BTreeSet::new();
    let mut pending = project_issues
        .iter()
        .map(|issue| issue.id.clone())
        .collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if !closure.insert(id.clone()) {
            continue;
        }
        let Some(issue) = issues.get(&id) else {
            return Err(Error::MissingDependency { issue: id.0 });
        };
        pending.extend(issue.blocked_by.iter().cloned());
    }
    let mut indegree: BTreeMap<IssueId, usize> =
        closure.iter().cloned().map(|id| (id, 0)).collect();
    let mut dependents: BTreeMap<IssueId, Vec<IssueId>> = BTreeMap::new();
    for issue_id in &closure {
        let issue = issues
            .get(issue_id)
            .ok_or_else(|| Error::MissingDependency {
                issue: issue_id.0.clone(),
            })?;
        for blocker in &issue.blocked_by {
            if closure.contains(blocker) {
                *indegree.entry(issue_id.clone()).or_default() += 1;
                dependents
                    .entry(blocker.clone())
                    .or_default()
                    .push(issue_id.clone());
            }
        }
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut layers = Vec::new();
    let mut emitted = 0usize;
    while !ready.is_empty() {
        let current = std::mem::take(&mut ready);
        let mut ordered_layer = Vec::with_capacity(current.len());
        for id in &current {
            let _issue = issues.get(id).ok_or_else(|| Error::MissingDependency {
                issue: id.0.clone(),
            })?;
            ordered_layer.push(id.clone());
            emitted += 1;
            if let Some(children) = dependents.get(id) {
                for child in children {
                    if let Some(count) = indegree.get_mut(child) {
                        *count -= 1;
                        if *count == 0 {
                            ready.insert(child.clone());
                        }
                    }
                }
            }
        }
        ordered_layer.sort_by(|left, right| {
            let left_identifier = issues.get(left).map(|issue| issue.identifier.as_str());
            let right_identifier = issues.get(right).map(|issue| issue.identifier.as_str());
            left_identifier
                .cmp(&right_identifier)
                .then_with(|| left.cmp(right))
        });
        layers.push(ordered_layer);
    }
    if emitted != closure.len() {
        let cycle = indegree
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(id, _)| id.0.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::DependencyCycle { issues: cycle });
    }
    Ok(layers)
}

fn search_snapshot(snapshot: &WorkspaceSnapshot, query: &str) -> Vec<SearchResult> {
    let needle = query.to_lowercase();
    let mut results = snapshot
        .projects
        .iter()
        .filter(|project| {
            [
                Some(project.id.0.as_str()),
                project.identifier.as_deref(),
                Some(project.name.as_str()),
                Some(project.description.as_str()),
            ]
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(&needle))
        })
        .map(|project| SearchResult {
            kind: model::SearchKind::Project,
            identifier: project
                .identifier
                .clone()
                .unwrap_or_else(|| project.id.0.clone()),
            name: project.name.clone(),
        })
        .chain(
            snapshot
                .issues
                .iter()
                .filter(|issue| {
                    [
                        issue.id.0.as_str(),
                        issue.identifier.as_str(),
                        issue.title.as_str(),
                        issue.description.as_deref().unwrap_or_default(),
                    ]
                    .into_iter()
                    .any(|field| field.to_lowercase().contains(&needle))
                })
                .map(|issue| SearchResult {
                    kind: model::SearchKind::Issue,
                    identifier: issue.identifier.clone(),
                    name: issue.title.clone(),
                }),
        )
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.identifier.cmp(&right.identifier))
    });
    results
}

/// Computes the ordered queue from exactly the supplied complete snapshot.
pub fn ready_from_snapshot(
    snapshot: &WorkspaceSnapshot,
    team: &str,
) -> Result<Vec<ReadyItem>, Error> {
    ready_queue(snapshot, team)
}

#[cfg(test)]
mod plan_chunk_tests {
    use super::acceptance_criteria_excerpt;

    /// Trace: FR-014-AC-5
    #[test]
    fn acceptance_excerpt_uses_markdown_heading_boundaries() {
        let description = "````md\n# Acceptance Criteria\ncode sample\n````\nOther heading\n===\nwrong section\n## acceptance criteria ###\nfirst body\n### Next\nignored";
        assert_eq!(
            acceptance_criteria_excerpt(Some(description)).as_deref(),
            Some("first body")
        );
        assert_eq!(
            acceptance_criteria_excerpt(Some("# Acceptance Criteria\n  \n## Next")),
            None
        );
        assert_eq!(
            acceptance_criteria_excerpt(Some("## Acceptance Criteria-ish\nbody")),
            None
        );
    }
}
