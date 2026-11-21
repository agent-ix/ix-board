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

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Verified one shared filter definition and both transport consumers at 39e709e6a0a1ce75c051b04c3cc3c16240f2cdc9. |
| FND-002 | fixed | Verified b99241a31f911951a3eac0caaf6d7260238737a7; exact after excerpt below. |
| FND-003 | fixed | Verified b99241a31f911951a3eac0caaf6d7260238737a7; exact after excerpt below. |

## Disposition verdict (round 1)

PASS. Inspected pushed clean head 39e709e6a0a1ce75c051b04c3cc3c16240f2cdc9. One borrowed crate-private UpdatedFilter with private lower-bound fields and constructor is now defined in src/linear.rs; detail and native summaries call the same constructor. The duplicate summary definition is deleted. Wire shape, absent full filter, nonnull delta watermark, archive flags, v15 partitioning and quota state are unchanged. No new findings. Coder focused delta/summary/retro rerun passes 12 tests, zero failures/ignored; /tmp/SWM-184-filter-fix-tests.log. Final fmt/lint pass. No reviewer API calls, branch edits, aggregate gate or cargo build. Ready for lead final gate. Custody destinations remain reviews/swm-184-full-query-filter-code-review.md and reviews/swm-184-full-query-filter-gap-analysis.md.

## Disposition evidence (round 1)

```yaml
{
  "dispositions": [
    {
      "fnd": "FND-001",
      "outcome": "fixed",
      "after_excerpt": "pub(crate) struct UpdatedFilter<'a> {\n    updated_at: UpdatedLowerBound<'a>,\n}"
    }
  ]
}
```

## New findings (disposition pass 2)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-002 | high | At prior head 39e709, a returned actual complexity below its reservation never refunded the difference. The observed 264-request refresh cost 73398 actual units but retained 2640000 estimated units, exhausting the 2700000 local budget and preventing a complete cold baseline. Reconcile only each durable reservation against actual returned cost, retain missing/unobserved estimates, and avoid restoring external headroom or refunding a newer window. Oracle: more than 270 cheap real HTTP pages complete with exact actual cost, with concurrent/replayed/old-window completions unable to refund unrelated charges. | src/quota.rs:586-590 (prior head 39e709) |
| FND-003 | low | The new replay regression uses a qualified trace attribute that the installed grammar rejects, so the test binds neither declared criterion. Use the already imported #[trace] form. Oracle: fresh matrix lists this replay test as a binder for FR-031-AC-4 and FR-031-AC-5. | src/quota.rs:920 (reviewed c25eafc) |

## Disposition verdict (round 2)

PASS at pushed clean b99241a31f911951a3eac0caaf6d7260238737a7. FND-001 remains fixed; FND-002 and FND-003 are fixed. Reviewed every quota scope-extension source/test diff from 39e709 through c25eafc and the final attribute-only b99241a. No new production defects found. Reservation tokens carry durable row identity, database identity, local generation and server-complexity generation; completion validates and consumes its own row exactly once in the same immediate transaction. Local counters replace only their generation's estimated charge. Missing costs and dropped tokens retain conservative charges. Server remaining is monotonic per observed window and pending estimates plus unobserved debt are deducted separately; completion does not credit headroom consumed by other tools. Fixed local reset is independent of rolling response headers. Old-generation responses cannot refund current local charges or debit current server debt. Legacy migration retains costs/cooldown and creates no refundable historic tokens. Wrong-store and replayed completions refuse atomically. HTTP request counters remain unchanged by settlement.

Inspected behavior assertions for multiprocess settlement, out-of-order externally consumed remaining, local/server rollover, absent actual costs, process-exit conservatism, wrong database, duplicate completion, migration and rolling resets. Real offline HTTP full-catalog regression completes 301 requests and 300 issues for exactly 30100 complexity; metadata reports successful complete refresh, all actual cost and zero estimates. Coder /tmp/SWM-184-quota-fix-tests.log: 252 focused tests green, zero failures/ignored, plus check/fmt/lint green. Final b99241a changes only a test attribute, and fresh reviewer matrix confirms replay-test bindings to AC4/5. No API calls, DB mutations, source edits, reviewer cargo build or aggregate gate. Operator-only one-time legacy refund assessment remains separate from code acceptance. Final lead aggregate gate remains required.

## Scope extension and disposition evidence (round 2)

```yaml
{
  "scope": [
    {
      "id": "FR-031-AC-4",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct diagnostic."
    },
    {
      "id": "FR-031-AC-5",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "Counters record requests attempted, returned complexity when available, estimated complexity otherwise, duration, mode, and success/failure for each refresh; partial failed refresh costs remain counted and are never described as a successful sync."
    }
  ],
  "findings": [
    {
      "fnd": "FND-002",
      "severity": "high",
      "confidence": "high",
      "method": "code-review",
      "check": "code-bug",
      "artifact_id": "FR-031-AC-4",
      "related_ids": [
        "FR-031-AC-5"
      ],
      "path": "src/quota.rs",
      "lines": "586-590 (prior head 39e709)",
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct diagnostic.",
      "finding": "At prior head 39e709, a returned actual complexity below its reservation never refunded the difference. The observed 264-request refresh cost 73398 actual units but retained 2640000 estimated units, exhausting the 2700000 local budget and preventing a complete cold baseline. Reconcile only each durable reservation against actual returned cost, retain missing/unobserved estimates, and avoid restoring external headroom or refunding a newer window. Oracle: more than 270 cheap real HTTP pages complete with exact actual cost, with concurrent/replayed/old-window completions unable to refund unrelated charges."
    },
    {
      "fnd": "FND-003",
      "severity": "low",
      "confidence": "high",
      "method": "code-review",
      "check": "coverage",
      "artifact_id": "src/quota.rs::tests::durable_ledger_rejects_replayed_completion_without_another_refund",
      "related_ids": [
        "FR-031-AC-4",
        "FR-031-AC-5"
      ],
      "path": "src/quota.rs",
      "lines": "920 (reviewed c25eafc)",
      "excerpt": "#[ix_trace_rs::trace(\"FR-031-AC-4\", \"FR-031-AC-5\")]",
      "finding": "The new replay regression uses a qualified trace attribute that the installed grammar rejects, so the test binds neither declared criterion. Use the already imported #[trace] form. Oracle: fresh matrix lists this replay test as a binder for FR-031-AC-4 and FR-031-AC-5."
    }
  ],
  "dispositions": [
    {
      "fnd": "FND-002",
      "outcome": "fixed",
      "after_excerpt": "if local_generation == reservation.generation {\n            let changed=transaction.execute(\"UPDATE linear_shared_quota SET complexity=complexity+?1 WHERE singleton=1 AND complexity>=?2\",\n                params![delta,reservation.estimated_cost]).map_err(Error::Cache)?;"
    },
    {
      "fnd": "FND-003",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-031-AC-4\", \"FR-031-AC-5\")]\n    #[test]\n    fn durable_ledger_rejects_replayed_completion_without_another_refund() {"
    }
  ],
  "bindings": [
    {
      "test_id": "src/quota.rs::tests::durable_ledger_rejects_replayed_completion_without_another_refund",
      "ac_id": "FR-031-AC-4",
      "trace": "correct"
    },
    {
      "test_id": "src/quota.rs::tests::durable_ledger_rejects_replayed_completion_without_another_refund",
      "ac_id": "FR-031-AC-5",
      "trace": "correct"
    }
  ]
}
```
