---
id: SR-6118
title: ix-board fleet assign code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@ecba3db7f07a4fc9897a510c3f9a9aeec0fe4a6c; Cargo.toml, Cargo.lock, src/fleet/assign.rs, src/fleet/focus.rs, src/fleet/live.rs, src/fleet/mod.rs, src/lib.rs, src/main.rs, tests/fleet_assign.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-035"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-037"
    type: reviews
---

# SR-6118: ix-board fleet assign code and Rust review

## Summary

Ticket: SWM-211 (also the ix-board half of SWM-257). PR agent-ix/ix-board#94
at ecba3db. Code review with the rust-review lane folded in, scoped to
`git diff origin/main...HEAD` (merge base 5af3141).

Gates run in the PR worktree (CARGO_BUILD_JOBS=4, target inside the worktree,
docker redis:7 for the relay tests): `make ci` (fmt-check, clippy -D warnings,
test, deny, audit-unsafe) at ecba3db on the d3027cf (v0.4.1) relay pin. Result
recorded in the Coverage section.

Lead questions answered:

- Ledger. origin/main has the FR-039 ledger (541c6c9) with the
  `assignment_history` columns FR-035 needs (`order_id`, `expect_ticket`,
  `sent_at`, `deadline_at`, `result`, `result_at`, `confirm_latency_s`,
  `focus_at_deadline`) but no writer for them. This PR should implement
  `AssignLedger` for the ledger and pass it from `run_fleet_assign`
  (FND-002). No `action` write is needed from assign: FR-039 reads
  orchestrator follow-ups from `action_send`, which `fleet tick` writes.
- Conflict. The branch conflicts with main in src/fleet/mod.rs and
  src/main.rs (FND-003); both are additive.
- Order size. Refusing an order over 4096 bytes is consistent with FR-035
  behavior 3 ("one relay order"); splitting would create several orders with
  separate receipts and break the order_id join. Measured Linear description
  sizes: SWM-200..275 median 436 B, p90 622 B, max 1282 B; QSL-1000..1060
  median 545 B, p90 1008 B, max 1888 B. Three tickets at p90 plus about 100 B
  of header each fit under 4096 B, so refusal does not make assign unusable
  in practice. The bound is unspecified in FR-035; see SR-6119.
- Wait. A 2 s poll is fine. The loop does read `AckExpired` from receipts and
  ends early on it; otherwise it decides at its own deadline (Queued time plus
  deadline), which matches the relay's. No finding beyond FND-004.
- AC-8 fake bus. Acceptable: the stale flag comes from relay-core's
  `fleet_registry`, and the test exercises ix-board's decision on it.

## Verdict

Request changes. The send, list write, order_id join and refusals are sound
and well tested against a real relay. But the confirmation check cannot
succeed for a lane that touched another ticket within the 10-minute focus
window, which is the usual case for a lane being handed new work under the
5-minute default deadline. The result is also recorded nowhere although the
ledger is on main, the branch conflicts with main, and a single relay read
error ends the wait with a wrong result.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | `confirmed` is unreachable for a lane that touched another ticket within the focus window. `wait_for_outcome` confirms only when `focus::derive` over the full 10-minute window returns `Ticket(first)`. A lane with an event on SWM-999 at t-60s that switches to SWM-211 after the order at t stays `Ambiguous` until t+540s, past the 5-minute default deadline, so it prints `acted_not_focused` (exit 1) while it is working the assigned ticket. The tests start every lane with no prior events, so they miss it. Fix: confirm on events since `sent_at` only, or have FR-035 state how the window is clipped. | src/fleet/assign.rs:601-618, src/fleet/focus.rs:120-150, src/main.rs:875-876 |
| FND-002 | medium | Assign results are recorded nowhere. `run_fleet_assign` passes `NoLedger`, and every report carries `LEDGER_SEAM` ("fleet ledger (SWM-215) is not on this build"), which is false since 541c6c9. The `assignment_history` columns exist on main, but nothing writes them. After the send, `let _ = ledger.record(..)` discards every ledger error, including the list-write-failure row that FR-035 behavior 5 requires. `confirm_latency_s` is `f64` in the report and `INTEGER` in the schema. | src/fleet/assign.rs:27, src/fleet/assign.rs:541, src/fleet/assign.rs:565-572, src/main.rs:880 |
| FND-003 | medium | The branch conflicts with origin/main (541c6c9): `git merge-tree` reports content conflicts in src/fleet/mod.rs (`pub mod focus` against `pub mod ledger`) and src/main.rs (`FleetCommand::Assign` against `FleetCommand::Ledger`, `Output::FleetAssign` against `Output::FleetLedger`, the run and render arms). Both sides are additive. The merged result has not been built or tested. | src/fleet/mod.rs:13, src/main.rs:185, src/main.rs:385, src/main.rs:660, src/main.rs:1300 |
| FND-004 | medium | A transient relay read error ends the wait with a wrong result. `bus.registry().map_or(deadline_at, ..)` treats a failed registry read as the deadline, so one Redis blip decides the outcome immediately as `timed_out`, `ack_expired` or `acted_not_focused` with `result_at` set to the deadline. Receipt and lane-event read errors are dropped (`if let Ok`, `unwrap_or_default`), so the same pass may also miss `Delivered` or `Acted`. A read error should skip the iteration, and repeated errors should be reported. | src/fleet/assign.rs:590-601 |
| FND-005 | medium | Focus uses the assigned tickets' team keys, not the configured team keys (FR-037 behavior 1). `ticket_keys(&report.tickets)` means a lane on QSL-5 is seen as `no_ticket`, and `focus_at_deadline` records `no_ticket` instead of QSL-5. A lane with events on both QSL-5 and SWM-211 in the window is `confirmed`, although FR-037 makes it `ambiguous`. | src/fleet/assign.rs:387-395, src/fleet/assign.rs:587 |
| FND-006 | low | On the confirmed path, `focus_at_deadline` is the focus at confirmation time: `deadline_at.min(now)`, where `now` is before the deadline. FR-035 behavior 10 and the field name mean the focus at the deadline. | src/fleet/assign.rs:615-616 |
| FND-007 | low | The `ticket_in` doc says "the number must end it", but the code does not check the character after the digits, so `swm-211abc` gives SWM-211. `derive` returns `no_worktree` only when both cwd and branch are empty, not when the cwd is outside a git worktree (FR-037 behavior 1), so cwd `/tmp` with no branch gives `no_ticket`. | src/fleet/focus.rs:68-107, src/fleet/focus.rs:133-140 |
| FND-008 | low | Relay and send failures exit 3 (`invalid`). `Error::Assign(_)` maps every `AssignError` to `Outcome::Invalid`, including `Send`, `Relay` and `Ledger`, which are runtime failures, not bad input or configuration. | src/main.rs:505 |
| FND-009 | low | docs/cli-reference.md does not document `ix-board fleet assign`: arguments, `--deadline`, the four outcomes, exit statuses, refusals and the orchestrator config key. | docs/cli-reference.md:477-487 |

## Coverage

- Rust gates: `make ci` at ecba3db, see the line below.
- Tests read against behavior: `fleet_assign.rs` exercises relay-core's real
  send, order, receipt, activity and FR-072 paths on redis:7. The fake bus is
  used only for role, own-lane and stale refusals.
- Safety: no `unsafe`, no panics on caller input (`issues[0]` is guarded by the
  non-empty check), no unbounded loop (the wait ends on the relay clock).
- `make ci`: make ci exit 0 at ecba3db on the d3027cf pin (fmt-check, clippy -D warnings, 50 test binaries green incl. fleet_assign 8/8 on redis:7, cargo deny, unsafe audit); the merge with main is not built (FND-003).

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-010 | high | Test helper restates ix-cli-kit's exit codes. `code()` in the `fleet_assign_output` tests maps `Outcome` to 0/1/3/4 by hand (`_ => 2`) instead of calling `Outcome::code()`, so the AC-5/AC-14 exit-status assertions check this copy, not the codes the binary returns; a change in ix-cli-kit's mapping would leave them green. Use `output_outcome(&output).code()`. | src/main.rs:2707-2716 |
| FND-011 | low | Pending-row completion is wired only into `fleet status` and only tested by calling `complete_pending` directly: `complete_assignments` is not exercised through `run_fleet_status`, and its `assignment completion` defect is added after `record_status` has committed the run, so that defect never reaches the ledger's defect rows. `fleet tick` (#95, SWM-214) also commits runs and does not call it, so a fleet that runs only `tick` leaves killed rows `pending`. | src/main.rs:860-884, src/fleet/ledger/assign_rows.rs:166-171 |

## Dispositions

Round 1, fix head ebb3323 (rebased onto 541c6c9).

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | - |
| FND-002 | fixed | - |
| FND-003 | fixed | - |
| FND-004 | fixed | - |
| FND-005 | fixed | - |
| FND-006 | fixed | - |
| FND-007 | fixed | - |
| FND-008 | fixed | - |
| FND-009 | fixed | - |

Round 2, fix head aaa4e1e.

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-010 | fixed | - |
| FND-011 | fixed | - |
