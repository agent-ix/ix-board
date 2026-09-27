---
id: SR-003
title: "Base spec review of SWM-2 ix-board requirements"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@094933f51df9876db35bfc4d4276b9ed49a82e58; spec/spec.md, spec/usecase/US-001-team-leader-reads.md, spec/functional/FR-001-ready-queue.md, spec/functional/FR-002-project-chunk.md, spec/functional/FR-003-search.md, spec/functional/FR-004-cache-polling.md, spec/functional/FR-005-structured-data-boundary.md, spec/functional/FR-006-team-leader-handoff.md, spec/non-functional/NFR-001-warm-cache-latency.md"
review_set: subset
---

## Summary

Reviewed all nine spec artifacts added for SWM-2 at the frozen PR #1 head. The first draft captures the central read behaviors, but it leaves a conflict in ready-queue ordering and an underspecified blocker-layer result for project-chunk.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The use case orders ready work by spec step, while FR-001 orders by project and ticket priorities without including step; the queue order is contradictory. | spec/usecase/US-001-team-leader-reads.md:43-44; spec/functional/FR-001-ready-queue.md:27-29 |
| FND-002 | medium | FR-002 requires blocker layers but does not define how far project-chunk traverses them or whether cross-project blockers are included. | spec/functional/FR-002-project-chunk.md:17-21 |

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | ca121ee288f86835584217247cab78e30b7c64bf |
| FND-002 | fixed | ca121ee288f86835584217247cab78e30b7c64bf |

FND-001 after excerpt:

> Then eligible work is ordered by project priority and project manual order, then by ticket priority and ticket manual order, with stable project and ticket identifiers as final tie-breakers; each row includes its applicable spec step and explains its readiness or position

FND-002 after excerpt:

> The ix-board `project-chunk <project>` command SHALL include in `blocker_layers` the full transitive `blocked_by` closure rooted at the requested project's open issues. The closure includes those open issues and every referenced prerequisite, including prerequisites from other projects or teams and prerequisites in `Completed` or `Canceled` states. The command orders layers topologically, with prerequisites before their dependents, and orders entries within each layer by stable Linear issue identifier. It reports a missing-dependency error if an issue in the closure is absent from the structured source data, and a dependency-cycle error if the closure contains a cycle.

Implementation cross-check at code SHA `8291ded94a067ba86b7354d0ebdae9dbbe9ac3b0`: `make_project_chunk` seeds traversal from requested-project open issues, follows every `blocked_by` edge through the snapshot, layers prerequisites before dependents with stable identifier ordering, and returns missing-dependency/cycle errors.
