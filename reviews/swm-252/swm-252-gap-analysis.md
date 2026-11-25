---
id: SR-6151
title: "Gap analysis of SWM-252 capacity history"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; spec/functional/FR-039-fleet-ledger.md, spec/functional/FR-041-fleet-saturation.md, src/fleet, tests"
review_set: subset
---

## Summary

Ticket: SWM-252. Head 31f2fa12dbae323729db69775d7749796a19d5e4. quoin matrix and a tag search show no test bound to any new or capacity criterion, and most capacity behaviour of FR-041 has no implementation.

## Verdict

**FAIL** - untagged criteria and unimplemented behaviour.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-039-AC-17, FR-039-AC-18 and FR-041-AC-10 to AC-23 have no tagged test; src/fleet/ledger/capacity/tests.rs carries no trace attribute. | src/fleet/ledger/capacity/tests.rs:87 |
| FND-002 | high | FR-039-AC-17 claims a crash injected during the version 2 migration leaves a version 2 ledger intact. apply_v3 and apply_v4 take no failure point and no test starts from a real version 2 ledger. | src/fleet/ledger/mod.rs:210 |
| FND-003 | medium | The version 3 migration test drops a table from a new version 4 file and seeds no rows, so preservation of previous rows in FR-039-AC-18 is never asserted. | src/fleet/ledger/capacity/tests.rs:219 |
| FND-004 | medium | The only end-to-end test injects a fake source with invented source names such as node_exporter series and config limit paths, so it passes while no production code reads any of them; it asserts no slot values, knee or repeat. | src/fleet/ledger/capacity/tests.rs:23 |
| FND-005 | high | saturation_sample, the sole input of window validity, has no writer in the repository, so even with a measurement source every real window would be invalid with deficit_coverage until SWM-217 lands. | src/fleet/ledger/schema.rs:36 |
| FND-006 | high | Unimplemented FR-041 behaviour: output sum and lane attribution (AC-10), observed lane changes (AC-19, AC-22, AC-23), derived evidence_kind (AC-20), drain_done and drain cap (AC-15), step_down and cool-off (AC-14), role-spec views (AC-17), queue wait p90, admission-only throttled share and headroom against limits. | spec/functional/FR-041-fleet-saturation.md:112 |

## Coverage

Plan completion: not assessed

Semantic review (intent, test, code) was not run as a separate pass. quoin matrix --repo . --json was run; criteria examined are the FR-039 and FR-041 criteria listed above.
