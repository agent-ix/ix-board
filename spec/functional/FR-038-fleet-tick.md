---
id: FR-038
title: Emit a deterministic action list for the minder
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-035"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-041"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
---
# FR-038: Emit a deterministic action list for the minder

## Description

The `ix-board fleet tick --json` command SHALL evaluate the fleet state once and
return an ordered list of actions for an agent minder to execute. The minder
executes the list and does not re-derive it. The command SHALL NOT perform any
action except to record the tick and its emitted actions in the ledger
([FR-039](./FR-039-fleet-ledger.md)).

## Behavior

Each action has `id`, `kind`, target lane or project, `reason`, evidence
references, `severity` and `due_by`. An action id stays stable across ticks while
its condition persists. Kinds and rules:

| Kind | Condition | Target |
|------|-----------|--------|
| `push_next_ticket` | lane is IDLE with a remaining assigned ticket; carries the first ticket of the lane's ix-relay FR-072 list and a reference to that list, leaving the list unchanged (the minder sends a plain order, not `fleet assign`) | the lane |
| `answer_dialog` | lane is BLOCKED(dialog), or BLOCKED(question) with relay reason `queued_question`, and a standing order in the SWM-226 configuration file matches it; carries the relay reason, and the order id and answer once the file format specifies them | the lane |
| `escalate_block` | lane is BLOCKED(login), BLOCKED(usage_limit) or BLOCKED(question); or BLOCKED(dialog) with no matching standing order; or the SWM-226 file is missing or unreadable. Severity high; carries the block reason | the lane's lead |
| `escalate_stuck` | lane is STUCK or LOOP | the lane's lead |
| `resend_order` | the lane's order is STALE-ORDER | the lane |
| `add_lane` | a host's fresh busy ratio is below its target busy ratio (per-host configuration, default 0.85), ready unassigned tickets exist and a lane slot is free. Busy ratio comes from `ix_fleet_host_busy_ratio{host}` in the bounded per-host FR-034 read. Missing, invalid, duplicate or stale readings create a host-scoped `prometheus.host_busy` defect and make only that host ineligible. Ready-work reading is owned by SWM-212. | the host |
| `escalate_stale_pr` | a lane-owned pull request has had no head or review activity longer than `stale_pr_after` (configurable, default 3h). The pull-request activity reader is owned by SWM-282; until it lands the tick reports it as `unavailable` | the lane's lead |
| `need_ready_tickets` | ready-buffer deficit greater than zero ([FR-041](./FR-041-fleet-saturation.md)). **Deferred to SWM-217**: the tick does not emit it yet | the owning planner project |
| `act_on_undetermined` | a reachable lane, on a host that is not `UNREACHABLE`, has a lane-scoped evidence defect: `no_events`, or a `missing_lane`, `stale_source` or `unreadable_source` defect naming that lane. Carries the defect, the lane's remaining assigned ticket if any, and `treat_as: idle`. A relay message to a BLOCKED pane is pane input, so for a lane whose class is BLOCKED the action is addressed to the lane's lead instead, with reason `lane_blocked_route_to_lead` | the lane, or its lead when the lane is BLOCKED |
| `ladder_rung` | a condition with a ladder persists past a rung time (below). Carries the parent action id, `rung` 1 to 3 and the recipient | the worker, the lane's lead or the orchestrator per rung |
| `escalate_drain` | a lane marked draining ([FR-041](./FR-041-fleet-saturation.md)) is still draining after the drain cap (configurable, default 4h) | the lane's lead |
| `fix_source` | a source-defect row exists, emitted in the tick in which it first appears. A source the tick reads itself (the FR-072 assignment lists) that fails to read is such a defect (source `tick.<source>`); a source with no reader yet (a seam named above) is not | the orchestrator, who fixes it or routes it to the owning repository |
| `route_dead_lane` | lane is DEAD; carries its status evidence | the orchestrator |
| `route_idle_lane` | lane is IDLE and its FR-072 assignment list is readable and empty; carries its status evidence | the orchestrator |

Actions are sorted by severity, lane and kind. The output for identical evidence is
identical. A lane with an unresolved defect is not excluded. An unreadable
SWM-226 configuration never produces `answer_dialog`.

The busy-ratio input is read once per host with the FR-034 watcher series. A
ratio is usable only when exactly one finite sample in `[0, 1]` exists and its
age is no more than twice the configured push interval. A missing, malformed,
duplicate or stale ratio records a host-scoped `prometheus.host_busy` defect;
the tick keeps other hosts' valid ratios and status evidence. It reuses the
status read and does not issue another Prometheus query for busy ratios.

### Undetermined lanes

A lane whose own evidence is missing or defective is a per-lane condition, distinct from a host-wide one. A host whose registry lease is stale keeps its lanes `UNREACHABLE` ([FR-034](./FR-034-fleet-lane-status.md)) and is handled through `fix_source`; it never produces `act_on_undetermined`. For a reachable host, a lane with a lane-scoped defect still has a class from [FR-034](./FR-034-fleet-lane-status.md), never `UNKNOWN`, and also gets one `act_on_undetermined` action, which the minder executes by treating the lane as idle. Its expected state, for `executed_at` ([FR-039](./FR-039-fleet-ledger.md)), is the lane `WORKING` or the defect cleared. The action resolves when the lane has no lane-scoped defect.

### Standing-order seam

The machine-readable format of the SWM-226 standing-order file and its match conditions are not yet specified. Until they are, a readable file is the match: the tick emits `answer_dialog` carrying the relay reason and no order id for a BLOCKED(dialog) or a `queued_question`, and the minder selects the order from the standing-orders reference. A missing or unreadable file emits `escalate_block`, and a free-form question (relay reason `question`) always emits `escalate_block`.

### In-fleet escalation ladder

The tick owns one in-fleet ladder; the owner is paged only by Alertmanager and the tick emits no action addressed to the owner, so no condition is paged twice. A condition is an action of kind `push_next_ticket`, `resend_order`, `act_on_undetermined`, `answer_dialog`, `escalate_block`, `escalate_stuck`, `escalate_stale_pr`, `escalate_drain`, `route_dead_lane` or `route_idle_lane`; its `condition_started_at` is the relay time of the tick that first emitted it, kept while its action id persists, including across a gap in ticks.

- **Rung 0** is the condition's own action, sent to that action's target in the same tick.
- **Rungs 1 to 3.** While the condition persists the tick emits a `ladder_rung` action to the worker (rung 1), the lane's lead (rung 2) and the orchestrator (rung 3), each in the first committed tick at or after 1, 2 and 3 tick intervals from `condition_started_at`. Each is emitted once. A condition on a lane whose class is BLOCKED skips rung 1: a relay message delivered to a blocked pane is pane input, so the tick sends no relay message to it, and rungs 2 and 3 follow at their usual times. A rung is emitted even when its recipient already received an earlier rung, and its `due_by` is the next tick interval. Its latency from the rung time is therefore at most one tick interval.
- **The ladder ends** in the tick after the condition clears; no further rung is emitted.
- **Owners.** `escalate_stuck` goes to the lane's lead, `escalate_stale_pr` to the lead of the lane that owns the pull request, and `escalate_drain` to the lane's lead, taken from the lane-to-lead configuration. A lane with no configured lead gives the action the orchestrator as its owner and the reason `lead_unresolved`. `fix_source` goes to the orchestrator; nothing the tick emits is addressed to the owner.
- **No lead for rung 2.** When the lane's lead cannot be resolved the tick does not address rung 2 to a missing recipient: at the rung 2 time it records a rung 2 row with reason `lead_unresolved` and emits rung 3 to the orchestrator in that same tick with reason `lead_unresolved_fallthrough`, instead of at 3 tick intervals. It is not emitted twice.
- **Recording.** Every rung 0 to 3 is an `action` row with `rung`, `parent_action_id`, `target_role`, `target_addr` and `condition_started_at`; the `escalation_rung` view of [FR-039](./FR-039-fleet-ledger.md) joins it to the relay send.

### `add_lane` suppression

The tick emits no `add_lane` for a host in any of these states and records each suppression in its output and in the ledger with the reason: `discovery_active` (a capacity run is open for the host, [FR-041](./FR-041-fleet-saturation.md)), `no_recorded_level` (the host has no recorded sustainable level), `at_recorded_level` (the host's non-draining lane count is at or above its recorded level) or `cool_off` (within the hysteresis ceiling after a step-down). Otherwise the existing `add_lane` condition applies. A draining lane counts toward neither the lane count nor a free slot.

The exporter ([FR-033](./FR-033-team-flow-effort-export.md)) SHALL read the last committed tick in the ledger as the source of the metrics `ix_fleet_tick_timestamp_seconds` and `ix_fleet_tick_actions{kind}`, and omits them when
the last tick is older than twice the configured tick interval, so an external alert sees a missing tick. A tick that cannot evaluate emits an error and commits
no tick record.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-038-AC-1 | Each action kind except the deferred `need_ready_tickets` is emitted by its fixture condition and not by its negation, with boundaries at exactly `stale_pr_after` and the busy-ratio target (the order deadline is the relay's: the tick acts on the relay's `AckExpired`). | Test |
| FR-038-AC-2 | Identical inputs give byte-identical action lists in the specified order, and ids persist across two ticks while the condition persists. | Test |
| FR-038-AC-3 | `answer_dialog` is emitted only with a matching SWM-226 standing order; no match, a missing file and an unreadable file each emit `escalate_block` and no `answer_dialog`. | Test |
| FR-038-AC-4 | BLOCKED login, usage_limit and question emit `escalate_block` targeted at the lane's lead with severity high; STUCK and LOOP emit `escalate_stuck`. | Test |
| FR-038-AC-5 | `stale_pr_after` default is 3h, a configured value overrides it, and the action targets the PR's lane lead. | Test |
| FR-038-AC-6 | A host below its configured busy-ratio target (default 0.85) with unassigned ready work and a free slot emits `add_lane`; at the target it does not. | Test |
| FR-038-AC-7 | The tick metric reflects the ledger's last committed tick, is omitted when older than twice the tick interval, and a failed tick commits no record. | Test |
| FR-038-AC-8 | `tick` makes no relay, Linear or GitHub write; a recording fake sees none. | Test |
| FR-038-AC-9 | A source defect appears as `fix_source` in the tick in which it first appears, keeps its action id while it persists, and the lane's other actions are still emitted. | Test |
| FR-038-AC-10 | A lane with a `no_events` defect on a reachable host emits one `act_on_undetermined` action targeted at the lane (its lead when the lane is BLOCKED, AC-21) with `treat_as: idle`, keeps its FR-034 class, and a lane of a host with a stale registry lease emits `fix_source` and no `act_on_undetermined`. | Test |
| FR-038-AC-11 | A persisting `escalate_stuck` condition emits rung 1 to the lane, rung 2 to its lead and rung 3 to the orchestrator in the first tick at or after 1, 2 and 3 tick intervals from the first emission, each once, at a 5 minute and at a 2 minute interval; the action id and `condition_started_at` persist across a missed tick. | Test |
| FR-038-AC-12 | A rung is emitted although the same recipient already received an earlier rung or the rung 0 action; once the condition clears no further rung is emitted; no emitted action has an owner as recipient. | Test |
| FR-038-AC-13 | `escalate_stuck` and `escalate_stale_pr` carry the lane's lead and the pull-request lane's lead as owner; a lane with no configured lead gives owner orchestrator with reason `lead_unresolved`. | Test |
| FR-038-AC-16 | A lane with no resolvable lead gets a rung 2 row with reason `lead_unresolved` and rung 3 to the orchestrator emitted in the same tick, not again at 3 intervals; `fix_source` is addressed to the orchestrator and no emitted action has `target_role` other than `lane`, `lead`, `orchestrator`, `planner` or `host`. | Test |
| FR-038-AC-14 | `add_lane` is not emitted for a host in an open capacity run, a host with no recorded level, a host at its recorded level or within its cool-off, each recorded with its reason; a host below its recorded level with ready work and a free slot emits it, and a draining lane counts as neither a lane nor a free slot. | Test |
| FR-038-AC-15 | A lane draining longer than the drain cap emits `escalate_drain` to the lane's lead and one still within the cap does not. | Test |
| FR-038-AC-17 | A condition on a BLOCKED lane emits no rung 1 and no relay message to the lane, and rungs 2 and 3 are emitted at 2 and 3 tick intervals; the same condition on a lane that is not BLOCKED emits rung 1. | Test |
| FR-038-AC-18 | BLOCKED(question) with relay reason `queued_question` emits `answer_dialog` carrying that reason when the standing-order file is readable, and `escalate_block` when it is missing or unreadable; relay reason `question` emits `escalate_block` in every case. | Test |
| FR-038-AC-19 | A DEAD lane emits `route_dead_lane` to the orchestrator as a rung 0 action with its ladder, and a WORKING lane does not. | Test |
| FR-038-AC-20 | An IDLE lane whose readable FR-072 list is empty emits `route_idle_lane` to the orchestrator; an IDLE lane with a ticket emits `push_next_ticket`; an unreadable list emits neither. | Test |
| FR-038-AC-21 | `act_on_undetermined` for a BLOCKED lane is addressed to the lane's lead and never to the lane; for any other class it is addressed to the lane. | Test |
| FR-038-AC-22 | A failed read of a source the tick reads itself is a source defect in its own tick (`complete` false, one `fix_source` to the orchestrator, closed when the read works again), and a source with no reader yet raises no defect. | Test |
| FR-038-AC-23 | The tick reuses bounded status host-series reads for busy ratios; missing, invalid, duplicate or older-than-two-push-interval ratios produce a defect for that host and do not discard another host's valid ratio or status evidence. | Test |

## Dependencies

- The minder sends `escalate_drain` as a tick action; that discharges the orchestrator's drain-cap escalation (orchestrator role spec ORCH-R30), and the role spec is to be updated to say so.
- [FR-034](./FR-034-fleet-lane-status.md), [FR-035](./FR-035-fleet-assign.md), [FR-039](./FR-039-fleet-ledger.md), [FR-041](./FR-041-fleet-saturation.md). Standing orders come from the SWM-226 configuration file. Host busy ratio and capacity come from the ix-relay watcher series (Prometheus HTTP API).
