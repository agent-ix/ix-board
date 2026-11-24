---
id: SR-6115
title: ix-board live STALE-ORDER receipt reader gap analysis against FR-034
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@7ffbd757c7741749674544de37e28bc7ebc76d87; spec/functional/FR-034-fleet-lane-status.md, src/fleet/live.rs, src/fleet/report.rs, src/main.rs, docs/cli-reference.md, tests/fleet.rs, tests/fleet_receipts_redis.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6115: ix-board live STALE-ORDER receipt reader gap analysis against FR-034

## Summary

Ticket: SWM-266. PR agent-ix/ix-board#90 at 7ffbd75. Planless gap analysis
scoped to the PR diff and FR-034. Plan completion: not assessed.
`quoin matrix --repo . --json` reports all 21 FR-034 criteria `tagged`.
FR-034-AC-12 is bound to eight tests, three of them new in this PR
(`recorded_ack_expired_receipts_give_expired_orders_per_lane_address`,
`an_undelivered_order_without_an_expiry_receipt_is_never_stale`,
`ack_expired_orders_are_read_from_a_real_relay_with_and_without_delivery`).
FR-034-AC-8 stays bound to three tests. Untagged criteria elsewhere in the
repository predate this PR and are out of scope. No stubs, `todo!` or
`unimplemented!` in the diff. Semantic review was not run as a separate pass;
the AC-12 and Inputs checks below were read against the code.

## Verdict

CONDITIONAL. AC-12 ("STALE-ORDER only when the relay recorded `AckExpired`,
no recomputed deadline") is met and traced. The gaps are a test that encodes
behaviour FR-034 forbids, a receipt-gap behaviour the lead has ruled on but
FR-034 does not state, and a 48h read bound that no requirement owns.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | `recorded_ack_expired_receipts_give_expired_orders_per_lane_address` is tagged FR-034-AC-12 and asserts that an `AckExpired` receipt for an unregistered queue is dropped. FR-034 Inputs says a record without a resolvable address is a source defect, and AC-17 requires a run-level defect for a registration with no resolvable address. No test checks that such an order surfaces. | tests/fleet.rs:1077-1097, spec/functional/FR-034-fleet-lane-status.md:25 |
| FND-002 | medium | FR-034 and the CLI reference do not say how a receipt-history gap (`RecordingGap`, `ObservationPending`, count `Retention`) is reported. The lead ruling (run-level `receipts_recording_gap` defect, lanes marked when their `timed_out` derivation spans the gap) needs a spec statement, a documented defect kind and a test; none exists. | spec/functional/FR-034-fleet-lane-status.md:30-32, docs/cli-reference.md:487 |
| FND-003 | low | The 48h receipt read bound is new behaviour with no owning requirement; only docs/cli-reference.md states it. It decides which expired orders are visible, so it belongs in FR-034 (see SR-6114 FND-003 for the defects it causes). | src/fleet/live.rs:293-296, docs/cli-reference.md:487 |

## Coverage

- Plan completion: not assessed
- FR-034 criteria: 21 of 21 tagged (quoin matrix, static).
- New tests: 3, all tagged FR-034-AC-12; all pass in `make ci` at 7ffbd75.
- Reverse gap: `RelayOrderReceipts`, `read_expired_orders`, `expired_orders_from` trace to FR-034 (STALE-ORDER rule 3, AC-8, AC-12); the 48h bound does not (FND-003).
- Semantic review: not run as a separate fan-out pass.

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-004 | low | The second clause of FR-034-AC-22 (an unresolved `AckExpired` order adds an `unreadable_source` defect) is asserted only by `an_expired_order_to_an_unregistered_recipient_is_a_defect_row`, which is tagged FR-034-AC-12 and FR-034-AC-17, not AC-22. | tests/fleet.rs:1122-1147 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | - |
| FND-002 | fixed | - |
| FND-003 | fixed | - |
