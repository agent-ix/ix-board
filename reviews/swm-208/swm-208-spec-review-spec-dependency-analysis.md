---
id: SR-6105
title: "spec-review/spec-dependency-analysis of ix-board fleet spec against ix-relay PR52 (PR86, SWM-208)"
type: SpecReview
analysis: dependency
scope: "agent-ix/ix-board@31ad309efb0118fa89173ee7a17dfa4eb4048fa8; spec/functional/FR-034..FR-043; contract source agent-ix/ix-relay PR52@e93d136 (FR-045, FR-067, FR-068, FR-069, FR-070, FR-071)"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-035"
    type: reviews
---

# SR-6105: ix-board fleet dependency and relay-contract analysis

## Summary

Ticket: SWM-208. Checks each ix-relay contract the fleet FRs assume against
what ix-relay PR #52 (head e93d136) actually specifies, plus `agent-msg work
agent` on ix-relay main. Also checks the external dependency on the unauthored
ix-build scheduler.

## Verdict

Changes requested. Series names match: ix-board uses the Prometheus
underscore names (`ix_fleet_probe_ok`) that PR #52 declares for its dotted
OTLP names. `Acted` and `TimedOut` names match. Several assumed contracts do
not exist or differ:

- An ordered per-lane `work agent` list is in neither PR #52 nor main.
- Per-lane compiler attribution and `wait_loop` are not exported.
- Event-kind and pane-class vocabularies differ.
- The never-delivered and order-only `TimedOut` semantics are not reflected.
- The relay's own `unknown` and 5-minute `working` lane state has no stated mapping.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-035 stores an ordered ticket list per lane in relay `work agent`. On ix-relay main, `work agent` is a single-ticket self-report (`<ID> --ticket --role --status`, "this session's bounded report"), and PR #52 does not extend it. The assumed write interface does not exist and has no ix-relay spec owner. | spec/functional/FR-035-fleet-assign.md:17-24 |
| FND-002 | high | FR-034 LOOP needs per-lane pane `wait_loop`, and FR-041 duty needs per-lane compiler attribution. PR #52 classifies both inside the collector (FR-069) but exports only `ix_fleet_lane_blocked{reason=dialog,login,usage_limit,question}` and no per-lane compiler series. No read path gives ix-board this evidence. | spec/functional/FR-034-fleet-lane-status.md:28-33 |
| FND-003 | medium | FR-034 event kinds ("tool-use, user-prompt, permission-prompt notification, idle-prompt notification, stop, session-end") and field "lane address" differ from PR #52 FR-068 (`pre_tool`, `post_tool`, `prompt`, `blocked` with `reason=permission`, `idle`, `stop` with `last_class`, `session_start`, `session_end`; `addr`, `harness`, `ts` in ms). No mapping is given for `blocked/permission` to BLOCKED(dialog) or `stop last_class=question` to BLOCKED(question). | spec/functional/FR-034-fleet-lane-status.md:24-27 |
| FND-004 | medium | FR-034 pane classes ("dialog, login, usage limit, question, queued-input, wait-loop") and its BLOCKED rule's pane `question` class do not match FR-069 (`dialog`, `login`, `usage_limit`, `queued_question`, `wait_loop`, `none`). The precedence rules also differ: FR-069 says a newer hook `blocked`/`idle` event beats the pane, while FR-034 compares with a "later hook tool event". | spec/functional/FR-034-fleet-lane-status.md:49-51 |
| FND-005 | medium | FR-034 STALE-ORDER covers only `Delivered` without `Acted`. Under FR-071 a never-delivered order times out from enqueue, and `Acted`/`TimedOut` exist only for messages sent with an ack deadline. FR-035 never says assign sends an order with a deadline, or how `--timeout` relates to the relay deadline. FR-067 also says `Acted` means the session acted, not that it acted on this message. | spec/functional/FR-034-fleet-lane-status.md:51-52 |
| FND-006 | medium | PR #52 exports `ix_fleet_lane_state` with `working` (hook activity within 5 min), `idle`, `blocked`, `ended` and `unknown`. FR-034 never says whether it reads this series. If it does, `unknown` and `ended` need a mapping (to IDLE evidence_missing plus a defect, and to DEAD). Either way, two "working" definitions (5m relay, 10m `working_window`) will disagree on dashboards and alerts. | spec/functional/FR-034-fleet-lane-status.md:44-46 |
| FND-007 | medium | The read path for watcher evidence is unstated. PR #52 pushes `host.busy_ratio`, `capacity_slots`, `probe_ok` and `lane.blocked` over OTLP only, while FR-034/FR-038 read them as inputs and never say whether ix-board queries Prometheus or Redis, or the staleness rule. The lane join key (relay `addr` server:workspace:tab, the series `lane` label, herdr agent ids) is not defined. | spec/functional/FR-038-fleet-tick.md:62 |
| FND-008 | medium | FR-041 supply depends on an unauthored ix-build scheduler (W6). Until it exists, deficit is always unavailable and `need_ready_tickets` never fires, so the saturation headline is inert. No interim supply (for example `lane_capacity` alone) or staging is stated. | spec/functional/FR-041-fleet-saturation.md:21-23 |

## Scope

- FR-034, FR-034-AC-2, FR-034-AC-5 (examined)
- FR-035, FR-035-AC-1, FR-035-AC-3 (examined)
- FR-037 (examined; `cwd`/`branch` fields match FR-068, clean)
- FR-038 (examined)
- FR-040 (examined; stop/idle kinds exist in FR-068, clean)
- FR-041 (examined)
- FR-043 (examined; replacement map consistent with FR-069 Replaces list, clean)
- ix-relay PR52 FR-045, FR-067, FR-068, FR-069, FR-070, FR-071 (context_only)

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-009 | medium | ix-relay PR52 at f1f1015 has no `TimedOut` receipt. FR-071 records `AckExpired` both for delivered orders and for never-delivered orders (measured from enqueue). FR-035 step 3, step 5 and AC-6 expect "a never-delivered TimedOut receipt" printing `timed_out`, and FR-034 Inputs, AC-8, Dependencies and spec.md still name `TimedOut`. ix-board must derive `timed_out` from `AckExpired` with no `Delivered` receipt. | spec/functional/FR-035-fleet-assign.md:32,53 |
| FND-010 | medium | FR-034 reads "pane classification per lane (dialog … wait_loop, none)" from Prometheus and ranks a pane class "captured after the last hook event" above hook evidence. PR52 exports pane classes only as `ix_fleet_lane_blocked{reason}`, mixed with the hook reasons `permission` and `question`, with no `none` series and no pane capture time (the sample time is push time). The precedence rule of FR-034-AC-7 has no source. | spec/functional/FR-034-fleet-lane-status.md:32-34,54-55,96 |
| FND-011 | low | FR-034 keeps `max_tool_runtime` (2h) as ix-board configuration, while PR52 configures it in relay `[fleet]` and exports it in `ix.fleet.config` next to `working_window` and `no_events_after`. This is the duplicate-constant pattern fixed for the working window; ix-board `no_events` also differs from relay `no_events_after`. | spec/functional/FR-034-fleet-lane-status.md:40-41 |
| FND-012 | low | spec.md out-of-scope still says `fleet assign` writes "relay `work agent`", but the write target is now the PR52 FR-072 assignment list. FR-072 also accepts writes only from a caller with the orchestrator role, a prerequisite FR-035 does not state. | spec/spec.md:100 |

## New findings (disposition pass 2)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-013 | medium | Relay `ix_fleet_lane_blocked` reasons are `permission`, `dialog`, `login`, `usage_limit`, `question`, `queued_question` and `wait_loop` (ix-relay PR52 FR-045 at 2d4ba5c / a680c33). The rewritten FR-034 BLOCKED rule and AC-6 map every reason except `queued_question`, which round 1 mapped to BLOCKED(question). A lane with queued follow-up input now falls through to WORKING or IDLE. | spec/functional/FR-034-fleet-lane-status.md:59-62,102 |
| FND-014 | low | FR-034-AC-12 still says "configured thresholds override defaults" for a list that includes relay-configured `max_tool_runtime`, while AC-16 says ix-board rejects local overrides of it. | spec/functional/FR-034-fleet-lane-status.md:109 |
| FND-015 | low | FR-034-AC-5 still says "Pane `wait_loop`", but rule 5 now reads the relay series reason `wait_loop`, and AC-7 forbids ix-board from using raw pane evidence. | spec/functional/FR-034-fleet-lane-status.md:101 |

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 0aef701 + ix-relay PR52 f1f1015 FR-072: ordered per-lane list keyed by lane address, written by the orchestrator; FR-035 depends on it |
| FND-002 | fixed | PR52 f1f1015 exports wait_loop (lane.blocked reason) and ix.fleet.lane.compile_seconds; FR-034/FR-041 consume them |
| FND-003 | fixed | 0aef701: FR-034 uses FR-068 kinds and fields with the mappings (blocked/permission to dialog, stop question to question); AC-6 |
| FND-004 | fixed | 0aef701: FR-034 pane classes match FR-069 (queued_question to question). The precedence rule is now explicit; its data source is the new FND-010 |
| FND-005 | fixed | 0aef701: STALE-ORDER covers never-delivered orders; assign sends an order with a deadline; Acted confirmed by derived focus (FR-035 step 5) |
| FND-006 | fixed | 0aef701 + PR52: relay lane.state has no unknown; one working window read from relay config; relay series advisory |
| FND-007 | fixed | 0aef701: join key is the relay address; Redis via relay-core and Prometheus HTTP API named; stale_source defect |
| FND-008 | fixed | 0aef701: FR-041 interim supply (server-capacity) and duty (watcher compile seconds); AC-5 |
| FND-009 | fixed | a8266e2: timed_out derived as AckExpired with no Delivered; TimedOut removed from FR-034, FR-035 and spec.md |
| FND-010 | fixed | a8266e2: FR-034 reads relay-resolved ix_fleet_lane_blocked{reason} and holds no hook-vs-pane precedence (AC-7) |
| FND-011 | fixed | a8266e2: max_tool_runtime and no_events_after read from relay ix.fleet.config, no local copy (AC-16) |
| FND-012 | fixed | a8266e2: spec.md names the FR-072 list and orchestrator role; FR-035 step 4 and AC-8 cover role and host_registry_stale refusals |
| FND-013 | fixed | ef0735f: FR-034 maps relay question and queued_question to BLOCKED(question); AC-6 |
| FND-014 | fixed | ef0735f: AC-12 limits overrides to ix-board-owned thresholds; relay-owned values cannot be overridden |
| FND-015 | fixed | ef0735f: AC-5 names the relay series reason wait_loop |
