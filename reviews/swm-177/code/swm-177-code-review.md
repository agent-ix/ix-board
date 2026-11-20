---
id: SR-6064
title: "Code review — native project status preservation"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; SWM-177 PR73; all ten changed src/tests files and affected model, cache, source and readiness callers"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-005", type: references}
  - {target: "ix://agent-ix/ix-board/FR-004", type: references}
  - {target: "ix://agent-ix/ix-board/FR-001", type: references}
---
## Summary

Independent code-review with the rust-review/rust-style lane for frozen head 6a43edb82fafa4721fea84d6efffe3cf061393b3, base 2154f2d5b1f2cc540772dfdf4e6ab27fd0479658. Ticket: SWM-177; consumer: https://linear.app/agent-ix/issue/SWM-176. One missing-evidence boundary remains incomplete. No branch edits, new worktree, compiler, live ix-board call, credential access, install or aggregate gate occurred.

## Verdict

**CONDITIONAL; fix or explicitly dispose FND-001 before merge.** Category-only completion, exact native names, canonical Started/Paused behavior, legacy public-record defaults and v13 cache isolation otherwise match the request. Final aggregate verification belongs to the lead.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | Missing native status type is rejected rather than retained as Unknown: RawProjectStatus.status_type lacks an absent-field default, so a status object containing only name fails deserialization before normalization. The absent-or-unsupported test checks unsupported strings and Default::default(), but never the omitted native type required by acceptance 2. | src/linear.rs:1608; src/linear.rs:3210 |

## Coverage

Methods: code-review with rust-review/rust-style; gap-analysis is SR-6065. No spec/plan diff, so spec-review and its sub-analyses are inapplicable. AGENTS/CLAUDE, CONTRIBUTING, Cargo, formatter, Clippy, deny and CI scope were inspected. No applicable AssuranceProfile exists under spec/. No duplicated private/public category enum, copied metric engine, new compatibility reader, dependency/version change, gate weakening or unrelated source repair was found. Runtime exact reviewer model identity is unavailable; marker model=unavailable records that limitation. Run: A0927D5B-719E-4CD7-B7DF-18D60AF02A28.

All ten changed paths were examined: src/chain.rs, src/hygiene.rs, src/lib.rs, src/linear.rs, src/model.rs, src/program_review.rs, src/reconcile.rs, tests/integration.rs, tests/retro.rs, tests/scope.rs. The seven fixture edits only supply explicit absent evidence. The existing private category vocabulary was moved into the public model; completion exhaustively maps six known categories and Unknown. New data never derives completion from names or issue counts. No new unsafe, async, resource-bound or numeric conversion path was introduced.

Oracle inspection: the fake HTTP test exercises actual GraphQL decoding/normalization, retaining custom completed/canceled names, misleading Started/Planned labels, custom Started names, Paused and unsupported type; an associated completed issue cannot make the Started project terminal. Whole-snapshot serde round trip compares the actual normalized snapshot. The real SQLite test seeds the old v12 namespace, invokes production ReadService against a fake transport, inspects both namespace rows, proves old bytes unchanged, checks new category/name fields, and decodes the old record as Unknown/None despite its Completed label. Cache key construction includes the model version and SHA256 of the fixture key, and SnapshotCache reads/writes/invalidation constrain their SQL by cache_key. The installed 0.6 binary was not executed or changed. Project JsonSchema derives include both fields and the typed enum; no checked-in Project schema exists to drift. The missing native type case is established by the required non-Option serde field without default; it was not executed under the build hold.

Existing lead-run logs inspected: scoped clippy --locked --all-targets exit0; Linear unit tests26 passed; affected integration30 passed/1 opt-in benchmark ignored; retro77 passed; scope6 passed. Logs: ../swm177-clippy.log, ../swm177-linear-tests.log, ../swm177-consumer-tests.log. These are retained lead execution evidence, not reviewer-run checks or engine-bound run evidence. No extra independent Cargo run is requested before the fix. After adding the missing-evidence regression, run the focused Linear unit lane under the lead's renewed admission; the single final aggregate remains pending.

The three new tests have no trace attribute. Existing FR-001/004/005 own the bounded repair, but their literal existing criteria do not separately specify category serialization/completion; do not fabricate a criterion binding or require a new metric-engine/spec artifact. This limitation remains visible in the accepted legacy matrix baseline, not a claim of new full trace coverage.

## Examined units

```yaml
scope:
  - {id: request-acceptance-1, path: SWM-177, role: examined, excerpt: "Preserve the native project-status category in a public typed model/API independently of native display name and existing canonical status behavior. Known completed/canceled categories classify terminal even with custom display names; active categories stay nonterminal regardless of misleading names/task totals."}
  - {id: request-acceptance-2, path: SWM-177, role: examined, excerpt: "Old records, absent or unsupported category evidence remain explicitly Unknown. Never infer completion from an old status string or done/total counters. Preserve exact native display names for new records, including custom Started names."}
  - {id: request-acceptance-3, path: SWM-177, role: examined, excerpt: "Native normalization, serialization/schema and callers/fixtures stay coherent. Existing canonical Started/Paused/readiness behavior is unchanged."}
  - {id: request-acceptance-4, path: SWM-177, role: examined, excerpt: "Existing installed0.6 CLI and new library can coexist against the same cache database without strict old readers receiving new-field payloads. Use the existing cache namespace/version mechanism; demonstrate independent old/new entries and prove prior payloads unchanged. New readers accept old project records with Unknown category."}
  - {id: request-acceptance-5, path: SWM-177, role: examined, excerpt: "Add meaningful deterministic fake-transport/model/cache regressions for custom terminal names, misleading nonterminal names, absent/unsupported categories, round trips and old/new cache coexistence. No real auth, token, credential store or live probe."}
  - {id: FR-001-AC-1, path: spec/functional/FR-001-ready-queue.md, role: examined, excerpt: "Given fixture projects in `Started` and `Paused`, `next` includes eligible tickets only from `Started` projects."}
  - {id: FR-004, path: spec/functional/FR-004-cache-polling.md, role: context_only, excerpt: "The ix-board read library SHALL use SQLite to cache structured Linear read data."}
  - {id: FR-005, path: spec/functional/FR-005-structured-data-boundary.md, role: context_only, excerpt: "The ix-board read commands SHALL base filtering, readiness, ordering, and other command decisions on structured Linear fields."}
```

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Round1 at 0216d32efbad4712f6caec866d4ee6bb79091f68 adds serde(default) to RawProjectStatus.type; the real fake-HTTP regression proves omitted evidence retains the exact Completed name with Unknown category/completion. RED94d40e5 fails for missing field type; GOOD passes all27 Linear unit tests. |

## Disposition pass 1

Current scoped code-review verdict: **PASS**. FND-001 is fixed; no new findings. Reviewed frozen head0216d32efbad4712f6caec866d4ee6bb79091f68, RED94d40e554a32b6c24babd61e671c99306f9c3f40, and the complete delta from the original reviewed6a43edb. Production delta is solely the absent-field default; the additional test uses the existing loopback seam and production fetch_workspace, removing type from the actual JSON and asserting exact native display name, Unknown category and Unknown completion. No alternate category inference, API/cache shape change or new caller behavior was introduced.

Exact fixed excerpt:

```rust
    #[serde(rename = "type", default)]
    status_type: ProjectStatusCategory,
```

The original Summary, Verdict, Findings and Coverage above remain historical and unmodified. The two committed review copies were byte-identical to the original scratch artifacts before this append. Current artifact destination: reviews/swm-177/code/swm-177-code-review.md.

Inspected lead-run logs ../swm177-missing-type-red.log (exit101, one intended failure with missing field type at the missing-evidence assertion), ../swm177-fix-lint.log (make -j1 fmt-check lint exit0; existing stable-rustfmt configuration warnings remain visible), ../swm177-fix-linear-tests.log (27 passed including the added regression; exit0). The negative control establishes that the new assertion reaches the production decoder and detects the original defect. These logs are actual lead-run evidence, not new reviewer executions.

No new check is requested. Review is clear for the lead's single final aggregate gate and latest artifact custody. SR-6065's unchanged177-criterion legacy FAIL/INCOMPLETE remains accepted outside scope. No compiler, source edit, new worktree/target, branch mutation, live ix-board call, auth, credential access, config/signing/install change or aggregate gate occurred during disposition. BUILDQUIESCENT. Run DD03AC1A-8D60-4BF5-B86A-A994EB5AEFFC; exact runtime model identity unavailable.

