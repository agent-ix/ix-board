---
id: SR-002
title: "Gap analysis — ix-board SWM-2 repository audit"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@456679207e32d312ca0bb6020e5238327f411836; spec/, src/, tests/, .github/workflows/ci.yml, Cargo.toml, deny.toml"
review_set: subset
---

## Summary

Audited the full ix-board repository at `456679207e32d312ca0bb6020e5238327f411836` in planless mode. The required Test Matrix is absent, and the coverage engine reports no backed acceptance-criterion rows; plan completion was not assessed.

## Verdict

**FAIL** — no Test Matrix exists, and none of the 21 test-verified acceptance criteria has a bound test symbol.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The repository has no Test Matrix, so the declared acceptance criteria have no durable test-to-requirement matrix | spec/ |
| FND-002 | high | None of the 21 Test acceptance criteria is backed by a trace-bound test symbol | spec/functional/, tests/integration.rs |
| FND-003 | medium | The `NFR-001` benchmark trace is untracked by the declared trace model | tests/integration.rs:533 |
| FND-004 | medium | Cache path override and JSON output are externally visible behavior without owning requirements | src/main.rs::Cli, src/main.rs::cache_path |

## Coverage

- Reconciliation: `quire coverage` (quire 0.33.0; spec-artifacts-process v0.26.0).
- Plan completion: not assessed
- Rows backed by a tagged test: 0 / 24 (the three `Inspection` rows are declared no-source-symbol checks; 21 `Test` criteria remain unbacked).
- Untracked symbols: 1 (`NFR-001` on `warm_cache_next_cli_runs_under_one_second`).
- Inventoried public behavior/configuration: 3 commands, 3 global CLI options, 1 cache-path environment override, and the public read-service API; 2 externally visible configuration/output behaviors are untraced.
- Source stubs: 0. Test stubs: 0. The ignored warm-cache benchmark names its lane.
- Semantic review: skipped; only the required mechanical gap-analysis steps were run.

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-001 | deferred | User-directed prerelease scope excludes EA and review ceremony; a Test Matrix is deferred until the first-draft implementation phase is complete. |
| FND-002 | deferred | User-directed prerelease scope excludes formal traceability ceremony; AC test tagging is deferred while first drafts are being built. |
| FND-003 | deferred | User-directed prerelease scope excludes formal traceability ceremony; benchmark trace cleanup is deferred with the other trace tags. |
| FND-004 | deferred | User directed that this prerelease phase focus on first-draft specification and implementation, without full behavior-spec ceremony; the CLI option contract can be captured in a later pass. |
