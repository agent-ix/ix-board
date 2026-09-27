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

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 0bba78bfc42ff3902c9e5e546195ec7df878ca76 |

FND-001 after_excerpt (from spec/functional/FR-008-drift-signals.md:108-115 at 0bba78bfc42ff3902c9e5e546195ec7df878ca76):

> 9. For each source query, ix-board SHALL determine whether all pages and
>    partitions needed for the reporting interval and historical queue have
>    been enumerated. If a GitHub PR/commit or Linear issue/history page or
>    partition is missing, truncated, or has an unresolved next-page cursor,
>    ix-board SHALL mark each affected signal unavailable with the incomplete
>    source and page/partition reason. It SHALL retain findings already proven
>    by received records but SHALL NOT treat partial counts as complete or use
>    them to conclude that a lead is `on_track` or has no alert.
