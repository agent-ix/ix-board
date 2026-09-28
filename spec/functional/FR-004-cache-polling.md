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

## Inputs

- Structured Linear records obtained through the read client.
- Cached records and their refresh metadata.
- The five-minute cache-entry lifetime.

## Outputs

- Cached records for reads that do not require a refresh.
- Refreshed records after a successful poll observes source changes.
- An error when an expired entry cannot be refreshed.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-004-AC-1 | Given an unexpired cache entry, repeated fixture reads use the cached value and return identical results without another source read. | Test |
| FR-004-AC-2 | Given an expired entry and a changed fixture source, the next read refreshes and returns the changed value. | Test |
| FR-004-AC-3 | Given an expired entry and a failed fixture refresh, the read returns an error and does not return the stale value. | Test |
| FR-004-AC-4 | Given identical cache state and fixture source, repeated reads return identical results. | Test |
| FR-004-AC-5 | The CLI and library `ReadService::open_default` SHALL use the same platform cache location, and `IX_BOARD_CACHE_PATH` SHALL override that location. On Unix, an absolute `XDG_CACHE_HOME` SHALL select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3` even when `HOME` is unset; otherwise `HOME` SHALL select `<HOME>/.cache/ix-board/linear.sqlite3`. | Test |
| FR-004-AC-6 | Given a non-absolute or absent `XDG_CACHE_HOME`, ix-board SHALL fall back to the available home directory; if neither an absolute XDG cache directory nor a home directory is available, the library SHALL return `CacheDirectoryUnavailable` rather than inventing a path. | Test |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
