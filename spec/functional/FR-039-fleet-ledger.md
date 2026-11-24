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
ended_at, focus, evidence digest), `idle_minutes` view (lane, host, day, minutes of
IDLE, BLOCKED, STALE-ORDER, LOOP and DEAD, and minutes while ready work existed),
`action` (action id, tick id, kind, lane, emitted_at, first_seen_executed_at,
resolved_at), `defect` (source, host, lane, reason, first_seen, resolved_at),
`assignment_history`, `calibration_run` and `audit_sample` ([FR-040](./FR-040-fleet-calibrate.md)),
`saturation_sample` ([FR-041](./FR-041-fleet-saturation.md)). A class change closes
the previous interval at the observation time; an unobserved gap between runs is
recorded as a gap interval, never as the prior class. Time-to-action is the elapsed
time from a lane entering a non-WORKING class to the first run in which it is
WORKING. Each run commits atomically; a crash leaves the previous state. The default
location is the state directory of the user, overridable by configuration, and a
path inside a git work tree is refused. The ledger is readable by a documented
read-only `fleet ledger query` command and by direct SQLite read. An unsupported
schema version is refused without rewriting. Retention is configurable with a
documented default and prunes by age.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-039-AC-1 | A run sequence WORKING, IDLE, IDLE, WORKING produces the expected closed intervals and idle minutes, and time-to-action equals the idle duration to the first WORKING run. | Test |
| FR-039-AC-2 | A gap between runs longer than twice the cadence yields a gap interval and contributes no minutes to any class. | Test |
| FR-039-AC-3 | A simulated crash between interval write and cursor update leaves the ledger at the previous committed state. | Test |
| FR-039-AC-4 | A ledger path inside a git work tree is refused; the default path is outside any work tree. | Test |
| FR-039-AC-5 | `fleet ledger query` returns idle minutes per lane per day and actions with time-to-action in text and JSON without writing. | Test |
| FR-039-AC-6 | An unsupported schema version is refused with no change; retention prunes only rows older than the configured age. | Test |
| FR-039-AC-7 | Emitted actions, defects and replaced assignments are each stored with their identifying fields and timestamps. | Test |

## Dependencies

- [FR-033](./FR-033-team-flow-effort-export.md) journal pattern; [FR-034](./FR-034-fleet-lane-status.md) classifications.
