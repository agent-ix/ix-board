---
id: SR-012
title: "Failure-domain review of SWM-4 drift requirements"
type: SpecReview
analysis: failure-domain
scope: "agent-ix/ix-board@e36c410591eb606e62fae04f02475bfb1f88db0d; spec/functional/FR-008-drift-signals.md, spec/functional/FR-009-drift-alerts-output.md, spec/spec.md, spec/usecase/US-002-team-leader-drift.md"
review_set: subset
---

## Summary

The new read is explicitly non-mutating and handles wholly unavailable sources. The specification does not define how to detect incomplete paginated source results, which could be mistaken for complete evidence and yield false zero counts or an on-track status.

## Verdict

**CONDITIONAL** — source completeness needs an explicit failure rule.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | Require complete enumeration of GitHub PR/commit and Linear issue/history pages, or mark affected signals unavailable when any page or partition is missing; partial results currently satisfy the written source-available condition. | spec/functional/FR-008-drift-signals.md:33-37,80-91,109 |
