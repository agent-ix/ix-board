---
id: SR-6121
title: ix-board fleet tick gap analysis
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@f950ed4fd3c0594d9b1e236d454844c2879bfb0b; FR-034, FR-038, FR-039 (rows written by fleet tick); src/fleet/tick/**, src/fleet/ledger/tick.rs, src/fleet/live.rs, tests/fleet_tick.rs, tests/fleet.rs; alignment with agent-ix/dev-team spec/roles/minder.md v4 (origin/main)"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-038"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-039"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6121: ix-board fleet tick gap analysis

## Summary

Ticket: SWM-214 (with SWM-256). PR agent-ix/ix-board#95 at head f950ed4.
This is a planless audit over the PR's requirements. The matrix comes from `quire matrix --scope .
--format json`; quire 0.52.1 with spec-artifacts-process@v0.29.2. The
reverse gap and stub checks cover the diff. Named seams were checked against
Linear SWM ownership and FR-038/FR-039 text.

## Verdict

FAIL. FR-038-AC-1..18 are all tagged and their tests assert the ladder,
owners, BLOCKED rung-1 skip, standing-order seam, add_lane suppression and
the tick metric. Three things fail the PR:

- FR-039-AC-6, AC-11 and AC-14 cover rows this PR now writes (`executed_at`,
  `lane_tick` source `tick` with `ready_work_available`, `tick.session_addr`)
  and are untagged.
- FR-034-AC-23 and part of AC-22 are satisfied only by tests of the dead
  receipt-history reader, which is coverage inflation.
- MIND-R17/R18 routing (DEAD lane, IDLE with empty list) has no FR-038 action
  and no owning ticket.

Seam ownership:

- SWM-226 owns the standing-order format, and FR-038 marks it.
- Ready work belongs to SWM-212 (plan coverage) and `need_ready_tickets` to
  SWM-217 (FR-041).
- Host busy ratio has no owning ticket. Lane PR activity has none either:
  SWM-269 is adjacent but does not name the `escalate_stale_pr` source.
- `action_send`, `cycle_missed`, `order_focus_at_deadline` and `session_turn`
  belong to SWM-255.
- FR-038 marks none of the unwired sources. Only docs/cli-reference.md does.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-039-AC-6 (`executed_at`), FR-039-AC-11 (`lane_tick` source `tick` and `ready_work_available`) and FR-039-AC-14 (tick session attribution) are untagged. The PR adds the writers for all three with no test. SR-6120 FND-002/004/005 show each one is also wrong in code. | src/fleet/ledger/tick.rs:112-125, src/fleet/ledger/tick.rs:152-189, spec/functional/FR-039-fleet-ledger.md |
| FND-002 | high | Coverage inflation: FR-034-AC-23 (paged receipt read, cursor, page bound) is tagged only on `receipt_pages_are_followed_by_cursor_and_the_read_is_bounded`, which tests the dead `collect_receipts`. Three FR-034-AC-22 tests test the dead `expired_orders_from` and its `receipts_recording_gap` path, which the live reader no longer raises. AC-23 and the recording-gap clause of AC-22 describe behaviour the shipped code no longer has. | tests/fleet.rs:1149-1330, src/fleet/live.rs:315-417, spec/functional/FR-034-fleet-lane-status.md:136-137 |
| FND-003 | medium | Minder spec v4 MIND-R17/R18 (route a DEAD lane, and an IDLE lane with an empty FR-072 list, to the orchestrator as rung 0 tick actions) has no FR-038 action kind, no producer and no owning SWM ticket. FR-038 does not mark it deferred. | spec/functional/FR-038-fleet-tick.md:32-48, src/fleet/tick/plan.rs:503-539 |
| FND-004 | medium | `need_ready_tickets` is an FR-038 action kind with no `ActionKind` variant. FR-038-AC-1 ("each action kind is emitted by its fixture condition") is tagged anyway. SWM-217 owns it, but FR-038 does not mark the deferral. | spec/functional/FR-038-fleet-tick.md:47, src/fleet/tick/mod.rs:28-53, tests/fleet_tick.rs:180-210 |
| FND-005 | medium | Host busy ratio, lane PR activity and ready work are permanently `unavailable` live, so `add_lane` and `escalate_stale_pr` (SWM-214 acceptance: "add lane when host busy below target", "stale PR >3h") never fire. FR-038 does not mark these seams, and busy ratio and PR activity have no owning ticket. | src/fleet/tick/live.rs:44-62, spec/functional/FR-038-fleet-tick.md:40-41 |
| FND-006 | medium | `act_on_undetermined` (MIND-R01, treat as idle) is emitted for a lane whose class is BLOCKED, which conflicts with MIND-R19 (no relay message to a BLOCKED lane). Neither FR-038 nor the role spec resolves the overlap. | src/fleet/tick/plan.rs:229-303, spec/functional/FR-038-fleet-tick.md:54 |
| FND-007 | low | MIND-R02 requires a failed probe to be routed in the tick it appears. FR-038 `fix_source` waits `source_defect_age` (default 30m), and a failed tick-source read produces no defect at all. The spec does not reconcile the two timings. | src/fleet/tick/plan.rs:305-340, spec/functional/FR-038-fleet-tick.md:48 |
| FND-008 | low | FR-039 lists `action_send`, `cycle_missed`, `order_focus_at_deadline` and `session_turn` as written by `fleet tick`. Nothing writes them yet. SWM-255 owns them, but neither FR-039 nor the PR marks them as unproduced. | spec/functional/FR-039-fleet-ledger.md |

## Coverage

- Matrix (FR-038): 18/18 criteria tagged. Bindings checked: AC-1, 2, 3, 4, 5,
  6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17 and 18 against tests/fleet_tick.rs.
  AC-12's owner assertion is vacuous (SR-6120 FND-009). AC-8 exercises only
  `TickFacts::read` with a fake, not the live `run`.
- Matrix (FR-034): AC-1..23 tagged. AC-22 and AC-23 are inflated (FND-002).
  The live `expired_orders` mapping is tested by
  `a_truncated_or_skipped_expired_order_list_is_a_defect_and_keeps_the_rows`
  and by tests/fleet_receipts_redis.rs against docker redis:7.
- Matrix (FR-039): tagged AC-2, 3, 4, 5, 9. Untagged AC-1, 6, 7, 8, 10, 11, 12,
  13, 14, 15, 16. AC-6/11/14 are in this PR's scope (FND-001). The rest belong to
  SWM-215/SWM-255 and predate this diff (context only).
- Reverse gap: `[fleet_tick].planners` has no owning behaviour (SR-6120 FND-007).
  Exporter `fleet_ledger`/`fleet_tick_interval_seconds` is owned by FR-038 (metric).
- Stubs: `LiveSources::host_busy`, `lane_prs` and `ready_work` return a fixed
  `SourceError`. They are declared seams, and the output reports them as
  `unavailable` (FND-005).
- Semantic review: skipped (not requested).
- Plan completion: not assessed
