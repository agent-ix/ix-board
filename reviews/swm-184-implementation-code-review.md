---
id: SR-6085
title: "SWM-184 code-review implementation review"
type: SpecReview
analysis: code-review
review_set: subset
scope: "agent-ix/ix-board; SWM-184; PR80; implementation diff origin/main...8eab311590845aebf0797cf44ea0192321afebd6"
---

## Summary

Ticket: SWM-184. Reviewed frozen implementation head 8eab311590845aebf0797cf44ea0192321afebd6, base ee9aa71. Code review includes the Rust lane and every applicable rust-style reference (API, implementation, safety/concurrency, wire/persistence, testing; Tokio is not used by changed synchronous code). No applicable AssuranceProfile. Unchanged specification review is outside this implementation review. Explicit delegated decisions on credential-bound partitions, deadline precedence, and fresh detail JSON compatibility were honored.

## Verdict

FAIL. 4 defects require correction in the same PR. No merge or release performed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The default reqwest redirect policy sends additional HTTP requests after one quota reservation. A loopback 307 fixture followed by a valid GraphQL response produced observed_http_sends=2 and requests_used=1. Disable automatic redirects (and ensure no other implicit retry), or admit every transport attempt. Oracle: redirect target receives no unreserved request; metered sends equal actual sends. | src/linear.rs:292,1131-1155 |
| FND-002 | medium | Adding private source_id fields to RoadmapInitiative, RoadmapProject and RoadmapUnplacedProject breaks their existing public struct-literal construction API. An unchanged external RoadmapInitiative { name, projects } consumer fails to compile. Keep stable IDs in an internal grouping representation without changing these public structs. Oracle: an external consumer using each previous public literal and legacy build_roadmap compiles unchanged. | src/program_review.rs:386-410 |
| FND-003 | medium | The native roadmap text renderer drops issue last-activity unavailability for every project and drops native progress, progress source, observation time and project updatedAt for unplaced projects. An unscoped roadmap containing only an unplaced project therefore cannot show its actual native progress or provenance. Render the required native and unavailable fields consistently for placed and unplaced rows. Oracle: text for both row types includes native percentage/source/observation, unavailable issue activity with reason, and distinctly labeled project update time. | src/main.rs:1276-1298 |
| FND-004 | medium | Summary fallback constructs freshness using now captured before the failed refresh and lock wait. After a slow HTTP failure, age_seconds understates actual source age; after a delayed 429, rate_limit_until is earlier than the persisted cooldown because a remaining duration observed after the request is added to the old time. Sample current time when fallback is returned and derive cooldown from authoritative absolute quota state. Oracle: delay a failed expired-summary refresh, then assert age at return and reported retry-until match the cache source-start and stored cooldown within clock precision. | src/summary_cache.rs:65,125-149 |

## Coverage

All 24 changed files inspected: Cargo.lock, Cargo.toml, docs/how-it-works.md, docs/setup.md, src/cache.rs, src/cache_status.rs, src/lib.rs, src/linear.rs, src/linear_summary.rs, src/main.rs, src/model.rs, src/program_review.rs, src/quota.rs, src/retro/live.rs, src/retro/sources.rs, src/settings.rs, src/summary_cache.rs, tests/bounded_read.rs, tests/cache_coordination.rs, tests/cache_status.rs, tests/delta_cache.rs, tests/integration.rs, tests/quota_process.rs, tests/summary_cache.rs. No CI lane or threshold changes. Duplication search found no new vendored source, copied schema, duplicate helper rule, source/test stub or suppressed gate. Detail delta edges, tombstones, partition mismatches, source-start watermarks, archive flags, full reconciliation, independent native catalogs, shared quotas, reset/header fallbacks and bounded deadline paths inspected.

Reviewer make fmt-check and make lint pass; focused six-suite rerun passes 60 top-level tests (37 bounded, 5 process cache, 2 cache status, 2 delta, 8 quota, 6 summary), /tmp/SWM-184-reviewer-tests.log. Coder final focused suite log independently inspected, with one pre-existing ignored NFR benchmark. No real Linear API contacted. Redirect reproduction /tmp/SWM-184-redirect.rs compiled against frozen built rlib: two HTTP sends, one reservation. Compatibility reproduction /tmp/SWM-184-api-compat.rs fails external literal compilation as reported. Aggregate make ci/deny/unsafe gate intentionally reserved for lead once after review, per dispatch. No branch edits. Exact runtime model identifier was not exposed by the harness; marker records that limitation.

## Examined scope

```yaml
{
  "clean": false,
  "scope": [
    {
      "id": "FR-004-AC-1",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an unexpired cache entry, repeated fixture reads use the cached value and return identical results without another source read."
    },
    {
      "id": "FR-004-AC-2",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an expired entry and a changed fixture source, the next read refreshes and returns the changed value."
    },
    {
      "id": "FR-004-AC-3",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an expired complete entry and a failed or rate-limited fixture refresh, an ordinary read returns that entry labeled stale with its unchanged actual age and cooldown in JSON and text; without a complete entry it returns a typed error; a bounded read returns no stale snapshot."
    },
    {
      "id": "FR-004-AC-4",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given identical cache state and fixture source, repeated reads return identical results."
    },
    {
      "id": "FR-004-AC-5",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an absolute `XDG_CACHE_HOME` on Unix with `HOME` unset or set, the CLI and `ReadService::open_default` both select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3`; given no absolute `XDG_CACHE_HOME` and a set `HOME`, both select `<HOME>/.cache/ix-board/linear.sqlite3`; given `IX_BOARD_CACHE_PATH`, both select that path instead."
    },
    {
      "id": "FR-004-AC-6",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a non-absolute or absent `XDG_CACHE_HOME`, the library falls back to the available home directory; given neither an absolute XDG cache directory nor a home directory, the library returns `CacheDirectoryUnavailable` and invents no path."
    },
    {
      "id": "FR-004-AC-7",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Project queries use a page size of 20 records."
    },
    {
      "id": "FR-004-AC-8",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Issue queries use a page size of 50 records."
    },
    {
      "id": "FR-004-AC-9",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A structured RATELIMITED response produces a typed LinearRateLimited error."
    },
    {
      "id": "FR-004-AC-10",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A rate-limit error carries the cooldown derived from exhausted quota resets."
    },
    {
      "id": "FR-004-AC-11",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Without a read policy, existing method signatures require no new argument and preserve the 30-second per-request timeout; TTL is 300 seconds by default, explicit environment overrides configuration, and zero/negative/non-integer TTL fails before a request."
    },
    {
      "id": "FR-004-AC-12",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds or more is refused as an invalid policy with no source read."
    },
    {
      "id": "FR-004-AC-13",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a snapshot whose source read began 250 seconds ago but whose cache write or report occurred 10 seconds ago, a bounded read with maximum age 240 seconds refreshes it; an entry with no evidence of when its source read began is refreshed rather than accepted; the returned fetch time is the actual source fetch time."
    },
    {
      "id": "FR-004-AC-14",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds."
    },
    {
      "id": "FR-004-AC-15",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a source whose credential resolution takes longer than the deadline before it becomes ready, a bounded read still performs its full budgeted read after readiness; the bounded read never invokes credential resolution, never retries it, and returns an existing credential error unchanged when the source reports one."
    },
    {
      "id": "FR-004-AC-16",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker."
    },
    {
      "id": "FR-004-AC-17",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A source that cannot honor a bound returns the explicit unsupported-bound error before any source read; its legacy read method still works unchanged."
    },
    {
      "id": "FR-004-AC-18",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A structured RATELIMITED response during a bounded read returns the existing typed rate-limit error with its cooldown, not a timeout, and the library does not retry or sleep; a deadline that expires first returns the timeout."
    },
    {
      "id": "FR-004-AC-19",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a cache that is locked for longer than the deadline, a bounded read returns the typed timeout within the deadline and does not read the source; a refreshed entry is written to the cache only before expiry."
    },
    {
      "id": "FR-004-AC-20",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Two separately launched processes read the same expired dataset concurrently; exactly one source refresh occurs and the waiter reuses it after rechecking. A held lock causes ordinary fallback/error within five seconds, and terminating the owner releases the OS lock without a stale lockfile preventing recovery."
    },
    {
      "id": "FR-004-AC-21",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A summary refresh, failed refresh, or fallback never advances detail freshness; pre-provenance snapshots have unknown age and fail bounded freshness checks. Fallback preserves unavailable components and does not invent zero values."
    },
    {
      "id": "FR-019-AC-1",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "The scope loader accepts the documented TOML program, initiative, project placement, repository, lane, and out-of-scope fields; rejects unknown fields and invalid or duplicate placements; and does not accept stored tallies."
    },
    {
      "id": "FR-019-AC-2",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Given the scope and live Linear catalogs, the check reports missing initiatives, missing projects, and unplaced projects; given GitHub results, it reports missing repositories and treats an unauthenticated 404 as unverifiable rather than asserting absence."
    },
    {
      "id": "FR-019-AC-3",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Every GitHub repository request has a finite timeout; a timed-out request returns an error identifying the repository and does not produce a successful check report."
    },
    {
      "id": "FR-019-AC-4",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Scope checking is read-only with respect to Linear and GitHub."
    },
    {
      "id": "FR-019-AC-5",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Cold and warm scope checks fetch no issue/history pages, reuse summary catalogs, and expose stale/age/cooldown metadata when using a last complete catalog."
    },
    {
      "id": "FR-020-AC-1",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "The roadmap groups projects under their initiative memberships, places a shared project exactly once under its declared primary initiative when scoped (or deterministic primary membership when unscoped), and reports remaining memberships as `also_in`."
    },
    {
      "id": "FR-020-AC-2",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "A cold roadmap performs no issue/history enumeration, reports native progress with `linear_native` provenance and observation time, and distinguishes it from ticket completion; unsupported count/activity fields are present as null with unavailable reasons in JSON and text. Project update time is never mislabeled issue activity and no scope tally is used."
    },
    {
      "id": "FR-020-AC-3",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "Scoped output contains only scope-declared initiatives and projects; the roadmap reports declared initiatives without projects and projects in multiple initiatives, and an unscoped roadmap reports projects with no initiative membership as unplaced."
    },
    {
      "id": "FR-020-AC-4",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "Initiative, project, and diagnostic rows, including each `also_in` membership list, are ordered case-insensitively by name, with original spelling as a deterministic tie-breaker when names have the same case-folded value."
    },
    {
      "id": "FR-020-AC-5",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "The roadmap uses a structured Linear snapshot and performs no Linear or GitHub writes."
    },
    {
      "id": "FR-020-AC-6",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "A scoped roadmap reports scope-declared initiatives and projects absent from Linear in `missing_scope_initiatives` and `missing_scope_projects`; reports Linear projects assigned to a scope-declared initiative but absent from the scope file in `projects_missing_from_scope`; and reports each scope-listed project with differing declared and Linear initiative memberships, with declared memberships absent from Linear as `missing_from_linear` and Linear memberships absent from the scope file as `missing_from_scope`."
    },
    {
      "id": "FR-030-AC-1",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Given a baseline, a fixture delta adds and updates records, removes observed archive/trash entries and obsolete memberships, preserves unchanged IDs, and produces the same result when replayed; all three entity queries use the last successful sync minus 60 seconds and include archived records."
    },
    {
      "id": "FR-030-AC-2",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "A failed or rate-limited later delta page leaves the entire dataset, high-water mark, and successful fetch provenance unchanged; the next attempt uses the previous successful high-water mark."
    },
    {
      "id": "FR-030-AC-3",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source catalog is removed. An ordinary intervening refresh uses only deltas, and output exposes the last full reconciliation time."
    },
    {
      "id": "FR-030-AC-4",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Cold roadmap and scope-check fixtures request no issue pages or histories; requesting a ticket-dependent report loads detail, subsequent reports reuse it, and a summary refresh never advances detail freshness."
    },
    {
      "id": "FR-030-AC-5",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Two retro workers sharing a fixture cache reuse the centrally coordinated snapshot/delta without each probing and forcing a full workspace fetch; ticket/history requests remain covered by the shared quota meter."
    },
    {
      "id": "FR-030-AC-6",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Fixtures independently change source endpoint, workspace, credential-derived authorization identity, cache-model/schema version, and dataset coverage while retaining overlapping stable record IDs; each mismatch (or absent compatibility metadata) requires a full baseline, never merges or returns the old partition as fallback, and a failed/blocked baseline exposes no old-partition records. An exact partition match permits incremental reuse; changing credentials does not reset the shared quota allowance."
    },
    {
      "id": "FR-031-AC-1",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "A fixture HTTP 429 with a non-JSON body and Retry-After, and a GraphQL RATELIMITED fixture with exhausted request/complexity resets, persist the latest applicable retry-until; two observations never shorten it; absent usable metadata yields 60 seconds."
    },
    {
      "id": "FR-031-AC-2",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "After one process records cooldown, a separately launched process using the same cache attempts workspace, direct issue, history, probe, and force-refresh reads and sends zero requests until expiry; after expiry one eligible attempt is permitted."
    },
    {
      "id": "FR-031-AC-3",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "With 2.1 seconds remaining the actual-server diagnostic is exactly `Linear rate limited; retry in 3s`; no retry or sleep occurs, and an empty cache fails without network."
    },
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
    },
    {
      "id": "FR-031-AC-6",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "`ix-board cache status` returns cache path, dataset coverage, snapshot age/staleness, last successful sync/full reconciliation, `rate_limit_until`, local budget/reset and latest refresh counters in JSON and text without resolving credentials or making any network request; unknown values are explicit."
    },
    {
      "id": "FR-031-AC-7",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "Tier fixtures produce the documented API-key/OAuth fallback limits and multipliers, default to free/basic with at most 50 percent share, and honor lower actual header capacity; explicit shares 0.1 and 0.5 apply to both request and complexity capacities, while zero, negative, above-0.5, non-finite, or malformed shares fail before network access. Two API keys sharing the cache consume one allowance and a query exceeding 10,000 estimated complexity is refused locally."
    },
    {
      "id": "src/program_review.rs::RoadmapInitiative",
      "path": "src/program_review.rs",
      "role": "examined",
      "excerpt": "Preserve existing public builder SDK APIs."
    }
  ],
  "findings": [
    {
      "fnd": "FND-001",
      "severity": "high",
      "confidence": "high",
      "method": "code-review",
      "check": "code-bug",
      "artifact_id": "FR-031-AC-4",
      "related_ids": [],
      "path": "src/linear.rs",
      "lines": "292,1131-1155",
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct diagnostic.",
      "finding": "The default reqwest redirect policy sends additional HTTP requests after one quota reservation. A loopback 307 fixture followed by a valid GraphQL response produced observed_http_sends=2 and requests_used=1. Disable automatic redirects (and ensure no other implicit retry), or admit every transport attempt. Oracle: redirect target receives no unreserved request; metered sends equal actual sends."
    },
    {
      "fnd": "FND-002",
      "severity": "medium",
      "confidence": "high",
      "method": "code-review",
      "check": "code-bug",
      "artifact_id": "src/program_review.rs::RoadmapInitiative",
      "related_ids": [],
      "path": "src/program_review.rs",
      "lines": "386-410",
      "excerpt": "Preserve existing public builder SDK APIs.",
      "finding": "Adding private source_id fields to RoadmapInitiative, RoadmapProject and RoadmapUnplacedProject breaks their existing public struct-literal construction API. An unchanged external RoadmapInitiative { name, projects } consumer fails to compile. Keep stable IDs in an internal grouping representation without changing these public structs. Oracle: an external consumer using each previous public literal and legacy build_roadmap compiles unchanged."
    },
    {
      "fnd": "FND-003",
      "severity": "medium",
      "confidence": "high",
      "method": "code-review",
      "check": "code-bug",
      "artifact_id": "FR-020-AC-2",
      "related_ids": [],
      "path": "src/main.rs",
      "lines": "1276-1298",
      "excerpt": "A cold roadmap performs no issue/history enumeration, reports native progress with `linear_native` provenance and observation time, and distinguishes it from ticket completion; unsupported count/activity fields are present as null with unavailable reasons in JSON and text. Project update time is never mislabeled issue activity and no scope tally is used.",
      "finding": "The native roadmap text renderer drops issue last-activity unavailability for every project and drops native progress, progress source, observation time and project updatedAt for unplaced projects. An unscoped roadmap containing only an unplaced project therefore cannot show its actual native progress or provenance. Render the required native and unavailable fields consistently for placed and unplaced rows. Oracle: text for both row types includes native percentage/source/observation, unavailable issue activity with reason, and distinctly labeled project update time."
    },
    {
      "fnd": "FND-004",
      "severity": "medium",
      "confidence": "high",
      "method": "code-review",
      "check": "code-bug",
      "artifact_id": "FR-004-AC-3",
      "related_ids": [],
      "path": "src/summary_cache.rs",
      "lines": "65,125-149",
      "excerpt": "Given an expired complete entry and a failed or rate-limited fixture refresh, an ordinary read returns that entry labeled stale with its unchanged actual age and cooldown in JSON and text; without a complete entry it returns a typed error; a bounded read returns no stale snapshot.",
      "finding": "Summary fallback constructs freshness using now captured before the failed refresh and lock wait. After a slow HTTP failure, age_seconds understates actual source age; after a delayed 429, rate_limit_until is earlier than the persisted cooldown because a remaining duration observed after the request is added to the old time. Sample current time when fallback is returned and derive cooldown from authoritative absolute quota state. Oracle: delay a failed expired-summary refresh, then assert age at return and reported retry-until match the cache source-start and stored cooldown within clock precision."
    }
  ],
  "bindings": []
}
```
