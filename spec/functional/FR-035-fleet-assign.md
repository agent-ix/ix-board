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
   overrides) so the relay applies `AckExpired`, and never-delivered `TimedOut`.
4. The command stores the list only after the relay accepts the order. A send
   failure writes no list. A list-write failure after an accepted send exits
   non-zero, names the sent order and records it in the ledger.
5. The command waits until the deadline. `Acted` means only that the session acted;
   assignment is confirmed only when the lane's derived focus
   ([FR-037](./FR-037-fleet-focus-and-drift.md)) equals the first assigned ticket
   before the deadline. The command prints one of `confirmed`, `acted_not_focused`,
   `ack_expired` or `timed_out` and exits zero only for `confirmed`.
6. A later `assign` replaces the list; the ledger retains the previous list
   ([FR-039](./FR-039-fleet-ledger.md)).

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-035-AC-1 | After a confirmed `assign` the relay fake holds the ordered list for the lane address and no `PLAN-*.md` file is read or written. | Test |
| FR-035-AC-2 | The order body carries each ticket's identifier, title, state, description and acceptance criteria byte-for-byte equal to the fixture in order, and carries the acknowledgement deadline. | Test |
| FR-035-AC-3 | A send failure leaves the stored list unchanged; a list-write failure after an accepted send exits non-zero, names the order and records it. | Test |
| FR-035-AC-4 | A ticket in another lane's assignment is refused with that lane named and no send; an unreadable, Done, Canceled or merged ticket fails before any send. | Test |
| FR-035-AC-5 | `Acted` with a focus on another ticket prints `acted_not_focused` and exits non-zero; focus on the first assigned ticket before the deadline prints `confirmed` and exits zero. | Test |
| FR-035-AC-6 | An `AckExpired` receipt and a never-delivered `TimedOut` receipt print `ack_expired` and `timed_out` respectively, exit non-zero and leave the ledger entry. | Test |
| FR-035-AC-7 | A second `assign` replaces the stored list and the ledger retains both lists with timestamps. | Test |

## Dependencies

- A new ix-relay requirement (PR #52) SHALL own an ordered ticket list per lane address, writable by `fleet assign` and readable by ix-board. Until it exists `fleet assign` cannot store lists.
- Orders with an acknowledgement deadline, and `Acted`/`AckExpired`/`TimedOut` receipts (ix-relay). Ticket text is read through the [FR-001](./FR-001-ready-queue.md) and [FR-017](./FR-017-batch-issue-read.md) readers.
