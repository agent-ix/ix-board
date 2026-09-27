---
id: FR-003
title: Search tickets and projects
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The ix-board search command SHALL return matching ticket and project records for a supplied query.
dependencies: []
---
# FR-003: Search tickets and projects

## Description

The ix-board `search <query>` command SHALL return matching ticket and project
records for the supplied query. Each result SHALL identify whether it is a
ticket or project and SHALL include the structured identity fields needed to
distinguish the record.

## Inputs

- A non-empty query string.

## Outputs

- Matching ticket and project records, each labeled with its record kind and
  structured identity.
- An empty result when no records match.
- A usage error for a missing or empty query.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-003-AC-1 | Given a query matching a ticket and a project, search returns both record kinds and identifies each result's kind. | Test |
| FR-003-AC-2 | Given a query with no match, search returns an empty result without fabricating records. | Test |
| FR-003-AC-3 | Given a missing or empty query, search reports invalid usage and performs no Linear read. | Test |
| FR-003-AC-4 | Given identical structured input and query, repeated searches return identical results and order. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)

## Open Questions

The fields that participate in matching, query syntax, case/diacritic behavior,
result limit, and ordering among equal matches are not defined by SWM-2 and need
product decisions before implementation.
