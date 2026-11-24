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
