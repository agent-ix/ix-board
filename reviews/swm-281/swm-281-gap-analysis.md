---
id: SR-6131
title: ix-board live host busy ratio gap analysis
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@07c69b89183b2960ecb84076ad5147e5157e2e74; SWM-281, FR-034, FR-038; src/fleet/{live,report}.rs, src/fleet/tick/live.rs, tests/fleet.rs, tests/fleet_tick.rs, docs/cli-reference.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-038"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6131: ix-board live host busy ratio gap analysis

## Summary

Ticket: SWM-281. PR agent-ix/ix-board#98 at head
07c69b89183b2960ecb84076ad5147e5157e2e74 (base 8bf053d). Planless audit scoped
to the diff and the ticket's four asks: wire the live source, remove the
`unavailable` entry, add a live-path test, update the FR-038 deferral. The
matrix comes from `quire matrix --scope . --format json` (quire 0.52.1).

## Verdict

**CONDITIONAL**. The source is read live and the unwired entry is gone from the
live tick. The live-path test does not cover the live path, and FR-038 and the
CLI reference still describe the reader as missing.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The live path is not exercised. The only new tick test builds `LiveSources` by hand over a fake `SeriesSource`: no `PrometheusSeries`, no query, no `run()`. Verified by mutation: replacing the wiring in `run()` with `series: None, hosts: Vec::new()` leaves lib (255), fleet (35), fleet_cli (3) and fleet_tick (30) tests all passing. The one CLI tick test exits 4 before reaching `LiveSources`. Drive canned Prometheus bodies through `PrometheusSeries`, the report's host list and `TickFacts` to an `add_lane` result. | src/fleet/tick/live.rs:285-299 |
| FND-002 | medium | None of the new refusals is tested: out-of-range or NaN value, two samples for one host, a host with no ratio in `host_busy`, and no Prometheus URL. Any of the branches could accept the value or be deleted without a failing test. | src/fleet/live.rs:270-279 |
| FND-003 | medium | The FR-038 deferral is not updated, which the ticket asks for. The `add_lane` row still says the busy-ratio reader is owned by SWM-281 and is reported `unavailable` until it lands. No requirement states the new behaviour: which hosts are read, what a missing, invalid, ambiguous or stale ratio does, and whether it is a `tick.host_busy` defect. The PR changes no file under spec/. | spec/functional/FR-038-fleet-tick.md:40 |
| FND-004 | medium | The CLI reference says host busy ratio has no reader yet, prints under `unavailable` as a named seam and raises no defect. The code now reads it and raises `tick.host_busy` when it fails. | docs/cli-reference.md:509 |
| FND-005 | low | The new test is tagged FR-038-AC-6, which is about emitting `add_lane` below the target and not at it. The test asserts only the host-to-ratio map, so the matrix counts it as live coverage of an AC it does not check. | src/fleet/tick/live.rs:286 |
| FND-006 | low | A fake tick source still returns `Unread::Unwired("SWM-281")`, naming this delivered ticket as the owner of an unbuilt reader. | tests/fleet_tick.rs:1225-1227 |

## Coverage

- FR-038 AC-1 to AC-22: all tagged. AC-6 binds the new test (FND-005) and the
  existing planner fixture test.
- FR-034 AC-17: the new series name is covered by the empty-result test.
  FR-034-AC-23 is untagged on the base as well; outside this diff, not a finding.
- Reverse gap: the busy-ratio refusal and range rules have no owning
  requirement (FND-003).
- Stubs: none in the diff. `lane_prs` (SWM-282) and `ready_work` (SWM-212)
  remain named seams, so `add_lane` still cannot fire live until SWM-212.
- Semantic review: skipped.
- Plan completion: not assessed

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-007 | medium | FR-038-AC-23 says a bad busy sample does not discard the host's status evidence, and no test asserts it. The new test cans empty blocked, probe and config results, so `assert!(invalid.config.working_window.is_none())` holds whatever the code does, and no assertion checks that blocked reasons, probes or thresholds survive beside an invalid or duplicate sample. | tests/fleet.rs:710-739 |

## Dispositions

Round 1, head 336d282879937eb6ddb49566bb27b7b0f55679d8.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The new test drives `PrometheusSeries`, `build_report`, `TickFacts` and `plan` to one `add_lane` for draco. The single assignment in `run()` stays uncovered: replacing it with an empty map leaves the tests green. |
| FND-002 | still-open: part of the refusals is still untested | NaN, duplicate, missing and stale now have tests. Mutation on this head: dropping the `[0, 1]` range check and emptying both the invalid/duplicate and the missing `prometheus.host_busy` defect arms leaves lib (255), fleet (37), fleet_cli (3) and fleet_tick (30) passing. Needed: an out-of-range sample such as 1.5, and a `build_report` assertion of the defect for an invalid and a missing sample. |
| FND-003 | fixed | The FR-038 row, a behaviour paragraph and AC-23 state the new behaviour. |
| FND-004 | fixed | The CLI reference describes the reader and the host-scoped defect. |
| FND-005 | fixed | The replacement test asserts the `add_lane` result and is tagged AC-6 and AC-23. |
| FND-006 | fixed | The fake returns an empty map. |

Round 2, head ba5546dbe9224ffb67dcbca130089df242ce1a24.

| FND | outcome | reason |
| --- | --- | --- |
| FND-002 | fixed | Out-of-range 1.5 and the invalid and missing defects are asserted. Mutations dropping the range check or the host_busy defect now fail the fleet tests. |
| FND-007 | fixed | Blocked reason, probe and working window are asserted beside an invalid sample. A mutation returning early after the invalid sample now fails. |
