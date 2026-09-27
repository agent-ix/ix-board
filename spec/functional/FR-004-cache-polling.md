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

The ix-board read library SHALL use SQLite to cache structured Linear read data
and SHALL poll Linear at five-minute intervals for changes. After a successful
poll observes a changed source record, the library SHALL invalidate or replace
the corresponding cached value before a subsequent command returns that value.
The cache SHALL NOT cause a command to return a value older than five minutes
when polling succeeds.

## Inputs

- Structured Linear records obtained through the read client.
- Cached records and their refresh metadata.
- The configured five-minute polling interval.

## Outputs

- Cached records for reads that do not require a refresh.
- Refreshed records after a successful poll observes source changes.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-004-AC-1 | Given an unchanged fixture read within the five-minute interval, a repeated read returns the cached value without changing the observable result. | Test (IT-001) |
| FR-004-AC-2 | Given a changed source fixture at the next successful poll, the prior cached value is invalidated and the subsequent command returns the changed value. | Test (IT-001) |
| FR-004-AC-3 | Given a successful poll and a cache entry older than five minutes, a command does not return that stale entry. | Test (IT-001) |
| FR-004-AC-4 | Given identical cache state and source fixture, repeated reads return identical results. | Test (IT-001) |

## Dependencies

- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)
- **Downstream**: [IT-001](../integration/IT-001-linear-read-fixtures.md)

## Open Questions

Decide whether five minutes is a periodic process interval or a maximum age per
cache entry, whether polling is per team/project or global, how concurrent
processes coordinate refresh, and what commands return when a poll fails. These
choices affect freshness and cache invalidation semantics.
