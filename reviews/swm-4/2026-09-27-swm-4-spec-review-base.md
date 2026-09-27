---
id: SR-008
title: "Base spec review of SWM-4 drift report"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@e36c410591eb606e62fae04f02475bfb1f88db0d; spec/functional/FR-008-drift-signals.md, spec/functional/FR-009-drift-alerts-output.md, spec/spec.md, spec/usecase/US-002-team-leader-drift.md"
review_set: subset
---

## Summary

US-002 traces to both new FRs, the IDs and links validate, and the five signals have fixture-oriented acceptance criteria. Three behavior boundaries need correction before implementation: merged PRs opened before the reporting window, zero spend thresholds, and the public JSON field contract.

## Verdict

**FAIL** — the merged-PR and zero-threshold cases can produce incorrect churn alerts.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Count and attribute PRs merged in the window even when they were created before it; otherwise the churn signal undercounts landed work. | spec/functional/FR-008-drift-signals.md:46-59,80-84,108 |
| FND-002 | high | Reject zero spend thresholds or require positive observed spend; zero is currently valid and makes an idle lead satisfy spend greater than or equal to the threshold. | spec/functional/FR-008-drift-signals.md:75-79; spec/functional/FR-009-drift-alerts-output.md:27-30 |
| FND-003 | medium | Define stable JSON field names and value shapes for leads, signals, alerts, and unassigned PRs so machine consumers have a testable output contract. | spec/functional/FR-009-drift-alerts-output.md:49-59,74 |

## Coverage notes

The new acceptance criteria name test verification but no implementation tests exist in this spec-only PR. The behavior is intended for the subsequent implementation PR. Boundary fixtures should include a PR opened before the window and merged inside it, a zero-spend idle lead, and exact JSON field/value assertions.

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 0bba78bfc42ff3902c9e5e546195ec7df878ca76 |
| FND-002 | fixed | 0bba78bfc42ff3902c9e5e546195ec7df878ca76 |
| FND-003 | fixed | 0bba78bfc42ff3902c9e5e546195ec7df878ca76 |

FND-001 after_excerpt (from spec/functional/FR-008-drift-signals.md:94-97 at 0bba78bfc42ff3902c9e5e546195ec7df878ca76):

>    the reporting window, regardless of when those PRs were opened. When a PR
>    opened before the window merges inside it, ix-board SHALL count it in merged-PR
>    churn after ticket-ID attribution, but not in new-PR scope or
>    busy-without-new-PR counts. ix-board SHALL report the three counts and

FND-002 after_excerpt (from spec/functional/FR-008-drift-signals.md:86-91 at 0bba78bfc42ff3902c9e5e546195ec7df878ca76):

> 6. When herdr reports a lead busy at report time **or** that lead's observed
>    OTel spend during the reporting window is greater than zero **and** reaches
>    the configured `spend_threshold`,
>    ix-board SHALL report busy-without-PR if that lead has no attributed PR
>    created in the window. The finding SHALL state which activity source met
>    the condition and the observed spend when applicable.

FND-003 after_excerpt (from spec/functional/FR-009-drift-alerts-output.md:91-94 at 0bba78bfc42ff3902c9e5e546195ec7df878ca76):

> | `leads[].signals` | Object with exactly the five required keys `pr_scope`, `veering`, `stall`, `busy_without_pr`, and `ticket_churn`. |
> | `leads[].signals.<signal>.availability` | Exactly `available` or `unavailable`. |
> | `leads[].signals.<signal>.missing_sources` | Array of source/partition reason strings; empty if available, nonempty if unavailable. |
> | `leads[].signals.pr_scope.prs` | Array of `{repository: string, number: integer, url: string, ticket_id: string, project_id: string or null, classification: "in_scope" or "out_of_scope"}`. |
