---
id: SR-013
title: "Scope and boundary review of SWM-4 drift requirements"
type: SpecReview
analysis: scope-boundary
scope: "agent-ix/ix-board@e36c410591eb606e62fae04f02475bfb1f88db0d; spec/functional/FR-008-drift-signals.md, spec/functional/FR-009-drift-alerts-output.md, spec/spec.md, spec/usecase/US-002-team-leader-drift.md"
review_set: subset
---

## Summary

ix-board owns the read, signal classification, alert decisions, and output; Linear, GitHub, herdr, and OTel supply external evidence. The spec makes these responsibilities and the no-write boundary explicit. The incomplete-source case is recorded under the failure-domain method.

## Verdict

**PASS** — no additional scope-allocation finding.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Allocation

| Requirement | Owner | Class |
| --- | --- | --- |
| FR-008 | ix-board drift classifier | core |
| FR-009 | ix-board drift renderer | core |

Linear and GitHub provide issue/PR events; herdr and OTel provide activity observations. ix-board treats missing observations as unavailable and performs no source mutation.
