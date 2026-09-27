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

The ix-board `next --team <T>` command SHALL return eligible, not-yet-active
tickets for the requested team. A project is running only when its Linear state
is `Started`; `Paused` projects and their tickets are excluded. Issue states
`Completed` and `Canceled` are closed; all other issue states are open. For
Linear workflow states, `type=completed` maps to `Completed` and
`type=canceled` maps to `Canceled`, regardless of display name; other workflow
types retain their display name. Thus a `Done` state with `type=completed` is
closed, while a canceled prerequisite remains unresolved. The
`Specced` spec state maps to Coding, while `Specify` and `Backlog` map to Specify.
Tickets already in Coding or Review, and tickets with any other spec state, are
not in the ready queue.

A ticket is blocked until every prerequisite in its recursive `blocked-by`
dependency graph is `Completed`; this includes prerequisites outside the
requested team. A canceled prerequisite remains unresolved. Order results by
project priority, then project manual order, then ticket priority, then ticket
manual order; use project and ticket identifiers as stable final tie-breakers.
Each row SHALL state the project, next step, and readiness facts that explain
its inclusion and position.

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
| FR-001-AC-1 | Given fixture projects in `Started` and `Paused`, `next` includes eligible tickets only from `Started` projects. | Test |
| FR-001-AC-2 | Given fixture issues in `Completed`, `Canceled`, and other states, only issues in other states are open; tickets in Coding or Review and tickets with unmapped spec states are excluded from the ready queue. | Test |
| FR-001-AC-3 | Given fixture tickets in `Backlog`, `Specify`, and `Specced`, their next steps are Specify, Specify, and Coding respectively. | Test |
| FR-001-AC-4 | A ticket remains excluded while any prerequisite in any recursive `blocked-by` layer, including a cross-team prerequisite, is not `Completed`; a canceled prerequisite does not resolve the block. | Test |
| FR-001-AC-5 | Eligible tickets are ordered by project priority/manual order, then ticket priority/manual order, with identifiers as final tie-breakers; each row explains its project, step, and readiness. | Test |
| FR-001-AC-6 | Given identical structured fixture data, repeated calls return the same rows, order, and reasons. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
