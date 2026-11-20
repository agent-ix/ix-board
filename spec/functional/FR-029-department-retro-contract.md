---
id: FR-029
title: Expose department evidence and assessment through retro
type: FR
relationships:
  - target: ix://agent-ix/ix-board/US-009
    type: implements
  - target: ix://agent-ix/dev-team/FR-001
    type: depends_on
  - target: ix://agent-ix/dev-team/FR-002
    type: depends_on
  - target: ix://agent-ix/ix-board/FR-024
    type: depends_on
  - target: ix://agent-ix/ix-board/FR-027
    type: depends_on
---
# FR-029: Expose department evidence and assessment through retro

## Description

Where department monitoring is configured, the retro commands SHALL expose a
versioned department envelope alongside the existing metric map, using the
canonical [evidence contract](ix://agent-ix/dev-team/FR-001),
[deterministic metrics contract](ix://agent-ix/dev-team/FR-002) and
[counting design](https://github.com/agent-ix/dev-team/blob/174dc0c31537d03c548401cae069111340a6714b/docs/department-monitoring.md).
Those artifacts own formulas, time boundaries, unknown semantics and class
aggregation; this requirement defines their local configuration and wire boundary.

## Inputs

The existing retro JSON configuration gains an optional `department` object.
Its `version` is `1`. It contains explicit team/project/repository/lane/host
selectors, a `policy_id`, source freshness limits and clock tolerance, catalog
metric parameters, class required/optional metric IDs and owner/action routes.
Selectors use source-native identities; display labels accompany identities
rather than replacing them. Parameters include lane work capacity, completion
obligations, state/lane mappings, dependency selection, diff-classifier version,
loop lookback/repeat count and the intervals required by selected metrics.
An absent parameter is recorded as missing evidence/configuration for dependent
measurements, rather than assigned a numeric default.

Each metric assessment policy names the metric ID, unit, comparator (`gt`, `gte`,
`lt` or `lte`), amber/red thresholds and comparison basis (`current`,
`since_previous` or `since_baseline`). Freshness and measurement-window policies
are explicit. Each class names its required and optional metrics and optional
owner/action route. No lane capacity is interpreted as a host build-slot limit.

The class keys are `flow_wip`, `serial_dependency`, `pin_version_churn`,
`capacity_resources`, `liveness`, `authority_policy`, `signal_integrity`,
`review_rework`, `decision_latency` and `comms_overhead`, matching the ten design
classes in order. These keys are stable across collectors and consumers.

## Outputs

The shared Rust report model and JSON serialization SHALL expose these fields
under `department`, with `version: 1`:

| Field | Contract |
|---|---|
| `catalog_version`, `policy_id`, `config_identity`, `scope` | Definition release, assessment policy and normalized measurement selectors/parameters; no credential values. |
| `observations` | Stable event ID, source-native entity ID, event/observed UTC time, scope, completeness and source reference; attributed classifications also carry actor/tool, classifier version and source offsets. |
| `measurements` | Stable metric ID and catalog version, `status`, nullable value, reason, unit/direction, scope, query/config identity, window start/end, source age, population/denominator and evidence references. |
| `loops` | Detector ID, availability/reason and findings with ordered evidence event IDs, time range, participants and deduplication key. |
| `classes` | All ten canonical class IDs, required/optional metric IDs, severity, independent completeness, missing metric IDs/reasons, applied policy/thresholds, source age, comparisons and route (`routed` with configured owner/action, or `unrouted`). |
| `rollup` | Canonical department severity and completeness with per-class missing-evidence diagnostics; no numeric combined health score. |

Scalar measurements use a numeric value. A keyed measurement uses rows keyed by
native entity/state/host/lane identity; duration rows expose count, median and
nearest-rank p90, and gauge/count rows expose their scalar value. Unavailable
measurements have null value and a reason, rather than an empty successful row
set. An available empty count population may be zero only after a complete read.
References resolve within the envelope or to a source-native record; credentials
and private transcript bodies are excluded.

## Behavior

- Where `department` is configured, `retro snapshot` SHALL append the department
  envelope with its existing snapshot record in the store of
  [FR-025](./FR-025-retro-store.md).
- Where `department` is absent, retro SHALL retain the existing configuration,
  metric/query and output behavior of [FR-024](./FR-024-retro-snapshot.md).
- The local catalog SHALL register the canonical flow/WIP, dependency,
  pin/version-churn and review/rework metrics and tag-repin/state-bounce loop
  detectors with their canonical definitions, units and direction.
- The collector SHALL preserve every existing catalog ID and formula, including
  `in_progress_stale`, reviewer rounds, full gates and red-main metrics.
- If a canonical class depends on a collector not implemented or configured,
  then the report SHALL retain that class and mark its dependent measurements
  unavailable with the missing collector/configuration reason.
- The assessment engine SHALL evaluate required/optional measurements and class
  rollup using the canonical precedence and independent completeness rules.
- If a measurement has no policy or required comparison, then the assessment engine SHALL emit `unassessed` with a reason and contribute unknown when required.
- If configuration has an unsupported version, duplicate class/metric policy ID, unknown catalog ID, invalid unit/comparator or threshold ordering, then retro SHALL exit nonzero naming the field before source reads or store writes.
- Where a comparator is `gt` or `gte`, red thresholds SHALL be at least amber;
  where it is `lt` or `lte`, red thresholds SHALL be at most amber.
- The collector SHALL use the canonical evidence and ordering rules for new
  measurements without applying their time boundaries to legacy metric IDs.

The snapshot record containing this envelope uses `schema_version: 2`.
The reader SHALL accept existing version-1 records without rewriting or
synthesizing department evidence. The reader SHALL reject unsupported record or
envelope versions naming the version before appending. This is an explicit
schema evolution under [FR-025](./FR-025-retro-store.md), not a migration layer.

`retro delta --json` SHALL expose department current/previous/baseline comparisons
using the record selection rules of [FR-027](./FR-027-retro-delta.md).
For new measurements, comparison identity comprises metric definition/catalog,
normalized scope, query parameters, window duration and applicable policy
identity; observation times advance normally and are not identity changes.
When an identity differs, delta SHALL emit `query_changed` with both identities
and no numeric trend. When a side lacks department evidence or is unavailable,
delta SHALL emit `unavailable` with a reason. Configuration serialization order
does not change normalized identity. Existing metric comparisons retain their
existing query semantics. Delta SHALL read only persisted evidence and perform
no network collection. Text rendering SHALL name unknown classes and missing
routes as well as known breaches. Later collectors and TUI consumers SHALL use
this model and assessment engine rather than compute parallel metrics.

## Acceptance Criteria

| ID | Criteria | Verification |
|---|---|---|
| FR-029-AC-1 | With department absent, existing snapshot/delta/backfill fixtures retain their metric IDs, values, queries and selection behavior. A department snapshot retains the legacy map and appends one version-2 line with the version-1 envelope; prior store bytes remain identical. | Test |
| FR-029-AC-2 | Two independent fixture teams with changed IDs/capacities serialize their configured scopes, observations, keyed measurements and evidence references; overlapping project membership counts native entities once. Replaying identical inputs/clock yields identical department payloads. | Test |
| FR-029-AC-3 | Missing policy, source timestamp, complete history, completion obligations, reviewer-ready time, lane capacity or dependency reachability produces the affected unavailable/unassessed reason while unrelated measurements remain available. A partial blocker graph or cycle never yields an arbitrarily finite successful chain length. | Test |
| FR-029-AC-4 | The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged. | Test |
| FR-029-AC-5 | Exact threshold boundary fixtures exercise all four comparators and three comparison bases. Reversed thresholds, wrong units, duplicate IDs, unknown IDs and unsupported department versions fail naming the field, with no source read and no appended line. Missing optional policy/owner produces unassessed/unrouted rather than a fabricated threshold/owner. | Test |
| FR-029-AC-6 | All ten classes remain in the report with later collectors absent. Red+unknown yields red/incomplete, amber+unknown amber/incomplete, green+unknown unknown/incomplete, all green green/complete and an empty required set unknown/incomplete; optional metrics change neither severity nor completeness. Department rollup uses the same rules. | Test |
| FR-029-AC-7 | Changed scope, formula/catalog identity, parameter, window or applicable assessment policy produces query_changed without numeric trend; reordered equivalent config does not. Missing department evidence in a version-1 baseline yields unavailable for new measurements while compatible old metrics still compare. Delta invokes no source reader. | Test |
| FR-029-AC-8 | Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set. | Test |
| FR-029-AC-9 | Empty duration populations are unavailable, singleton count/median/p90 agree, zero denominators are unavailable, and partial pages, stale sources and excessive future skew invalidate dependent measurements without turning a class green. | Test |
| FR-029-AC-10 | Version-1 history remains byte-identical when read; unsupported record/envelope versions fail naming the version and leave the store unchanged. Shared JSON round trips preserve keyed values, evidence, comparison states, class completeness and owner/action routing without requiring a live source. | Test |

## Dependencies

- [US-009](../usecase/US-009-review-work-convergence.md), existing retro intent.
- [FR-024](./FR-024-retro-snapshot.md) through
  [FR-028](./FR-028-retro-backfill.md), existing catalog/store/history behavior.
- [FR-018](./FR-018-agent-activity-verdicts.md), lifecycle evidence remains owned
  by its existing collector; this requirement does not redefine execution state.
- [FR-001](ix://agent-ix/dev-team/FR-001) and
  [FR-002](ix://agent-ix/dev-team/FR-002) are cross-repository contracts, not the
  unrelated local ready-queue and project-chunk requirements with the same IDs.
- SWM-163 implements this interface and its flow/dependency/churn/rework slice;
  SWM-164 adds operational collectors to it; SWM-165 renders it.
