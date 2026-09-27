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

The ix-board `search <query>` command SHALL perform case-insensitive substring
matching over structured project and issue IDs, names, titles, and descriptions.
It SHALL search neither comments nor unstructured prose outside those fields.
Each result SHALL identify whether it is a project or issue and include its
structured identity. Results SHALL be ordered by record kind (projects before
issues), then identifier.

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
| FR-003-AC-1 | Given a query matching structured project and issue IDs, names, titles, or descriptions with different casing, search returns both record kinds and identifies each result's kind. | Test |
| FR-003-AC-2 | Given a query with no match, search returns an empty result without fabricating records. | Test |
| FR-003-AC-3 | Given a missing or empty query, search reports invalid usage and performs no Linear read. | Test |
| FR-003-AC-4 | Given a query that appears only in a comment, search does not return that record. | Test |
| FR-003-AC-5 | Given identical structured input and query, repeated searches return identical results ordered by kind and identifier. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
