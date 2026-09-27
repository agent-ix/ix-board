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
