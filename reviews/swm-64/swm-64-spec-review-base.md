---
id: SR-6001
title: "Base spec review of ix-board SWM-64 priority-goal drift"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board; FR-008, FR-009, spec/spec.md, US-002"
review_set: subset
---

## Summary

The change maps the user story to deterministic goal membership, progress, staffing, crowding, evidence completeness, and JSON/text requirements. Two verification gaps remain: AC-11 does not exercise blocker-worker counting, and the QSL L2 oracle does not state its expected worker count/minimum values.

## Verdict

**CONDITIONAL** — the distinct-assignee target is preserved, but its blocker branch and the supplied-history worker oracle are not fully discriminating.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-008-AC-11 tests distinct assignees only on goal issues, so it does not verify that active direct/transitive blocker assignees enter the same distinct-worker set as required by Behavior 10. A worker-count implementation that omits blockers can pass this AC; add blocker cases that show a shared assignee counts once and a distinct active blocker assignee increments the count. | FR-008-AC-11 |
| FND-002 | medium | The QSL L2 oracle says to report the configured distinct-worker count/minimum but gives neither expected number nor complete goal configuration. The named deterministic fixture has two active goal issues (QSL-485 and QSL-486) assigned to two distinct identities and a minimum of 1, so its expected count is 2/1; state the expected count and minimum so the QSL replay cannot pass with an arbitrary or issue-proxy value. | FR-008-AC-13, FR-009-AC-10 |

## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | The revised AC exercises two goal issues sharing one assignee, an open blocker assigned to a second assignee, and another blocker assigned to the first assignee; it states that goal issues and open blockers are counted together by distinct assignee ID and that only Coding/In Progress (or equivalent started) states contribute. |
| FND-002 | fixed | Both QSL L2 ACs now explicitly require two distinct active assignees against minimum one. |
