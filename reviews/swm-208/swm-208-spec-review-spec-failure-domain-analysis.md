---
id: SR-6104
title: "spec-review/spec-failure-domain-analysis of ix-board fleet spec (PR86, SWM-208)"
type: SpecReview
analysis: failure-domain
scope: "agent-ix/ix-board@31ad309efb0118fa89173ee7a17dfa4eb4048fa8; spec/functional/FR-034..FR-041"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-040"
    type: reviews
---

# SR-6104: ix-board fleet failure-domain analysis

## Summary

Ticket: SWM-208. Unstated failure modes, partial failures, identity confusion
and numeric edge cases in the fleet FRs, with the relay contracts of ix-relay
PR #52 (FR-067/068/069/071) taken as the evidence sources.

## Verdict

Changes requested (medium only). The spec already handles missing sources well
(defect rows, never a silent zero). What it leaves out: bounded relay history
truncating ground truth, cross-host clock skew, a zero-duty division, partial
failure in assign, double assignment, and concurrent ledger writers.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-040 grades over `--since` windows and a 24h target, and FR-041 uses a 24h duty window, but the relay activity stream (FR-068) keeps at most 20,000 events and 24h. The ix-board ledger stores no hook events. A busy fleet truncates ground truth, and the spec does not say this is reported or how it is avoided. | spec/functional/FR-040-fleet-calibrate.md:24-30 |
| FND-002 | medium | FR-034 compares hook `ts` (writing-host clock, ms) with ix-board's clock against 5m/10m/15m thresholds and gives no skew bound or handling (FR-018 has one). A skewed host misclassifies WORKING, LOOP or DEAD. | spec/functional/FR-034-fleet-lane-status.md:46-62 |
| FND-003 | medium | FR-041 `lanes_needed = ceil(sustainable_slots / mean duty)` is undefined when mean duty is 0 or every lane's duty is unavailable, and the spec does not say which lanes enter the mean. | spec/functional/FR-041-fleet-saturation.md:30-31 |
| FND-004 | medium | FR-035 writes the `work agent` list before sending. A failed send leaves a stored list that FR-036 counts as owned work the lane never received; behaviour 5 only stops the ledger from claiming it was sent. | spec/functional/FR-035-fleet-assign.md:24-35 |
| FND-005 | medium | FR-035 does not refuse a ticket already assigned to another lane, so one ticket can sit in two queues. FR-036 coverage and FR-041 buffer then double-count, or two lanes work the same ticket. | spec/functional/FR-035-fleet-assign.md:21-24 |
| FND-006 | medium | FR-039 accepts appends from both `fleet status` and `fleet tick`, possibly concurrent (a minder and a human). No single-writer or ordering rule covers interval closing, and "twice the cadence" has no cadence for ad-hoc runs. | spec/functional/FR-039-fleet-ledger.md:31-34 |

## Scope

- FR-034, FR-034-AC-6, FR-034-AC-7 (examined)
- FR-035, FR-035-AC-4, FR-035-AC-5 (examined)
- FR-036 (context_only)
- FR-039, FR-039-AC-2, FR-039-AC-3 (examined)
- FR-040, FR-040-AC-6 (examined)
- FR-041, FR-041-AC-1, FR-041-AC-3 (examined)
- FR-038 (examined, clean for failure domain: failed tick emits no metric)

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-007 | medium | FR-039 `ground_truth_sample` is labelled at each tick, but FR-040 truth at `t` needs events up to `t+W` (active in [t-W,t+W]; inactive when no tool-use follows within W). A label written at tick time `t` cannot see that future, so persisted labels for spans beyond relay retention are premature or wrong. Label sample `t-W` at tick `t` instead. | spec/functional/FR-039-fleet-ledger.md:28-30; spec/functional/FR-040-fleet-calibrate.md:27-29 |
| FND-008 | low | `executed_at` = first `post_tool` on the target lane after emission. That fires at once on a lane that is already working, and for lead-targeted actions (`escalate_*`, `escalate_stale_pr`) the lane's tool use is unrelated to the lead acting, so time-to-action reads near zero. | spec/functional/FR-039-fleet-ledger.md:45-48 |

## New findings (disposition pass 2)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-009 | medium | FR-036 never mentions UNREACHABLE. Tickets in an UNREACHABLE lane's assignment count as owned, so they never appear in `unowned_ready`, and the lane can appear in `short_queues` and invite more tickets to a lane nobody can reach. The lead decision says UNREACHABLE counts as not working; coverage needs to surface its assignments, for example as `unreachable_assignments` or as unowned. | spec/functional/FR-036-fleet-plan-coverage.md:23-28 |
| FND-010 | medium | FR-041 never mentions UNREACHABLE. The ready buffer subtracts tickets held by UNREACHABLE lanes, and supply and demand still count a stale host's `sustainable_slots`/`lane_capacity` and its lanes' duty. FR-034's "counts as not working for capacity" has no capacity rule behind it. | spec/functional/FR-041-fleet-saturation.md:25-39 |

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 0aef701: FR-040 detects history truncation (unavailable: history_truncated) and persists ground_truth_sample per tick; AC-8 |
| FND-002 | fixed | 0aef701: FR-034 uses relay server time for all ages, `clock_skew` defect over max_clock_skew; AC-13 |
| FND-003 | fixed | 0aef701: FR-041 excludes lanes below minimum observed time; no_duty / zero_duty make demand unavailable; AC-4 |
| FND-004 | fixed | 0aef701: FR-035 stores the list only after the relay accepts the order; a send failure writes no list. Lead decision: no rollback after an accepted send; exit non-zero and record the order (step 4, AC-3) |
| FND-005 | fixed | 0aef701: FR-035 step 2 refuses a ticket in another lane's assignment and names that lane; AC-4 |
| FND-006 | fixed | 0aef701: FR-039 single-writer lock, monotonic sequence, gap rule at tick cadence (default 5m); AC-2, AC-3 |
| FND-007 | fixed | a8266e2: FR-039 writes labels for t-W at tick t, never t; FR-040-AC-9 |
| FND-008 | fixed | a8266e2: executed_at = target lane's first transition into the action's expected state; lead/project actions have none |
