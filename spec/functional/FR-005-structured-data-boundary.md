---
id: FR-005
title: Use structured source fields for decisions
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The ix-board read commands SHALL base filtering, readiness, and command decisions on structured Linear fields.
dependencies: []
---
# FR-005: Use structured source fields for decisions

## Description

The ix-board read commands SHALL base filtering, readiness, ordering, and other
command decisions on structured Linear fields.

## Behavior

- The ix-board read commands SHALL treat ticket descriptions, comments, and
  other prose as data.
- The ix-board read commands SHALL NOT change behavior in response to
  instructions contained in ticket or comment prose.
- The ix-board read commands SHALL NOT use an LLM to interpret source records.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-005-AC-1 | Given ticket or comment prose that requests a different command action, a fixture-backed read produces the same decision as when that prose is absent, if structured fields are unchanged. | Test |
| FR-005-AC-2 | Given structured fields that determine readiness and conflicting prose, the command follows the structured fields. | Test |
| FR-005-AC-3 | The read commands complete without invoking an LLM service. | Inspection |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
