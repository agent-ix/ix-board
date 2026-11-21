---
id: SR-6094
title: "SWM-185 gap-analysis of quota-state implementation"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; SWM-185; origin/main...8d2d8df774974e5e976f46c57a4ced9520f0039b; changed quota, transport, SDK, status, tests"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-032", type: references}
---

## Summary

Ticket SWM-185, PR agent-ix/ix-board#83. Reviewed frozen HEAD 8d2d8df774974e5e976f46c57a4ced9520f0039b against origin/main 0f05379e85ea91fd451c2884aa752a714540bdb1, using the single assigned worktree. This proportional implementation review found three concrete code failures and corresponding gaps in otherwise substantial fixture coverage.

## Verdict

**CONDITIONAL** — findings require resolution in the same PR. No owner approval gate is inferred.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | Migration coverage only forces paired_evidence back to zero before reopening; it never interleaves an old writer after migration, so it misses the reproduced loss of a live legacy restrictive constraint. | tests/quota_state.rs:488-526 |
| FND-002 | medium | The simultaneous-blocker test checks only QuotaDecision display, never the legacy reserve error; the legacy actual-server retry delay is wrong while this test remains green. | tests/quota_state.rs:192-258 |
| FND-003 | medium | The bounded typed-refusal fixture covers only ServerReserveHeld through FixtureSource and misses actual ServerBackoff through LinearGraphQlSource's legacy conversion, where both decision access paths lose the captured metadata. | tests/sdk_quota_outcome.rs:235-268 |

## Reproduction and repair constraints

1. Open a new gate on a fixture DB, then execute the old gate's post-migration header update setting request_limit=10000, request_remaining=250 and request_reset=NOW+10000. New decision(10) returns Available and clears request_remaining to NULL. The old shared cooldown column is still honored: setting cooldown=NOW+10000 produces ServerBackoff. This matters during the explicitly identified mixed CLI/SDK rollout. Either safely retain/import interleaved old restrictive evidence without sliding expiry or losing charge ownership, or install a technical fence that rejects old quota writers and document/test cutover. A coordination assumption alone does not repair it.
2. Observe requests remaining250, limit10000, resetNOW+1000 and HTTP429 retry-after3. decision(10) renders `Linear rate limited; retry in 3s`; reserve(10,None) renders `Linear rate limited; retry in 1s`. Derive actual-server retry from its own blocker, retaining independent earliest reevaluation semantics.
3. A real LinearGraphQlSource with explicit fixture credential, unreachable loopback endpoint and preseeded persisted 60-second ServerBackoff returns existing LinearRateLimited on workspace_snapshot_bounded; Error::quota_decision is None and refresh_outcome is Unknown. Preserve existing FR-004-AC-18 variant matching and public construction contracts; provide captured provenance through an additive bounded evaluated envelope/accessor or equivalent separate API, rather than changing that legacy error contract. No request is sent by this proof.

Read-only Rust proof files: /tmp/SWM-185-code-review/probe.rs and bounded.rs, linked against the existing frozen worktree rlibs. Only scratch fixture databases were written; the candidate source/tests/branch were untouched.

## Coverage

Methods: code-review with rust-review and rust-style API, implementation, safety/concurrency, wire/persistence, testing and Tokio references; separate quoin:gap-analysis with explicitly authorized semantic intent/test/code inspection. No spec/plan diff exists, so no additional spec-review methods apply. No applicable AssuranceProfile exists. All 16 changed paths were inspected, including Cargo version/lock, docs, public SDK API additions, shared SQLite evaluator, observation compaction, transport, summaries, status and all changed tests. No CI gate was changed, vendored rule source, duplicated transport/evaluator, live stub or test-only production bypass was found. The ordinary/evaluated API variants share their transport/evaluator; the identified legacy conversions are the defects.

Reconciliation: quire matrix (quire 0.52.0, engine0.62.2), static output /tmp/SWM-185-code-matrix.json; no run evidence read. All ten FR-032 criteria are tagged by imported #[trace] attributes. Full-repository background totals are tagged114, untagged177, method-without-symbol9; these unchanged/outside-diff areas are not promoted into new SWM-185 findings or represented as a full-repository PASS. Diagnostics include platform cfg symbol collisions, historical undeclared/comment-style trace forms, and an NFR Acceptance-Criteria section mismatch. This is a proportional changed-behavior audit, not a baseline repair campaign.

Plan completion: not assessed

Semantic review: examined all ten FR-032 criteria, FR-031 admission/backoff/counters and selected FR-004/FR-030 compatibility/fallback obligations; remaining criteria below are context only. Six fixed seeds each drive240 production events via real SQLite, reservation/settlement, time movement and reopen. The independent oracle keeps an uncompacted observation list and performs its own arithmetic, rather than calling the production evaluator. Every sequence asserts at least three windows/three retirements, reports its seed/event prefix and continuously checks decisions and counters. The test is meaningful for its generated request/complexity domain; it does not cover mixed-version writers, actual backoff, endpoint changes or public legacy conversion. No claim is made that it covers all domains alone. The claimed old min/max mutation log was inspected as supporting evidence, not rerun by changing frozen source.

Focused execution:74 integration tests passed across bounded_read37, cache_coordination5, cache_status3, quota_backoff1, quota_process15, quota_state8 and sdk_quota_outcome5. Separate quota unit check is recorded in /tmp/SWM-185-code-units.log. First command named nonexistent target quota and exited101 before executing; corrected focused command passed. Logs: /tmp/SWM-185-code-focused.log. No real Linear API was used by tests/proofs. No aggregate make ci, new target directory, separate worktree, source write, commit or push was performed; the lead owns the final once-only aggregate gate.

Reverse inventory: command/API families map to FR-001..032 (ready/project/search/cache/structured fields/queue controls/drift/focus/credentials/batch/agents/program/reconcile/chain/orphans/retro/incremental/quota). Eight changed behavior groups (atomic decision/admission, paired observation recovery, reservation settlement/migration, actual-server transport, ordinary SDK outcomes, summary envelopes, offline status, compatibility serialization) map to FR-032 with FR-031/004/030 context. Changed-scope unowned behaviors0, source stubs0, test stubs0. Full unrelated implementations were inventoried by owning family, not re-reviewed for defects.

## Examined units

```yaml
{
  "scope": [
    {
      "excerpt": "Given an unexpired cache entry, repeated fixture reads use the cached value and return identical results without another source read.",
      "id": "FR-004-AC-1",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given an expired entry and a changed fixture source, the next read refreshes and returns the changed value.",
      "id": "FR-004-AC-2",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given an expired complete entry and a failed or rate-limited fixture refresh, an ordinary read returns that entry labeled stale with its unchanged actual age and cooldown in JSON and text; without a complete entry it returns a typed error; a bounded read returns no stale snapshot.",
      "id": "FR-004-AC-3",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined"
    },
    {
      "excerpt": "Given identical cache state and fixture source, repeated reads return identical results.",
      "id": "FR-004-AC-4",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given an absolute `XDG_CACHE_HOME` on Unix with `HOME` unset or set, the CLI and `ReadService::open_default` both select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3`; given no absolute `XDG_CACHE_HOME` and a set `HOME`, both select `<HOME>/.cache/ix-board/linear.sqlite3`; given `IX_BOARD_CACHE_PATH`, both select that path instead.",
      "id": "FR-004-AC-5",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given a non-absolute or absent `XDG_CACHE_HOME`, the library falls back to the available home directory; given neither an absolute XDG cache directory nor a home directory, the library returns `CacheDirectoryUnavailable` and invents no path.",
      "id": "FR-004-AC-6",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Project queries use a page size of 20 records.",
      "id": "FR-004-AC-7",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Issue queries use a page size of 50 records.",
      "id": "FR-004-AC-8",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "A structured RATELIMITED response produces a typed LinearRateLimited error.",
      "id": "FR-004-AC-9",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined"
    },
    {
      "excerpt": "A rate-limit error carries the cooldown derived from exhausted quota resets.",
      "id": "FR-004-AC-10",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Without a read policy, existing method signatures require no new argument and preserve the 30-second per-request timeout; TTL is 300 seconds by default, explicit environment overrides configuration, and zero/negative/non-integer TTL fails before a request.",
      "id": "FR-004-AC-11",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds or more is refused as an invalid policy with no source read.",
      "id": "FR-004-AC-12",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given a snapshot whose source read began 250 seconds ago but whose cache write or report occurred 10 seconds ago, a bounded read with maximum age 240 seconds refreshes it; an entry with no evidence of when its source read began is refreshed rather than accepted; the returned fetch time is the actual source fetch time.",
      "id": "FR-004-AC-13",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds.",
      "id": "FR-004-AC-14",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given a source whose credential resolution takes longer than the deadline before it becomes ready, a bounded read still performs its full budgeted read after readiness; the bounded read never invokes credential resolution, never retries it, and returns an existing credential error unchanged when the source reports one.",
      "id": "FR-004-AC-15",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker.",
      "id": "FR-004-AC-16",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "A source that cannot honor a bound returns the explicit unsupported-bound error before any source read; its legacy read method still works unchanged.",
      "id": "FR-004-AC-17",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "A structured RATELIMITED response during a bounded read returns the existing typed rate-limit error with its cooldown, not a timeout, and the library does not retry or sleep; a deadline that expires first returns the timeout.",
      "id": "FR-004-AC-18",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined"
    },
    {
      "excerpt": "Given a cache that is locked for longer than the deadline, a bounded read returns the typed timeout within the deadline and does not read the source; a refreshed entry is written to the cache only before expiry.",
      "id": "FR-004-AC-19",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Two separately launched processes read the same expired dataset concurrently; exactly one source refresh occurs and the waiter reuses it after rechecking. A held lock causes ordinary fallback/error within five seconds, and terminating the owner releases the OS lock without a stale lockfile preventing recovery.",
      "id": "FR-004-AC-20",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "A summary refresh, failed refresh, or fallback never advances detail freshness; pre-provenance snapshots have unknown age and fail bounded freshness checks. Fallback preserves unavailable components and does not invent zero values.",
      "id": "FR-004-AC-21",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only"
    },
    {
      "excerpt": "Given a baseline, a fixture delta adds and updates records, removes observed archive/trash entries and obsolete memberships, preserves unchanged IDs, and produces the same result when replayed; all three entity queries use the last successful sync minus 60 seconds and include archived records.",
      "id": "FR-030-AC-1",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "context_only"
    },
    {
      "excerpt": "A failed or rate-limited later delta page leaves the entire dataset, high-water mark, and successful fetch provenance unchanged; the next attempt uses the previous successful high-water mark.",
      "id": "FR-030-AC-2",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined"
    },
    {
      "excerpt": "With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source catalog is removed. An ordinary intervening refresh uses only deltas, and output exposes the last full reconciliation time.",
      "id": "FR-030-AC-3",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "context_only"
    },
    {
      "excerpt": "Cold roadmap and scope-check fixtures request no issue pages or histories; requesting a ticket-dependent report loads detail, subsequent reports reuse it, and a summary refresh never advances detail freshness.",
      "id": "FR-030-AC-4",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "context_only"
    },
    {
      "excerpt": "Two retro workers sharing a fixture cache reuse the centrally coordinated snapshot/delta without each probing and forcing a full workspace fetch; ticket/history requests remain covered by the shared quota meter.",
      "id": "FR-030-AC-5",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "context_only"
    },
    {
      "excerpt": "Fixtures independently change source endpoint, workspace, credential-derived authorization identity, cache-model/schema version, and dataset coverage while retaining overlapping stable record IDs; each mismatch (or absent compatibility metadata) requires a full baseline, never merges or returns the old partition as fallback, and a failed/blocked baseline exposes no old-partition records. An exact partition match permits incremental reuse; changing credentials does not reset the shared quota allowance.",
      "id": "FR-030-AC-6",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "context_only"
    },
    {
      "excerpt": "A fixture HTTP 429 with a non-JSON body and Retry-After, and a GraphQL RATELIMITED fixture with exhausted request/complexity resets, persist the latest applicable retry-until; two observations never shorten it; absent usable metadata yields 60 seconds.",
      "id": "FR-031-AC-1",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "After one process records cooldown, a separately launched process using the same cache attempts workspace, direct issue, history, probe, and force-refresh reads and sends zero requests until expiry; after expiry one eligible attempt is permitted.",
      "id": "FR-031-AC-2",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "With 2.1 seconds remaining the actual-server diagnostic is exactly `Linear rate limited; retry in 3s`; no retry or sleep occurs, and an empty cache fails without network.",
      "id": "FR-031-AC-3",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct typed diagnostic under [FR-032](FR-032-quota-admission-recovery.md).",
      "id": "FR-031-AC-4",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "Counters record requests attempted, returned complexity when available, estimated complexity otherwise, duration, mode, and success/failure for each refresh; partial failed refresh costs remain counted and are never described as a successful sync.",
      "id": "FR-031-AC-5",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "`ix-board cache status` returns cache path, dataset coverage, snapshot age/staleness, last successful sync/full reconciliation, `rate_limit_until`, local budget/reset and latest refresh counters in JSON and text without resolving credentials or making any network request; unknown values are explicit.",
      "id": "FR-031-AC-6",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "Tier fixtures produce the documented API-key/OAuth fallback limits and multipliers, default to free/basic with at most 50 percent share, and honor lower actual header capacity; explicit shares 0.1 and 0.5 apply to both request and complexity capacities, while zero, negative, above-0.5, non-finite, or malformed shares fail before network access. Two API keys sharing the cache consume one allowance and a query exceeding 10,000 estimated complexity is refused locally.",
      "id": "FR-031-AC-7",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined"
    },
    {
      "excerpt": "With request remaining 250, request reserve 500, local requests/complexity zero, no cooldown, and a future paired reset, real persisted admission sends zero calls and returns ServerReserveHeld with requests dimension and the paired deadline. SDK fallback and cache-status JSON/text expose that reason; rate_limit_until has no active cooldown.",
      "id": "FR-032-AC-1",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "Fixture decisions independently exercise every state, simultaneous local/server/backoff holds, both capacity dimensions, endpoint constraints, unknown evidence, and an oversized single request. Primary-state precedence and every blocker/deadline match the contract; the earliest transition is not labeled a guarantee of admission.",
      "id": "FR-032-AC-2",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "With a 501-request admission threshold, remaining 250 expiring at T1 followed by remaining 1000 expiring at T2 blocks before T1 and retires only the old constraint exactly at T1, without carrying 250 to T2. Replacing 1000 with 400 keeps the newer hold until T2. Reversing arrival order never bypasses a live constraint or resurrects it after expiry.",
      "id": "FR-032-AC-3",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "Reset-only, remaining-only, limit-only, missing, malformed, past-reset, equal-time, and out-of-order fixture headers never splice one response's low remaining onto another's later reset. Specifically, remaining 250 with reset T1 arriving after T1 never becomes a new fallback-hour hold. Repeated moving resets cannot extend a retained constraint; missing-reset evidence expires at its fixed fallback bound. Retired evidence is unknown, not invented positive remaining.",
      "id": "FR-032-AC-4",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "A deterministic clock-driven sequence spans at least three local windows and three server observation expiries while requests, varying actual-cost settlements, and moving reset headers continue. Local usage resets only at fixed local deadlines; every admission satisfies all applicable live constraints; expired low evidence does not cause an indefinite hold, and local zero never proves remote availability.",
      "id": "FR-032-AC-5",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "Real SQLite reopen/crash-recovery and concurrent-gate fixtures preserve active holds, paired observation ownership, pending/debt charges and single-use reservations. Late settlement across each kind of generation boundary, duplicated settlement, wrong-store tokens, legacy unpaired rows, corrupt provenance, and backward clock movement cannot refund or admit incorrectly.",
      "id": "FR-032-AC-6",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "Local HTTP fixtures return HTTP 429 and GraphQL RATELIMITED with retry metadata. Until the persisted latest retry-until, independently opened gates and every normal/direct/history/force path make exactly zero further calls even across local resets, newer high remaining observations, and restart. At expiry only a normal otherwise-eligible protected request is admitted; no automatic retry or probe runs.",
      "id": "FR-032-AC-7",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "A WorkspaceSource/local-HTTP integration returns a complete compatible stale snapshot on each typed quota refusal, preserving source-fetch age and successful sync markers. A source error and lock refusal remain distinct outcomes; a bounded read returns its typed refusal without stale data; absent legacy outcome metadata is unknown without message parsing.",
      "id": "FR-032-AC-8",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "An external consumer fixture compiles unchanged legacy public struct literals and existing read calls while another consumer obtains typed outcomes through the additive API. Changing human failure-message wording does not change either consumer's typed classification; old serialized metadata without the new outcome retains documented unknown behavior.",
      "id": "FR-032-AC-9",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "A deterministic sequence/model test drives production admission, persistence reopen, observations, reservations, and settlement using a controlled clock and real storage. Its independent oracle asserts the safety and bounded-expiry properties above after each event, records reproducible failing sequences, and includes generated or enumerated interleavings rather than merely echoing the production evaluator. All tests use fixture sources/local HTTP and make no real Linear API calls.",
      "id": "FR-032-AC-10",
      "path": "spec/functional/FR-032-quota-admission-recovery.md",
      "role": "examined"
    },
    {
      "excerpt": "**I want** useful planning evidence with visible freshness and quota availability",
      "id": "US-010",
      "path": "spec/usecase/US-010-read-within-linear-quota.md",
      "role": "context_only"
    }
  ]
}
```

## Test bindings examined

```yaml
{
  "bindings": [
    {
      "ac_id": "FR-032-AC-1",
      "test_id": "tests/cache_status.rs::offline_status_names_zero_usage_server_reserve_hold_with_owned_deadline",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-1",
      "test_id": "tests/quota_state.rs::paired_constraints_expire_independently_in_both_arrival_orders",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-1",
      "test_id": "tests/sdk_quota_outcome.rs::stale_sdk_read_retains_atomic_quota_reason_and_successful_age",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-1",
      "test_id": "tests/sdk_quota_outcome.rs::native_summary_envelope_keeps_legacy_api_and_captured_quota_outcome",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-2",
      "test_id": "tests/cache_status.rs::offline_status_names_zero_usage_server_reserve_hold_with_owned_deadline",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-2",
      "test_id": "tests/quota_state.rs::paired_constraints_expire_independently_in_both_arrival_orders",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-2",
      "test_id": "tests/quota_state.rs::precedence_keeps_every_blocker_and_unknown_transition",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-3",
      "test_id": "tests/quota_state.rs::paired_constraints_expire_independently_in_both_arrival_orders",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-3",
      "test_id": "tests/quota_state.rs::generated_interleavings_match_independent_uncompacted_accounting_oracle",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-4",
      "test_id": "tests/quota_state.rs::partial_headers_never_slide_owned_deadlines_and_backward_clock_never_resurrects",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-4",
      "test_id": "tests/quota_state.rs::generated_interleavings_match_independent_uncompacted_accounting_oracle",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-4",
      "test_id": "tests/quota_state.rs::bounded_observation_storage_fails_closed_without_sliding_or_discarding_live_ownership",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-4",
      "test_id": "tests/quota_state.rs::short_fallback_and_invalid_arithmetic_keep_fixed_expiry_and_owned_debt",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-5",
      "test_id": "tests/quota_state.rs::partial_headers_never_slide_owned_deadlines_and_backward_clock_never_resurrects",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-5",
      "test_id": "tests/quota_state.rs::generated_interleavings_match_independent_uncompacted_accounting_oracle",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::partial_headers_never_slide_owned_deadlines_and_backward_clock_never_resurrects",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::precedence_keeps_every_blocker_and_unknown_transition",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::generated_interleavings_match_independent_uncompacted_accounting_oracle",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::legacy_pairing_preserves_deadline_and_pending_debt_and_corruption_fails_closed",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::independent_concurrent_gates_cannot_overspend_one_paired_remaining_request",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::bounded_observation_storage_fails_closed_without_sliding_or_discarding_live_ownership",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::short_fallback_and_invalid_arithmetic_keep_fixed_expiry_and_owned_debt",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-7",
      "test_id": "tests/cache_coordination.rs::persisted_cooldown_blocks_all_actual_transport_entry_points_in_another_process",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-7",
      "test_id": "tests/quota_backoff.rs::actual_http_and_graphql_backoff_survives_resets_high_observations_reopen_and_all_paths",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-8",
      "test_id": "tests/sdk_quota_outcome.rs::stale_sdk_read_retains_atomic_quota_reason_and_successful_age",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-8",
      "test_id": "tests/sdk_quota_outcome.rs::source_and_lock_outcomes_do_not_classify_failure_message_text",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-8",
      "test_id": "tests/sdk_quota_outcome.rs::bounded_sdk_read_returns_typed_refusal_without_stale_snapshot",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-8",
      "test_id": "tests/sdk_quota_outcome.rs::native_summary_envelope_keeps_legacy_api_and_captured_quota_outcome",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-9",
      "test_id": "tests/sdk_quota_outcome.rs::stale_sdk_read_retains_atomic_quota_reason_and_successful_age",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-9",
      "test_id": "tests/sdk_quota_outcome.rs::source_and_lock_outcomes_do_not_classify_failure_message_text",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-9",
      "test_id": "tests/sdk_quota_outcome.rs::bounded_sdk_read_returns_typed_refusal_without_stale_snapshot",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-9",
      "test_id": "tests/sdk_quota_outcome.rs::legacy_public_literals_compile_and_absent_outcome_is_unknown",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-9",
      "test_id": "tests/sdk_quota_outcome.rs::native_summary_envelope_keeps_legacy_api_and_captured_quota_outcome",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-10",
      "test_id": "tests/quota_state.rs::generated_interleavings_match_independent_uncompacted_accounting_oracle",
      "trace": "correct"
    }
  ]
}
```


## Dispositions

Round1, reviewed frozen f39c31029b0842085ec916d9823eb8ac4ce36b22 after fix commits dac7afd563d7e436bb69dac3c8cd161efff67e74,0cbe749e8e157f6bf11c06190e78e8567f27f564,f39c31029b0842085ec916d9823eb8ac4ce36b22. Original findings and historical verdict above remain unchanged.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Five real-SQLite cutover fixtures now execute old writer SQL after migration, verify transaction rollback and debt/token ownership, quarantine old in-flight HTTP429/GraphQLRATELIMITED, exercise an old admission transaction racing migration, retain known cooldown across reopen/windows and preserve cached partition/success markers. They test the chosen technical fence instead of assuming a completed migration remains isolated. Refs: tests/quota_writer_cutover.rs:71. |
| FND-002 | fixed | New simultaneous-blocker regression invokes both gate.decision and the legacy gate.reserve, asserting earliest reevaluation1s, actual retry3s, unchanged LinearRateLimited variant and exact matching rendered errors. Refs: tests/quota_state.rs:701. |
| FND-003 | fixed | A real LinearGraphQlSource under persisted actual-server backoff now exercises workspace_snapshot_bounded. Its assertions require existing LinearRateLimited60s, captured ServerBackoff metadata, zero queuedHTTP connections, no published snapshot and stable attempt evidence after another gate expires cooldown. Another fixture verifies bounded refresh/reuse/failure/preflight outcomes. Refs: tests/bounded_quota_outcome.rs:92. |

### Round1 evidence and current verdict

**PASS** for this changed-scope review: all three findings have current fixed outcomes and no new concrete regression was found. All six outcomes across SR-6093 and SR-6094 are fixed; no missing or still-open disposition remains. This is mergeable from the reviewer lane, subject to the lead's separately owned final aggregate gate.

82 focused integration tests passed across bounded_quota_outcome2, bounded_read37, cache_coordination5, cache_status3, quota_backoff1, quota_process15, quota_state9, quota_writer_cutover5 and sdk_quota_outcome5. Four focused quota/backoff unit tests passed. Logs: /tmp/SWM-185-code-review/disposition-1-focused.log and /tmp/SWM-185-code-review/disposition-1-units.log. All use local fixture HTTP/SQLite/source/clock seams; no real Linear requests. This pass rechecked fixes and affected behavior without repeating the whole-repository matrix or unrelated audits. No aggregate CI run, source/test/branch edit, commit, push, extra worktree or target directory was made.

The technical fence is a schema compatibility guard, not a security barrier against an operator deliberately altering guard columns or triggers. Actual pre-paired client SQL cannot set the new guard and fails before send. Each new guarded operation is private, starts an IMMEDIATE transaction, and restores committed guard0; error/callback refusal rolls back. Old reservations committed before migration are captured atomically and keep indefinite unknown-response quarantine. Rebuilding does not clear that condition; the documented operator reconciliation remains necessary only if such tokens exist. New-protocol abandoned tokens retain conservative charges without entering legacy quarantine. Existing request/complexity debt, single-use/replay/wrong-store checks, fixed-window/generation behavior and stale/strict-bounded contracts passed the focused regression lanes. Existing actual cooldown remains higher precedence than the quarantine and cannot be shortened by old writes.

Plan completion: not assessed

No additional methods became applicable: code-review/Rust lane and semantic gap disposition were completed; no new spec/plan diff or applicable AssuranceProfile was introduced. has-review was already present.

### Round1 fixed excerpts

```yaml
{
  "dispositions": [
    {
      "after_excerpt": "    let error=old.execute(\"UPDATE linear_shared_quota SET request_limit=10000,request_remaining=250,request_reset=?1 WHERE singleton=1\",[NOW+10000]).unwrap_err();\n    assert_upgrade(&error);\n    assert_eq!(\n        current.decision(10).unwrap().next_reevaluation_at,\n        Some(NOW + 1000),\n        \"rejected old update cannot slide imported evidence\"",
      "fnd": "FND-001",
      "outcome": "fixed"
    },
    {
      "after_excerpt": "    assert_eq!(decision.next_reevaluation_at, Some(NOW + 1000));\n    let error = gate.reserve(10, None).unwrap_err();\n    assert!(\n        matches!(error,ix_board::Error::LinearRateLimited {retry_after} if retry_after==Duration::from_secs(3))\n    );\n    assert_eq!(error.to_string(), \"Linear rate limited; retry in 3s\");\n    assert_eq!(decision.to_string(), error.to_string());",
      "fnd": "FND-002",
      "outcome": "fixed"
    },
    {
      "after_excerpt": "    let captured = match service.refresh_outcome() {\n        RefreshOutcome::QuotaRefused { decision } => decision.clone(),\n        other => panic!(\"bounded attempt lost typed quota provenance: {other:?}\"),\n    };\n    assert_eq!(captured.state, QuotaState::ServerBackoff);\n    assert_eq!(captured.evaluated_at, QUOTA_NOW);\n    assert_eq!(captured.next_reevaluation_at, Some(QUOTA_NOW + 60_000));",
      "fnd": "FND-003",
      "outcome": "fixed"
    }
  ]
}
```
