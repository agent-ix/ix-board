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
`ground_truth_sample` (lane, time, active or inactive, derived from hook events; at tick `t` the command writes labels for `t-W` and never for `t`, because truth at a time needs events up to `W` later; [FR-040](./FR-040-fleet-calibrate.md)), `calibration_run`,
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
- **Time to action.** The command computes `executed_at` as the first transition of
  the target lane into the action's expected state after emission: `push_next_ticket`
  expects WORKING with a focus matching the pushed ticket, `resend_order` expects
  a non-STALE-ORDER class, `answer_dialog` expects a class other than BLOCKED(dialog).
  A lane already in the expected state at emission does not count. An action aimed
  at a lead or a project (`escalate_*`, `need_ready_tickets`, `fix_source`) has no
  `executed_at` and resolves when its condition clears. Time-to-action is the elapsed time from a lane entering a
  non-WORKING class to the first run in which it is WORKING.
- **Integrity.** Each run commits atomically; a crash leaves the previous state.
  The default location is the user state directory, overridable by configuration,
  and a path inside a git work tree is refused. A documented read-only `fleet ledger query`
  command and direct SQLite read serve the evaluator. An unsupported schema version
  is refused without rewriting. Retention is configurable and prunes by age.

### Schema version 2: evidence for the role specs

Version 2 adds the tables below and the columns named for existing tables. The
migration from version 1 is additive and keeps every row. The dev-team
orchestrator and minder role specs evaluate their rules by query references
`ledger.<name>`; each reference resolves to a read-only view named `<name>`
in the ledger, defined here or in the requirement named with it, so an
evaluator reads stored facts and computes nothing itself. The ledger holds
these records so that one store joins them to lane intervals, actions and ticks
under one retention rule; the relay holds transport records of bounded age only.

| Table | Columns | Written by |
|-------|---------|------------|
| `plan_coverage_tick` | tick id, `unowned_ready_count`, `short_queue_count`, `stale_assignment_count`, `unavailable_lists` | `fleet tick` ([FR-036](./FR-036-fleet-plan-coverage.md)) |
| `plan_coverage_gap` | gap id, `kind` (`unowned_ready`, `short_queue`, `stale_assignment`), subject (ticket or lane), detail, `opened_tick`, `opened_at`, `closed_tick`, `closed_at` | `fleet tick` ([FR-036](./FR-036-fleet-plan-coverage.md)) |
| `lane_tick` | tick id, lane, host, class, `remaining_assigned`, `ready_work_available` | `fleet tick` and `fleet status` |
| `drift_finding` | finding id, lane, rule, focus, `opened_tick`, `opened_at`, `closed_at`, evidence times | `fleet tick` ([FR-037](./FR-037-fleet-focus-and-drift.md)) |
| `assignment_history` (extended) | adds `order_id`, `expect_ticket`, `sent_at`, `deadline_at`, `result` (`pending`, `confirmed`, `acted_not_focused`, `ack_expired`, `timed_out`), `result_at`, `confirm_latency_s`, `focus_at_deadline` | `fleet assign` ([FR-035](./FR-035-fleet-assign.md)) |
| `order_focus_at_deadline` | `order_id`, `sender_role` (`orchestrator`, `minder`), `sender_addr`, lane, `expect_ticket`, `deadline_at`, `focus_ticket`, `focus_state`, `outcome` (`confirmed`, `acted_not_focused`, `ack_expired`, `timed_out`), `followup_at` | `fleet tick` and `fleet status` |
| `action` (extended) | adds `severity`, `due_by`, `target_role` (`lane`, `lead`, `orchestrator`, `planner`, `host`, `fleet_owner`), `target_addr`, `condition_started_at`, `parent_action_id`, `rung` (0 to 3) | `fleet tick` ([FR-038](./FR-038-fleet-tick.md)) |
| `action_send` | `action_id`, `sent_at`, `sender_addr`, `recipient_addr`, receipt id | `fleet tick` and `fleet status`, from relay receipts |
| `tick` (extended) | adds `session_addr` and `session_role` | `fleet tick` |
| `session_turn` | `session_addr`, `session_role`, `started_at` | `fleet tick` and `fleet status`, from hook `prompt` events of the configured minder and orchestrator addresses |
| `cycle_event` | `cycle_id`, `session_addr`, `started_at`, `ended_at`, `interval_s` | `fleet cycle` ([FR-044](./FR-044-fleet-cycle-and-dialog-answer-log.md)) |
| `dialog_answer` | see [FR-044](./FR-044-fleet-cycle-and-dialog-answer-log.md) | `fleet answer-log record` |
| `capacity_run`, `capacity_step`, `capacity_step_lane_output`, `capacity_level`, `capacity_event`, `host_output_sample` | see [FR-041](./FR-041-fleet-saturation.md) | `fleet capacity` and `fleet tick` |

- **Gap rows.** A `plan_coverage_gap` row is opened by the first tick whose report
  contains the gap and closed by the first later tick whose report does not. A
  tick whose list is `unavailable` neither opens nor closes the gaps of that list.
  `open_cycles` of any row with an open and a close time is the number of
  `cycle_event.ended_at` values strictly after the open time and strictly before
  the close time (before the evaluation time while the row is open): a row
  closed before the end of the first cycle that ends after it opened has
  `open_cycles` 0.
- **Ready work per lane.** `ready_work_available` is true for a lane in a tick when
  the lane has a remaining assigned ticket or an unowned ready ticket exists in
  the same tick's plan-coverage report.
- **Order focus at the deadline.** For every relay order carrying
  `--expect-ticket`, whoever sent it, the first committed run after its deadline
  passes records one `order_focus_at_deadline` row. `focus_ticket` is the lane's
  derived focus ([FR-037](./FR-037-fleet-focus-and-drift.md)) computed with the
  focus window ending at `deadline_at`, not at the run time. `outcome` is
  `confirmed` when it equals `expect_ticket`; otherwise it is derived from the
  receipts as in [FR-035](./FR-035-fleet-assign.md). `followup_at` is the relay
  time of the first later order or message from the same sender session to the
  lane or its lead, and stays empty until one exists.
- **Action sends.** `action_send` rows join an action to the relay receipt of the
  message that carried it, matched by recipient address and time after
  `emitted_at`, because receipts carry no message body. A run records a send once.
- **Session attribution.** `fleet tick` records the address of the session that ran
  it (the `--session` value, else the relay-resolved caller address) and the role
  configured for that address (`minder`, `orchestrator` or `other`). A run from an
  address with no configured role records role `other`.

Role-spec query views owned by this requirement (the capacity views are defined
in [FR-041](./FR-041-fleet-saturation.md); `answer_dialog_latency_max` and
`dialog_answer_violations` in [FR-044](./FR-044-fleet-cycle-and-dialog-answer-log.md)):

| View (`ledger.<name>`) | Definition |
|------|------------|
| `lanes_with_ready_work` | `lane_tick` rows with `ready_work_available` |
| `plan_coverage_gap_rows` | `plan_coverage_gap` rows |
| `plan_coverage_gap_open_cycles_max` | maximum `open_cycles` over `plan_coverage_gap` rows |
| `short_queue_open_cycles_max` | the same, for `kind` `short_queue` |
| `idle_lane_with_ready_work_ticks` | `lane_tick` rows with class `IDLE` and `ready_work_available` |
| `idle_lane_episode_with_ready_work_max` | longest run of consecutive `lane_tick` rows of one lane in `idle_lane_with_ready_work_ticks`, as `open_cycles` between its first and last tick |
| `drift_findings` | `drift_finding` rows |
| `drift_finding_open_cycles_max` | maximum `open_cycles` over `drift_finding` rows |
| `drift_lane_minutes_share` | minutes of `drift_finding` overlapping `lane_interval` class `WORKING`, over all `WORKING` lane minutes |
| `assignments_sent` | `assignment_history` rows with `sent_at` |
| `unconfirmed_assign_results` | `assignment_history` rows with `result` in `acted_not_focused`, `ack_expired`, `timed_out` |
| `unconfirmed_assignments_without_followup` | rows of `unconfirmed_assign_results` with no later `assignment_history` row for the lane and no `resend_order` or escalation `action` for it emitted by the end of the first cycle ending after `result_at` |
| `assignment_confirm_latency_p90`, `assignment_confirm_latency_max` | p90 and maximum of `confirm_latency_s` over `assignments_sent`; an unconfirmed row counts as its deadline length |
| `ladder_rungs_emitted` | `action` rows with `rung` 1 to 3, plus the `rung` 0 row of each condition that has a ladder |
| `escalation_rung` | `ladder_rungs_emitted` joined with `action_send`: action id, `condition_started_at`, `rung`, `target_role`, `target_addr`, `emitted_at`, `sent_at` |
| `orchestrator_rungs_emitted` | `ladder_rungs_emitted` with `target_role` `orchestrator` |
| `orchestrator_rung_open_cycles_max` | maximum `open_cycles` between `emitted_at` and `resolved_at` of `orchestrator_rungs_emitted` rows |
| `escalation_rung_send_latency_max` | maximum of `sent_at - emitted_at` over `escalation_rung`, in tick intervals, an unsent row measured to the evaluation time |
| `escalation_rungs_suppressed` | `escalation_rung` rows with no `sent_at` after `due_by` |
| `act_on_undetermined_actions`, `push_next_ticket_actions`, `answer_dialog_actions`, `escalate_block_actions`, `add_lane_actions` | `action` rows of that `kind` |
| `act_on_undetermined_unexecuted_ticks` | per action, the number of later committed ticks that emit it again with no `action_send` row and no `executed_at` |
| `minder_addressed_actions` | `action` rows with `target_role` `lane` or `lead` |
| `tick_actions_unexecuted_by_minder` | `minder_addressed_actions` past `due_by` with no `action_send` row and, for `answer_dialog`, no `dialog_answer` row |
| `minder_turns_without_committed_tick` | `session_turn` rows of role `minder` with no `tick` row of the same `session_addr` committed from the turn start to one tick interval later |
| `minder_orders_unconfirmed_at_deadline` | `order_focus_at_deadline` rows with `sender_role` `minder` and `outcome` other than `confirmed` |
| `unconfirmed_orders_not_resent_or_escalated` | those rows with no `followup_at` within one tick interval after `deadline_at` |

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-039-AC-1 | A tick sequence WORKING, IDLE, IDLE, WORKING produces the expected closed intervals and idle minutes, and time-to-action equals the idle duration to the first WORKING run. | Test |
| FR-039-AC-2 | A span with no committed tick for more than twice the tick interval yields a gap interval and no minutes for any class; ad-hoc status runs do not change this. | Test |
| FR-039-AC-3 | Two concurrent writers serialize under the lock with strictly increasing sequence numbers; a writer that cannot take the lock in the bound fails without writing. | Test |
| FR-039-AC-4 | A simulated crash between interval write and cursor update leaves the previous committed state. | Test |
| FR-039-AC-5 | A ledger path inside a git work tree is refused, and the default path is outside any work tree. | Test |
| FR-039-AC-6 | `executed_at` equals the first transition of the target lane into the expected state (push_next_ticket: WORKING with matching focus); a lane already in that state at emission yields none until a later transition; an action aimed at a lead or project has none and resolves when its condition clears. | Test |
| FR-039-AC-7 | `fleet ledger query` returns idle minutes per lane per day and actions with time-to-action in text and JSON without writing. | Test |
| FR-039-AC-8 | An unsupported schema version is refused with no change, retention prunes only rows older than the configured age, and replaced assignments, defects and ground-truth samples are stored with their timestamps. | Test |
| FR-039-AC-9 | Opening a version 1 ledger migrates it to version 2 with every earlier row unchanged; a ledger of a later version is refused without a change. | Test |
| FR-039-AC-10 | A gap present in tick 1 and absent in tick 3 gives one `plan_coverage_gap` row with `opened_tick` 1 and `closed_tick` 3; an `unavailable` list in tick 2 neither closes nor reopens it; `open_cycles` counts only cycle ends strictly between open and close. | Test |
| FR-039-AC-11 | `lane_tick` holds one row per lane per committed tick with class and `ready_work_available` true exactly when the lane has a remaining ticket or an unowned ready ticket exists; `idle_lane_with_ready_work_ticks` and `idle_lane_episode_with_ready_work_max` equal the hand-computed values on a fixture. | Test |
| FR-039-AC-12 | An order with `--expect-ticket` sent by the minder and one sent by `fleet assign` each get one `order_focus_at_deadline` row in the first run after the deadline, with the focus computed over the window ending at `deadline_at` (a focus change after the deadline does not alter it) and outcomes `confirmed`, `acted_not_focused`, `ack_expired` and `timed_out` matching their receipts. | Test |
| FR-039-AC-13 | A relay send from the minder address to an action's target after `emitted_at` produces one `action_send` row, a repeated run does not add a second, and a send before `emitted_at` or to another recipient produces none. | Test |
| FR-039-AC-14 | A tick run with `--session` records that address and its configured role; without it the relay-resolved caller address is recorded; an unconfigured address records role `other`; a minder `session_turn` with no tick within one interval appears in `minder_turns_without_committed_tick`. | Test |
| FR-039-AC-15 | Every view in the role-spec view table exists in a fresh ledger under the name `<name>`, is readable through `fleet ledger query`, and returns the hand-computed value on a fixture covering escalation rungs (sent, late and suppressed), unconfirmed assignments with and without a follow-up, and unexecuted actions. | Test |

## Dependencies

- [FR-033](./FR-033-team-flow-effort-export.md) journal pattern; [FR-034](./FR-034-fleet-lane-status.md) classifications and relay-server-time rule.
