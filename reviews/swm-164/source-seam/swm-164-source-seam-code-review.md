---
id: SR-6060
title: "Code review — SWM-164 saved-provider source seam"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; SWM-164 follow-up; src/linear.rs, src/retro/live.rs, tests/credential_provider.rs; FR-015 and FR-029"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-015", type: references}
  - {target: "ix://agent-ix/ix-board/FR-029", type: references}
---
## Summary

Ticket SWM-164 upstream follow-up for SWM-165 SR088-FND002. Independent frozen-head review of PR71 at 896bbfcc906ec5e3ecb919f66600b26c3aa375d3, base63080bc6bcea0bc6ee032c4f2b36b304d6e853d9. The production construction seam preserves SDK resolution and source collection; four test-evidence defects need correction.

## Verdict

**CONDITIONAL** — no production credential-resolution or collection bug found, but the in-scope trace bindings and finite regression failure need repair before merge.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The saved-provider fixture is bound to FR-029-AC-4 but supplies only empty workspace pages and never invokes or asserts the canonical metric oracles named by that criterion. Remove the unsupported binding or exercise the claimed metric behavior. | tests/credential_provider.rs:63 |
| FND-002 | medium | The saved-provider fixture is bound to FR-029-AC-8 but has no duplicate or sequenced events and never runs or asserts tag-repin/state-bounce findings. Remove the unsupported binding or exercise the claimed loop behavior. | tests/credential_provider.rs:63 |
| FND-003 | medium | The new FR-015-AC-2 bindings mark the compound header/cache-identity criterion tagged, but none of their test bodies asserts the cache namespace, and the environment/provider-error/CLI-fallback tests assert neither an HTTP header nor cache identity. Keep bindings faithful to actual assertions and bind the existing real header/cache-identity oracle with the supported attribute. | tests/credential_provider.rs:63-243 |
| FND-004 | low | The loopback server blocks on three listener.accept calls and the test joins it without a deadline. If a regression returns complete evidence after only the two workspace requests and omits the freshness read, the request-count assertions are never reached and the test hangs indefinitely. Bound server completion so a missing request fails finitely. | tests/credential_provider.rs:91-94,166-171 |

## Coverage

Methods: code-review with rust-review/rust-style lane. Gap-analysis is separate SR-6061. No spec/plan changes; spec-review sub-analyses inapplicable. No applicable AssuranceProfile found. Repository AGENTS/CLAUDE, Make/Cargo/lint/formatter configuration, Rust API/implementation/testing/safety/boundary instructions and SDK SecretStore/SecretValue behavior were inspected. No CI, version, dependency, license or unsafe gate changes.

All three diff paths and their production callers were read. The source uses SDK resolve_from for controlled environment snapshots; absent override never reads process environment. The saved provider wins before lazy CLI fallback, and typed provider failures propagate before fallback. Credentials remain SecretValue and are exposed only at the existing HTTP and cache hashing boundaries. The production from_env wrapper delegates through this path; LiveLinear resolves once and clones the configured source into the existing read service and history reader. Explicit cache construction uses the existing cache implementation. No duplicate resolver, metrics, compatibility layer, vendoring or product-auth feature was introduced.

Independent make fmt-check, focused cargo clippy --lib --test credential_provider, five credential_provider tests and 23 linear::tests pass, each exit0. Logs are retained in the source-seam-review directory. The author's adverse log was inspected: ambient-environment reread fails the saved-key HTTP assertion and the real collection assertion (exit101), then the restored tests pass. This confirms an effective saved-provider oracle, not full downstream TUI startup acceptance. The latter remains SWM165's responsibility.

No real credential store/status, credential-backed ix-board command, token extraction/injection, ACL/signing changes, source edits, pushes, extra worktree or aggregate gate. Compiler phases were sequenced under explicit admission and jobs2 with the same retained target. Exact reviewer model ID is unavailable; run 58729A6B-235E-448B-BD4F-B15BC248C5C3.

## Examined units

- FR-015-AC-1 (examined), spec/functional/FR-015-protected-credential-value.md: A synthetic credential is retained as SecretValue; its Debug representation is redacted and does not contain the fixture text.
- FR-015-AC-2 (examined), spec/functional/FR-015-protected-credential-value.md: The existing injected HTTP request receives the unchanged fixture Authorization header and the existing cache namespace remains stable.
- FR-015-AC-3 (examined), spec/functional/FR-015-protected-credential-value.md: Empty and whitespace-only constructor inputs still return MissingApiKey.
- FR-029-AC-4 (examined), spec/functional/FR-029-department-retro-contract.md: The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged.
- FR-029-AC-8 (examined), spec/functional/FR-029-department-retro-contract.md: Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set.
- saved-provider-startup-seam (examined), tests/credential_provider.rs: The public credential construction seam feeds the real cached Linear reader.
- provider-construction (examined), src/linear.rs: pub fn from_credential_store<B: CredentialBackend>(store: &SecretStore<B>, environment_override: Option<OsString>, cli_fallback: impl FnOnce() -> Option<String>, endpoint: &str) -> Result<Self, Error>
- live-reader-construction (examined), src/retro/live.rs: pub fn with_source(source: LinearGraphQlSource, cache_path: &Path) -> Result<Self, crate::Error>

## Bindings

| Test | AC | Trace |
| --- | --- | --- |
| saved_provider_without_override_reaches_real_department_collection | FR-029-AC-4 | wrong; FND-001 |
| saved_provider_without_override_reaches_real_department_collection | FR-029-AC-8 | wrong; FND-002 |
| saved_provider_without_override_reaches_real_department_collection | FR-015-AC-2 | incomplete; FND-003 |
| explicit_environment_snapshot_precedes_provider | FR-015-AC-2 | does not assert criterion; FND-003 |
| provider_access_failures_propagate_without_cli_fallback | FR-015-AC-2 | does not assert criterion; FND-003 |
| cli_fallback_runs_only_after_absent_provider_and_missing_stays_error | FR-015-AC-2 | does not assert criterion; FND-003 |
| invalid_environment_snapshot_never_falls_through | FR-015-AC-3 | checks whitespace constructor refusal; empty/non-UTF-8 environment adds supplemental guard |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Round1 at43b72b3: the saved-provider test no longer claims FR-029-AC-4; matrix confirms no credential_provider binder for that criterion. |
| FND-002 | fixed | Round1 at43b72b3: the saved-provider test no longer claims FR-029-AC-8; matrix confirms no credential_provider binder for that criterion. |
| FND-003 | fixed | Round1 at43b72b3: unsupported supplemental FR-015-AC-2 tags were removed. The actual redaction/header/cache-identity oracle now has the supported trace attribute for FR-015-AC-1/2 and its exact behavior passes. |
| FND-004 | fixed | Round1 at43b72b3: recv_timeout(5s) must observe server completion before join. The inspected missing-freshness mutation fails exit101 after5.01s at the explicit all-three-requests deadline rather than hanging. Restored provider tests pass. |

## Disposition pass 1

Frozen revision43b72b376c734a81b6b29fc36e783640de97d00e independently inspected. Original findings and historical binding table above remain unchanged. All four findings are fixed, with no new findings. **Current scoped code/Rust verdict: PASS; mergeable after the lead's final aggregate gate.**

Independent confined checks: five credential_provider integration tests exit0; the protected_credential_preserves_authentication_and_cache_identity unit test exit0; git diff --check exit0. Author fix clippy/fmt/23-linear-test results and adverse log inspected. The production source API is unchanged in this fix; no new credential behavior, global environment mutation or duplicate resolver appeared. The server channel observes successful completion before join; a missing request now has a finite failure. No source changes, push, credentials, extra worktree or aggregate gate. All reviewer compiler/test commands completed; BUILD QUIESCENT.

Current matrix: FR-015-AC-1/2 bind the real protected-credential oracle; credential_provider no longer claims FR-029-AC-4/8 or the unsupported header/cache bindings. Supplemental resolver guards remain tests without false criterion claims. FR-015-AC-3 keeps the fixture's real whitespace refusal binding. Repository assurance is still FAIL/INCOMPLETE for177 legacy untagged criteria, retained by SR6061.

