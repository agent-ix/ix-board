// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The version 3 named selection: a team with canonical include and exclude members.
//!
//! The document stores selector identities only. Membership is recomputed from a
//! snapshot on every evaluation (see [`crate::evaluate_named_batch`]).

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Error, MemberOutcome, TeamKey, WorkspaceSnapshot, scope};

/// The only schema version the named-selection decoder reads.
pub const NAMED_SELECTION_SCHEMA_VERSION: u32 = 3;

/// One canonical selector. `kind` is one of `initiative`, `project`, `milestone` or
/// `issue` on the wire; a milestone also carries the project that owns it.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NamedMember {
    /// A Linear initiative UUID.
    Initiative {
        /// Initiative UUID.
        id: String,
    },
    /// A Linear project UUID.
    Project {
        /// Project UUID.
        id: String,
    },
    /// A milestone UUID with the UUID of the project that owns it.
    Milestone {
        /// Milestone UUID.
        id: String,
        /// Owning project UUID.
        project_id: String,
    },
    /// A Linear issue UUID; its descendants are selected with it.
    Issue {
        /// Issue UUID.
        id: String,
    },
}

impl NamedMember {
    /// The wire spelling of the member's kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Initiative { .. } => "initiative",
            Self::Project { .. } => "project",
            Self::Milestone { .. } => "milestone",
            Self::Issue { .. } => "issue",
        }
    }

    /// The member's Linear UUID.
    pub fn id(&self) -> &str {
        match self {
            Self::Initiative { id }
            | Self::Project { id }
            | Self::Milestone { id, .. }
            | Self::Issue { id } => id,
        }
    }

    fn is_blank(&self) -> bool {
        let project_blank = matches!(self, Self::Milestone { project_id, .. } if project_id.trim().is_empty());
        self.id().trim().is_empty() || project_blank
    }
}

/// A team's named selection: what to include and what to exclude.
///
/// An empty `include` list is the explicit empty selection; it never means the whole
/// team. Read stored documents with [`decode_named_selection`], which reports an
/// unsupported version separately from an invalid document.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedSelection {
    /// Always [`NAMED_SELECTION_SCHEMA_VERSION`].
    #[schemars(range(min = 3, max = 3))]
    pub schema_version: u32,
    /// The Linear team key that fences the selection.
    pub team: TeamKey,
    /// Members whose closures are selected. Empty selects nothing.
    pub include: Vec<NamedMember>,
    /// Members whose closures are removed from this selection only.
    pub exclude: Vec<NamedMember>,
}

impl NamedSelection {
    /// Builds a canonical selection: each list sorted by kind, then ID (a milestone's
    /// project breaks ties) and without duplicates.
    pub fn new(team: TeamKey, include: Vec<NamedMember>, exclude: Vec<NamedMember>) -> Self {
        Self {
            schema_version: NAMED_SELECTION_SCHEMA_VERSION,
            team,
            include: canonical_list(include),
            exclude: canonical_list(exclude),
        }
    }

    /// This selection in canonical order, with the supported version.
    pub fn canonical(&self) -> Self {
        Self::new(self.team.clone(), self.include.clone(), self.exclude.clone())
    }

    /// Encodes the canonical JSON document: the same selection always gives the same
    /// bytes, whatever order its members were built in.
    pub fn encode(&self) -> Result<String, Error> {
        serde_json::to_string(&self.canonical()).map_err(|error| Error::InvalidNamedSelection {
            message: error.to_string(),
        })
    }
}

fn canonical_list(members: Vec<NamedMember>) -> Vec<NamedMember> {
    members
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidNamedSelection {
        message: message.into(),
    }
}

/// Decodes a stored named selection.
///
/// The version is checked before anything else: any version other than 3, including a
/// version 2 `FocusRecord`, is [`Error::UnsupportedSchemaVersion`], and nothing is
/// converted or filled in. A malformed version 3 document (unknown or missing member,
/// milestone without `project_id`, `project_id` on another kind, unknown kind, blank
/// identifier or duplicate member) is [`Error::InvalidNamedSelection`]. The result is in
/// canonical order.
pub fn decode_named_selection(bytes: &[u8]) -> Result<NamedSelection, Error> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
    let version = value
        .as_object()
        .ok_or_else(|| invalid("a named selection must be a JSON object"))?
        .get("schema_version")
        .ok_or_else(|| invalid("missing schema_version"))?;
    let found = version
        .as_i64()
        .or_else(|| {
            version
                .as_u64()
                .map(|found| i64::try_from(found).unwrap_or(i64::MAX))
        })
        .ok_or_else(|| invalid("schema_version must be an integer"))?;
    if found != i64::from(NAMED_SELECTION_SCHEMA_VERSION) {
        return Err(Error::UnsupportedSchemaVersion {
            found,
            supported: NAMED_SELECTION_SCHEMA_VERSION,
        });
    }
    let selection: NamedSelection =
        serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
    if selection.team.0.trim().is_empty() {
        return Err(invalid("team must not be blank"));
    }
    for (list, members) in [("include", &selection.include), ("exclude", &selection.exclude)] {
        if members.iter().any(NamedMember::is_blank) {
            return Err(invalid(format!("{list} holds a member with a blank identifier")));
        }
        let distinct = members.iter().collect::<BTreeSet<_>>();
        if distinct.len() != members.len() {
            return Err(invalid(format!("{list} holds a duplicate member")));
        }
    }
    Ok(selection.canonical())
}

/// One member named the way a person would, before canonicalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NamedMemberInput {
    /// An initiative UUID, identifier such as `I-10`, or exact name.
    Initiative(String),
    /// A project UUID, identifier, or exact name.
    Project(String),
    /// A milestone UUID, unique name, or `<project>/<milestone>`; `project`, when
    /// given, must name the project that owns it.
    Milestone {
        /// The milestone selector, with the existing slash-safe grammar.
        selector: String,
        /// The project the caller believes owns it.
        project: Option<String>,
    },
    /// A ticket UUID or identifier.
    Ticket(String),
}

/// A named selection before its members are resolved to canonical IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedSelectionInput {
    /// The Linear team key.
    pub team: String,
    /// Members to include.
    pub include: Vec<NamedMemberInput>,
    /// Members to exclude.
    pub exclude: Vec<NamedMemberInput>,
}

/// Resolves a named selection against one snapshot into its canonical document.
///
/// Uses the existing resolvers, so names, identifiers, UUIDs and slash-containing
/// milestone names behave as they do for `focus status`. Any member that is missing,
/// ambiguous, a ticket of another team, a project or initiative holding none of the
/// team's work, or a milestone owned by another project fails the whole call with no
/// partial document.
pub fn canonicalize_named_selection(
    snapshot: &WorkspaceSnapshot,
    input: &NamedSelectionInput,
) -> Result<NamedSelection, Error> {
    if input.team.trim().is_empty() {
        return Err(Error::InvalidTeam);
    }
    let team = TeamKey(input.team.clone());
    let known = snapshot.issues.iter().any(|issue| issue.team_key == team)
        || snapshot
            .projects
            .iter()
            .any(|project| project.team_keys.contains(&team));
    if !known {
        return Err(Error::ScopeSelection {
            kind: "team",
            identifier: input.team.clone(),
            matches: 0,
        });
    }
    let resolve = |members: &[NamedMemberInput]| {
        members
            .iter()
            .map(|member| canonical_member(snapshot, &team, member))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(NamedSelection::new(
        team.clone(),
        resolve(&input.include)?,
        resolve(&input.exclude)?,
    ))
}

fn holds_team_work(snapshot: &WorkspaceSnapshot, project_id: &str, team: &TeamKey) -> bool {
    snapshot.projects.iter().any(|project| {
        project.id.0 == project_id && project.team_keys.contains(team)
    }) || snapshot.issues.iter().any(|issue| {
        issue.team_key == *team && issue.project_id.as_ref().is_some_and(|id| id.0 == project_id)
    })
}

fn rejected(kind: &'static str, identifier: &str, outcome: MemberOutcome) -> Error {
    Error::NamedMemberRejected {
        kind,
        identifier: identifier.to_owned(),
        outcome,
    }
}

fn canonical_member(
    snapshot: &WorkspaceSnapshot,
    team: &TeamKey,
    member: &NamedMemberInput,
) -> Result<NamedMember, Error> {
    match member {
        NamedMemberInput::Initiative(value) => {
            let id = scope::resolve_initiative_id(snapshot, value)?;
            let holds = snapshot
                .projects
                .iter()
                .filter(|project| project.initiatives.iter().any(|i| i.id == id))
                .any(|project| holds_team_work(snapshot, &project.id.0, team));
            if !holds {
                return Err(rejected("initiative", value, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Initiative { id })
        }
        NamedMemberInput::Project(value) => {
            let id = scope::resolve_project_id(snapshot, value)?.0;
            if !holds_team_work(snapshot, &id, team) {
                return Err(rejected("project", value, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Project { id })
        }
        NamedMemberInput::Ticket(value) => {
            let id = scope::resolve_ticket_id(snapshot, value)?;
            let in_team = snapshot
                .issues
                .iter()
                .any(|issue| issue.id == id && issue.team_key == *team);
            if !in_team {
                return Err(rejected("issue", value, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Issue { id: id.0 })
        }
        NamedMemberInput::Milestone { selector, project } => {
            let id = scope::resolve_milestone_id(snapshot, selector)?;
            let owners = snapshot
                .projects
                .iter()
                .filter(|project| project.milestones.iter().any(|m| m.id == id))
                .map(|project| project.id.0.clone())
                .collect::<Vec<_>>();
            let [owner] = owners.as_slice() else {
                return Err(Error::ScopeSelection {
                    kind: "milestone",
                    identifier: selector.clone(),
                    matches: owners.len(),
                });
            };
            if let Some(claimed) = project
                && scope::resolve_project_id(snapshot, claimed)?.0 != *owner
            {
                return Err(rejected("milestone", selector, MemberOutcome::ProjectMismatch));
            }
            if !holds_team_work(snapshot, owner, team) {
                return Err(rejected("milestone", selector, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Milestone {
                id,
                project_id: owner.clone(),
            })
        }
    }
}
