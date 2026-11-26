---
id: FR-027
title: Compare retro snapshots with the previous snapshot and the baseline
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-009"
    type: implements
---
# FR-027: Compare retro snapshots with the previous snapshot and the baseline

## Description

`ix-board retro delta --config <file> [--json]` or `ix-board retro delta --team <team> [--json]` SHALL read the store of the
configured team and, for each metric of the latest record, report the value, the
change since the previous record, and the change since the baseline. The latest
and previous records SHALL be the two records of kind `snapshot` with the
greatest `taken_at`. The baseline SHALL be the record named by
`baseline_taken_at` in the configuration, or the earliest record of either kind
when that field is absent. Backfilled records serve only as baseline and
history. The command SHALL read no Linear or GitHub source.

`baseline_taken_at` SHALL name the record whose `taken_at` is the same instant,
fractional seconds included. When it matches no record, the command SHALL stop with
an error naming the value, before it checks the number of snapshot records. When
fewer than two snapshot records exist, the command SHALL
report that no delta exists and exit zero.

The `ix-board retro delta` command SHALL include each metric's family from the
FR-024 catalog as `class` in the delta output, and SHALL include the FR-029
department catalog class as `department_class` when that metric has a department
definition, resolving these output fields from the current catalogs. The
`ix-board::retro::decode_saved_report` reader SHALL accept earlier delta JSON
that omits `class` and `department_class`.

For a metric with direction `up` or `down`, the command SHALL set `regressing`
when the change since the previous record moved against the direction in each of
the latest two consecutive comparisons between snapshot records, which needs three
snapshots. The command SHALL report a change only
when both sides are `ok` and their `query` for that metric are equal. When either
side is `unavailable`, the command SHALL report `unavailable`; when the queries
differ, it SHALL report `query_changed` and show both queries. A metric with
direction `neutral` SHALL show changes and never `regressing`. The command SHALL
print no single combined health score.

The delta report SHALL also expose a `defects` array. A latest two-record increase
in `prs_idle`, `fix_rounds_max` or `agents_idle_minutes_max` emits one
`retro_metric_rising` defect with the metric ID and both observed values. A
`lead_wait_without_named_wait` defect is emitted when the latest
`unnamed_wait_minutes_max` value is strictly greater than 20 and rises from the
previous value or crosses 20 for the first time; with one snapshot, a value over
20 is a first crossing. Defect generation reads stored records and records each
finding in the fleet ledger. `--notify` sends findings to the cadence manifest's
single `alert_recipient` (`server:workspace:tab`).

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-027-AC-1 | For a store of three snapshots, each metric shows its value, the change from the previous snapshot and the change from the baseline, with the baseline defaulting to the earliest record. | Test |
| FR-027-AC-2 | A `down`-direction metric that rose in each of the last two comparisons is `regressing`; one that rose once and fell once is not. | Test |
| FR-027-AC-3 | A metric unavailable on either side reports `unavailable`, and a metric whose `query` differs reports `query_changed` with both queries; neither reports a numeric change. | Test |
| FR-027-AC-4 | A `neutral` metric is never `regressing`. | Test |
| FR-027-AC-5 | With one snapshot the command reports that no delta exists and exits zero; output contains no combined score. | Test |
| FR-027-AC-6 | The command makes no network request: it is run with source readers that fail the test if invoked, and still succeeds from the store alone. | Test |
| FR-027-AC-7 | A backfill run after a snapshot leaves the snapshot as the latest record, and a backfilled record can serve as the baseline. | Test |
| FR-027-AC-8 | A `baseline_taken_at` matching no record stops the command with an error naming the value. | Test |
| FR-027-AC-9 | `ix-board retro delta` output includes each metric's FR-024 catalog family as `class` and the FR-029 department class as `department_class` where defined, resolving both from catalogs. | Test |
| FR-027-AC-10 | `ix-board::retro::decode_saved_report` decodes delta JSON written before the `class` and `department_class` output fields existed. | Test |
| FR-027-AC-11 | Hand-written records that show each watched metric rising produce one structured defect with the metric's stored prior value and current value; flat, falling, unavailable, or query-changed metrics produce none. Unnamed wait emits only above 20 and only on a rise or first crossing, including the first snapshot. | Test |
| FR-027-AC-12 | Without `--notify`, delta makes no notification call; with `--notify`, each defect is sent once to the cadence manifest's `alert_recipient`, and missing or invalid routing configuration returns an error without sending. | Test |
| FR-027-AC-13 | Each emitted defect is appended as an individual fleet ledger defect row with team, class, metric id, previous and current values. | Test |
| FR-027-AC-14 | `retro delta --team T` reads team T's store without a retro config file; `--notify` reads its recipient from the selected cadence manifest. | Test |

## Dependencies

- **Upstream**: [FR-024](./FR-024-retro-snapshot.md), [FR-025](./FR-025-retro-store.md), [FR-026](./FR-026-retro-unavailable-not-zero.md).
- **Downstream**: [FR-028](./FR-028-retro-backfill.md) supplies backfilled baselines.
