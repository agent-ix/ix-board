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

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-006 | low | The FR-039 view table lists `idle_lane_episodes_with_ready_work`, `lane_tick_rows` and `lane_span` twice (identical or near-identical rows 161-163 and 164-166). | spec/functional/FR-039-fleet-ledger.md:161-166 |
| FND-007 | low | The follow-up mapping leaves three deferrals without a ticket: AC-8's ground-truth samples go to "FR-040 owner" (no SWM ticket found for calibrate), AC-6 (`executed_at`) and AC-16 (`cycle_missed`) are listed as "not in this PR" with no ticket. SWM-214 (tick) and SWM-255 (which lists orchestrator cycle events) are the natural owners of AC-6 and AC-16. | spec/functional/FR-039-fleet-ledger.md:181-191 |

## Dispositions

Round 1, fix head 69af642.

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | FR-039 amended: `lane_tick` has `source` (`status`, `tick`) with a CHECK, AC-11 and the Run rows rule say both commands write it, and `lane_tick_rows` keeps one row per lane per `observed_ms` with the tick row winning; `lane_tick_source_and_dedup_use_one_row_per_run_time` covers the dedup. |
| FND-002 | deferred | AC-1, 6, 8, 10, 11, 12-16 are untagged until their writers land: SWM-214 (tick, actions, time-to-action), SWM-211 (assign), SWM-212 (plan coverage), SWM-213 (drift), SWM-255 (ledger persistence of coverage, drift, assign results, rungs, cycles), SWM-252 (capacity), SWM-258 (answer log). The AC-10 formula examples are now asserted by `open_cycles_matches_the_fr_039_examples`. |
| FND-003 | still-open | The unit-test partial tags are removed, but tests/fleet_cli.rs:58 still tags `status_writes_the_ledger_and_query_reads_the_defect_without_writing` with FR-039-AC-7, which reads `defect_rows` only, not idle minutes or actions with time-to-action; the matrix still reports AC-7 tagged. Remove that tag (or test idle minutes and actions) to match the PR body. |
| FND-004 | fixed | `the_default_path_is_under_the_state_directory_and_outside_any_work_tree` checks both XDG and HOME defaults pass `refuse_inside_git`, the real default is outside the checkout, and a state dir inside a work tree is refused. |
| FND-005 | fixed | FR-039 now names `host_tick` and the run columns in Behavior and lists `idle_lane_episodes_with_ready_work`, `lane_tick_rows` and `lane_span` in the view table (duplicated, see FND-006). |
