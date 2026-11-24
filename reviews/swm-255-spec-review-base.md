---
id: SR-6108
title: "spec-review/base review of SWM-255"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board; SWM-252, SWM-255, SWM-256, SWM-257, SWM-258; spec/functional/FR-035..039, FR-041, FR-044, spec/usecase/US-011, spec/spec.md"
review_set: subset
---

## Summary

Ticket: SWM-255 (batched with SWM-252, SWM-256, SWM-257, SWM-258). Reviewed PR ix-board#88 at c5941ff53265d3fa36169868c3a40793dce4a791, diff origin/main...HEAD (FR-035 to FR-039, FR-041, new FR-044, US-011, spec.md). Lens: base. The review checked that every query reference the merged dev-team role specs (origin/main spec/roles/orchestrator.md and minder.md) attribute to these tickets resolves to a concrete view or series. No AssuranceProfile exists under spec, so no enforced review run applies. This is a spec-only PR, so code-review and gap-analysis are not applicable.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Two query references the orchestrator role spec attributes to SWM-255/SWM-256 resolve to no view: ledger.defect_rows (ORCH-R08-E1 trigger) and ledger.add_lane_action_open_cycles_max (ORCH-R27-E1 query). The view table defines add_lane_actions but no open-cycles view for it, and no defect_rows view over the defect table. Add both views (add_lane_action_open_cycles_max = max open_cycles between emitted_at and resolved_at of add_lane actions) with fixture ACs, or the role specs' thresholds are not computable. | spec/functional/FR-039-fleet-ledger.md:109-111 |
| FND-002 | high | Two references the orchestrator role spec attributes to SWM-252 are undefined: prom.throttled_hosts_without_step_down (ORCH-R22-E1 query; only its trigger prom.hosts_throttled_at_recorded_level is defined) and prom.reserve_breach_or_thermal_abort_events (ORCH-R25-E1 trigger; FR-041 records overload_event rows but names no view or series by that name). The defined trigger also joins 'the watcher's throttled-share series', which no series name identifies (ORCH-M07 calls it prom.host_throttled_share, itself undefined). Define both, with the 'throttled above 5% for an hour' window and the step-down join for R22. | spec/functional/FR-041-fleet-saturation.md:156 |
| FND-003 | high | An unconfirmed assignment counts as exactly its deadline length (default 5 minutes). ORCH-R05-E1 is assignment_confirm_latency_max <= 5 and ORCH-M04's target is p90 <= 5, so an assignment that never confirms reads as compliant: neither evaluation can ever fail on the defect R05 exists to catch. Count unconfirmed rows as unbounded (or deadline + epsilon, or exclude and count them separately) so a missed confirmation exceeds the threshold. | spec/functional/FR-039-fleet-ledger.md:127 |
| FND-004 | high | open_cycles counts cycle ends strictly between open and close, so a gap closed within its first cycle (the role spec's definition of 'in the cycle it appears') has open_cycles 0, and one closed a full cycle late has 1. The role specs threshold every such rule at <= 1 (ORCH-R06-E1, R10-E1, R13-E1, R27-E1), so a one-cycle-late close passes. Either count the opening cycle (compliance = 1, late = 2) or note that the role specs need '= 0'; the two documents must agree on one base. | spec/functional/FR-039-fleet-ledger.md:84-88 |
| FND-005 | medium | The follow-up test accepts any resend_order or escalation action emitted for the lane. Those actions are emitted automatically by fleet tick for an expired order, so ORCH-R04-E1 passes without the orchestrator re-assigning or escalating anything. The 'by the end of the first cycle' bound is also ambiguous for the 'later assignment_history row' clause. Count only orchestrator acts (a later assignment_history row, or an orchestrator-sent relay message to the lane or lead, via action_send or relay receipts) within the bound. | spec/functional/FR-039-fleet-ledger.md:126 |
| FND-006 | medium | lane_count_reductions and raises_during_cool_off see only capacity_event rows the orchestrator writes itself through fleet capacity, with evidence_kind taken from the drain --reason it chooses. A lane removed or added any other way is invisible, and a claimed thermal reason is never checked against the scheduler series, so ORCH-R12-E1 (reductions without scheduler evidence) cannot catch the thermal-excuse it targets. The writer of lane_added events is also unspecified. Derive reductions and raises from observed lane_tick counts per host and verify evidence_kind against the pause/admission/abort/headroom series at that time. | spec/functional/FR-041-fleet-saturation.md:154 |
| FND-007 | medium | valid is defined as deficit_zero_throughout, so 'valid steps with deficit_zero_throughout false' is empty by construction and ORCH-R17-E1 can never fail. The actual violation (stepping up after an inconclusive window instead of repeating it) is not measured. Conversely discovery_steps_violating_ramp flags a correctly repeated step, whose lanes equal the previous row's, unless 'previous step' means previous step_no; say which. | spec/functional/FR-041-fleet-saturation.md:132 |
| FND-008 | medium | The numerator is a host_output_sample (trailing-hour tickets, PRs and commits) with no stated basis, while the denominator is the step's host_output_per_hour, which is in commits per hour when output_basis is commits. ORCH-M06 defines host output as tickets closed plus PRs merged. State that the sample is computed on the recorded step's output_basis, and say how a commits-basis level compares. | spec/functional/FR-041-fleet-saturation.md:155 |
| FND-009 | low | The view passes when an escalate_drain action row exists, which fleet tick emits automatically, not when the escalation was sent; it should join action_send. draining_lanes also keeps a lane after drain_done until lane_removed, so a finished drain awaiting removal still ages toward the cap. Per the lead's decision the minder executing escalate_drain satisfies ORCH-R30; the role spec text ('SHALL be escalated' by the orchestrator) needs a follow-up note. | spec/functional/FR-041-fleet-saturation.md:148 |
| FND-010 | low | Run end writes a capacity_level row only for a run that ended on a valid step, so a compliant inconclusive_no_demand run with no valid step (or a throttled end) appears in discovery_runs_without_recorded_level and fails ORCH-R19-E1. Exclude end reasons that legitimately record no new level, or record the returned-to level as a row. | spec/functional/FR-041-fleet-saturation.md:136 |

## Verdict

FAIL. The deciding check fails: four query references the merged orchestrator role spec attributes to SWM-252/255/256 resolve to nothing (FND-001, FND-002), and three defined views make their role-spec threshold uncomputable or unfailable (FND-003, FND-004, FND-007). Every other attributed reference was traced and resolves: ORCH-M04/M05/M06, ORCH-R03/R04/R05/R06/R09/R10/R12/R13/R16-R21/R23/R24/R26/R28-R30 triggers and queries, MIND-R01/R04/R05/R07/R09/R10/R11/R12/R13/R14 ledger references, and the SWM-257 evidence for ORCH-R01/R02 (expect_ticket, order_id on FR-072 transitions). The ladder rungs at 1/2/3 tick intervals match the minder role spec. FR-044 is unique against origin/main and the only other open PR (#59, docs only).

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

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | verified in fix commit 4dbc082 |
| FND-002 | fixed | verified in fix commit 4dbc082 |
| FND-003 | fixed | verified in fix commit 4dbc082 |
| FND-004 | fixed | verified in fix commit 4dbc082 |
| FND-005 | fixed | verified in fix commit 4dbc082 |
| FND-006 | fixed | verified in fix commit 4dbc082 |
| FND-007 | fixed | verified in fix commit 4dbc082 |
| FND-008 | fixed | verified in fix commit 4dbc082 |
| FND-009 | fixed | verified in fix commit 4dbc082 |
| FND-010 | fixed | verified in fix commit 4dbc082 |
| FND-011 | fixed | verified in fix commit 65f6b84 (round 2) |
| FND-012 | fixed | verified in fix commit 65f6b84 (round 2) |
| FND-013 | fixed | verified in fix commit 699c01a (round 3) |

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-011 | medium | The new elapsed-time clock counts max(1, ceil(elapsed / cycle_interval)), and the role specs threshold it at <= 1, i.e. closed within 10 minutes of being opened. The role spec allows until the end of the first cycle that ends after the condition is recorded, up to about two intervals depending on cycle phase. On top of that, the close is observed only by the next 5-minute tick. A compliant orchestrator that acts in its next cycle can therefore read 2 and fail. Allow one cycle of phase plus one tick of detection lag (e.g. ceil((elapsed - tick_interval) / cycle_interval) or nominal boundaries crossed), and add the role-spec note that cycles are now clocked, not read from cycle_event. | spec/functional/FR-039-fleet-ledger.md:92-95 |
| FND-012 | medium | lane_count_reductions now includes lane_removed, and evidence_kind is computed from the 15 minutes before each event. A lane removed when a legitimate overload or knee drain finishes, hours after its draining_mark, will usually have empty evidence_kind and be counted as an ORCH-R12 violation. A lane that disappears for a reason other than an orchestrator decision (crash, session exit) is also counted as an orchestrator reduction via lane_change_unrecorded. Let a lane_removed that follows a draining_mark inherit that mark's evidence_kind, and classify removals not caused by the orchestrator separately. | spec/functional/FR-041-fleet-saturation.md:159 |

## Disposition evidence (round 1)

```yaml
{
 "dispositions": [
  {
   "fnd": "FND-001",
   "outcome": "fixed",
   "after_excerpt": "| `defect_rows` | `defect` rows: id, source, host, lane, kind, `first_seen_at`, `closed_at`, `routed_ticket`, `routed_at` |\n| `add_lane_action_open_cycles_max` | maximum `open_cycles` between `emitted_at` and `resolved_at` of `add_lane` actions |"
  },
  {
   "fnd": "FND-002",
   "outcome": "fixed",
   "after_excerpt": "| `prom.throttled_hosts_without_step_down` | series `ix_fleet_capacity_throttled_without_step_down{host}` |\n| `prom.reserve_breach_or_thermal_abort_events` | series `ix_fleet_capacity_overload_events_total{host,cause}`, from `overload_event` rows |"
  },
  {
   "fnd": "FND-003",
   "outcome": "fixed",
   "after_excerpt": "| `assignment_confirm_latency_p90`, `assignment_confirm_latency_max` | p90 and maximum of `confirm_latency_s` over `assignments_sent` whose deadline has passed. An unconfirmed row has no latency and counts as unbounded: the view returns an empty value with a non-zero `unconfirmed` count whenever the p90 rank (or, for the maximum, any row) is unconfirmed, and the evaluator treats an empty value as a violation, never as the deadline length |"
  },
  {
   "fnd": "FND-004",
   "outcome": "fixed",
   "after_excerpt": "`open_cycles` of any row with an open time and a close time is\n  `max(1, ceil((close_or_now - open) / cycle_interval))`, where `cycle_interval`\n  is the configured orchestrator cycle (default 10 minutes, the same value\n  `fleet cycle` records as `interval_s`) and `close_or_now` is the close time, or"
  },
  {
   "fnd": "FND-005",
   "outcome": "fixed",
   "after_excerpt": "| `unconfirmed_assignments_without_followup` | rows of `unconfirmed_assign_results` with no orchestrator action within one `cycle_interval` after `result_at`: a later `assignment_history` row for the lane (a re-assignment), or an `action_send` whose sender is the orchestrator address and whose recipient is the lane or its lead. The tick's automatic `resend_order` action is not an orchestrator action and does not count |"
  },
  {
   "fnd": "FND-006",
   "outcome": "fixed",
   "after_excerpt": "- **Lane changes are observed, not reported.** The tick writes a `lane_added` or `lane_removed` event whenever a host's non-draining lane count in `lane_tick` rises or falls between consecutive ticks. The event carries the reason of a `fleet capacity step`, `drain` or `remove` issued within one tick interval before the change, and `lane_change_unrecorded` when there is none, so a lane changed outside `fleet capacity` is visible."
  },
  {
   "fnd": "FND-007",
   "outcome": "fixed",
   "after_excerpt": "| `discovery_steps_judged_without_demand` | steps whose knee or level decision used an invalid window: a step that follows an invalid window with a new `step_no` instead of a repeat, a `knee_signal` set on an invalid step, and `capacity_level` rows whose `evidence_steps` include an invalid step |"
  },
  {
   "fnd": "FND-008",
   "outcome": "fixed",
   "after_excerpt": "| `host_output_share_of_recorded_level` | latest `host_output_sample` output per hour, on the `output_basis` stored with the host's current `capacity_level` (tickets closed plus pull requests merged, or commits pushed), over `host_output_per_hour` of the step at that level, in percent |"
  },
  {
   "fnd": "FND-009",
   "outcome": "fixed",
   "after_excerpt": "| `drains_over_cap_without_escalation` | `draining_lanes` older than the drain cap with no `action_send` for an `escalate_drain` action ([FR-038](./FR-038-fleet-tick.md)) for the lane; the tick emits `escalate_drain` and the minder sends it, which discharges the orchestrator's escalation duty (the role spec is to be updated to say so) |"
  },
  {
   "fnd": "FND-010",
   "outcome": "fixed",
   "after_excerpt": "| `discovery_runs_without_recorded_level` | ended runs with a non-empty `found_level` and no `capacity_level` row of the same `run_id`; a run ending with no new level is excluded |"
  }
 ],
 "new_findings": [
  {
   "fnd": "FND-011",
   "severity": "medium",
   "confidence": "medium",
   "method": "spec-review/base",
   "check": "soundness",
   "artifact_id": "FR-039",
   "related_ids": [
    "ORCH-R06-E1",
    "ORCH-R13-E1",
    "ORCH-R27-E1",
    "ORCH-R10-E1"
   ],
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "lines": "92-95",
   "excerpt": "`open_cycles` of any row with an open time and a close time is\n  `max(1, ceil((close_or_now - open) / cycle_interval))`, where `cycle_interval`\n  is the configured orchestrator cycle (default 10 minutes, the same value\n  `fleet cycle` records as `interval_s`) and `close_or_now` is the close time, or",
   "finding": "The new elapsed-time clock counts max(1, ceil(elapsed / cycle_interval)), and the role specs threshold it at <= 1, i.e. closed within 10 minutes of being opened. The role spec allows until the end of the first cycle that ends after the condition is recorded, up to about two intervals depending on cycle phase. On top of that, the close is observed only by the next 5-minute tick. A compliant orchestrator that acts in its next cycle can therefore read 2 and fail. Allow one cycle of phase plus one tick of detection lag (e.g. ceil((elapsed - tick_interval) / cycle_interval) or nominal boundaries crossed), and add the role-spec note that cycles are now clocked, not read from cycle_event."
  },
  {
   "fnd": "FND-012",
   "severity": "medium",
   "confidence": "medium",
   "method": "spec-review/base",
   "check": "soundness",
   "artifact_id": "FR-041",
   "related_ids": [
    "ORCH-R12-E1",
    "FR-041-AC-19",
    "FR-041-AC-20"
   ],
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "lines": "159",
   "excerpt": "| `lane_reductions_without_throttle_evidence` | `lane_count_reductions` with empty `evidence_kind`, or reason `lane_change_unrecorded` |",
   "finding": "lane_count_reductions now includes lane_removed, and evidence_kind is computed from the 15 minutes before each event. A lane removed when a legitimate overload or knee drain finishes, hours after its draining_mark, will usually have empty evidence_kind and be counted as an ORCH-R12 violation. A lane that disappears for a reason other than an orchestrator decision (crash, session exit) is also counted as an orchestrator reduction via lane_change_unrecorded. Let a lane_removed that follows a draining_mark inherit that mark's evidence_kind, and classify removals not caused by the orchestrator separately."
  }
 ]
}
```

## New findings (disposition pass 2)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-013 | medium | The FND-011 fix left FR-039 inconsistent with itself. FR-039-AC-10 still states open_cycles = max(1, ceil((close_or_now - open) / cycle_interval)) with no grace, yet its examples (closed at 24 minutes counts 1, at 26 counts 2) only hold with the grace term; on its own formula 24 minutes counts 3. The prose still says 'one still open after two cycles counts 2' and that the <= 1 threshold means 'closed within one cycle', both false with a 15-minute grace (20 minutes counts 1; more than 25 is needed for 2). One sentence is also garbled: 'must read as 1, not as late and `close_or_now` is the close time'. Put the grace term in the AC formula and correct or remove the two stale prose claims. | spec/functional/FR-039-fleet-ledger.md:186 |

## Disposition evidence (round 2)

```yaml
{
 "dispositions": [
  {
   "fnd": "FND-011",
   "outcome": "fixed",
   "after_excerpt": "`max(1, ceil((close_or_now - open - grace) / cycle_interval))`, where `cycle_interval`\n  is the configured orchestrator cycle (default 10 minutes, the same value"
  },
  {
   "fnd": "FND-012",
   "outcome": "fixed",
   "after_excerpt": "- **Lane changes are observed, not reported.** The tick writes a `lane_added`, `lane_removed` or `lane_lost` event whenever a host's non-draining lane count in `lane_tick` rises or falls between consecutive ticks. The event carries the reason of a `fleet capacity step`, `drain` or `remove` issued within one tick interval before the change, and `lane_change_unrecorded` when there is none, so a lane changed outside `fleet capacity` is visible. A removal that follows a `draining_mark` of the same lane inherits that mark's `reason` and `evidence_kind`. A lane that disappears without a `draining_ma"
  }
 ],
 "new_findings": [
  {
   "fnd": "FND-013",
   "severity": "medium",
   "confidence": "high",
   "method": "spec-review/base",
   "check": "soundness",
   "artifact_id": "FR-039-AC-10",
   "related_ids": [
    "FR-039"
   ],
   "path": "spec/functional/FR-039-fleet-ledger.md",
   "lines": "186",
   "excerpt": "| FR-039-AC-10 | A gap present in tick 1 and absent in tick 3 gives one `plan_coverage_gap` row with `opened_tick` 1 and `closed_tick` 3; an `unavailable` list in tick 2 neither closes nor reopens it; `open_cycles` equals `max(1, ceil((close_or_now - open) / cycle_interval))` on the evaluation clock: with a 10 minute cycle and 5 minute tick (grace 15 minutes) a gap closed at 4 minutes and one closed at 24 minutes each count 1 (the second is an orchestrator that acted in its next cycle), one closed at 26 minutes counts 2, and one still open counts by elapsed time while no `cycle_event` row is w",
   "finding": "The FND-011 fix left FR-039 inconsistent with itself. FR-039-AC-10 still states open_cycles = max(1, ceil((close_or_now - open) / cycle_interval)) with no grace, yet its examples (closed at 24 minutes counts 1, at 26 counts 2) only hold with the grace term; on its own formula 24 minutes counts 3. The prose still says 'one still open after two cycles counts 2' and that the <= 1 threshold means 'closed within one cycle', both false with a 15-minute grace (20 minutes counts 1; more than 25 is needed for 2). One sentence is also garbled: 'must read as 1, not as late and `close_or_now` is the close time'. Put the grace term in the AC formula and correct or remove the two stale prose claims."
  }
 ]
}
```

## Disposition evidence (round 3)

```yaml
{
 "dispositions": [
  {
   "fnd": "FND-013",
   "outcome": "fixed",
   "after_excerpt": "| FR-039-AC-10 | A gap present in tick 1 and absent in tick 3 gives one `plan_coverage_gap` row with `opened_tick` 1 and `closed_tick` 3; an `unavailable` list in tick 2 neither closes nor reopens it; `open_cycles` equals `max(1, ceil((close_or_now - open - grace) / cycle_interval))` on the evaluation clock: with a 10 minute cycle and grace of 15 minutes (10 minutes plus a 5 minute tick) a gap closed at 4, 24 and 25 minutes counts 1 each (the later ones are an orchestrator that acted in its next cycle), one closed at 26 minutes counts 2, one closed at 36 minutes counts 3, and one still open co"
  }
 ]
}
```
