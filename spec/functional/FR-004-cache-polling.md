---
id: FR-004
title: Refresh cached Linear reads by polling
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-2
description: The ix-board read library SHALL use a local SQLite cache and refresh cached Linear data on a five-minute polling cadence.
dependencies: []
---
# FR-004: Refresh cached Linear reads by polling

## Description

The ix-board read library SHALL use SQLite to cache structured Linear read data.
Each cache entry expires five minutes after it is fetched. A read of an expired
entry SHALL refresh it from Linear before returning. A refresh failure SHALL
return an error. Stale cache data SHALL NOT be returned after a failed refresh.
A successful refresh SHALL replace the expired value.

## Bounded reads (opt-in)

The library SHALL additionally offer a **bounded read** that a caller opts into
with an explicit read policy: a maximum cache age and an operation-wide
deadline. The policy is a parameter of the new operation only. Every existing
read method keeps its legacy behavior: the five-minute entry lifetime, the
30-second per-request transport timeout, the existing typed errors, and no
deadline.

- **Age.** A cached entry satisfies the policy only when the age of the data,
  measured from when the source read that produced it began, is no greater than
  the caller's maximum age. The measure is the actual source fetch, never the
  time the entry was written to the cache, the time a record was reported, or
  the time of a later read. An entry that cannot show when its source read
  began (for example an entry written before that evidence was kept) does not
  satisfy a policy. An entry that does not satisfy the policy is refreshed under
  the same deadline. A maximum age above the five-minute entry lifetime is
  refused as an invalid policy; the policy can only be stricter than the
  legacy lifetime.
- **Deadline.** The deadline covers the whole operation: the cache read and any
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
- The five-minute cache-entry lifetime.
- For a bounded read only: the caller's maximum cache age and operation-wide
  deadline, and a source that can honor them.

## Outputs

- Cached records for reads that do not require a refresh.
- Refreshed records after a successful poll observes source changes.
- An error when an expired entry cannot be refreshed.
- For a bounded read: the snapshot with its actual source fetch time, or a
  typed timeout, unsupported-bound, invalid-policy, rate-limit, or existing
  failure error, and no partial snapshot.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-004-AC-1 | Given an unexpired cache entry, repeated fixture reads use the cached value and return identical results without another source read. | Test |
| FR-004-AC-2 | Given an expired entry and a changed fixture source, the next read refreshes and returns the changed value. | Test |
| FR-004-AC-3 | Given an expired entry and a failed fixture refresh, the read returns an error and does not return the stale value. | Test |
| FR-004-AC-4 | Given identical cache state and fixture source, repeated reads return identical results. | Test |
| FR-004-AC-5 | The CLI and library `ReadService::open_default` SHALL use the same platform cache location, and `IX_BOARD_CACHE_PATH` SHALL override that location. On Unix, an absolute `XDG_CACHE_HOME` SHALL select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3` even when `HOME` is unset; otherwise `HOME` SHALL select `<HOME>/.cache/ix-board/linear.sqlite3`. | Test |
| FR-004-AC-6 | Given a non-absolute or absent `XDG_CACHE_HOME`, ix-board SHALL fall back to the available home directory; if neither an absolute XDG cache directory nor a home directory is available, the library SHALL return `CacheDirectoryUnavailable` rather than inventing a path. | Test |
| FR-004-AC-7 | Project queries use a page size of 20 records. | Test |
| FR-004-AC-8 | Issue queries use a page size of 50 records. | Test |
| FR-004-AC-9 | A structured RATELIMITED response produces a typed LinearRateLimited error. | Test |
| FR-004-AC-10 | A rate-limit error carries the cooldown derived from exhausted quota resets. | Test |
| FR-004-AC-11 | Without a read policy, every existing read method keeps the 300-second entry lifetime, the 30-second per-request timeout, no deadline, and its existing typed errors; AC-1 through AC-10 pass unchanged and no existing method signature requires a new argument. | Test |
| FR-004-AC-12 | Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds or more is refused as an invalid policy with no source read. | Test |
| FR-004-AC-13 | Given a snapshot whose source read began 250 seconds ago but whose cache write or report occurred 10 seconds ago, a bounded read with maximum age 240 seconds refreshes it; an entry with no evidence of when its source read began is refreshed rather than accepted; the returned fetch time is the actual source fetch time. | Test |
| FR-004-AC-14 | Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with no more than D plus one legacy request timeout elapsed; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining. | Test |
| FR-004-AC-15 | Given a source whose credential resolution takes longer than the deadline before it becomes ready, a bounded read still performs its full budgeted read after readiness; the bounded read never invokes credential resolution, never retries it, and returns an existing credential error unchanged when the source reports one. | Test |
| FR-004-AC-16 | After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker. | Test |
| FR-004-AC-17 | A source that cannot honor a bound returns the explicit unsupported-bound error before any source read; its legacy read method still works unchanged. | Test |
| FR-004-AC-18 | A structured RATELIMITED response during a bounded read returns the existing typed rate-limit error with its cooldown, not a timeout, and the library does not retry or sleep; a deadline that expires first returns the timeout. | Test |
| FR-004-AC-19 | Given a cache that is locked for longer than the deadline, a bounded read returns the typed timeout within the deadline and does not read the source; a refreshed entry is written to the cache only before expiry. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
- **Consumers**: [FR-010](FR-010-explicit-work-scope.md),
  [FR-012](FR-012-focus-status-and-ready-queue.md) and
  [FR-016](FR-016-focus-record.md) use the bounded read for named-focus
  validation; the consumer chooses its own policy values.
