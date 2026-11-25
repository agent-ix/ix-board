---
id: FR-010
title: Resolve explicit work scope and leaf progress
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
---
# FR-010: Resolve explicit work scope and leaf progress

## Description

Structured scope selection SHALL resolve initiative, project, milestone and ticket selectors
and measure leaf-task progress from one complete snapshot.

## Behavior

Selectors form a deduplicated union. Initiative selectors include their projects;
initiative selectors accept Linear UUIDs, human-readable identifiers such as
`I-10`, and exact names without case sensitivity. The source snapshot SHALL
retain the structured initiative identifier for this resolution.
Milestone selectors accept a Linear UUID, a globally unique name without case
sensitivity, or `<project identifier|name>/<milestone name>`. A slash may occur
within either name; the resolver considers the whole input and every project
separator interpretation, then requires one distinct milestone ID. A milestone
contributes only issues assigned to it and its containing project for display;
it does not select the entire project. An ambiguous name fails and
lists candidate project names and UUIDs. A project UUID or identifier takes precedence
over a colliding project name, consistently with `project-chunk`.
ticket selectors include descendants. Parent relationships are distinct from
prerequisite dependencies. Completed leaves count as done; canceled and duplicate
leaves do not count as work. Source fetch timestamps survive cached reads.

## Named selection

A **named selection** is an additive input that reuses the resolvers, closures,
team fence, and leaf classification above. It does not change `ScopeSelection`,
`FocusSelection`, or their results. A named selection has one team, an
**include** list, and an **exclude** list. Each list holds canonical members of
the four existing kinds: initiative, project, milestone, and ticket. A milestone
member carries its owning project ID as well as its milestone ID. The selection
stores selector identities only, never expanded issue IDs.

- **Effective set.** For one selection, the effective issue set is the union of
  the include closures minus the union of the exclude closures, then intersected
  with the team fence. The closures are the existing ones: an initiative selects
  the projects it holds in the current snapshot and their issues; a project
  selects its assigned issues; a milestone selects only its assigned issues; a
  ticket selects itself and its descendants. An exclusion removes by the same
  closure, and an exclusion wins over every inclusion of the same issue within
  its own selection. An exclusion is local to its selection.
- **Combination.** The combined set of several selections of one team is the
  union of each selection's effective set, never the union of includes minus the
  union of excludes. Selections of different teams are separate groups and are
  never combined.
- **Leaves.** Leaf classification keeps full snapshot ancestry. Excluding a child
  does not make its parent a leaf. Excluding a ticket removes that ticket and all
  of its descendants from the selection, even a descendant that the same
  selection includes explicitly, because an exclusion wins. Ancestry is global
  and separate from exclusion: the parent of an excluded or removed child is
  still not a leaf. Canceled and duplicate leaves
  and parents are not counted, as above.
- **Intent.** A named selection with an empty include list is an explicit empty
  selection: its effective set is empty, whatever its team and exclusions, and it
  never means the whole team. A request with zero selections is a distinct
  no-active-selection result. Both differ from a `ScopeSelection` with only a
  team, which still selects the whole team, and from an empty inline
  `FocusSelection`, which is still rejected.
- **Dynamic membership.** Membership is recomputed from the current snapshot and
  the stored selector identities on every evaluation. A project that joins or
  leaves an included or excluded initiative changes membership with no change to
  the selection.
- **Qualified milestones.** A milestone member is valid only when its recorded
  project owns it in the snapshot. A string input keeps the existing grammar,
  including slashes inside either name, and canonicalizes to the milestone ID and
  its owning project ID. A recorded pair whose owner differs is reported, not
  repaired, and a same-named milestone of another project is never selected.
- **Member outcomes.** Every include and exclude member has one typed outcome:
  `Resolved`, `Missing`, `WrongTeam`, `ProjectMismatch`, or `Unavailable`,
  listed in a deterministic order. `WrongTeam` applies to a ticket of another
  team. A project, initiative, or milestone that holds other-team work stays
  resolved, and the other-team work is left out by the team fence. `Missing` is
  reported only when authoritative enumeration evidence covers the member's kind
  and visibility domain (for example archived and hidden records). A snapshot
  being complete for the records it fetched does not by itself prove that it is
  the catalog for a kind. Without such evidence an absent project, milestone, or
  ticket is `Unavailable`; it may be a record that exists but was not enumerated.
  An initiative absent from every project association, and a team with no project
  or issue in the snapshot, are likewise `Unavailable` unless catalog evidence
  proves otherwise. The library SHALL NOT add a catalog service to obtain that
  evidence. The outcome is never inferred from a label, a name, or
  a ticket count. A selection with any outcome other than `Resolved` is
  incomplete: its result names the outcomes, applies only resolved members, and is
  never presented as complete.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-010-AC-1 | Initiative, project and ticket selectors resolve from structured relationships; overlapping selectors count a ticket only once. | Test |
| FR-010-AC-2 | Progress excludes parents, canceled and duplicate leaves and groups remaining leaves by workflow stage. | Test |
| FR-010-AC-3 | Missing or ambiguous selectors and cyclic ancestry return explicit errors; ancestry ends at an unavailable parent (FR-045). | Test |
| FR-010-AC-4 | Transport retains initiative identities, parent IDs and workflow categories; cached reads preserve actual fetch age and forced refresh replaces it. | Test |
| FR-010-AC-5 | A team boundary selects only tickets owned by that team, including unprojected tickets when no narrower selectors exist; project, initiative and ticket selectors intersect that boundary, while leaf classification retains full snapshot ancestry. Unknown teams are refused. | Test |
| FR-010-AC-6 | Project and initiative membership follows assigned project IDs without expanding into descendants in other projects. Explicit ticket selection includes descendants across projects within the team boundary; global ancestry still prevents counting parents as leaves. | Test |
| FR-010-AC-7 | A Linear initiative identifier resolves to the same project and issue set as its UUID or exact name; missing and ambiguous selectors still fail explicitly. | Test |
| FR-010-AC-8 | A milestone UUID, unique name, or project-qualified name selects only its assigned issues, contributes its containing project for reporting, and unions without duplication with other selectors; slash characters inside either name resolve, missing names fail, and ambiguous names list project candidates. | Test |
| FR-010-AC-9 | Given a named selection including initiative I (projects P1 and P2) and excluding P2, the effective leaves are P1's; a P1 leaf excluded by ticket is absent; a P2 leaf also included by ticket is absent because the exclusion wins; removing the exclusions from the selection restores exactly those leaves and no others. | Test |
| FR-010-AC-10 | Excluding an initiative removes the leaves of its current projects; excluding a project removes its assigned leaves; excluding a milestone removes only its assigned leaves and keeps the rest of its project; excluding a ticket removes it and its descendants, including those in other projects; every exclusion is limited to the selection's team. | Test |
| FR-010-AC-11 | Given selections A (includes P) and B (includes P, excludes ticket T in P) of one team, A keeps T, B does not, and the combined set contains T once; given A (includes P1) and B (excludes P1), B's exclusion removes nothing from A. | Test |
| FR-010-AC-12 | Given a parent with two children, excluding one child leaves the parent a non-leaf and uncounted; excluding the parent while including both children counts neither child and never the parent, because the parent exclusion removes its descendants even when the same selection includes them; excluding only a child never promotes its parent to a leaf; a selected ticket whose descendants are all excluded is not turned into a leaf. | Test |
| FR-010-AC-13 | A named selection with an empty include list returns the explicit-empty state with an empty issue set, project set, and leaf set, with and without exclusions and for a known team, and never the whole team; a request with zero selections returns the distinct no-active-selection state; a `ScopeSelection` with only a known team still selects the whole team and an empty inline `FocusSelection` is still rejected. | Test |
| FR-010-AC-14 | Across two snapshots in which project P2 joins and then leaves initiative I, the same selection keeps unchanged bytes and its effective set follows each snapshot; the stored selection contains no issue IDs other than ticket members. | Test |
| FR-010-AC-15 | A milestone member whose recorded project owns it selects its assigned issues; the same milestone ID recorded with another project reports `ProjectMismatch` and selects nothing; a same-named milestone in another project is not selected; string inputs `<project>/<milestone>`, a unique bare name, and names containing slashes canonicalize to the milestone ID with its owning project ID under the AC-8 grammar, and an ambiguous name still fails listing candidates. | Test |
| FR-010-AC-16 | For include and exclude members that are resolved, proven absent by enumeration evidence, a ticket of another team, a milestone with the wrong project, and not provable, the outcomes are `Resolved`, `Missing`, `WrongTeam`, `ProjectMismatch`, and `Unavailable` respectively, in deterministic order, naming kind and identifier; a name, label, or ticket count never changes an outcome; a selection with any non-resolved outcome is reported incomplete with those outcomes. | Test |
| FR-010-AC-17 | An initiative with no project association in the snapshot and no catalog evidence is `Unavailable`, not `Missing`; a team that appears in no project or issue gives an unavailable result rather than an empty or healthy one; an absent project, milestone, or ticket ID is `Missing` only when authoritative enumeration evidence covers that kind and visibility domain, and otherwise `Unavailable`, so an existing but archived or unseen project that the snapshot did not enumerate is `Unavailable` and never `Missing`, even when the snapshot is complete. Legacy `ScopeSelection` still refuses an unknown team. | Test |
| FR-010-AC-18 | An unresolved include member leaves its selection incomplete and contributes no leaves; an unresolved exclude member removes nothing, is reported, and leaves its selection incomplete; neither is silently ignored or turned into a zero result. | Test |
| FR-010-AC-19 | A selection of team X that includes an initiative holding X and Y tasks counts only X's leaves and reports the team-boundary reason; a ticket member of team Y is `WrongTeam`. | Test |
| FR-010-AC-20 | All existing FR-010 fixtures for `ScopeSelection` and `FocusSelection` (AC-1 through AC-8) return identical results and errors after the named selection exists. | Test |

## Dependencies

- Linear project initiatives, ticket parent relationships and workflow categories.
- The existing complete workspace snapshot and cache in ReadService.
- [FR-004](FR-004-cache-polling.md) for the opt-in bounded read that a
  named-selection caller may use to obtain the snapshot.
