// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The version 3 named selection: a team with canonical include and exclude members.
//!
//! The document stores selector identities only. Membership is recomputed from a
//! snapshot on every evaluation (see [`crate::evaluate_named_batch`]).

use std::collections::BTreeSet;
use std::convert::Infallible;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::read_bound::Budget;
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
        let project_blank = matches!(
            self,
            Self::Milestone { project_id, .. } if project_id.trim().is_empty()
        );
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
        Self::new(
            self.team.clone(),
            self.include.clone(),
            self.exclude.clone(),
        )
    }

    /// Encodes the canonical JSON document: the same selection always gives the same
    /// bytes, whatever order its members were built in.
    pub fn encode(&self) -> Result<String, Error> {
        serde_json::to_string(&self.canonical()).map_err(|error| Error::InvalidNamedSelection {
            message: error.to_string(),
        })
    }
}

/// The one canonical ordering: sorted by kind, then ID (a milestone's project breaks ties),
/// without duplicates. `check` runs once for every member before it is ordered, and its
/// error stops the ordering, so a bounded caller supplies its budget and a caller with no
/// bound supplies a check that cannot fail.
fn canonical_list_with<E>(
    members: Vec<NamedMember>,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<Vec<NamedMember>, E> {
    let mut ordered = BTreeSet::new();
    for member in members {
        check()?;
        ordered.insert(member);
    }
    Ok(ordered.into_iter().collect())
}

/// [`canonical_list_with`] with no bound. Its check can never fail, and the error type
/// [`Infallible`] shows that: there is nothing to handle or discard.
fn canonical_list(members: Vec<NamedMember>) -> Vec<NamedMember> {
    let Ok(ordered) = canonical_list_with(members, || Ok::<(), Infallible>(()));
    ordered
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
    for (list, members) in [
        ("include", &selection.include),
        ("exclude", &selection.exclude),
    ] {
        if members.iter().any(NamedMember::is_blank) {
            return Err(invalid(format!(
                "{list} holds a member with a blank identifier"
            )));
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
///
/// This standalone form has no deadline. A caller that must bound the cache lookup, the
/// source read and this resolution together uses
/// [`ReadService::canonicalize_named_selection_bounded`](crate::ReadService::canonicalize_named_selection_bounded).
pub fn canonicalize_named_selection(
    snapshot: &WorkspaceSnapshot,
    input: &NamedSelectionInput,
) -> Result<NamedSelection, Error> {
    canonicalize_within(snapshot, input, Budget::unbounded())
}

/// [`canonicalize_named_selection`] under a budget shared with the read that produced
/// `snapshot`. Every scan, and the final ordering of each member list, checks the same
/// budget and stops with [`Error::ReadTimeout`] once it has expired.
pub(crate) fn canonicalize_within(
    snapshot: &WorkspaceSnapshot,
    input: &NamedSelectionInput,
    budget: Budget<'_>,
) -> Result<NamedSelection, Error> {
    let resolved = resolve_members(snapshot, input, budget)?;
    order_within(resolved, budget)
}

/// Which member list of a [`NamedSelectionInput`] an input occurrence belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NamedList {
    /// [`NamedSelectionInput::include`].
    Include,
    /// [`NamedSelectionInput::exclude`].
    Exclude,
}

/// What one input occurrence resolved to, in the same validation pass that produced the
/// canonical selection.
///
/// The occurrence is identified by `list` and `index`: the position of `input` in
/// [`NamedSelectionInput::include`] or [`NamedSelectionInput::exclude`] as the caller
/// supplied them. Repeated or equal inputs are separate occurrences with separate
/// indexes, and the same `member` can appear for several occurrences and in both lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNamedInput {
    /// The list the occurrence was supplied in.
    pub list: NamedList,
    /// The occurrence's zero-based index within that list, as supplied.
    pub index: usize,
    /// The input exactly as supplied at that list and index.
    pub input: NamedMemberInput,
    /// The canonical member that occurrence resolved to.
    pub member: NamedMember,
}

/// A canonical [`NamedSelection`] together with what every input occurrence resolved to.
///
/// `selection` is sorted and de-duplicated per list and equals what
/// [`canonicalize_named_selection`] returns for the same snapshot and input.
/// `associations` is in input order, not canonical order: every occurrence of
/// `include` in supplied order, then every occurrence of `exclude` in supplied order,
/// each exactly once. Join an occurrence to your own data by `(list, index)`, never by
/// position in `selection`. The wire document carries no association.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNamedSelection {
    /// The canonical selection to store or publish.
    pub selection: NamedSelection,
    /// One entry per input occurrence, in input order.
    pub associations: Vec<ResolvedNamedInput>,
}

impl ResolvedNamedSelection {
    /// The association of the occurrence at `index` of `list`, if the input had one.
    pub fn association(&self, list: NamedList, index: usize) -> Option<&ResolvedNamedInput> {
        self.associations
            .iter()
            .find(|entry| entry.list == list && entry.index == index)
    }
}

/// Like [`canonicalize_within`], and also keeps what every input occurrence resolved to.
/// It runs the same resolution and the same ordering; it only records the per-occurrence
/// members that the canonical ordering would otherwise discard.
pub(crate) fn resolve_within(
    snapshot: &WorkspaceSnapshot,
    input: &NamedSelectionInput,
    budget: Budget<'_>,
) -> Result<ResolvedNamedSelection, Error> {
    let resolved = resolve_members(snapshot, input, budget)?;
    let mut associations = Vec::with_capacity(input.include.len() + input.exclude.len());
    for (list, inputs, members) in [
        (NamedList::Include, &input.include, &resolved.include),
        (NamedList::Exclude, &input.exclude, &resolved.exclude),
    ] {
        for (index, (supplied, member)) in inputs.iter().zip(members).enumerate() {
            budget.check()?;
            associations.push(ResolvedNamedInput {
                list,
                index,
                input: supplied.clone(),
                member: member.clone(),
            });
        }
    }
    Ok(ResolvedNamedSelection {
        selection: order_within(resolved, budget)?,
        associations,
    })
}

/// Every input occurrence resolved, in input order and before any ordering.
struct ResolvedMembers {
    team: TeamKey,
    include: Vec<NamedMember>,
    exclude: Vec<NamedMember>,
}

/// The one resolution pass: validates the team and resolves every occurrence of both
/// lists, in input order. Any failure fails the whole call.
fn resolve_members(
    snapshot: &WorkspaceSnapshot,
    input: &NamedSelectionInput,
    budget: Budget<'_>,
) -> Result<ResolvedMembers, Error> {
    if input.team.trim().is_empty() {
        return Err(Error::InvalidTeam);
    }
    let team = TeamKey(input.team.clone());
    if !team_is_known(snapshot, &team, budget)? {
        return Err(Error::ScopeSelection {
            kind: "team",
            identifier: input.team.clone(),
            matches: 0,
        });
    }
    let resolve = |members: &[NamedMemberInput]| {
        members
            .iter()
            .map(|member| canonical_member(snapshot, &team, member, budget))
            .collect::<Result<Vec<_>, _>>()
    };
    let include = resolve(&input.include)?;
    let exclude = resolve(&input.exclude)?;
    Ok(ResolvedMembers {
        team,
        include,
        exclude,
    })
}

/// The one canonical ordering of resolved members, under `budget`.
fn order_within(resolved: ResolvedMembers, budget: Budget<'_>) -> Result<NamedSelection, Error> {
    Ok(NamedSelection {
        schema_version: NAMED_SELECTION_SCHEMA_VERSION,
        team: resolved.team,
        include: canonical_list_within(resolved.include, budget)?,
        exclude: canonical_list_within(resolved.exclude, budget)?,
    })
}

/// [`canonical_list`] that checks `budget` for every member it orders.
fn canonical_list_within(
    members: Vec<NamedMember>,
    budget: Budget<'_>,
) -> Result<Vec<NamedMember>, Error> {
    canonical_list_with(members, || budget.check())
}

fn team_is_known(
    snapshot: &WorkspaceSnapshot,
    team: &TeamKey,
    budget: Budget<'_>,
) -> Result<bool, Error> {
    for issue in &snapshot.issues {
        budget.check()?;
        if issue.team_key == *team {
            return Ok(true);
        }
    }
    for project in &snapshot.projects {
        budget.check()?;
        if budget.contains(&project.team_keys, team)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn holds_team_work(
    snapshot: &WorkspaceSnapshot,
    project_id: &str,
    team: &TeamKey,
    budget: Budget<'_>,
) -> Result<bool, Error> {
    for project in &snapshot.projects {
        budget.check()?;
        if project.id.0 == project_id && budget.contains(&project.team_keys, team)? {
            return Ok(true);
        }
    }
    for issue in &snapshot.issues {
        budget.check()?;
        if issue.team_key == *team
            && issue
                .project_id
                .as_ref()
                .is_some_and(|id| id.0 == project_id)
        {
            return Ok(true);
        }
    }
    Ok(false)
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
    budget: Budget<'_>,
) -> Result<NamedMember, Error> {
    budget.check()?;
    match member {
        NamedMemberInput::Initiative(value) => {
            let id = scope::resolve_initiative_id_within(snapshot, value, budget)?;
            let mut holds = false;
            for project in &snapshot.projects {
                budget.check()?;
                if budget.any(&project.initiatives, |i| i.id == id)?
                    && holds_team_work(snapshot, &project.id.0, team, budget)?
                {
                    holds = true;
                    break;
                }
            }
            if !holds {
                return Err(rejected("initiative", value, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Initiative { id })
        }
        NamedMemberInput::Project(value) => {
            let id = scope::resolve_project_id_within(snapshot, value, budget)?.0;
            if !holds_team_work(snapshot, &id, team, budget)? {
                return Err(rejected("project", value, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Project { id })
        }
        NamedMemberInput::Ticket(value) => {
            let id = scope::resolve_ticket_id_within(snapshot, value, budget)?;
            let mut in_team = false;
            for issue in &snapshot.issues {
                budget.check()?;
                if issue.id == id && issue.team_key == *team {
                    in_team = true;
                    break;
                }
            }
            if !in_team {
                return Err(rejected("issue", value, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Issue { id: id.0 })
        }
        NamedMemberInput::Milestone { selector, project } => {
            let id = scope::resolve_milestone_id_within(snapshot, selector, None, budget)?;
            let mut owners = Vec::new();
            for candidate in &snapshot.projects {
                budget.check()?;
                if budget.any(&candidate.milestones, |m| m.id == id)? {
                    owners.push(candidate.id.0.clone());
                }
            }
            let [owner] = owners.as_slice() else {
                return Err(Error::ScopeSelection {
                    kind: "milestone",
                    identifier: selector.clone(),
                    matches: owners.len(),
                });
            };
            if let Some(claimed) = project
                && scope::resolve_project_id_within(snapshot, claimed, budget)?.0 != *owner
            {
                return Err(rejected(
                    "milestone",
                    selector,
                    MemberOutcome::ProjectMismatch,
                ));
            }
            if !holds_team_work(snapshot, owner, team, budget)? {
                return Err(rejected("milestone", selector, MemberOutcome::WrongTeam));
            }
            Ok(NamedMember::Milestone {
                id,
                project_id: owner.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    use ix_trace_rs::trace;

    use super::*;
    use crate::Clock;
    use crate::read_bound::Deadline;

    fn initiative(id: &str) -> NamedMember {
        NamedMember::Initiative { id: id.into() }
    }

    fn project(id: &str) -> NamedMember {
        NamedMember::Project { id: id.into() }
    }

    fn milestone(id: &str, project_id: &str) -> NamedMember {
        NamedMember::Milestone {
            id: id.into(),
            project_id: project_id.into(),
        }
    }

    fn issue(id: &str) -> NamedMember {
        NamedMember::Issue { id: id.into() }
    }

    /// Every kind, out of order, with duplicates and two milestones sharing one ID.
    fn mixed() -> Vec<NamedMember> {
        vec![
            issue("t2"),
            milestone("m2", "p1"),
            project("p2"),
            initiative("i2"),
            milestone("m1", "p2"),
            issue("t1"),
            milestone("m1", "p1"),
            project("p1"),
            initiative("i1"),
            milestone("m1", "p2"),
            issue("t2"),
            initiative("i1"),
            project("p1"),
        ]
    }

    fn canonical_mixed() -> Vec<NamedMember> {
        vec![
            initiative("i1"),
            initiative("i2"),
            project("p1"),
            project("p2"),
            milestone("m1", "p1"),
            milestone("m1", "p2"),
            milestone("m2", "p1"),
            issue("t1"),
            issue("t2"),
        ]
    }

    #[trace("FR-016-AC-6", "FR-016-AC-11")]
    #[test]
    fn the_bounded_and_unbounded_orderings_are_one_rule_for_every_kind() {
        let mut reversed = mixed();
        reversed.reverse();
        let lists = [
            Vec::new(),
            vec![project("p1"), project("p1")],
            vec![milestone("m1", "p2"), milestone("m1", "p1")],
            mixed(),
            reversed,
        ];
        for list in lists {
            let unbounded = canonical_list(list.clone());
            assert_eq!(
                canonical_list_within(list.clone(), Budget::unbounded()).expect("no bound"),
                unbounded
            );
            let built = NamedSelection::new(TeamKey("SWM".into()), list.clone(), list);
            assert_eq!(built.include, unbounded);
            assert_eq!(built.exclude, unbounded);
        }
        assert_eq!(canonical_list(mixed()), canonical_mixed());
        assert!(canonical_list(Vec::new()).is_empty());
    }

    /// A clock that moves one second on every reading, so each budget check costs one.
    struct StepClock {
        base: Instant,
        reads: AtomicU32,
    }

    impl Clock for StepClock {
        fn now_epoch_seconds(&self) -> i64 {
            0
        }

        fn monotonic_now(&self) -> Instant {
            let reads = self.reads.fetch_add(1, Ordering::SeqCst);
            self.base + Duration::from_secs(1) * reads
        }
    }

    #[trace("FR-004-AC-14", "FR-016-AC-11")]
    #[test]
    fn the_shared_ordering_checks_the_budget_for_every_member_it_orders() {
        let clock = StepClock {
            base: Instant::now(),
            reads: AtomicU32::new(0),
        };
        let deadline = Deadline::start(&clock, Duration::from_secs(5));
        assert!(matches!(
            canonical_list_within(mixed(), Budget::within(&deadline)),
            Err(Error::ReadTimeout { .. })
        ));

        let clock = StepClock {
            base: Instant::now(),
            reads: AtomicU32::new(0),
        };
        let deadline = Deadline::start(&clock, Duration::from_secs(1_000));
        assert_eq!(
            canonical_list_within(mixed(), Budget::within(&deadline)).expect("time to spare"),
            canonical_mixed()
        );
    }
}
