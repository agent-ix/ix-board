---
id: SR-6084
title: "spec-review/failure-domain review of SWM-184"
type: SpecReview
analysis: failure-domain
scope: "agent-ix/ix-board; SWM-184; nine changed specification artifacts"
review_set: subset
---


## Disposition verdict (round 1)

PASS. Inspected pushed frozen head 02cbea072ee8dd6939b7a64a4453dd3386e96caa against ba55d51. Both narrow fixes preserve the strict bounded-read contract and shared cross-credential quota gate. Changed FR-030/FR-031 validation and git diff --check pass. No new findings; no implementation acceptance claimed. Original review text remains unchanged.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Verified committed correction at 02cbea072ee8dd6939b7a64a4453dd3386e96caa; exact after excerpt below. |

## Disposition evidence (round 1)

```yaml
{
  "dispositions": [
    {
      "fnd": "FND-001",
      "outcome": "fixed",
      "after_excerpt": "Test |"
    }
  ]
}
```

## Summary

Ticket: SWM-184. Reviewed PR ix-board#79 at ba55d51da548328d1d0b2d64d930029182389c68. spec-review/failure-domain uses the selected base plus failure-domain scope. No AssuranceProfile or review_policy exists under spec, so no enforced review run applies. This is a specification-only candidate; implementation and test acceptance are deferred to its implementation PR.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | Baseline compatibility has no defined source or visibility identity. Reusing deltas after a workspace or authorization-visibility change can preserve inaccessible unchanged records until reconciliation. Define compatibility by source/workspace, authorization visibility partition, dataset coverage and persistence schema; require a fresh authoritative baseline on mismatch and a fixture proving no cross-partition reuse. | spec/functional/FR-030-incremental-linear-cache.md:21,31-32,45 |

## Verdict

CONDITIONAL. Reviewed every statement and acceptance criterion listed below. The strict bounded-read ceiling, deadlines, no-stale behavior, atomic delta publication, observed-tombstone limitation, and native-summary unknown counts remain explicit.

## Validation evidence

Explicit validation of all nine changed files passed (exit 0), with advisory EARS warnings. git diff --check passed. Full-spec validation independently failed on unchanged FR-017 missing Dependencies and spec/roadmap/ears-campaign-stories.md missing type; git diff and origin/main content confirm these predate this PR. Installed quoin 0.36.7 and quire 0.52.0 (engine 0.62.2), Draco. Matrix strict exited 1 with 196 untagged criteria and 32 design gaps; new requirements await implementation and no implemented-coverage claim is made. No hashes, digests, pins or repository-file tracking requirements were introduced. Numeric quota defaults match https://linear.app/developers/rate-limiting. Unselected lenses: integrity, dependency, evidence, risk-complexity, scope-boundary, ears-conformance. No production code means no rust-review, review-react, or gap-analysis.

## Examined scope

```yaml
{
  "clean": false,
  "scope": [
    {
      "id": "FR-004-statement-1",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "The ix-board read library SHALL use shared SQLite storage to cache structured\nLinear read data. Ordinary reads SHALL reuse a snapshot within the configured\nTTL and otherwise attempt the coordinated incremental refresh defined by\n[FR-030](FR-030-incremental-linear-cache.md). TTL SHALL resolve from\n`IX_BOARD_CACHE_TTL_SECONDS`, then `[cache].ttl_seconds` in configuration, then\n300 seconds. Values SHALL be positive integer seconds; invalid explicit values\nSHALL fail before network access. The default remains five minutes after adopting\nincremental reads; a longer lifetime is not the quota remedy."
    },
    {
      "id": "FR-004-statement-2",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Before refreshing a dataset, the coordinator SHALL acquire an exclusive OS file\nlock adjacent to the shared SQLite file and recheck freshness and quota state\nafter acquisition. The lock SHALL cover refresh and atomic publication; lock\nownership SHALL be released by RAII and process termination. An ordinary wait\nSHALL last at most five seconds; a bounded wait SHALL also respect its remaining\ndeadline. Waiters SHALL reuse a completed refresh rather than duplicate it. A\nforced refresh SHALL bypass TTL only, never quota, locking, or freshness recheck\nagainst a refresh completed after the operation"
    },
    {
      "id": "FR-004-statement-3",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "If cooldown, local quota refusal, lock timeout, or refresh failure prevents an\nordinary read, the service SHALL return its last complete compatible snapshot\nwhen available. All such CLI results SHALL expose `stale`, `age_seconds`, and\n`rate_limit_until` in JSON and equivalent labeled text; age SHALL refer to the\nlast successful source-fetch start, never the failed attempt. Unknown age SHALL\nbe null/unknown and SHALL NOT be reported fresh. The result SHALL retain the\nfailure reason; an unavailable component SHALL remain unavailable rather than\nbecoming a zero count. Without a complete snapshot "
    },
    {
      "id": "FR-004-statement-4",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "The library SHALL additionally offer a **bounded read** that a caller opts into\nwith an explicit read policy: a maximum cache age and an operation-wide\ndeadline. The policy is a parameter of the new operation only. Every existing\nread method keeps its signature and the 30-second per-request transport timeout;\nordinary reads adopt the coordinated TTL and explicitly labeled fallback above."
    },
    {
      "id": "FR-004-statement-5",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "- **Age.** A cached entry satisfies the policy only when the age of the data,\n  measured from when the source read that produced it began, is no greater than\n  the caller's maximum age. The measure is the actual source fetch, never the\n  time the entry was written to the cache, the time a record was reported, or\n  the time of a later read. An entry that cannot show when its source read\n  began (for example an entry written before that evidence was kept) does not\n  satisfy a policy. An entry that does not satisfy the policy is refreshed under\n  the same deadline. A maximum age above the fixed 3"
    },
    {
      "id": "FR-004-statement-6",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "- Structured Linear records obtained through the read client.\n- Cached records and their refresh metadata.\n- The configured ordinary cache-entry lifetime (default five minutes).\n- For a bounded read only: the caller's maximum cache age and operation-wide\n  deadline, and a source that can honor them."
    },
    {
      "id": "FR-004-statement-7",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "- Cached records for reads that do not require a refresh.\n- Refreshed records after a successful poll observes source changes.\n- A labeled last complete snapshot, or typed error without one, when refresh fails.\n- For a bounded read: the snapshot with its actual source fetch time, or a\n  typed timeout, unsupported-bound, invalid-policy, rate-limit, or existing\n  failure error, and no partial snapshot."
    },
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
      "id": "FR-004-statement-8",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)\n- **Consumers**: [FR-010](FR-010-explicit-work-scope.md),\n  [FR-012](FR-012-focus-status-and-ready-queue.md) and\n  [FR-016](FR-016-focus-record.md) use the bounded read for named-focus\n  validation; the consumer chooses its own policy values.\n"
    },
    {
      "id": "FR-019-statement-1",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "The `scope check` command SHALL validate a TOML program scope file against\nstructured Linear initiative and project catalogs and GitHub repository\navailability. It SHALL report missing initiatives, missing projects, missing\nrepositories, and projects that are not placed in the declared scope. It SHALL\nmake no changes to Linear or GitHub."
    },
    {
      "id": "FR-019-statement-2",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "The command SHALL reuse the shared initiative/project summary catalogs without\nenumerating issues or histories; it SHALL expose Linear source freshness even\nwhen a quota cooldown requires the last complete catalog."
    },
    {
      "id": "FR-019-statement-3",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "The scope file SHALL describe program membership and ownership only. It SHALL\nnot persist counts or other values that can be derived from live source data.\nGitHub repository probes SHALL have a bounded request timeout and report a\ntimeout as an error naming the affected repository."
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
      "id": "FR-019-statement-4",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "- Structured Linear initiative and project catalog reads.\n- GitHub repository API reads.\n"
    },
    {
      "id": "FR-020-statement-1",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "The `roadmap` command SHALL build a deterministic program view from one\nstructured Linear workspace snapshot, with an optional program scope file.\nIt SHALL group projects under initiatives, place a shared project once, show\nother initiative memberships, identify projects without an initiative, and\nidentify initiatives without projects. A scoped roadmap SHALL include only\nthe initiatives and projects declared by that scope."
    },
    {
      "id": "FR-020-statement-2",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "The command SHALL use the shared initiative/project summary cache without\nenumerating issues or histories. Each project row SHALL use native Linear\n`Project.progress` for progress and identify its provenance as `linear_native`.\nNative progress is an estimate-weighted fraction with started work contributing\npartial progress; it is not the completed-ticket percentage. Available native\ncount summaries SHALL identify their source and observation time; unsupported\nopen/completed/canceled counts or issue last-activity values SHALL be null with an\nunavailable reason, never inferred from progress or f"
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
      "id": "FR-020-statement-3",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "- Structured Linear initiative/project summary records with the freshness and coverage metadata in [FR-030](FR-030-incremental-linear-cache.md).\n- Optional program scope file conforming to FR-019.\n"
    },
    {
      "id": "FR-030-statement-1",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "The read service SHALL cache initiative/project summaries separately from detailed\nticket datasets, with explicit coverage and source-fetch provenance for each.\nSummary reads SHALL NOT make a detail cache fresh. Commands SHALL request ticket\ndata only when their answer needs tickets. Retro workspace reads and update probes\nSHALL use the shared refresh coordinator rather than forcing duplicate full scans."
    },
    {
      "id": "FR-030-statement-2",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "When a compatible baseline exists, the coordinator SHALL request changed issues,\nprojects, and initiatives using `updatedAt >= last_successful_sync - 60 seconds`,\nmerge by stable ID, and preserve unchanged records. It SHALL include archived\nrecords and remove observed archived/trashed records from active results, including\nobsolete memberships and relationships. Replays SHALL be idempotent. Each dataset's\nhigh-water mark SHALL be its successful refresh start time, committed atomically\nwith the complete merged dataset only after every required page succeeds. Related\ndata whose changes cannot be"
    },
    {
      "id": "FR-030-statement-3",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "The coordinator SHALL perform a full authoritative reconciliation for an absent or\nincompatible baseline and when the last full reconciliation is at least 24 hours\nold. A full reconciliation SHALL remove IDs missing from the accessible source\ncatalog. Since Linear list queries do not enumerate hard deletions, delta output\nSHALL expose the last full reconciliation time and SHALL NOT claim immediate\nhard-deletion completeness. A full detailed scan SHALL occur only when detailed\ndata is requested; a summary-only command SHALL reconcile only summary catalogs."
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
      "id": "FR-030-statement-4",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "- [FR-004](FR-004-cache-polling.md) defines coordinated publication and freshness.\n- [FR-020](FR-020-program-roadmap.md) defines native summary semantics.\n- [FR-031](FR-031-shared-linear-quota.md) applies to all refresh requests.\n"
    },
    {
      "id": "FR-031-statement-1",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "The Linear transport SHALL persist server cooldown and local request-budget state\nin the shared SQLite cache without storing credentials. The quota gate SHALL be\nshared conservatively across credentials on the same machine/cache so two API keys\nfor one authenticated user cannot multiply its allowance. Every Linear request, including direct, history, update probes,\nand forced refresh paths, SHALL consult and reserve against that state before send."
    },
    {
      "id": "FR-031-statement-2",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "When HTTP 429 or a GraphQL `RATELIMITED` error occurs, the transport SHALL persist\nthe latest applicable retry-until from Retry-After, exhausted request/complexity\nreset headers, or structured error retry metadata before returning the error.\nNon-JSON HTTP 429 bodies SHALL retain rate-limit classification. When no usable\nretry metadata exists, the transport SHALL use a documented conservative 60-second\ncooldown. Concurrent observations SHALL never shorten an active cooldown."
    },
    {
      "id": "FR-031-statement-3",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "While server cooldown is active, all processes sharing this cache SHALL issue zero\nLinear requests. The CLI SHALL print exactly `Linear rate limited; retry in <N>s`\non stderr, with N the ceiling of remaining seconds. No automatic sleep or retry\nSHALL occur. Ordinary snapshot reads use the fallback in\n[FR-004](FR-004-cache-polling.md); empty-cache reads fail locally. Bounded reads\nretain their strict error contract."
    },
    {
      "id": "FR-031-statement-4",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "The transport SHALL enforce a configurable conservative request and estimated\ncomplexity budget per time window across those processes, atomically reserving\nbefore send and honoring available remaining-quota/reset headers with a reserve.\nConfiguration SHALL accept plan tiers `free`, `basic`, `business`, and `enterprise`;\nthe default SHALL be free/basic. Tier fallback SHALL apply multipliers 1, 1, 2,\nand 5 respectively to API-key 2,500 requests/3,000,000 complexity per hour or\nOAuth 5,000 requests/2,000,000 complexity per hour, with a 10,000 single-query\ncomplexity ceiling. Actual Limit/Remaini"
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
      "excerpt": "Tier fixtures produce the documented API-key/OAuth fallback limits and multipliers, default to free/basic with at most 50 percent share, reject invalid configuration, and honor lower actual header capacity; two API keys sharing the cache consume one allowance and a query exceeding 10,000 estimated complexity is refused locally."
    },
    {
      "id": "FR-031-statement-5",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "- [FR-004](FR-004-cache-polling.md) defines freshness, single-flight, fallback, and bounded reads.\n- Tests use fixture transports and controlled clocks, never the real Linear API.\n"
    },
    {
      "id": "NFR-001-statement-1",
      "path": "spec/non-functional/NFR-001-warm-cache-latency.md",
      "role": "examined",
      "excerpt": "The ix-board CLI SHALL complete `next --team <T>` in less than 1.0 second when\nthe required team data is available in a warm cache."
    },
    {
      "id": "NFR-001-statement-2",
      "path": "spec/non-functional/NFR-001-warm-cache-latency.md",
      "role": "examined",
      "excerpt": "Warm means a complete compatible dataset within its configured TTL, with no\nrefresh owner holding the lock. The warm path SHALL make zero Linear requests,\nincluding update probes and quota discovery. This target does not increase the\n300-second ceiling for strict bounded reads or justify extending ordinary TTL;\nincremental reuse and summary coverage reduce source costs instead."
    },
    {
      "id": "NFR-001-statement-3",
      "path": "spec/non-functional/NFR-001-warm-cache-latency.md",
      "role": "examined",
      "excerpt": "The benchmark reports elapsed time for each warm-cache run and verifies that\nevery run meets the target.\n"
    },
    {
      "id": "spec-index-statement-1",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "SWM-2 gives team leaders deterministic Linear reads for ready work, project\ncontext, and search through the ix-board CLI and library. SWM-3 adds explicit\nproject status and priority writes so team leaders can control the run queue.\nSWM-4 reports evidence of work outside that queue and work that is active but\nnot landing. SWM-64 extends drift with priority-goal staffing and crowding\nassessments using configured distinct-worker minimums and stale thresholds.\nSWM-102 adds program scope validation against live Linear and\nGitHub catalogs; SWM-103 adds a live program roadmap with derived project\npro"
    },
    {
      "id": "spec-index-statement-2",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "- The `next --team <T>`, `project-chunk <project>`, and `search <query>` read\n  commands and their corresponding library behavior.\n- Ready-queue eligibility, blocker layers, reasons, and ordering.\n- Shared incremental Linear reads, summary/detail coverage, and SQLite caching\n  with configurable TTL (default five minutes), coordinated refresh, quota\n  backoff, and explicitly labeled last-snapshot fallback for ordinary reads.\n- Fixture-backed checks for queue layering, readiness, and cache invalidation.\n- The warm-cache latency target and team-leader skill handoff.\n- `project set-status` for `St"
    },
    {
      "id": "spec-index-statement-3",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "- Webhook-based cache refresh; SWM-2 uses polling.\n- LLM-based interpretation or actions based on ticket or comment prose.\n- Mutating Linear fields other than project status and priority, or mutating\n  GitHub data. The retro store (FR-025) is local state outside every repository,\n  not Linear or GitHub data.\n- Combining retro metrics into a single health score, and scoring subjective\n  qualities such as brief clarity or role discipline; the retro commands report\n  structured measures only.\n- Reading or writing GitHub Issues as part of planner focus resolution.\n- Reading or writing GitHub Issue"
    },
    {
      "id": "spec-index-statement-4",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "ix-board is a deterministic Linear/GitHub CLI and library. SWM-2 adds fast,\ncached Linear reads for a team leader: a reasoned ready queue, a project summary,\nand search across tickets and projects. SWM-3 adds narrow project controls:\n`Started` runs a project and `Paused` holds it; project priority participates in\nthe queue order defined by FR-001. SWM-4 adds a read-only drift report for the\nconfigured leads. SWM-102 and SWM-103 add scope validation and live roadmap\nreports. SWM-104 adds read-only project-to-pull-request reconciliation through\nFR-021. SWM-33 adds inline focus expansion from str"
    },
    {
      "id": "spec-index-statement-5",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "- Team leaders use the CLI and team-leader skill to inspect team and project work.\n- Team planners use inline focus selectors to resolve their planned\n  initiatives, projects, issues, and blockers.\n- Team leads use `focus status` to inspect progress, blockers, and ready work\n  within the selected focus.\n- Team planners use inline focus selectors and hygiene reports to inspect\n  planned work, correct tracking gaps, and see changes since a prior report.\n- Team leads use `focus plan-chunk` to hand off an ordered slice of open focus\n  issues with their readiness and dependency context.\n- Program r"
    },
    {
      "id": "spec-index-statement-6",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "SWM-184 adds [US-010](./usecase/US-010-read-within-linear-quota.md),\n[FR-030](./functional/FR-030-incremental-linear-cache.md), and\n[FR-031](./functional/FR-031-shared-linear-quota.md). It revises\n[FR-004](./functional/FR-004-cache-polling.md) ordinary freshness/fallback while\npreserving strict bounded reads, and uses native summaries for\n[FR-020](./functional/FR-020-program-roadmap.md) and\n[FR-019](./functional/FR-019-program-scope-check.md)."
    },
    {
      "id": "spec-index-statement-7",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "The use case US-001 traces to FR-001 through FR-007 and NFR-001. US-002 traces\nto FR-008 and FR-009, which depend on the queue and structured-data boundary.\nUS-003 traces to FR-010 and FR-011, reusing FR-001/FR-002 blocker semantics.\nUS-004 traces to FR-010 through FR-012, composing the existing focus expansion\nand queue rules into a status result.\nUS-005 traces to FR-013, which uses the canonical focus from FR-011 and the\nstructured-source boundary in FR-005.\nUS-006 traces to FR-010, FR-011, FR-012, and FR-014, composing focus\nmembership, blocker layers, and readiness into an ordered handoff "
    },
    {
      "id": "spec-index-statement-8",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "- SWM-2, “ix-board: next, project-chunk and search with cache” (Linear).\n- SWM-4, “ix-board drift: out-of-scope and churn alerts” (Linear).\n- SWM-64, “Detect priority goal crowding and rebalance planner capacity” (Linear).\n- SWM-141, “Specify team-retro metric catalog, snapshot store and ix-board retro CLI contract” (Linear).\n- The ix-board README and source repository.\n"
    },
    {
      "id": "US-008-statement-1",
      "path": "spec/usecase/US-008-review-program-scope-and-roadmap.md",
      "role": "examined",
      "excerpt": "**As a** program reviewer\n**I want** to validate the declared program boundary and view project progress\nfrom live Linear and GitHub data\n**So that** I can run a repeatable review without maintaining hand-written\nscripts or stored tallies."
    },
    {
      "id": "US-008-statement-2",
      "path": "spec/usecase/US-008-review-program-scope-and-roadmap.md",
      "role": "examined",
      "excerpt": "- **Given** a program scope file with initiatives, projects, and repositories\n- **When** I run `ix-board scope check`\n- **Then** ix-board reports missing and unplaced entries using live catalogs\n  and identifies repository checks that could not be verified"
    },
    {
      "id": "US-008-statement-3",
      "path": "spec/usecase/US-008-review-program-scope-and-roadmap.md",
      "role": "examined",
      "excerpt": "- **Given** an optional scope file and Linear initiative/project summaries\n- **When** I run `ix-board roadmap`\n- **Then** ix-board groups projects under initiatives and derives project\n  progress with explicit native-summary provenance, freshness, and unavailable counts"
    },
    {
      "id": "US-008-statement-4",
      "path": "spec/usecase/US-008-review-program-scope-and-roadmap.md",
      "role": "examined",
      "excerpt": "- **Given** a program scope file and one Linear workspace snapshot\n- **When** I run `ix-board orphans --scope <file>`\n- **Then** ix-board reports unprojected open and completed issues for the\n  scoped projects' teams with read-only project suggestions\n"
    },
    {
      "id": "US-010-statement-1",
      "path": "spec/usecase/US-010-read-within-linear-quota.md",
      "role": "examined",
      "excerpt": "**As a** team lead sharing Linear access with other reporting processes\n**I want** useful planning evidence with visible freshness and quota availability\n**So that** repeated reports remain usable without exhausting our shared access."
    },
    {
      "id": "US-010-statement-2",
      "path": "spec/usecase/US-010-read-within-linear-quota.md",
      "role": "examined",
      "excerpt": "- **Given** a previous successful report and a Linear cooldown\n- **When** another process requests the report\n- **Then** it sees the last evidence with its age and cooldown without another Linear request"
    },
    {
      "id": "US-010-statement-3",
      "path": "spec/usecase/US-010-read-within-linear-quota.md",
      "role": "examined",
      "excerpt": "- **Given** a program containing many tickets\n- **When** I request a roadmap or scope check\n- **Then** I receive initiative and project summaries without downloading every ticket"
    },
    {
      "id": "US-010-statement-4",
      "path": "spec/usecase/US-010-read-within-linear-quota.md",
      "role": "examined",
      "excerpt": "- **Given** a named-focus operation requiring a strict freshness bound\n- **When** fresh evidence cannot be obtained within its deadline\n- **Then** it fails without publishing stale evidence\n"
    }
  ],
  "findings": [
    {
      "fnd": "FND-001",
      "severity": "medium",
      "confidence": "high",
      "method": "spec-review/failure-domain",
      "check": "ambiguous",
      "artifact_id": "FR-030-AC-3",
      "related_ids": [
        "FR-030",
        "FR-004"
      ],
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "lines": "21,31-32,45",
      "excerpt": "With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source catalog is removed. An ordinary intervening refresh uses only deltas, and output exposes the last full reconciliation time.",
      "finding": "Baseline compatibility has no defined source or visibility identity. Reusing deltas after a workspace or authorization-visibility change can preserve inaccessible unchanged records until reconciliation. Define compatibility by source/workspace, authorization visibility partition, dataset coverage and persistence schema; require a fresh authoritative baseline on mismatch and a fixture proving no cross-partition reuse."
    }
  ],
  "bindings": []
}
```
