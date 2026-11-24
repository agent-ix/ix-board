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

The command family `ix-board fleet capacity` SHALL also record capacity discovery per host (runs, steps, windows, the found knee, the recorded sustainable level and its history, draining marks) in the fleet ledger and export the recorded level and discovery state as Prometheus series.

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

## Capacity discovery history

The orchestrator finds and holds each host's sustainable lane count by discovery
runs (dev-team orchestrator role spec). ix-board owns the evidence: it records
each run, step and measurement window, judges them deterministically, and keeps
the recorded level and its history. The orchestrator decides when to run and
issues the commands; ix-board does not add or remove lanes. Lanes are the unit,
and build slots are read from the build scheduler, never ramped here.

### Commands

`fleet capacity start <host> --trigger <first|re_discovery|step_down>` opens a run;
`step <host>` records that one lane was added and opens a hold at the new lane
count; `window <host>` closes the current hold, measures it and writes the step;
`end <host> --reason <knee|build_slots_saturated|step_bound|inconclusive_no_demand|throttled>`
closes the run and records the level; `drain <host> <lane> --reason <memory_below_reserve|thermal_abort|throttle|knee_step_back>`
marks a lane draining; `remove <host> <lane>` records the lane's removal;
`status [--json]` prints runs, levels and draining lanes. All write
through the ledger writer lock ([FR-039](./FR-039-fleet-ledger.md)); `start` is refused when the
host already has an open run, when two runs of that host started the same UTC day, or when the host is within its cool-off.

### Tables (ledger version 2)

| Table | Columns |
|-------|---------|
| `capacity_run` | `run_id`, host, `trigger`, `started_at`, `ended_at`, `end_reason` (`knee`, `build_slots_saturated`, `step_bound`, `inconclusive_no_demand`, `throttled`), `found_level` |
| `capacity_step` | `run_id`, `step_no`, host, `lanes`, `window_start`, `window_end`, `rampup_end` (30 minutes after the lane was added, configurable), `valid`, `inconclusive_reason` (`deficit_nonzero`), `deficit_zero_throughout`, `host_output_per_hour`, `tickets_closed`, `prs_merged`, `commits_pushed`, `output_basis` (`tickets_prs` or `commits`), `queue_wait_p90_s`, `throttled_share`, `memory_headroom_min`, `thermal_headroom_min`, `slots_busy_equals_sustainable`, `knee_signal`, `knee_reason` (`output_gain`, `throttle`) |
| `capacity_step_lane_output` | `run_id`, `step_no`, lane, `tickets_closed`, `prs_merged`, `commits_pushed` |
| `capacity_level` | host, `level`, `recorded_at`, `run_id`, `reason`, `evidence_steps`, `superseded_at` |
| `capacity_event` | event id, host, lane, `kind` (`lane_added`, `draining_mark`, `drain_done`, `lane_removed`, `step_down`, `overload_event`, `drain_escalated`), `reason`, `evidence_kind` (`pause`, `admission_block`, `build_abort`, `memory_below_reserve`, `knee`, `throttle_share`, empty), `cycle_id`, `at`, `ceiling_until` |
| `host_output_sample` | host, `at`, `lanes`, trailing-hour `tickets_closed`, `prs_merged`, `commits_pushed` |

### Measurement

- **Host output.** `window` sums, over the host's lanes, tickets closed plus pull
  requests merged in the window, from Linear and GitHub, attributing each to a
  lane by the lane's focus and branch ([FR-037](./FR-037-fleet-focus-and-drift.md)); per-lane figures go to
  `capacity_step_lane_output`. Throughput is lane output, not builds. When
  tickets closed plus pull requests merged in the window number fewer than 5 the
  step is measured in commits pushed per hour and `output_basis` is `commits`.
  The window excludes the ramp-up period.
- **Valid window.** A window is `valid` only when the ready-buffer deficit of every
  `saturation_sample` of the host in the window was zero
  (`deficit_zero_throughout`); otherwise it is inconclusive with reason
  `deficit_nonzero` and the next hold repeats the step (the repeated step keeps its `step_no`).
- **Knee signal.** A valid step has `knee_signal` when the gain in `host_output_per_hour`
  over the previous valid step is below 50% of that previous step's output divided by its
  lane count (`knee_reason` `output_gain`), or `throttled_share` exceeds 5% (`throttle`). Both thresholds are configurable.
  Headroom, throttled share, queue wait and slots come from the build scheduler
  and watcher series; `slots_busy_equals_sustainable` is true when busy equalled sustainable
  throughout the window.
- **Run end.** `end` writes `found_level` and, for a run that ended on a valid
  step, a `capacity_level` row for the host with `evidence_steps`; the previous
  level row gets `superseded_at`. History is never deleted. `end` with
  `inconclusive_no_demand` is accepted only after two consecutive inconclusive steps and records the last validly measured level; `step_bound` only after 8 steps.
- **Step-down and cool-off.** `drain` with an overload reason writes a
  `draining_mark` event in the cycle of the overload and a `step_down` event whose
  `ceiling_until` is the cool-off end (default 6 hours); `lane_added` events before it are raises
  during cool-off. A `drain_done` event is written by the tick when the lane's current ticket has
  a commit or pull request and the scheduler shows no build for it; `remove` before it
  is recorded as a removal before drain. `overload_event` rows are written by the tick from the scheduler
  series (memory headroom below the profile reserve, a thermal build abort).
- **Host output samples.** Each tick writes one `host_output_sample` per host.

### Prometheus series

The exporter ([FR-033](./FR-033-team-flow-effort-export.md)) SHALL serve, per host with bounded labels:
`ix_fleet_capacity_recorded_level{host}` (the current `capacity_level.level`, omitted when none),
`ix_fleet_capacity_recorded_level_timestamp_seconds{host}`,
`ix_fleet_capacity_discovery_state{host,state}` (1 for the current state, one of `none`, `running`, `ended`) and
`ix_fleet_capacity_draining_lanes{host}`.

### Role-spec query views

| View (`ledger.<name>` or `prom.<name>`) | Definition |
|------|------------|
| `discovery_steps` | `capacity_step` rows |
| `discovery_steps_violating_ramp` | steps whose `lanes` is not the previous step's plus one, or whose measured span after `rampup_end` is shorter than the configured window |
| `discovery_steps_judged_without_demand` | `valid` steps with `deficit_zero_throughout` false |
| `discovery_knee_signals` | steps with `knee_signal` |
| `discovery_raised_past_knee` | steps of a run after a `knee_signal` step with `lanes` above that step's |
| `discovery_runs_ended` | `capacity_run` rows with `ended_at` |
| `discovery_runs_without_recorded_level` | ended runs with no `capacity_level` row of the same `run_id` |
| `unrecorded_hosts_at_zero_deficit` | `saturation_sample` rows with deficit zero for a host with no current `capacity_level` |
| `unrecorded_hosts_without_discovery_run` | those rows with no `capacity_run` started within one cycle after the sample |
| `hosts_with_recorded_level` | `capacity_level` rows with `superseded_at` empty |
| `host_minutes_above_recorded_level_outside_bounded_discovery` | minutes in which the host's non-draining `lane_tick` count exceeds its `capacity_level.level` and no run with 8 or fewer steps was open |
| `host_step_downs` | `capacity_event` rows of kind `step_down` |
| `raises_during_cool_off` | `lane_added` events before the host's `ceiling_until` that raise the lane count above the ceiling |
| `discovery_runs` | `capacity_run` rows |
| `discovery_runs_per_host_day_max` | maximum count of runs started per host per UTC day |
| `overload_events_without_draining_mark_in_cycle` | `overload_event` rows with no `draining_mark` of the host by the end of the first cycle ending after them |
| `lane_removals`, `lane_removals_before_drain` | `lane_removed` events; those with no earlier `draining_mark` or no `drain_done` for the lane |
| `draining_lanes` | `draining_mark` events with no `lane_removed` |
| `drains_over_cap_without_escalation` | `draining_lanes` older than the drain cap with no `escalate_drain` action ([FR-038](./FR-038-fleet-tick.md)) |
| `discovery_inconclusive_windows` | `capacity_step` rows with `valid` false |
| `no_demand_runs_not_ended` | runs with two consecutive inconclusive steps and no `ended_at`, or an `end_reason` other than `inconclusive_no_demand` |
| `discovery_slot_saturated_windows` | valid steps with `slots_busy_equals_sustainable` |
| `discovery_raised_past_slot_saturation` | steps after such a step in the run with `lanes` above that step's |
| `lane_count_reductions` | `capacity_event` rows of kind `draining_mark` or `step_down`, with `reason` and `evidence_kind` |
| `lane_reductions_without_throttle_evidence` | `lane_count_reductions` with empty `evidence_kind` |
| `host_output_share_of_recorded_level` | latest `host_output_sample` output per hour over `host_output_per_hour` of the step at the recorded level, in percent |
| `prom.hosts_throttled_at_recorded_level` | the watcher's throttled-share series joined with `ix_fleet_capacity_recorded_level{host}` |

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
| FR-041-AC-10 | `fleet capacity window` on a fixture computes host output as tickets closed plus pull requests merged per hour over the post-ramp-up window with per-lane attribution by focus and branch in `capacity_step_lane_output`, and uses commits per hour with `output_basis` `commits` when the sum is under 5. | Test |
| FR-041-AC-11 | A window with any `saturation_sample` of positive deficit is `valid` false with `inconclusive_reason` `deficit_nonzero`, and a window with all zeros is valid. | Test |
| FR-041-AC-12 | `knee_signal` is set at an output gain just under and not at just over 50% of the previous step's per-lane mean, and when throttled share exceeds 5%; `slots_busy_equals_sustainable` is true only when busy equalled sustainable throughout. | Test |
| FR-041-AC-13 | `end` writes `found_level` and a `capacity_level` row with evidence steps and sets `superseded_at` on the previous row, keeping both; `end inconclusive_no_demand` is refused before two consecutive inconclusive steps and `step_bound` before 8 steps. | Test |
| FR-041-AC-14 | `start` is refused for a host with an open run, for a third run on one UTC day, and within the cool-off; a `drain` for `memory_below_reserve` writes a `draining_mark` and a `step_down` with `ceiling_until` six hours later, and a `lane_added` before it is listed by `raises_during_cool_off`. | Test |
| FR-041-AC-15 | `drain_done` is recorded by the tick only when the lane's ticket has a commit or pull request and no build runs; a `remove` before it is listed by `lane_removals_before_drain`; a drain older than the cap without an `escalate_drain` action is listed by `drains_over_cap_without_escalation`. | Test |
| FR-041-AC-16 | The exporter serves `ix_fleet_capacity_recorded_level{host}`, its timestamp series, `ix_fleet_capacity_discovery_state{host,state}` and `ix_fleet_capacity_draining_lanes{host}` with labels bounded by host and state, omits the level for a host with none, and the values equal the ledger. | Test |
| FR-041-AC-17 | Every capacity view in the role-spec query table exists under its name and returns the hand-computed value on a fixture covering a knee run, a no-demand run, a slot-saturated run and an overload step-down. | Test |

## Dependencies

- Final supply and duty: locked-build scheduler (W6, spec not yet authored). Interim: `agent-msg server-capacity` slots and the ix-relay watcher per-lane compiler seconds (ix-relay PR #52, target contract). Lane build evidence also uses [FR-034](./FR-034-fleet-lane-status.md).
