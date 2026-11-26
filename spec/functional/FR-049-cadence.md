---
id: FR-049
title: Run recurring program jobs from a persistent cadence ledger
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-024"
    type: depends_on
---
# FR-049: Run recurring program jobs from a persistent cadence ledger

## Description

`ix-board cadence due --program P --team T` SHALL read a configured cadence
manifest and the existing fleet ledger, and report every matching job as due,
overdue, or not due. A job without a successful or failed run record is due
immediately. A job is due when its interval has elapsed since its last
completed run, and overdue when at least one additional interval has elapsed.
The command exits 0 when no job is due, 1 when at least one job is due, and 2
when at least one job is overdue. `--alert` SHALL send a relay notification to
the single `alert_recipient` address in the manifest when the configured
cadence checker is missing or due/overdue. That job is the manifest entry whose
id is `cadence-checker`.

`ix-board cadence run <job>` SHALL run exactly the argv sequence declared by
the matching manifest job, without a shell, and SHALL persist the start time,
completion time and outcome in the existing fleet ledger whether the command
succeeds or fails. A snapshot job may declare `after_success`, which SHALL be a
`retro delta --config <file> --notify` argv and SHALL run only after the snapshot
exits successfully. A failed follow-up makes the cadence run fail. Supported
job kinds are `retro_snapshot`, `fleet_tick` and `hygiene`. Jobs are configured
by program and team and declare a stable job id, positive interval, kind, argv
sequence and owner role. The cadence loader SHALL reject an invalid or
ambiguous manifest entry before execution. Cadence does not install or run a
host scheduler.

## Configuration

The cadence manifest is JSON, selected by `IX_BOARD_CADENCE_MANIFEST` or the
default `cadence.json` in ix-board's user configuration directory. Its job
entries use `program`, `team`, `id`, `interval_secs`, `kind`, `command`, and
`owner_role`. A snapshot job may also name an `after_success` argv for its
stored delta notification. The optional top-level `alert_recipient` is one complete
`server:workspace:tab` relay address; `--alert` requires it when an alert is
sent. `command` is an argv array; shell expansion is not performed. The
manifest is shared configuration and is not stored in the fleet ledger.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-049-AC-1 | Due exits 0 with no due jobs, 1 with a due job, and 2 with an overdue job; an absent run is due and exactly one interval elapsed is due rather than overdue. | Test |
| FR-049-AC-2 | A completed run's outcome and timestamps survive reopening the fleet ledger, and a stopped job becomes overdue as its recorded completion ages. | Test |
| FR-049-AC-3 | A run executes the declared argv without shell interpretation and records both success and failure outcomes. | Test |
| FR-049-AC-4 | Program/team selection, unknown jobs, duplicate jobs, invalid intervals and malformed manifest fields are handled deterministically before running a job. | Test |
| FR-049-AC-5 | `--alert` emits an alert only when the configured cadence checker job is overdue. | Test |
| FR-049-AC-6 | Cadence uses the existing fleet ledger and does not create a scheduler or write state into a repository. | Test |
| FR-049-AC-7 | A valid snapshot job runs its configured notifying delta only after snapshot success; snapshot failure skips the delta, and a delta failure is recorded as a failed cadence run. Tests use hand-written retro records and a fake relay sender. | Test |

## Dependencies

- [FR-039](./FR-039-fleet-ledger.md) provides the persistent SQLite ledger and
  its single-writer discipline.
- [FR-024](./FR-024-retro-snapshot.md) defines scheduled retro snapshots.

## References

- SWM-325, “ix-board cadence: due/overdue check, ledger and run for recurring program jobs” (Linear).
