---
id: SR-028
title: "Gap analysis — ix-board repository and SWM-104"
type: SpecReview
analysis: gap-analysis
scope: "spec/, src/, tests/, plugin/skills/ix-board-planning/SKILL.md; SWM-104"
review_set: subset
---

## Summary

Disposition recheck of SWM-104 at `37cc3184edf6a6445ec54eebf3347830804f3e14` against base `699080d633f58c51c0bfa965d900c3be76ea29f0`. FR-021 now owns and trace-tags reconciliation; the same 50 baseline criteria remain untagged. Plan completion was not assessed.

## Verdict

**CONDITIONAL** — FND-001 is fixed; FND-002..051 remain unchanged repository-baseline gaps accepted without change for this ticket.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | `reconcile` is a new user-visible command with ticket-level acceptance criteria but no owning functional requirement in `spec/`; the declared ix-board scope and Test Matrix therefore have no criterion that owns or trace-tags this implementation. Add a formal requirement and trace-tagged tests so specification coverage can be checked. | src/main.rs:129-139 |
| FND-002 | high | FR-001-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-001-ready-queue.md:52 |
| FND-003 | high | FR-001-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-001-ready-queue.md:54 |
| FND-004 | high | FR-001-AC-5 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-001-ready-queue.md:56 |
| FND-005 | high | FR-001-AC-6 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-001-ready-queue.md:57 |
| FND-006 | high | FR-002-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:49 |
| FND-007 | high | FR-002-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:50 |
| FND-008 | high | FR-002-AC-4 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:51 |
| FND-009 | high | FR-002-AC-5 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:52 |
| FND-010 | high | FR-002-AC-6 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:53 |
| FND-011 | high | FR-002-AC-7 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:54 |
| FND-012 | high | FR-002-AC-8 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-002-project-chunk.md:55 |
| FND-013 | high | FR-003-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-003-search.md:39 |
| FND-014 | high | FR-003-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-003-search.md:40 |
| FND-015 | high | FR-003-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-003-search.md:41 |
| FND-016 | high | FR-003-AC-4 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-003-search.md:42 |
| FND-017 | high | FR-003-AC-5 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-003-search.md:43 |
| FND-018 | high | FR-004-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-004-cache-polling.md:39 |
| FND-019 | high | FR-004-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-004-cache-polling.md:40 |
| FND-020 | high | FR-004-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-004-cache-polling.md:41 |
| FND-021 | high | FR-004-AC-4 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-004-cache-polling.md:42 |
| FND-022 | high | FR-004-AC-5 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-004-cache-polling.md:43 |
| FND-023 | high | FR-004-AC-6 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-004-cache-polling.md:44 |
| FND-024 | high | FR-005-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-005-structured-data-boundary.md:32 |
| FND-025 | high | FR-005-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-005-structured-data-boundary.md:33 |
| FND-026 | high | FR-007-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-007-project-queue-controls.md:51 |
| FND-027 | high | FR-007-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-007-project-queue-controls.md:52 |
| FND-028 | high | FR-007-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-007-project-queue-controls.md:53 |
| FND-029 | high | FR-007-AC-4 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-007-project-queue-controls.md:54 |
| FND-030 | high | FR-007-AC-6 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-007-project-queue-controls.md:56 |
| FND-031 | high | FR-008-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:128 |
| FND-032 | high | FR-008-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:129 |
| FND-033 | high | FR-008-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:130 |
| FND-034 | high | FR-008-AC-4 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:131 |
| FND-035 | high | FR-008-AC-5 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:132 |
| FND-036 | high | FR-008-AC-6 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:133 |
| FND-037 | high | FR-008-AC-7 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:134 |
| FND-038 | high | FR-008-AC-8 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:135 |
| FND-039 | high | FR-008-AC-9 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-008-drift-signals.md:136 |
| FND-040 | high | FR-009-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:112 |
| FND-041 | high | FR-009-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:113 |
| FND-042 | high | FR-009-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:114 |
| FND-043 | high | FR-009-AC-4 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:115 |
| FND-044 | high | FR-009-AC-5 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:116 |
| FND-045 | high | FR-009-AC-6 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:117 |
| FND-046 | high | FR-009-AC-7 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-009-drift-alerts-output.md:118 |
| FND-047 | high | FR-013-AC-14 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-013-focus-hygiene-and-diff.md:187 |
| FND-048 | high | FR-016-AC-1 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-016-focus-record.md:30 |
| FND-049 | high | FR-016-AC-2 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-016-focus-record.md:31 |
| FND-050 | high | FR-016-AC-3 has no test function tagged with its id in the computed Test Matrix. | spec/functional/FR-016-focus-record.md:32 |
| FND-051 | high | NFR-001-M-1 has no test function tagged with its id in the computed Test Matrix. | spec/non-functional/NFR-001-warm-cache-latency.md:21 |

## Coverage

- Reconciliation: quoin matrix (quoin 0.28.3, quire 0.36.2).
- Plan completion: not assessed.
- Criteria: tagged 75 · untagged 50 · tagged-by-ignored-test 0 · method-without-symbol 8.
- Evidence: bound 0 · stale 0 · suspect 0 · undischarged 0 · no run evidence 133.
- The matrix includes 20 declared requirements. FR-021 has three criteria and all are tagged; the exact 50 pre-existing untagged criteria remain. Source stubs 0; test stubs 0.
- Semantic review: skipped (optional; not requested).
- `spec/spec.md` has no `id`, so no root `ix://` reference edge could be resolved without inventing one.


## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | FR-021 now owns reconcile; all three new criteria are trace-tagged, including AC-2 for incomplete attachment pagination. |
| FND-002 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-003 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-004 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-005 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-006 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-007 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-008 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-009 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-010 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-011 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-012 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-013 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-014 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-015 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-016 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-017 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-018 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-019 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-020 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-021 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-022 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-023 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-024 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-025 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-026 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-027 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-028 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-029 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-030 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-031 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-032 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-033 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-034 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-035 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-036 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-037 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-038 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-039 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-040 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-041 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-042 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-043 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-044 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-045 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-046 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-047 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-048 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-049 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-050 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
| FND-051 | accepted-no-change | Unchanged repository-baseline matrix gap outside SWM-104 reconciliation scope; the team chose to keep this ticket scoped to reconcile and not include unrelated traceability backfill. |
