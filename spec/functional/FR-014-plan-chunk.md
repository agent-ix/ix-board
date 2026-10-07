---
id: FR-014
title: Prepare a dependency-ordered plan chunk from a focus
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-006"
    type: implements
provenance:
  source: SWM-36
description: The ix-board CLI and library SHALL emit a bounded, dependency-ordered slice of open focus issues with per-issue readiness and handoff details.
dependencies:
  - FR-001
  - FR-002
  - FR-005
  - FR-010
  - FR-011
  - FR-012
---
# FR-014: Prepare a dependency-ordered plan chunk from a focus

## Description

The CLI SHALL provide `focus plan-chunk` over the inline selectors and focus
membership defined by FR-010, FR-011, and FR-012. It SHALL accept the same
repeated initiative, project, and issue selectors as `focus status`, plus an
optional positive `--size N`. If `--size` is omitted, the command SHALL emit all
open focus issues. `--size N` SHALL limit the result to at most N emitted issue
rows; blocker-only issues and terminal focus issues SHALL not count toward N.

The command SHALL select every resolved focus issue that FR-001 classifies as
open (neither `Completed` nor `Canceled`) before applying the size limit. When
a resolved focus issue is open, ix-board SHALL include it in the candidate set
regardless of blocker state, project run state, workflow state, or spec state.
This is a dependency-ordered handoff view, not a filtered FR-001 ready queue.
Each row SHALL show whether the issue is ready under FR-001, with FR-001
readiness facts explaining why it is or is not ready. ix-board SHALL derive
readiness using the issue's owning project/team and full transitive blocker
rules from FR-001; FR-001 queue ordering SHALL not replace this requirement's
ordering.
When a workflow state has no new Specify or Coding step, the readiness reason
SHALL name that state and explain the eligible planning states in plain language.

The command SHALL derive each focus issue's dependency layer from the complete
transitive `blocked_by` closure defined by FR-002. Every node in that closure,
including blocker-only, completed, and canceled issues, SHALL participate in
the layer calculation. The layer of a node with no blocker prerequisites in the
closure SHALL be 0. The layer of every other node SHALL be one plus the maximum
layer of its blocker prerequisites in the closure; this is the longest-path
depth from a node without prerequisites. ix-board SHALL emit only open focus
issues as rows.

The command SHALL order rows by this complete tuple, in ascending order:
(dependency layer; FR-001 project order; milestone order within that project,
with unmilestoned issues after milestone-assigned issues; stable Linear issue
identifier). FR-001 project order SHALL use project priority, project manual
order, then stable project identifier. Within a project, milestone order SHALL
use Linear milestone `sortOrder` ascending, then milestone identifier. This
command's row order SHALL not use FR-001 ticket priority or ticket manual
order. The stable issue identifier SHALL be the final tie-breaker. Missing
blocker records and dependency cycles SHALL retain FR-002's errors.

For each emitted issue, the output SHALL include its stable identifier, title,
milestone name or `null`, readiness boolean and reasons, the stable identifiers
of other open focus issues whose transitive blocker closure contains it
(`unblocks`, sorted by identifier), and an acceptance-criteria excerpt. The
ix-board CLI and library SHALL derive the excerpt only from the issue's
structured description text; the excerpt is display data. The ix-board CLI and
library SHALL use excerpt text only to render the output. The ix-board CLI and
library SHALL derive focus membership, readiness, and row order from structured
records. The excerpt SHALL use the first Markdown heading whose title is exactly
`Acceptance Criteria` (heading depth 1 through 6; surrounding whitespace is
ignored; matching is case-insensitive). Its section body ends at the next
Markdown heading of any depth. After trimming surrounding whitespace, the
excerpt SHALL contain the first 280 Unicode scalar values of that body and
append `…` when more than 280 remain. If no matching heading or no non-whitespace
section body exists, the excerpt SHALL be `null`.

The operation SHALL use structured Linear fields and local deterministic
processing. It SHALL NOT interpret ticket or comment prose, invoke an LLM, or
read or write GitHub Issues.

## Inputs

- One or more inline initiative, project, or issue selectors accepted by
  `focus status` under FR-010, FR-011, and FR-012.
- An optional positive integer `--size N`.
- Complete structured Linear focus, issue-description, milestone-order, and
  blocker records.

Empty focus, invalid size, incomplete source records, missing blocker records,
and dependency cycles SHALL produce explicit errors and no partial chunk.

## Outputs

- A deterministic sequence of at most N open focus issue rows (or every open
  focus issue when N is omitted), ordered by dependency layer; FR-001 project
  priority, project manual order, and stable project identifier; milestone
  `sortOrder` and identifier within each project with unmilestoned issues last;
  then stable issue identifier.
- Each row's identifier, title, milestone, FR-001 readiness result and reasons,
  sorted `unblocks` focus identifiers, and bounded acceptance-criteria excerpt.
- The actual source snapshot fetch timestamp, preserving cache age under
  FR-010.
- Explicit errors for empty focus, invalid size, incomplete source data, a
  missing blocker, or a dependency cycle; no partial result on error.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-014-AC-1 | The returned chunk includes only open focus issues and orders them by the complete tuple of dependency layer, FR-001 project order, milestone order within the project with unmilestoned issues last, and stable issue identifier; completed, canceled, and blocker-only nodes affect layer calculation but are not emitted. | Test |
| FR-014-AC-2 | The handoff candidate set includes every open focus issue, including issues that are ready, blocked, in a paused project, in Coding or Review, or have an unmapped spec state; each row's readiness result and reasons match FR-001 eligibility for its owning project/team and complete blocker closure. | Test |
| FR-014-AC-3 | Given `--size N`, exactly the first min(N, candidate-count) issue rows appear after deterministic ordering; blocker-only nodes and completed/canceled focus issues do not count toward N. Omitted size returns all open focus issues; zero, negative, non-integer, and malformed values return an error with no partial chunk. | Test |
| FR-014-AC-4 | Given a focus prerequisite with multiple transitive open focus dependents, its `unblocks` lists each dependent once in stable identifier order; unrelated and blocker-only issues are absent. | Test |
| FR-014-AC-5 | Given descriptions with a matching Acceptance Criteria Markdown heading, the excerpt is the trimmed section body through the next heading, limited to 280 Unicode scalar values with `…` only when truncated; absent headings or empty bodies yield null, and description text does not change focus membership or readiness. | Test |
| FR-014-AC-6 | Given identical complete Linear records and size, repeated calls return identical rows, readiness reasons, unblocks, excerpts, ordering, and source fetch timestamp; missing blocker data, a dependency cycle, incomplete input, or empty focus returns an explicit error without partial rows. | Test |
| FR-014-AC-7 | The operation uses only structured Linear reads and makes no GitHub Issues request or mutation. | Inspection |
| FR-014-AC-8 | Given one focus issue with prerequisite paths of unequal lengths in its complete blocker closure and another focus issue on a shorter path, each node receives layer zero when it has no blocker prerequisites and otherwise one plus its deepest prerequisite layer; the longer path's focus issue sorts after the shorter-path issue regardless of milestone order. | Test |
| FR-014-AC-9 | Given unmilestoned and milestone-assigned open focus issues across projects, dependency layer sorts first, FR-001 project priority/manual order/project identifier sorts next, and within each project milestone-assigned rows follow Linear milestone order before its unmilestoned rows; stable issue identifier resolves remaining ties. | Test |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md),
  [FR-002](FR-002-project-chunk.md),
  [FR-005](FR-005-structured-data-boundary.md),
  [FR-010](FR-010-explicit-work-scope.md),
  [FR-011](FR-011-inline-planner-focus.md),
  [FR-012](FR-012-focus-status-and-ready-queue.md)
- Structured Linear issue, issue-description, project, milestone, workflow,
  spec-state, and blocker records.
