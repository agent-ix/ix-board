---
id: SR-6063
title: "Gap analysis — ix-board 0.6.0 release metadata"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; SWM-160 release; repository matrix and source inventory, metadata-only PR72"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
  - {target: "ix://agent-ix/ix-board/FR-001", type: references}
---
## Summary

Planless repository baseline audit accompanying metadata-only PR72 at de1ee66ba485ffefc1c8d5969e4245fd9476b68c. No functional source or criterion changes; the existing legacy traceability limitation remains visible and is accepted outside this release's scope.

## Verdict

**FAIL; repository assurance INCOMPLETE** —177legacy criteria remain untagged. This unchanged baseline does not introduce a scoped release defect and is not a claim of full repository assurance. Metadata review SR-6062 passes.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The computed repository matrix retains177untagged legacy criteria, including FR-001-AC-1. Existing gaps prevent full repository assurance; the complete criterion list is retained in untagged.txt. The version-only change introduces no new criterion or source gap. | spec/functional/FR-001-ready-queue.md:52; matrix.json; untagged.txt |

## Coverage

Plan completion: not assessed

Reconciliation: quoin matrix --repo /Users/peter/dev/ix-board-department-release --json exit0, quoin0.36.7 / quire0.52.0 engine0.62.2. The computed result is229criteria:44tagged,177untagged,8method-without-symbol, zero ignored-only criteria. All229have no bound run evidence. This matches the accepted source-seam baseline, not a new release coverage failure. Matrix/coverage JSON is retained beside this artifact.

quire coverage --scope /Users/peter/dev/ix-board-department-release --json exits0: Rust census327candidates,269tagged,106bound, rust-trace-attribute form; zero untracked_symbols. Existing cfg symbol collisions, legacy comment forms and extraction diagnostics remain; coverage reports228criteria while matrix includes229 with NFR-001-M-1. Static tagging and retained test logs do not establish engine-bound execution evidence.

Reverse inventory examined public module/CLI declarations and source/test stub searches across the retained tree. Existing behavior families—ready queues/project chunks/search, cache, project controls, focus, program review/reconcile, blocker chains/orphans, drift, agents, credentials, retro config/store/catalog/metrics/snapshots/deltas/backfill, department assessments and source adapters—map to existing FR001–029/NFR001. Unsupported historical collectors explicitly return unavailable, not successful empty coverage. No newly unowned behavior, source stub, test stub or inflated test claim is introduced: git establishes src/tests/spec/examples/CI unchanged from accepted8381a60. This is metadata-level reverse-gap inspection, not a fresh semantic review of every old implementation.

No explicit plan supplied or selected. Optional semantic fan-out skipped. No applicable AssuranceProfile. No compiler, tests, aggregate gate, real credentials or runtime ix-board command, source edit, extra worktree, push, release or install. Artifact/live acceptance remains outside this read-only audit.

## Examined units

- FR-001-AC-1 (examined), spec/functional/FR-001-ready-queue.md:52: Given fixture projects in `Started` and `Paused`, `next` includes eligible tickets only from `Started` projects.
- package-version (examined), Cargo.toml:3: version = "0.6.0"
- locked-package-version (examined), Cargo.lock:1083: version = "0.6.0"

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | accepted-no-change | Round1 standing authority: the lead explicitly excludes the accepted177legacy gaps from this version-only release. Preserve repository FAIL/INCOMPLETE; no broad repair or duplicate functional gate. |
