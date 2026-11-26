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

The `ix-board fleet saturation [collect] [--json]` command SHALL compute supply (sustainable
build slots and lane capacity), demand (lanes needed to saturate those slots), and
the ready-buffer deficit. The command SHALL export the gauge
`ix_fleet_ready_buffer_deficit`, and `fleet tick` SHALL emit a `need_ready_tickets`
action per owning planner project for a positive deficit.

The command family `ix-board fleet capacity` SHALL also record capacity discovery per host (runs, steps, windows, the found knee, the recorded sustainable level and its history, draining marks) in the fleet ledger and export the recorded level and discovery state as Prometheus series.

## Behavior

- **Supply** per host: `sustainable_slots` and optional `lane_capacity`. The final source of
  `sustainable_slots` is the build scheduler (locked-build, W6). Until it exists,
  `sustainable_slots` comes from the explicit `fleet.capacity_limits.<host>.build_slots`
  ix-board configuration value, with that config path retained as its source basis;
  `fleet.capacity_limits.<host>.lane_capacity` is an optional operator-entered demand bound.
  Absence of that setting is normal and never makes demand unavailable. Memory reserve
  and thermal limit also come from explicit per-host `fleet.capacity_limits.<host>` config.
  A host whose registry lease is stale
  ([FR-034](./FR-034-fleet-lane-status.md)) contributes no slots and no lane capacity.
- **Duty cycle** per lane: build seconds divided by observed engaged lane seconds over a
  trailing window (default 24h). The final source is locked-build; the interim
  source is `increase(ix_fleet_lane_compile_seconds_total{host,lane,job="ix-relay-watcher"}[window])`.
  State coverage uses all `ix_fleet_lane_state_ratio{host,lane}` samples. Engaged lane seconds
  count samples where `state!="idle"` and multiply by the configured push interval, capped at the
  window length for each lane; this is the watcher-presence basis because no observed-lane-seconds
  series exists. Both watcher inputs need at least 90% sample coverage. A lane with less than the
  configured minimum of engaged time is excluded from the mean; idle time does not dilute duty.
- **Capacity measurement sources and basis.** Every field in a discovery step and
  each per-lane output observation SHALL retain its source name, availability,
  reason, coverage and observation time. A field with less than 90% window
  coverage SHALL be serialized as `null` with its reason; it SHALL NOT be
  replaced with zero. Interim sustainable slots use the explicit
  `fleet.capacity_limits.<host>.build_slots` value. Relay
  SWM-242 supplies `ix_build_slots_busy`, `ix_build_slots_configured`,
  `ix_build_queue_wait_seconds`, `ix_build_admission_blocked_seconds_total`
  and `ix_build_builds_total{outcome}`. Queue-wait p90 is computed from the
  histogram buckets. `ix_build_slots_configured` is the configured slot count;
  interim `sustainable_slots` comes from `fleet.capacity_limits.<host>.build_slots`.
  Queue-wait p90 aggregates all priority classes into one histogram before
  quantile calculation. Memory uses node-exporter `node_memory_MemAvailable_bytes`
  when covered, then relay-watcher free memory derived from
  `ix_fleet_host_memory_total_bytes - ix_fleet_host_memory_used_bytes`; the
  explicit per-host memory reserve is in `[fleet.capacity_limits.<host>]`.
  Temperature uses a CPU-package sensor selected by host name: titan uses
  `node_hwmon_temp_celsius{chip="pci0000:00_0000:00:18_3",sensor="temp1"}`,
  luna uses `node_thermal_zone_temp{type="x86_pkg_temp"}`, terra uses
  `node_thermal_zone_temp{sensor="CPU Package"}`, draco uses
  `node_thermal_temperature_celsius{sensor=~"PMU tdie.*"}`, and any other host
  uses `node_thermal_temperature_celsius`. The explicit per-host thermal limit
  comes from `fleet.capacity_limits.<host>`; the selector, limit and source
  basis are persisted per field.
- **Demand**: `lanes_needed = ceil(sustainable_slots / mean duty)` per host,
  bounded only when an operator has entered `fleet.capacity_limits.<host>.lane_capacity`.
  An absent lane limit does not make `lanes_needed` or the ready-buffer deficit
  unavailable. Duty is compiler seconds divided by observed engaged lane seconds;
  engaged time counts watcher `ix_fleet_lane_state_ratio` samples in any state
  other than `idle`, times the push interval. Idle samples never dilute duty.
  Lanes below the minimum engaged time are excluded from the mean; if no lanes
  meet that minimum, duty is unavailable. A stale host contributes zero supply
  and demand. `UNREACHABLE` lanes are excluded from demand and from the mean.
- **Ready buffer**: the count of ready, unassigned and unstarted tickets
  ([FR-001](./FR-001-ready-queue.md) queue, minus tickets in any reachable lane's
  assignment; tickets held by an `UNREACHABLE` lane are not subtracted) across the configured code-lead
  projects. Target buffer is `lanes_needed x queue_depth` (default 2).
  `deficit = max(0, target - buffer)`.
  When a host has a current registry lease but no reachable lanes yet, discovery
  may start when `max(0, ready_buffer - all_hosts_unsupplied_lanes) + queued_behind_first`
  is at least one. `queued_behind_first` counts distinct unfinished tickets after
  the first entry in any reachable lane's assignment list. This bootstrap rule
  applies only to discovery start, never to a capacity-step window. A stale host
  cannot start discovery.
- An unreadable input makes each dependent value `unavailable` with its reason. The
  command SHALL NOT report a deficit of zero from missing data and omits the gauge.
- Coverage is measured on the push-interval grid: each interval contributes at
  most one covered interval when at least one matching sample is present. Duplicate
  series and extra samples within an interval do not increase coverage. Explicit
  configuration has full coverage only when its value is present; counters use
  their series-presence grid while their value is computed by `increase` over the
  requested window. A Linear or GitHub count has full coverage only when ix-board
  read every ticket, attachment list, pull request and commit list on which that
  count depends; otherwise it is `null` with its reason.
- Interim supply and interim duty are sufficient for the deficit and for
  `need_ready_tickets`; the action does not wait for the build scheduler.
- A positive deficit becomes one action per owning planner project: shares follow
  each project's lane count, the remainder goes to the project with the smallest
  buffer, ties by project id. The tick writes the project and positive requested count
  on the `action` row; its existing relay-time `emitted_at` and `resolved_at` record
  when the deficit first appeared and when it cleared.
- `fleet saturation --json` is a read-only report. The `fleet saturation collect
  --json` subcommand SHALL append one live saturation sample per configured host
  to the local ledger without sending relay messages or lane orders. A draco
  macOS LaunchAgent SHALL run this collector every 300 seconds, matching the
  default `fleet.tick_interval_secs`; collection is independent of panel reads
  and manual invocation. Missing or unavailable samples do not count as zero.

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
count (a lane count from `lane_tick` that does not match is a `lane_change_unrecorded` event, below); it takes
`--lanes`, `--lane-added-at-ms` and `--rampup-end-ms`, and may take `--repeat-of` for a repeated hold.
`window <host>` closes the current hold, measures it and writes the step;
`end <host> --reason <knee|build_slots_saturated|step_bound|inconclusive_no_demand|throttled>`
closes the run and records the level; `drain <host> <lane> --reason <memory_below_reserve|thermal_abort|throttle|knee_step_back>`
marks a lane draining; `remove <host> <lane>` records the lane's removal;
`status [--json]` prints runs, levels and draining lanes. All write
through the ledger writer lock ([FR-039](./FR-039-fleet-ledger.md)); `start` is refused when the
host already has an open run, when two runs of that host started the same UTC day, or when the host is within its cool-off.

### Tables (ledger version 6)

| Table | Columns |
|-------|---------|
| `capacity_run` | `run_id`, host, `trigger`, `started_at`, `ended_at`, `end_reason` (`knee`, `build_slots_saturated`, `step_bound`, `inconclusive_no_demand`, `throttled`), `found_level` |
| `capacity_window` | one open hold per run: `run_id`, `step_no`, `repeat_of`, host, `lanes`, `lane_added_at`, `rampup_end`, `window_min_ms` |
| `capacity_step` | `run_id`, `step_no`, `repeat_of` (the `step_no` of an inconclusive window this one repeats, else empty), host, `lanes`, `window_start`, `window_end`, `rampup_end`, `window_min_ms`, `valid`, `inconclusive_reason` (`demand_short`, `deficit_coverage`), `deficit_coverage` (union of distinct sample intervals that cover demand), `deficit_zero_throughout` (separate backlog indicator), `host_output_per_hour`, `tickets_closed`, `prs_merged`, `commits_pushed`, `output_basis` (`tickets_prs` or `commits`), `queue_wait_p90_s`, `throttled_share`, `memory_headroom_min`, `thermal_headroom_min`, `slots_busy_equals_sustainable`, `knee_signal`, `knee_reason` (`output_gain`, `throttle`), `source_basis` (JSON object keyed by field, with availability, source, reason, coverage and observation time) |
| `capacity_step_lane_output` | `run_id`, `step_no`, lane, `window_end`, `tickets_closed`, `prs_merged`, `commits_pushed`, `source_basis` (same JSON shape per lane field) |
| `capacity_level` | host, `level`, `output_basis` (`tickets_prs` or `commits`, of the step the level was found at), `recorded_at`, `run_id`, `reason`, `evidence_steps`, `superseded_at` |
| `capacity_event` | event id, host, lane, `kind` (`lane_added`, `draining_mark`, `drain_done`, `lane_removed`, `lane_lost`, `step_down`, `overload_event`), `reason` (as given, or `lane_change_unrecorded`), `evidence_kind` (`admission_block`, `build_abort`, `memory_below_reserve`, `thermal_abort`, `thermal_limit_breach`, `knee`, `throttle_share`, empty; set by ix-board, below), `cycle_id`, `at`, `ceiling_until`, `drain_cap_ms` (configured cap captured on each `draining_mark`, default 4 hours), `source_basis` |
| `host_output_sample` | host, `at`, `lanes`, trailing-hour `tickets_closed`, `prs_merged`, `commits_pushed`, `source_basis` (JSON source, availability, reason, coverage and observation time for each current sample field) |
| `saturation_sample` | one row per host and collection time: ready buffer, active/supplied/unsupplied lane counts, ready share, queued tickets behind first assignment, zero-lane bootstrap pool, registry-current flag, deficit, source basis and unavailable reason |
| `capacity_throttle_sample` | host, time, interval, admission-blocked share, coverage, unavailable reason and `admission_only` source basis; no pause-seconds series is recorded |
| `capacity_overload_state` | host, breach cause, active state, update time and source basis; `capacity_event` stores only false-to-true overload transitions |

### Measurement

- **Host output.** `window` sums, over the host's lanes, tickets closed plus pull
  requests merged in the window, from Linear and GitHub, attributing tickets to
  lanes from relay work events and ledger assignment history, and pull requests
  through Linear issue attachments or ticket identifiers in PR title/branch;
  per-lane figures go to
  `capacity_step_lane_output`. Throughput is lane output, not builds. When
  tickets closed plus pull requests merged in the window number fewer than 5 the
  step is measured in commits pushed per hour and `output_basis` is `commits`.
  The window excludes the ramp-up period.
- **Demand evidence.** A supplied lane is reachable and holds at least one assigned ticket not completed, canceled or duplicated; backlog state types such as `Backlog` and `Specify` count. Other reachable lanes are unsupplied. The global ready buffer counts ready, unassigned, unstarted tickets. Each host receives a ready share apportioned by its unsupplied-lane count, using largest remainder and host ID for ties, capped at its unsupplied count; shares sum to no more than the global buffer. For a zero-lane host with a current registry lease, discovery-start bootstrap availability is `max(0, ready_buffer - all_hosts_unsupplied_lanes) + distinct_unfinished_tickets_queued_behind_first`; this is a shared eligibility threshold, not a per-host ticket allocation. The collector stores `ready_buffer`, `active_lanes`, `supplied_lanes`, `unsupplied_lanes`, `ready_share`, `bootstrap_ticket_pool`, `registry_current`, and source basis on each `saturation_sample`.
- **History retention.** Capacity runs, windows, steps, levels, saturation/throttle
  samples and capacity events are retained without age pruning because they are the
  evidence for the current recorded level and role-spec history views.
- **Valid window.** Demand coverage depends only on ready-buffer, assignment, lane and registry evidence; it never depends on duty, `lanes_needed` or deficit. A sample covers a step only when it has at least one reachable lane and `supplied_lanes + ready_share >= step.lanes`; the zero-lane bootstrap rule never covers a step window. Every sample in the window must have complete demand evidence and cover demand. Covering sample intervals are unioned by timestamp, so duplicate or clumped rows do not inflate their span; the resulting coverage must be at least 90% of the window. Missing or incomplete evidence is invalid with reason `deficit_coverage`; any sample that falls short is invalid with reason `demand_short`. A positive target-buffer deficit does not invalidate a window. `deficit_zero_throughout` remains a separate backlog indicator. After an invalid window the hold repeats the step: the repeated row keeps its `step_no` and carries `repeat_of`.
- **Knee signal.** A valid step has `knee_signal` when the gain in `host_output_per_hour`
  over the previous valid step is below 50% of that previous step's output divided by its
  lane count (`knee_reason` `output_gain`), or `throttled_share` exceeds 5% (`throttle`). Both thresholds are configurable.
  Queue wait, busy and configured slots come from SWM-242 build series; interim
  sustainable slots come from `fleet.capacity_limits.<host>.build_slots`. Memory and thermal
  readings come from node-exporter/relay-watcher and are compared with explicit
  per-host limits. `slots_busy_equals_sustainable` is true only when busy and
  sustainable counts are both available and equal throughout the window. Slot
  saturation is optional evidence: its absence does not invalidate a window or
  prevent an output knee or the eight-step bound from ending discovery. The
  `build_slots_saturated` end reason is allowed only when both slot values are
  available and equal throughout the step; missing values remain unavailable
  and are never treated as zero.
- **Run end.** `end` writes `found_level` and, for a run whose level was found on a valid step, a `capacity_level` row for the host with `evidence_steps` and the `output_basis` of that step; the previous level row gets `superseded_at`. For `knee`, `found_level` is the lane count of the last valid step before the knee step, never the knee step's own count. History is never deleted. A run that ends with no new level (`found_level` empty, as with `inconclusive_no_demand` returning the host to its last validly measured level, or `throttled` with no valid step) writes no row and is not a run without a recorded level. `inconclusive_no_demand` is accepted only after two consecutive inconclusive steps; `step_bound` only after 8 steps.
- **Lane changes are observed, not reported.** The tick writes a `lane_added`, `lane_removed` or `lane_lost` event whenever a host's non-draining lane count in `lane_tick` rises or falls between consecutive ticks. The event carries the reason of a `fleet capacity step`, `drain` or `remove` issued within one tick interval before the change, and `lane_change_unrecorded` when there is none, so a lane changed outside `fleet capacity` is visible. A removal that follows a `draining_mark` of the same lane inherits that mark's `reason` and `evidence_kind`. A lane that disappears without a `draining_mark` or `fleet capacity` command and whose class in the previous tick was `DEAD` or `UNREACHABLE`, or whose session ended (a crash, a session exit or a host loss), is written as `lane_lost`, not `lane_removed`: it is not a reduction the orchestrator caused, so it is excluded from `lane_count_reductions` and `lane_change_unrecorded`, and listed separately by `lanes_lost` so a crashing lane stays visible.
- **Evidence is checked, not claimed.** `evidence_kind` is set by ix-board, never taken from the command: `admission_block` when `ix_build_admission_blocked_seconds_total` grew in the 15 minutes before the event, `build_abort` when `ix_build_builds_total` gained an outcome in the closed set `aborted_memory`, `aborted_thermal`, `aborted_rss_cap` or `aborted_pause_exceeded` in that period, `memory_below_reserve` when node-exporter `node_memory_MemAvailable_bytes` (or the configured host memory fallback) was below the configured per-host reserve, `knee` when the host's latest valid `capacity_step` has `knee_signal`, `throttle_share` when admission-blocked share exceeded 5% over the trailing hour. A temperature reading at the configured limit does not prove that a build was aborted; `thermal_abort` is corroborated by `aborted_thermal` in `ix_build_builds_total`. A reduction with none of these has empty `evidence_kind` and is a violation.
- **Step-down and cool-off.** `drain` with an overload reason writes a `draining_mark` event in the cycle of the overload and a `step_down` event whose `ceiling_until` is the cool-off end (default 6 hours). A `lane_added` event before `ceiling_until` that raises the lane count above the ceiling is a raise during cool-off. A `drain_done` event is written by the tick only when the lane's watcher state is current and not `working` and `increase(ix_fleet_lane_compile_seconds_total{host,lane,job="ix-relay-watcher"}[W])` is zero; W defaults to five minutes and is configurable. Current watcher state with no compile series means no build. A missing lane series is recorded as `lane_lost`; stale watcher state stays unavailable. A `lane_removed` event before `drain_done` is a removal before drain. `overload_event` rows are written by the tick from memory and thermal limit breaches.
- **Throttled share.** Until the dev-team FR-010 governor exists, throttled
  share is admission-blocked share only: the increase of
  `ix_build_admission_blocked_seconds_total` divided by the window length, with
  source basis `admission_only`. No governor removal is implied.
  `ix_build_pause_seconds_total` is absent until that producer exists; ix-board
  never queries it or substitutes zero. The JSON `prom.host_throttled_share`
  field carries its basis and coverage.
- **Host output samples.** Each tick writes one `host_output_sample` per host, with tickets closed, pull requests merged and commits pushed over the trailing hour, so a share can be computed on either basis. Lane-to-ticket mapping uses relay work events and ledger assignment records; pull requests and commits are attributed through a Linear issue attachment or a GitHub ticket identifier in title or branch. The source basis is retained on each current sample and on `host_output_share_of_recorded_level`; an unavailable current sample carries its reason. Items without a ticket mapping are counted as unattributed alongside the attributed host value, and do not invalidate that value. When no ticket mapping exists for the host, the observation is unavailable rather than zero.

### Prometheus series

The exporter ([FR-033](./FR-033-team-flow-effort-export.md)) SHALL serve, per host with bounded labels:
`ix_fleet_ready_buffer_deficit{host}` from the latest `saturation_sample` no older than 600 seconds,
`ix_fleet_ready_buffer_deficit_timestamp_seconds{host}` for that sample's timestamp, and
`ix_fleet_ready_buffer_deficit_unavailable{host,reason}` when the value is missing, stale, future-dated or has unavailable inputs; the value gauge is omitted in those cases and `reason` is bounded,
`ix_fleet_host_output_share_of_recorded_level{host}` (fraction from 0 to 1) and
`ix_fleet_host_output_share_of_recorded_level_timestamp_seconds{host}` from the current output sample,
`ix_fleet_capacity_recorded_level{host}` (the current `capacity_level.level`, omitted when none),
`ix_fleet_capacity_recorded_level_timestamp_seconds{host}`,
`ix_fleet_capacity_discovery_state{host,state}` (1 for the current state, one of `none`, `running`, `ended`) and
`ix_fleet_capacity_draining_lanes{host}`,
`ix_fleet_capacity_throttled_at_level{host}` (1 when the host has a recorded level and its 5-minute throttled share exceeded 5% in every 5-minute interval of the trailing hour, else 0),
`ix_fleet_capacity_throttled_without_step_down{host}` (1 when `throttled_at_level` is 1 and no `step_down` event of the host was recorded since that hour began, else 0), and
`ix_fleet_capacity_overload_events_total{host,cause}` (a counter of `overload_event` rows, `cause` one of `memory_below_reserve`, `thermal_limit_breach` or `thermal_abort`; a temperature breach is never called a scheduler abort).

The build scheduler and node-exporter/relay-watcher series named above are read
through pluggable measurement sources. The requirement owns no scheduler or
watcher behavior.

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
| `unrecorded_hosts_with_demand` | per-sample rows with complete ready-buffer, assignment and registry evidence and no capacity level at that sample time; positive-lane samples require `supplied_lanes + ready_share >= active_lanes`, while a current-lease zero-lane sample requires `bootstrap_ticket_pool >= 1` |
| `unrecorded_hosts_without_discovery_run` | mature `unrecorded_hosts_with_demand` sample rows with no run started during the following 10-minute cycle, no run open at sample time, fewer than two starts on that sample's UTC day by sample time, and no active cool-off at sample time; it is evaluated per sample over the query window |
| `unrecorded_hosts_ready_for_discovery` | current start candidates: each host's latest non-future `unrecorded_hosts_with_demand` sample no older than 600 seconds, with no current level, open run, second daily run or active cool-off |
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
| `host_output_share_of_recorded_level` | one row per host with a current level: the latest host output sample normalized by its `sample_interval_ms`, on the `output_basis` stored with the host's current `capacity_level`, divided by the level's exact evidence `step_no` output per hour, in percent; the row carries the sample's `source_basis` and `unavailable_reason` |
| `prom.hosts_throttled_at_recorded_level` | series `ix_fleet_capacity_throttled_at_level{host}` |
| `prom.throttled_hosts_without_step_down` | series `ix_fleet_capacity_throttled_without_step_down{host}` |
| `prom.reserve_breach_or_thermal_abort_events` | series `ix_fleet_capacity_overload_events_total{host,cause}`, from `overload_event` rows; cause includes `thermal_limit_breach` when the measured host temperature exceeds its configured limit |
| `prom.host_throttled_share` | throttled share as defined under Measurement |

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-041-AC-1 | Fixtures with known slots and engaged-time duty yield the hand-computed `lanes_needed`, target buffer and deficit without a lane limit; when an operator-entered `fleet.capacity_limits.<host>.lane_capacity` is present it bounds `lanes_needed`, while its absence never makes either value unavailable. | Test |
| FR-041-AC-2 | Ready tickets assigned to a reachable lane or started do not count toward the buffer; only ready, unassigned, unstarted tickets do. A current-lease zero-lane host meets the discovery-start gate when `max(0, ready_buffer - all_hosts_unsupplied_lanes) + queued_behind_first >= 1`; `queued_behind_first` counts distinct unfinished tickets after the first ticket in reachable lane assignment lists. This rule never covers a capacity-step window; a stale host is ineligible. | Test |
| FR-041-AC-8 | Tickets assigned to an UNREACHABLE lane still count in the ready buffer, while tickets assigned to a reachable lane do not. | Test |
| FR-041-AC-9 | A host with a stale registry lease contributes zero slots and zero lane capacity to supply, and its UNREACHABLE lanes are excluded from demand and from the duty mean. | Test |
| FR-041-AC-3 | A buffer at or above target gives deficit zero and no action; one below gives the exact shortfall. | Test |
| FR-041-AC-4 | A mean duty of zero, no lane with duty, missing sustainable slots or a missing queue makes dependent values `unavailable` with reason, omits the gauge, and never reports deficit zero; an absent optional lane cap does not. | Test |
| FR-041-AC-5 | With interim supply from configured build slots and watcher compiler seconds divided by observed non-idle lane seconds, idle time does not dilute duty and a positive deficit still yields `need_ready_tickets`; an optional operator lane limit bounds demand only when entered, and locked-build data replaces interim supply and duty when present. | Test |
| FR-041-AC-24 | A positive deficit yields one planner action with `project` and the requested additional ticket count persisted on its action row; the row's `emitted_at` is the first deficient tick and `resolved_at` is set only when the project deficit clears. | Test |
| FR-041-AC-6 | A deficit of N produces per-project actions whose counts sum to N, deterministically. | Test |
| FR-041-AC-7 | `ix_fleet_ready_buffer_deficit` has bounded labels, matches the read-only command output, and each `fleet saturation collect` run stores one `saturation_sample` per host with global ready buffer, active/supplied/unsupplied lane counts, allocated ready share, `queued_behind_first`, `bootstrap_ticket_pool`, current-lease evidence, source basis and unavailable reason; the exporter omits the value when the newest sample is older than 600 seconds and serves its timestamp and a bounded unavailable reason. | Test |
| FR-041-AC-10 | `fleet capacity window` on a fixture computes host output as tickets closed plus pull requests merged per hour over the post-ramp-up window with per-lane attribution by relay/ledger lane assignment and Linear PR attachment or GitHub ticket id; it records unattributed items without discarding attributed host output, and uses commits per hour with `output_basis` `commits` when the sum is under 5. | Test |
| FR-041-AC-11 | Each sample stores global ready buffer, active, supplied and unsupplied lane counts, ready share, bootstrap ticket pool and current registry evidence. Any assigned ticket outside Completed, Canceled or Duplicate supplies its reachable lane, including backlog state types. Supplied lanes plus ready share cover a positive-lane step; a current-lease zero-lane host may start discovery when `max(0, ready_buffer - all_hosts_unsupplied_lanes) + queued_behind_first >= 1`, but zero-lane bootstrap never covers a step window. A window is valid only when every sample has complete demand evidence and covers demand and covering sample intervals span at least 90%; duplicate or clumped timestamps do not inflate coverage. Missing evidence is `deficit_coverage`, short demand is `demand_short`. Deficit availability, duty and `lanes_needed` do not affect demand coverage. Positive deficit is compatible with a valid window. The repeated hold keeps its `step_no` with `repeat_of`, and `discovery_steps_violating_ramp` does not list it. The current discovery-start view requires a latest sample no older than 600 seconds, no current run, fewer than two starts on the current UTC day and no active cool-off. The evaluator violation view is per sample and allows one 10-minute cycle for a start. | Test |
| FR-041-AC-18 | A step that follows an invalid window with a new `step_no`, a `knee_signal` on an invalid step, and a `capacity_level` whose evidence includes an invalid step are each listed by `discovery_steps_judged_without_demand`; a correctly repeated step is not. | Test |
| FR-041-AC-12 | `knee_signal` is set at an output gain just under and not at just over 50% of the previous step's per-lane mean, and when throttled share exceeds 5%; `slots_busy_equals_sustainable` is true only when busy equalled sustainable throughout. Missing slot evidence does not invalidate the window or block an output knee or step bound; `build_slots_saturated` is available only when both slot values are available and equal throughout. | Test |
| FR-041-AC-13 | `end` writes `found_level` and a `capacity_level` row with evidence steps and the step's `output_basis`, and sets `superseded_at` on the previous row, keeping both; a run ending `inconclusive_no_demand` with no new level writes no row and is not listed by `discovery_runs_without_recorded_level`, while an ended run with a `found_level` and no row is; `end inconclusive_no_demand` is refused before two consecutive inconclusive steps, `step_bound` before 8 steps, and `build_slots_saturated` while either slot value is unavailable or busy and sustainable slots did not remain equal through the valid step. | Test |
| FR-041-AC-14 | `start` is refused for a host with an open run, for a third run on one UTC day, and within the cool-off; a `drain` for `memory_below_reserve` writes a `draining_mark` and a `step_down` with `ceiling_until` six hours later, and a `lane_added` event before it that raises the lane count is listed by `raises_during_cool_off`. | Test |
| FR-041-AC-19 | A host's non-draining `lane_tick` count rising or falling between ticks writes a `lane_added` or `lane_removed` event carrying the reason of a `fleet capacity` command issued within one tick interval before it, and `lane_change_unrecorded` otherwise, which `lane_change_unrecorded` and `lane_reductions_without_throttle_evidence` list. | Test |
| FR-041-AC-22 | A `lane_removed` after a `draining_mark` of the same lane carries that mark's `reason` and `evidence_kind` and is not listed by `lane_reductions_without_throttle_evidence` when the mark was corroborated. | Test |
| FR-041-AC-23 | A lane that vanishes after a tick in which its class was `DEAD`, or `UNREACHABLE`, with no `draining_mark` or command, is a `lane_lost` event: it is listed by `lanes_lost`, and absent from `lane_count_reductions`, `lane_change_unrecorded` and `lane_reductions_without_throttle_evidence`; a disappearing `WORKING` lane with no mark or command is `lane_removed` with reason `lane_change_unrecorded`. | Test |
| FR-041-AC-20 | `evidence_kind` is set by ix-board: a `drain --reason thermal_abort` with no admission-block or `aborted_` build outcome in the preceding 15 minutes has empty `evidence_kind` and is listed by `lane_reductions_without_throttle_evidence`; the same drain after admission-block growth has `admission_block`, or after an `aborted_` build outcome alone has `build_abort`, and in either case is not listed. No `ix_build_pause_seconds_total` series is queried or represented as zero before its producer exists. | Test |
| FR-041-AC-15 | `drain_done` is recorded only when the drain-marked lane has a current watcher state other than `working` and no compile increase during the configured build window (default 5 minutes); a current non-working state with no compile series proves no build, a missing lane is `lane_lost`, and stale watcher evidence remains unavailable. A `lane_removed` event before drain completion is listed by `lane_removals_before_drain`; a drain older than the drain cap ([FR-038](./FR-038-fleet-tick.md), default 4h) recorded on its `draining_mark` with no sent `escalate_drain` action (`action_send`) is listed by `drains_over_cap_without_escalation`. | Test |
| FR-041-AC-16 | The exporter serves `ix_fleet_capacity_recorded_level{host}`, its timestamp series, `ix_fleet_capacity_discovery_state{host,state}`, `ix_fleet_capacity_draining_lanes{host}`, `ix_fleet_capacity_throttled_at_level{host}`, `ix_fleet_capacity_throttled_without_step_down{host}` and `ix_fleet_capacity_overload_events_total{host,cause}` with labels bounded by host, state and cause, omits the level for a host with none, and the values equal the ledger; a host throttled above 5% in every interval of an hour with no `step_down` shows `throttled_without_step_down` 1, and 0 after a `step_down`. | Test |
| FR-041-AC-17 | Every capacity view and `prom.` name in the role-spec query table exists under its name and returns the hand-computed value on a fixture covering a knee run, a no-demand run, a slot-saturated run and an overload step-down. | Test |
| FR-041-AC-21 | `host_output_share_of_recorded_level` uses the `output_basis` stored with the recorded level for both the numerator and the denominator: a level found on `commits` is compared with commits per hour, a level on `tickets_prs` with tickets closed plus pull requests merged; one row names the exact evidence `step_no` per host, includes `sample_interval_ms`, and retains current sample `source_basis` and `unavailable_reason`. Prometheus exports the share and its sample timestamp. | Test |
| FR-041-AC-25 | The pull-request reader sends a User-Agent, maps lane-to-ticket from current relay work events and ledger assignment history, uses Linear issue attachments as authoritative PR links, and scans configured repositories for open PRs whose title or head branch contains a mapped ticket identifier. An attached PR remains attributed when title and branch omit the identifier. Unattributed PRs do not invalidate mapped host output; a missing mapping or incomplete repository scan is unavailable, not zero. An HTTP fixture exercises both the attachment and title/branch reader paths. | Test |

## Dependencies

- Final supply and duty: locked-build scheduler (W6). Interim supply: explicit per-host `fleet.capacity_limits.<host>.build_slots`; interim duty: the ix-relay watcher per-lane compiler seconds divided by observed non-idle lane-state seconds (ix-relay PR #52, target contract). An optional lane-demand bound comes only from explicit `fleet.capacity_limits.<host>.lane_capacity`. Lane build evidence also uses [FR-034](./FR-034-fleet-lane-status.md).
- Capacity discovery reads `ix_build_queue_wait_seconds`, `ix_build_slots_busy`, `ix_build_slots_configured`, `ix_build_admission_blocked_seconds_total` and `ix_build_builds_total{outcome}` from dev-team FR-010/FR-015; interim sustainable slots from explicit per-host configuration; memory and thermal readings and explicit per-host limits as described above; Linear/GitHub for host output; [FR-037](./FR-037-fleet-focus-and-drift.md) focus for per-lane attribution; and the [FR-039](./FR-039-fleet-ledger.md) ledger. `fleet tick` ([FR-038](./FR-038-fleet-tick.md)) writes `drain_done`, `overload_event`, `lane_added`, `lane_removed` and `host_output_sample`. These producers are enablement owned by their respective projects; this requirement adds none.
- `ix_build_builds_total{outcome}` uses the closed outcome set `ok`, `failed`, `cancelled`, `timed_out`, `aborted_memory`, `aborted_thermal`, `aborted_rss_cap` and `aborted_pause_exceeded` from dev-team FR-015. FR-010 termination reason values already include the `aborted_` prefix; producers and consumers use each value once, without adding a second prefix.
- The dev-team orchestrator role spec still names `ix_build_slots_sustainable`, `ix_build_pause_seconds_total` and `ix_build_probe_value`; SWM-317 replaces them with `ix_build_slots_configured` and the admission-blocked share.
