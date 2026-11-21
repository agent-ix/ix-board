// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Native project and initiative catalogs without ticket reads (FR-019, FR-020, FR-030).

use crate::linear::{LinearGraphQlSource, UpdatedFilter};
use crate::{Error, Initiative, Project, ProjectId, ProjectStatusCategory};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const PAGE_LIMIT: usize = 1_000;
const PROJECTS: &str = r#"
query SummaryProjects($first:Int!, $after:String, $filter:ProjectFilter, $includeArchived:Boolean!) {
 projects(first:$first, after:$after, filter:$filter, includeArchived:$includeArchived, orderBy:updatedAt) {
  nodes { id name description updatedAt archivedAt trashed progress priority sortOrder targetDate
   status { name type }
   initiatives(first:10, includeArchived:false) { nodes { id name } pageInfo { hasNextPage endCursor } }
  }
  pageInfo { hasNextPage endCursor }
 }
}"#;
const INITIATIVES: &str = r#"
query SummaryInitiatives($first:Int!, $after:String, $filter:InitiativeFilter, $includeArchived:Boolean!) {
 initiatives(first:$first, after:$after, filter:$filter, includeArchived:$includeArchived, orderBy:updatedAt) {
  nodes { id name updatedAt archivedAt trashed status health
   projects(first:10, includeArchived:false, includeSubInitiatives:false) { nodes { id } pageInfo { hasNextPage endCursor } }
  }
  pageInfo { hasNextPage endCursor }
 }
}"#;
const PROJECT_MEMBERSHIPS: &str = r#"
query SummaryProjectMemberships($id:String!, $after:String) {
 project(id:$id) { initiatives(first:50, after:$after, includeArchived:false) {
 nodes { id name } pageInfo { hasNextPage endCursor } } }
}"#;
const INITIATIVE_MEMBERSHIPS: &str = r#"
query SummaryInitiativeMemberships($id:String!, $after:String) {
 initiative(id:$id) { projects(first:50, after:$after, includeArchived:false, includeSubInitiatives:false) {
 nodes { id } pageInfo { hasNextPage endCursor } } }
}"#;

/// Complete summary catalog, or an unpublished batch of changed records.
///
/// List queries exclude hard deletions. Only full reconciliation can remove an
/// unobserved deletion; this type carries no claim of immediate deletion coverage.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SummarySnapshot {
    /// Project records, including tombstones in a change batch.
    pub projects: Vec<ProjectSummary>,
    /// Initiative records, including tombstones in a change batch.
    pub initiatives: Vec<InitiativeSummary>,
}

/// Native project facts. Ticket-dependent fields are deliberately absent.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSummary {
    /// Project identity, status and complete initiative membership.
    pub project: Project,
    /// Linear native estimate-weighted progress in 0..=1, including started work.
    /// This is not a completed-ticket percentage; provenance is `linear_native`.
    pub progress: Option<f64>,
    /// Project update time, never issue last activity.
    pub updated_at: String,
    /// Whether Linear observed this project as archived.
    pub archived: bool,
    /// Whether Linear observed this project in trash.
    pub trashed: bool,
}

/// Native initiative status and direct project membership; no aggregate progress exists.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitiativeSummary {
    /// Stable identity and name.
    pub initiative: Initiative,
    /// Native lifecycle status.
    pub status: String,
    /// Latest reported native health, if any.
    pub health: Option<String>,
    /// Initiative update time.
    pub updated_at: String,
    /// Whether Linear observed this initiative as archived.
    pub archived: bool,
    /// Whether Linear observed this initiative in trash.
    pub trashed: bool,
    /// Complete direct membership; replaces old links when this record changes.
    pub project_ids: Vec<ProjectId>,
}

impl LinearGraphQlSource {
    /// Reads only native summary catalogs, including observed tombstones.
    pub fn fetch_summary(&self) -> Result<SummarySnapshot, Error> {
        self.fetch_summary_filtered(None)
    }

    /// Reads records updated since the successful sync minus a 60-second overlap.
    /// No batch is returned until all root and membership pages succeed.
    pub fn fetch_summary_changes(&self, since_epoch: i64) -> Result<SummarySnapshot, Error> {
        let epoch = since_epoch.checked_sub(60).ok_or(Error::CacheTimestamp)?;
        let timestamp = chrono::DateTime::from_timestamp(epoch, 0)
            .ok_or(Error::CacheTimestamp)?
            .to_rfc3339();
        self.fetch_summary_filtered(Some(&timestamp))
    }

    fn fetch_summary_filtered(&self, since: Option<&str>) -> Result<SummarySnapshot, Error> {
        let mut result = SummarySnapshot::default();
        let mut cursor = Cursor::new("summary projects");
        loop {
            let data: ProjectData =
                self.request(PROJECTS, Variables::new(20, cursor.after(), since))?;
            for mut raw in data.projects.nodes {
                let mut nested = Cursor::new("project initiative memberships");
                while nested.advance(&raw.initiatives.page_info)? {
                    let data: ProjectMembershipData = self.request(
                        PROJECT_MEMBERSHIPS,
                        MembershipVariables {
                            id: &raw.id,
                            after: nested.after(),
                        },
                    )?;
                    let page = data.project.ok_or(Error::MissingWorkspaceData)?.initiatives;
                    raw.initiatives.nodes.extend(page.nodes);
                    raw.initiatives.page_info = page.page_info;
                }
                result.projects.push(raw.into_summary()?);
            }
            if !cursor.advance(&data.projects.page_info)? {
                break;
            }
        }
        result.initiatives = self.fetch_initiatives_filtered(since, None)?;
        Ok(result)
    }

    pub(crate) fn fetch_initiative_changes(
        &self,
        since_epoch: i64,
        deadline: Option<&crate::Deadline<'_>>,
    ) -> Result<Vec<InitiativeSummary>, Error> {
        let timestamp = chrono::DateTime::from_timestamp(since_epoch.saturating_sub(60), 0)
            .ok_or(Error::CacheTimestamp)?
            .to_rfc3339();
        self.fetch_initiatives_filtered(Some(&timestamp), deadline)
    }

    fn fetch_initiatives_filtered(
        &self,
        since: Option<&str>,
        deadline: Option<&crate::Deadline<'_>>,
    ) -> Result<Vec<InitiativeSummary>, Error> {
        let mut initiatives = Vec::new();
        let mut cursor = Cursor::new("summary initiatives");
        loop {
            let data: InitiativeData = self.request_within(
                deadline,
                INITIATIVES,
                Variables::new(20, cursor.after(), since),
            )?;
            for mut raw in data.initiatives.nodes {
                let mut nested = Cursor::new("initiative project memberships");
                while nested.advance(&raw.projects.page_info)? {
                    let data: InitiativeMembershipData = self.request_within(
                        deadline,
                        INITIATIVE_MEMBERSHIPS,
                        MembershipVariables {
                            id: &raw.id,
                            after: nested.after(),
                        },
                    )?;
                    let page = data.initiative.ok_or(Error::MissingWorkspaceData)?.projects;
                    raw.projects.nodes.extend(page.nodes);
                    raw.projects.page_info = page.page_info;
                }
                initiatives.push(raw.into_summary());
            }
            if !cursor.advance(&data.initiatives.page_info)? {
                break;
            }
        }
        Ok(initiatives)
    }
}

/// Merges stable IDs idempotently and replaces observed memberships and tombstones.
/// Pass an empty baseline for authoritative full reconciliation.
pub fn merge_summary(baseline: &SummarySnapshot, changes: SummarySnapshot) -> SummarySnapshot {
    let mut projects: BTreeMap<_, _> = baseline
        .projects
        .iter()
        .cloned()
        .map(|p| (p.project.id.clone(), p))
        .collect();
    let mut initiatives: BTreeMap<_, _> = baseline
        .initiatives
        .iter()
        .cloned()
        .map(|i| (i.initiative.id.clone(), i))
        .collect();
    for project in changes.projects {
        if project.archived || project.trashed {
            projects.remove(&project.project.id);
        } else {
            projects.insert(project.project.id.clone(), project);
        }
    }
    for initiative in changes.initiatives {
        let id = initiative.initiative.id.clone();
        for project in projects.values_mut() {
            project.project.initiatives.retain(|member| member.id != id);
            if !initiative.archived
                && !initiative.trashed
                && initiative.project_ids.contains(&project.project.id)
            {
                project
                    .project
                    .initiatives
                    .push(initiative.initiative.clone());
            }
        }
        if initiative.archived || initiative.trashed {
            initiatives.remove(&id);
        } else {
            initiatives.insert(id, initiative);
        }
    }
    for project in projects.values_mut() {
        project
            .project
            .initiatives
            .retain(|i| initiatives.contains_key(&i.id));
        project.project.initiatives.sort_by(|a, b| a.id.cmp(&b.id));
        project.project.initiatives.dedup_by(|a, b| a.id == b.id);
    }
    for initiative in initiatives.values_mut() {
        initiative.project_ids = projects
            .values()
            .filter(|project| {
                project
                    .project
                    .initiatives
                    .iter()
                    .any(|member| member.id == initiative.initiative.id)
            })
            .map(|project| project.project.id.clone())
            .collect();
    }
    SummarySnapshot {
        projects: projects.into_values().collect(),
        initiatives: initiatives.into_values().collect(),
    }
}

#[derive(Serialize)]
struct Variables<'a> {
    first: i32,
    after: Option<&'a str>,
    filter: Option<UpdatedFilter<'a>>,
    #[serde(rename = "includeArchived")]
    include_archived: bool,
}
impl<'a> Variables<'a> {
    fn new(first: i32, after: Option<&'a str>, since: Option<&'a str>) -> Self {
        Self {
            first,
            after,
            include_archived: since.is_some(),
            filter: since.map(UpdatedFilter::new),
        }
    }
}
#[derive(Serialize)]
struct MembershipVariables<'a> {
    id: &'a str,
    after: Option<&'a str>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection<T> {
    nodes: Vec<T>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}
struct Cursor {
    resource: &'static str,
    seen: BTreeSet<String>,
    cursor: Option<String>,
}
impl Cursor {
    fn new(resource: &'static str) -> Self {
        Self {
            resource,
            seen: BTreeSet::new(),
            cursor: None,
        }
    }
    fn after(&self) -> Option<&str> {
        self.cursor.as_deref()
    }
    fn advance(&mut self, page: &PageInfo) -> Result<bool, Error> {
        if !page.has_next_page {
            return Ok(false);
        }
        let cursor = page.end_cursor.as_ref().ok_or(Error::MissingCursor)?;
        if cursor.is_empty() || !self.seen.insert(cursor.clone()) {
            return Err(Error::InvalidCursor {
                resource: self.resource.to_owned(),
            });
        }
        if self.seen.len() >= PAGE_LIMIT {
            return Err(Error::PaginationLimit { pages: PAGE_LIMIT });
        }
        self.cursor = Some(cursor.clone());
        Ok(true)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectData {
    projects: Connection<RawProject>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitiativeData {
    initiatives: Connection<RawInitiative>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectMembershipData {
    project: Option<ProjectMembership>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectMembership {
    initiatives: Connection<Identity>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitiativeMembershipData {
    initiative: Option<InitiativeMembership>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitiativeMembership {
    projects: Connection<Id>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    id: String,
    name: String,
}
impl Identity {
    fn into_initiative(self) -> Initiative {
        Initiative {
            identifier: String::new(),
            id: self.id,
            name: self.name,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    name: String,
    #[serde(rename = "type")]
    category: ProjectStatusCategory,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawProject {
    id: String,
    name: String,
    description: String,
    updated_at: String,
    archived_at: Option<String>,
    trashed: Option<bool>,
    progress: f64,
    priority: i32,
    sort_order: f64,
    target_date: Option<String>,
    status: Status,
    initiatives: Connection<Identity>,
}
impl RawProject {
    fn into_summary(self) -> Result<ProjectSummary, Error> {
        if !self.progress.is_finite() || !(0.0..=1.0).contains(&self.progress) {
            return Err(Error::GraphQl {
                messages: "native project progress outside 0..1".to_owned(),
            });
        }
        let status = if self.status.category == ProjectStatusCategory::Started {
            "Started".to_owned()
        } else {
            self.status.name.clone()
        };
        Ok(ProjectSummary {
            progress: Some(self.progress),
            updated_at: self.updated_at.clone(),
            archived: self.archived_at.is_some(),
            trashed: self.trashed.unwrap_or(false),
            project: Project {
                id: ProjectId(self.id),
                identifier: None,
                name: self.name,
                updated_at: Some(self.updated_at),
                description: self.description,
                content: None,
                status,
                status_name: Some(self.status.name),
                status_category: self.status.category,
                priority: self.priority,
                sort_order: self.sort_order,
                target_date: self.target_date,
                team_keys: Vec::new(),
                milestones: Vec::new(),
                initiatives: self
                    .initiatives
                    .nodes
                    .into_iter()
                    .map(Identity::into_initiative)
                    .collect(),
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawInitiative {
    id: String,
    name: String,
    updated_at: String,
    archived_at: Option<String>,
    trashed: Option<bool>,
    status: String,
    health: Option<String>,
    projects: Connection<Id>,
}
impl RawInitiative {
    fn into_summary(self) -> InitiativeSummary {
        InitiativeSummary {
            initiative: Initiative {
                identifier: String::new(),
                id: self.id,
                name: self.name,
            },
            status: self.status,
            health: self.health,
            updated_at: self.updated_at,
            archived: self.archived_at.is_some(),
            trashed: self.trashed.unwrap_or(false),
            project_ids: self
                .projects
                .nodes
                .into_iter()
                .map(|p| ProjectId(p.id))
                .collect(),
        }
    }
}
