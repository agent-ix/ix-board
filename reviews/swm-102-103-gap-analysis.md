---
id: SR-028
title: "Gap analysis — ix-board SWM-102/103 repository audit"
type: SpecReview
analysis: gap-analysis
scope: "spec/, src/, tests/; PR changes in src/program_review.rs, src/linear.rs, src/main.rs and plugin/skills/ix-board-planning/SKILL.md"
review_set: subset
---

## Summary

The initial audit at head `8b83413bde585a7b483e355ed301eac0715f74a3` found no specification for the new commands. At head `ddc9d45`, FR-019 and FR-020, US-008, and trace bindings still cover the new behavior; prior dispositions for the 50 unchanged untagged criteria and 24 unchanged untracked symbols remain in force.

## Verdict

**FAIL** — the new public CLI behavior is unspecified, and the current repository matrix has untagged criteria.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The 50 listed requirements have no tagged test in the computed matrix. These gaps are present unchanged at the PR parent. | FR-001-AC-1, FR-001-AC-3, FR-001-AC-5, FR-001-AC-6, FR-002-AC-2, FR-002-AC-3, FR-002-AC-4, FR-002-AC-5, FR-002-AC-6, FR-002-AC-7, FR-002-AC-8, FR-003-AC-1, FR-003-AC-2, FR-003-AC-3, FR-003-AC-4, FR-003-AC-5, FR-004-AC-1, FR-004-AC-2, FR-004-AC-3, FR-004-AC-4, FR-004-AC-5, FR-004-AC-6, FR-005-AC-1, FR-005-AC-2, FR-007-AC-1, FR-007-AC-2, FR-007-AC-3, FR-007-AC-4, FR-007-AC-6, FR-008-AC-1, FR-008-AC-2, FR-008-AC-3, FR-008-AC-4, FR-008-AC-5, FR-008-AC-6, FR-008-AC-7, FR-008-AC-8, FR-008-AC-9, FR-009-AC-1, FR-009-AC-2, FR-009-AC-3, FR-009-AC-4, FR-009-AC-5, FR-009-AC-6, FR-009-AC-7, FR-013-AC-14, FR-016-AC-1, FR-016-AC-2, FR-016-AC-3, NFR-001-M-1 |
| FND-002 | high | The new user-visible `scope check` and `roadmap` commands have no owning FR or use case in `spec/`. | src/main.rs:114-130, src/program_review.rs:178-183, src/program_review.rs:366-374 |
| FND-003 | medium | The coverage report lists 24 test symbols whose self-named IDs are not declared criteria. | src/cache.rs, src/hygiene.rs, tests/cache_path.rs, tests/integration.rs |

## Coverage

- Reconciliation: quoin matrix (quoin 0.28.3, quire 0.36.2).
- Static criteria: 65 tagged, 50 untagged, 0 tagged-by-ignored-test, 6 method-without-symbol (121 matrix rows).
- Evidence: no run evidence for 121 rows.
- `quire coverage`: 24 untracked test symbols, 5 unmatched tags, 14 declaration diagnostics.
- The same 50 untagged statuses were observed at parent `10d87f4` and PR head `8b83413`.
- The changed CLI behavior inventory found 2 untraced commands; no stub was found in the changed implementation or new tests.
- Plan completion: not assessed.
- Semantic review: skipped; no explicit opt-in was given.
- `spec/spec.md` has no artifact `id`, so no `ix://` spec-root relationship was authored.


## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | accepted-no-change | The 50 criteria are unchanged from the PR parent and outside SWM-102/103; this fix adds only the program-review requirements. |
| FND-002 | fixed | FR-019/FR-020 and US-008 now specify both commands, with matrix bindings for all test-verified criteria. |
| FND-003 | accepted-no-change | The same 24 test symbols were present at the PR parent; this fix does not change those legacy tests. |

### FND-002 after excerpt

```text
The `scope check` command SHALL validate a TOML program scope file against
structured Linear initiative and project catalogs and GitHub repository
availability.

The `roadmap` command SHALL build a deterministic program view from one
structured Linear workspace snapshot, with an optional program scope file.
```


## Disposition pass 2 verification

No gap-analysis findings were left pending a new disposition from the prior round. The prior outcomes remain current: FND-001 and FND-003 are accepted-no-change; FND-002 is fixed. The head change only synchronizes the timeout regression test and does not alter specifications, test trace tags, or the repository-wide coverage counts.
