---
id: FR-034
title: Classify every registered agent lane from fused evidence
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-018"
    type: depends_on
---
# FR-034: Classify every registered agent lane from fused evidence

## Description

The `ix-board fleet status [--json]` command SHALL emit exactly one row for every
registered lane, each with one class: `WORKING`, `IDLE`, `BLOCKED(reason)`, `LOOP`,
`STUCK`, `STALE-ORDER`, `DEAD` or `UNREACHABLE`. A lane is an agent session addressed by the relay
address `server:workspace:tab`. The command SHALL compute classes from structured
evidence and SHALL NOT use an LLM or a lane's own message as primary evidence.
The command SHALL NOT emit `UNKNOWN` as a class.

## Inputs

- **Join key.** The relay address `server:workspace:tab` is the lane identity in all
  three sources below; a record without a resolvable address is a source defect.
- **Relay Redis, read through relay-core:** hook events, order receipts and the
  ordered per-lane assignment list. Event kinds and fields follow the ix-relay
  contract: kinds `pre_tool`, `post_tool`, `prompt`, `blocked{reason}`, `idle`,
  `stop{last_class}`, `session_start`, `session_end`; fields `addr`, `harness`,
  `ts` (milliseconds, relay server time). Receipt states include `Delivered`,
  `Acted` and `AckExpired`; ix-board derives `timed_out` as an `AckExpired` receipt with no
  `Delivered` receipt. The lane registry is the fleet-wide registry assembled from
  leased per-host lists; a lane on an expired host is marked `stale`.
- **Prometheus HTTP API:** the watcher series, namely the relay-resolved `ix_fleet_lane_blocked{reason}`
  per lane (hook and pane evidence already combined by the relay, with reasons
  including `dialog`, `login`, `usage_limit`, `question`, `permission` and `wait_loop`),
  per-lane compiler seconds, `ix_fleet_probe_ok{host,source}`, host busy ratio and capacity.
- **herdr and git/GitHub:** herdr agent state and title, and pull-request head
  movements and commits on the branch of the lane's worktree.
- The [FR-018](./FR-018-agent-activity-verdicts.md) agent verdict where readable.
- **Working window.** ix-board SHALL read the working window from the ix-relay
  configuration (default 10m, configured once) and SHALL NOT hold its own constant.
  ix-board SHALL likewise read `max_tool_runtime` (default 2h) and `no_events_after`
  from the relay `ix.fleet.config` and SHALL hold no local copy of either.
  Other thresholds are ix-board configuration with defaults: `loop_stops` 3,
  `dead_after` 15m, `max_clock_skew` 30s.
- **Time.** The command SHALL use relay server time for every event comparison.
  Host-local clocks are not used for hook ages.

## Behavior

The command evaluates classes in this order and emits the first that matches:

0. `UNREACHABLE`: the relay marks the lane `stale` because its host registry lease has
   expired. The class is never IDLE, counts as not working for capacity, and the row
   carries a `stale_host_registry` defect.
1. `DEAD`: the latest event is `session_end`, or the lane has no event, no herdr
   pane and a registry entry older than `dead_after`.
2. `BLOCKED(reason)`: the relay-resolved `ix_fleet_lane_blocked{reason}` for the lane
   gives the reason: `dialog` (relay reasons `dialog` and `permission`), `login`,
   `usage_limit` or `question` (relay reasons `question` and `queued_question`). ix-board SHALL NOT apply its own precedence between
   hook and pane evidence; the relay resolution is the single source of truth.
3. `STALE-ORDER`: an order to the lane has an `AckExpired` receipt, including
   `timed_out` (no `Delivered` receipt).
4. `STUCK`: a `pre_tool` event has no matching `post_tool` for longer than `max_tool_runtime`.
5. `LOOP`: the relay series shows reason `wait_loop`, or the lane has at least `loop_stops` `stop`
   events with no `pre_tool` event between them within the window.
6. `WORKING`: a `prompt`, `pre_tool` or `post_tool` event is younger than the working
   window, or a `pre_tool` event without a matching `post_tool` is younger than
   `max_tool_runtime`. Hook evidence alone is sufficient. When a lane has no hook
   evidence in the window, `WORKING` requires at least two agreeing weak signals
   from distinct sources among herdr title, pane text and self-report; a single
   weak signal is never sufficient.
7. `IDLE`: otherwise.

When a lane's evidence is missing, stale or failed, the command SHALL class the lane
by these rules using the evidence available and SHALL add a source-defect row naming source,
host, lane, reason and first-seen time. The defect kinds are `missing_lane`,
`missing_host`, `no_events` (a registered lane with no event for `no_events_after`),
`stale_source` (a source older than twice its scrape or push interval),
`clock_skew` (relay server time and the ix-board clock differ by more than
`max_clock_skew`) and `unreadable_source` and `stale_host_registry`. A report with any defect SHALL NOT
state that all hosts or lanes are in target, and its JSON carries `complete: false`.

Each row carries lane, host, class, reason, state age, last event time, the
evidence items used with timestamps and sources, the focus
([FR-037](./FR-037-fleet-focus-and-drift.md)) and the evidence-schema version.
A failing host SHALL NOT remove the rows of other hosts.
The command SHALL bound each host read by a per-host timeout.
The command SHALL exit non-zero when any defect row exists. ix-board classification
is authoritative for fleet decisions; the relay lane-state series is advisory.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-034-AC-1 | A fixture with N registered lanes across hosts yields exactly N rows, each with one of the eight defined classes (`WORKING`, `IDLE`, `BLOCKED(reason)`, `LOOP`, `STUCK`, `STALE-ORDER`, `DEAD`, `UNREACHABLE`); no evidence combination in the matrix yields UNKNOWN. | Test |
| FR-034-AC-2 | A `pre_tool`, `post_tool` or `prompt` event inside the working window yields WORKING with no compile, PR or commit signal; one older than the window yields IDLE; no `IDLE(unconfirmed_activity)` subreason exists. | Test |
| FR-034-AC-3 | A lane with no hook evidence is WORKING only with two agreeing weak signals from distinct sources; each single weak signal alone yields IDLE. | Test |
| FR-034-AC-4 | An open `pre_tool` is WORKING until `max_tool_runtime` and STUCK after it; a matching `post_tool` closes it. | Test |
| FR-034-AC-5 | The relay series reason `wait_loop`, and `loop_stops` stop events with no tool event between them, each yield LOOP; fewer stops or an interleaved `pre_tool` do not. | Test |
| FR-034-AC-6 | The mapping fixtures give relay `ix_fleet_lane_blocked` reasons `dialog` and `permission` BLOCKED(dialog), `question` and `queued_question` BLOCKED(question), `login` and `usage_limit` their reasons, and a `session_end` DEAD. | Test |
| FR-034-AC-7 | With the relay series reporting a reason that disagrees with raw hook or pane fixtures, the class follows the relay series; the code holds no hook-versus-pane precedence rule (an inspection of the source finds none). | Test |
| FR-034-AC-8 | An order with `AckExpired` and a `Delivered` receipt, and one with `AckExpired` and no `Delivered` receipt (`timed_out`), each yield STALE-ORDER. | Test |
| FR-034-AC-9 | A lane with no event for the relay `no_events_after` is IDLE with a `no_events` defect; a lane the relay marks `stale` is `UNREACHABLE` with a `stale_host_registry` defect, never IDLE, and counts as not working in capacity figures; a failed probe, unreadable source, or a source older than twice its interval adds the matching defect while the lane is still emitted. | Test |
| FR-034-AC-10 | A configured lane with no registry record and a configured host with no lanes give `missing_lane`/`missing_host` defects, `complete` false and a non-zero exit. | Test |
| FR-034-AC-11 | The working window equals the value read from the relay configuration; a fixture changing it changes classification and the code holds no second constant (an inspection of the source finds one definition). | Test |
| FR-034-AC-16 | `max_tool_runtime` and `no_events_after` equal the relay `ix.fleet.config` values and ix-board configuration rejects local overrides of them. | Test |
| FR-034-AC-12 | Boundary fixtures at exactly and one second beyond the working window, relay-configured `max_tool_runtime`, `dead_after` and the order deadline classify as specified; configured values override the defaults of the ix-board-owned thresholds (`loop_stops`, `dead_after`, `max_clock_skew`) only, and the relay-owned values (working window, `max_tool_runtime`, `no_events_after`) are read from the relay and cannot be overridden. | Test |
| FR-034-AC-13 | A relay-to-ix-board clock difference above `max_clock_skew` raises `clock_skew` and every age is still computed from relay server time; host-local timestamps never enter an age. | Test |
| FR-034-AC-14 | One host timing out leaves other hosts' rows unchanged and adds an `unreadable_source` defect within the per-host bound. | Test |
| FR-034-AC-15 | Text and `--json` carry the same rows, defects and `complete`; two runs on identical evidence give identical JSON once run-time-dependent fields (run timestamp and state ages) are masked. | Test |

## Dependencies

- ix-relay PR #52 (d6e79fb: fleet-wide lane registry from leased per-host lists, `ix.fleet.config`): hook events with the kinds and fields above, `Delivered`/`Acted`/`AckExpired` receipts, the resolved `ix_fleet_lane_blocked{reason}` including `wait_loop`, per-lane compiler seconds, `ix_fleet_probe_ok`, and `ix_fleet_lane_state` without `unknown`. A divergence changes this requirement.
- The ordered per-lane assignment list is owned by a new ix-relay requirement in PR #52 and is a dependency of [FR-035](./FR-035-fleet-assign.md).
- [FR-018](./FR-018-agent-activity-verdicts.md) supplies the agent verdict; [FR-039](./FR-039-fleet-ledger.md) records classifications.
