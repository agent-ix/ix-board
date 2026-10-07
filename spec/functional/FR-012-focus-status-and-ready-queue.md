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
issue whose transitive `blocked_by` closure contains that blocker. Missing
blocker records and dependency cycles SHALL produce the FR-002 errors.

The ready queue SHALL contain focus issues that satisfy FR-001's full ready
eligibility rules, including project-running state, issue and spec state, and
all transitive cross-team blockers being `Completed`. It SHALL use FR-001's
ordering and readiness reasons, restricted to the resolved focus issue set.
The output SHALL preserve the distinction between focus issues and blocker-only
issues; blocker-only issues SHALL NOT enter the ready queue unless they are
also members of the resolved focus issue set.

The optional repeatable `--work-milestone` selector SHALL accept an exact
milestone name or ID among the selected projects. If supplied, blocker roots
and ready issues SHALL be limited to focus issues assigned to matching
milestones. The complete prerequisite closure of those roots SHALL remain
visible even when a prerequisite belongs to another milestone or project.
Project and milestone progress SHALL remain for the full selected focus, so
the report retains its denominator. A selector matching no selected project
milestone SHALL fail before emitting a partial report. The JSON result SHALL
include the effective work-milestone selectors; no selector means all focus
issues are considered for blockers and ready work.

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
| FR-012-AC-6 | An empty focus, incomplete source snapshot, missing blocker record, or dependency cycle returns an explicit error and no partial status result. | Test |
| FR-012-AC-7 | Project focus issues without a milestone appear in `unmilestoned` with open-state, in-progress, and leaf-progress facts; parent issues contribute to open-state counts but not leaf progress, and terminal output explains the difference. The terminal fetch time is readable UTC with age while JSON retains `fetched_at`. | Test |
| FR-012-AC-8 | A work-milestone selector keeps full project and milestone progress but restricts blocker roots and ready issues to matching milestones, preserving cross-milestone prerequisites; an unknown selector fails explicitly. | Test |
| FR-012-AC-9 | An empty filtered ready queue reports that it is empty and counts otherwise-ready issues excluded by the selected milestones. | Test |
| FR-012-AC-10 | A project reached only through milestone focus is marked `partial_focus`, shows only selected or member-bearing milestones, and labels its project totals as selected-focus totals. The work-milestone view filter uses the same name, UUID, qualification, and ambiguity resolution as saved milestone focus. | Test |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md),
  [FR-002](FR-002-project-chunk.md),
  [FR-005](FR-005-structured-data-boundary.md),
  [FR-010](FR-010-explicit-work-scope.md),
  [FR-011](FR-011-inline-planner-focus.md)
- Structured Linear project, milestone, issue, workflow-state, target-date,
  and blocker records.
