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
command result. The result distinguishes each category and preserves the
relationships present in the structured source records.

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
| FR-002-AC-1 | Given a project fixture containing a goal, milestones, open tickets, and blocker dependencies, one `project-chunk` result contains all four categories. | Test |
| FR-002-AC-2 | Given a project with no milestones or no open tickets, the result represents the empty category and still returns the other available categories. | Test |
| FR-002-AC-3 | Given an unknown project identifier, the command reports not found and does not substitute another project's data. | Test |
| FR-002-AC-4 | Given identical structured project input, repeated calls return the same categories and dependency relationships. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)

## Open Questions

The status values that count as “open,” treatment of canceled tickets, and
whether goal and milestone records are fetched from Linear or GitHub remain to
be decided.
