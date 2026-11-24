---
id: SR-6109
title: "spec-review/integrity review of SWM-255"
type: SpecReview
analysis: integrity
scope: "agent-ix/ix-board; SWM-252, SWM-255, SWM-256, SWM-257, SWM-258; spec/functional/FR-035..039, FR-041, FR-044, spec/usecase/US-011, spec/spec.md"
review_set: subset
---

## Summary

Ticket: SWM-255 (batched with SWM-252, SWM-256, SWM-257, SWM-258). Reviewed PR ix-board#88 at c5941ff53265d3fa36169868c3a40793dce4a791, diff origin/main...HEAD (FR-035 to FR-039, FR-041, new FR-044, US-011, spec.md). Lens: integrity. The review checked that every query reference the merged dev-team role specs (origin/main spec/roles/orchestrator.md and minder.md) attribute to these tickets resolves to a concrete view or series. No AssuranceProfile exists under spec, so no enforced review run applies. This is a spec-only PR, so code-review and gap-analysis are not applicable.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | fix_source is still addressed to 'the fleet owner' and FR-039 adds target_role fleet_owner, while the new ladder text says 'the tick emits no action addressed to the owner, so no condition is paged twice' and FR-038-AC-12 requires that no emitted action has an owner as recipient. The orchestrator role spec's ownership table gives fix_source to the orchestrator. As written, a source defect reaches the owner through the tick and through Alertmanager (W3): the double page the ladder rules out. Retarget fix_source to the orchestrator and drop fleet_owner from target_role. | spec/functional/FR-038-fleet-tick.md:46 |
| FND-002 | low | FR-044 writes cycle_event.ended_reason (superseded) and FR-044-AC-2 asserts it, but the FR-039 version-2 table lists cycle_event columns without ended_reason. | spec/functional/FR-039-fleet-ledger.md:77 |
| FND-003 | low | The prose matches a send by recipient and time only; FR-039-AC-13 adds 'from the minder address'. Neither says whether one receipt can satisfy two actions to the same recipient (a rung 0 push and a rung 1 to the same lane, or rung 0 escalate_stuck and rung 2 to the same lead). If it can, one message marks both sent and MIND-R05-E1 misses a suppressed rung. State the sender constraint and one-receipt-per-action matching. | spec/functional/FR-039-fleet-ledger.md:101-102 |
| FND-004 | low | lane_tick is written by fleet tick and fleet status, but FR-039-AC-11 says one row per lane per committed tick and idle_lane_episode_with_ready_work_max counts consecutive rows. Rows from ad-hoc status runs would split or pad episodes. Say status writes none, or key episodes on tick rows only. | spec/functional/FR-039-fleet-ledger.md:69 |
| FND-005 | low | A lane with no configured lead gives the rung 0 action owner orchestrator with reason lead_unresolved, but rung 2 is still addressed to 'the lane's lead', which does not exist. Say where rung 2 goes in that case. | spec/functional/FR-038-fleet-tick.md:63 |

## Verdict

FAIL on FND-001: fix_source still targets the fleet owner, which contradicts the ladder's no-owner-paging rule and FR-038-AC-12 and risks a double page with Alertmanager (W3). The other findings are low-severity consistency gaps. Otherwise the ladder, undetermined-lane and add_lane suppression sections are consistent across FR-038, FR-039 and FR-041, and US-011 traces FR-044.

## Validation evidence

quire validate --scope . on all nine changed files exited 0; git diff --check is clean. quoin 0.36.10, quire 0.52.1, spec-artifacts-process@v0.29.2. No hashes, digests, pins or tracking records were introduced. Selected lenses: base, integrity, failure-domain, dependency. Lenses not selected: evidence, risk-complexity, scope-boundary, ears-conformance.

## Examined scope

```yaml
{
 "clean": false,
 "scope": [
  {
   "id": "FR-035-step-8-10",
   "path": "spec/functional/FR-035-fleet-assign.md",
   "role": "examined",
   "excerpt": "8. **Expected ticket.** The order SHALL carry `--expect-ticket` equal to the first assigned ticket id (the ix-relay FR-071 field, at most 64 bytes), so the relay stores the expectation with the order and the minder and evaluator compare it with the lane focus. An order whose first ticket id exceeds 64 bytes fails before any send."
  },
  {
   "id": "FR-036-persistence",
   "path": "spec/functional/FR-036-fleet-plan-coverage.md",
   "role": "examined",
   "excerpt": "**Persistence.** Each `fleet tick` SHALL record its plan-coverage report in the ledger ([FR-039](./FR-039-fleet-ledger.md)): one `plan_coverage_tick` row with the three counts and the names of any `unavailable` lists, and one `plan_coverage_gap` row per gap, opened by the first tick that reports it (an unowned ready ticket, a short queue with its remaining count and minimum, a stale assignment with its reason) and closed by the first later tick that does not. A gap row records the tick and relay time of each. Gap age is the time since `opened_at`, so a gap that persists across ticks keeps one "
  },
  {
   "id": "FR-037-step-5",
   "path": "spec/functional/FR-037-fleet-focus-and-drift.md",
   "role": "examined",
   "excerpt": "5. **Drift history.** Each `fleet tick` SHALL record every drift finding in the ledger ([FR-039](./FR-039-fleet-ledger.md)) as a `drift_finding` row (lane, rule, focus, evidence times), opened by the first tick that reports it and closed by the first later tick in which the lane has no finding of that rule for that focus. A lane whose focus is `ambiguous` or without focus has its open findings left open and gains none. The lane-minutes with a finding are those between `opened_at` and `closed_at` in which the lane's ledger interval is `WORKING`."
  },
  {
   "id": "FR-038-undetermined",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "A lane whose own evidence is missing or defective is a per-lane condition, distinct from a host-wide one. A host whose registry lease is stale keeps its lanes `UNREACHABLE` ([FR-034](./FR-034-fleet-lane-status.md)) and is handled through `fix_source`; it never produces `act_on_undetermined`. For a reachable host, a lane with a lane-scoped defect still has a class from [FR-034](./FR-034-fleet-lane-status.md), never `UNKNOWN`, and also gets one `act_on_undetermined` action, which the minder executes by treating the lane as idle. Its expected state, for `executed_at` ([FR-039](./FR-039-fleet-ledg"
  },
  {
   "id": "FR-038-ladder",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "The tick owns one in-fleet ladder; the owner is paged only by Alertmanager and the tick emits no action addressed to the owner, so no condition is paged twice. A condition is an action of kind `push_next_ticket`, `resend_order`, `act_on_undetermined`, `answer_dialog`, `escalate_block`, `escalate_stuck`, `escalate_stale_pr` or `escalate_drain`; its `condition_started_at` is the relay time of the tick that first emitted it, kept while its action id persists, including across a gap in ticks."
  },
  {
   "id": "FR-038-add-lane-suppression",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "The tick emits no `add_lane` for a host in any of these states and records each suppression in its output and in the ledger with the reason: `discovery_active` (a capacity run is open for the host, [FR-041](./FR-041-fleet-saturation.md)), `no_recorded_level` (the host has no recorded sustainable level), `at_recorded_level` (the host's non-draining lane count is at or above its recorded level) or `cool_off` (within the hysteresis ceiling after a step-down). Otherwise the existing `add_lane` condition applies. A draining lane counts toward neither the lane count nor a free slot."
  },
  {
   "id": "FR-039-v2",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "Version 2 adds the tables below and the columns named for existing tables. The"
  },
  {
   "id": "FR-041-capacity",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "The command family `ix-board fleet capacity` SHALL also record capacity discovery per host (runs, steps, windows, the found knee, the recorded sustainable level and its history, draining marks) in the fleet ledger and export the recorded level and discovery state as Prometheus series."
  },
  {
   "id": "FR-044",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "ix-board SHALL give the orchestrator a `fleet cycle` command that records the start and end of each orchestrator cycle, and the minder a `fleet answer-log record` command that records every dialog answer it sends, both in the fleet ledger ([FR-039](./FR-039-fleet-ledger.md)). The dev-team role specs count cycles from the cycle events and find unauthorized answers from the answer log. No other requirement owns either command."
  },
  {
   "id": "US-011",
   "path": "spec/usecase/US-011-keep-the-fleet-saturated.md",
   "role": "context_only",
   "excerpt": "relationships: traces_to ix://agent-ix/ix-board/FR-044 (added)"
  },
  {
   "id": "FR-035-AC-9",
   "path": "spec/functional/FR-035-fleet-assign.md",
   "role": "examined",
   "excerpt": "The relay fake receives the order with `--expect-ticket` equal to the first assigned ticket; a first ticket id of 65 bytes fails before any send."
  },
  {
   "id": "FR-035-AC-10",
   "path": "spec/functional/FR-035-fleet-assign.md",
   "role": "examined",
   "excerpt": "The FR-072 list write made by `assign` carries `order_id` equal to the accepted order identity, and the fake relay's transition record for that write holds it; a list write not made by `assign` holds none."
  },
  {
   "id": "FR-035-AC-11",
   "path": "spec/functional/FR-035-fleet-assign.md",
   "role": "examined",
   "excerpt": "For each of `confirmed`, `acted_not_focused`, `ack_expired` and `timed_out` the `assignment_history` row holds `order_id`, `expect_ticket`, `sent_at`, `deadline_at`, `result`, `result_at`, `confirm_latency_s` and `focus_at_deadline` as computed from the fixture; an unconfirmed row's latency is the deadline length."
  },
  {
   "id": "FR-035-AC-12",
   "path": "spec/functional/FR-035-fleet-assign.md",
   "role": "examined",
   "excerpt": "A command killed after the send leaves a `pending` row, and the next committed run sets its `result` and `focus_at_deadline` from the receipts and the focus at the deadline."
  },
  {
   "id": "FR-036-AC-8",
   "path": "spec/functional/FR-036-fleet-plan-coverage.md",
   "role": "examined",
   "excerpt": "Two ticks reporting the same unowned ready ticket give one `plan_coverage_gap` row with `opened_tick` the first and no close; a third tick without it sets `closed_tick` and `closed_at`; the `plan_coverage_tick` counts of each tick equal that tick's lists."
  },
  {
   "id": "FR-036-AC-9",
   "path": "spec/functional/FR-036-fleet-plan-coverage.md",
   "role": "examined",
   "excerpt": "A tick whose queue source is unreadable records `unavailable_lists` naming `unowned_ready`, opens and closes no `unowned_ready` row, and an ad-hoc `plan-coverage` run writes no ledger row."
  },
  {
   "id": "FR-037-AC-7",
   "path": "spec/functional/FR-037-fleet-focus-and-drift.md",
   "role": "examined",
   "excerpt": "A `not_assigned` finding present in ticks 1 and 2 and absent in tick 3 gives one `drift_finding` row opened at tick 1 and closed at tick 3; a different rule or focus on the same lane gives its own row."
  },
  {
   "id": "FR-037-AC-8",
   "path": "spec/functional/FR-037-fleet-focus-and-drift.md",
   "role": "examined",
   "excerpt": "A lane that becomes `ambiguous` keeps its open finding open and gains no new row; the drift lane-minutes share over a fixture equals the hand-computed value counting only `WORKING` minutes."
  },
  {
   "id": "FR-038-AC-10",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "A lane with a `no_events` defect on a reachable host emits one `act_on_undetermined` action targeted at the lane with `treat_as: idle`, keeps its FR-034 class, and a lane of a host with a stale registry lease emits `fix_source` and no `act_on_undetermined`."
  },
  {
   "id": "FR-038-AC-11",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "A persisting `escalate_stuck` condition emits rung 1 to the lane, rung 2 to its lead and rung 3 to the orchestrator in the first tick at or after 1, 2 and 3 tick intervals from the first emission, each once, at a 5 minute and at a 2 minute interval; the action id and `condition_started_at` persist across a missed tick."
  },
  {
   "id": "FR-038-AC-12",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "A rung is emitted although the same recipient already received an earlier rung or the rung 0 action; once the condition clears no further rung is emitted; no emitted action has an owner as recipient."
  },
  {
   "id": "FR-038-AC-13",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "`escalate_stuck` and `escalate_stale_pr` carry the lane's lead and the pull-request lane's lead as owner; a lane with no configured lead gives owner orchestrator with reason `lead_unresolved`."
  },
  {
   "id": "FR-038-AC-14",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "`add_lane` is not emitted for a host in an open capacity run, a host with no recorded level, a host at its recorded level or within its cool-off, each recorded with its reason; a host below its recorded level with ready work and a free slot emits it, and a draining lane counts as neither a lane nor a free slot."
  },
  {
   "id": "FR-038-AC-15",
   "path": "spec/functional/FR-038-fleet-tick.md",
   "role": "examined",
   "excerpt": "A lane draining longer than the drain cap emits `escalate_drain` to the lane's lead and one still within the cap does not."
  },
  {
   "id": "FR-039-AC-9",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "Opening a version 1 ledger migrates it to version 2 with every earlier row unchanged; a ledger of a later version is refused without a change."
  },
  {
   "id": "FR-039-AC-10",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "A gap present in tick 1 and absent in tick 3 gives one `plan_coverage_gap` row with `opened_tick` 1 and `closed_tick` 3; an `unavailable` list in tick 2 neither closes nor reopens it; `open_cycles` counts only cycle ends strictly between open and close."
  },
  {
   "id": "FR-039-AC-11",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "`lane_tick` holds one row per lane per committed tick with class and `ready_work_available` true exactly when the lane has a remaining ticket or an unowned ready ticket exists; `idle_lane_with_ready_work_ticks` and `idle_lane_episode_with_ready_work_max` equal the hand-computed values on a fixture."
  },
  {
   "id": "FR-039-AC-12",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "An order with `--expect-ticket` sent by the minder and one sent by `fleet assign` each get one `order_focus_at_deadline` row in the first run after the deadline, with the focus computed over the window ending at `deadline_at` (a focus change after the deadline does not alter it) and outcomes `confirmed`, `acted_not_focused`, `ack_expired` and `timed_out` matching their receipts."
  },
  {
   "id": "FR-039-AC-13",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "A relay send from the minder address to an action's target after `emitted_at` produces one `action_send` row, a repeated run does not add a second, and a send before `emitted_at` or to another recipient produces none."
  },
  {
   "id": "FR-039-AC-14",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "A tick run with `--session` records that address and its configured role; without it the relay-resolved caller address is recorded; an unconfigured address records role `other`; a minder `session_turn` with no tick within one interval appears in `minder_turns_without_committed_tick`."
  },
  {
   "id": "FR-039-AC-15",
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "role": "examined",
   "excerpt": "Every view in the role-spec view table exists in a fresh ledger under the name `<name>`, is readable through `fleet ledger query`, and returns the hand-computed value on a fixture covering escalation rungs (sent, late and suppressed), unconfirmed assignments with and without a follow-up, and unexecuted actions."
  },
  {
   "id": "FR-041-AC-10",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "`fleet capacity window` on a fixture computes host output as tickets closed plus pull requests merged per hour over the post-ramp-up window with per-lane attribution by focus and branch in `capacity_step_lane_output`, and uses commits per hour with `output_basis` `commits` when the sum is under 5."
  },
  {
   "id": "FR-041-AC-11",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "A window with any `saturation_sample` of positive deficit is `valid` false with `inconclusive_reason` `deficit_nonzero`, and a window with all zeros is valid."
  },
  {
   "id": "FR-041-AC-12",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "`knee_signal` is set at an output gain just under and not at just over 50% of the previous step's per-lane mean, and when throttled share exceeds 5%; `slots_busy_equals_sustainable` is true only when busy equalled sustainable throughout."
  },
  {
   "id": "FR-041-AC-13",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "`end` writes `found_level` and a `capacity_level` row with evidence steps and sets `superseded_at` on the previous row, keeping both; `end inconclusive_no_demand` is refused before two consecutive inconclusive steps and `step_bound` before 8 steps."
  },
  {
   "id": "FR-041-AC-14",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "`start` is refused for a host with an open run, for a third run on one UTC day, and within the cool-off; a `drain` for `memory_below_reserve` writes a `draining_mark` and a `step_down` with `ceiling_until` six hours later, and a `lane_added` before it is listed by `raises_during_cool_off`."
  },
  {
   "id": "FR-041-AC-15",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "`drain_done` is recorded by the tick only when the lane's ticket has a commit or pull request and no build runs; a `remove` before it is listed by `lane_removals_before_drain`; a drain older than the cap without an `escalate_drain` action is listed by `drains_over_cap_without_escalation`."
  },
  {
   "id": "FR-041-AC-16",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "The exporter serves `ix_fleet_capacity_recorded_level{host}`, its timestamp series, `ix_fleet_capacity_discovery_state{host,state}` and `ix_fleet_capacity_draining_lanes{host}` with labels bounded by host and state, omits the level for a host with none, and the values equal the ledger."
  },
  {
   "id": "FR-041-AC-17",
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "role": "examined",
   "excerpt": "Every capacity view in the role-spec query table exists under its name and returns the hand-computed value on a fixture covering a knee run, a no-demand run, a slot-saturated run and an overload step-down."
  },
  {
   "id": "FR-044-AC-1",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "`fleet cycle start` then `end` writes one `cycle_event` with the session, both times and `interval_s` 600 by default and the given value with `--interval`."
  },
  {
   "id": "FR-044-AC-2",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "A second `start` of the same session while a cycle is open closes the first with `ended_reason` `superseded` at the new start, and another session's open cycle is untouched."
  },
  {
   "id": "FR-044-AC-3",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "An answer for an `answer_dialog` action whose standing order matches is recorded with the order id, the answer text, the lane class `BLOCKED(dialog)` and an empty `violation`."
  },
  {
   "id": "FR-044-AC-4",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "An answer with no order, or an order that does not match the dialog or the action, is recorded with `violation` `no_standing_order`, stores no text, stores `answer_len`, and appears in `dialog_answer_violations`; the command exits zero."
  },
  {
   "id": "FR-044-AC-5",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "Any answer recorded while the lane's class is `BLOCKED(login)` or `BLOCKED(usage_limit)` is `login_input`, including one that cites a valid standing order, and stores no text."
  },
  {
   "id": "FR-044-AC-6",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "`answer_dialog_latency_max` equals the hand-computed maximum on a fixture and measures an unanswered action to the evaluation time; `answer_dialog_actions` and `dialog_answer` rows join on `action_id`."
  },
  {
   "id": "FR-044-AC-7",
   "path": "spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md",
   "role": "examined",
   "excerpt": "`dialog_answer` rows survive until the ledger retention age and are pruned with other rows, and a ledger inside a git work tree is still refused."
  }
 ]
}
```
