---
id: FR-035
title: Assign an ordered ticket list to a lane through relay work data
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-037"
    type: depends_on
---
# FR-035: Assign an ordered ticket list to a lane through relay work data

## Description

The `ix-board fleet assign <lane> <ticket>... [--deadline <duration>]` command SHALL
send the exact text of the assigned tickets to the lane as a relay order with an
acknowledgement deadline, store the ordered ticket list in the relay assignment
record for that lane address, and report whether the lane began the first ticket.
The command SHALL NOT store assignments in `PLAN-*.md` or any prose file.

## Behavior

1. The command validates every ticket against the ix-board cache and fails before
   any send when a ticket is unreadable, Done, Canceled or merged.
2. The command refuses a ticket already present in another lane's assignment and
   names that lane. The command fails before any send.
3. The command sends one relay order whose body contains, per ticket in order, the
   identifier, title, state, description and acceptance criteria exactly as read,
   plus the queue position. The order carries the deadline (default 5m, `--deadline`
   overrides) so the relay applies `AckExpired` (also to an order never delivered).
4. The command writes the list through the ix-relay FR-072 assignment list. FR-072 accepts writes only from a caller with the orchestrator role (an accidental-write guard, not authentication) and refuses a lane on a stale host with `host_registry_stale`. The command SHALL check both conditions before sending, report a refusal and exit non-zero without sending.
5. The command stores the list only after the relay accepts the order. A send
   failure writes no list. A list-write failure after an accepted send exits
   non-zero, names the sent order and records it in the ledger.
6. The command waits until the deadline. `Acted` means only that the session acted;
   assignment is confirmed only when the lane's derived focus
   ([FR-037](./FR-037-fleet-focus-and-drift.md)) equals the first assigned ticket
   before the deadline. The command prints one of `confirmed`, `acted_not_focused`,
   `ack_expired` or `timed_out`, where `timed_out` is `AckExpired` with no `Delivered` receipt and exits zero only for `confirmed`.
7. A later `assign` replaces the list; the ledger retains the previous list
   ([FR-039](./FR-039-fleet-ledger.md)).

8. **Expected ticket.** The order SHALL carry `--expect-ticket` equal to the first assigned ticket id (the ix-relay FR-071 field, at most 64 bytes), so the relay stores the expectation with the order and the minder and evaluator compare it with the lane focus. An order whose first ticket id exceeds 64 bytes fails before any send.
9. **Order id on the list write.** The relay identity of the accepted order SHALL be passed as `order_id` on the FR-072 list write, so each assignment-list transition records the order that carried it. A write that is not made by `fleet assign` carries no order id.
10. **Ledger join.** Before the send the command writes an `assignment_history` row (`order_id` empty, `result` `pending`); after the relay accepts the order it records `order_id`, `expect_ticket`, `sent_at` and `deadline_at`; at the outcome of step 6 it records `result`, `result_at`, `confirm_latency_s` (send to the first run whose derived focus equals the first ticket; empty when unconfirmed, which the ledger views count as unbounded and never as the deadline length) and `focus_at_deadline`. A command killed while waiting leaves `pending`; the first later committed run completes the row from receipts and the focus at the deadline ([FR-039](./FR-039-fleet-ledger.md) order focus at the deadline). The ledger therefore joins assign, order, receipt and confirmation by `order_id`.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-035-AC-1 | After a confirmed `assign` the relay fake holds the ordered list for the lane address and no `PLAN-*.md` file is read or written. | Test |
| FR-035-AC-2 | The order body carries each ticket's identifier, title, state, description and acceptance criteria byte-for-byte equal to the fixture in order, and carries the acknowledgement deadline. | Test |
| FR-035-AC-3 | A send failure leaves the stored list unchanged; a list-write failure after an accepted send exits non-zero, names the order and records it. | Test |
| FR-035-AC-4 | A ticket in another lane's assignment is refused with that lane named and no send; an unreadable, Done, Canceled or merged ticket fails before any send. | Test |
| FR-035-AC-5 | `Acted` with a focus on another ticket prints `acted_not_focused` and exits non-zero; focus on the first assigned ticket before the deadline prints `confirmed` and exits zero. | Test |
| FR-035-AC-6 | An `AckExpired` receipt after `Delivered` prints `ack_expired`, and an `AckExpired` receipt with no `Delivered` receipt prints `timed_out`; both, exit non-zero and leave the ledger entry. | Test |
| FR-035-AC-7 | A second `assign` replaces the stored list and the ledger retains both lists with timestamps. | Test |
| FR-035-AC-8 | A caller without the orchestrator role, and a lane the relay marks stale (`host_registry_stale`), each make the command exit non-zero before any send and write no list. | Test |
| FR-035-AC-9 | The relay fake receives the order with `--expect-ticket` equal to the first assigned ticket; a first ticket id of 65 bytes fails before any send. | Test |
| FR-035-AC-10 | The FR-072 list write made by `assign` carries `order_id` equal to the accepted order identity, and the fake relay's transition record for that write holds it; a list write not made by `assign` holds none. | Test |
| FR-035-AC-11 | For each of `confirmed`, `acted_not_focused`, `ack_expired` and `timed_out` the `assignment_history` row holds `order_id`, `expect_ticket`, `sent_at`, `deadline_at`, `result`, `result_at`, `confirm_latency_s` and `focus_at_deadline` as computed from the fixture; an unconfirmed row has an empty `confirm_latency_s`. | Test |
| FR-035-AC-12 | A command killed after the send leaves a `pending` row, and the next committed run sets its `result` and `focus_at_deadline` from the receipts and the focus at the deadline. | Test |

## Dependencies

- A new ix-relay requirement (PR #52) SHALL own an ordered ticket list per lane address, writable by `fleet assign` and readable by ix-board. Until it exists `fleet assign` cannot store lists.
- ix-relay change required by SWM-257, ticketed as SWM-259 (blocks SWM-257; SWM-257 is not dispatched before SWM-259 lands): the FR-072 list write SHALL accept an optional `order_id` and each transition record SHALL hold it (FR-072 transition records currently hold operation, lane, writer, version and ticket ids only); FR-071 `--expect-ticket` is already specified. Until it lands, `assign` cannot satisfy steps 8 to 10.
- Orders with an acknowledgement deadline, and `Delivered`/`Acted`/`AckExpired` receipts (ix-relay). Ticket text is read through the [FR-001](./FR-001-ready-queue.md) and [FR-017](./FR-017-batch-issue-read.md) readers.
