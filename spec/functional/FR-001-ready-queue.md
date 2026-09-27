---
id: FR-001
title: Produce a reasoned ready queue
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The ix-board library and CLI SHALL return an ordered ready queue for a requested team using project running state, blocker dependencies, and spec state.
dependencies: []
---
# FR-001: Produce a reasoned ready queue

## Description

The ix-board `next --team <T>` command SHALL return the ready tickets for the
requested team. A ticket is eligible only when its project is running and no
ticket in its `blocked-by` dependency layers remains unresolved. The applicable
spec state SHALL determine the ticket's next step. Each returned row SHALL
include a reason that states the readiness and step facts used to include and
position it.

## Inputs

- A team identifier accepted by `next --team <T>`.
- Structured project, ticket, spec-state, and `blocked-by` dependency data.

## Outputs

- An ordered list of ready ticket rows, each with its next step and a human-
  readable reason grounded in the structured input records.
- A deterministic empty result when no ticket satisfies the eligibility rules.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-001-AC-1 | Given a fixture graph with a running project and an eligible ticket, `next` includes that ticket and a reason identifying its project and spec step. | Test (IT-001) |
| FR-001-AC-2 | Given a ticket whose project is not running, `next` omits that ticket from the ready queue. | Test (IT-001) |
| FR-001-AC-3 | Given a fixture with multiple `blocked-by` layers, `next` omits a ticket while any prerequisite in its dependency layers is unresolved and includes it when all prerequisites are resolved. | Test (IT-001) |
| FR-001-AC-4 | Given identical structured inputs, repeated calls return the same ticket order and reasons. | Test (IT-001) |
| FR-001-AC-5 | Given multiple eligible tickets, the output order follows the configured spec-state step order and states a reason for each row. | Test (IT-001) |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
- **Downstream**: [IT-001](../integration/IT-001-linear-read-fixtures.md)

## Open Questions

The exact Linear value for a running project, spec-state-to-step mapping, tie
ordering, and behavior for blockers outside the requested team remain undecided.
The implementation must not invent these policies; resolve them before treating
the corresponding acceptance criteria as fully specified.
