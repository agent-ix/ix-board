---
id: FR-037
title: Derive lane focus from hook evidence and feed drift
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-008"
    type: constrains
  - target: "ix://agent-ix/ix-board/FR-035"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
---
# FR-037: Derive lane focus from hook evidence and feed drift

## Description

ix-board SHALL derive each lane's focus ticket from the worktree directory and git
branch its tool-call hook events touch, never from self-report. It SHALL compute
lane drift from that focus and the lane's assignment, and supply focus and drift
evidence as live inputs to `ix-board drift` ([FR-008](./FR-008-drift-signals.md)).

## Behavior

1. **Focus.** The focus window is configurable (default 10m). The focus of a lane is the ticket identifier parsed from the branch, else from the worktree directory name, of its most recent hook event with a `cwd` inside a git worktree, using the configured team keys. When events inside the window show two or more distinct tickets, the lane is `ambiguous`: the report carries the most recent ticket and the full list, and the drift rules are not evaluated for it. No ticket, or a `cwd` outside a worktree, yields no focus with reason `no_ticket` or `no_worktree`. A parsed identifier absent from the ix-board cache is `unresolved`. Focus is computed on read; ix-board keeps no focus store.
2. **Drift** is one or more of: `not_assigned` (focus not in the lane's
   assignment), `not_in_plan` (focus not in any lane's assignment or the ready
   queue), `priority_inversion` (lane WORKING while a higher-ranked assigned ticket
   waits and the lane's focus is not that ticket). Each finding carries lane, focus,
   rule and evidence times. A lane with no assignment record yields `no_assignment`
   and no other rule.
3. **Live inputs.** The fleet evidence supplies the `herdr_busy`-equivalent lane
   busy state, the lane's attributed work, and GitHub branch/PR/commit evidence
   used by `drift` where those inputs are otherwise absent. An input absent from
   fleet evidence stays absent; it is not synthesized.
4. A lead whose focus is derivable has that focus set from this evidence; a lead
   with a manually configured focus has the conflict reported as `focus_mismatch`.

5. **Drift history.** Each `fleet tick` SHALL record every drift finding in the ledger ([FR-039](./FR-039-fleet-ledger.md)) as a `drift_finding` row (lane, rule, focus, evidence times), opened by the first tick that reports it and closed by the first later tick in which the lane has no finding of that rule for that focus. A lane whose focus is `ambiguous` or without focus has its open findings left open and gains none. The lane-minutes with a finding are those between `opened_at` and `closed_at` in which the lane's ledger interval is `WORKING`.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-037-AC-1 | Hook events with cwd in worktree `ix-board-swm-208` on branch `swm-208-x` set focus SWM-208; branch takes precedence over directory name when they differ. | Test |
| FR-037-AC-2 | Events within the window naming two distinct tickets give `ambiguous` with the most recent ticket and the list and no drift finding; a switch to a new ticket older than the window yields the new focus; no ticket and a cwd outside a worktree give `no_ticket` and `no_worktree`. | Test |
| FR-037-AC-3 | Focus outside the assignment gives `not_assigned`; focus outside assignments and ready queue gives `not_in_plan`; a WORKING lane on a lower-ranked ticket while a higher-ranked assigned ticket waits gives `priority_inversion`. | Test |
| FR-037-AC-4 | Self-reported focus text in a message or pane never changes the derived focus. | Test |
| FR-037-AC-5 | A `drift` fixture run supplies lane busy state and branch/PR/commit evidence from fleet evidence, and signal output matches the same fixture supplied through the existing explicit inputs. | Test |
| FR-037-AC-6 | A parsed ticket not in the cache gives `unresolved`; a configured focus differing from derived focus gives `focus_mismatch`; no focus store is created or written. | Test |
| FR-037-AC-7 | A `not_assigned` finding present in ticks 1 and 2 and absent in tick 3 gives one `drift_finding` row opened at tick 1 and closed at tick 3; a different rule or focus on the same lane gives its own row. | Test |
| FR-037-AC-8 | A lane that becomes `ambiguous` keeps its open finding open and gains no new row; the drift lane-minutes share over a fixture equals the hand-computed value counting only `WORKING` minutes. | Test |

## Dependencies

- [FR-034](./FR-034-fleet-lane-status.md) hook events and [FR-035](./FR-035-fleet-assign.md) assignments. ix-relay PR #52 SHALL provide hook events carrying `cwd` and `branch` through relay-core (target contract, ix-relay PR #52).
