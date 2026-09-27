// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Linear GraphQL transport and strict DTO conversion for the selected fields.

use crate::{
    Error, Issue, IssueId, Milestone, Project, ProjectId, TeamKey, WorkspaceSnapshot,
    WorkspaceSource,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PAGE_SIZE: i32 = 50;
const RELATION_PAGE_SIZE: i32 = 50;
const PAGE_LIMIT: usize = 1_000;
const ENDPOINT: &str = "https://api.linear.app/graphql";

const PROJECT_QUERY: &str = r#"
query WorkspaceProjects($first: Int!, $after: String) {
  projects(first: $first, after: $after) {
    nodes {
      id identifier name description content priority sortOrder
      status { name }
      teams(first: $first) { nodes { key } pageInfo { hasNextPage } }
      projectMilestones(first: $first) {
        nodes { id name description sortOrder }
        pageInfo { hasNextPage }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}"#;

const ISSUE_QUERY: &str = r#"
query WorkspaceIssues($first: Int!, $relationFirst: Int!, $after: String) {
  issues(first: $first, after: $after) {
    nodes {
      id identifier title description priority sortOrder
      state { name }
      team { key }
      project { id }
      relations(first: $relationFirst) {
        nodes { type issue { id } relatedIssue { id } }
        pageInfo { hasNextPage }
      }
    }
    pageInfo { hasNextPage endCursor }
  }
}"#;

/// Linear GraphQL client that reads only structured project and issue fields.
pub struct LinearGraphQlSource {
    client: Client,
    api_key: String,
    cache_namespace: String,
}

impl LinearGraphQlSource {
    /// Creates a source using the supplied Linear API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(Error::MissingApiKey);
        }
        let cache_namespace = format!("linear:sha256:{:x}", Sha256::digest(api_key.as_bytes()));
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            api_key,
            cache_namespace,
        })
    }

    /// Reads the API key from LINEAR_API_KEY.
    pub fn from_env() -> Result<Self, Error> {
        let api_key = std::env::var("LINEAR_API_KEY").map_err(|_| Error::MissingApiKey)?;
        Self::new(api_key)
    }

    fn query<T: DeserializeOwned>(
        &self,
        query: &'static str,
        after: Option<&str>,
        relation_first: Option<i32>,
    ) -> Result<T, Error> {
        let response = self
            .client
            .post(ENDPOINT)
            .header(reqwest::header::AUTHORIZATION, &self.api_key)
            .json(&GraphQlRequest {
                query,
                variables: Variables {
                    first: PAGE_SIZE,
                    relation_first,
                    after: after.map(str::to_owned),
                },
            })
            .send()?
            .error_for_status()?
            .json::<GraphQlResponse<T>>()?;
        if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
            return Err(Error::GraphQl {
                messages: errors
                    .into_iter()
                    .map(|error| error.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            });
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
            let result: ProjectData = self.query(PROJECT_QUERY, after.as_deref(), None)?;
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
                projects.push(Project {
                    id: ProjectId(raw.id),
                    identifier: raw.identifier,
                    name: raw.name,
                    description: raw.description,
                    content: raw.content,
                    status: raw.status.name,
                    priority: raw.priority,
                    sort_order: raw.sort_order,
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
            let result: IssueData =
                self.query(ISSUE_QUERY, after.as_deref(), Some(RELATION_PAGE_SIZE))?;
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
                let state = raw.state.name;
                Issue {
                    id: IssueId(raw.id.clone()),
                    identifier: raw.identifier,
                    title: raw.title,
                    description: raw.description,
                    state,
                    team_key: TeamKey(raw.team.key),
                    project_id: raw.project.map(|project| ProjectId(project.id)),
                    priority: raw.priority,
                    sort_order: raw.sort_order,
                    blocked_by: blocked_by
                        .get(&raw.id)
                        .into_iter()
                        .flat_map(|ids| ids.iter().cloned())
                        .map(IssueId)
                        .collect(),
                }
            })
            .collect();
        Ok(WorkspaceSnapshot { projects, issues })
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
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: Variables,
}

#[derive(Serialize)]
struct Variables {
    first: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "relationFirst")]
    relation_first: Option<i32>,
    after: Option<String>,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
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
#[serde(rename_all = "camelCase")]
struct RawProject {
    id: String,
    identifier: Option<String>,
    name: String,
    description: String,
    content: Option<String>,
    priority: i32,
    sort_order: f64,
    status: RawProjectStatus,
    teams: Connection<RawTeam>,
    project_milestones: Connection<RawMilestone>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjectStatus {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTeam {
    key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct RawMilestone {
    id: String,
    name: String,
    description: Option<String>,
    sort_order: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueData {
    issues: Connection<RawIssue>,
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
    state: RawState,
    team: RawTeam,
    project: Option<RawIssueProject>,
    relations: Connection<RawRelation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawState {
    name: String,
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
    use super::{IssueData, ProjectData, collect_blocked_by};

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
                    "status": {"name": "Started"},
                    "teams": {
                        "nodes": [{"key": "SWM"}],
                        "pageInfo": {"hasNextPage": false}
                    },
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
        assert_eq!(project.projects.nodes[0].status.name, "Started");
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
                    "state": {"name": "Specced"},
                    "team": {"key": "SWM"},
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
        assert!(
            collect_blocked_by(&issues.issues.nodes)
                .get("issue-uuid")
                .is_some_and(|ids| ids.contains("blocker-uuid"))
        );
    }
}
