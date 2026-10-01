---
id: FR-011
title: Resolve inline planner focus into issues and blocker layers
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-003"
    type: implements
provenance:
  source: SWM-33
description: The ix-board library and CLI SHALL resolve inline initiative, project, and issue selectors into a current Linear issue set with blocker layers.
dependencies:
  - FR-001
  - FR-002
  - FR-004
  - FR-010
---
# FR-011: Resolve inline planner focus into issues and blocker layers

## Description

When a planner supplies one or more initiative, project, or issue selectors,
ix-board SHALL resolve them against the current structured Linear workspace
snapshot. Focus selection SHALL use the selector membership,
deduplication, and issue-descendant rules in FR-010. An initiative selects its
assigned projects; a project selects all issues assigned to that project,
including issues assigned to project milestones and issues with no milestone;
an issue selector selects that issue and its descendants. The result SHALL
preserve milestone associations and distinguish milestone-assigned issues from
project-level issues without a milestone.

The CLI SHALL provide `focus expand` to resolve inline focus selectors.
`focus expand` SHALL accept repeated and combined `--initiative`, `--project`,
and `--issue` options. These selectors SHALL be mutually unioned under FR-010.

An expansion result SHALL distinguish the selected projects, their milestone
metadata, issues assigned to each milestone, project-level issues without a
milestone, explicitly selected issues and descendants, and `blocker_layers`.
The blocker layers SHALL apply FR-002's complete transitive `blocked_by`
closure and layer ordering, rooted at every issue in the resolved focus set.
They SHALL include prerequisites regardless of their project, team, or
workflow state. Focus issue membership and blocker closure membership SHALL
remain distinguishable in the result.

Focus resolution SHALL use Linear as its sole issue source. If ix-board expands
a focus, the source client SHALL NOT query or mutate GitHub Issues. The source
client SHALL fetch the workspace records in separate paginated project and
issue queries, including each issue's project-milestone identity, then join
initiative, project, milestone, issue, and blocker relationships locally.
Each query SHALL remain below Linear's 10,000 complexity limit. The source
client SHALL NOT issue a nested team-to-project-to-initiative query that
exceeds that limit.

## Inputs

- One or more inline selectors: `--initiative <id-or-name>`,
  `--project <id-identifier-or-name>`, and/or `--issue <id-or-identifier>`.
- The `focus expand` operation.
- The current complete Linear workspace snapshot and its fetch metadata.

Matching, deduplication, descendants, missing-selector, and ambiguous-selector
behavior follow FR-010. Empty focus input SHALL be rejected.

## Outputs

- A deterministic expansion containing the canonical resolved project IDs,
  their milestone metadata, the deduplicated focus issue IDs, and blocker
  layers with stable issue identifiers.
- The snapshot fetch timestamp so callers can identify the age of the source
  data.
- Explicit errors for invalid input, missing or ambiguous selectors,
  incomplete source records, missing blocker records, or dependency cycles.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-011-AC-1 | Given combined inline initiative, project, and issue selectors, expansion follows FR-010 membership rules, returns the union once per issue, and distinguishes focus issues from added blocker issues. | Test |
| FR-011-AC-2 | Given an initiative with assigned projects, each project's milestones and issues assigned to those projects are present; issues assigned to a milestone are grouped under it, and issues without a milestone are identified as project-level work. | Test |
| FR-011-AC-3 | Given an explicit issue with descendants, the issue and its descendants are included under FR-010 rules; every resolved focus issue roots a complete FR-002 blocker closure, including cross-project, cross-team, completed, and canceled prerequisites. Layers order prerequisites before dependents and entries by stable Linear issue identifier. | Test |
| FR-011-AC-4 | Empty selector input or any missing or ambiguous selector produces an explicit error and no partial expansion. | Test |
| FR-011-AC-5 | An incomplete snapshot, missing blocker record, or dependency cycle returns an explicit error and no partial expansion; repeated expansion of identical complete Linear data returns identical output and ordering. | Test |
| FR-011-AC-6 | Recorded Linear project, initiative, milestone, issue, project-milestone association, and relation fixtures produce the expansion without a GitHub Issues request; transport evidence shows separate paginated project and issue queries, client-side relationship joins, and each issued query remains below 10,000 complexity. | Test |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md),
  [FR-002](FR-002-project-chunk.md),
  [FR-004](FR-004-cache-polling.md),
  [FR-010](FR-010-explicit-work-scope.md)
- Linear structured initiative, project, milestone, issue, and blocker records.
