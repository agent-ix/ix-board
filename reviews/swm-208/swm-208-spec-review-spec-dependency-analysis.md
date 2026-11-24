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
