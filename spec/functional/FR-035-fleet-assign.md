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
   before the deadline. Confirmation reads only the hook events after the order was queued (`sent_at`): the focus window of FR-037 is clipped at `sent_at`, so a lane that touched another ticket just before the order is not held `ambiguous` by that earlier work, and the first event at or before the deadline whose derived focus is the first ticket confirms. The command prints one of `confirmed`, `acted_not_focused`,
   `ack_expired` or `timed_out`, where `timed_out` is `AckExpired` with no `Delivered` receipt and exits zero only for `confirmed`.
7. A later `assign` replaces the list; the ledger retains the previous list
   ([FR-039](./FR-039-fleet-ledger.md)).

8. **Expected ticket.** The order SHALL carry `--expect-ticket` equal to the first assigned ticket id (the ix-relay FR-071 field, at most 64 bytes), so the relay stores the expectation with the order and the minder and evaluator compare it with the lane focus. An order whose first ticket id exceeds 64 bytes fails before any send.
9. **Order id on the list write.** The relay identity of the accepted order SHALL be passed as `order_id` on the FR-072 list write, so each assignment-list transition records the order that carried it. A write that is not made by `fleet assign` carries no order id.
10. **Ledger join.** One `assignment_history` row holds one assignment: `lane`, `ticket` (the ordered ticket ids joined by commas) and `assigned_at` (the send time). Before the send the command writes the row (`order_id` empty, `result` `pending`); after the relay accepts the order it records `order_id`, `expect_ticket`, `sent_at` and `deadline_at` and sets `replaced_at` on the lane's earlier rows, which the ledger retains; at the outcome of step 6 it records `result`, `result_at`, `confirm_latency_s` (whole seconds from `sent_at` to the first event whose derived focus equals the first ticket; empty when unconfirmed, which the ledger views count as unbounded and never as the deadline length) and `focus_at_deadline`, the derived focus over the FR-037 window ending at `deadline_at`. A lane confirmed before its deadline leaves `focus_at_deadline` empty. A command killed while waiting leaves `pending`. The first `fleet status` run after the deadline completes every such row from the order's receipts and the lane's events up to the deadline: it sets `result`, `result_at` and `confirm_latency_s` of a `pending` row by the rules of step 6, and sets `focus_at_deadline` of every row that has none ([FR-039](./FR-039-fleet-ledger.md) order focus at the deadline). The ledger therefore joins assign, order, receipt and confirmation by `order_id`. A send failure withdraws the pending row. The row is written to the ledger the `[fleet]` configuration resolves (`IX_BOARD_LEDGER_PATH`, `ledger_path`, the state directory); the command takes the ledger's writer lock only for each write, never while it waits.
11. **Focus keys.** The derived focus reads ticket identifiers by the configured `[fleet] team_keys` and by the team keys of the assigned tickets, and uses the configured `[fleet] focus_window_secs` (default 10m).
12. **Further refusals, before any send.** The command refuses: the caller's own lane (`own_lane`); a lane with no unique registered session (`no_recipient`); an order body over the relay message limit of 4096 bytes, which names the size and asks for fewer tickets (the exact text is never truncated and one assignment is never split into several orders, which would split its receipts and its `order_id`); and ticket text holding a character the relay refuses as unsafe (bidirectional controls, zero-width characters and other control characters except newline and tab).
13. **Failures are reported, never dropped.** A relay read that fails while the command waits is retried on the next poll; the relay clock is carried forward locally meanwhile, the outcome is decided at the deadline from what was read, and the report carries a `relay_read_errors` defect with the count and the last cause. A ledger write that fails after the send is a `ledger_unwritable` defect and a non-zero exit. A ledger that cannot take the pending row before the send fails the command before any send.
14. **Exit codes.** `0`: `confirmed`. `1`: `acted_not_focused`, `ack_expired` or `timed_out`. `3`: a refusal of the request or its configuration (unreadable, closed or duplicate tickets, `not_orchestrator`, `own_lane`, `invalid_lane`, `unknown_lane`, `host_registry_stale`, `no_recipient`, a ticket held by another lane, an oversized or unsafe order, an order spec the relay would refuse). `4`: an operational failure: the relay cannot be read or connected, the send fails, the list write fails after the accepted send, or the ledger cannot be written.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-035-AC-1 | After a confirmed `assign` the relay fake holds the ordered list for the lane address and no `PLAN-*.md` file is read or written. | Test |
| FR-035-AC-2 | The order body carries each ticket's identifier, title, state, description and acceptance criteria byte-for-byte equal to the fixture in order, and carries the acknowledgement deadline. | Test |
| FR-035-AC-3 | A send failure leaves the stored list unchanged and withdraws the pending row; a list-write failure after an accepted send exits with the internal code, names the order and leaves its row in the ledger. | Test |
| FR-035-AC-4 | A ticket in another lane's assignment is refused with that lane named and no send; an unreadable, Done, Canceled or merged ticket fails before any send. | Test |
| FR-035-AC-5 | `Acted` with a focus on another ticket prints `acted_not_focused` and exits non-zero; focus on the first assigned ticket before the deadline prints `confirmed` and exits zero. | Test |
| FR-035-AC-6 | An `AckExpired` receipt after `Delivered` prints `ack_expired`, and an `AckExpired` receipt with no `Delivered` receipt prints `timed_out`; both exit non-zero and leave the ledger row. | Test |
| FR-035-AC-7 | A second `assign` replaces the stored list and the ledger retains both lists with timestamps. | Test |
| FR-035-AC-8 | A caller without the orchestrator role, and a lane the relay marks stale (`host_registry_stale`), each make the command exit non-zero before any send and write no list. | Test |
| FR-035-AC-9 | The relay fake receives the order with `--expect-ticket` equal to the first assigned ticket; a first ticket id of 65 bytes fails before any send. | Test |
| FR-035-AC-10 | The FR-072 list write made by `assign` carries `order_id` equal to the accepted order identity, and the fake relay's transition record for that write holds it; a list write not made by `assign` holds none. | Test |
| FR-035-AC-11 | For each of `confirmed`, `acted_not_focused`, `ack_expired` and `timed_out` the stored `assignment_history` row holds `order_id`, `expect_ticket`, `sent_at`, `deadline_at`, `result`, `result_at`, `confirm_latency_s` and `focus_at_deadline` as computed from the fixture; an unconfirmed row has an empty `confirm_latency_s`; a row confirmed before its deadline gets `focus_at_deadline` from the first run after the deadline. | Test |
| FR-035-AC-12 | A command killed after the send leaves a `pending` row, and the next committed run sets its `result` and `focus_at_deadline` from the receipts and the focus at the deadline. | Test |
| FR-035-AC-13 | An order body over 4096 bytes, ticket text with an unsafe character, the caller's own lane and a lane with several registered sessions are each refused before any send. | Test |
| FR-035-AC-14 | A relay read that fails while waiting does not end the wait and is reported as a `relay_read_errors` defect; a ledger write that fails after the send is a `ledger_unwritable` defect and a non-zero exit; the exit codes are those of behavior 14. | Test |
| FR-035-AC-15 | A lane with an event on another ticket before the order that then works the first ticket reaches `confirmed`; focus uses the configured team keys, and `focus_at_deadline` is the focus at the deadline, not at the confirming event. | Test |
| FR-035-AC-16 | The receipt read spans relay retention and follows the page cursor until none remains; it is bounded at `RECEIPT_EVENT_CAP / RECEIPT_PAGE_CAP + 1` pages and fails when the bound is reached; a strict coverage reason fails the read while the remaining reasons are reported as gaps. | Test |

## Dependencies

- A new ix-relay requirement (PR #52) SHALL own an ordered ticket list per lane address, writable by `fleet assign` and readable by ix-board. Until it exists `fleet assign` cannot store lists.
- ix-relay change required by SWM-257, ticketed as SWM-259 (blocks SWM-257; SWM-257 is not dispatched before SWM-259 lands): the FR-072 list write SHALL accept an optional `order_id` and each transition record SHALL hold it (FR-072 transition records currently hold operation, lane, writer, version and ticket ids only); FR-071 `--expect-ticket` is already specified. Until it lands, `assign` cannot satisfy steps 8 to 10.
- Orders with an acknowledgement deadline, and `Delivered`/`Acted`/`AckExpired` receipts (ix-relay). Ticket text is read through the [FR-001](./FR-001-ready-queue.md) and [FR-017](./FR-017-batch-issue-read.md) readers.
