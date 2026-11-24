---
id: FR-039
title: Record fleet state durations, idle minutes and actions in a SQLite ledger
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-033"
    type: depends_on
---
# FR-039: Record fleet state durations, idle minutes and actions in a SQLite ledger

## Description

Each `fleet status` and `fleet tick` run SHALL append to a schema-versioned SQLite
ledger stored outside every git work tree, using the atomic-commit journal pattern
of [FR-033](./FR-033-team-flow-effort-export.md). The ledger lets an evaluator and
a director count idle minutes and time-to-action from stored facts.

## Behavior

Tables (version 1): `lane_interval` (lane, host, class, reason, started_at,
ended_at, focus), `idle_minutes` view (lane, host, day, minutes per non-WORKING
class, and minutes while ready work existed), `action` (id, tick id, kind, lane,
emitted_at, executed_at, resolved_at), `defect`, `assignment_history`,
`ground_truth_sample` (lane, time, active or inactive, derived from hook events
at each tick; [FR-040](./FR-040-fleet-calibrate.md)), `calibration_run`,
`audit_sample`, `saturation_sample` ([FR-041](./FR-041-fleet-saturation.md)),
`tick` (id, started_at, committed_at) and `run` (sequence, kind, relay time).

- **Writers.** A writer SHALL hold an exclusive single-writer lock and assign a
  monotonic sequence number to its run; a concurrent run waits up to a bounded time
  and then fails without writing. Intervals are closed in sequence order.
- **Cadence and gaps.** The tick interval is configured (default 5m). The gap rule
  applies at tick cadence: when no committed tick exists for more than twice the
  tick interval, the span is recorded as a gap interval, never as the prior class.
  An ad-hoc `status` run appends observations and does not alter the gap rule.
- **Time.** All times are relay server time ([FR-034](./FR-034-fleet-lane-status.md)).
- **Time to action.** The command computes `executed_at` as the first `post_tool`
  event on the target lane after the action's emission. An action with no lane
  (such as `need_ready_tickets`) has no `executed_at` and is resolved when its
  condition clears. Time-to-action is the elapsed time from a lane entering a
  non-WORKING class to the first run in which it is WORKING.
- **Integrity.** Each run commits atomically; a crash leaves the previous state.
  The default location is the user state directory, overridable by configuration,
  and a path inside a git work tree is refused. A documented read-only `fleet ledger query`
  command and direct SQLite read serve the evaluator. An unsupported schema version
  is refused without rewriting. Retention is configurable and prunes by age.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-039-AC-1 | A tick sequence WORKING, IDLE, IDLE, WORKING produces the expected closed intervals and idle minutes, and time-to-action equals the idle duration to the first WORKING run. | Test |
| FR-039-AC-2 | A span with no committed tick for more than twice the tick interval yields a gap interval and no minutes for any class; ad-hoc status runs do not change this. | Test |
| FR-039-AC-3 | Two concurrent writers serialize under the lock with strictly increasing sequence numbers; a writer that cannot take the lock in the bound fails without writing. | Test |
| FR-039-AC-4 | A simulated crash between interval write and cursor update leaves the previous committed state. | Test |
| FR-039-AC-5 | A ledger path inside a git work tree is refused, and the default path is outside any work tree. | Test |
| FR-039-AC-6 | `executed_at` equals the first `post_tool` on the lane after the action; a lane-less action has none and resolves when its condition clears. | Test |
| FR-039-AC-7 | `fleet ledger query` returns idle minutes per lane per day and actions with time-to-action in text and JSON without writing. | Test |
| FR-039-AC-8 | An unsupported schema version is refused with no change, retention prunes only rows older than the configured age, and replaced assignments, defects and ground-truth samples are stored with their timestamps. | Test |

## Dependencies

- [FR-033](./FR-033-team-flow-effort-export.md) journal pattern; [FR-034](./FR-034-fleet-lane-status.md) classifications and relay-server-time rule.
