---
id: FR-002
title: Return a project chunk in one read
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The ix-board project-chunk command SHALL return the requested project's goal, milestones, open tickets, and blocker layers together.
dependencies: []
---
# FR-002: Return a project chunk in one read

## Description

The ix-board `project-chunk <project>` command SHALL return the requested
project's goal, milestones, open tickets, and blocker layers together in one
command result. Issues in `Completed` or `Canceled` states are closed; issues in
all other states are open. The result distinguishes each category and preserves
relationships from structured Linear records.

The ix-board `project-chunk <project>` command SHALL include in
`blocker_layers` the full transitive `blocked_by` closure rooted at the requested
project's open issues. The closure includes those open issues and every
referenced prerequisite, including prerequisites from other projects or teams
and prerequisites in `Completed` or `Canceled` states. The command orders layers
topologically, with prerequisites before their dependents, and orders entries
within each layer by stable Linear issue identifier. It reports a
missing-dependency error if an issue in the closure is absent from the structured
source data, and a dependency-cycle error if the closure contains a cycle.

## Inputs

- A project identifier accepted by `project-chunk <project>`.

## Outputs

- One result containing the project's goal, milestones, open tickets, and
  blocker-layer relationships.
- A clear not-found result when the requested project does not exist in the
  structured source data.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-002-AC-1 | Given a project fixture containing a goal, milestones, open tickets, and blocker dependencies, one `project-chunk` result contains all four categories; `Completed` and `Canceled` issues are excluded from open tickets and other states are included. | Test |
| FR-002-AC-2 | Given a project with no milestones or no open tickets, the result represents the empty category and still returns the other available categories. | Test |
| FR-002-AC-3 | Given an unknown project identifier, the command reports not found and does not substitute another project's data. | Test |
| FR-002-AC-4 | Given identical structured project input, repeated calls return the same categories and dependency relationships. | Test |
| FR-002-AC-5 | Given open issues in the requested project, `blocker_layers` contains their full transitive `blocked_by` closure, including prerequisites in other projects or teams and prerequisites in `Completed` or `Canceled` states. | Test |
| FR-002-AC-6 | Given a dependency graph for the closure, layers place prerequisites before dependents and order entries within each layer by stable Linear issue identifier. | Test |
| FR-002-AC-7 | Given a `blocked_by` reference in the closure to an issue absent from the structured source data, `project-chunk` reports a missing-dependency error. | Test |
| FR-002-AC-8 | Given a cycle in the dependency closure, `project-chunk` reports a dependency-cycle error. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
