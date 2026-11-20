---
id: SR-6061
title: "Gap analysis — SWM-164 saved-provider source seam"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; SWM-164 follow-up; repository spec/source/test inventory, changed source construction and credential_provider tests"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
  - {target: "ix://agent-ix/ix-board/FR-015", type: references}
  - {target: "ix://agent-ix/ix-board/FR-029", type: references}
---
## Summary

Repository-driven planless audit for SWM-164 follow-up PR71 at 896bbfcc906ec5e3ecb919f66600b26c3aa375d3, enabling downstream SWM165 SR088-FND002. No newly unowned production behavior or hollow implementation was found. Existing repository trace gaps remain; new inaccurate test claims are recorded by code/Rust review SR-6060.

## Verdict

**FAIL; repository assurance INCOMPLETE** — 178 existing criteria are untagged. This is the previously accepted legacy limitation (180 at base, before two newly tagged FR015 criteria); it is not authority to claim whole-repository PASS or to repair unrelated criteria in this source seam.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The computed repository matrix retains 178 untagged legacy criteria, including FR-015-AC-1. These existing gaps prevent full repository assurance; the complete criterion list is retained in untagged.txt. The upstream seam introduces no new criterion. | spec/functional/FR-015-protected-credential-value.md:35; matrix.json; untagged.txt |

## Coverage

Plan completion: not assessed

Reconciliation: quoin matrix --repo /Users/peter/dev/ix-board-swm-164-seam --json, exit0, quoin0.36.7 / quire0.52.0 engine0.62.2. Static result: 229 criteria, 43 tagged, 178 untagged, 8 method-without-symbol, 0 tagged-by-ignored-test. All229 have no bound run evidence. Static tagging is not proof of behavior: inaccurate new bindings and missing cache oracle are specifically recorded in SR6060 FND001–003.

quire coverage --scope /Users/peter/dev/ix-board-swm-164-seam --json exit0: Rust census327 candidates,273 tagged,109 bound, rust-trace-attribute marker form; zero untracked_symbols and no diagnostic for credential_provider.rs. Existing cfg symbol collisions, old comment marker forms and NFR-001 extraction diagnostics remain. coverage's totals show228 criteria while matrix emits229 including NFR-001-M-1; preserve this extraction distinction rather than asserting equal denominators. Raw matrix/coverage and authoring-pack JSON retained next to the artifacts.

Reverse inventory inspected module/CLI/public declarations and stub searches across49 Rust source/test files. Sixteen behavior families map to existing FR001–029/NFR001: ready queues, project chunks, search/issues, cache, project controls, focus expansion/status/hygiene/planning, program scope/roadmap/reconcile, blocker chains/orphans, drift, agents, credentials, retro config/catalog, append-only store, metrics, snapshots/delta/backfill and department model/assessment/source adapters. Existing unsupported collector branches explicitly return unavailable and are not hollow claims of successful coverage. New seams refactor credential construction/read-service injection already owned by FR015 and FR029; no second resolver or metric definition. No new meaningful unowned behavior, source stub or test stub found. Detailed changed-path depth is recorded by SR6060; this inventory is not a claim to have semantically re-reviewed every old source implementation.

Five new tests exercise real provider, SDK resolver, source HTTP/cache and LiveLinear collection through public APIs. Their fake backend is at the external credential boundary, not a replacement of the tested collection. Existing source-level protected_credential_preserves_authentication_and_cache_identity and empty_credential_inputs_remain_missing tests pass within23 linear tests but use legacy comment tags; the new falsely broadened tags do not cure that contract. Downstream actual scoped startup collection remains unaccepted until SWM165 supplies its production-path test.

No explicit plan supplied or selected. Optional semantic fan-out skipped; normal code-review test-intent checks ran. No applicable AssuranceProfile. No production credential read, environment-secret retrieval, source editing, extra worktree, push/install/version/auth change or aggregate gate.

## Examined units

- FR-015-AC-1 (examined), spec/functional/FR-015-protected-credential-value.md: A synthetic credential is retained as SecretValue; its Debug representation is redacted and does not contain the fixture text.
- FR-015-AC-2 (examined), spec/functional/FR-015-protected-credential-value.md: The existing injected HTTP request receives the unchanged fixture Authorization header and the existing cache namespace remains stable.
- FR-015-AC-3 (examined), spec/functional/FR-015-protected-credential-value.md: Empty and whitespace-only constructor inputs still return MissingApiKey.
- FR-029-AC-4 (examined), spec/functional/FR-029-department-retro-contract.md: The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged.
- FR-029-AC-8 (examined), spec/functional/FR-029-department-retro-contract.md: Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set.
- saved-provider-startup-seam (examined), tests/credential_provider.rs: The public credential construction seam feeds the real cached Linear reader.
- provider-construction (examined), src/linear.rs: pub fn from_credential_store<B: CredentialBackend>(store: &SecretStore<B>, environment_override: Option<OsString>, cli_fallback: impl FnOnce() -> Option<String>, endpoint: &str) -> Result<Self, Error>
- live-reader-construction (examined), src/retro/live.rs: pub fn with_source(source: LinearGraphQlSource, cache_path: &Path) -> Result<Self, crate::Error>

## Standing baseline disposition

FND-001 is retained as the historical repository limitation under the lead's explicit instruction to preserve the unrelated known baseline and avoid broad repair. It remains a FAIL for repository assurance; this acceptance does not clear SR6060's four new in-scope findings.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | accepted-no-change | Round1: the lead explicitly excludes the existing legacy trace baseline from this source refactor. Preserve repository FAIL/INCOMPLETE; do not waive new SR6060 findings. |

