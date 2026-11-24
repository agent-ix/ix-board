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
---
# FR-038: Emit a deterministic action list for the minder

## Description

The `ix-board fleet tick --json` command SHALL evaluate the fleet state once and
return an ordered list of actions for an agent minder to execute. The minder
executes the list and does not re-derive it. `tick` SHALL NOT perform an action
itself except to record the tick and its emitted actions in the ledger
([FR-039](./FR-039-fleet-ledger.md)).

## Behavior

Each action has `id`, `kind`, target lane or project, `reason`, evidence
references and `due_by`. The action ids are stable for the same condition across
ticks while it persists. Kinds and rules:

| Kind | Condition |
|------|-----------|
| `push_next_ticket` | lane is IDLE with a remaining assigned ticket; carries ticket and the `assign` command |
| `answer_dialog` | lane is BLOCKED(dialog) and a configured standing order matches the dialog classification; carries the order id and answer. A login or usage-limit block yields `escalate_block` and never an answer |
| `resend_order` | order unacted longer than `order_ack_deadline` |
| `add_lane` | a host's busy ratio is below its target while ready tickets are unowned and a lane slot is free |
| `escalate_stale_pr` | a lane-owned pull request has had no head or review activity for more than 3h; target is the PR owner |
| `need_ready_tickets` | ready-buffer deficit greater than zero ([FR-041](./FR-041-fleet-saturation.md)) |
| `fix_source` | a source-defect row persists beyond its configured age |

Actions are sorted by severity, then lane, then kind. The output for identical
evidence is identical. A lane with an unresolved defect is never excluded from the
list. Each tick exposes a tick metric (`ix_fleet_tick_timestamp_seconds` and
`ix_fleet_tick_actions{kind}`) through the exporter surface of
[FR-033](./FR-033-team-flow-effort-export.md) so an external alert can detect a missing tick.
A tick that cannot evaluate emits an error and no tick metric update.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-038-AC-1 | Each action kind is emitted by its fixture condition and not emitted when the condition is negated, including a boundary at exactly 5m for orders and 3h for pull requests. | Test |
| FR-038-AC-2 | Identical inputs produce byte-identical action lists in the specified order; action ids persist across two ticks while the condition persists. | Test |
| FR-038-AC-3 | A dialog without a matching standing order, a login block and a usage-limit block never emit `answer_dialog` and emit `escalate_block`. | Test |
| FR-038-AC-4 | `push_next_ticket` for an IDLE lane carries the next unfinished assigned ticket and none for a lane with an empty queue. | Test |
| FR-038-AC-5 | Each successful tick updates the tick metric; a failed tick does not. | Test |
| FR-038-AC-6 | `tick` makes no relay, Linear or GitHub write; a recording fake sees none. | Test |
| FR-038-AC-7 | A source defect appears as `fix_source` after its configured age and the lane's actions are still emitted. | Test |

## Dependencies

- [FR-034](./FR-034-fleet-lane-status.md), [FR-035](./FR-035-fleet-assign.md), [FR-041](./FR-041-fleet-saturation.md). Host busy ratio and capacity come from ix-relay SWM-201 fleet series (assumed).
