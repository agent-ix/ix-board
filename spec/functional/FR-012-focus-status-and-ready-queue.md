---
id: FR-012
title: Report status, blockers, and ready work for a focus
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-004"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-005"
    type: depends_on
provenance:
  source: SWM-34
description: The ix-board CLI and library SHALL report project and milestone status, blocker states, and eligible ready work for an inline focus.
dependencies:
  - FR-001
  - FR-002
  - FR-004
  - FR-005
  - FR-010
  - FR-011
---
# FR-012: Report status, blockers, and ready work for a focus

## Description

The CLI SHALL provide `focus status` over the inline selectors and expansion
defined by FR-010 and FR-011. One result SHALL include the status of the focus,
its blocker details, and its ready queue. This requirement adds those views to
the existing focus expansion; it SHALL NOT redefine selector membership,
issue descendants, or blocker traversal.

For every selected project and its milestones, the result SHALL report the
open issue count grouped by Linear workflow-state display name, the target
date when present, and the issues in progress. An issue is open when FR-001
classifies it as neither `Completed` nor `Canceled`. An issue is in progress
when its Linear workflow-state type is `started`. Open-state groups and issue
lists SHALL be ordered deterministically by state display name and stable
Linear issue identifier, respectively.

Each project SHALL also report an `unmilestoned` group for focus issues
assigned to that project without a milestone. It SHALL use the same open-state,
in-progress, and leaf-progress rules as milestone groups, including when the
group is empty. Terminal output SHALL label open-state counts as including
parent issues and progress as counting leaf tasks.

Open-state counts and in-progress rows SHALL include only resolved focus
member issues assigned to the corresponding project or milestone. Blocker-only
issues SHALL NOT contribute to these counts or rows unless they are also
resolved focus members.

Progress SHALL reuse FR-010's leaf-work classification: completed leaves
count as done; canceled and duplicate leaves do not count as work; parents do
not count as leaves. Project progress SHALL count classified leaf issues in
that project that belong to the resolved focus. Milestone progress SHALL count
the subset assigned to that milestone. Each issue SHALL contribute at most
once to each applicable project or milestone group. The result SHALL include
done and total leaf counts and the completed percentage. For a group with no
countable leaves, the CLI SHALL report zero done and total counts.
For a group with no countable leaves, the CLI SHALL report a `null` percentage.
Each milestone's target date SHALL come from its Linear milestone record; a
project's target date SHALL come from its Linear project record. The CLI SHALL
report a `null` target date when the corresponding Linear record has no date.

For every focus issue, the result SHALL expose its complete transitive
`blocked_by` closure from FR-011, including each blocker’s stable identifier,
title, workflow-state display name, and FR-001 state classification
(`Completed`, `Canceled`, or open). The result SHALL preserve the blocker-layer
ordering and include blockers from every project and team, including completed
and canceled blockers. The result SHALL report the aggregate FR-011 blocker
layers, with each blocker included once in its layer and a `focus_root_ids`
list sorted by stable Linear issue identifier, naming every resolved focus
issue whose transitive `blocked_by` closure contains that blocker. Dependency
cycles SHALL produce the FR-002 error; unavailable records follow
[FR-046](FR-046-dangling-references.md).

The ready queue SHALL contain focus issues that satisfy FR-001's full ready
eligibility rules, including project-running state, issue and spec state, and
all transitive cross-team blockers being `Completed`. It SHALL use FR-001's
ordering and readiness reasons, restricted to the resolved focus issue set.
The output SHALL preserve the distinction between focus issues and blocker-only
issues; blocker-only issues SHALL NOT enter the ready queue unless they are
also members of the resolved focus issue set.

The optional repeatable `--work-milestone` selector SHALL accept an exact
milestone name or ID among the selected projects. If supplied, the CLI and
library SHALL limit blocker roots and ready issues to focus issues assigned to
matching milestones. The complete prerequisite closure of those roots SHALL remain
visible even when a prerequisite belongs to another milestone or project.
Project and milestone progress SHALL remain for the full selected focus, so
the report retains its denominator. A selector matching no selected project
milestone SHALL fail before emitting a partial report. The JSON result SHALL
include the effective work-milestone selectors; no selector means all focus
issues are considered for blockers and ready work.

## Named-selection summaries

The library SHALL additionally evaluate a **batch** of FR-010 named selections
against one snapshot and return, in one deterministic result, a summary for each
selection and a combined summary for each team group. This extends, and does not
replace, `focus status`; it reuses FR-001 readiness, FR-002 prerequisite
closure, FR-010 membership and leaf rules, and the existing project status
category. It introduces no second calculator and no store.

- **One snapshot.** Every selection and every combination in one batch is
  computed from one snapshot, whose actual fetch time the result carries. The
  caller's opaque key for each selection (for example a focus ID and revision) is
  returned unchanged with that selection's result, so a result is never presented
  as another selection or another snapshot. The result order is the order of the
  FR-010 member and selection keys, then by name, then by ID, and the same input
  gives the same output.
- **Per-selection summary.** Each selection reports its state (`empty`, `none`,
  `complete`, `incomplete`, or `unavailable`), its FR-010 member outcomes, its
  deduplicated leaf set, and for that set: done, total, remaining by stage,
  ready, and blocked counts. A ready leaf is one that FR-001 would place in its
  ready queue. A blocked leaf is an open leaf whose complete FR-002 transitive
  `blocked_by` closure contains a prerequisite not classified `Completed`.
- **Placements.** Each selection reports initiative rows with their projects,
  standalone project rows, and direct-ticket rows. The selection's effective
  exclusions apply first: an explicitly excluded project is omitted and gets no
  standalone project row, even if the same selection also includes it directly.
  A project that is not so excluded, that the selection includes directly or
  that holds a selected leaf, and that is not displayed under any selected
  initiative, is a **standalone project row** carrying an
  initiative-link status: `none selected` when the selection includes no
  initiative that holds it (a known absence), or `unavailable` when initiative
  links for it cannot be established from the snapshot. These two are distinct
  and an unavailable link is never reported as no selected initiative. A project
  row is never dropped because no initiative is displayed, and a directly
  included project that is authoritatively known to be empty and is not excluded
  keeps its standalone row with zero counts. A row's counts are restricted to that selection's leaves.
  A row is marked **partial**, with one or more reasons, when the selection covers
  only part of the Linear object: a milestone-only inclusion, a ticket-only
  inclusion, an exclusion, or the team boundary. A row is marked **shared** when
  it appears in more than one selection of the batch or under more than one
  initiative, and every placement of it shows the same value for the same leaf
  scope.
- **Combination.** For each team, the combined summary counts the union of the
  leaf sets of that team's selections once per leaf, with the same fields.
  Different teams are separate groups with no cross-team total, union, or
  readiness. No result sums per-selection, per-initiative, or per-project
  totals. The union's state follows its selections: it is `complete` only when
  every non-`empty` selection of that team is `complete` (and at least one is
  non-`empty`). When any selection of the team is `incomplete` or `unavailable`,
  including alongside `empty` selections, the union is `incomplete` (or
  `unavailable` when no selection of the team has a usable result), its counts are marked partial,
  and it names, for each such selection, the caller's key and the cause (the
  unresolved or unavailable members and the unknown facts). The provisional
  leaves of an incomplete selection are never merged into a union presented as
  complete merely because another selection of the team is complete. `empty`
  selections add no leaves and do not degrade a union that is otherwise known;
  unknown-dependent membership or metrics of any selection still contaminate the
  union's availability and provenance. A team whose selections
  are all `empty` has union state `empty`. Partial availability is therefore
  visible per team and per selection.
- **Prerequisites.** An excluded prerequisite stays in the complete blocker
  closure as evidence and is not counted as a member of the selection. An
  exclusion SHALL NOT make an included leaf ready or hide a blocker.
- **Unknown is not zero.** A selection that is `empty` (explicit empty include
  list), the batch with zero selections (`none`), and a selection with an
  unresolved member, an unavailable member or team, or an incomplete result are
  distinct states. A project whose native status category is `Unknown` keeps that
  classification in its row. Counts that depend on unresolved, unavailable, or
  unknown facts are marked unavailable or partial, never complete, healthy, or
  zero.
- **Failures.** A snapshot that is incomplete, a parent cycle, or a dependency
  cycle returns the existing explicit error for the whole batch and no partial
  result. An unavailable record is an [FR-046](FR-046-dangling-references.md)
  warning on the batch. A failed or timed-out read
  returns the FR-004 error and no result. The library keeps no last-good result
  and no revision cache; the caller keeps any last-good result with its original
  fetch time.

## Inputs

- One or more inline initiative, project, or issue selectors accepted by
  `focus status` and resolved under FR-010 and FR-011.
- A complete structured Linear workspace snapshot with projects, project
  milestones, issues, workflow states, project target dates, and blocker
  relations.

Selector matching, deduplication, issue descendants, and focus membership
follow FR-010 and FR-011. An empty focus or incomplete source snapshot SHALL
produce an explicit error and no partial status result.

## Outputs

- A deterministic status result containing project and milestone target dates,
  open issue counts by state, in-progress issue rows, and leaf progress counts
  and percentage, restricted to resolved focus members.
- Aggregate blocker layers with each blocker’s current workflow state and
  classification, plus the sorted `focus_root_ids` attribution list.
- The FR-001 eligible ready queue for focus issues, with deterministic order
  and reasons.
- The actual snapshot fetch timestamp, preserving cache age under FR-010.
  Terminal output SHALL show the fetch time as UTC ISO time and its age; JSON
  SHALL retain the Unix-second `fetched_at` value.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-012-AC-1 | Given selected projects and milestones, `focus status` reports each group's target date, open issue counts grouped by workflow-state display name, and in-progress issues (`type=started`) in stable order, considering only resolved focus members assigned to the group. `Completed` and `Canceled` issues are not counted as open; blocker-only issues do not contribute unless they are also focus members. | Test |
| FR-012-AC-2 | Given project and milestone issue trees with completed, open, canceled, duplicate, and parent issues, progress uses FR-010 leaf classification, reports done/total and percentage, and returns zero counts with a null percentage for an empty countable set. | Test |
| FR-012-AC-3 | Given two focus issues with a shared transitive blocker and other blockers across projects and teams in open, completed, and canceled states, aggregate layers include the full FR-002 closure, each blocker appears once in its layer, and each blocker row reports its identifier, title, state, FR-001 classification, and sorted `focus_root_ids`. The shared blocker row lists both focus issues in identifier order; other blockers list only the focus issue whose closure contains them. | Test |
| FR-012-AC-4 | Given focus issues with different project, spec, and blocker eligibility, the ready queue includes exactly the focus issues eligible under FR-001, applies FR-001 ordering, and includes its readiness reasons; blocker-only issues are excluded. | Test |
| FR-012-AC-5 | Given identical complete Linear records, repeated `focus status` calls return identical project and milestone groups, issue lists, blocker layers, ready queue order, and reasons; the result reports the source snapshot's actual fetch timestamp. | Test |
| FR-012-AC-6 | An empty focus, incomplete source snapshot, or dependency cycle returns an explicit error and no partial status result. | Test |
| FR-012-AC-7 | Project focus issues without a milestone appear in `unmilestoned` with open-state, in-progress, and leaf-progress facts; parent issues contribute to open-state counts but not leaf progress, and terminal output explains the difference. The terminal fetch time is readable UTC with age while JSON retains `fetched_at`. | Test |
| FR-012-AC-8 | A work-milestone selector keeps full project and milestone progress but restricts blocker roots and ready issues to matching milestones, preserving cross-milestone prerequisites; an unknown selector fails explicitly. | Test |
| FR-012-AC-9 | An empty filtered ready queue reports that it is empty and counts otherwise-ready issues excluded by the selected milestones. | Test |
| FR-012-AC-10 | A project reached only through milestone or issue focus is marked `partial_focus`, shows only selected or member-bearing milestones, and labels its project totals as selected-focus totals. The work-milestone view filter uses the same name, UUID, qualification, and ambiguity rules as saved milestone focus, but searches only focus projects so an out-of-focus name collision does not block a view. | Test |
| FR-012-AC-11 | Given three named selections of two teams and one snapshot, one batch call returns each selection's result with its caller key, one combined result per team, and one snapshot fetch time; repeating the call on identical input returns identical order and values; a call on a changed snapshot returns results that all come from that one snapshot. | Test |
| FR-012-AC-12 | Given a selection with open, completed, canceled, duplicate and parent issues, the per-selection done, total, remaining-by-stage, ready, and blocked counts equal the FR-010 leaf classification and the FR-001 and FR-002 results for the same leaves; two selections over the same project with different effective sets report their own counts. | Test |
| FR-012-AC-13 | Given two selections of one team that share project P and one leaf placed under two initiatives, the combined result counts each leaf once, each per-selection row keeps its own count, P carries the shared mark in every placement with the same value for the same leaf scope, and no output field equals a sum of overlapping row totals. | Test |
| FR-012-AC-14 | Given selections of teams X and Y, the results form separate groups and no total, union, or readiness spans teams. | Test |
| FR-012-AC-15 | Partial reasons are reported on initiative rows and project rows for a milestone-only inclusion, a ticket-only inclusion, an exclusion, and a team boundary that left other-team leaves out, each alone and together; a ticket-only selection whose project is not included lists its tickets as direct-ticket rows with the project partial and never becomes a project or team selection. | Test |
| FR-012-AC-16 | Given an included leaf whose only open prerequisite is excluded by the selection, the leaf is not ready and is counted blocked, the prerequisite appears in the complete blocker closure and is not counted in the selection's totals; the same holds for an excluded prerequisite in another project or team; excluding a completed prerequisite changes no readiness. | Test |
| FR-012-AC-17 | An explicit empty selection returns state `empty` with no rows and a valid zero leaf count; a batch with zero selections returns `none` with no totals at all; a selection with an unresolved or unavailable member or an unknown team returns `incomplete` or `unavailable` with counts marked unavailable or partial; the four results are distinguishable, and no result that depends on an unresolved, unavailable, or unknown fact is reported as complete, healthy, or zero. | Test |
| FR-012-AC-18 | A project whose native status category is `Unknown` is reported with that classification and its row is not reported as complete or healthy; the existing status category derivation is reused and not changed. | Test |
| FR-012-AC-19 | A parent cycle, a dependency cycle, or an incomplete snapshot fails the whole batch with the existing explicit error and no partial result; a bounded-read timeout or rate limit returns its FR-004 error and no result; the library stores no last-good result, revision cache, or named definition. | Test |
| FR-012-AC-20 | Inspection confirms the batch evaluation reuses the FR-010 resolvers and leaf classification, the FR-001 readiness evaluation, the FR-002 closure, and the existing status category, and adds no second calculator, store, service, or raw tracker read. | Inspection |
| FR-012-AC-21 | Given a batch of team X with a `complete` selection A and a selection B whose exclude member is unresolved, team X's union is `incomplete`, is not reported complete or healthy, names B's key and cause, and marks its counts partial; A's own result stays `complete`; a team Y group with only complete selections stays `complete`; a team whose selections are all `empty` reports union `empty`; a `complete` selection A plus an `empty` selection B yields a `complete` union; an `incomplete` A plus an `empty` B yields an `incomplete` union (`unavailable` when no selection has a usable result). | Test |
| FR-012-AC-22 | Given a selection that includes only project P directly and no initiative, the result contains a standalone project row for P with initiative-link status `none selected`; given a snapshot whose initiative links for P cannot be established, the row reports `unavailable` instead; the two are distinguishable, and in neither case is P's row omitted or P's leaves counted as a direct ticket row; given a selection that includes P and also excludes P, no standalone row for P appears; given a selection that directly includes an authoritatively known-empty P without excluding it, the standalone row for P is retained with zero counts. Direct-ticket placement is unchanged. | Test |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md),
  [FR-002](FR-002-project-chunk.md),
  [FR-005](FR-005-structured-data-boundary.md),
  [FR-010](FR-010-explicit-work-scope.md),
  [FR-011](FR-011-inline-planner-focus.md),
  [FR-004](FR-004-cache-polling.md) for the bounded snapshot read
- Structured Linear project, milestone, issue, workflow-state, target-date,
  and blocker records.
