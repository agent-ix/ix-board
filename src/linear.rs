// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Linear GraphQL transport for selected reads and project status/priority writes.

use ix_cli_kit::secrets::{AppScope, SecretKey, SecretStore, SecretValue};

use crate::{
    Clock, DriftConfig, DriftEvidence, DriftTicket, Error, HistoricalProject, Initiative, Issue,
    IssueId, IssueTransition, Milestone, Project, ProjectId, ProjectMutationSource,
    ProjectPriority, ProjectRunStatus, SystemClock, TeamKey, WorkflowStateType, WorkspaceSnapshot,
    WorkspaceSource,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::process::Command as ProcessCommand;

// Keep the project query below Linear's complexity limit while retaining nested page coverage.
const PROJECT_PAGE_SIZE: i32 = 20;
// 50 issues with up to 50 relations and 50 attachments each remain below the query complexity limit.
const ISSUE_PAGE_SIZE: i32 = 50;
const PROJECT_NESTED_PAGE_SIZE: i32 = 50;
const INITIATIVE_PAGE_SIZE: i32 = 50;
const RELATION_PAGE_SIZE: i32 = 50;
const PAGE_LIMIT: usize = 1_000;
// Invalidate cached snapshots predating the project activity timestamp.
const CACHE_MODEL_VERSION: &str = "v10";
const ENDPOINT: &str = "https://api.linear.app/graphql";

const PROJECT_QUERY: &str = r#"
query WorkspaceProjects($first: Int!, $nestedFirst: Int!, $after: String) {
  projects(first: $first, after: $after) {
    nodes {
      id identifier name description content priority sortOrder targetDate updatedAt
      status { name type }
      initiatives(first: 10) { nodes { id identifier name } pageInfo { hasNextPage } }
      teams(first: $nestedFirst) { nodes { key } pageInfo { hasNextPage } }
      projectMilestones(first: $nestedFirst) {
        nodes { id name description sortOrder targetDate }
        pageInfo { hasNextPage }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}"#;

const INITIATIVES_QUERY: &str = r#"
query WorkspaceInitiatives($first: Int!, $after: String) {
  initiatives(first: $first, after: $after) {
    nodes { id identifier name }
    pageInfo { hasNextPage endCursor }
  }
}"#;

const ISSUE_QUERY: &str = r#"
query WorkspaceIssues($first: Int!, $relationFirst: Int!, $after: String) {
  issues(first: $first, after: $after) {
    nodes {
      id identifier title description priority sortOrder createdAt updatedAt
      state { name type }
      team { key }
      assignee { id }
      project { id }
      projectMilestone { id }
      parent { id }
      relations(first: $relationFirst) {
        nodes { type issue { id } relatedIssue { id } }
        pageInfo { hasNextPage }
      }
      attachments(first: 50) {
        nodes { url }
        pageInfo { hasNextPage }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}"#;

const ISSUE_HISTORY_QUERY: &str = r#"
query IssueHistory($id: String!, $first: Int!, $after: String) {
  issue(id: $id) {
    id
    history(first: $first, after: $after) {
      nodes { id createdAt fromState { name type } toState { name type } }
      pageInfo { hasNextPage endCursor }
    }
  }
}"#;

const HISTORY_PAGE_SIZE: i32 = 100;

const PROJECT_STATUSES_QUERY: &str = r#"
query ProjectStatuses($first: Int!, $after: String) {
  projectStatuses(first: $first, after: $after) {
    nodes { id name type team { id } }
    pageInfo { hasNextPage endCursor }
  }
}"#;

const PROJECT_STATUS_SCOPE_QUERY: &str = r#"
query ProjectStatusScope($id: String!) {
  project(id: $id) { id status { team { id } } }
}"#;

const PROJECT_UPDATE_MUTATION: &str = r#"
mutation ProjectUpdate($id: String!, $input: ProjectUpdateInput!) {
  projectUpdate(id: $id, input: $input) {
    success
    project { id }
  }
}"#;

/// Linear GraphQL client for structured project/issue reads and scoped project updates.
pub struct LinearGraphQlSource {
    client: Client,
    api_key: SecretValue,
    endpoint: String,
    cache_namespace: String,
}

impl LinearGraphQlSource {
    /// Creates a source using the supplied Linear API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        Self::with_endpoint(api_key, ENDPOINT)
    }

    fn with_endpoint(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<Self, Error> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(Error::MissingApiKey);
        }
        let cache_namespace = format!(
            "linear:{CACHE_MODEL_VERSION}:sha256:{:x}",
            Sha256::digest(api_key.as_bytes())
        );
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            api_key: SecretValue::new(api_key),
            endpoint: endpoint.into(),
            cache_namespace,
        })
    }

    /// Resolves LINEAR_API_KEY, the saved OS credential, then the configured Linear CLI.
    pub fn from_env() -> Result<Self, Error> {
        let scope = AppScope::try_from("ix-board")?;
        let key = SecretKey::try_from("linear-api-key")?;
        if let Some(resolved) =
            SecretStore::system().resolve(None, Some("LINEAR_API_KEY"), &scope, &key)?
        {
            return Self::new(resolved.value.expose_secret());
        }
        Self::new(linear_cli_token().ok_or(Error::MissingApiKey)?)
    }

    /// Reports which configured credential source is available without exposing its value.
    pub fn credential_source() -> Result<Option<String>, Error> {
        let scope = AppScope::try_from("ix-board")?;
        let key = SecretKey::try_from("linear-api-key")?;
        if let Some(resolved) =
            SecretStore::system().resolve(None, Some("LINEAR_API_KEY"), &scope, &key)?
        {
            return Ok(Some(resolved.source.as_str().to_owned()));
        }
        Ok(linear_cli_token().map(|_| "linear_cli".to_owned()))
    }

    /// Fetches every workspace initiative through Linear's paged initiatives connection.
    pub fn fetch_initiatives(&self) -> Result<Vec<Initiative>, Error> {
        let mut initiatives = Vec::new();
        let mut after = None;
        for page in 0..PAGE_LIMIT {
            let result: InitiativeData = self.request(
                INITIATIVES_QUERY,
                PageVariables {
                    first: INITIATIVE_PAGE_SIZE,
                    after,
                },
            )?;
            let connection = result.initiatives;
            initiatives.extend(connection.nodes);
            if !connection.page_info.has_next_page {
                return Ok(initiatives);
            }
            after = Some(
                connection
                    .page_info
                    .end_cursor
                    .ok_or(Error::MissingCursor)?,
            );
            if page + 1 == PAGE_LIMIT {
                return Err(Error::PaginationLimit { pages: PAGE_LIMIT });
            }
        }
        Err(Error::PaginationLimit { pages: PAGE_LIMIT })
    }

    /// Collects current Linear scope and issue transitions for live priority-goal drift.
    pub fn collect_drift_evidence(&self, config: &DriftConfig) -> Result<DriftEvidence, Error> {
        let report_epoch = crate::drift::timestamp_epoch(&config.report_time)?;
        let window_start = report_epoch - config.window_hours * 3600.0;
        let snapshot = self.fetch_workspace()?;
        let projects_by_id: HashMap<_, _> = snapshot
            .projects
            .iter()
            .map(|project| (project.id.0.as_str(), project))
            .collect();
        let milestones_by_id: HashMap<_, _> = snapshot
            .projects
            .iter()
            .flat_map(|project| {
                project
                    .milestones
                    .iter()
                    .map(move |milestone| (milestone.id.as_str(), milestone.name.as_str()))
            })
            .collect();
        let mut evidence = DriftEvidence {
            projects: snapshot
                .projects
                .iter()
                .map(|project| HistoricalProject {
                    id: project.id.0.clone(),
                    identifier: project.identifier.clone(),
                    team_keys: project
                        .team_keys
                        .iter()
                        .map(|team| team.0.clone())
                        .collect(),
                    status: project.status.clone(),
                    priority: project.priority,
                    sort_order: project.sort_order,
                })
                .collect(),
            ..DriftEvidence::default()
        };

        let mut history_issue_ids = HashSet::new();
        let mut priority_goal_team_keys = HashSet::new();
        let mut priority_goal_assignees = HashSet::new();
        for goal in &config.priority_goals {
            let project_exists = snapshot.projects.iter().any(|project| {
                project.identifier.as_deref() == Some(goal.goal.as_str())
                    || project.name == goal.goal
                    || project
                        .milestones
                        .iter()
                        .any(|milestone| milestone.name == goal.goal)
            }) || goal
                .ticket_ids
                .iter()
                .any(|id| snapshot.issues.iter().any(|issue| issue.identifier == *id));
            if !project_exists {
                evidence.missing.push(crate::MissingEvidence {
                    source: "linear.issues".to_owned(),
                    reason: format!("configured priority goal '{}' was not found", goal.goal),
                });
            }
            for ticket_id in &goal.ticket_ids {
                if !snapshot
                    .issues
                    .iter()
                    .any(|issue| issue.identifier.eq_ignore_ascii_case(ticket_id))
                {
                    evidence.missing.push(crate::MissingEvidence {
                        source: "linear.issues".to_owned(),
                        reason: format!(
                            "configured priority goal '{}' ticket '{}' was not found",
                            goal.goal, ticket_id
                        ),
                    });
                }
            }
        }

        let issue_matches_goal = |issue: &Issue| {
            let project_identifier = issue
                .project_id
                .as_ref()
                .and_then(|id| projects_by_id.get(id.0.as_str()))
                .map(|project| {
                    project
                        .identifier
                        .clone()
                        .unwrap_or_else(|| project.name.clone())
                });
            let milestone_identifier = issue
                .project_milestone_id
                .as_deref()
                .and_then(|id| milestones_by_id.get(id))
                .map(|name| (*name).to_owned());
            config.priority_goals.iter().any(|goal| {
                project_identifier.as_deref() == Some(goal.goal.as_str())
                    || milestone_identifier.as_deref() == Some(goal.goal.as_str())
                    || goal
                        .ticket_ids
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&issue.identifier))
            })
        };
        let issues_by_id: HashMap<_, _> = snapshot
            .issues
            .iter()
            .map(|issue| (issue.id.0.as_str(), issue))
            .collect();
        let mut pending_blockers = Vec::new();
        for issue in snapshot
            .issues
            .iter()
            .filter(|issue| issue_matches_goal(issue))
        {
            priority_goal_team_keys.insert(issue.team_key.0.clone());
            if let Some(assignee) = issue.assignee_id.as_deref() {
                priority_goal_assignees.insert(assignee.to_owned());
            }
            pending_blockers.extend(issue.blocked_by.iter().map(|id| id.0.clone()));
        }
        let mut visited_blockers = HashSet::new();
        while let Some(blocker_id) = pending_blockers.pop() {
            if !visited_blockers.insert(blocker_id.clone()) {
                continue;
            }
            history_issue_ids.insert(blocker_id.clone());
            if let Some(blocker) = issues_by_id.get(blocker_id.as_str()) {
                if let Some(assignee) = blocker.assignee_id.as_deref() {
                    priority_goal_assignees.insert(assignee.to_owned());
                }
                pending_blockers.extend(blocker.blocked_by.iter().map(|id| id.0.clone()));
            }
        }
        if !priority_goal_team_keys.is_empty() && priority_goal_assignees.is_empty() {
            evidence.missing.push(crate::MissingEvidence {
                source: "linear.issues".to_owned(),
                reason: "priority goal issues and their blockers have no assignee identities; cross-team activity cannot be attributed".to_owned(),
            });
        }

        for issue in &snapshot.issues {
            let project = issue
                .project_id
                .as_ref()
                .and_then(|id| projects_by_id.get(id.0.as_str()));
            let project_identifier = project.map(|project| {
                project
                    .identifier
                    .clone()
                    .unwrap_or_else(|| project.name.clone())
            });
            let milestone_identifier = issue
                .project_milestone_id
                .as_deref()
                .and_then(|id| milestones_by_id.get(id))
                .map(|name| (*name).to_owned());
            let belongs_to_goal = config.priority_goals.iter().any(|goal| {
                project_identifier.as_deref() == Some(goal.goal.as_str())
                    || milestone_identifier.as_deref() == Some(goal.goal.as_str())
                    || goal
                        .ticket_ids
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&issue.identifier))
            });
            let recently_updated = issue
                .updated_at
                .as_deref()
                .map(crate::drift::timestamp_epoch)
                .transpose()?
                .is_some_and(|updated| updated >= window_start);
            let relevant_to_goal_capacity = priority_goal_team_keys.contains(&issue.team_key.0)
                || issue
                    .assignee_id
                    .as_ref()
                    .is_some_and(|assignee| priority_goal_assignees.contains(assignee));
            if belongs_to_goal || (recently_updated && relevant_to_goal_capacity) {
                history_issue_ids.insert(issue.id.0.clone());
            }
            let Some(created_at) = issue.created_at.clone() else {
                evidence.missing.push(crate::MissingEvidence {
                    source: "linear.issues".to_owned(),
                    reason: format!("{} has no creation timestamp", issue.identifier),
                });
                continue;
            };
            evidence.tickets.push(DriftTicket {
                id: issue.id.0.clone(),
                identifier: issue.identifier.clone(),
                team_key: issue.team_key.0.clone(),
                assignee_id: issue.assignee_id.clone(),
                project_id: issue.project_id.as_ref().map(|id| id.0.clone()),
                project_identifier,
                milestone_identifier,
                linked_pr_urls: issue
                    .attachment_urls
                    .iter()
                    .filter(|url| url.contains("github.com/") && url.contains("/pull/"))
                    .cloned()
                    .collect(),
                state: issue.state.clone(),
                state_type: issue.state_type,
                spec_state: "unknown".to_owned(),
                priority: issue.priority,
                sort_order: issue.sort_order,
                blocked_by: issue.blocked_by.iter().map(|id| id.0.clone()).collect(),
                created_at,
                updated_at: issue.updated_at.clone(),
                state_entered_at: None,
            });
        }

        // Goal progress depends on the whole transitive blocker chain, even when
        // a blocker itself has not recently changed state.

        let mut history_ids: Vec<_> = history_issue_ids.into_iter().collect();
        history_ids.sort();
        let mut state_events: HashMap<String, Vec<(String, Option<String>, String)>> =
            HashMap::new();
        for issue_id in history_ids {
            let mut after = None;
            for page in 0..PAGE_LIMIT {
                let result: IssueHistoryData = self.request(
                    ISSUE_HISTORY_QUERY,
                    IssueHistoryVariables {
                        id: issue_id.clone(),
                        first: HISTORY_PAGE_SIZE,
                        after: after.clone(),
                    },
                )?;
                let Some(issue) = result.issue else {
                    evidence.missing.push(crate::MissingEvidence {
                        source: "linear.history".to_owned(),
                        reason: format!("history was unavailable for issue {issue_id}"),
                    });
                    break;
                };
                for event in issue.history.nodes {
                    let Some(to_state) = event.to_state else {
                        continue;
                    };
                    let to_state = canonical_issue_state(to_state.name, to_state.status_type);
                    let from_state = event
                        .from_state
                        .map(|state| canonical_issue_state(state.name, state.status_type));
                    state_events.entry(issue_id.clone()).or_default().push((
                        event.created_at.clone(),
                        from_state,
                        to_state.clone(),
                    ));
                    evidence.transitions.push(IssueTransition {
                        event_id: event.id,
                        issue_id: issue_id.clone(),
                        ticket_id: snapshot
                            .issues
                            .iter()
                            .find(|ticket| ticket.id.0 == issue_id)
                            .map(|ticket| ticket.identifier.clone())
                            .unwrap_or_else(|| issue_id.clone()),
                        to_state,
                        occurred_at: event.created_at,
                        queue_before: None,
                    });
                }
                if !issue.history.page_info.has_next_page {
                    break;
                }
                after = issue.history.page_info.end_cursor;
                if after.is_none() {
                    evidence.missing.push(crate::MissingEvidence {
                        source: "linear.history".to_owned(),
                        reason: format!("history page cursor was missing for issue {issue_id}"),
                    });
                    break;
                }
                if page + 1 == PAGE_LIMIT {
                    evidence.missing.push(crate::MissingEvidence {
                        source: "linear.history".to_owned(),
                        reason: format!(
                            "history exceeded the {PAGE_LIMIT}-page limit for issue {issue_id}"
                        ),
                    });
                }
            }
        }

        for issue in &snapshot.issues {
            let Some(ticket) = evidence.tickets.iter_mut().find(|row| row.id == issue.id.0) else {
                continue;
            };
            let created_epoch = crate::drift::timestamp_epoch(&ticket.created_at)?;
            if created_epoch > report_epoch {
                continue;
            }
            let events = state_events.entry(issue.id.0.clone()).or_default();
            events.sort_by(|left, right| {
                let left_at = crate::drift::timestamp_epoch(&left.0).unwrap_or(f64::NEG_INFINITY);
                let right_at = crate::drift::timestamp_epoch(&right.0).unwrap_or(f64::NEG_INFINITY);
                left_at.total_cmp(&right_at)
            });
            let future_state = events
                .iter()
                .filter_map(|(at, from_state, _)| {
                    let at_epoch = crate::drift::timestamp_epoch(at).ok()?;
                    (at_epoch > report_epoch).then_some((at_epoch, at, from_state))
                })
                .min_by(|left, right| left.0.total_cmp(&right.0));
            let prior_state = events
                .iter()
                .filter_map(|(at, _, to_state)| {
                    let at_epoch = crate::drift::timestamp_epoch(at).ok()?;
                    (at_epoch <= report_epoch).then_some((at_epoch, at, to_state))
                })
                .max_by(|left, right| left.0.total_cmp(&right.0));
            if let Some((_, _, Some(state))) = future_state {
                ticket.state = state.clone();
                ticket.state_entered_at = events
                    .iter()
                    .filter_map(|(at, _, to_state)| {
                        let at_epoch = crate::drift::timestamp_epoch(at).ok()?;
                        (at_epoch <= report_epoch && to_state == state)
                            .then_some((at_epoch, at.as_str()))
                    })
                    .max_by(|left, right| left.0.total_cmp(&right.0))
                    .map(|(_, at)| at.to_owned())
                    .or_else(|| Some(ticket.created_at.clone()));
            } else if let Some((_, entered_at, state)) = prior_state {
                ticket.state = state.clone();
                ticket.state_entered_at = Some(entered_at.clone());
            } else {
                ticket.state_entered_at = Some(ticket.created_at.clone());
            }
        }
        evidence.tickets.retain(|ticket| {
            crate::drift::timestamp_epoch(&ticket.created_at)
                .is_ok_and(|created| created <= report_epoch)
        });

        evidence.missing.extend([
            crate::MissingEvidence {
                source: "github.prs".to_owned(),
                reason: "live drift reads Linear issues and state history only".to_owned(),
            },
            crate::MissingEvidence {
                source: "github.commits".to_owned(),
                reason: "live drift reads Linear issues and state history only".to_owned(),
            },
            crate::MissingEvidence {
                source: "herdr.busy".to_owned(),
                reason: "no live Herdr source is configured".to_owned(),
            },
            crate::MissingEvidence {
                source: "otel.spend".to_owned(),
                reason: "no live OTel source is configured".to_owned(),
            },
        ]);
        Ok(evidence)
    }

    fn query<T: DeserializeOwned>(
        &self,
        query: &'static str,
        first: i32,
        after: Option<&str>,
        relation_first: Option<i32>,
        nested_first: Option<i32>,
    ) -> Result<T, Error> {
        self.request(
            query,
            Variables {
                first,
                relation_first,
                nested_first,
                after: after.map(str::to_owned),
            },
        )
    }

    fn request<T: DeserializeOwned, V: Serialize>(
        &self,
        query: &'static str,
        variables: V,
    ) -> Result<T, Error> {
        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::AUTHORIZATION, self.api_key.expose_secret())
            .json(&GraphQlRequest { query, variables })
            .send()?;
        let status_error = response.error_for_status_ref().err();
        let retry_after = rate_limit_delay(response.headers(), std::time::SystemTime::now());
        // Linear deliberately uses HTTP 400 for GraphQL RATELIMITED responses.
        // Inspect its structured errors before returning the HTTP status error.
        let response = match response.json::<GraphQlResponse<T>>() {
            Ok(response) => response,
            Err(error) => return Err(Error::Http(status_error.unwrap_or(error))),
        };
        if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
            if errors.iter().any(|error| {
                error
                    .extensions
                    .as_ref()
                    .and_then(|ext| ext.code.as_deref())
                    == Some("RATELIMITED")
            }) {
                return Err(Error::LinearRateLimited { retry_after });
            }
            return Err(Error::GraphQl {
                messages: errors
                    .into_iter()
                    .map(|error| error.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            });
        }
        if let Some(error) = status_error {
            return Err(Error::Http(error));
        }
        response.data.ok_or(Error::MissingWorkspaceData)
    }
}

impl WorkspaceSource for LinearGraphQlSource {
    fn cache_namespace(&self) -> String {
        self.cache_namespace.clone()
    }

    fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, Error> {
        let mut projects = Vec::new();
        let mut after = None;
        for page in 0..PAGE_LIMIT {
            let result: ProjectData = self.query(
                PROJECT_QUERY,
                PROJECT_PAGE_SIZE,
                after.as_deref(),
                None,
                Some(PROJECT_NESTED_PAGE_SIZE),
            )?;
            let connection = result.projects;
            for raw in connection.nodes {
                if raw.teams.page_info.has_next_page {
                    return Err(Error::NestedPaginationLimit {
                        resource: format!("project {} teams", raw.id),
                    });
                }
                if raw.project_milestones.page_info.has_next_page {
                    return Err(Error::NestedPaginationLimit {
                        resource: format!("project {} milestones", raw.id),
                    });
                }
                if raw.initiatives.page_info.has_next_page {
                    return Err(Error::NestedPaginationLimit {
                        resource: format!("project {} initiatives", raw.id),
                    });
                }
                projects.push(Project {
                    initiatives: raw.initiatives.nodes,
                    id: ProjectId(raw.id),
                    identifier: raw.identifier,
                    name: raw.name,
                    updated_at: raw.updated_at,
                    description: raw.description,
                    content: raw.content,
                    status: canonical_project_status(raw.status.name, raw.status.status_type),
                    priority: raw.priority,
                    sort_order: raw.sort_order,
                    target_date: raw.target_date,
                    team_keys: raw
                        .teams
                        .nodes
                        .into_iter()
                        .map(|team| TeamKey(team.key))
                        .collect(),
                    milestones: raw
                        .project_milestones
                        .nodes
                        .into_iter()
                        .map(|milestone| Milestone {
                            id: milestone.id,
                            name: milestone.name,
                            description: milestone.description,
                            sort_order: milestone.sort_order,
                            target_date: milestone.target_date,
                        })
                        .collect(),
                });
            }
            if !connection.page_info.has_next_page {
                break;
            }
            after = Some(
                connection
                    .page_info
                    .end_cursor
                    .ok_or(Error::MissingCursor)?,
            );
            if page + 1 == PAGE_LIMIT {
                return Err(Error::PaginationLimit { pages: PAGE_LIMIT });
            }
        }

        let mut raw_issues = Vec::new();
        let mut after = None;
        for page in 0..PAGE_LIMIT {
            let result: IssueData = self.query(
                ISSUE_QUERY,
                ISSUE_PAGE_SIZE,
                after.as_deref(),
                Some(RELATION_PAGE_SIZE),
                None,
            )?;
            let connection = result.issues;
            for raw in connection.nodes {
                if raw.relations.page_info.has_next_page {
                    return Err(Error::RelationsLimit {
                        issue: raw.identifier,
                    });
                }
                raw_issues.push(raw);
            }
            if !connection.page_info.has_next_page {
                break;
            }
            after = Some(
                connection
                    .page_info
                    .end_cursor
                    .ok_or(Error::MissingCursor)?,
            );
            if page + 1 == PAGE_LIMIT {
                return Err(Error::PaginationLimit { pages: PAGE_LIMIT });
            }
        }

        let blocked_by = collect_blocked_by(&raw_issues);
        let issues = raw_issues
            .into_iter()
            .map(|raw| {
                let state_type = raw.state.status_type;
                let state_name = raw.state.name;
                let state = canonical_issue_state(state_name.clone(), state_type);
                Issue {
                    id: IssueId(raw.id.clone()),
                    identifier: raw.identifier,
                    title: raw.title,
                    description: raw.description,
                    state,
                    state_name,
                    state_type,
                    parent_id: raw.parent.map(|p| IssueId(p.id)),
                    team_key: TeamKey(raw.team.key),
                    assignee_id: raw.assignee.map(|assignee| assignee.id),
                    project_id: raw.project.map(|project| ProjectId(project.id)),
                    project_milestone_id: raw.project_milestone.map(|milestone| milestone.id),
                    priority: raw.priority,
                    sort_order: raw.sort_order,
                    blocked_by: blocked_by
                        .get(&raw.id)
                        .into_iter()
                        .flat_map(|ids| ids.iter().cloned())
                        .map(IssueId)
                        .collect(),
                    created_at: raw.created_at,
                    updated_at: raw.updated_at,
                    attachments_complete: raw
                        .attachments
                        .as_ref()
                        .is_some_and(|attachments| !attachments.page_info.has_next_page),
                    attachment_urls: raw
                        .attachments
                        .into_iter()
                        .flat_map(|attachments| attachments.nodes)
                        .map(|item| item.url)
                        .collect(),
                }
            })
            .collect();
        Ok(WorkspaceSnapshot {
            projects,
            issues,
            fetched_at: SystemClock.now_epoch_seconds(),
        })
    }
}

impl ProjectMutationSource for LinearGraphQlSource {
    fn set_project_status(
        &self,
        project_id: &ProjectId,
        status: ProjectRunStatus,
    ) -> Result<(), Error> {
        let scope: ProjectStatusScopeData = self.request(
            PROJECT_STATUS_SCOPE_QUERY,
            ProjectIdVariables {
                id: project_id.0.clone(),
            },
        )?;
        let Some(project) = scope.project else {
            return Err(Error::ProjectNotFound {
                identifier: project_id.0.clone(),
            });
        };
        let team_id = project.status.team.map(|team| team.id);
        let statuses = self.project_statuses()?;
        let status_id = resolve_project_status_id(&statuses, status, team_id.as_deref())?;
        self.update_project(
            project_id,
            ProjectUpdateInput {
                status_id: Some(status_id),
                priority: None,
                sort_order: None,
            },
        )
    }

    fn set_project_priority(
        &self,
        project_id: &ProjectId,
        priority: ProjectPriority,
    ) -> Result<(), Error> {
        self.update_project(
            project_id,
            ProjectUpdateInput {
                status_id: None,
                priority: Some(priority.linear_value()),
                sort_order: None,
            },
        )
    }

    fn set_project_sort_order(&self, project_id: &ProjectId, sort_order: f64) -> Result<(), Error> {
        if !sort_order.is_finite() {
            return Err(Error::InvalidProjectSortOrder);
        }
        self.update_project(
            project_id,
            ProjectUpdateInput {
                status_id: None,
                priority: None,
                sort_order: Some(sort_order),
            },
        )
    }
}

impl LinearGraphQlSource {
    fn project_statuses(&self) -> Result<Vec<RawProjectStatusOption>, Error> {
        let mut statuses = Vec::new();
        let mut after = None;
        for page in 0..PAGE_LIMIT {
            let result: ProjectStatusesData = self.query(
                PROJECT_STATUSES_QUERY,
                PROJECT_PAGE_SIZE,
                after.as_deref(),
                None,
                None,
            )?;
            let connection = result.project_statuses;
            statuses.extend(connection.nodes);
            if !connection.page_info.has_next_page {
                return Ok(statuses);
            }
            after = Some(
                connection
                    .page_info
                    .end_cursor
                    .ok_or(Error::MissingCursor)?,
            );
            if page + 1 == PAGE_LIMIT {
                return Err(Error::PaginationLimit { pages: PAGE_LIMIT });
            }
        }
        Err(Error::PaginationLimit { pages: PAGE_LIMIT })
    }

    fn update_project(
        &self,
        project_id: &ProjectId,
        input: ProjectUpdateInput,
    ) -> Result<(), Error> {
        let result: ProjectUpdateData = self.request(
            PROJECT_UPDATE_MUTATION,
            ProjectUpdateVariables {
                id: project_id.0.clone(),
                input,
            },
        )?;
        let payload = result.project_update;
        if !payload.success
            || payload
                .project
                .is_none_or(|project| project.id != project_id.0)
        {
            return Err(Error::ProjectUpdateRejected {
                identifier: project_id.0.clone(),
            });
        }
        Ok(())
    }
}

fn resolve_project_status_id(
    statuses: &[RawProjectStatusOption],
    requested: ProjectRunStatus,
    team_id: Option<&str>,
) -> Result<String, Error> {
    let matching = statuses
        .iter()
        .filter(|status| status.team.as_ref().map(|team| team.id.as_str()) == team_id)
        .filter(|status| match requested {
            ProjectRunStatus::Started => status.status_type == LinearProjectStatusType::Started,
            ProjectRunStatus::Paused => status.name == "Paused",
        })
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [status] => Ok(status.id.clone()),
        [] => Err(Error::ProjectStatusUnavailable {
            status: requested.as_str().to_owned(),
        }),
        _ => Err(Error::ProjectStatusAmbiguous {
            status: requested.as_str().to_owned(),
        }),
    }
}

fn canonical_project_status(name: String, status_type: LinearProjectStatusType) -> String {
    if name == ProjectRunStatus::Paused.as_str() {
        ProjectRunStatus::Paused.as_str().to_owned()
    } else if status_type == LinearProjectStatusType::Started {
        ProjectRunStatus::Started.as_str().to_owned()
    } else {
        name
    }
}

fn collect_blocked_by(raw_issues: &[RawIssue]) -> BTreeMap<String, BTreeSet<String>> {
    let mut blocked_by = BTreeMap::<String, BTreeSet<String>>::new();
    for issue in raw_issues {
        for relation in &issue.relations.nodes {
            if relation.relation_type == "blocks" {
                blocked_by
                    .entry(relation.related_issue.id.clone())
                    .or_default()
                    .insert(relation.issue.id.clone());
            }
        }
    }
    blocked_by
}

#[derive(Serialize)]
struct GraphQlRequest<'a, V> {
    query: &'a str,
    variables: V,
}

#[derive(Serialize)]
struct Variables {
    first: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "relationFirst")]
    relation_first: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "nestedFirst")]
    nested_first: Option<i32>,
    after: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageVariables {
    first: i32,
    after: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IssueHistoryVariables {
    id: String,
    first: i32,
    after: Option<String>,
}

#[derive(Serialize)]
struct ProjectIdVariables {
    id: String,
}

#[derive(Serialize)]
struct ProjectUpdateVariables {
    id: String,
    input: ProjectUpdateInput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectUpdateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    status_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_order: Option<f64>,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
    extensions: Option<GraphQlErrorExtensions>,
}

// GraphQL error extensions are an open metadata map. Consume the selected
// discriminant while allowing Linear's unrelated error metadata.
#[derive(Deserialize)]
struct GraphQlErrorExtensions {
    code: Option<String>,
}

fn rate_limit_delay(
    headers: &reqwest::header::HeaderMap,
    now: std::time::SystemTime,
) -> std::time::Duration {
    let now_ms = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    // Multiple quotas can be exhausted at once; wait until all indicated
    // request/endpoint/complexity reset times have elapsed.
    let reset_ms = ["requests", "endpoint-requests", "complexity"]
        .into_iter()
        .filter_map(|quota| {
            let remaining = headers
                .get(format!("x-ratelimit-{quota}-remaining"))?
                .to_str()
                .ok()?;
            if remaining.parse::<u64>().ok()? != 0 {
                return None;
            }
            headers
                .get(format!("x-ratelimit-{quota}-reset"))?
                .to_str()
                .ok()?
                .parse::<u64>()
                .ok()
        })
        .max();
    reset_ms
        .and_then(|reset| u64::try_from(u128::from(reset).saturating_sub(now_ms)).ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(60))
        .max(std::time::Duration::from_secs(60))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    nodes: Vec<T>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectData {
    projects: Connection<RawProject>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitiativeData {
    initiatives: Connection<Initiative>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct ProjectStatusesData {
    project_statuses: Connection<RawProjectStatusOption>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectStatusScopeData {
    project: Option<RawProjectStatusScope>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjectStatusScope {
    #[serde(rename = "id")]
    _id: String,
    status: RawProjectStatusScopeStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjectStatusScopeStatus {
    team: Option<RawStatusTeam>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStatusTeam {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct ProjectUpdateData {
    project_update: ProjectUpdatePayload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectUpdatePayload {
    success: bool,
    project: Option<UpdatedProject>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdatedProject {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawProject {
    id: String,
    identifier: Option<String>,
    name: String,
    updated_at: Option<String>,
    description: String,
    content: Option<String>,
    priority: i32,
    sort_order: f64,
    target_date: Option<String>,
    status: RawProjectStatus,
    teams: Connection<RawTeam>,
    project_milestones: Connection<RawMilestone>,
    initiatives: Connection<Initiative>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjectStatus {
    name: String,
    #[serde(rename = "type")]
    status_type: LinearProjectStatusType,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum LinearProjectStatusType {
    Backlog,
    Planned,
    Started,
    Paused,
    Completed,
    Canceled,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawProjectStatusOption {
    id: String,
    name: String,
    #[serde(rename = "type")]
    status_type: LinearProjectStatusType,
    team: Option<RawStatusTeam>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTeam {
    key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUser {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawMilestone {
    id: String,
    name: String,
    description: Option<String>,
    sort_order: f64,
    target_date: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueData {
    issues: Connection<RawIssue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueHistoryData {
    issue: Option<RawIssueHistory>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawIssueHistory {
    #[serde(rename = "id")]
    _id: String,
    history: Connection<RawIssueHistoryEvent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawIssueHistoryEvent {
    id: String,
    created_at: String,
    from_state: Option<RawState>,
    to_state: Option<RawState>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    id: String,
    identifier: String,
    title: String,
    description: Option<String>,
    priority: f64,
    sort_order: f64,
    created_at: Option<String>,
    updated_at: Option<String>,
    state: RawState,
    team: RawTeam,
    assignee: Option<RawUser>,
    project: Option<RawIssueProject>,
    project_milestone: Option<RawIssueProject>,
    parent: Option<RawIssueProject>,
    relations: Connection<RawRelation>,
    attachments: Option<Connection<RawAttachment>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawAttachment {
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawState {
    name: String,
    #[serde(rename = "type")]
    status_type: WorkflowStateType,
}

fn canonical_issue_state(name: String, status_type: WorkflowStateType) -> String {
    match status_type {
        WorkflowStateType::Completed => "Completed".to_owned(),
        WorkflowStateType::Canceled => "Canceled".to_owned(),
        WorkflowStateType::Duplicate => "Duplicate".to_owned(),
        WorkflowStateType::Triage
        | WorkflowStateType::Backlog
        | WorkflowStateType::Unstarted
        | WorkflowStateType::Started => name,
    }
}

fn linear_cli_token() -> Option<String> {
    let output = ProcessCommand::new("linear")
        .args(["auth", "token"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawIssueProject {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawRelation {
    #[serde(rename = "type")]
    relation_type: String,
    issue: RawRelationIssue,
    related_issue: RawRelationIssue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelationIssue {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::{
        CACHE_MODEL_VERSION, INITIATIVE_PAGE_SIZE, ISSUE_PAGE_SIZE, IssueData, LinearGraphQlSource,
        LinearProjectStatusType, PROJECT_PAGE_SIZE, PROJECT_UPDATE_MUTATION, ProjectData,
        ProjectUpdateInput, RawProjectStatusOption, canonical_project_status, collect_blocked_by,
        resolve_project_status_id,
    };
    use crate::Error;
    use crate::{
        Issue, IssueId, Project, ProjectId, ProjectMutationSource, ProjectPriority,
        ProjectRunStatus, ReadService, TeamKey, WorkspaceSnapshot, WorkspaceSource,
    };
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::JoinHandle;

    struct CachedSnapshotSource {
        namespace: String,
        snapshot: WorkspaceSnapshot,
    }

    impl WorkspaceSource for CachedSnapshotSource {
        fn fetch_workspace(&self) -> Result<WorkspaceSnapshot, crate::Error> {
            Ok(self.snapshot.clone())
        }

        fn cache_namespace(&self) -> String {
            self.namespace.clone()
        }
    }

    fn mock_linear(responses: Vec<Value>) -> (String, JoinHandle<Vec<Value>>) {
        mock_linear_http(
            responses
                .into_iter()
                .map(|body| (200, Vec::new(), body))
                .collect(),
        )
    }

    #[test]
    fn workspace_initiatives_follow_recorded_pages() {
        let first: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/linear/initiative-page-one.json"
        ))
        .expect("decode first recorded initiative page");
        let second: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/linear/initiative-page-two.json"
        ))
        .expect("decode second recorded initiative page");
        let (endpoint, server) = mock_linear(vec![first, second]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");
        let initiatives = source.fetch_initiatives().expect("all initiative pages");
        let requests = server.join().expect("mock server");
        assert_eq!(
            initiatives
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["QSL Core", "QSL Ecosystem"]
        );
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["variables"]["first"], INITIATIVE_PAGE_SIZE);
        assert!(requests[0]["variables"]["after"].is_null());
        assert_eq!(requests[1]["variables"]["after"], "cursor-core");
    }

    type MockResponse = (u16, Vec<(&'static str, String)>, Value);

    fn mock_linear_http(responses: Vec<MockResponse>) -> (String, JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Linear endpoint");
        let endpoint = format!(
            "http://{}/graphql",
            listener.local_addr().expect("local addr")
        );
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, headers, response) in responses {
                let (mut stream, _) = listener.accept().expect("accept GraphQL request");
                requests.push(read_request(&mut stream).0);
                let body = serde_json::to_vec(&response).expect("encode GraphQL response");
                write!(
                    stream,
                    "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                )
                .expect("write response headers");
                for (name, value) in headers {
                    write!(stream, "{name}: {value}\r\n").expect("write quota header");
                }
                stream.write_all(b"\r\n").expect("end headers");
                stream.write_all(&body).expect("write response body");
            }
            requests
        });
        (endpoint, server)
    }

    fn read_request(stream: &mut TcpStream) -> (Value, String) {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let (header_end, content_length) = loop {
            let count = stream.read(&mut buffer).expect("read request");
            assert_ne!(count, 0, "request ended before headers completed");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_text = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = header_text
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("content length"))
                    })
                    .expect("Content-Length header");
                break (header_end + 4, content_length);
            }
        };
        while bytes.len() < header_end + content_length {
            let count = stream.read(&mut buffer).expect("read request body");
            assert_ne!(count, 0, "request ended before body completed");
            bytes.extend_from_slice(&buffer[..count]);
        }
        let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
        let body = serde_json::from_slice(&bytes[header_end..header_end + content_length])
            .expect("decode GraphQL request");
        (body, headers)
    }

    fn satisfy_unused_mock_requests(endpoint: &str, count: usize) {
        let authority = endpoint
            .strip_prefix("http://")
            .and_then(|value| value.strip_suffix("/graphql"))
            .expect("mock endpoint authority");
        let body = br#"{"query":"mock shutdown","variables":{}}"#;
        for _ in 0..count {
            if let Ok(mut stream) = TcpStream::connect(authority) {
                let _ = write!(
                    stream,
                    "POST /graphql HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            }
        }
    }

    fn connection(nodes: Value) -> Value {
        serde_json::json!({
            "nodes": nodes,
            "pageInfo": {"hasNextPage": false, "endCursor": null}
        })
    }

    fn project_status_response() -> Value {
        serde_json::json!({"data": {"project": {"id": "project-id", "status": {"team": null}}}})
    }

    fn project_statuses_response() -> Value {
        serde_json::json!({"data": {"projectStatuses": connection(serde_json::json!([
            {"id": "823c10d9-99c0-44ab-bb7a-26a453d3412f", "name": "In Progress", "type": "started", "team": null},
            {"id": "aa78afd2-2630-412e-82af-45e504471946", "name": "Paused", "type": "planned", "team": null},
            {"id": "planned-id", "name": "Planned", "type": "planned", "team": null}
        ]))}})
    }

    fn update_response() -> Value {
        serde_json::json!({"data": {"projectUpdate": {"success": true, "project": {"id": "project-id"}}}})
    }

    #[test]
    fn status_mutations_discover_live_status_ids_and_write_only_status_id() {
        let (endpoint, server) = mock_linear(vec![
            project_status_response(),
            project_statuses_response(),
            update_response(),
            project_status_response(),
            project_statuses_response(),
            update_response(),
        ]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");

        source
            .set_project_status(
                &ProjectId("project-id".to_owned()),
                ProjectRunStatus::Started,
            )
            .expect("set Started");
        source
            .set_project_status(
                &ProjectId("project-id".to_owned()),
                ProjectRunStatus::Paused,
            )
            .expect("set Paused");

        let requests = server.join().expect("mock server");
        let updates = requests
            .iter()
            .filter(|request| {
                request["query"]
                    .as_str()
                    .is_some_and(|query| query.contains("mutation ProjectUpdate"))
            })
            .collect::<Vec<_>>();
        assert_eq!(updates.len(), 2);
        assert_eq!(
            updates[0]["variables"]["input"],
            serde_json::json!({
                "statusId": "823c10d9-99c0-44ab-bb7a-26a453d3412f"
            })
        );
        assert_eq!(
            updates[1]["variables"]["input"],
            serde_json::json!({
                "statusId": "aa78afd2-2630-412e-82af-45e504471946"
            })
        );
    }

    /// Trace: FR-011-AC-6, FR-012-AC-1
    #[test]
    fn decoded_started_project_snapshot_remains_in_next_queue() {
        let project = serde_json::json!({
            "id": "project-id", "identifier": "SWM-P1", "name": "Project", "description": "Goal",
            "content": null, "priority": 2, "sortOrder": 1.0, "targetDate": "2026-12-01",
            "status": {"name": "In Progress", "type": "started"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "initiatives": connection(serde_json::json!([])),
            "projectMilestones": connection(serde_json::json!([{
                "id": "milestone-id", "name": "M1", "description": null, "sortOrder": 1.0,
                "targetDate": "2026-11-20"
            }]))
        });
        let issue = serde_json::json!({
            "id": "issue-id", "identifier": "SWM-3", "title": "Project ticket", "description": null,
            "priority": 2.0, "sortOrder": 1.0, "state": {"name": "Specced", "type": "started"}, "team": {"key": "SWM"},
            "project": {"id": "project-id"},
            "projectMilestone": {"id": "milestone-id"},
            "relations": connection(serde_json::json!([]))
        });
        let (endpoint, server) = mock_linear(vec![
            serde_json::json!({"data": {"projects": connection(serde_json::json!([project]))}}),
            serde_json::json!({"data": {"issues": connection(serde_json::json!([issue]))}}),
        ]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");
        let cache_path = std::env::temp_dir().join(format!(
            "ix-board-linear-transport-{}.sqlite",
            std::process::id()
        ));
        let mut service = ReadService::open(source, &cache_path).expect("open read service");
        let queue = service.next("SWM").expect("read next queue");
        let snapshot = service.workspace_snapshot().expect("workspace snapshot");
        let requests = server.join().expect("mock server");
        let _ = std::fs::remove_file(cache_path);

        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].identifier, "SWM-3");
        assert_eq!(queue[0].project, "SWM-P1");
        assert_eq!(
            snapshot.issues[0].project_milestone_id.as_deref(),
            Some("milestone-id")
        );
        assert_eq!(
            snapshot.projects[0].target_date.as_deref(),
            Some("2026-12-01")
        );
        assert_eq!(
            snapshot.projects[0].milestones[0].target_date.as_deref(),
            Some("2026-11-20")
        );
        assert_eq!(snapshot.issues[0].state_name, "Specced");
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]["query"]
                .as_str()
                .unwrap()
                .contains("projects(first:")
        );
        assert!(
            requests[0]["query"]
                .as_str()
                .unwrap()
                .contains("targetDate")
        );
        assert!(
            requests[1]["query"]
                .as_str()
                .unwrap()
                .contains("issues(first:")
        );
        assert!(
            requests[1]["query"]
                .as_str()
                .unwrap()
                .contains("projectMilestone { id }")
        );
        assert_eq!(requests[0]["variables"]["first"], PROJECT_PAGE_SIZE);
        assert_eq!(requests[1]["variables"]["first"], ISSUE_PAGE_SIZE);
        assert!(requests.iter().all(|request| {
            !request["query"]
                .as_str()
                .is_some_and(|query| query.contains("github"))
        }));
    }

    #[test]
    fn decoded_paused_project_snapshot_is_excluded_from_next_queue() {
        let project = serde_json::json!({
            "id": "project-id", "identifier": "SWM-P1", "name": "Project", "description": "Goal",
            "content": null, "priority": 2, "sortOrder": 1.0,
            "status": {"name": "Paused", "type": "planned"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "initiatives": connection(serde_json::json!([])),
            "projectMilestones": connection(serde_json::json!([]))
        });
        let issue = serde_json::json!({
            "id": "issue-id", "identifier": "SWM-3", "title": "Project ticket", "description": null,
            "priority": 2.0, "sortOrder": 1.0, "state": {"name": "Specced", "type": "started"}, "team": {"key": "SWM"},
            "project": {"id": "project-id"},
            "relations": connection(serde_json::json!([]))
        });
        let (endpoint, server) = mock_linear(vec![
            serde_json::json!({"data": {"projects": connection(serde_json::json!([project]))}}),
            serde_json::json!({"data": {"issues": connection(serde_json::json!([issue]))}}),
        ]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");
        let cache_path = std::env::temp_dir().join(format!(
            "ix-board-linear-paused-transport-{}.sqlite",
            std::process::id()
        ));
        let mut service = ReadService::open(source, &cache_path).expect("open read service");
        let queue = service.next("SWM").expect("read next queue");
        server.join().expect("mock server");
        let _ = std::fs::remove_file(cache_path);

        assert!(queue.is_empty());
    }

    #[test]
    /// FR-001-AC-2, FR-001-AC-4: verify Linear workflow type mapping and blocker completion.
    fn completed_type_unblocks_and_canceled_type_remains_unresolved() {
        let project = serde_json::json!({
            "id": "project-id", "identifier": "SWM-P1", "name": "Project", "description": "Goal",
            "content": null, "priority": 2, "sortOrder": 1.0,
            "status": {"name": "In Progress", "type": "started"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "initiatives": connection(serde_json::json!([{ "id": "init", "identifier": "I-10", "name": "Delivery" }])),
            "projectMilestones": connection(serde_json::json!([]))
        });
        let completed_blocker = serde_json::json!({
            "id": "completed-blocker", "identifier": "SWM-2", "title": "Done prerequisite",
            "description": null, "priority": 2.0, "sortOrder": 1.0,
            "state": {"name": "Done", "type": "completed"}, "team": {"key": "SWM"},
            "project": null,
            "relations": connection(serde_json::json!([{
                "type": "blocks", "issue": {"id": "completed-blocker"},
                "relatedIssue": {"id": "ready-ticket"}
            }]))
        });
        let canceled_blocker = serde_json::json!({
            "id": "canceled-blocker", "identifier": "SWM-5", "title": "Canceled prerequisite",
            "description": null, "priority": 2.0, "sortOrder": 2.0,
            "state": {"name": "Canceled", "type": "canceled"}, "team": {"key": "SWM"},
            "project": null,
            "relations": connection(serde_json::json!([{
                "type": "blocks", "issue": {"id": "canceled-blocker"},
                "relatedIssue": {"id": "canceled-dependent"}
            }]))
        });
        let ready_ticket = serde_json::json!({
            "parent": {"id": "completed-blocker"}, "id": "ready-ticket", "identifier": "SWM-6", "title": "Now unblocked",
            "description": null, "priority": 2.0, "sortOrder": 3.0,
            "state": {"name": "Specced", "type": "started"}, "team": {"key": "SWM"},
            "project": {"id": "project-id"}, "relations": connection(serde_json::json!([]))
        });
        let canceled_dependent = serde_json::json!({
            "id": "canceled-dependent", "identifier": "SWM-7", "title": "Still blocked",
            "description": null, "priority": 2.0, "sortOrder": 4.0,
            "state": {"name": "Specced", "type": "started"}, "team": {"key": "SWM"},
            "project": {"id": "project-id"}, "relations": connection(serde_json::json!([]))
        });
        let (endpoint, server) = mock_linear(vec![
            serde_json::json!({"data": {"projects": connection(serde_json::json!([project]))}}),
            serde_json::json!({"data": {"issues": connection(serde_json::json!([
                completed_blocker,
                canceled_blocker,
                ready_ticket,
                canceled_dependent
            ]))}}),
        ]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");
        let cache_path = std::env::temp_dir().join(format!(
            "ix-board-linear-completed-transport-{}.sqlite",
            std::process::id()
        ));
        let mut service = ReadService::open(source, &cache_path).expect("open read service");
        let queue = service.next("SWM").expect("read next queue");
        let snapshot = service.workspace_snapshot().unwrap();
        assert_eq!(snapshot.projects[0].initiatives[0].id, "init");
        let child = snapshot
            .issues
            .iter()
            .find(|i| i.identifier == "SWM-6")
            .unwrap();
        assert_eq!(child.parent_id, Some(IssueId("completed-blocker".into())));
        assert_eq!(child.state_type, crate::WorkflowStateType::Started);
        assert!(snapshot.fetched_at > 0);
        server.join().expect("mock server");
        let _ = std::fs::remove_file(cache_path);

        assert_eq!(
            queue
                .iter()
                .map(|item| item.identifier.as_str())
                .collect::<Vec<_>>(),
            ["SWM-6"]
        );
    }

    #[test]
    fn pre_canonicalization_cache_with_done_blocker_is_refetched() {
        let cache_path = std::env::temp_dir().join(format!(
            "ix-board-linear-cache-version-{}.sqlite",
            std::process::id()
        ));
        let old_namespace = format!(
            "linear:v2:sha256:{:x}",
            Sha256::digest("test-key".as_bytes())
        );
        let stale_snapshot = WorkspaceSnapshot {
            fetched_at: 0,

            projects: vec![Project {
                initiatives: Vec::new(),

                id: ProjectId("project-id".to_owned()),
                identifier: Some("SWM-P1".to_owned()),
                name: "Project".to_owned(),
                updated_at: None,
                description: "Goal".to_owned(),
                content: None,
                status: "Started".to_owned(),
                priority: 2,
                sort_order: 1.0,
                target_date: None,
                team_keys: vec![TeamKey("SWM".to_owned())],
                milestones: Vec::new(),
            }],
            issues: vec![
                Issue {
                    parent_id: None,
                    state_type: crate::WorkflowStateType::Unstarted,

                    id: IssueId("blocker-id".to_owned()),
                    identifier: "SWM-2".to_owned(),
                    title: "Done blocker cached before canonicalization".to_owned(),
                    description: None,
                    state: "Done".to_owned(),
                    state_name: "Done".to_owned(),
                    team_key: TeamKey("SWM".to_owned()),
                    assignee_id: None,
                    project_id: None,
                    project_milestone_id: None,
                    priority: 2.0,
                    sort_order: 1.0,
                    blocked_by: Vec::new(),
                    created_at: None,
                    updated_at: None,
                    attachment_urls: Vec::new(),
                    attachments_complete: true,
                },
                Issue {
                    parent_id: None,
                    state_type: crate::WorkflowStateType::Unstarted,

                    id: IssueId("ready-ticket".to_owned()),
                    identifier: "SWM-6".to_owned(),
                    title: "Ready after refresh".to_owned(),
                    description: None,
                    state: "Specced".to_owned(),
                    state_name: "Specced".to_owned(),
                    team_key: TeamKey("SWM".to_owned()),
                    assignee_id: None,
                    project_id: Some(ProjectId("project-id".to_owned())),
                    project_milestone_id: None,
                    priority: 2.0,
                    sort_order: 2.0,
                    blocked_by: vec![IssueId("blocker-id".to_owned())],
                    created_at: None,
                    updated_at: None,
                    attachment_urls: Vec::new(),
                    attachments_complete: true,
                },
            ],
        };
        {
            let mut old_service = ReadService::open(
                CachedSnapshotSource {
                    namespace: old_namespace.clone(),
                    snapshot: stale_snapshot,
                },
                &cache_path,
            )
            .expect("open v2 cache seed service");
            assert!(
                old_service
                    .next("SWM")
                    .expect("read v2 cached queue")
                    .is_empty()
            );
        }

        let project = serde_json::json!({
            "id": "project-id", "identifier": "SWM-P1", "name": "Project", "description": "Goal",
            "content": null, "priority": 2, "sortOrder": 1.0,
            "status": {"name": "In Progress", "type": "started"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "initiatives": connection(serde_json::json!([])),
            "projectMilestones": connection(serde_json::json!([]))
        });
        let blocker = serde_json::json!({
            "id": "blocker-id", "identifier": "SWM-2", "title": "Done blocker",
            "description": null, "priority": 2.0, "sortOrder": 1.0,
            "state": {"name": "Done", "type": "completed"}, "team": {"key": "SWM"},
            "project": null,
            "relations": connection(serde_json::json!([{
                "type": "blocks", "issue": {"id": "blocker-id"},
                "relatedIssue": {"id": "ready-ticket"}
            }]))
        });
        let ready_ticket = serde_json::json!({
            "id": "ready-ticket", "identifier": "SWM-6", "title": "Ready after refresh",
            "description": null, "priority": 2.0, "sortOrder": 2.0,
            "state": {"name": "Specced", "type": "started"}, "team": {"key": "SWM"},
            "project": {"id": "project-id"}, "relations": connection(serde_json::json!([]))
        });
        let (endpoint, server) = mock_linear(vec![
            serde_json::json!({"data": {"projects": connection(serde_json::json!([project]))}}),
            serde_json::json!({"data": {"issues": connection(serde_json::json!([
                blocker,
                ready_ticket
            ]))}}),
        ]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint.clone())
            .expect("source with local endpoint");
        assert_ne!(source.cache_namespace(), old_namespace);
        let mut service =
            ReadService::open(source, &cache_path).expect("open current cache service");
        let queue = service
            .next("SWM")
            .expect("refresh old cache and read next");
        drop(service);
        satisfy_unused_mock_requests(&endpoint, 2);
        let requests = server.join().expect("mock server");
        let _ = std::fs::remove_file(cache_path);

        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].identifier, "SWM-6");
        assert!(requests.iter().any(|request| {
            request["query"]
                .as_str()
                .is_some_and(|query| query.contains("query WorkspaceProjects"))
        }));
        assert!(requests.iter().any(|request| {
            request["query"]
                .as_str()
                .is_some_and(|query| query.contains("query WorkspaceIssues"))
        }));
    }

    /// Trace: FR-004-AC-9, FR-004-AC-10
    #[test]
    fn http_400_rate_limit_is_typed_and_keeps_quota_reset_delay() {
        let (endpoint, server) = mock_linear_http(vec![(
            400,
            vec![
                ("X-RateLimit-Requests-Remaining", "0".into()),
                ("X-RateLimit-Requests-Reset", "1".into()),
            ],
            serde_json::json!({"errors": [{"message": "quota exceeded", "extensions": {"code": "RATELIMITED", "type": "extra server metadata"}}]}),
        )]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint).unwrap();
        let error = source.fetch_workspace().unwrap_err();
        let crate::Error::LinearRateLimited { retry_after } = error else {
            panic!("wrong error: {error:?}")
        };
        assert_eq!(retry_after, std::time::Duration::from_secs(60));
        assert!(
            crate::Error::LinearRateLimited { retry_after }
                .to_string()
                .contains("rate limited")
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }

    /// Trace: FR-004-AC-9, FR-004-AC-10
    #[test]
    fn rate_limit_delay_uses_exhausted_quotas_and_rejects_invalid_metadata() {
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(100);
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in [
            ("x-ratelimit-requests-remaining", "0"),
            ("x-ratelimit-requests-reset", "200000"),
            ("x-ratelimit-endpoint-requests-remaining", "0"),
            ("x-ratelimit-endpoint-requests-reset", "400000"),
            ("x-ratelimit-complexity-remaining", "1"),
            ("x-ratelimit-complexity-reset", "999999"),
        ] {
            headers.insert(name, value.parse().unwrap());
        }
        assert_eq!(
            super::rate_limit_delay(&headers, now),
            std::time::Duration::from_secs(300)
        );
        headers.insert("x-ratelimit-complexity-remaining", "0".parse().unwrap());
        assert_eq!(
            super::rate_limit_delay(&headers, now),
            std::time::Duration::from_millis(899999)
        );
        headers.clear();
        headers.insert("x-ratelimit-requests-remaining", "bad".parse().unwrap());
        headers.insert("x-ratelimit-requests-reset", "bad".parse().unwrap());
        assert_eq!(
            super::rate_limit_delay(&headers, now),
            std::time::Duration::from_secs(60)
        );
        headers.insert("x-ratelimit-requests-remaining", "0".parse().unwrap());
        headers.insert("x-ratelimit-requests-reset", "1".parse().unwrap());
        assert_eq!(
            super::rate_limit_delay(&headers, now),
            std::time::Duration::from_secs(60)
        );
    }

    /// Trace: FR-004-AC-9, FR-004-AC-10
    #[test]
    fn non_rate_graphql_and_http_failures_remain_distinct() {
        for (body, graphql) in [
            (
                serde_json::json!({"errors": [{"message": "invalid query", "extensions": {"code": "OTHER"}}]}),
                true,
            ),
            (serde_json::json!({"proxy": "unavailable"}), false),
        ] {
            let (endpoint, server) = mock_linear_http(vec![(400, vec![], body)]);
            let source = LinearGraphQlSource::with_endpoint("test-key", endpoint).unwrap();
            let error = source.fetch_workspace().unwrap_err();
            match error {
                crate::Error::GraphQl { messages } => {
                    assert!(graphql);
                    assert_eq!(messages, "invalid query");
                }
                crate::Error::Http(error) => {
                    assert!(!graphql);
                    assert_eq!(error.status(), Some(reqwest::StatusCode::BAD_REQUEST));
                }
                other => panic!("wrong error: {other:?}"),
            }
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }

    /// Trace: FR-004-AC-7, FR-004-AC-8
    #[test]
    fn workspace_transport_uses_linear_complexity_safe_page_size() {
        let empty = connection(serde_json::json!([]));
        let (endpoint, server) = mock_linear(vec![
            serde_json::json!({"data": {"projects": empty.clone()}}),
            serde_json::json!({"data": {"issues": empty}}),
        ]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");

        source.fetch_workspace().expect("fetch empty workspace");

        let requests = server.join().expect("mock server");
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]["query"]
                .as_str()
                .is_some_and(|query| query.contains("status { name type }"))
        );
        assert_eq!(requests[0]["variables"]["first"], 20);
        assert_eq!(requests[0]["variables"]["nestedFirst"], 50);
        assert!(requests[1]["query"].as_str().is_some_and(|query| {
            query.contains("state { name type }") && query.contains("assignee { id }")
        }));
        assert_eq!(requests[1]["variables"]["first"], 50);
        assert!(requests[1]["variables"]["nestedFirst"].is_null());
    }

    /// Trace: FR-021-AC-2
    #[test]
    fn reconcile_does_not_claim_all_prs_merged_from_truncated_linear_attachments() {
        let project = serde_json::json!({
            "id": "project-reconcile",
            "identifier": "SWM",
            "name": "Program Review",
            "updatedAt": null,
            "description": "",
            "content": null,
            "priority": 0,
            "sortOrder": 0.0,
            "targetDate": null,
            "status": {"name": "Started", "type": "started"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "projectMilestones": connection(serde_json::json!([])),
            "initiatives": connection(serde_json::json!([]))
        });
        let project_response = serde_json::json!({
            "data": {"projects": connection(serde_json::json!([project]))}
        });
        let issue_response: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/reconcile/linear-oversubscribed-issue-attachments.json"
        ))
        .expect("decode Linear response with more than 50 attachments");
        let (endpoint, server) = mock_linear(vec![project_response, issue_response]);
        let source = LinearGraphQlSource::with_endpoint("test-key", endpoint)
            .expect("source with local endpoint");
        let snapshot = source.fetch_workspace().expect("fetch fixture workspace");
        server.join().expect("mock server");

        let issue = snapshot.issues.first().expect("fixture issue");
        assert_eq!(issue.attachment_urls.len(), 50);
        assert!(!issue.attachments_complete);
        let evidence: crate::reconcile::ReconcileEvidence = serde_json::from_str(include_str!(
            "../tests/fixtures/reconcile/github-oversubscribed-prs.json"
        ))
        .expect("decode GitHub evidence with an unseen open PR");
        let pull_requests = &evidence.pull_requests["agent-ix/reconcile-demo"];
        assert_eq!(pull_requests.len(), 51);
        assert_eq!(pull_requests[50].state, "OPEN");
        assert!(!issue.attachment_urls.contains(&pull_requests[50].url));
        let report = crate::reconcile::evaluate_reconcile(
            snapshot.projects.first().expect("fixture project"),
            &snapshot.issues,
            &evidence,
            vec!["agent-ix/reconcile-demo".to_owned()],
            crate::reconcile::DEFAULT_STALE_DAYS,
            crate::drift::timestamp_epoch("2026-10-21T00:00:00Z").unwrap(),
        )
        .expect("reconcile fixture evidence");

        assert!(report.findings.merged_pr_open_ticket.is_empty());
        assert!(report.warnings.iter().any(|warning| {
            warning.contains("incomplete attachment data")
                && warning.contains("all-linked-PRs-merged")
        }));
    }

    #[test]
    fn typed_graphql_records_decode_selected_camel_case_fields() {
        let project: ProjectData = serde_json::from_value(serde_json::json!({
            "projects": {
                "nodes": [{
                    "id": "project-uuid",
                    "identifier": "SWM-P1",
                    "name": "Project",
                    "description": "Goal",
                    "content": "Project content",
                    "priority": 1,
                    "sortOrder": 2.0,
                    "status": {"name": "In Progress", "type": "started"},
                    "teams": {
                        "nodes": [{"key": "SWM"}],
                        "pageInfo": {"hasNextPage": false}
                    },
                    "initiatives": connection(serde_json::json!([])),
            "projectMilestones": {
                        "nodes": [{
                            "id": "milestone-uuid",
                            "name": "Milestone",
                            "description": null,
                            "sortOrder": 1.0
                        }],
                        "pageInfo": {"hasNextPage": false}
                    }
                }],
                "pageInfo": {"hasNextPage": false, "endCursor": null}
            }
        }))
        .expect("project response");
        assert_eq!(project.projects.nodes[0].status.name, "In Progress");
        assert_eq!(
            project.projects.nodes[0].status.status_type,
            LinearProjectStatusType::Started
        );
        assert_eq!(
            project.projects.nodes[0].project_milestones.nodes[0].name,
            "Milestone"
        );

        let issues: IssueData = serde_json::from_value(serde_json::json!({
            "issues": {
                "nodes": [{
                    "id": "issue-uuid",
                    "identifier": "SWM-2",
                    "title": "Issue",
                    "description": null,
                    "priority": 2.0,
                    "sortOrder": 3.0,
                    "state": {"name": "Specced", "type": "started"},
                    "team": {"key": "SWM"},
                    "assignee": {"id": "worker-uuid"},
                    "project": {"id": "project-uuid"},
                    "relations": {
                        "nodes": [{
                            "type": "blocks",
                            "issue": {"id": "blocker-uuid"},
                            "relatedIssue": {"id": "issue-uuid"}
                        }],
                        "pageInfo": {"hasNextPage": false}
                    }
                }],
                "pageInfo": {"hasNextPage": false, "endCursor": null}
            }
        }))
        .expect("issue response");
        assert_eq!(
            issues.issues.nodes[0].relations.nodes[0].relation_type,
            "blocks"
        );
        assert_eq!(
            issues.issues.nodes[0].relations.nodes[0].issue.id,
            "blocker-uuid"
        );
        assert_eq!(
            issues.issues.nodes[0]
                .assignee
                .as_ref()
                .map(|assignee| assignee.id.as_str()),
            Some("worker-uuid")
        );
        assert!(
            collect_blocked_by(&issues.issues.nodes)
                .get("issue-uuid")
                .is_some_and(|ids| ids.contains("blocker-uuid"))
        );
    }

    #[test]
    fn status_resolution_uses_semantic_type_and_team_scope() {
        let statuses = [
            RawProjectStatusOption {
                id: "started-id".to_owned(),
                name: "In Progress".to_owned(),
                status_type: LinearProjectStatusType::Started,
                team: None,
            },
            RawProjectStatusOption {
                id: "paused-id".to_owned(),
                name: "Paused".to_owned(),
                status_type: LinearProjectStatusType::Planned,
                team: None,
            },
            RawProjectStatusOption {
                id: "team-started-id".to_owned(),
                name: "Team Started".to_owned(),
                status_type: LinearProjectStatusType::Started,
                team: Some(super::RawStatusTeam {
                    id: "team-id".to_owned(),
                }),
            },
        ];

        assert_eq!(
            resolve_project_status_id(&statuses, ProjectRunStatus::Started, None)
                .expect("global started status"),
            "started-id"
        );
        assert_eq!(
            resolve_project_status_id(&statuses, ProjectRunStatus::Paused, None)
                .expect("global paused status"),
            "paused-id"
        );
        assert_eq!(
            resolve_project_status_id(&statuses, ProjectRunStatus::Started, Some("team-id"))
                .expect("team started status"),
            "team-started-id"
        );
    }

    #[test]
    fn status_resolution_refuses_missing_and_ambiguous_matches() {
        let status = RawProjectStatusOption {
            id: "started-id".to_owned(),
            name: "Started".to_owned(),
            status_type: LinearProjectStatusType::Started,
            team: None,
        };
        assert!(matches!(
            resolve_project_status_id(&[], ProjectRunStatus::Started, None),
            Err(crate::Error::ProjectStatusUnavailable { .. })
        ));
        assert!(matches!(
            resolve_project_status_id(&[status.clone(), status], ProjectRunStatus::Started, None),
            Err(crate::Error::ProjectStatusAmbiguous { .. })
        ));
    }

    #[test]
    fn project_statuses_canonicalize_started_by_type_and_preserve_paused_by_name() {
        assert_eq!(
            canonical_project_status("In Progress".to_owned(), LinearProjectStatusType::Started),
            "Started"
        );
        assert_eq!(
            canonical_project_status("Paused".to_owned(), LinearProjectStatusType::Planned),
            "Paused"
        );
        assert_eq!(
            canonical_project_status("Completed".to_owned(), LinearProjectStatusType::Completed),
            "Completed"
        );
    }

    #[test]
    fn project_update_input_serializes_only_the_requested_field() {
        let status = serde_json::to_value(ProjectUpdateInput {
            status_id: Some("status-id".to_owned()),
            priority: None,
            sort_order: None,
        })
        .expect("serialize status update");
        assert_eq!(status, serde_json::json!({"statusId": "status-id"}));

        let priority = serde_json::to_value(ProjectUpdateInput {
            status_id: None,
            priority: Some(ProjectPriority::Urgent.linear_value()),
            sort_order: None,
        })
        .expect("serialize priority update");
        assert_eq!(priority, serde_json::json!({"priority": 1}));

        let order = serde_json::to_value(ProjectUpdateInput {
            status_id: None,
            priority: None,
            sort_order: Some(3.5),
        })
        .expect("serialize order update");
        assert_eq!(order, serde_json::json!({"sortOrder": 3.5}));
    }

    /// Trace: FR-015-AC-1, FR-015-AC-2
    #[test]
    fn protected_credential_preserves_authentication_and_cache_identity() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/graphql", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (_, headers) = read_request(&mut stream);
            let body =
                br#"{"data":{"projectUpdate":{"success":true,"project":{"id":"project-id"}}}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(body).unwrap();
            headers
        });
        let source = LinearGraphQlSource::with_endpoint("fictional-test-token", endpoint).unwrap();
        let debug = format!("{:?}", source.api_key);
        assert_eq!(debug, "SecretValue([REDACTED])");
        assert!(!debug.contains("fictional-test-token"));
        assert_eq!(
            source.cache_namespace,
            format!(
                "linear:{CACHE_MODEL_VERSION}:sha256:{:x}",
                Sha256::digest(b"fictional-test-token")
            )
        );
        source
            .request::<Value, _>(PROJECT_UPDATE_MUTATION, serde_json::json!({}))
            .unwrap();
        let headers = server.join().unwrap();
        assert!(
            headers
                .lines()
                .any(|line| line.eq_ignore_ascii_case("authorization: fictional-test-token"))
        );
    }

    /// Trace: FR-015-AC-3
    #[test]
    fn empty_credential_inputs_remain_missing() {
        for value in ["", "  ", "\t\n"] {
            assert!(matches!(
                LinearGraphQlSource::new(value),
                Err(Error::MissingApiKey)
            ));
        }
    }
}
