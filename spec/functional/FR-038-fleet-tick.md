---
id: FR-038
title: Emit a deterministic action list for the minder
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-035"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-041"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
---
# FR-038: Emit a deterministic action list for the minder

## Description

The `ix-board fleet tick --json` command SHALL evaluate the fleet state once and
return an ordered list of actions for an agent minder to execute. The minder
executes the list and does not re-derive it. The command SHALL NOT perform any
action except to record the tick and its emitted actions in the ledger
([FR-039](./FR-039-fleet-ledger.md)).

## Behavior

Each action has `id`, `kind`, target lane or project, `reason`, evidence
references, `severity` and `due_by`. An action id stays stable across ticks while
its condition persists. Kinds and rules:

| Kind | Condition | Target |
|------|-----------|--------|
| `push_next_ticket` | lane is IDLE with a remaining assigned ticket; carries the ticket and the `assign` command | the lane |
| `answer_dialog` | lane is BLOCKED(dialog) and a standing order in the SWM-226 configuration file matches the dialog; carries the order id and answer | the lane |
| `escalate_block` | lane is BLOCKED(login), BLOCKED(usage_limit) or BLOCKED(question); or BLOCKED(dialog) with no matching standing order; or the SWM-226 file is missing or unreadable. Severity high; carries the block reason | the lane's lead |
| `escalate_stuck` | lane is STUCK or LOOP | the lane's lead |
| `resend_order` | the lane's order is STALE-ORDER | the lane |
| `add_lane` | a host's busy ratio is below its target busy ratio (per-host configuration, default 0.85), ready unassigned tickets exist and a lane slot is free | the host |
| `escalate_stale_pr` | a lane-owned pull request has had no head or review activity longer than `stale_pr_after` (configurable, default 3h) | the lane's lead |
| `need_ready_tickets` | ready-buffer deficit greater than zero ([FR-041](./FR-041-fleet-saturation.md)) | the owning planner project |
| `fix_source` | a source-defect row persists beyond its configured age | the fleet owner |

Actions are sorted by severity, lane and kind. The output for identical evidence is
identical. A lane with an unresolved defect is not excluded. An unreadable
SWM-226 configuration never produces `answer_dialog`.

The exporter ([FR-033](./FR-033-team-flow-effort-export.md)) SHALL read the last committed tick in the ledger as the source of the metrics `ix_fleet_tick_timestamp_seconds` and `ix_fleet_tick_actions{kind}`, and SHALL omit them when
the last tick is older than twice the configured tick interval, so an external alert sees a missing tick. A tick that cannot evaluate emits an error and commits
no tick record.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-038-AC-1 | Each action kind is emitted by its fixture condition and not by its negation, with boundaries at exactly the order deadline, `stale_pr_after` and the busy-ratio target. | Test |
| FR-038-AC-2 | Identical inputs give byte-identical action lists in the specified order, and ids persist across two ticks while the condition persists. | Test |
| FR-038-AC-3 | `answer_dialog` is emitted only with a matching SWM-226 standing order; no match, a missing file and an unreadable file each emit `escalate_block` and no `answer_dialog`. | Test |
| FR-038-AC-4 | BLOCKED login, usage_limit and question emit `escalate_block` targeted at the lane's lead with severity high; STUCK and LOOP emit `escalate_stuck`. | Test |
| FR-038-AC-5 | `stale_pr_after` default is 3h, a configured value overrides it, and the action targets the PR's lane lead. | Test |
| FR-038-AC-6 | A host below its configured busy-ratio target (default 0.85) with unassigned ready work and a free slot emits `add_lane`; at the target it does not. | Test |
| FR-038-AC-7 | The tick metric reflects the ledger's last committed tick, is omitted when older than twice the tick interval, and a failed tick commits no record. | Test |
| FR-038-AC-8 | `tick` makes no relay, Linear or GitHub write; a recording fake sees none. | Test |
| FR-038-AC-9 | A source defect appears as `fix_source` after its configured age and the lane's other actions are still emitted. | Test |

## Dependencies

- [FR-034](./FR-034-fleet-lane-status.md), [FR-035](./FR-035-fleet-assign.md), [FR-039](./FR-039-fleet-ledger.md), [FR-041](./FR-041-fleet-saturation.md). Standing orders come from the SWM-226 configuration file. Host busy ratio and capacity come from the ix-relay watcher series (Prometheus HTTP API).
