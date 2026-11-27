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
succeeds or fails. A snapshot job MAY declare `after_success`.

When declared, `after_success` SHALL be exactly a `retro delta --config <file>`
or `retro delta --team <team>` argv, optionally followed by `--notify` for
interactive harness callers. Scheduled jobs SHALL NOT notify: a scheduler has
no genuine harness identity. The default follow-up SHALL omit `--notify`;
the minder/program-manager reads stored snapshot defects through `retro delta`
and job outcomes through `cadence due`. Cadence SHALL run the follow-up only
after the snapshot exits successfully, passing the selected manifest path
when notification is explicitly requested. A failed
follow-up SHALL make the cadence run fail. Supported
job kinds are `retro_snapshot`, `fleet_tick`, `hygiene` and `host_check`. A
`host_check` job SHALL use exactly the argv `["host", "check"]` and run on the
machine where cadence is invoked. Jobs are configured
by program and team and declare a stable job id, positive interval, kind, argv
sequence and owner role. The cadence loader SHALL reject an invalid or
ambiguous manifest entry before execution. Cadence does not install or run a
host scheduler.

## Configuration

First `cadence due --program QSL --team qsl` on each host generates three local
jobs without extra flags: `retro-qsl` (1800 seconds), `cadence-checker` (host
check, 300 seconds), and `hygiene-check` (300 seconds). The checker host check
also records the scheduler heartbeat. Hygiene uses the local identity when
`--host` is omitted. Cadence accepts `hygiene check`, never `hygiene clean` or
`--apply`; existing `focus hygiene` jobs remain supported. Each host has its
own manifest and ledger. A failed check still records its outcome and time;
cadence freshness is distinct from check health.

Existing manifests are preserved. To adopt the new defaults, move an existing
manifest to an operator-selected backup path, then run `due` again. Set its
single `alert_recipient` to the owning minder's full relay address before
using interactive notifications. `due --alert --dry-run` exercises the same alert
decision and address validation, prints the intended recipient on stderr, and
does not invoke the relay. `all` is invalid even for dry runs.

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
| FR-049-AC-7 | A valid snapshot job runs its configured delta only after snapshot success; snapshot failure skips the delta, and a delta failure is recorded as a failed cadence run. Non-notifying delta and explicit interactive --notify argv are accepted. Cadence tests cover both failure transitions. Delta tests use hand-written retro records and an injected sender; no test sends a real notification. | Test |
| FR-049-AC-8 | First `cadence due` without an explicit or environment manifest creates a default 30-minute retro snapshot job for the requested program/team, followed by `retro delta --team <team>` without notifications; reruns are byte-for-byte no-ops and existing manifests are never replaced. | Test |
| FR-049-AC-9 | A host-check job accepts only `host check`, runs that argv as a child without shell expansion, and records its success or failure in the cadence ledger. | Test |
| FR-049-AC-10 | Default generation adds a 300-second local host-check heartbeat and a 300-second local hygiene check. Check runs without host flags using local identity; cadence rejects hygiene clean and apply argv. Existing manifests remain unchanged. | Test |
| FR-049-AC-11 | An alert dry run validates and names one full relay recipient without starting notification transport; broadcast addresses are rejected. | Test |

## Deployment

Cadence does not install a scheduler. The examples under `deploy/` invoke the
Rust CLI directly; no script or shell loop is needed. They schedule CHECK
only and never delete targets. Install the binary at `~/.local/bin/ix-board`
on each host. Ensure non-login PATH contains `gh`, `agent-msg`, and any source
tools required by retro. Supply host identity and telemetry through the host's
existing ix-board setup. Never copy credential values into unit files.

On draco, the `deploy/launchd/com.agentix.ix-board-cadence-*.plist` examples
use Peter's `/Users/peter` paths. Copy them to `~/Library/LaunchAgents/` and
bootstrap each into `gui/<uid>`. There are separate 30-minute snapshot,
5-minute host-check and hygiene-check jobs, plus a one-minute `due`
checker without `--alert`. All scheduled jobs omit `--notify` and `--alert`;
`alert_recipient` remains available for interactive harness-run commands only.
Use `launchctl bootout` / `bootstrap` on one job to demonstrate aging
without altering ledger timestamps; a job is overdue after two full intervals.

On Linux, copy `deploy/systemd-user/` contents to `~/.config/systemd/user/`.
Run `systemctl --user daemon-reload`, then enable and start
`ix-board-cadence@cadence-checker.timer`,
`ix-board-cadence@hygiene-check.timer`, and `ix-board-cadence-due.timer`.
Run the snapshot timer only on the program's snapshot host; the included
`ix-board-cadence@retro-qsl.timer.d/interval.conf` sets it to 30 minutes.
The instance service executes `cadence run %i`; the due service preserves
the CLI's 0/1/2 freshness results as normal service outcomes. Inspect job
failures separately with `systemctl --user status` and the ledger. User
timers require a persistent user manager (login lingering or an active login).

For an overdue drill, stop one check timer, wait until two intervals have
elapsed since its last completed run, observe `due` exit 2 naming it, and
restart the timer. Do not backdate ledger records. After 24 hours rerun
`ix-board cadence due --program QSL --team qsl --json` on every host and retain
the output and exit status. Exit 1 can occur between a job becoming due and
its scheduler starting it; exit 2 means overdue and needs investigation.

## Dependencies

- [FR-039](./FR-039-fleet-ledger.md) provides the persistent SQLite ledger and
  its single-writer discipline.
- [FR-024](./FR-024-retro-snapshot.md) defines scheduled retro snapshots.

## References

- SWM-325, “ix-board cadence: due/overdue check, ledger and run for recurring program jobs” (Linear).
