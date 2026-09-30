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

Structured scope selection SHALL resolve initiative, project and ticket selectors
and measure leaf-task progress from one complete snapshot.

## Behavior

Selectors form a deduplicated union. Initiative selectors include their projects;
ticket selectors include descendants. Parent relationships are distinct from
prerequisite dependencies. Completed leaves count as done; canceled and duplicate
leaves do not count as work. Source fetch timestamps survive cached reads.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-010-AC-1 | Initiative, project and ticket selectors resolve from structured relationships; overlapping selectors count a ticket only once. | Test |
| FR-010-AC-2 | Progress excludes parents, canceled and duplicate leaves and groups remaining leaves by workflow stage. | Test |
| FR-010-AC-3 | Missing or ambiguous selectors, cyclic ancestry and missing ancestors return explicit errors. | Test |
| FR-010-AC-4 | Transport retains initiative identities, parent IDs and workflow categories; cached reads preserve actual fetch age and forced refresh replaces it. | Test |

## Dependencies

- Linear project initiatives, ticket parent relationships and workflow categories.
- The existing complete workspace snapshot and cache in ReadService.
