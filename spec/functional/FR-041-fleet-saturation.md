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
the ready-buffer deficit. The command SHALL export the gauge
`ix_fleet_ready_buffer_deficit`, and `fleet tick` SHALL emit a `need_ready_tickets`
action per owning planner project for a positive deficit.

## Behavior

- **Supply** per host: `sustainable_slots` and `lane_capacity`. The final source of
  `sustainable_slots` is the build scheduler (locked-build, W6). Until it exists, the
  interim source is the host's `server-capacity` build slots. Lane capacity comes from
  `server-capacity`. A host whose registry lease is stale
  ([FR-034](./FR-034-fleet-lane-status.md)) contributes no slots and no lane capacity.
- **Duty cycle** per lane: build seconds divided by observed lane seconds over a
  trailing window (default 24h). The final source is locked-build; the interim
  source is the watcher's per-lane compiler-attribution seconds. A lane with less
  than a configured minimum of observed time has duty `unavailable` and is
  excluded from the mean.
- **Demand**: `lanes_needed = ceil(sustainable_slots / mean duty)` per host,
  bounded by `lane_capacity`. When no lane has a duty value, or the mean duty is
  zero, demand is `unavailable` with reason `no_duty` or `zero_duty`. `UNREACHABLE`
  lanes are excluded from demand and from the mean.
- **Ready buffer**: the count of ready, unassigned and unstarted tickets
  ([FR-001](./FR-001-ready-queue.md) queue, minus tickets in any reachable lane's
  assignment; tickets held by an `UNREACHABLE` lane are not subtracted) across the configured code-lead
  projects. Target buffer is `lanes_needed x queue_depth` (default 2).
  `deficit = max(0, target - buffer)`.
- An unreadable input makes each dependent value `unavailable` with its reason. The
  command SHALL NOT report a deficit of zero from missing data and omits the gauge.
- Interim supply and interim duty are sufficient for the deficit and for
  `need_ready_tickets`; the action does not wait for the build scheduler.
- A positive deficit becomes one action per owning planner project: shares follow
  each project's lane count, the remainder goes to the project with the smallest
  buffer, ties by project id.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-041-AC-1 | Fixtures with known slots, capacity and duty yield the hand-computed `lanes_needed`, target buffer and deficit; `lanes_needed` never exceeds `lane_capacity`. | Test |
| FR-041-AC-2 | Ready tickets assigned to a reachable lane or started do not count toward the buffer; only ready, unassigned, unstarted tickets do. | Test |
| FR-041-AC-8 | Tickets assigned to an UNREACHABLE lane still count in the ready buffer, while tickets assigned to a reachable lane do not. | Test |
| FR-041-AC-9 | A host with a stale registry lease contributes zero slots and zero lane capacity to supply, and its UNREACHABLE lanes are excluded from demand and from the duty mean. | Test |
| FR-041-AC-3 | A buffer at or above target gives deficit zero and no action; one below gives the exact shortfall. | Test |
| FR-041-AC-4 | A mean duty of zero, no lane with duty, a missing capacity or a missing queue makes dependent values `unavailable` with reason, omits the gauge, and never reports deficit zero. | Test |
| FR-041-AC-5 | With only interim supply (server-capacity) and watcher compiler seconds a positive deficit still yields `need_ready_tickets`; when locked-build data is present it replaces both interim sources. | Test |
| FR-041-AC-6 | A deficit of N produces per-project actions whose counts sum to N, deterministically. | Test |
| FR-041-AC-7 | `ix_fleet_ready_buffer_deficit` has bounded labels, matches the command output, and each run stores a `saturation_sample` ledger row. | Test |

## Dependencies

- Final supply and duty: locked-build scheduler (W6, spec not yet authored). Interim: `agent-msg server-capacity` slots and the ix-relay watcher per-lane compiler seconds (ix-relay PR #52, target contract). Lane build evidence also uses [FR-034](./FR-034-fleet-lane-status.md).
