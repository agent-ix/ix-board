---
id: FR-012
title: Report status, blockers, and ready work for a focus
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-004"
    type: implements
provenance:
  source: SWM-34
description: The ix-board CLI and library SHALL report project and milestone status, blocker states, and eligible ready work for an inline focus.
dependencies:
  - FR-001
  - FR-002
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
and canceled blockers. Missing blocker records and dependency cycles SHALL
produce the FR-002 errors.

The ready queue SHALL contain focus issues that satisfy FR-001's full ready
eligibility rules, including project-running state, issue and spec state, and
all transitive cross-team blockers being `Completed`. It SHALL use FR-001's
ordering and readiness reasons, restricted to the resolved focus issue set.
The output SHALL preserve the distinction between focus issues and blocker-only
issues; blocker-only issues SHALL NOT enter the ready queue unless they are
also members of the resolved focus issue set.

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
  and percentage.
- Per-focus-issue blocker layers with each blocker’s current workflow state and
  classification.
- The FR-001 eligible ready queue for focus issues, with deterministic order
  and reasons.
- The actual snapshot fetch timestamp, preserving cache age under FR-010.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-012-AC-1 | Given a focus with selected projects and milestones, `focus status` reports each project's and milestone's target date, open issue counts grouped by workflow-state display name, and in-progress issues (`type=started`) in stable order. `Completed` and `Canceled` issues are not counted as open. | Test |
| FR-012-AC-2 | Given project and milestone issue trees with completed, open, canceled, duplicate, and parent issues, progress uses FR-010 leaf classification, reports done/total and percentage, and returns zero counts with a null percentage for an empty countable set. | Test |
| FR-012-AC-3 | Given focus issues with transitive blockers across projects and teams in open, completed, and canceled states, each focus issue's blocker layers include the full FR-002 closure and report each blocker's identifier, title, state, and FR-001 classification. | Test |
| FR-012-AC-4 | Given focus issues with different project, spec, and blocker eligibility, the ready queue includes exactly the focus issues eligible under FR-001, applies FR-001 ordering, and includes its readiness reasons; blocker-only issues are excluded. | Test |
| FR-012-AC-5 | Given identical complete Linear records, repeated `focus status` calls return identical project and milestone groups, issue lists, blocker layers, ready queue order, and reasons; the result reports the source snapshot's actual fetch timestamp. | Test |
| FR-012-AC-6 | An empty focus, incomplete source snapshot, missing blocker record, or dependency cycle returns an explicit error and no partial status result. | Test |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md),
  [FR-002](FR-002-project-chunk.md),
  [FR-010](FR-010-explicit-work-scope.md),
  [FR-011](FR-011-inline-planner-focus.md)
- Structured Linear project, milestone, issue, workflow-state, target-date,
  and blocker records.
