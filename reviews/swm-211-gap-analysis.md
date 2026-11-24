---
id: SR-6119
title: ix-board fleet assign gap analysis against FR-035 and FR-037
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@ecba3db7f07a4fc9897a510c3f9a9aeec0fe4a6c; spec/functional/FR-035-fleet-assign.md, spec/functional/FR-037-fleet-focus-and-drift.md, src/fleet/assign.rs, src/fleet/focus.rs, src/main.rs, tests/fleet_assign.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-035"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-037"
    type: reviews
---

# SR-6119: ix-board fleet assign gap analysis against FR-035 and FR-037

## Summary

Ticket: SWM-211 (also the ix-board half of SWM-257). PR agent-ix/ix-board#94
at ecba3db. A planless gap analysis scoped to the PR diff, FR-035 and the
focus part of FR-037. Plan completion was not assessed.

`quoin matrix --repo . --json` (quoin 0.36.10) results:

- FR-035-AC-1 to AC-11 are `tagged`, all in tests/fleet_assign.rs.
- FR-035-AC-12 is `untagged`.
- FR-037-AC-1 to AC-8 are all `untagged`.
- No run evidence is bound.

The semantic review was not run, because this is a reviewer subagent with no
opt-in. The tagged tests were read against their criteria by hand.

FR-039 (541c6c9, on main) owns the `assignment_history` schema, including
every FR-035 column. SWM-255 lists "fleet assign results ... in
assignment_history" as its scope, and the SWM-215 review deferred FR-039
assign persistence to SWM-211 and SWM-255. SWM-257's third criterion ("Ledger
joins assign -> order -> receipt -> confirmation") is not met by this PR.

## Verdict

FAIL. FR-035-AC-12 is untagged and unimplemented. Four FR-035 criteria are
tagged by tests that check an in-memory recorder, not the ledger they name.
The FR-037 focus rules implemented here carry no trace tags. FR-035 and FR-037
also conflict on the confirmation window.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-035-AC-12 is untagged and unimplemented. No `pending` row is written to the ledger, and no later committed run completes it from receipts and the focus at the deadline. The ledger schema is on main, so the PR either implements this (the writer, plus the completion step in the `fleet status` and tick run) or names SWM-255 as owner and states the deferral. SWM-257's ledger-join criterion depends on it. | spec/functional/FR-035-fleet-assign.md:64, src/main.rs:880 |
| FND-002 | medium | Coverage inflation on FR-035-AC-3, AC-6, AC-7 and AC-11. Each criterion names the ledger: "records it", "leave the ledger entry", "the ledger retains both lists with timestamps", "the `assignment_history` row holds". The tagged tests assert a test-local `Recorder` that the production command never uses (`NoLedger`). AC-11's tests also leave most of the row unasserted for `acted_not_focused`, `ack_expired` and `timed_out`: `sent_at`, `deadline_at`, `result_at` and `focus_at_deadline`. Either wire the FR-039 ledger and assert the stored rows, or remove the tags. | tests/fleet_assign.rs:272-383, tests/fleet_assign.rs:386-460, tests/fleet_assign.rs:497-549 |
| FND-003 | medium | FR-035 behavior 6 and FR-037 behavior 1 conflict. Confirmation is "derived focus equals the first ticket" over the 10-minute focus window, under a 5-minute default deadline. For any lane that touched another ticket in the previous 10 minutes, the derived focus is `ambiguous` for the whole deadline, so `confirmed` is unreachable. FR-035 should state that confirmation considers events after `sent_at` only, or otherwise how the window is clipped. See SR-6118 FND-001 for the code. | spec/functional/FR-035-fleet-assign.md:38-43, spec/functional/FR-037-fleet-focus-and-drift.md:26 |
| FND-004 | medium | The FR-037 focus rules in this PR have untagged tests. `fleet::focus` implements branch-over-directory precedence, the `ambiguous` window and `no_ticket`/`no_worktree` (AC-1, part of AC-2). Its unit tests carry no `#[trace]`, so the matrix shows FR-037 entirely untagged. `unresolved` (part of AC-6) is not implemented. Tag the tests for what they cover, and name the owner of the rest of FR-037. | src/fleet/focus.rs:152-201 |
| FND-005 | low | Behavior with no owning requirement. Four refusals are not in FR-035: an order body over the relay's 4096-byte limit (assign fewer tickets), ticket text with an unsafe character, the caller's own lane, and a lane with zero or several registered sessions (`no_recipient`). Refusing an oversized order is the right reading of behavior 3's "one relay order", and measured ticket sizes make three tickets fit routinely (see SR-6118). FR-035 should state the bound and the refusal. | src/fleet/assign.rs:428-488 |
| FND-006 | low | FR-035-AC-5 says the command "prints" `acted_not_focused` or `confirmed` and exits zero only for `confirmed`. The tests check the library `AssignResult` only. No CLI test checks the printed text or the exit status mapping in `output_outcome`. | tests/fleet_assign.rs:386-412, src/main.rs:525-527 |

## Coverage

- FR-035 criteria: 12; tagged 11, untagged 1 (AC-12); no run evidence bound.
- Tagged and fully exercised against a real relay (redis:7): AC-1, AC-2,
  AC-4, AC-8 (fake bus for the registry refusals), AC-9 and AC-10.
- Tagged but partial: AC-3, AC-6, AC-7 and AC-11 (ledger replaced by a test
  recorder, FND-002), and AC-5 (library result only, FND-006).
- FR-037 criteria: 8; all untagged. AC-1 and part of AC-2 are implemented in
  this PR (FND-004). AC-3 to AC-8 (drift, live inputs, drift history) are
  outside this PR.
- Stubs: `NoLedger` is a no-op on the production path (SR-6118 FND-002).
- SWM-211's ticket criterion "the matching _lead-bin shell scripts are
  replaced" is outside this repository and was not assessed.
- Semantic review: skipped.
- Plan completion: not assessed
