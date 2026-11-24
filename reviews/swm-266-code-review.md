---
id: SR-6114
title: ix-board live STALE-ORDER receipt reader code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@7ffbd757c7741749674544de37e28bc7ebc76d87; Cargo.toml, Cargo.lock, docs/cli-reference.md, src/fleet/live.rs, src/main.rs, tests/fleet.rs, tests/fleet_receipts_redis.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6114: ix-board live STALE-ORDER receipt reader code and Rust review

## Summary

Ticket: SWM-266. PR agent-ix/ix-board#90 at 7ffbd75. Code review with the
rust-review lane folded in, scoped to `git diff origin/main...HEAD`. The
relay-core side was read at the pinned rev 28297d4, which is ix-relay main
(compare ahead_by 0): `receipts.rs`, `store/receipts.rs` (`receipt_export`,
`populate_export`, `read_receipts`), `store.rs` send path, and the Lua
`receipt_core.lua`, `receipts.lua`, `order_expire.lua`.

Checked and clean:

- Queue to lane mapping. The receipt `target` is the recipient's canonical
  `AgentName` (store.rs:636-684). `order_expire.lua` writes the order's
  `recipient` as `target` on `AckExpired`, and `Acted` carries the same queue.
  Keying `lanes` on `reg.name` and mapping to `reg.full_address()` matches
  `read_relay` (live.rs:39), so the join in report.rs:702 lines up. The docker
  redis:7 test proves it end to end through `send_as`, `observe_delivery` and
  `expire_orders`.
- No local deadline logic. `RECEIPT_WINDOW_MS` is only the read interval; the
  expiry decision is the relay's `AckExpired` record.
- Strict failure for `SourceUnavailable`, `CursorEvicted` and
  `CursorUnavailable` is in place, and `make ci` passes at 7ffbd75.

## Verdict

Request changes. The happy path is correct and well tested against a real
relay. Two required behaviours are missing: the lead's ruling on
`RecordingGap` (one run-level `receipts_recording_gap` defect plus marking the
`timed_out` lanes whose derivation spans the gap) is not implemented, and
`AckExpired` orders whose recipient has no registered address are dropped
silently instead of surfacing as defects. The 48h read window also produces a
false `timed_out` for expiries recorded 24h to 48h ago, and stops showing an
expired, never-acted order after 48h.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | A receipt-history `RecordingGap` is ignored silently. The lead ruling requires one run-level `receipts_recording_gap` defect and marking each lane whose `timed_out` derivation spans the gap, since a lost `Delivered` receipt turns into a false `timed_out`. Neither exists (no such `DefectKind`, no lane marking). `ObservationPending` and `Retention` within the 48h window carry the same false-`timed_out` or missed-expiry risk and are also dropped; `page.complete` is never read. | src/fleet/live.rs:378-391 |
| FND-002 | high | An `AckExpired` receipt whose `target` queue has no registered lane with a verified address is dropped without a defect (`if let Some(addr)`; unverified registrations are filtered out by `full_address()?`). FR-034 Inputs says a record without a resolvable address is a source defect. The fixture test asserts the drop (`receipt("d", AckExpired, "gone")`). | src/fleet/live.rs:323-327, src/fleet/live.rs:360-367, tests/fleet.rs:1094 |
| FND-003 | medium | The 48h window is wrong at both ends. A relay order deadline runs from enqueue up to `ORDER_ACK_MAX_MS` (24h), so an `AckExpired` observed 24h to 48h ago can have its `Delivered` before `since_ms`, giving a false `timed_out`; the doc comment claims the window prevents this. And an `AckExpired` with no `Acted` older than 48h falls out of the read, so the lane stops being STALE-ORDER although the order was never acted on; FR-034 sets no such age bound. | src/fleet/live.rs:293-296, src/fleet/live.rs:372-377 |
| FND-004 | medium | Paging cost. Every `receipt_export` call re-reads the whole stream (`XRANGE` up to 10,001 entries plus `HGETALL` of the receipt meta hash, which holds one `event:` field per retained event) in an atomic pipeline and decodes every event, then returns 1,000. A full 48h window costs up to 10 full reads per run (about 100k JSON decodes and 200k items over the wire) on the shared production relay. The reader also opens a second connection and repeats `load_core`, `agents()` and `lane_lists()` already done by `read_relay`, and `expired_orders_from` scans all events twice per `AckExpired` (O(E x A)). `RECEIPT_MAX_PAGES = 20` is unreachable given `RECEIPT_EVENT_CAP / RECEIPT_PAGE_CAP = 10`. | src/fleet/live.rs:299, src/fleet/live.rs:311-316, src/fleet/live.rs:339-346, src/fleet/live.rs:369-397 |
| FND-005 | medium | The strict-failure and paging branches are untested: no test drives `SourceUnavailable`, `CursorEvicted`, `CursorUnavailable`, a multi-page read, or the page cap, and none covers a `RecordingGap` page. The redis test covers only one page with no coverage reasons. | src/fleet/live.rs:378-401, tests/fleet_receipts_redis.rs:110-154 |

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-006 | medium | `Retention` is a run-level gap on every run once the stream has hit its 10,000-event cap. The read now starts at `now - RECEIPT_WINDOW_MS`, and relay-core raises the retention boundary to `evicted_through_ms + 1`. Any count eviction in the last 7 days therefore sets `Retention`, so `receipts_recording_gap` fires and every run is partial. On top of that, `receipt_gap` marks every `timed_out` row whenever any gap exists. The lead ruling asks to mark only lanes whose `timed_out` derivation spans the gap. Under `Retention` alone, an order whose `Queued` receipt is still in view cannot have lost its later `Delivered` to head eviction, and it is still marked. | src/fleet/live.rs:381-390, src/fleet/live.rs:416-428, src/fleet/report.rs:672-684, src/fleet/report.rs:817-819 |
| FND-007 | low | Each unresolved expired order adds its own `unreadable_source` defect. The read now spans 7 days, so expired orders to sessions that have since restarted keep the run partial for up to 7 days, one defect row each, with no aggregation. | src/fleet/report.rs:686-697 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | - |
| FND-002 | fixed | - |
| FND-003 | fixed | - |
| FND-004 | deferred | The ix-board side is fixed: one shared connection, single-pass matching, page bound `RECEIPT_EVENT_CAP / RECEIPT_PAGE_CAP + 1`. The per-page full-stream re-read is in relay-core `receipt_export` and is deferred to SWM-273, a read-only expired-order reader. Measured on a throwaway redis:7 holding 10,000 events of about 300 bytes: about 1.8 ms of Redis time and about 2 MB on the wire per page. A full read is 11 pages, about 20 ms of Redis time and about 22 MB per run. At one run every 5 minutes that is about 6 GB a day. This is acceptable as an interim until SWM-273 lands. |
| FND-005 | fixed | - |
