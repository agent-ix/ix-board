---
id: SR-6120
title: ix-board fleet tick code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@f950ed4fd3c0594d9b1e236d454844c2879bfb0b; git diff origin/main...HEAD: src/fleet/tick/{mod,plan,live,settings}.rs, src/fleet/ledger/{mod,tick}.rs, src/fleet/{live,report,mod}.rs, src/main.rs, src/bin/ix-team-exporter.rs, src/team_metrics/{config,tests}.rs, tests/{fleet,fleet_tick}.rs, Cargo.toml, docs/cli-reference.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-038"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-039"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6120: ix-board fleet tick code and Rust review

## Summary

Ticket: SWM-214 (with SWM-256). PR agent-ix/ix-board#95 at head f950ed4.
Code review with the rust-review lane folded in, scoped to
`git diff origin/main...HEAD`. Checked against FR-038, the FR-039 rows the
tick writes, FR-034 STALE-ORDER through `Relay::expired_orders()`, and the
dev-team minder role spec v4 (`spec/roles/minder.md` on origin/main).

## Verdict

Not mergeable as is. FND-001 (high) is the standing no-dead-code rule: the
receipt-history reader (`expired_orders_from`, `collect_receipts`,
`RECEIPT_MAX_PAGES`, `ReceiptRead.gaps`, `ExpiredOrder.gap_spanned`,
`LaneRow.receipt_gap`, `DefectKind::ReceiptsRecordingGap`) is no longer reached
from the live path. Only tests in `tests/fleet.rs` call it. Delete it and its tests.
FND-002 (high): `executed_at` is wrong for `answer_dialog` and
`push_next_ticket`, so MIND-M02 and MIND-R01-E1 evidence is wrong.

What holds: the tick never addresses the owner. Every `Target.role` is one of
`lane`, `lead`, `orchestrator` or `host`, and every address comes from `[fleet_tick]`
configuration or the lane row. The ladder emits rungs 1 to 3 once each at 1, 2
and 3 intervals from `condition_started_at`. It skips rung 1 for a BLOCKED lane (MIND-R19)
and falls through to rung 3 when no lead is configured. Action ids persist by
(kind, subject). Sorting is deterministic. A tick commits in one transaction
and a failed relay read exits 4 with no tick row. The ix-relay pin is v0.4.1
(= ix-relay main). #95 merges cleanly into origin/main. PR #94 already
conflicts with main because it predates #93. When #94 is rebased, its conflicts with #95 are
adjacent additive lines in `src/main.rs` (`FleetCommand`, `Output`,
`output_outcome`, `run`) and the `src/fleet/mod.rs` module list, with no semantic overlap.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Dead receipt-history reader kept after the switch to `Relay::expired_orders()`. `expired_orders_from`, `collect_receipts`, `RECEIPT_MAX_PAGES`, `ReceiptRead.gaps`, `ExpiredOrder.gap_spanned`, `LaneRow.receipt_gap`, the `ReceiptsRecordingGap` defect branch and the unused `_now_ms` parameter are only exercised by tests. The standing no-compat/no-dead-code rule requires deleting them with their tests (tests/fleet.rs 1100-1330) and the `receipts_recording_gap` doc text. | src/fleet/live.rs:315-417, src/fleet/live.rs:426-428, src/fleet/report.rs:127, src/fleet/report.rs:276, src/fleet/report.rs:708-716, src/fleet/report.rs:880-883, tests/fleet.rs:1100-1330, docs/cli-reference.md:25 |
| FND-002 | high | `executed()` marks `answer_dialog`, `resend_order` and `act_on_undetermined` executed whenever their condition clears, and `push_next_ticket` whenever the lane is WORKING. An `answer_dialog` that turns into `escalate_block` because the standing-order file vanished is recorded as executed while the lane is still BLOCKED(dialog). A push is counted as executed when the lane works on a different ticket (FR-039 requires a matching focus). | src/fleet/ledger/tick.rs:112-125, src/fleet/ledger/tick.rs:238-245 |
| FND-003 | medium | A failed FR-072 assignment read only lands in `unavailable`. `complete` is `report.complete`, so the tick exits 0, drops every `push_next_ticket`, and writes `remaining_assigned` NULL with no defect or `fix_source`. This is a failed probe read as healthy (MIND-R02). | src/fleet/tick/mod.rs:245-269, src/fleet/ledger/tick.rs:267-275 |
| FND-004 | medium | `ready_work_available` is written as `false` for a lane with no assigned ticket when ready work is unavailable, which is the always-on live case. Unknown is stored as "no ready work", so the idle-with-ready-work views undercount. It should be NULL while `facts.ready` is `None`. | src/fleet/ledger/tick.rs:171-175 |
| FND-005 | medium | Without `--session`, the tick records `session_addr` NULL. FR-039 Session attribution requires the relay-resolved caller address. | src/fleet/ledger/tick.rs:152-156, src/main.rs:185-189 |
| FND-006 | medium | `act_on_undetermined` targets the lane with `treat_as: idle` even when the lane's FR-034 class is BLOCKED. Executing it means a relay order to a blocked pane, which MIND-R19 forbids. Rung 1 is skipped for BLOCKED lanes, but rung 0 is not. | src/fleet/tick/plan.rs:229-303 |
| FND-007 | low | The `[fleet_tick].planners` key is parsed and stored but never read ("unused until FR-041 lands"). This is dead configuration, and `docs/cli-reference.md` does not list it. | src/fleet/tick/settings.rs:22, src/fleet/tick/settings.rs:49, src/fleet/tick/settings.rs:160-165 |
| FND-008 | low | A `lead_unresolved` rung 2 row that is recorded but not sent still counts in `ix_fleet_tick_actions{kind="ladder_rung"}`, because the metric query counts every ladder row emitted at the last tick. | src/fleet/ledger/tick.rs:356-361 |
| FND-009 | low | The owner check in `rungs_ignore_earlier_messages_end_with_the_condition_and_never_address_the_owner` asserts `target.role != "owner"`. `role` is a `&'static str` from a closed set, so the assertion cannot fail. The structural guarantee holds, but this test does not exercise it. | tests/fleet_tick.rs:653-655 |

## Gates

`CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=<worktree>/target make ci` at f950ed4,
with docker `redis:7` available for `tests/fleet_receipts_redis.rs`: exit 0, 50 test binaries green, including the docker redis:7 receipts test.
target/ was deleted afterwards.
