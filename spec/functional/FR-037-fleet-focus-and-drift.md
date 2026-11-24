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
---
# FR-037: Derive lane focus from hook evidence and feed drift

## Description

ix-board SHALL derive each lane's focus ticket from the worktree directory and git
branch its tool-call hook events touch, never from self-report. It SHALL compute
lane drift from that focus and the lane's assignment, and supply focus and drift
evidence as live inputs to `ix-board drift` ([FR-008](./FR-008-drift-signals.md)).

## Behavior

1. **Focus.** The focus of a lane is the ticket identifier parsed from the branch,
   else from the worktree directory name, of the most recent hook event with a
   `cwd` inside a git worktree, using the configured team keys. Zero or more than
   one distinct identifier among the events in the window yields no focus with a
   reason (`no_ticket`, `ambiguous`, `no_worktree`), never a guess. A parsed
   identifier must resolve in the ix-board cache; otherwise focus is
   `unresolved`.
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

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-037-AC-1 | Hook events with cwd in worktree `ix-board-swm-208` on branch `swm-208-x` set focus SWM-208; branch takes precedence over directory name when they differ. | Test |
| FR-037-AC-2 | Events with no ticket, with two distinct tickets in the window, or with a cwd outside a git worktree yield no focus with the matching reason and no drift finding beyond `no_assignment`. | Test |
| FR-037-AC-3 | Focus outside the assignment gives `not_assigned`; focus outside assignments and ready queue gives `not_in_plan`; a WORKING lane on a lower-ranked ticket while a higher-ranked assigned ticket waits gives `priority_inversion`. | Test |
| FR-037-AC-4 | Self-reported focus text in a message or pane never changes the derived focus. | Test |
| FR-037-AC-5 | A `drift` fixture run supplies lane busy state and branch/PR/commit evidence from fleet evidence, and signal output matches the same fixture supplied through the existing explicit inputs. | Test |
| FR-037-AC-6 | A parsed ticket not in the cache gives `unresolved`; a configured focus differing from derived focus gives `focus_mismatch`. | Test |

## Dependencies

- [FR-034](./FR-034-fleet-lane-status.md) hook events and [FR-035](./FR-035-fleet-assign.md) assignments. ix-relay SWM-201 SHALL provide hook events carrying `cwd` and `branch` (assumed).
