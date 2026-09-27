---
id: SR-005
title: "Base spec review of SWM-3 ix-board project controls"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@6115a38cf4f569be9ba5d1692c33a820627de125; README.md, spec/spec.md, spec/usecase/US-001-team-leader-reads.md, spec/functional/FR-001-ready-queue.md, spec/functional/FR-007-project-queue-controls.md"
review_set: subset
---

## Summary

Reviewed the SWM-3 project-control requirement and its README, master-spec, use-case, and queue-contract context at the frozen PR #3 head. Status controls and mutation boundaries are clear, but the priority input is not implementable or testable as written because the accepted values and CLI representation are undefined.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-007 does not define the accepted project-priority values or how callers encode them, leaving validation and the unsupported-priority acceptance criterion without a testable boundary. | spec/functional/FR-007-project-queue-controls.md:22-23, 35-36, 51 |

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 85a583010c481f030227384766a5292eabe330a8 |

FND-001 after_excerpt:

> - `project set-priority <project> <0|1|2|3|4>` accepts one decimal integer
>   token and sets the matching Linear project priority: `0` = No priority,
>   `1` = Urgent, `2` = High, `3` = Medium, and `4` = Low. Other tokens are
>   invalid.
