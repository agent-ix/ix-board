// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic Linear project controls and cached reads for ready work, context, and search.

mod cache;
mod linear;
mod model;

use cache::SnapshotCache;
pub use cache::{CACHE_TTL_SECONDS, Clock, SystemClock};
pub use linear::LinearGraphQlSource;
pub use model::{
    ChunkIssue, Issue, IssueId, Milestone, NextStep, Project, ProjectChunk, ProjectId,
    ProjectPriority, ProjectRunStatus, ReadyItem, SearchKind, SearchResult, TeamKey,
    WorkspaceSnapshot,
};

use model::{compare_f64, issue_is_open};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use thiserror::Error as ThisError;

/// Errors returned by the library's read and cache boundaries.
#[derive(Debug, ThisError)]
pub enum Error {
    /// A Linear HTTP request failed.
    #[error("Linear request failed: {0}")]
    Http(#[from] reqwest::Error),
    /// LINEAR_API_KEY is missing or empty.
    #[error("LINEAR_API_KEY is required for Linear reads")]
    MissingApiKey,
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
    /// The requested project was not present in the source snapshot.
    #[error("project not found: {identifier}")]
    ProjectNotFound { identifier: String },
    /// A project status token was not one of the supported values.
    #[error("invalid project status '{value}'; expected Started or Paused")]
    InvalidProjectStatus { value: String },
    /// A project priority token was not one of the exact Linear priority values.
    #[error("invalid project priority '{value}'; expected one of 0, 1, 2, 3, or 4")]
    InvalidProjectPriority { value: String },
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

/// Source capable of updating the two Linear project fields ix-board owns.
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
}

/// Cached read service shared by the CLI and library consumers.
pub struct ReadService<S, C = SystemClock> {
    source: S,
    cache: SnapshotCache,
    clock: C,
}

impl<S: WorkspaceSource> ReadService<S, SystemClock> {
    /// Opens a service with the system clock and a SQLite cache at cache_path.
    pub fn open(source: S, cache_path: &Path) -> Result<Self, Error> {
        Self::with_clock(source, cache_path, SystemClock)
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

    /// Searches structured project and issue IDs, names, titles, and descriptions.
    pub fn search(&mut self, query: &str) -> Result<Vec<SearchResult>, Error> {
        if query.trim().is_empty() {
            return Err(Error::InvalidQuery);
        }
        let snapshot = self.snapshot()?;
        Ok(search_snapshot(&snapshot, query))
    }

    fn snapshot(&mut self) -> Result<WorkspaceSnapshot, Error> {
        let now = self.clock.now_epoch_seconds();
        if let Some(snapshot) = self.cache.fresh(now)? {
            return Ok(snapshot);
        }
        let snapshot = self.source.fetch_workspace()?;
        self.cache
            .replace(self.clock.now_epoch_seconds(), &snapshot)?;
        Ok(snapshot)
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
        if !issue.team_key.0.eq_ignore_ascii_case(team) || !issue_is_open(issue) {
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
    let mut pending = issue.blocked_by.to_vec();
    let mut visited = HashSet::new();
    while let Some(blocker_id) = pending.pop() {
        if !visited.insert(blocker_id.clone()) {
            continue;
        }
        let Some(blocker) = issues.get(&blocker_id) else {
            return Err(Error::MissingDependency {
                issue: blocker_id.0,
            });
        };
        if blocker.state != "Completed" {
            return Ok(false);
        }
        pending.extend(blocker.blocked_by.iter().cloned());
    }
    Ok(true)
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

fn dependency_layers(
    snapshot: &WorkspaceSnapshot,
    project_issues: &[&Issue],
) -> Result<Vec<Vec<String>>, Error> {
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
        let mut names = Vec::with_capacity(current.len());
        for id in &current {
            let issue = issues.get(id).ok_or_else(|| Error::MissingDependency {
                issue: id.0.clone(),
            })?;
            names.push(issue.identifier.clone());
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
        names.sort();
        layers.push(names);
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
