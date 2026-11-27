---
id: FR-004
title: Refresh cached Linear reads by polling
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The ix-board read library SHALL share coordinated SQLite snapshots with explicit freshness and strict opt-in bounded reads.
dependencies: []
---
# FR-004: Refresh cached Linear reads by polling

## Description

The ix-board read library SHALL use shared SQLite storage to cache structured
Linear read data. Ordinary reads SHALL reuse a snapshot within the configured
TTL and otherwise attempt the coordinated incremental refresh defined by
[FR-030](FR-030-incremental-linear-cache.md). TTL SHALL resolve from
`IX_BOARD_CACHE_TTL_SECONDS`, then `[cache].ttl_seconds` in configuration, then
300 seconds. Values SHALL be positive integer seconds; invalid explicit values
SHALL fail before network access. The default remains five minutes after adopting
incremental reads; a longer lifetime is not the quota remedy.

Before refreshing a dataset, the coordinator SHALL acquire an exclusive OS file
lock adjacent to the shared SQLite file and recheck freshness and quota state
after acquisition. The lock SHALL cover refresh and atomic publication; lock
ownership SHALL be released by RAII and process termination. An ordinary wait
SHALL last at most five seconds; a bounded wait SHALL also respect its remaining
deadline. Waiters SHALL reuse a completed refresh rather than duplicate it. A
forced refresh SHALL bypass TTL only, never quota, locking, or freshness recheck
against a refresh completed after the operation began.

If cooldown, local quota refusal, lock timeout, or refresh failure prevents an
ordinary read, the service SHALL return its last complete compatible snapshot
when available. All such CLI results SHALL expose `stale`, `age_seconds`, and
`rate_limit_until` in JSON and equivalent labeled text; age SHALL refer to the
last successful source-fetch start, never the failed attempt. Unknown age SHALL
be null/unknown and SHALL NOT be reported fresh. The result SHALL retain the
failure reason; an unavailable component SHALL remain unavailable rather than
becoming a zero count. Without a complete snapshot the read SHALL fail with a
typed error. No failure SHALL advance successful freshness or sync metadata.
The strict bounded-read contract below overrides ordinary fallback.

## Bounded reads (opt-in)

The library SHALL additionally offer a **bounded read** that a caller opts into
with an explicit read policy: a maximum cache age and an operation-wide
deadline. The policy is a parameter of the new operation only. Every existing
read method keeps its signature and the 30-second per-request transport timeout;
ordinary reads adopt the coordinated TTL and explicitly labeled fallback above.

- **Age.** A cached entry satisfies the policy only when the age of the data,
  measured from when the source read that produced it began, is no greater than
  the caller's maximum age. The measure is the actual source fetch, never the
  time the entry was written to the cache, the time a record was reported, or
  the time of a later read. An entry that cannot show when its source read
  began (for example an entry written before that evidence was kept) does not
  satisfy a policy. An entry that does not satisfy the policy is refreshed under
  the same deadline. A maximum age above the fixed 300-second ceiling is
  refused as an invalid policy; the policy can only be stricter than the
  300-second bounded-read ceiling, independently of the configurable ordinary TTL.
- **Deadline.** The deadline is the bound on the whole operation, measured from
  the start below, with no additional allowance of one request timeout. It covers
  the whole operation: the cache read and any
  cache lock wait, every page of every paginated source query, every nested
  read, and the cache write of a refreshed entry. No request starts after the
  deadline has expired, and each request's timeout is the lesser of its legacy
  per-request timeout and the time remaining. Expiry returns a typed timeout
  error that is distinct from a transport failure and a rate limit.
- **Start.** The deadline starts when the bounded read begins on a source whose
  credentials are already resolved. Credential resolution happens before, under
  its existing operating-system-managed behavior, and may wait for user
  interaction. The bounded read does not bound that wait, and it neither
  resolves credentials again nor retries, falls back, or passes a credential
  around that behavior.
- **No continuation.** A timeout or failure returns no snapshot, stores no
  partial or unfinished data as a fresh entry, and returns no stale entry. It
  leaves no thread, request, or retry running after the call returns, and a
  source completion that arrives after expiry cannot change the cache or the
  result. The library adds no retry or sleep loop.
- **Capability.** A source that cannot honor a requested bound SHALL fail the
  bounded read with an explicit typed error before it reads, and SHALL NOT
  perform an unbounded read in its place. The legacy read method of such a
  source is unchanged.
- **Publication.** The bounded read returns a snapshot only when it is complete
  and within the policy. A caller that persists or publishes a result does so
  after that return, outside the deadline; a timeout therefore leaves the caller
  nothing to publish. Last-good storage and adoption history remain with the
  caller.

## Inputs

- Structured Linear records obtained through the read client.
- Cached records and their refresh metadata.
- The configured ordinary cache-entry lifetime (default five minutes).
- For a bounded read only: the caller's maximum cache age and operation-wide
  deadline, and a source that can honor them.

## Outputs

- Cached records for reads that do not require a refresh.
- Refreshed records after a successful poll observes source changes.
- A labeled last complete snapshot, or typed error without one, when refresh fails.
- For a bounded read: the snapshot with its actual source fetch time, or a
  typed timeout, unsupported-bound, invalid-policy, rate-limit, or existing
  failure error, and no partial snapshot.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-004-AC-1 | Given an unexpired cache entry, repeated fixture reads use the cached value and return identical results without another source read. | Test |
| FR-004-AC-2 | Given an expired entry and a changed fixture source, the next read refreshes and returns the changed value. | Test |
| FR-004-AC-3 | Given an expired complete entry and a failed or rate-limited fixture refresh, an ordinary read returns that entry labeled stale with its unchanged actual age and cooldown in JSON and text; without a complete entry it returns a typed error; a bounded read returns no stale snapshot. | Test |
| FR-004-AC-4 | Given identical cache state and fixture source, repeated reads return identical results. | Test |
| FR-004-AC-5 | Given an absolute `XDG_CACHE_HOME` on Unix with `HOME` unset or set, the CLI and `ReadService::open_default` both select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3`; given no absolute `XDG_CACHE_HOME` and a set `HOME`, both select `<HOME>/.cache/ix-board/linear.sqlite3`; given `IX_BOARD_CACHE_PATH`, both select that path instead. | Test |
| FR-004-AC-6 | Given a non-absolute or absent `XDG_CACHE_HOME`, the library falls back to the available home directory; given neither an absolute XDG cache directory nor a home directory, the library returns `CacheDirectoryUnavailable` and invents no path. | Test |
| FR-004-AC-7 | Project queries use a page size of 20 records. | Test |
| FR-004-AC-8 | Issue queries use a page size of 50 records. | Test |
| FR-004-AC-9 | A structured RATELIMITED response produces a typed LinearRateLimited error. | Test |
| FR-004-AC-10 | A rate-limit error carries the cooldown derived from exhausted quota resets. | Test |
| FR-004-AC-11 | Without a read policy, existing method signatures require no new argument and preserve the 30-second per-request timeout; TTL is 300 seconds by default, explicit environment overrides configuration, and zero/negative/non-integer TTL fails before a request. | Test |
| FR-004-AC-12 | Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds or more is refused as an invalid policy with no source read. | Test |
| FR-004-AC-13 | Given a snapshot whose source read began 250 seconds ago but whose cache write or report occurred 10 seconds ago, a bounded read with maximum age 240 seconds refreshes it; an entry with no evidence of when its source read began is refreshed rather than accepted; the returned fetch time is the actual source fetch time. | Test |
| FR-004-AC-14 | Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds. | Test |
| FR-004-AC-15 | Given a source whose credential resolution takes longer than the deadline before it becomes ready, a bounded read still performs its full budgeted read after readiness; the bounded read never invokes credential resolution, never retries it, and returns an existing credential error unchanged when the source reports one. | Test |
| FR-004-AC-16 | After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker. | Test |
| FR-004-AC-17 | A source that cannot honor a bound returns the explicit unsupported-bound error before any source read; its legacy read method still works unchanged. | Test |
| FR-004-AC-18 | A structured RATELIMITED response during a bounded read returns the existing typed rate-limit error with its cooldown, not a timeout, and the library does not retry or sleep; a deadline that expires first returns the timeout. | Test |
| FR-004-AC-19 | Given a cache that is locked for longer than the deadline, a bounded read returns the typed timeout within the deadline and does not read the source; a refreshed entry is written to the cache only before expiry. | Test |
| FR-004-AC-20 | Two separately launched processes read the same expired dataset concurrently; exactly one source refresh occurs and the waiter reuses it after rechecking. A held lock causes ordinary fallback/error within five seconds, and terminating the owner releases the OS lock without a stale lockfile preventing recovery. | Test |
| FR-004-AC-21 | A summary refresh, failed refresh, or fallback never advances detail freshness; pre-provenance snapshots have unknown age and fail bounded freshness checks. Fallback preserves unavailable components and does not invent zero values. | Test |

## Dependencies

Cache recovery (SWM-399): default path selection verifies restricted permissions,
SQLite writes and the adjacent lock file. Permission/read-only failures select a
private per-user temporary cache automatically; corruption and lock contention
remain errors. An explicit override never silently falls back. Ordinary refresh
lock waits remain bounded to five seconds and identify the lock path and owner PID.

| ID | Criteria | Verification |
| --- | --- | --- |
| FR-004-AC-22 | If the default cache or its adjacent lock is inaccessible or read-only, automatic selection opens a writable private per-user temporary cache. An explicit IX_BOARD_CACHE_PATH failure names the path and ownership/permission or override fix. Corrupt databases and contention are reported without silently switching caches. | Test |
| FR-004-AC-23 | A held refresh lock reports its path and holder PID within five seconds; absent legacy holder metadata is explicitly unknown. Stale metadata cannot prevent acquisition after the OS releases the lock. Bounded reads retain their operation deadline. | Test |

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
- **Consumers**: [FR-010](FR-010-explicit-work-scope.md),
  [FR-012](FR-012-focus-status-and-ready-queue.md) and
  [FR-016](FR-016-focus-record.md) use the bounded read for named-focus
  validation; the consumer chooses its own policy values.
