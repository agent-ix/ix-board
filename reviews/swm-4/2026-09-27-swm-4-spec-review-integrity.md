---
id: SR-010
title: "Integrity review of SWM-4 drift requirements"
type: SpecReview
analysis: integrity
scope: "agent-ix/ix-board@e36c410591eb606e62fae04f02475bfb1f88db0d; spec/functional/FR-008-drift-signals.md, spec/functional/FR-009-drift-alerts-output.md, spec/spec.md, spec/usecase/US-002-team-leader-drift.md"
review_set: subset
---

## Summary

US-002 traces to FR-008 and FR-009, and each new FR has explicit verification criteria. The veering rule requires a historical ready queue immediately before each Coding transition, but the declared inputs do not include histories for all queue inputs.

## Verdict

**FAIL** — the historical queue requirement cannot be implemented from the specified evidence.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Specify historical project status, priority/order, ticket state/spec state, and dependency data or a defined reconstruction source for the ready queue immediately before each Coding transition; current records plus transition times cannot establish the required historical rank. | spec/functional/FR-008-drift-signals.md:33-34,60-68,105 |

## Traceability

| Use case | Requirement | Verification |
| --- | --- | --- |
| US-002 | FR-008 | FR-008-AC-1 through FR-008-AC-7 |
| US-002 | FR-009 | FR-009-AC-1 through FR-009-AC-6 |
