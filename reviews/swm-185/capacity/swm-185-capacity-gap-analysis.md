---
id: SR-6096
title: "SWM-185 observation capacity gap-analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; PR84; origin/main...74740f718f79f3168fd9f58ce3741363b7445add; Cargo version/lock, docs/how-it-works.md, src/quota.rs, src/quota_decision.rs, tests/quota_capacity.rs, tests/quota_capacity_envelope.rs, tests/quota_state.rs"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-032", type: references}
---

## Summary

Ticket SWM-185, PR agent-ix/ix-board#84. Reviewed exact frozen HEAD 74740f718f79f3168fd9f58ce3741363b7445add against base fcb0b35872cda1ece51f47ce854535919c75b45e in the sole assigned worktree. The fix raises the finite retained-observation bound to 131072, adds nondestructive SQLite indexes, and summarizes explanatory evidence while continuing to evaluate every retained constraint and return every actual blocker. No defect was found in the changed scope.

## Verdict

**PASS** — no findings. The prior production availability defect is exercised beyond the former 512-row limit and the prior lost-evidence hold remains untouched until its owning expiry. No approval gate or assurance profile is inferred.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Coverage

Methods: code-review with rust-review/rust-style API, implementation, wire/persistence, safety/concurrency and testing references; separate quoin:gap-analysis with the explicitly authorized semantic intent/test/code assessment. Repository AGENTS/CLAUDE, Cargo/Make configuration and the exact eight-path diff were inspected. No spec/plan changes require another spec-review method. No applicable AssuranceProfile exists. No gate weakening, duplicated evaluator/transport, new writer protocol, unsafe implementation, source stub, test stub or test-only production bypass was found. The capacity constant has one production owner. Fixture constants express boundary scenarios without exposing production tokens or adding production test hooks.

The only persistence additions are indexes on expires_at and (dimension,remaining). They do not alter ledger/schema fields, ownership, charge accounting, paired expiry, trigger fencing or cooldown semantics. fcb0b358 writers remain accounting-compatible but keep their 512 capacity and can extend the unknown-evidence hold; every refreshing consumer must adopt the accepted0.7.2 version before runtime verification. No source/API signature or existing public literal field was changed.

Each observation is still evaluated before representative evidence is selected. The representative comparison uses i128 effective_remaining-minus-reserve, then owning deadline/id for deterministic ties. Fixed request estimate per dimension makes this the restrictive margin for that labeled evaluation. Actual blockers are appended independently after summarization, so simultaneous constraints and each owning deadline remain available to primary precedence and earliest reevaluation. Underflow handling, corrupt provenance refusal and cap-overflow fail-closed behavior remain in force. All131072 observations can still be retained; nothing is retired except by the existing dominance/owning-expiry rules. All actual blockers may produce a large diagnostic; the8KiB fixture bound applies to its ordinary nonblocking/small-blocker scenarios, not to an arbitrary blocker population.

Normal-volume evidence: the controlled-clock production fixture admits and settles6750 Business operations across three fixed local windows, including adjacent boundary bursts, all three header dimensions and actual complexity125. It independently keeps a chronological VecDeque of unexpired responses, asserts request/complexity headroom before each production reservation, checks exact row ownership versus3x frontier length and local usage after every full window, and reopens between windows. It crosses the failed512-row limit through real admission/response code, not direct insertion. The pre-fix mutation log /private/tmp/swm185-cap-old-red.log fails at zero-based request171 (the172nd attempt), before its configured allocation, demonstrating sensitivity to the production defect. That historical mutation was inspected; frozen source was not mutated by this reviewer.

Resource-bound evidence: the maximum fallback fixture derives capacities for all eight supported tier/auth pairs, checks the largest local allocation12500, and bounds two adjacent complete populations at75000 rows and three at112500. The75000-row fixture seeds a valid non-dominated chronological constraint frontier in real SQLite for tractable resource setup, then uses public production decision, reservation, settlement, response ingestion, refusal, exact owning expiry and reopen. It verifies an additional three observations are retained via real ingestion, two independent request blockers remain visible, remote expiry does not refund local usage, and diagnostics stay compact for that scenario. The hard-bound fixture separately seeds131072 constraints and drives one additional real ingestion, requiring EvidenceUnavailable until the discarded constraint's absolute expiry. These bulk setups avoid quadratic seeding; they do not replace the evaluator or bypass accounting on the asserted operations.

The chosen finite bound covers the documented fallback envelope with a one-window observation horizon. It is not represented as a universal guarantee for arbitrary higher server limits, unusually short configured windows, unbounded future reset horizons or unlimited header-only callers. Those cases still hit an honest fail-closed bound. The docs and source comment state that qualification. Increasing the bound never clears a previously persisted evidence_error_until; the upgrade fixture reproduces555 local requests/194396 complexity, retains600 new constraints, refuses until the old absolute deadline, then admits an ordinary protected request without resetting usage or inventing a cooldown.

Reconciliation: quire matrix (quire0.52.0, engine0.62.2), /tmp/SWM-185-capacity-review/matrix.json. All ten FR-032 criteria are tagged; new volume/envelope tests bind AC5/6/10 through imported #[trace]. The full-repository background remains114 tagged,177 untagged,9 method-without-symbol. Historical diagnostics and out-of-diff baseline coverage gaps are recorded without implying a whole-repository assurance PASS or adding unrelated fix requirements to this PR. No execution-evidence store was read.

Plan completion: not assessed

Changed-behavior reverse inventory: retained-capacity policy, representative evidence serialization, nondestructive indexing and regression/resource fixtures all map to FR-032 AC2/4/5/6/10; FR-031 supplies existing quota/local-allocation context. Changed-scope unowned behaviors0; source stubs0; test stubs0. No unrelated repository implementations were re-reviewed.

Focused validation:86 integration tests passed in quota_capacity2, quota_capacity_envelope2, quota_state9, quota_backoff1, quota_process15, quota_writer_cutover5, bounded_quota_outcome2, sdk_quota_outcome5, bounded_read37, cache_status3, cache_coordination5. Four focused quota/backoff units passed. Logs: /tmp/SWM-185-capacity-review/focused.log and units.log. No aggregate gate, actual Linear API call, live cache access, source/test/branch mutation, extra worktree or extra target directory. The lead owns the final aggregate gate and live installed-cache/ARCH acceptance. This review does not claim the running production service is repaired merely because fixture tests pass.

## Examined units

```yaml
{
  "scope": [
    {
      "excerpt": "A fixture HTTP 429 with a non-JSON body and Retry-After, and a GraphQL RATELIMITED fixture with exhausted request/complexity resets, persist the latest applicable retry-until; two observations never shorten it; absent usable metadata yields 60 seconds.",
      "id": "FR-031-AC-1",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
    },
    {
      "excerpt": "After one process records cooldown, a separately launched process using the same cache attempts workspace, direct issue, history, probe, and force-refresh reads and sends zero requests until expiry; after expiry one eligible attempt is permitted.",
      "id": "FR-031-AC-2",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
    },
    {
      "excerpt": "With 2.1 seconds remaining the actual-server diagnostic is exactly `Linear rate limited; retry in 3s`; no retry or sleep occurs, and an empty cache fails without network.",
      "id": "FR-031-AC-3",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
    },
    {
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct typed diagnostic under [FR-032](FR-032-quota-admission-recovery.md).",
      "id": "FR-031-AC-4",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
    },
    {
      "excerpt": "Counters record requests attempted, returned complexity when available, estimated complexity otherwise, duration, mode, and success/failure for each refresh; partial failed refresh costs remain counted and are never described as a successful sync.",
      "id": "FR-031-AC-5",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
    },
    {
      "excerpt": "`ix-board cache status` returns cache path, dataset coverage, snapshot age/staleness, last successful sync/full reconciliation, `rate_limit_until`, local budget/reset and latest refresh counters in JSON and text without resolving credentials or making any network request; unknown values are explicit.",
      "id": "FR-031-AC-6",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
    },
    {
      "excerpt": "Tier fixtures produce the documented API-key/OAuth fallback limits and multipliers, default to free/basic with at most 50 percent share, and honor lower actual header capacity; explicit shares 0.1 and 0.5 apply to both request and complexity capacities, while zero, negative, above-0.5, non-finite, or malformed shares fail before network access. Two API keys sharing the cache consume one allowance and a query exceeding 10,000 estimated complexity is refused locally.",
      "id": "FR-031-AC-7",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "context_only"
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
    }
  ]
}
```

## Test bindings examined

```yaml
{
  "bindings": [
    {
      "ac_id": "FR-032-AC-4",
      "test_id": "tests/quota_state.rs::bounded_observation_storage_fails_closed_without_sliding_or_discarding_live_ownership",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-5",
      "test_id": "tests/quota_capacity.rs::full_business_allocation_across_three_windows_keeps_non_dominated_evidence_available",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-5",
      "test_id": "tests/quota_capacity.rs::upgraded_capacity_preserves_previous_lost_evidence_hold_until_its_absolute_expiry",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-5",
      "test_id": "tests/quota_capacity_envelope.rs::documented_fallback_allocations_fit_the_maximum_enterprise_oauth_envelope",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-5",
      "test_id": "tests/quota_capacity_envelope.rs::seventy_five_thousand_frontier_rows_admit_settle_expire_and_reopen_without_evidence_loss",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_capacity.rs::full_business_allocation_across_three_windows_keeps_non_dominated_evidence_available",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_capacity.rs::upgraded_capacity_preserves_previous_lost_evidence_hold_until_its_absolute_expiry",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_capacity_envelope.rs::seventy_five_thousand_frontier_rows_admit_settle_expire_and_reopen_without_evidence_loss",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-6",
      "test_id": "tests/quota_state.rs::bounded_observation_storage_fails_closed_without_sliding_or_discarding_live_ownership",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-10",
      "test_id": "tests/quota_capacity.rs::full_business_allocation_across_three_windows_keeps_non_dominated_evidence_available",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-10",
      "test_id": "tests/quota_capacity.rs::upgraded_capacity_preserves_previous_lost_evidence_hold_until_its_absolute_expiry",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-10",
      "test_id": "tests/quota_capacity_envelope.rs::documented_fallback_allocations_fit_the_maximum_enterprise_oauth_envelope",
      "trace": "correct"
    },
    {
      "ac_id": "FR-032-AC-10",
      "test_id": "tests/quota_capacity_envelope.rs::seventy_five_thousand_frontier_rows_admit_settle_expire_and_reopen_without_evidence_loss",
      "trace": "correct"
    }
  ]
}
```
