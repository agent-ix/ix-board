---
id: SR-6103
title: "spec-review/base review of ix-board fleet spec (PR86, SWM-208)"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@31ad309efb0118fa89173ee7a17dfa4eb4048fa8; spec/functional/FR-008, FR-018, FR-033, FR-034..FR-043, spec/spec.md, spec/usecase/US-011"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-038"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-041"
    type: reviews
---

# SR-6103: ix-board fleet spec review (base, intent, integrity, EARS)

## Summary

Ticket: SWM-208 (also SWM-209..219, SWM-210). Spec-only PR #86, head
31ad309. Reviewed against the plan's Context, W2 and W6 saturation sections
and the lead's six decisions on the author's open questions. Integrity and
EARS checks are folded in; `quire validate` passes every changed file, with
grammar warnings only. Contract checks against ix-relay PR #52 are in
SR-6105 (dependency analysis); failure modes are in SR-6104.

## Verdict

Changes requested. The spec covers every W2 bullet and its structure is
sound: one FR per command, testable ACs, explicit unavailable-not-zero rules,
and read-only Linear (consistent with decisions 2, 3 and 5). Three places
contradict the lead's decisions: the WORKING rule and its AC (decision 1)
and the ready-buffer population (decision 4). FR-038 also leaves the stale-PR
owner and threshold and the standing-order source undefined (decision 6).

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-034 WORKING requires hook activity AND an independent signal, and adds IDLE(unconfirmed_activity); decision 1 says hook tool-use/prompt events in the window alone classify WORKING and drops unconfirmed_activity. | spec/functional/FR-034-fleet-lane-status.md:56-62 |
| FND-002 | high | FR-034-AC-2 tests the contradicted rule: hook activity without compile/PR/commit must be IDLE(unconfirmed_activity), not WORKING. | spec/functional/FR-034-fleet-lane-status.md:84 |
| FND-003 | medium | FR-034 LOOP requires "no independent signal for loop_after"; decision 1 bases LOOP on repeated wait patterns, so the independent-signal condition should go. | spec/functional/FR-034-fleet-lane-status.md:53-55 |
| FND-004 | high | FR-041 ready buffer counts ready tickets "not yet started, assigned or unassigned"; decision 4 is ready, unassigned and unstarted only. | spec/functional/FR-041-fleet-saturation.md:32-34 |
| FND-005 | medium | FR-038 escalate_stale_pr hardcodes "more than 3h" (not a configurable default) and targets "the PR owner"; decision 6 makes the owner the PR's lane lead. | spec/functional/FR-038-fleet-tick.md:37 |
| FND-006 | medium | FR-038 answer_dialog depends on "a configured standing order" with no named source; decision 6 says standing orders come from the SWM-226 config file. The dependency and the behaviour when that file is absent or unreadable are missing. | spec/functional/FR-038-fleet-tick.md:34 |
| FND-007 | medium | FR-038 emits `escalate_block` (rule text and AC-3), but the kinds table does not define it: no condition row, target or severity. | spec/functional/FR-038-fleet-tick.md:34,54 |
| FND-008 | medium | FR-038/FR-033-AC-14: tick is a CLI run, the gauges are served by the scraped exporter, and nothing says how the exporter learns the last successful tick (for example from the FR-039 ledger) or when the value counts as stale. | spec/functional/FR-033-team-flow-effort-export.md:42,63 |
| FND-009 | medium | FR-038 add_lane compares busy ratio with "its target", which no input or configuration defines. | spec/functional/FR-038-fleet-tick.md:36 |
| FND-010 | medium | FR-037 focus is taken from "the most recent hook event" but goes ambiguous when the window holds two tickets. The window is undefined, and a lane that switches tickets loses its focus for the whole window. | spec/functional/FR-037-fleet-focus-and-drift.md:24-30 |
| FND-011 | medium | FR-037 item 4 "has that focus set from this evidence" is a write to an unnamed focus store. That conflicts with spec.md's out-of-scope line that only `fleet assign` writes, and conflates lane ticket focus with ix-board focus selections. | spec/functional/FR-037-fleet-focus-and-drift.md:41-42 |
| FND-012 | medium | FR-039 `action.first_seen_executed_at` has no evidence source: tick does not execute actions, and no rule says how execution is observed. | spec/functional/FR-039-fleet-ledger.md:27-28 |
| FND-013 | low | FR-034-AC-9 expects byte-identical JSON apart from the run timestamp, but rows carry state age, which depends on run time. | spec/functional/FR-034-fleet-lane-status.md:90 |
| FND-014 | low | quire grammar warnings: FR-034 line 65 missing subject, line 75 non-singular and agentless passive; FR-043 line 17 agentless passive. | spec/functional/FR-034-fleet-lane-status.md:65,75 |

## Scope

- FR-034 (examined), FR-034-AC-1..AC-9 (examined)
- FR-035 (examined), FR-035-AC-1..AC-6 (examined)
- FR-036 (examined), FR-036-AC-1..AC-6 (examined)
- FR-037 (examined), FR-037-AC-1..AC-6 (examined)
- FR-038 (examined), FR-038-AC-1..AC-7 (examined)
- FR-039 (examined), FR-039-AC-1..AC-7 (examined)
- FR-040 (examined), FR-040-AC-1..AC-7 (examined; consistent with decisions 2 and 3)
- FR-041 (examined), FR-041-AC-1..AC-6 (examined)
- FR-042 (examined), FR-042-AC-1..AC-5 (examined)
- FR-043 (examined), FR-043-AC-1..AC-4 (examined)
- FR-018 SWM-210 additions, FR-018-AC-27..AC-29 (examined, clean)
- FR-008 live-input bullet (examined, clean); FR-033 addition (examined)
- US-011 (examined, clean); spec/spec.md scope and out-of-scope additions (examined)
