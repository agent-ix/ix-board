---
id: FR-036
title: Report plan coverage gaps between ready work and lane assignments
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-001"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-035"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
---
# FR-036: Report plan coverage gaps between ready work and lane assignments

## Description

The `ix-board fleet plan-coverage [--json]` command SHALL report ready tickets that
no lane's assignment contains, lanes whose remaining queue holds fewer than a
configured minimum (default 2) tickets, and assignments that contain a Done,
Canceled or merged ticket. Ready work SHALL come from the [FR-001](./FR-001-ready-queue.md) queue for the configured teams.

## Behavior

The report has three lists: `unowned_ready` (ticket, team, project, queue
position, optional note), `short_queues` (lane, remaining count, minimum) and
`stale_assignments` (lane, ticket, reason `done`, `canceled` or `merged`). A lane's
remaining queue excludes finished tickets. A ticket held only by an `UNREACHABLE`
lane ([FR-034](./FR-034-fleet-lane-status.md)) is not owned: it appears in
`unowned_ready` with the note `owner_unreachable`. An `UNREACHABLE` lane never
appears in `short_queues`. When the queue or assignment source is unreadable the
list that depends on it is `unavailable` with a reason, never empty. The text and
JSON outputs agree. The command exits zero only when all three lists are empty and
available; this is the zero-gap state.

**Persistence.** Each `fleet tick` SHALL record its plan-coverage report in the ledger ([FR-039](./FR-039-fleet-ledger.md)): one `plan_coverage_tick` row with the three counts and the names of any `unavailable` lists, and one `plan_coverage_gap` row per gap, opened by the first tick that reports it (an unowned ready ticket, a short queue with its remaining count and minimum, a stale assignment with its reason) and closed by the first later tick that does not. A gap row records the tick and relay time of each. Gap age is the time since `opened_at`, so a gap that persists across ticks keeps one row. An ad-hoc `plan-coverage` run writes nothing.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-036-AC-1 | A ready ticket in no assignment appears in `unowned_ready` with queue position; one present in any assignment does not. | Test |
| FR-036-AC-2 | A lane with one remaining ticket (minimum 2) appears in `short_queues`; a lane with two does not; a configured minimum overrides the default. | Test |
| FR-036-AC-3 | An assignment containing a Done, Canceled or merged ticket is listed with the matching reason. | Test |
| FR-036-AC-4 | In the zero-gap fixture all three lists are empty, available, and the exit status is zero. | Test |
| FR-036-AC-5 | An unreadable ready queue yields `unavailable` for `unowned_ready` with a reason and a non-zero exit, not an empty list. | Test |
| FR-036-AC-7 | A ready ticket assigned only to an UNREACHABLE lane appears in `unowned_ready` with note `owner_unreachable`, and that lane is absent from `short_queues` even with fewer than the minimum remaining. | Test |
| FR-036-AC-6 | Text and JSON carry identical entries. | Test |
| FR-036-AC-8 | Two ticks reporting the same unowned ready ticket give one `plan_coverage_gap` row with `opened_tick` the first and no close; a third tick without it sets `closed_tick` and `closed_at`; the `plan_coverage_tick` counts of each tick equal that tick's lists. | Test |
| FR-036-AC-9 | A tick whose queue source is unreadable records `unavailable_lists` naming `unowned_ready`, opens and closes no `unowned_ready` row, and an ad-hoc `plan-coverage` run writes no ledger row. | Test |

## Dependencies

- [FR-001](./FR-001-ready-queue.md) provides ready work; [FR-035](./FR-035-fleet-assign.md) provides assignments.
