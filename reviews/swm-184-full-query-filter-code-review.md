---
id: SR-6087
title: "SWM-184 full query filter code-review"
type: SpecReview
analysis: code-review
review_set: subset
scope: "agent-ix/ix-board; SWM-184 PR81; src/linear.rs and changed delta/retro fixtures"
---

## Summary

Reviewed frozen 6a44b8412fc1ea0fe17ffa2219a5787424c37c7f against 15f39764b4ada73a249ba180e26d4e9cf37231ae. This is the post-merge full-query null-filter repair; previous SR6085/6086 findings remain fixed and unchanged. Code review includes applicable Rust style/API/wire/safety/testing references.

## Verdict

CONDITIONAL. Correctness checks pass; the narrow duplication finding requires correction before merge.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | The new UpdatedFilter/UpdatedLowerBound wire shape duplicates existing UpdatedFilter/Since in src/linear_summary.rs:276-284. Searching src for UpdatedFilter and updated_at: Since identifies two owners of the same updatedAt.gte rule, so future wire changes require parallel edits. Use one shared crate-private typed filter with suitable borrowing or generic ownership. Oracle: both detail and native-summary full/delta query-shape tests pass through that one definition. | src/linear.rs:1822-1840 |

## Coverage

Reviewed all three changed files and all query/variable construction call sites. Detail full requests omit the entire filter variable, so no nested null gte is constructed; deltas send a nonnull typed filter and last successful start minus 60 seconds. Full includeArchived=false; delta=true. Both paged connections retain cursors. Native summaries already send a whole optional filter with null rather than nested null; the regression inspects that shape. v15 source namespace changes detail and derived summary partitions; old rows survive and singleton quota is retained (one prior request, complexity seven; nine new sends produce ten requests and 90007 complexity). No quota transport control or public API changes.

Fresh quire matrix reports all four examined criteria tagged, including the new poisoned-baseline regression; whole repository remains 104 tagged, 177 untagged, 9 inspection-only. Existing evidence-store limitation remains no bound run evidence. Focused coder logs /tmp/SWM-184-filter-tests.log show 227 tests passing with zero ignored/failures; fmt/lint logs pass. Reviewer read every new assertion, inspected all changed code, and ran fresh matrix plus git diff --check. No real API calls or reviewer cargo build (lead owns release build). No applicable AssuranceProfile; unchanged specs were not re-reviewed.

Plan completion: not assessed

Reconciliation: quire matrix (quire 0.52.0, engine 0.62.2). Semantic fanout not requested; ordinary source/test alignment inspected. Source/test stubs: 0. New untraced behavior families: 0.

## Examined scope

```yaml
{
  "clean": false,
  "scope": [
    {
      "id": "FR-030-AC-1",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Given a baseline, a fixture delta adds and updates records, removes observed archive/trash entries and obsolete memberships, preserves unchanged IDs, and produces the same result when replayed; all three entity queries use the last successful sync minus 60 seconds and include archived records."
    },
    {
      "id": "FR-030-AC-3",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source catalog is removed. An ordinary intervening refresh uses only deltas, and output exposes the last full reconciliation time."
    },
    {
      "id": "FR-030-AC-6",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Fixtures independently change source endpoint, workspace, credential-derived authorization identity, cache-model/schema version, and dataset coverage while retaining overlapping stable record IDs; each mismatch (or absent compatibility metadata) requires a full baseline, never merges or returns the old partition as fallback, and a failed/blocked baseline exposes no old-partition records. An exact partition match permits incremental reuse; changing credentials does not reset the shared quota allowance."
    },
    {
      "id": "FR-031-AC-4",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct diagnostic."
    },
    {
      "id": "src/linear.rs::UpdatedFilter",
      "path": "src/linear.rs",
      "role": "examined",
      "excerpt": "struct UpdatedFilter {\n    updated_at: UpdatedLowerBound,\n}"
    }
  ],
  "findings": [
    {
      "fnd": "FND-001",
      "severity": "low",
      "confidence": "high",
      "method": "code-review",
      "check": "other",
      "artifact_id": "src/linear.rs::UpdatedFilter",
      "related_ids": [
        "FR-030-AC-1"
      ],
      "path": "src/linear.rs",
      "lines": "1822-1840",
      "excerpt": "struct UpdatedFilter {\n    updated_at: UpdatedLowerBound,\n}",
      "finding": "The new UpdatedFilter/UpdatedLowerBound wire shape duplicates existing UpdatedFilter/Since in src/linear_summary.rs:276-284. Searching src for UpdatedFilter and updated_at: Since identifies two owners of the same updatedAt.gte rule, so future wire changes require parallel edits. Use one shared crate-private typed filter with suitable borrowing or generic ownership. Oracle: both detail and native-summary full/delta query-shape tests pass through that one definition."
    }
  ],
  "bindings": []
}
```
