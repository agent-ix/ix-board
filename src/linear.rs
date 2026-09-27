// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Linear GraphQL transport for selected reads and project status/priority writes.

use crate::{
    Error, Issue, IssueId, Milestone, Project, ProjectId, ProjectMutationSource, ProjectPriority,
    ProjectRunStatus, TeamKey, WorkspaceSnapshot, WorkspaceSource,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const PAGE_SIZE: i32 = 50;
const RELATION_PAGE_SIZE: i32 = 50;
const PAGE_LIMIT: usize = 1_000;
// Invalidate cached snapshots that stored a display name instead of the semantic started state.
const CACHE_MODEL_VERSION: &str = "v2";
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
    api_key: String,
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
            api_key,
            endpoint: endpoint.into(),
            cache_namespace,
        })
    }

    /// Reads the API key from LINEAR_API_KEY for Linear reads and project updates.
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
        self.request(
            query,
            Variables {
                first: PAGE_SIZE,
                relation_first,
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
            .header(reqwest::header::AUTHORIZATION, &self.api_key)
            .json(&GraphQlRequest { query, variables })
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
                    status: canonical_project_status(raw.status.name, raw.status.status_type),
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
            },
        )
    }
}

impl LinearGraphQlSource {
    fn project_statuses(&self) -> Result<Vec<RawProjectStatusOption>, Error> {
        let mut statuses = Vec::new();
        let mut after = None;
        for page in 0..PAGE_LIMIT {
            let result: ProjectStatusesData =
                self.query(PROJECT_STATUSES_QUERY, after.as_deref(), None)?;
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
    use super::{
        IssueData, LinearGraphQlSource, LinearProjectStatusType, ProjectData, ProjectUpdateInput,
        RawProjectStatusOption, canonical_project_status, collect_blocked_by,
        resolve_project_status_id,
    };
    use crate::{ProjectId, ProjectMutationSource, ProjectPriority, ProjectRunStatus, ReadService};
    use serde_json::Value;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::JoinHandle;

    fn mock_linear(responses: Vec<Value>) -> (String, JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Linear endpoint");
        let endpoint = format!(
            "http://{}/graphql",
            listener.local_addr().expect("local addr")
        );
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept GraphQL request");
                requests.push(read_request(&mut stream));
                let body = serde_json::to_vec(&response).expect("encode GraphQL response");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .expect("write response headers");
                stream.write_all(&body).expect("write response body");
            }
            requests
        });
        (endpoint, server)
    }

    fn read_request(stream: &mut TcpStream) -> Value {
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
        serde_json::from_slice(&bytes[header_end..header_end + content_length])
            .expect("decode GraphQL request")
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

    #[test]
    fn decoded_started_project_snapshot_remains_in_next_queue() {
        let project = serde_json::json!({
            "id": "project-id", "identifier": "SWM-P1", "name": "Project", "description": "Goal",
            "content": null, "priority": 2, "sortOrder": 1.0,
            "status": {"name": "In Progress", "type": "started"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "projectMilestones": connection(serde_json::json!([]))
        });
        let issue = serde_json::json!({
            "id": "issue-id", "identifier": "SWM-3", "title": "Project ticket", "description": null,
            "priority": 2.0, "sortOrder": 1.0, "state": {"name": "Specced"}, "team": {"key": "SWM"},
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
            "ix-board-linear-transport-{}.sqlite",
            std::process::id()
        ));
        let mut service = ReadService::open(source, &cache_path).expect("open read service");
        let queue = service.next("SWM").expect("read next queue");
        server.join().expect("mock server");
        let _ = std::fs::remove_file(cache_path);

        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].identifier, "SWM-3");
        assert_eq!(queue[0].project, "SWM-P1");
    }

    #[test]
    fn decoded_paused_project_snapshot_is_excluded_from_next_queue() {
        let project = serde_json::json!({
            "id": "project-id", "identifier": "SWM-P1", "name": "Project", "description": "Goal",
            "content": null, "priority": 2, "sortOrder": 1.0,
            "status": {"name": "Paused", "type": "planned"},
            "teams": connection(serde_json::json!([{"key": "SWM"}])),
            "projectMilestones": connection(serde_json::json!([]))
        });
        let issue = serde_json::json!({
            "id": "issue-id", "identifier": "SWM-3", "title": "Project ticket", "description": null,
            "priority": 2.0, "sortOrder": 1.0, "state": {"name": "Specced"}, "team": {"key": "SWM"},
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
        })
        .expect("serialize status update");
        assert_eq!(status, serde_json::json!({"statusId": "status-id"}));

        let priority = serde_json::to_value(ProjectUpdateInput {
            status_id: None,
            priority: Some(ProjectPriority::Urgent.linear_value()),
        })
        .expect("serialize priority update");
        assert_eq!(priority, serde_json::json!({"priority": 1}));
    }
}
