---
id: FR-006
title: Direct team leaders to ix-board commands
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The team-leader skill SHALL direct users to the ix-board read commands, and delivery SHALL remove the named Python exporter.
dependencies: []
---
# FR-006: Direct team leaders to ix-board commands

## Description

The team-leader skill SHALL direct users to `ix-board next --team <T>`,
`ix-board project-chunk <project>`, and `ix-board search <query>` for their
corresponding read tasks. Delivery SHALL remove
`/home/peter/dev/linear-initiative-export.py`.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-006-AC-1 | The team-leader skill names all three commands and describes when to use each. | Inspection |
| FR-006-AC-2 | After delivery, the named Python exporter does not exist at its specified path. | Inspection |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)

## Open Questions

Confirm the authoritative team-leader skill location and the environment in
which the named home-directory script must be removed; both locations are
outside the current repository scope.
