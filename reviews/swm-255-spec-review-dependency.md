---
id: SR-6111
title: "spec-review/dependency review of SWM-255"
type: SpecReview
analysis: dependency
scope: "agent-ix/ix-board; SWM-252, SWM-255, SWM-256, SWM-257, SWM-258; spec/functional/FR-035..039, FR-041, FR-044, spec/usecase/US-011, spec/spec.md"
review_set: subset
---

## Summary

Ticket: SWM-255 (batched with SWM-252, SWM-256, SWM-257, SWM-258). Reviewed PR ix-board#88 at c5941ff53265d3fa36169868c3a40793dce4a791, diff origin/main...HEAD (FR-035 to FR-039, FR-041, new FR-044, US-011, spec.md). Lens: dependency. The review checked that every query reference the merged dev-team role specs (origin/main spec/roles/orchestrator.md and minder.md) attribute to these tickets resolves to a concrete view or series. No AssuranceProfile exists under spec, so no enforced review run applies. This is a spec-only PR, so code-review and gap-analysis are not applicable.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The new capacity discovery reads build scheduler series (dev-team FR-015: pause, admission, probe, slots), Linear and GitHub for host output, FR-037 focus for attribution, and the FR-039 ledger, and relies on fleet tick (FR-038) to write drain_done, overload_event and host_output_sample. None of these is in FR-041's relationships or Dependencies, and the dependency line still calls the scheduler 'spec not yet authored'. Add depends_on edges and separate the scheduler-series enablement from the feature work. | spec/functional/FR-041-fleet-saturation.md:182 |
| FND-002 | medium | Steps 8 to 10 cannot be met until ix-relay FR-072 accepts order_id, and ORCH-R02-E1 depends on it, but the change has no ix-relay ticket, requirement id or relationship edge, only prose. File the ix-relay enablement ticket and link it as blocking SWM-257 so the work is not dispatched before it lands. | spec/functional/FR-035-fleet-assign.md:69 |
| FND-003 | low | FR-039's views now depend on tables owned by FR-041 and FR-044, and on hook prompt events (ix-relay FR-068) for session_turn and relay receipts (FR-067, FR-071) for action_send and order_focus_at_deadline; none is in its Dependencies or relationships. FR-036 and FR-037 now write the ledger but have no depends_on FR-039 edge. | spec/functional/FR-039-fleet-ledger.md:164 |

## Verdict

CONDITIONAL. The new upstreams (build scheduler series, the ix-relay FR-072 order_id change, hook prompt events, ledger cross-ownership) are named in prose or not at all, without relationship edges or an enablement ticket. Nothing here blocks the spec, but SWM-257 must not be dispatched before the ix-relay change has a ticket.

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

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | verified in fix commit 4dbc082 |
| FND-002 | fixed | verified in fix commit 4dbc082 |
| FND-003 | fixed | verified in fix commit 4dbc082 |
| FND-004 | fixed | verified in fix commit 65f6b84 (round 2) |

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-004 | low | build_abort evidence assumes ix_build_builds_total carries an outcome label beginning aborted_. dev-team FR-015 (origin/main) lists the outcome label with no value set, and FR-010 defines aborted_<reason> only as an FR-005 termination reason. The coupling is unverified. Have dev-team pin the outcome values in FR-015 (ticket the enablement), or read the termination reason from the FR-005 JSON events. | spec/functional/FR-041-fleet-saturation.md:112 |

## Disposition evidence (round 1)

```yaml
{
 "dispositions": [
  {
   "fnd": "FND-001",
   "outcome": "fixed",
   "after_excerpt": "- Capacity discovery reads build scheduler series from dev-team FR-010 and FR-015 (`ix_build_pause_seconds_total`, `ix_build_admission_blocked_seconds_total`, `ix_build_probe_value`, `ix_build_slots_busy`, `ix_build_slots_sustainable`, `ix_build_queue_wait_seconds`, `ix_build_builds_total{outcome}`), Linear and GitHub for host output, [FR-037](./FR-037-fleet-focus-and-drift.md) focus for per-lane attribution, and the [FR-039](./FR-039-fleet-ledger.md) ledger. `fleet tick` ([FR-038](./FR-038-fleet-tick.md)) writes `drain_done`, `overload_event`, `lane_added`, `lane_removed` and `host_output_sam"
  },
  {
   "fnd": "FND-002",
   "outcome": "fixed",
   "after_excerpt": "- ix-relay change required by SWM-257, ticketed as SWM-259 (blocks SWM-257; SWM-257 is not dispatched before SWM-259 lands): the FR-072 list write SHALL accept an optional `order_id` and each transition record SHALL hold it (FR-072 transition records currently hold operation, lane, writer, version and ticket ids only); FR-071 `--expect-ticket` is already specified. Until it lands, `assign` cannot satisfy steps 8 to 10."
  },
  {
   "fnd": "FND-003",
   "outcome": "fixed",
   "after_excerpt": "- Tables written under [FR-041](./FR-041-fleet-saturation.md) (`capacity_*`, `host_output_sample`) and [FR-044](./FR-044-fleet-cycle-and-dialog-answer-log.md) (`cycle_event`, `dialog_answer`) are owned by those requirements, which depend on this one; the views over them are defined there. [FR-036](./FR-036-fleet-plan-coverage.md) and [FR-037](./FR-037-fleet-focus-and-drift.md) write `plan_coverage_*` and `drift_finding` through this ledger."
  }
 ],
 "new_findings": [
  {
   "fnd": "FND-004",
   "severity": "low",
   "confidence": "high",
   "method": "spec-review/dependency",
   "check": "other",
   "artifact_id": "FR-041",
   "related_ids": [
    "dev-team FR-015",
    "dev-team FR-010"
   ],
   "path": "spec/functional/FR-041-fleet-saturation.md",
   "lines": "112",
   "excerpt": "- **Evidence is checked, not claimed.** `evidence_kind` is set by ix-board, never taken from the command: `pause` or `admission_block` when `ix_build_pause_seconds_total` or `ix_build_admission_blocked_seconds_total` (dev-team FR-010, FR-015) grew in the 15 minutes before the event, `build_abort` when `ix_build_builds_total` gained an outcome beginning `aborted_` (an `aborted_thermal` or `aborted_memory` abort) in that period, `memory_below_reserve` when `ix_build_probe_value` for memory was below the profile reserve in it, `knee` when the host's latest valid `capacity_step` has `knee_signal`,",
   "finding": "build_abort evidence assumes ix_build_builds_total carries an outcome label beginning aborted_. dev-team FR-015 (origin/main) lists the outcome label with no value set, and FR-010 defines aborted_<reason> only as an FR-005 termination reason. The coupling is unverified. Have dev-team pin the outcome values in FR-015 (ticket the enablement), or read the termination reason from the FR-005 JSON events."
  }
 ]
}
```

## Disposition evidence (round 2)

```yaml
{
 "dispositions": [
  {
   "fnd": "FND-004",
   "outcome": "fixed",
   "after_excerpt": "- `aborted_*` outcomes of `ix_build_builds_total{outcome}` (used as `build_abort` evidence) depend on SWM-260, which has dev-team FR-015 enumerate the outcome values; until it lands the `aborted_` prefix of dev-team FR-010 is the contract."
  }
 ],
 "new_findings": []
}
```
