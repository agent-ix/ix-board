---
id: FR-034
title: Classify every registered agent lane from fused evidence
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-018"
    type: depends_on
---
# FR-034: Classify every registered agent lane from fused evidence

## Description

The `ix-board fleet status [--json]` command SHALL emit exactly one row for every
registered lane, each with one class: `WORKING`, `IDLE`, `BLOCKED(reason)`, `LOOP`,
`STALE-ORDER` or `DEAD`. A lane is an agent session addressed by a relay address on
a host. Classification SHALL be computed by ix-board from structured evidence and
SHALL NOT depend on any agent's self-report or an LLM. `UNKNOWN` SHALL NOT be a
class of `fleet status`.

## Inputs

- The lane registry: the union of lanes with a relay `work agent` record
  ([FR-035](./FR-035-fleet-assign.md)), lanes reported by `herdr agent list` on each
  enabled machine, and lanes named in fleet configuration. A configured host is
  part of the registry even when it reports no lane.
- Hook events from ix-relay: `{lane address, session, ts, kind, cwd, tool, branch}`
  with kinds tool-use, user-prompt, permission-prompt notification, idle-prompt
  notification, stop (with last-message class) and session-end.
- Independent signals from the relay-watcher host collector: a compiler or test
  process attributed to the lane's worktree, per-lane pane-tail classification
  (dialog, login, usage limit, question, queued-input, wait-loop text), and
  `ix_fleet_probe_ok{host,source}` collector health.
- Git/GitHub evidence: commits and pull-request head movements on the branch of
  the lane's worktree.
- Order state from ix-relay delivery receipts (`Acted`, `TimedOut`).
- The [FR-018](./FR-018-agent-activity-verdicts.md) agent verdict where readable.
- Thresholds in configuration with defaults: `working_window` 10m,
  `loop_after` 30m, `dead_after` 15m, `order_ack_deadline` 5m.

## Behavior

Classes are evaluated in this order and the first that matches is emitted:

1. `DEAD`: a session-end event is the latest lane event, or no hook event and no
   herdr pane exist for the lane and the lane's registry entry is older than `dead_after`.
2. `BLOCKED(reason)`: the latest hook notification, or a pane-tail classification
   newer than any later hook tool event, is a dialog, login, usage-limit or
   question; `reason` is `dialog`, `login`, `usage_limit` or `question`.
3. `STALE-ORDER`: an order sent to the lane has been `Delivered` without `Acted`
   for longer than `order_ack_deadline`, or is `TimedOut`.
4. `LOOP`: hook tool activity is present within `working_window`, but the lane has
   produced no independent signal for `loop_after` and either its pane tail shows
   wait-loop text or its last tool-call fingerprints repeat identically.
5. `WORKING`: a hook tool-use or user-prompt event younger than `working_window`
   **and** at least one independent signal inside that window: a compiler or test
   process attributed to the lane's worktree, a pull-request head movement, or a
   commit on its branch. Hook activity alone and herdr `working` alone are never sufficient.
6. `IDLE`: otherwise. An `IDLE` row carries a `subreason`: `quiet` (no recent
   activity), `unconfirmed_activity` (recent hook activity without an independent
   signal, younger than `loop_after`) or `evidence_missing`.

Missing, stale or failed evidence SHALL NOT produce an unclassified row. The lane
SHALL be classed by the rules above using the evidence that is available, and the
report SHALL add a source-defect row naming the source, host, lane (when known),
reason and first-seen time. A lane absent from the registry sources but present in
configuration, and a configured host reporting no lanes, are each a defect row of
kind `missing_lane` or `missing_host`. A fleet report with any defect row SHALL NOT
state that all hosts or lanes are in target; the JSON carries `complete: false`.

Each row carries lane, host, class, reason/subreason, state age, last hook event
time, each evidence item used with its timestamp and source, the focus ticket
([FR-037](./FR-037-fleet-focus-and-drift.md)) and the evidence-schema version.
One failing host SHALL NOT remove other hosts' rows and SHALL be bounded by a
per-host timeout. The command exits non-zero when any defect row exists.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-034-AC-1 | A fixture with N registered lanes across hosts yields exactly N rows, one class each from the six defined classes; no row has class UNKNOWN under any evidence combination in the fixture matrix. | Test |
| FR-034-AC-2 | Hook activity inside `working_window` with no compile, PR-head move or commit is IDLE(unconfirmed_activity), not WORKING; adding any one independent signal yields WORKING; an independent signal with no hook activity is not WORKING. | Test |
| FR-034-AC-3 | A lane whose host probe failed, whose hook events are absent, or whose herdr read timed out is still emitted with a class and a source-defect row naming source, host and reason. | Test |
| FR-034-AC-4 | A configured lane with no registry record and a configured host with no lanes each produce a `missing_lane`/`missing_host` defect row, `complete` is false and the exit status is non-zero. | Test |
| FR-034-AC-5 | The fixture matrix for the precedence DEAD, BLOCKED, STALE-ORDER, LOOP, WORKING, IDLE yields the first matching class when several match; BLOCKED(reason) carries each of dialog, login, usage_limit and question. | Test |
| FR-034-AC-6 | Boundary fixtures at exactly and one second beyond `working_window`, `loop_after`, `dead_after` and `order_ack_deadline` classify as specified, and thresholds read from configuration override the defaults. | Test |
| FR-034-AC-7 | A host whose collection exceeds its timeout is reported as a defect while rows from other hosts are present and unchanged; the run completes within the configured timeout plus a fixed bound. | Test |
| FR-034-AC-8 | Text output and `--json` carry the same rows, classes, defects and `complete` flag; JSON has a versioned schema and example in the docs. | Test |
| FR-034-AC-9 | Two runs over identical evidence produce byte-identical JSON apart from the run timestamp field. | Test |

## Dependencies

- ix-relay SWM-201 (parallel spec) SHALL provide: hook events with the fields above, the `Acted`/`TimedOut` receipt states, the `work agent` assignment record, and the fleet series including `ix_fleet_probe_ok`. These are assumed contracts, not yet verified; any difference changes this requirement.
- [FR-018](./FR-018-agent-activity-verdicts.md) supplies the agent verdict; [FR-039](./FR-039-fleet-ledger.md) records each classification.
