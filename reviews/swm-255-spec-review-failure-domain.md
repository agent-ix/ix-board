---
id: SR-6110
title: "spec-review/failure-domain review of SWM-255"
type: SpecReview
analysis: failure-domain
scope: "agent-ix/ix-board; SWM-252, SWM-255, SWM-256, SWM-257, SWM-258; spec/functional/FR-035..039, FR-041, FR-044, spec/usecase/US-011, spec/spec.md"
review_set: subset
---

## Summary

Ticket: SWM-255 (batched with SWM-252, SWM-256, SWM-257, SWM-258). Reviewed PR ix-board#88 at c5941ff53265d3fa36169868c3a40793dce4a791, diff origin/main...HEAD (FR-035 to FR-039, FR-041, new FR-044, US-011, spec.md). Lens: failure-domain. The review checked that every query reference the merged dev-team role specs (origin/main spec/roles/orchestrator.md and minder.md) attribute to these tickets resolves to a concrete view or series. No AssuranceProfile exists under spec, so no enforced review run applies. This is a spec-only PR, so code-review and gap-analysis are not applicable.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Every open_cycles view counts cycle_event.ended_at, which only the orchestrator writes. An orchestrator that hangs, crashes or stops calling fleet cycle (the 2026-11-23/24 failure) writes no further ends, so every open gap, drift finding and rung keeps open_cycles 0 and every cycle-bounded evaluation passes while nothing acts. A crashed cycle is closed only by the next start. Also unstated: which sessions' cycles count when several sessions write cycles. Count nominal ends (started_at + k x interval_s) for an open or missing cycle, or fall back to tick intervals, and name the counted session. | spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md:23 |
| FND-002 | medium | A window with no saturation_sample rows (ticks down, or the deficit unavailable and so not stored as a value) satisfies 'every sample was zero' vacuously and is valid, which is the false knee the rule exists to prevent. Require sample coverage across the window, and treat an unavailable deficit as nonzero. | spec/functional/FR-041-fleet-saturation.md:95-98 |
| FND-003 | medium | login_input keys only on lane_class_at_answer, taken from the last committed run, which can be up to one tick stale; the declared --class login or usage_limit is ignored. A login answer typed at a lane last classified BLOCKED(dialog) with a matching standing order is recorded with no violation. Text privacy still holds (text is stored only when it equals the configured answer), but the violation is missed. Set login_input when dialog_class is login or usage_limit as well. | spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md:32 |
| FND-004 | medium | The record command has no argument or input channel for the answer text, yet stores answer_text and answer_len and compares the text with the configured answer. If the text is passed on argv it is exposed in the process list and the minder transcript before the never-store rule applies. Specify stdin (or a file descriptor) for the text and forbid argv. | spec/functional/FR-044-fleet-cycle-and-dialog-answer-log.md:29 |
| FND-005 | low | The v1-to-v2 migration is said to be additive and keep every row, but nothing says it runs in one transaction under the writer lock, or what a crash mid-migration leaves. FR-039-AC-9 covers only the happy path. Add atomicity and an interrupted-migration case. | spec/functional/FR-039-fleet-ledger.md:56-57 |
| FND-006 | low | ready_work_available is true for every lane whenever any unowned ready ticket exists anywhere, even one in a project or on a host the lane cannot take. ORCH-R09 says 'ready work ... that the lane can take'. This gives false idle-with-ready-work episodes. Restrict it to tickets in the lane's eligible projects. | spec/functional/FR-039-fleet-ledger.md:89-91 |

## Verdict

FAIL on FND-001: cycle counting depends on the orchestrator writing its own cycle ends, so a dead orchestrator makes every cycle-bounded evaluation pass. Privacy: verified that answer text is never stored for a violation, including login_input on BLOCKED(login) and BLOCKED(usage_limit) lanes, because text is stored only when it equals the configured standing-order answer. FND-003 and FND-004 are detection and input-channel gaps, not storage leaks. The v1-to-v2 migration is additive and keeps rows (FR-039-AC-9); only atomicity is unstated.

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
  }
 ]
}
```
