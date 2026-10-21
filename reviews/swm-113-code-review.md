---
id: SR-2028
title: "Code review — ix-board SWM-113"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; PR #55 at d661a6e11a7520e63a58bb1639af15ea04101764"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-020"
    type: reviews
---

## Summary

Reviewed the exact PR #55 head `d661a6e11a7520e63a58bb1639af15ea04101764`.

The previous `also_in` ordering finding is fixed. `build_roadmap` now applies `compare_names` to the source memberships and `sort_names` to `also_in` in both scoped and unscoped modes. `compare_names` orders by lowercase name and uses original spelling as the deterministic tie-breaker. The new test exercises scoped and unscoped `also_in` output, including a same-folded-name pair in scoped mode. The previous scope diagnostic ordering fix remains: diagnostic vectors, mismatch rows, and both membership-difference arrays use the same comparator, with mixed-case coverage in `scoped_roadmap_diagnostic_names_sort_case_insensitively_with_stable_ties`.

FR-020-AC-4 now explicitly includes each `also_in` membership list and the original-spelling tie-breaker.

## Verdict

**PASS** — no new findings.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Verification

- `cargo test roadmap_also_in_names_sort_case_insensitively_in_scoped_and_unscoped_modes --lib`: passed.
- `cargo test scoped_roadmap_diagnostic_names_sort_case_insensitively_with_stable_ties --lib`: passed.
- No new findings.
