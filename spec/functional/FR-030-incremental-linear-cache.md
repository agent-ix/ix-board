---
id: FR-030
title: Merge shared incremental Linear datasets
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-010"
    type: satisfies
provenance:
  source: SWM-184
---
# FR-030: Merge shared incremental Linear datasets

## Description

The read service SHALL cache initiative/project summaries separately from detailed
ticket datasets, with explicit coverage and source-fetch provenance for each.
Summary reads SHALL NOT make a detail cache fresh. Commands SHALL request ticket
data only when their answer needs tickets. Retro workspace reads and update probes
SHALL use the shared refresh coordinator rather than forcing duplicate full scans.

When a compatible baseline exists, the coordinator SHALL request changed issues,
projects, and initiatives using `updatedAt >= last_successful_sync - 60 seconds`,
merge by stable ID, and preserve unchanged records. It SHALL include archived
records and remove observed archived/trashed records from active results, including
obsolete memberships and relationships. Replays SHALL be idempotent. Each dataset's
high-water mark SHALL be its successful refresh start time, committed atomically
with the complete merged dataset only after every required page succeeds. Related
data whose changes cannot be enumerated SHALL be refreshed or explicitly marked
unavailable; a partial delta SHALL NOT certify a complete fresh workspace.

The coordinator SHALL perform a full authoritative reconciliation for an absent or
incompatible baseline and when the last full reconciliation is at least 24 hours
old. A full reconciliation SHALL remove IDs missing from the accessible source
catalog. Since Linear list queries do not enumerate hard deletions, delta output
SHALL expose the last full reconciliation time and SHALL NOT claim immediate
hard-deletion completeness. A full detailed scan SHALL occur only when detailed
data is requested; a summary-only command SHALL reconcile only summary catalogs.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-030-AC-1 | Given a baseline, a fixture delta adds and updates records, removes observed archive/trash entries and obsolete memberships, preserves unchanged IDs, and produces the same result when replayed; all three entity queries use the last successful sync minus 60 seconds and include archived records. | Test |
| FR-030-AC-2 | A failed or rate-limited later delta page leaves the entire dataset, high-water mark, and successful fetch provenance unchanged; the next attempt uses the previous successful high-water mark. | Test |
| FR-030-AC-3 | With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source catalog is removed. An ordinary intervening refresh uses only deltas, and output exposes the last full reconciliation time. | Test |
| FR-030-AC-4 | Cold roadmap and scope-check fixtures request no issue pages or histories; requesting a ticket-dependent report loads detail, subsequent reports reuse it, and a summary refresh never advances detail freshness. | Test |
| FR-030-AC-5 | Two retro workers sharing a fixture cache reuse the centrally coordinated snapshot/delta without each probing and forcing a full workspace fetch; ticket/history requests remain covered by the shared quota meter. | Test |

## Dependencies

- [FR-004](FR-004-cache-polling.md) defines coordinated publication and freshness.
- [FR-020](FR-020-program-roadmap.md) defines native summary semantics.
- [FR-031](FR-031-shared-linear-quota.md) applies to all refresh requests.
