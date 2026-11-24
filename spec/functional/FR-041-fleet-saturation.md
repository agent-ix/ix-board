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
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-037"
    type: depends_on
  - target: "ix://agent-ix/dev-team/FR-010"
    type: depends_on
  - target: "ix://agent-ix/dev-team/FR-015"
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
`step <host>` records the orchestrator's intent that one lane was added and opens a hold at the new lane
count (a lane count from `lane_tick` that does not match is a `lane_change_unrecorded` event, below); `window <host>` closes the current hold, measures it and writes the step;
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
| `capacity_step` | `run_id`, `step_no`, `repeat_of` (the `step_no` of an inconclusive window this one repeats, else empty), host, `lanes`, `window_start`, `window_end`, `rampup_end` (30 minutes after the lane was added, configurable), `valid`, `inconclusive_reason` (`deficit_nonzero`, `deficit_coverage`), `deficit_coverage`, `deficit_zero_throughout`, `host_output_per_hour`, `tickets_closed`, `prs_merged`, `commits_pushed`, `output_basis` (`tickets_prs` or `commits`), `queue_wait_p90_s`, `throttled_share`, `memory_headroom_min`, `thermal_headroom_min`, `slots_busy_equals_sustainable`, `knee_signal`, `knee_reason` (`output_gain`, `throttle`) |
| `capacity_step_lane_output` | `run_id`, `step_no`, lane, `tickets_closed`, `prs_merged`, `commits_pushed` |
| `capacity_level` | host, `level`, `output_basis` (`tickets_prs` or `commits`, of the step the level was found at), `recorded_at`, `run_id`, `reason`, `evidence_steps`, `superseded_at` |
| `capacity_event` | event id, host, lane, `kind` (`lane_added`, `draining_mark`, `drain_done`, `lane_removed`, `lane_lost`, `step_down`, `overload_event`), `reason` (as given, or `lane_change_unrecorded`), `evidence_kind` (`pause`, `admission_block`, `build_abort`, `memory_below_reserve`, `knee`, `throttle_share`, empty; set by ix-board, below), `cycle_id`, `at`, `ceiling_until` |
| `host_output_sample` | host, `at`, `lanes`, trailing-hour `tickets_closed`, `prs_merged`, `commits_pushed` |

### Measurement

- **Host output.** `window` sums, over the host's lanes, tickets closed plus pull
  requests merged in the window, from Linear and GitHub, attributing each to a
  lane by the lane's focus and branch ([FR-037](./FR-037-fleet-focus-and-drift.md)); per-lane figures go to
  `capacity_step_lane_output`. Throughput is lane output, not builds. When
  tickets closed plus pull requests merged in the window number fewer than 5 the
  step is measured in commits pushed per hour and `output_basis` is `commits`.
  The window excludes the ramp-up period.
- **Valid window.** A window is `valid` only when `saturation_sample` rows of the host cover at least 90% of the window (sample count times the tick interval over the window length) and every one shows a deficit of zero. A window with no samples, or with an unavailable deficit (which stores no value and so is not a covering sample), is invalid with reason `deficit_coverage`; a window with any positive deficit is invalid with reason `deficit_nonzero`. After an invalid window the hold repeats the step: the repeated row keeps its `step_no` and carries `repeat_of`.
- **Knee signal.** A valid step has `knee_signal` when the gain in `host_output_per_hour`
  over the previous valid step is below 50% of that previous step's output divided by its
  lane count (`knee_reason` `output_gain`), or `throttled_share` exceeds 5% (`throttle`). Both thresholds are configurable.
  Headroom, throttled share, queue wait and slots come from the build scheduler
  and watcher series; `slots_busy_equals_sustainable` is true when busy equalled sustainable
  throughout the window.
- **Run end.** `end` writes `found_level` and, for a run whose level was found on a valid step, a `capacity_level` row for the host with `evidence_steps` and the `output_basis` of that step; the previous level row gets `superseded_at`. History is never deleted. A run that ends with no new level (`found_level` empty, as with `inconclusive_no_demand` returning the host to its last validly measured level, or `throttled` with no valid step) writes no row and is not a run without a recorded level. `inconclusive_no_demand` is accepted only after two consecutive inconclusive steps; `step_bound` only after 8 steps.
- **Lane changes are observed, not reported.** The tick writes a `lane_added`, `lane_removed` or `lane_lost` event whenever a host's non-draining lane count in `lane_tick` rises or falls between consecutive ticks. The event carries the reason of a `fleet capacity step`, `drain` or `remove` issued within one tick interval before the change, and `lane_change_unrecorded` when there is none, so a lane changed outside `fleet capacity` is visible. A removal that follows a `draining_mark` of the same lane inherits that mark's `reason` and `evidence_kind`. A lane that disappears without a `draining_mark` or `fleet capacity` command and whose class in the previous tick was `DEAD` or `UNREACHABLE`, or whose session ended (a crash, a session exit or a host loss), is written as `lane_lost`, not `lane_removed`: it is not a reduction the orchestrator caused, so it is excluded from `lane_count_reductions` and `lane_change_unrecorded`, and listed separately by `lanes_lost` so a crashing lane stays visible.
- **Evidence is checked, not claimed.** `evidence_kind` is set by ix-board, never taken from the command: `pause` or `admission_block` when `ix_build_pause_seconds_total` or `ix_build_admission_blocked_seconds_total` (dev-team FR-010, FR-015) grew in the 15 minutes before the event, `build_abort` when `ix_build_builds_total` gained an outcome beginning `aborted_` (an `aborted_thermal` or `aborted_memory` abort) in that period, `memory_below_reserve` when `ix_build_probe_value` for memory was below the profile reserve in it, `knee` when the host's latest valid `capacity_step` has `knee_signal`, `throttle_share` when throttled share (below) exceeded 5% over the trailing hour. A reduction with none of these has empty `evidence_kind` and is a violation.
- **Step-down and cool-off.** `drain` with an overload reason writes a `draining_mark` event in the cycle of the overload and a `step_down` event whose `ceiling_until` is the cool-off end (default 6 hours). A `lane_added` event before `ceiling_until` that raises the lane count above the ceiling is a raise during cool-off. A `drain_done` event is written by the tick when the lane's current ticket has a commit or pull request and the scheduler shows no build for it; a `lane_removed` event before it is a removal before drain. `overload_event` rows are written by the tick from the scheduler and watcher series (memory below the profile reserve, a thermal build abort).
- **Throttled share.** The throttled share of a host over a window is the increase of `ix_build_pause_seconds_total` plus `ix_build_admission_blocked_seconds_total` (dev-team FR-010, FR-015) over the window length; it is the series `prom.host_throttled_share`.
- **Host output samples.** Each tick writes one `host_output_sample` per host, with tickets closed, pull requests merged and commits pushed over the trailing hour, so a share can be computed on either basis.

### Prometheus series

The exporter ([FR-033](./FR-033-team-flow-effort-export.md)) SHALL serve, per host with bounded labels:
`ix_fleet_capacity_recorded_level{host}` (the current `capacity_level.level`, omitted when none),
`ix_fleet_capacity_recorded_level_timestamp_seconds{host}`,
`ix_fleet_capacity_discovery_state{host,state}` (1 for the current state, one of `none`, `running`, `ended`) and
`ix_fleet_capacity_draining_lanes{host}`,
`ix_fleet_capacity_throttled_at_level{host}` (1 when the host has a recorded level and its 5-minute throttled share exceeded 5% in every 5-minute interval of the trailing hour, else 0),
`ix_fleet_capacity_throttled_without_step_down{host}` (1 when `throttled_at_level` is 1 and no `step_down` event of the host was recorded since that hour began, else 0), and
`ix_fleet_capacity_overload_events_total{host,cause}` (a counter of `overload_event` rows, `cause` one of `memory_below_reserve`, `thermal_abort`).

The watcher series (`prom.host_throttled_share`) and scheduler series named above are read from the build scheduler (dev-team FR-010, FR-015); this requirement owns no scheduler behavior.

### Role-spec query views

| View (`ledger.<name>` or `prom.<name>`) | Definition |
|------|------------|
| `discovery_steps` | `capacity_step` rows |
| `discovery_steps_violating_ramp` | non-repeated steps whose `lanes` is not the previous step's plus one, and any step whose measured span after `rampup_end` is shorter than the configured window; a step with `repeat_of` set (same lane count as the step it repeats) is not a violation |
| `discovery_steps_judged_without_demand` | steps whose knee or level decision used an invalid window: a step that follows an invalid window with a new `step_no` instead of a repeat, a `knee_signal` set on an invalid step, and `capacity_level` rows whose `evidence_steps` include an invalid step |
| `discovery_knee_signals` | steps with `knee_signal` |
| `discovery_raised_past_knee` | steps of a run after a `knee_signal` step with `lanes` above that step's |
| `discovery_runs_ended` | `capacity_run` rows with `ended_at` |
| `discovery_runs_without_recorded_level` | ended runs with a non-empty `found_level` and no `capacity_level` row of the same `run_id`; a run ending with no new level is excluded |
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
| `lane_change_unrecorded` | `lane_added` and `lane_removed` events with reason `lane_change_unrecorded` (never `lane_lost`) |
| `lanes_lost` | `lane_lost` events, with lane, host and the class before loss |
| `draining_lanes` | `draining_mark` events with no `drain_done` and no `lane_removed` for the lane |
| `drains_over_cap_without_escalation` | `draining_lanes` older than the drain cap with no `action_send` for an `escalate_drain` action ([FR-038](./FR-038-fleet-tick.md)) for the lane; the tick emits `escalate_drain` and the minder sends it, which discharges the orchestrator's escalation duty (the role spec is to be updated to say so) |
| `discovery_inconclusive_windows` | `capacity_step` rows with `valid` false |
| `no_demand_runs_not_ended` | runs with two consecutive inconclusive steps and no `ended_at`, or an `end_reason` other than `inconclusive_no_demand` |
| `discovery_slot_saturated_windows` | valid steps with `slots_busy_equals_sustainable` |
| `discovery_raised_past_slot_saturation` | steps after such a step in the run with `lanes` above that step's |
| `lane_count_reductions` | `lane_removed` (not `lane_lost`), `draining_mark` and `step_down` `capacity_event` rows (including `lane_change_unrecorded`), with `reason` and `evidence_kind` |
| `lane_reductions_without_throttle_evidence` | `lane_count_reductions` with empty `evidence_kind`, or reason `lane_change_unrecorded` |
| `host_output_share_of_recorded_level` | latest `host_output_sample` output per hour, on the `output_basis` stored with the host's current `capacity_level` (tickets closed plus pull requests merged, or commits pushed), over `host_output_per_hour` of the step at that level, in percent |
| `prom.hosts_throttled_at_recorded_level` | series `ix_fleet_capacity_throttled_at_level{host}` |
| `prom.throttled_hosts_without_step_down` | series `ix_fleet_capacity_throttled_without_step_down{host}` |
| `prom.reserve_breach_or_thermal_abort_events` | series `ix_fleet_capacity_overload_events_total{host,cause}`, from `overload_event` rows |
| `prom.host_throttled_share` | throttled share as defined under Measurement |

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
| FR-041-AC-11 | A window whose `saturation_sample` rows cover under 90% of its length, one with no samples, one containing an unavailable deficit, and one with any positive deficit are each `valid` false (`deficit_coverage`, `deficit_coverage`, `deficit_coverage`, `deficit_nonzero`); a window with at least 90% coverage and all zeros is valid; the repeated hold keeps its `step_no` with `repeat_of`, and `discovery_steps_violating_ramp` does not list it. | Test |
| FR-041-AC-18 | A step that follows an invalid window with a new `step_no`, a `knee_signal` on an invalid step, and a `capacity_level` whose evidence includes an invalid step are each listed by `discovery_steps_judged_without_demand`; a correctly repeated step is not. | Test |
| FR-041-AC-12 | `knee_signal` is set at an output gain just under and not at just over 50% of the previous step's per-lane mean, and when throttled share exceeds 5%; `slots_busy_equals_sustainable` is true only when busy equalled sustainable throughout. | Test |
| FR-041-AC-13 | `end` writes `found_level` and a `capacity_level` row with evidence steps and the step's `output_basis`, and sets `superseded_at` on the previous row, keeping both; a run ending `inconclusive_no_demand` with no new level writes no row and is not listed by `discovery_runs_without_recorded_level`, while an ended run with a `found_level` and no row is; `end inconclusive_no_demand` is refused before two consecutive inconclusive steps and `step_bound` before 8 steps. | Test |
| FR-041-AC-14 | `start` is refused for a host with an open run, for a third run on one UTC day, and within the cool-off; a `drain` for `memory_below_reserve` writes a `draining_mark` and a `step_down` with `ceiling_until` six hours later, and a `lane_added` event before it that raises the lane count is listed by `raises_during_cool_off`. | Test |
| FR-041-AC-19 | A host's non-draining `lane_tick` count rising or falling between ticks writes a `lane_added` or `lane_removed` event carrying the reason of a `fleet capacity` command issued within one tick interval before it, and `lane_change_unrecorded` otherwise, which `lane_change_unrecorded` and `lane_reductions_without_throttle_evidence` list. | Test |
| FR-041-AC-22 | A `lane_removed` after a `draining_mark` of the same lane carries that mark's `reason` and `evidence_kind` and is not listed by `lane_reductions_without_throttle_evidence` when the mark was corroborated. | Test |
| FR-041-AC-23 | A lane that vanishes after a tick in which its class was `DEAD`, or `UNREACHABLE`, with no `draining_mark` or command, is a `lane_lost` event: it is listed by `lanes_lost`, and absent from `lane_count_reductions`, `lane_change_unrecorded` and `lane_reductions_without_throttle_evidence`; a disappearing `WORKING` lane with no mark or command is `lane_removed` with reason `lane_change_unrecorded`. | Test |
| FR-041-AC-20 | `evidence_kind` is set by ix-board: a `drain --reason thermal_abort` with no pause, admission-block or `aborted_` build outcome in the preceding 15 minutes has empty `evidence_kind` and is listed by `lane_reductions_without_throttle_evidence`, and the same drain after such a series change has `build_abort` and is not listed. | Test |
| FR-041-AC-15 | `drain_done` is recorded by the tick only when the lane's ticket has a commit or pull request and no build runs; a `lane_removed` event before it is listed by `lane_removals_before_drain`; a drain older than the cap with no sent `escalate_drain` action (`action_send`) is listed by `drains_over_cap_without_escalation`. | Test |
| FR-041-AC-16 | The exporter serves `ix_fleet_capacity_recorded_level{host}`, its timestamp series, `ix_fleet_capacity_discovery_state{host,state}`, `ix_fleet_capacity_draining_lanes{host}`, `ix_fleet_capacity_throttled_at_level{host}`, `ix_fleet_capacity_throttled_without_step_down{host}` and `ix_fleet_capacity_overload_events_total{host,cause}` with labels bounded by host, state and cause, omits the level for a host with none, and the values equal the ledger; a host throttled above 5% in every interval of an hour with no `step_down` shows `throttled_without_step_down` 1, and 0 after a `step_down`. | Test |
| FR-041-AC-17 | Every capacity view and `prom.` name in the role-spec query table exists under its name and returns the hand-computed value on a fixture covering a knee run, a no-demand run, a slot-saturated run and an overload step-down. | Test |
| FR-041-AC-21 | `host_output_share_of_recorded_level` uses the `output_basis` stored with the recorded level for both the numerator and the denominator: a level found on `commits` is compared with commits per hour, a level on `tickets_prs` with tickets closed plus pull requests merged. | Test |

## Dependencies

- Final supply and duty: locked-build scheduler (W6). Interim: `agent-msg server-capacity` slots and the ix-relay watcher per-lane compiler seconds (ix-relay PR #52, target contract). Lane build evidence also uses [FR-034](./FR-034-fleet-lane-status.md).
- Capacity discovery reads build scheduler series from dev-team FR-010 and FR-015 (`ix_build_pause_seconds_total`, `ix_build_admission_blocked_seconds_total`, `ix_build_probe_value`, `ix_build_slots_busy`, `ix_build_slots_sustainable`, `ix_build_queue_wait_seconds`, `ix_build_builds_total{outcome}`), Linear and GitHub for host output, [FR-037](./FR-037-fleet-focus-and-drift.md) focus for per-lane attribution, and the [FR-039](./FR-039-fleet-ledger.md) ledger. `fleet tick` ([FR-038](./FR-038-fleet-tick.md)) writes `drain_done`, `overload_event`, `lane_added`, `lane_removed` and `host_output_sample`. The scheduler series are enablement owned by dev-team; this requirement adds none.
- `aborted_*` outcomes of `ix_build_builds_total{outcome}` (used as `build_abort` evidence) depend on SWM-260, which blocks SWM-252, which has dev-team FR-015 enumerate the outcome values; until it lands the `aborted_` prefix of dev-team FR-010 is the contract.
