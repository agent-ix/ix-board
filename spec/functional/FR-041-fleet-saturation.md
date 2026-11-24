---
id: FR-041
title: Compute fleet saturation and the ready-buffer deficit
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-001"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-036"
    type: depends_on
---
# FR-041: Compute fleet saturation and the ready-buffer deficit

## Description

The `ix-board fleet saturation [--json]` command SHALL compute supply (sustainable
build slots and lane capacity), demand (lanes needed to saturate those slots), and
the ready-buffer deficit, and `fleet tick` SHALL turn a positive deficit into a
`need_ready_tickets` action routed to planners by project. It exports the gauge
`ix_fleet_ready_buffer_deficit`.

## Behavior

- **Supply** per host: `sustainable_slots` from the build scheduler's measured
  headroom, and `lane_capacity` from the host's `server-capacity` setting.
- **Duty cycle** per lane: build-process seconds divided by observed lane seconds
  over a trailing window (default 24h), from the scheduler and hook-attributed
  evidence. A lane without enough observed time has duty `unavailable`.
- **Demand**: `lanes_needed = ceil(sustainable_slots / mean duty)` per host,
  bounded by `lane_capacity`.
- **Ready buffer**: the count of ready tickets ([FR-001](./FR-001-ready-queue.md)) not yet started, assigned or
  unassigned, across the configured code-lead projects. Target buffer is
  `lanes_needed x queue_depth` (default 2). `deficit = max(0, target - buffer)`.
- Each input that cannot be read makes the dependent value `unavailable`, with its
  reason; a deficit is never reported as zero from missing data, and the gauge is
  omitted.
- A positive deficit becomes one `need_ready_tickets` action per owning planner
  project with its share: shares are allocated to projects in proportion to their
  lanes' count, remainder to the project with the smallest buffer, ties by project id.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-041-AC-1 | Fixtures with known slots, capacity and duty yield the hand-computed `lanes_needed`, target buffer and deficit; `lanes_needed` never exceeds `lane_capacity`. | Test |
| FR-041-AC-2 | A buffer at or above target gives deficit zero and no action; one below gives the exact shortfall. | Test |
| FR-041-AC-3 | A missing scheduler reading, capacity setting, duty window or ready queue makes dependent values `unavailable`, omits the gauge, and never emits deficit zero. | Test |
| FR-041-AC-4 | A deficit of N produces per-project `need_ready_tickets` actions whose counts sum to N, deterministically for fixed inputs. | Test |
| FR-041-AC-5 | The gauge `ix_fleet_ready_buffer_deficit` is exposed with bounded labels only and matches the command output. | Test |
| FR-041-AC-6 | Each run stores a `saturation_sample` row in the ledger. | Test |

## Dependencies

- A build scheduler exposing sustainable slots and build seconds per lane (ix-build W6 spec, not yet authored; assumed interface). Host capacity from `agent-msg server-capacity`. Lane build duty also uses [FR-034](./FR-034-fleet-lane-status.md) evidence.
