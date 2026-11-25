// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Evaluation of named selections against one workspace snapshot.
//!
//! Membership, leaf classification, readiness and prerequisite closure are the
//! existing FR-010, FR-001 and FR-002 implementations; this module only combines them
//! per selection and per team and reports what is known and what is not.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::read_bound::Budget;
use crate::reference_warnings::focus_warnings;
use crate::scope::{LeafClassification, classify_leaves_within};
use crate::{
    Error, FocusBlocker, Issue, IssueId, NamedMember, NamedSelection, Project, ProjectId,
    ProjectStatusCategory, ReferenceWarning, TeamKey, WorkflowStateType, WorkspaceSnapshot,
};

/// What the snapshot's source enumerates authoritatively.
///
/// A record absent from a snapshot is `Missing` only when the matching flag is set, because
/// a complete snapshot of the records it fetched is not a catalog of every record
/// (archived and hidden ones may exist but not be enumerated). Without evidence the
/// outcome is `Unavailable`. The default claims nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CatalogEvidence {
    /// Every initiative, including archived and hidden ones, was enumerated.
    pub initiatives: bool,
    /// Every project, including archived and hidden ones, was enumerated.
    pub projects: bool,
    /// Every project milestone, including archived and hidden ones, was enumerated.
    pub milestones: bool,
    /// Every ticket, including archived and hidden ones, was enumerated.
    pub tickets: bool,
    /// Each project's initiative links are complete in the snapshot.
    pub initiative_links: bool,
}

/// The typed result for one include or exclude member.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MemberOutcome {
    /// The member exists and applies to the selection.
    Resolved,
    /// Authoritative enumeration evidence shows the record does not exist.
    Missing,
    /// A ticket of another team.
    WrongTeam,
    /// A milestone whose recorded project does not own it.
    ProjectMismatch,
    /// The snapshot cannot prove the record exists or does not.
    Unavailable,
}

/// Which list of the selection a member came from.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    /// The selection's include list.
    Include,
    /// The selection's exclude list.
    Exclude,
}

/// The outcome for one member, naming its list, kind and identifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberReport {
    /// The list the member was in.
    pub list: ListKind,
    /// The member, with its kind and canonical IDs.
    pub member: NamedMember,
    /// What the snapshot established about it.
    pub outcome: MemberOutcome,
}

/// How much of a selection or team result is known.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectionState {
    /// The include list is empty: an explicit selection of nothing.
    Empty,
    /// Every member resolved.
    Complete,
    /// Some member did not resolve; counts cover only the resolved members.
    Incomplete,
    /// Nothing could be established: an unknown team, or no include member available.
    Unavailable,
}

/// Whether a request carried any selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatchState {
    /// The request had zero selections: no active selection, and no totals.
    None,
    /// The request had one or more selections.
    Evaluated,
}

/// Whether counts are complete, partial, or not available.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CountAvailability {
    /// The counts cover everything the selection names.
    Complete,
    /// Counts cover only resolved members; unresolved ones may add more.
    Partial,
    /// There are no counts; zeros here mean unknown, not none.
    Unavailable,
}

/// Leaf counts, from the FR-010 leaf classification and FR-001/FR-002 results.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LeafCounts {
    /// Countable leaf tasks.
    pub total: usize,
    /// Completed leaf tasks.
    pub done: usize,
    /// Remaining leaf tasks by workflow stage.
    pub remaining_by_stage: BTreeMap<String, usize>,
    /// Leaves FR-001 would place in its ready queue.
    pub ready: usize,
    /// Open leaves with a prerequisite, anywhere in the snapshot, that is not Completed.
    pub blocked: usize,
}

/// Why a row covers only part of its Linear object.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PartialReason {
    /// Reached only through milestone members.
    MilestoneOnly,
    /// Reached only through ticket members.
    TicketOnly,
    /// An exclusion removed part of it.
    Exclusion,
    /// The team boundary left other-team leaves out.
    TeamBoundary,
}

/// A fact the snapshot could not establish for a row.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum UnknownFact {
    /// The project's native status category is `Unknown`.
    ProjectStatusCategory,
    /// The project's initiative links cannot be established from the snapshot.
    InitiativeLinks,
    /// The selection reaches a blocker, parent or project without a record, or selects by
    /// ticket in a team where a parent is missing, so its membership, readiness or
    /// blocking is not fully known.
    UnavailableReferences,
}

/// Initiative-link status of a standalone project row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InitiativeLink {
    /// Known: the selection includes no initiative that holds the project.
    NoneSelected,
    /// The initiative links cannot be established from the snapshot.
    Unavailable,
}

/// A project shown under an initiative or on its own.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectRow {
    /// Project UUID.
    pub project_id: ProjectId,
    /// Project identifier, falling back to its UUID.
    pub identifier: String,
    /// Project name.
    pub name: String,
    /// Linear's native status category, kept as `Unknown` when not established.
    pub status_category: ProjectStatusCategory,
    /// Counts restricted to this selection's leaves in the project.
    pub counts: LeafCounts,
    /// Why the row covers only part of the project; empty when it covers all of it.
    pub partial: Vec<PartialReason>,
    /// True when the project appears in more than one placement of the batch.
    pub shared: bool,
    /// Set on standalone rows only.
    pub initiative_link: Option<InitiativeLink>,
    /// Facts that keep the row from being reported healthy or complete.
    pub unknown_facts: Vec<UnknownFact>,
}

/// An included initiative with the projects displayed under it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitiativeRow {
    /// Initiative UUID.
    pub initiative_id: String,
    /// Initiative identifier such as `I-10`.
    pub identifier: String,
    /// Initiative name.
    pub name: String,
    /// Projects it holds, less excluded ones, restricted to this selection.
    pub projects: Vec<ProjectRow>,
    /// Counts over this selection's leaves in those projects, never a sum of the rows.
    pub counts: LeafCounts,
    /// Why the row covers only part of the initiative.
    pub partial: Vec<PartialReason>,
    /// True when the initiative appears in more than one selection of the batch.
    pub shared: bool,
}

/// A ticket member whose project is not itself included.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TicketRow {
    /// Ticket UUID.
    pub issue_id: IssueId,
    /// Ticket identifier.
    pub identifier: String,
    /// Owning project, when it has one.
    pub project_id: Option<ProjectId>,
    /// Counts over the ticket's selected leaves.
    pub counts: LeafCounts,
    /// True when the ticket appears in more than one selection of the batch.
    pub shared: bool,
}

/// The result for one selection, carrying the caller's key unchanged.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionSummary {
    /// The caller's opaque key.
    pub key: String,
    /// The selection's team.
    pub team: TeamKey,
    /// How much of the result is known.
    pub state: SelectionState,
    /// False when the team appears in no project or issue of the snapshot.
    pub team_known: bool,
    /// Every member's outcome, in deterministic order.
    pub members: Vec<MemberReport>,
    /// The deduplicated leaf set, sorted by UUID.
    pub leaf_ids: Vec<IssueId>,
    /// Counts over the leaf set.
    pub counts: LeafCounts,
    /// Whether `counts` is complete, partial, or not available.
    pub counts_availability: CountAvailability,
    /// Included initiatives with their projects.
    pub initiatives: Vec<InitiativeRow>,
    /// Standalone project rows.
    pub projects: Vec<ProjectRow>,
    /// Direct-ticket rows.
    pub tickets: Vec<TicketRow>,
    /// The complete prerequisite closure of the selection's issues in FR-002 layer order.
    /// A prerequisite the selection excludes stays here as evidence and is never counted.
    pub blocker_layers: Vec<Vec<FocusBlocker>>,
    /// Unknown facts anywhere in this selection's rows.
    pub unknown_facts: Vec<UnknownFact>,
}

/// Why a selection keeps its team's union from being complete or fully known.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnionCause {
    /// The selection's caller key.
    pub key: String,
    /// The selection's own state.
    pub state: SelectionState,
    /// True when the selection's team is unknown to the snapshot.
    pub team_unavailable: bool,
    /// The members that did not resolve.
    pub members: Vec<MemberReport>,
    /// Unknown facts in the selection's rows.
    pub unknown_facts: Vec<UnknownFact>,
}

/// The combined result for one team: its selections' leaves counted once each.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamSummary {
    /// The team.
    pub team: TeamKey,
    /// Union state: complete only when every non-empty selection is complete.
    pub state: SelectionState,
    /// Keys of the team's selections, in order.
    pub selection_keys: Vec<String>,
    /// The union of the usable selections' leaves, sorted by UUID.
    pub leaf_ids: Vec<IssueId>,
    /// Counts over the union, never a sum of the selections' counts.
    pub counts: LeafCounts,
    /// Whether `counts` is complete, partial, or not available.
    pub counts_availability: CountAvailability,
    /// Selections that keep the union from being complete or fully known.
    pub causes: Vec<UnionCause>,
}

/// One snapshot's results for a batch of selections.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedBatch {
    /// The snapshot's actual fetch time, Unix seconds.
    pub fetched_at: i64,
    /// `none` when the request had no selection; there are then no totals at all.
    pub state: BatchState,
    /// One result per selection, ordered by key.
    pub selections: Vec<SelectionSummary>,
    /// One combined result per team, ordered by team. Teams are never combined.
    pub teams: Vec<TeamSummary>,
    /// Unavailable references of the selections' issues; omitted when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ReferenceWarning>,
}

/// One selection with the caller's opaque key (for example a focus ID and revision).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedFocusRequest {
    /// Returned unchanged with the selection's result; must be unique in a batch.
    pub key: String,
    /// The selection to evaluate.
    pub selection: NamedSelection,
}

/// Evaluates every selection and every team union against one snapshot.
///
/// Membership is recomputed from the snapshot and the stored identities. A parent cycle or
/// dependency cycle fails the whole batch with that error and no partial result. An
/// unavailable parent, blocker, project or milestone record is reported in
/// [`NamedBatch::warnings`] and the batch is still evaluated; a selection that depends on
/// such a parent, blocker or project is incomplete with
/// [`UnknownFact::UnavailableReferences`] (FR-046).
pub fn evaluate_named_batch(
    snapshot: &WorkspaceSnapshot,
    evidence: &CatalogEvidence,
    requests: &[NamedFocusRequest],
) -> Result<NamedBatch, Error> {
    evaluate_batch(snapshot, evidence, requests, Budget::unbounded())
}

/// The one evaluation engine. A bounded read passes the deadline it already started, so
/// validation and every traversal below stop with [`Error::ReadTimeout`] once it has
/// expired; the public function above passes no bound and behaves as it always did.
pub(crate) fn evaluate_batch(
    snapshot: &WorkspaceSnapshot,
    evidence: &CatalogEvidence,
    requests: &[NamedFocusRequest],
    budget: Budget<'_>,
) -> Result<NamedBatch, Error> {
    budget.check()?;
    let mut keys = BTreeSet::new();
    for request in requests {
        if !keys.insert(&request.key) {
            return Err(Error::DuplicateSelectionKey {
                key: request.key.clone(),
            });
        }
    }
    if requests.is_empty() {
        return Ok(NamedBatch {
            fetched_at: snapshot.fetched_at,
            state: BatchState::None,
            selections: Vec::new(),
            teams: Vec::new(),
            warnings: Vec::new(),
        });
    }
    let context = Context::new(snapshot, evidence, budget)?;
    let mut ordered = requests.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.key.cmp(&right.key));
    let drafts = ordered
        .into_iter()
        .map(|request| context.draft(request))
        .collect::<Result<Vec<_>, _>>()?;
    let all_leaves = drafts
        .iter()
        .flat_map(|draft| draft.leaves.leaf_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let readiness = context.readiness(&all_leaves)?;
    let mut selections = drafts
        .iter()
        .map(|draft| context.summarize(draft, &readiness))
        .collect::<Result<Vec<_>, _>>()?;
    budget.check()?;
    mark_shared(&mut selections, budget)?;
    let teams = context.team_summaries(&selections, &readiness)?;
    let warnings = drafts
        .iter()
        .flat_map(|draft| draft.warnings.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(NamedBatch {
        fetched_at: snapshot.fetched_at,
        state: BatchState::Evaluated,
        selections,
        teams,
        warnings,
    })
}

/// Issues and projects one resolved member selects, before the team fence.
#[derive(Default)]
struct Closure {
    projects: BTreeSet<ProjectId>,
    issues: BTreeSet<IssueId>,
}

/// One list's evaluation: a report per member, and the closure of each resolved member.
struct ListEvaluation {
    reports: Vec<MemberReport>,
    resolved: Vec<(NamedMember, Closure)>,
}

/// One selection's membership, computed but not yet counted.
struct Draft<'a> {
    request: &'a NamedFocusRequest,
    state: SelectionState,
    team_known: bool,
    /// An initiative member resolved without established links, so hidden links may add
    /// leaves to it or to what its exclusion removes.
    links_uncertain: bool,
    /// A reference this selection depends on has no record.
    references_uncertain: bool,
    /// Unavailable references of the effective issues.
    warnings: Vec<ReferenceWarning>,
    members: Vec<MemberReport>,
    include: Vec<(NamedMember, Closure)>,
    included: BTreeSet<IssueId>,
    excluded: BTreeSet<IssueId>,
    excluded_projects: BTreeSet<ProjectId>,
    effective: BTreeSet<IssueId>,
    leaves: LeafClassification,
    /// Included, non-excluded initiatives with the projects they hold.
    initiatives: Vec<(String, BTreeSet<ProjectId>)>,
    /// Projects the selection covers whole: directly or through an included initiative.
    fully_included: BTreeSet<ProjectId>,
    /// Projects shown under an included initiative.
    displayed: BTreeSet<ProjectId>,
}

struct Readiness {
    ready: BTreeSet<IssueId>,
    blocked: BTreeSet<IssueId>,
}

struct Context<'a> {
    snapshot: &'a WorkspaceSnapshot,
    evidence: &'a CatalogEvidence,
    issues: BTreeMap<IssueId, &'a Issue>,
    projects: BTreeMap<ProjectId, &'a Project>,
    children: BTreeMap<IssueId, Vec<IssueId>>,
    project_issues: BTreeMap<ProjectId, Vec<IssueId>>,
    parents: BTreeSet<IssueId>,
    budget: Budget<'a>,
}

impl<'a> Context<'a> {
    fn new(
        snapshot: &'a WorkspaceSnapshot,
        evidence: &'a CatalogEvidence,
        budget: Budget<'a>,
    ) -> Result<Self, Error> {
        let mut children: BTreeMap<IssueId, Vec<IssueId>> = BTreeMap::new();
        let mut project_issues: BTreeMap<ProjectId, Vec<IssueId>> = BTreeMap::new();
        let mut parents = BTreeSet::new();
        for issue in &snapshot.issues {
            budget.check()?;
            if let Some(parent) = &issue.parent_id {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(issue.id.clone());
                parents.insert(parent.clone());
            }
            if let Some(project) = &issue.project_id {
                project_issues
                    .entry(project.clone())
                    .or_default()
                    .push(issue.id.clone());
            }
        }
        budget.check()?;
        Ok(Self {
            snapshot,
            evidence,
            issues: snapshot.issues.iter().map(|i| (i.id.clone(), i)).collect(),
            projects: snapshot
                .projects
                .iter()
                .map(|p| (p.id.clone(), p))
                .collect(),
            children,
            project_issues,
            parents,
            budget,
        })
    }

    fn team_known(&self, team: &TeamKey) -> bool {
        self.snapshot.issues.iter().any(|i| i.team_key == *team)
            || self
                .snapshot
                .projects
                .iter()
                .any(|p| p.team_keys.contains(team))
    }

    fn holds_team_work(&self, project: &Project, team: &TeamKey) -> bool {
        project.team_keys.contains(team)
            || self
                .project_issues
                .get(&project.id)
                .into_iter()
                .flatten()
                .any(|id| self.issues.get(id).is_some_and(|i| i.team_key == *team))
    }

    fn absent(proven: bool) -> MemberOutcome {
        if proven {
            MemberOutcome::Missing
        } else {
            MemberOutcome::Unavailable
        }
    }

    fn member_closure(
        &self,
        team: &TeamKey,
        member: &NamedMember,
    ) -> Result<(MemberOutcome, Closure), Error> {
        self.budget.check()?;
        let resolved = match member {
            NamedMember::Initiative { id } => self.initiative_closure(id),
            NamedMember::Project { id } => self.project_closure(id),
            NamedMember::Milestone { id, project_id } => self.milestone_closure(id, project_id),
            NamedMember::Issue { id } => self.ticket_closure(team, id)?,
        };
        self.budget.check()?;
        Ok(resolved)
    }

    fn initiative_closure(&self, id: &str) -> (MemberOutcome, Closure) {
        let projects = self
            .snapshot
            .projects
            .iter()
            .filter(|p| p.initiatives.iter().any(|i| i.id == id))
            .map(|p| p.id.clone())
            .collect::<BTreeSet<_>>();
        if projects.is_empty() {
            return (Self::absent(self.evidence.initiatives), Closure::default());
        }
        let issues = self.issues_of(&projects);
        (MemberOutcome::Resolved, Closure { projects, issues })
    }

    fn project_closure(&self, id: &str) -> (MemberOutcome, Closure) {
        let project = ProjectId(id.to_owned());
        if !self.projects.contains_key(&project) {
            return (Self::absent(self.evidence.projects), Closure::default());
        }
        let projects = BTreeSet::from([project]);
        let issues = self.issues_of(&projects);
        (MemberOutcome::Resolved, Closure { projects, issues })
    }

    fn milestone_closure(&self, id: &str, project_id: &str) -> (MemberOutcome, Closure) {
        let mut owners = self
            .snapshot
            .projects
            .iter()
            .filter(|p| p.milestones.iter().any(|m| m.id == id))
            .peekable();
        if owners.peek().is_none() {
            return (Self::absent(self.evidence.milestones), Closure::default());
        }
        if !owners.any(|p| p.id.0 == project_id) {
            return (MemberOutcome::ProjectMismatch, Closure::default());
        }
        let issues = self
            .snapshot
            .issues
            .iter()
            .filter(|i| i.project_milestone_id.as_deref() == Some(id))
            .map(|i| i.id.clone())
            .collect();
        (
            MemberOutcome::Resolved,
            Closure {
                projects: BTreeSet::new(),
                issues,
            },
        )
    }

    fn ticket_closure(&self, team: &TeamKey, id: &str) -> Result<(MemberOutcome, Closure), Error> {
        let root = IssueId(id.to_owned());
        let Some(issue) = self.issues.get(&root) else {
            return Ok((Self::absent(self.evidence.tickets), Closure::default()));
        };
        if issue.team_key != *team {
            return Ok((MemberOutcome::WrongTeam, Closure::default()));
        }
        let mut issues = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(next) = pending.pop() {
            self.budget.check()?;
            if issues.insert(next.clone()) {
                pending.extend(self.children.get(&next).into_iter().flatten().cloned());
            }
        }
        Ok((
            MemberOutcome::Resolved,
            Closure {
                projects: BTreeSet::new(),
                issues,
            },
        ))
    }

    fn issues_of(&self, projects: &BTreeSet<ProjectId>) -> BTreeSet<IssueId> {
        projects
            .iter()
            .flat_map(|p| self.project_issues.get(p).into_iter().flatten())
            .cloned()
            .collect()
    }

    fn evaluate_list(
        &self,
        team: &TeamKey,
        list: ListKind,
        members: &[NamedMember],
    ) -> Result<ListEvaluation, Error> {
        let mut reports = Vec::new();
        let mut resolved = Vec::new();
        for member in members {
            let (outcome, closure) = self.member_closure(team, member)?;
            reports.push(MemberReport {
                list,
                member: member.clone(),
                outcome,
            });
            if outcome == MemberOutcome::Resolved {
                resolved.push((member.clone(), closure));
            }
        }
        Ok(ListEvaluation { reports, resolved })
    }

    fn draft<'r>(&self, request: &'r NamedFocusRequest) -> Result<Draft<'r>, Error> {
        let selection = &request.selection;
        let team = &selection.team;
        self.budget.check()?;
        let ListEvaluation {
            reports: mut members,
            resolved: include,
        } = self.evaluate_list(team, ListKind::Include, &selection.include)?;
        let ListEvaluation {
            reports: excluded_reports,
            resolved: exclude,
        } = self.evaluate_list(team, ListKind::Exclude, &selection.exclude)?;
        members.extend(excluded_reports);
        members.sort_by(|a, b| (a.list, &a.member).cmp(&(b.list, &b.member)));
        let team_known = self.team_known(team);
        let mut state = selection_state(selection, team_known, &members);
        let links_uncertain = state != SelectionState::Empty
            && self.initiative_links_uncertain(team, include.iter().chain(&exclude))?;
        if links_uncertain && state == SelectionState::Complete {
            // The known leaves stay, but membership cannot be reported as complete.
            state = SelectionState::Incomplete;
        }
        let mut draft = Draft {
            request,
            state,
            team_known,
            links_uncertain,
            references_uncertain: false,
            warnings: Vec::new(),
            members,
            included: closure_issues(&include),
            excluded: closure_issues(&exclude),
            excluded_projects: exclude
                .iter()
                .flat_map(|(_, closure)| closure.projects.iter().cloned())
                .collect(),
            include,
            effective: BTreeSet::new(),
            leaves: LeafClassification::default(),
            initiatives: Vec::new(),
            fully_included: BTreeSet::new(),
            displayed: BTreeSet::new(),
        };
        if state == SelectionState::Empty {
            return Ok(draft);
        }
        draft.effective = draft
            .included
            .difference(&draft.excluded)
            .filter(|id| self.issues.get(*id).is_some_and(|i| i.team_key == *team))
            .cloned()
            .collect();
        draft.leaves = classify_leaves_within(self.snapshot, &draft.effective, self.budget)?;
        draft.warnings = focus_warnings(self.snapshot, &draft.effective, self.budget)?;
        draft.references_uncertain =
            state != SelectionState::Unavailable && self.references_uncertain(&draft, &exclude)?;
        if draft.references_uncertain && draft.state == SelectionState::Complete {
            draft.state = SelectionState::Incomplete;
        }
        self.place_projects(&mut draft, &exclude)?;
        Ok(draft)
    }

    /// Whether the selection's membership, readiness or blocking rests on a record the
    /// snapshot does not hold. A milestone reference changes none of them. A ticket
    /// member covers its descendants through parent links, so a missing parent anywhere
    /// among the team's issues may hide some of them.
    fn references_uncertain(
        &self,
        draft: &Draft<'_>,
        exclude: &[(NamedMember, Closure)],
    ) -> Result<bool, Error> {
        if draft
            .warnings
            .iter()
            .any(|warning| !matches!(warning, ReferenceWarning::UnavailableMilestone { .. }))
        {
            return Ok(true);
        }
        let by_ticket = draft
            .include
            .iter()
            .chain(exclude)
            .any(|(member, _)| matches!(member, NamedMember::Issue { .. }));
        if !by_ticket {
            return Ok(false);
        }
        for issue in &self.snapshot.issues {
            self.budget.check()?;
            if issue.team_key == draft.request.selection.team
                && issue
                    .parent_id
                    .as_ref()
                    .is_some_and(|parent| !self.issues.contains_key(parent))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether an initiative include or exclude member may cover more than the snapshot
    /// shows. Without established initiative links, a project holding the team's work
    /// that is not known to belong to the initiative may still belong to it, so the
    /// initiative's leaves (or the leaves its exclusion removes) are not fully known.
    fn initiative_links_uncertain<'r>(
        &self,
        team: &TeamKey,
        resolved: impl IntoIterator<Item = &'r (NamedMember, Closure)>,
    ) -> Result<bool, Error> {
        if self.evidence.initiative_links {
            return Ok(false);
        }
        for (member, closure) in resolved {
            if !matches!(member, NamedMember::Initiative { .. }) {
                continue;
            }
            for project in &self.snapshot.projects {
                self.budget.check()?;
                if !closure.projects.contains(&project.id) && self.holds_team_work(project, team) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Works out which projects the selection covers whole and which sit under an initiative.
    fn place_projects(
        &self,
        draft: &mut Draft<'_>,
        exclude: &[(NamedMember, Closure)],
    ) -> Result<(), Error> {
        let excluded_initiatives = exclude
            .iter()
            .filter(|(member, _)| matches!(member, NamedMember::Initiative { .. }))
            .map(|(member, _)| member.id().to_owned())
            .collect::<BTreeSet<_>>();
        for (member, closure) in &draft.include {
            self.budget.check()?;
            match member {
                NamedMember::Initiative { id } if !excluded_initiatives.contains(id) => {
                    let projects = &closure.projects - &draft.excluded_projects;
                    draft.displayed.extend(projects.iter().cloned());
                    draft.fully_included.extend(projects);
                    draft
                        .initiatives
                        .push((id.clone(), closure.projects.clone()));
                }
                NamedMember::Project { .. } => {
                    let projects = &closure.projects - &draft.excluded_projects;
                    draft.fully_included.extend(projects);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn is_countable_leaf(&self, issue: &Issue) -> bool {
        !self.parents.contains(&issue.id)
            && !matches!(
                issue.state_type,
                WorkflowStateType::Canceled | WorkflowStateType::Duplicate
            )
    }

    /// Readiness and blocking for every leaf, from the existing FR-001 and FR-002 code.
    fn readiness(&self, leaves: &BTreeSet<IssueId>) -> Result<Readiness, Error> {
        let roots = leaves
            .iter()
            .filter_map(|id| self.issues.get(id).copied())
            .collect::<Vec<_>>();
        crate::dependency_layers_within(self.snapshot, &roots, self.budget)?;
        let ready_identifiers = crate::ready_queue_within(
            self.snapshot,
            |issue| leaves.contains(&issue.id),
            self.budget,
        )?
        .into_iter()
        .map(|item| item.identifier)
        .collect::<BTreeSet<_>>();
        let by_id = self
            .snapshot
            .issues
            .iter()
            .map(|issue| (&issue.id, issue))
            .collect::<HashMap<_, _>>();
        let mut ready = BTreeSet::new();
        let mut blocked = BTreeSet::new();
        for issue in roots {
            self.budget.check()?;
            if ready_identifiers.contains(&issue.identifier) {
                ready.insert(issue.id.clone());
            }
            if crate::model::issue_is_open(issue)
                && !crate::unresolved_blockers_within(issue, &by_id, self.budget)?.is_empty()
            {
                blocked.insert(issue.id.clone());
            }
        }
        Ok(Readiness { ready, blocked })
    }

    fn counts<'i>(
        &self,
        leaves: impl IntoIterator<Item = &'i IssueId>,
        readiness: &Readiness,
    ) -> Result<LeafCounts, Error> {
        let mut counts = LeafCounts::default();
        for id in leaves {
            self.budget.check()?;
            let Some(issue) = self.issues.get(id) else {
                continue;
            };
            counts.total += 1;
            if issue.state_type == WorkflowStateType::Completed {
                counts.done += 1;
            } else {
                *counts
                    .remaining_by_stage
                    .entry(issue.state.clone())
                    .or_default() += 1;
            }
            counts.ready += usize::from(readiness.ready.contains(id));
            counts.blocked += usize::from(readiness.blocked.contains(id));
        }
        Ok(counts)
    }

    fn project_leaves<'d>(
        &self,
        draft: &'d Draft<'_>,
        project: &ProjectId,
    ) -> Result<Vec<&'d IssueId>, Error> {
        let mut found = Vec::new();
        for id in &draft.leaves.leaf_ids {
            self.budget.check()?;
            if self
                .issues
                .get(id)
                .is_some_and(|i| i.project_id.as_ref() == Some(project))
            {
                found.push(id);
            }
        }
        Ok(found)
    }

    fn summarize(
        &self,
        draft: &Draft<'_>,
        readiness: &Readiness,
    ) -> Result<SelectionSummary, Error> {
        self.budget.check()?;
        let selection = &draft.request.selection;
        let usable = matches!(
            draft.state,
            SelectionState::Complete | SelectionState::Incomplete
        );
        let (initiatives, projects, tickets, blocker_layers) = if usable {
            let roots = draft
                .effective
                .iter()
                .filter_map(|id| self.issues.get(id).copied())
                .collect::<Vec<_>>();
            (
                self.initiative_rows(draft, readiness)?,
                self.standalone_rows(draft, readiness)?,
                self.ticket_rows(draft, readiness)?,
                crate::focus_blocker_layers_within(self.snapshot, &roots, self.budget)?,
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };
        let unknown_facts = initiatives
            .iter()
            .flat_map(|row| &row.projects)
            .chain(&projects)
            .flat_map(|row| row.unknown_facts.iter().copied())
            .chain(
                draft
                    .links_uncertain
                    .then_some(UnknownFact::InitiativeLinks),
            )
            .chain(
                draft
                    .references_uncertain
                    .then_some(UnknownFact::UnavailableReferences),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        Ok(SelectionSummary {
            key: draft.request.key.clone(),
            team: selection.team.clone(),
            state: draft.state,
            team_known: draft.team_known,
            members: draft.members.clone(),
            leaf_ids: draft.leaves.leaf_ids.iter().cloned().collect(),
            counts: self.counts(&draft.leaves.leaf_ids, readiness)?,
            counts_availability: match draft.state {
                SelectionState::Empty | SelectionState::Complete => CountAvailability::Complete,
                SelectionState::Incomplete => CountAvailability::Partial,
                SelectionState::Unavailable => CountAvailability::Unavailable,
            },
            initiatives,
            projects,
            tickets,
            blocker_layers,
            unknown_facts,
        })
    }

    fn project_row(
        &self,
        draft: &Draft<'_>,
        readiness: &Readiness,
        project: &Project,
        link: Option<InitiativeLink>,
    ) -> Result<ProjectRow, Error> {
        self.budget.check()?;
        let mut unknown_facts = Vec::new();
        if project.status_category == ProjectStatusCategory::Unknown {
            unknown_facts.push(UnknownFact::ProjectStatusCategory);
        }
        if link == Some(InitiativeLink::Unavailable) {
            unknown_facts.push(UnknownFact::InitiativeLinks);
        }
        let leaves = self.project_leaves(draft, &project.id)?;
        Ok(ProjectRow {
            project_id: project.id.clone(),
            identifier: project
                .identifier
                .clone()
                .unwrap_or_else(|| project.id.0.clone()),
            name: project.name.clone(),
            status_category: project.status_category,
            counts: self.counts(leaves, readiness)?,
            partial: self.project_reasons(draft, project)?.into_iter().collect(),
            shared: false,
            initiative_link: link,
            unknown_facts,
        })
    }

    fn project_reasons(
        &self,
        draft: &Draft<'_>,
        project: &Project,
    ) -> Result<BTreeSet<PartialReason>, Error> {
        let team = &draft.request.selection.team;
        let mut reasons = BTreeSet::new();
        let project_issues = self.project_issues.get(&project.id);
        for id in project_issues.into_iter().flatten() {
            self.budget.check()?;
            let Some(issue) = self.issues.get(id) else {
                continue;
            };
            if !draft.included.contains(id) {
                continue;
            }
            let excluded = draft.excluded.contains(id);
            if excluded && issue.team_key == *team {
                reasons.insert(PartialReason::Exclusion);
            }
            if !excluded && issue.team_key != *team && self.is_countable_leaf(issue) {
                reasons.insert(PartialReason::TeamBoundary);
            }
        }
        if draft.fully_included.contains(&project.id) {
            return Ok(reasons);
        }
        for (member, closure) in &draft.include {
            self.budget.check()?;
            match member {
                NamedMember::Milestone { project_id, .. } if *project_id == project.id.0 => {
                    reasons.insert(PartialReason::MilestoneOnly);
                }
                NamedMember::Issue { .. } => {
                    for id in &closure.issues {
                        self.budget.check()?;
                        if draft.effective.contains(id)
                            && self
                                .issues
                                .get(id)
                                .is_some_and(|i| i.project_id.as_ref() == Some(&project.id))
                        {
                            reasons.insert(PartialReason::TicketOnly);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(reasons)
    }

    fn initiative_rows(
        &self,
        draft: &Draft<'_>,
        readiness: &Readiness,
    ) -> Result<Vec<InitiativeRow>, Error> {
        let team = &draft.request.selection.team;
        let mut rows = Vec::new();
        for (id, held) in &draft.initiatives {
            self.budget.check()?;
            let (identifier, name) = self.initiative_names(id);
            let mut partial = BTreeSet::new();
            let mut shown = Vec::new();
            let mut leaves = Vec::new();
            for project in held.iter().filter_map(|p| self.projects.get(p)) {
                self.budget.check()?;
                if draft.excluded_projects.contains(&project.id) {
                    partial.insert(PartialReason::Exclusion);
                    continue;
                }
                partial.extend(
                    self.project_reasons(draft, project)?
                        .into_iter()
                        .filter(|r| {
                            matches!(r, PartialReason::Exclusion | PartialReason::TeamBoundary)
                        }),
                );
                leaves.extend(self.project_leaves(draft, &project.id)?);
                if self.holds_team_work(project, team) {
                    shown.push(self.project_row(draft, readiness, project, None)?);
                }
            }
            shown.sort_by(|a, b| {
                (&a.identifier, &a.project_id).cmp(&(&b.identifier, &b.project_id))
            });
            rows.push(InitiativeRow {
                initiative_id: id.clone(),
                identifier,
                name,
                projects: shown,
                counts: self.counts(leaves, readiness)?,
                partial: partial.into_iter().collect(),
                shared: false,
            });
        }
        rows.sort_by(|a, b| {
            (&a.identifier, &a.initiative_id).cmp(&(&b.identifier, &b.initiative_id))
        });
        Ok(rows)
    }

    fn initiative_names(&self, id: &str) -> (String, String) {
        self.snapshot
            .projects
            .iter()
            .flat_map(|p| &p.initiatives)
            .find(|i| i.id == id)
            .map_or_else(
                || (id.to_owned(), id.to_owned()),
                |i| (i.identifier.clone(), i.name.clone()),
            )
    }

    fn standalone_rows(
        &self,
        draft: &Draft<'_>,
        readiness: &Readiness,
    ) -> Result<Vec<ProjectRow>, Error> {
        let mut holders = BTreeSet::new();
        for (member, closure) in &draft.include {
            self.budget.check()?;
            if matches!(member, NamedMember::Project { .. }) {
                holders.extend(closure.projects.iter().cloned());
            }
        }
        for id in &draft.leaves.leaf_ids {
            self.budget.check()?;
            if let Some(project) = self.issues.get(id).and_then(|i| i.project_id.clone()) {
                holders.insert(project);
            }
        }
        let link = if self.evidence.initiative_links {
            InitiativeLink::NoneSelected
        } else {
            InitiativeLink::Unavailable
        };
        let mut rows = Vec::new();
        for id in holders {
            self.budget.check()?;
            if draft.excluded_projects.contains(&id) || draft.displayed.contains(&id) {
                continue;
            }
            if let Some(project) = self.projects.get(&id) {
                rows.push(self.project_row(draft, readiness, project, Some(link))?);
            }
        }
        rows.sort_by(|a, b| (&a.identifier, &a.project_id).cmp(&(&b.identifier, &b.project_id)));
        Ok(rows)
    }

    fn ticket_rows(
        &self,
        draft: &Draft<'_>,
        readiness: &Readiness,
    ) -> Result<Vec<TicketRow>, Error> {
        let mut rows = Vec::new();
        for (member, closure) in &draft.include {
            self.budget.check()?;
            if !matches!(member, NamedMember::Issue { .. }) {
                continue;
            }
            let Some(issue) = self.issues.get(&IssueId(member.id().to_owned())) else {
                continue;
            };
            let covered = issue
                .project_id
                .as_ref()
                .is_some_and(|p| draft.fully_included.contains(p));
            if covered || !draft.effective.contains(&issue.id) {
                continue;
            }
            let leaves = closure
                .issues
                .iter()
                .filter(|id| draft.leaves.leaf_ids.contains(*id));
            rows.push(TicketRow {
                issue_id: issue.id.clone(),
                identifier: issue.identifier.clone(),
                project_id: issue.project_id.clone(),
                counts: self.counts(leaves, readiness)?,
                shared: false,
            });
        }
        rows.sort_by(|a, b| (&a.identifier, &a.issue_id).cmp(&(&b.identifier, &b.issue_id)));
        Ok(rows)
    }

    fn team_summaries(
        &self,
        selections: &[SelectionSummary],
        readiness: &Readiness,
    ) -> Result<Vec<TeamSummary>, Error> {
        let mut by_team: BTreeMap<&TeamKey, Vec<&SelectionSummary>> = BTreeMap::new();
        for selection in selections {
            by_team.entry(&selection.team).or_default().push(selection);
        }
        by_team
            .into_iter()
            .map(|(team, group)| {
                self.budget.check()?;
                let mut leaves = BTreeSet::new();
                for selection in group.iter().filter(|s| {
                    matches!(
                        s.state,
                        SelectionState::Complete | SelectionState::Incomplete
                    )
                }) {
                    for id in &selection.leaf_ids {
                        self.budget.check()?;
                        leaves.insert(id.clone());
                    }
                }
                let state = union_state(&group);
                Ok(TeamSummary {
                    team: team.clone(),
                    state,
                    selection_keys: group.iter().map(|s| s.key.clone()).collect(),
                    counts: self.counts(&leaves, readiness)?,
                    leaf_ids: leaves.into_iter().collect(),
                    counts_availability: match state {
                        SelectionState::Empty | SelectionState::Complete => {
                            CountAvailability::Complete
                        }
                        SelectionState::Incomplete => CountAvailability::Partial,
                        SelectionState::Unavailable => CountAvailability::Unavailable,
                    },
                    causes: union_causes(&group),
                })
            })
            .collect()
    }
}

fn closure_issues(resolved: &[(NamedMember, Closure)]) -> BTreeSet<IssueId> {
    resolved
        .iter()
        .flat_map(|(_, closure)| closure.issues.iter().cloned())
        .collect()
}

fn selection_state(
    selection: &NamedSelection,
    team_known: bool,
    members: &[MemberReport],
) -> SelectionState {
    if selection.include.is_empty() {
        return SelectionState::Empty;
    }
    if !team_known {
        return SelectionState::Unavailable;
    }
    let mut includes = members.iter().filter(|m| m.list == ListKind::Include);
    if includes.all(|m| m.outcome == MemberOutcome::Unavailable) {
        return SelectionState::Unavailable;
    }
    if members.iter().all(|m| m.outcome == MemberOutcome::Resolved) {
        SelectionState::Complete
    } else {
        SelectionState::Incomplete
    }
}

fn union_state(group: &[&SelectionSummary]) -> SelectionState {
    let non_empty = group
        .iter()
        .filter(|s| s.state != SelectionState::Empty)
        .collect::<Vec<_>>();
    if non_empty.is_empty() {
        return SelectionState::Empty;
    }
    if non_empty
        .iter()
        .all(|s| s.state == SelectionState::Complete)
    {
        return SelectionState::Complete;
    }
    if non_empty.iter().any(|s| {
        matches!(
            s.state,
            SelectionState::Complete | SelectionState::Incomplete
        )
    }) {
        SelectionState::Incomplete
    } else {
        SelectionState::Unavailable
    }
}

fn union_causes(group: &[&SelectionSummary]) -> Vec<UnionCause> {
    group
        .iter()
        .filter(|s| {
            matches!(
                s.state,
                SelectionState::Incomplete | SelectionState::Unavailable
            ) || !s.unknown_facts.is_empty()
        })
        .map(|s| UnionCause {
            key: s.key.clone(),
            state: s.state,
            team_unavailable: !s.team_known,
            members: s
                .members
                .iter()
                .filter(|m| m.outcome != MemberOutcome::Resolved)
                .cloned()
                .collect(),
            unknown_facts: s.unknown_facts.clone(),
        })
        .collect()
}

/// Marks every placement of a project, initiative or ticket that appears more than once.
fn mark_shared(selections: &mut [SelectionSummary], budget: Budget<'_>) -> Result<(), Error> {
    let mut projects: BTreeMap<ProjectId, usize> = BTreeMap::new();
    let mut initiatives: BTreeMap<String, usize> = BTreeMap::new();
    let mut tickets: BTreeMap<IssueId, usize> = BTreeMap::new();
    for selection in selections.iter() {
        budget.check()?;
        for initiative in &selection.initiatives {
            budget.check()?;
            *initiatives
                .entry(initiative.initiative_id.clone())
                .or_default() += 1;
        }
        for row in selection
            .initiatives
            .iter()
            .flat_map(|i| &i.projects)
            .chain(&selection.projects)
        {
            budget.check()?;
            *projects.entry(row.project_id.clone()).or_default() += 1;
        }
        for ticket in &selection.tickets {
            budget.check()?;
            *tickets.entry(ticket.issue_id.clone()).or_default() += 1;
        }
    }
    for selection in selections.iter_mut() {
        budget.check()?;
        for initiative in &mut selection.initiatives {
            budget.check()?;
            initiative.shared = initiatives[&initiative.initiative_id] > 1;
            for row in &mut initiative.projects {
                budget.check()?;
                row.shared = projects[&row.project_id] > 1;
            }
        }
        for row in &mut selection.projects {
            budget.check()?;
            row.shared = projects[&row.project_id] > 1;
        }
        for ticket in &mut selection.tickets {
            budget.check()?;
            ticket.shared = tickets[&ticket.issue_id] > 1;
        }
    }
    Ok(())
}
