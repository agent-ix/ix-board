---
id: FR-028
title: Backfill a retro baseline from history
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-009"
    type: implements
---
# FR-028: Backfill a retro baseline from history

## Description

`ix-board retro backfill --config <file> --since <date> [--until <date>]
[--window <duration>] [--json]` (default window `24h`, as for snapshot) SHALL append one record of kind `backfill` per
UTC day in the range, each with `taken_at` at `23:59:59Z` of that day, using the
same sources as [FR-024](./FR-024-retro-snapshot.md). For each record, the command SHALL measure window
metrics over the half-open trailing window that ends at the next UTC midnight, so that
consecutive days are contiguous. `--until` SHALL default to the last complete UTC day, and
the command SHALL refuse a day that has not ended.

The command SHALL reconstruct only these metrics from history: `prs_merged_per_hour`,
`pr_cycle_median_hours`, `pr_cycle_p90_hours` (from pull-request creation and
merge timestamps), and `path_closed`, `intake_new`, `net_closed_minus_new` and
`done_reopened` (from Linear issue creation and state history). Every other
metric SHALL be `unavailable` with reason `not reconstructible`, including
`path_remaining`, because the blocker graph at a past time is not recorded. For the same
reason the Linear metrics use the current path and the current project membership of each
issue.

When a day already has a backfill record, the command SHALL skip that day and
report it. When any `snapshot` record has a `taken_at` from 00:00:00 of the first day up to,
but excluding, 00:00:00 after the last day, the command SHALL refuse the range, name that record and append nothing.
The command SHALL write under the rules of [FR-025](./FR-025-retro-store.md).

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-028-AC-1 | A three-day range appends three `backfill` records with `taken_at` at the end of each UTC day. | Test |
| FR-028-AC-2 | Running the same range twice appends nothing the second time and reports each skipped day. | Test |
| FR-028-AC-3 | Metrics needing live state (`path_remaining`, agent verdicts, in-progress staleness, open pull-request counts) are `unavailable` with reason `not reconstructible` in every backfill record. | Test |
| FR-028-AC-4 | A range containing a `snapshot` record's `taken_at` is refused with a message naming that record, and nothing is appended. | Test |
| FR-028-AC-5 | For a synthetic fixture of pull-request timestamps with a known count, median and 90th percentile inside a 24h trailing window, the backfilled record for that day equals those values exactly; the fixture holds no real project data. | Test |
| FR-028-AC-6 | The per-day record for a day with no merged pull requests has `prs_merged_per_hour` equal to 0 and both cycle metrics `unavailable` with reason `empty population`. | Test |
| FR-028-AC-7 | A backfill record's `query` strings include its window, and a delta between it and a snapshot taken with a different window reports `query_changed` ([FR-027](./FR-027-retro-delta.md)). | Test |

## Dependencies

- **Upstream**: [FR-024](./FR-024-retro-snapshot.md), [FR-025](./FR-025-retro-store.md), [FR-026](./FR-026-retro-unavailable-not-zero.md); the issue-history reader that drift uses ([FR-008](./FR-008-drift-signals.md)) and SWM-132's attributed history once it lands.
- **Downstream**: [FR-027](./FR-027-retro-delta.md).
