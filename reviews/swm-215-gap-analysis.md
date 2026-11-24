---
id: SR-6117
title: ix-board fleet ledger gap analysis against FR-039
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@1cf3cfe979407f308a7b5abf82b0f7d97482708e; spec/functional/FR-039-fleet-ledger.md, src/fleet/ledger/mod.rs, src/fleet/ledger/schema.rs, src/fleet/ledger/tests.rs, src/fleet/report.rs, src/main.rs, tests/fleet_cli.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-039"
    type: reviews
---

# SR-6117: ix-board fleet ledger gap analysis against FR-039

## Summary

Ticket: SWM-215. PR agent-ix/ix-board#93 at 1cf3cfe. Planless gap analysis
scoped to the PR diff and FR-039. Plan completion: not assessed.
`quoin matrix --repo . --json` (quoin 0.36.10) reports FR-039-AC-1, 2, 3, 4,
5, 7, 8, 9, 11 and 15 `tagged` and AC-6, 10, 12, 13, 14 and 16 `untagged`
(no run evidence bound). The semantic review was not run (reviewer subagent,
no opt-in); the tagged tests were read against their criteria by hand.

The lead's decision for the AC-11 conflict is that `fleet status` writes
`lane_tick` until `fleet tick` exists, with FR-039-AC-11 amended in this PR so
`lane_tick` carries a `source` column (`status` or `tick`) and the views use
all rows deduplicated by run time. The PR changes no spec file.

## Verdict

FAIL. Six criteria are untagged (declared as follow-up scope), the code
contradicts FR-039-AC-11 as written and the lead's amendment is not made,
and four tagged criteria are tagged by tests that cover only part of them.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-039-AC-11 is not amended and the code contradicts it. The spec still says `fleet status` writes no `lane_tick` row and the table is written by `fleet tick` only; the code writes `lane_tick` from every status run and the AC-11 test is tagged against that. Per the lead decision the PR must amend AC-11 and the Behavior table, add a `source` column (`status`, `tick`) to `lane_tick`, and deduplicate rows by run time in the views; none of the three is done (no `source` column, views partition over all rows with no dedup). | spec/functional/FR-039-fleet-ledger.md:75, spec/functional/FR-039-fleet-ledger.md:113-114, src/fleet/ledger/schema.rs:66-72, src/fleet/ledger/schema.rs:170-230 |
| FND-002 | medium | Six criteria are untagged: FR-039-AC-6, AC-10, AC-12, AC-13, AC-14, AC-16. The PR names them as follow-up scope (SWM-214, 211, 212, 213, 255, 252, 258). AC-10's `open_cycles` SQL matches its examples when evaluated directly, but no test asserts the 4, 24, 25, 26 and 36 minute cases. | spec/functional/FR-039-fleet-ledger.md:181-190 |
| FND-003 | medium | Coverage inflation on four tagged criteria. AC-1's test asserts idle minutes only; time-to-action is not computed anywhere. AC-7's tests return idle minutes and defects, not actions with time-to-action. AC-8's tests cover version refusal and retention, not replaced assignments, defects and ground-truth samples stored with timestamps. AC-15's test covers `defect_rows` only, of the whole view table. Each is tagged as if the criterion were met. | src/fleet/ledger/tests.rs:82-100, src/fleet/ledger/tests.rs:229-264, src/fleet/ledger/tests.rs:266-317, src/fleet/ledger/tests.rs:466-489 |
| FND-004 | low | AC-5's default-path test asserts the suffix `ix-board/fleet-ledger.sqlite3` only; it never checks that the default path is outside a git work tree, which is what AC-5 states. | src/fleet/ledger/tests.rs:222-226 |
| FND-005 | low | Reverse gap: the `host_tick` table and the `idle_lane_episodes_with_ready_work` and `lane_span` views have no owning requirement in FR-039; the PR names `host_tick` as a deviation but the spec is not updated. | src/fleet/ledger/schema.rs:74-77, src/fleet/ledger/schema.rs:176-206 |

## Coverage

- FR-039 criteria: 16; tagged 10, untagged 6; no run evidence bound.
- Tagged and fully exercised: AC-2, AC-3, AC-4, AC-9. Tagged and partial:
  AC-1, AC-7, AC-8, AC-15 (FND-003), AC-5 (FND-004), AC-11 (contradicts the
  spec, FND-001).
- Stubs: none on live paths; placeholder tables have no writer or reader.
- Semantic review: skipped.
- Plan completion: not assessed
