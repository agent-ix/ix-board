---
id: FR-035
title: Assign an ordered ticket list to a lane through relay work data
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
---
# FR-035: Assign an ordered ticket list to a lane through relay work data

## Description

The `ix-board fleet assign <lane> <ticket>... [--timeout <duration>]` command SHALL
store the ordered ticket list as the lane's structured `work agent` assignment in
ix-relay, send each assigned ticket's exact text in a relay message to the lane,
and wait for the delivery receipt `Acted`. Assignment SHALL NOT be stored in
`PLAN-*.md` or any prose file.

## Behavior

1. The command validates every ticket against the ix-board cache and fails before
   any write when a ticket is unreadable, Done, Canceled or already merged.
2. It writes the ordered list to the lane's relay `work agent` record. A later
   `assign` replaces the list; the previous list is retained in the ledger
   ([FR-039](./FR-039-fleet-ledger.md)).
3. The message body contains, per ticket in order, the identifier, title, state,
   description and acceptance criteria exactly as read from the tracker, plus the
   queue position. No ticket is referenced by a vague phrase such as "next ready".
   A lane without tracker authentication can work from the message alone.
4. The command waits until the receipt is `Acted`, `TimedOut`, or `--timeout`
   (default 5m) expires. It prints the final receipt state, exits non-zero for
   anything but `Acted`, and records the order and each state change in the ledger.
5. A relay write failure is reported and the stored list is not claimed as sent.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-035-AC-1 | After `assign`, the relay fake holds the ordered list in `work agent` for the lane and no `PLAN-*.md` file is read or written. | Test |
| FR-035-AC-2 | The sent message contains each ticket's identifier, title, state, description and acceptance criteria byte-for-byte equal to the fixture, in assignment order, and no tracker call is needed to interpret it. | Test |
| FR-035-AC-3 | The command returns zero only when the fake relay reports `Acted`; `Delivered` without `Acted` until timeout exits non-zero and prints the timeout visibly. | Test |
| FR-035-AC-4 | An unreadable, Done, Canceled or merged ticket makes the command fail before any relay write or message. | Test |
| FR-035-AC-5 | A relay write or send failure exits non-zero with the failing step named and leaves no sent-claim in the ledger. | Test |
| FR-035-AC-6 | A second `assign` replaces the stored list and the ledger retains both lists with timestamps. | Test |

## Dependencies

- ix-relay SWM-201 SHALL provide the `work agent` write/read interface and the `Acted` receipt (assumed).
- Ticket text is read through the existing FR-001/FR-017 readers; ix-board does not write Linear.
