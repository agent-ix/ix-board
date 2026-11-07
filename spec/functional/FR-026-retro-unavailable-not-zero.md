---
id: FR-026
title: Report unreadable metrics as unavailable with their query
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-009"
    type: implements
---
# FR-026: Report unreadable metrics as unavailable with their query

## Description

The retro commands SHALL write each metric in a record with `status` (`ok` or `unavailable`), `value`
(a number, or null when unavailable), `unit`, `direction`, `source` and `query`.

If a source cannot be read, is incomplete, or is not named in the
configuration, then the command SHALL set `status` to `unavailable`, set `value` to
null, and record a `reason` naming the failed or missing source.

The command SHALL NOT report zero or a partial count for an unavailable metric.

If a paginated
read ends before its last page, then the command SHALL mark every metric that
depends on it `unavailable`.

The retro commands SHALL write in `query` the counting rule used together with the window and the
thresholds applied, so that two records computed by different rules or windows
are distinguishable. One unavailable metric SHALL NOT stop the other metrics
from being computed or the record from being appended. When a record is
appended, the command SHALL exit zero and list the unavailable ids on stderr.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-026-AC-1 | With the GitHub source failing, every flow metric is `unavailable` with a reason naming GitHub, `value` is null rather than 0, and the progress metrics are still computed. | Test |
| FR-026-AC-2 | A pull-request listing that stops before its last page makes the dependent metrics `unavailable`, not a smaller count. | Test |
| FR-026-AC-3 | A metric whose source the configuration does not name is `unavailable` with reason `not configured`. | Test |
| FR-026-AC-4 | Two runs that differ only in `--window`, or only in a threshold, produce different `query` strings for the affected metrics. | Test |
| FR-026-AC-5 | The command exits zero and appends the record when some metrics are unavailable, and prints the unavailable ids on stderr. | Test |
| FR-026-AC-6 | A median over zero merged pull requests is `unavailable` with reason `empty population`, while a merge rate over zero merges is the value 0. | Test |

## Dependencies

- **Upstream**: [FR-024](./FR-024-retro-snapshot.md), [FR-025](./FR-025-retro-store.md); the incomplete-read rule used by drift in [FR-008](./FR-008-drift-signals.md).
- **Downstream**: [FR-027](./FR-027-retro-delta.md), [FR-028](./FR-028-retro-backfill.md).
