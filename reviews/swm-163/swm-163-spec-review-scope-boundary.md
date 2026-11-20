---
id: SR-6054
title: "SWM-163 scope-boundary review"
type: SpecReview
analysis: scope-boundary
scope: "agent-ix/ix-board; spec/functional/FR-029-department-retro-contract.md; SWM-163"
review_set: subset
---

## Summary

CHANGES REQUIRED: one medium-severity shared wire-model gap for later lifecycle evidence.

Ticket: SWM-163. Frozen candidate 9b46543eca0b508edc6a7513296a9bf859d13e25; base ba904671d92abf90082a74dffb1860afca9034fd. One existing ticket worktree. No source edits, builds, tests, gate, merge, alternate tree or credential-backed ix-board invocation. Exact model ID unavailable in harness; model=unavailable records this provenance limitation. The accepted canonical contract is context, not reopened scope. No AssuranceProfile exists in the repository spec tree; no enforced review policy applies. Dispatch explicitly authorized direct proportional subset review.

## Findings

| ID | Severity | Summary | Refs |
|---|---|---|---|
| FND-001 | medium | The shared measurement value contract permits numeric scalars and numeric keyed/distribution rows only, but the canonical SWM-164 catalog includes lane_execution_state, a lifecycle state with timestamps and uncertainty. The later-collector reuse requirement gives no way to preserve that categorical value without inventing numeric state codes or bypassing the shared model. Add an explicit typed categorical/source-observation reference variant that reuses FR-018 lifecycle output and preserves unknown/reason; specify its assessment/comparison eligibility and an offline round-trip fixture without reimplementing lifecycle rules. | spec/functional/FR-029-department-retro-contract.md:67-71 |

## Method Evidence

Allocation: retro owns config/evidence/assessment (core), store/history (infrastructure), availability (cross-cutting); dev-team owns canonical formulas/role interpretation; SWM-158 and FR-018 own lifecycle classification; SWM-164 supplies operational collectors; SWM-165 renders. External tracker/VCS/host/relay are assumed observations with mandatory completeness validation. Canonical fixture behavior is guaranteed by ACs. No auth repair, probe, scheduler, TUI or model arithmetic is authorized by this PR.

## Validation

Targeted FR-029: quire 0.52.0 (engine 0.62.2), exit 0, 1/1 grammar-clean and no findings; git diff --check exit 0. Full-tree quire validate --scope . spec/**/*.md --summary exits 3: unchanged FR-004 AC-5/6 grammar errors, FR-016/017 missing Dependencies and roadmap/ears-campaign-stories missing type. Existing FR-012/014/016/017 warnings also appear. These are unchanged from base, not PR findings. Actual resolved quoin executable /Users/peter/.nvm/versions/node/v24.15.0/bin/quoin version 0.36.7; quire is in the same directory. Targeted validation expected/passed; whole-tree pass unavailable due to those precise legacy defects. No changed artifact is embedded by existing tests. Spec-only aggregate gate SKIP. Untagged new ACs are future implementation obligations, not implementation findings.

## Reviewed Scope

```yaml
{
  "scope": [
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Where department monitoring is configured, the retro commands SHALL expose a\nversioned department envelope alongside the existing metric map, using the\ncanonical [evidence contract](ix://agent-ix/dev-team/FR-001),\n[deterministic metrics contract](ix://agent-ix/dev-team/FR-002) and\n[counting design](https://github.com/agent-ix/dev-team/blob/174dc0c31537d03c548401cae069111340a6714b/docs/department-monitoring.md).\nThose artifacts own formulas, time boundaries, unknown semantics and class\naggregation; this requirement defines their local configuration and wire boundary."
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "The existing retro JSON configuration gains an optional `department` object.\nIts `version` is `1`. It contains explicit team/project/repository/lane/host\nselectors, a `policy_id`, source freshness limits and clock tolerance, catalog\nmetric parameters, class required/optional metric IDs and owner/action routes.\nSelectors use source-native identities; display labels accompany identities\nrather than replacing them. Parameters include lane work capacity, completion\nobligations, state/lane mappings, dependency selection, diff-classifier version,\nloop lookback/repeat count and the intervals required"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Each metric assessment policy names the metric ID, unit, comparator (`gt`, `gte`,\n`lt` or `lte`), amber/red thresholds and comparison basis (`current`,\n`since_previous` or `since_baseline`). Freshness and measurement-window policies\nare explicit. Each class names its required and optional metrics and optional\nowner/action route. No lane capacity is interpreted as a host build-slot limit."
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "The class keys are `flow_wip`, `serial_dependency`, `pin_version_churn`,\n`capacity_resources`, `liveness`, `authority_policy`, `signal_integrity`,\n`review_rework`, `decision_latency` and `comms_overhead`, matching the ten design\nclasses in order. These keys are stable across collectors and consumers."
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "The shared Rust report model and JSON serialization SHALL expose these fields\nunder `department`, with `version: 1`:"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "| `catalog_version`, `policy_id`, `config_identity`, `scope` | Definition release, assessment policy and normalized measurement selectors/parameters; no credential values. |"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "| `observations` | Stable event ID, source-native entity ID, event/observed UTC time, scope, completeness and source reference; attributed classifications also carry actor/tool, classifier version and source offsets. |"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "| `measurements` | Stable metric ID and catalog version, `status`, nullable value, reason, unit/direction, scope, query/config identity, window start/end, source age, population/denominator and evidence references. |"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "| `loops` | Detector ID, availability/reason and findings with ordered evidence event IDs, time range, participants and deduplication key. |"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "| `classes` | All ten canonical class IDs, required/optional metric IDs, severity, independent completeness, missing metric IDs/reasons, applied policy/thresholds, source age, comparisons and route (`routed` with configured owner/action, or `unrouted`). |"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "| `rollup` | Canonical department severity and completeness with per-class missing-evidence diagnostics; no numeric combined health score. |"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Scalar measurements use a numeric value. A keyed measurement uses rows keyed by\nnative entity/state/host/lane identity; duration rows expose count, median and\nnearest-rank p90, and gauge/count rows expose their scalar value. Unavailable\nmeasurements have null value and a reason, rather than an empty successful row\nset. An available empty count population may be zero only after a complete read.\nReferences resolve within the envelope or to a source-native record; credentials\nand private transcript bodies are excluded."
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "- Where `department` is configured, `retro snapshot` SHALL append the department\n  envelope with its existing snapshot record in the store of\n  [FR-025](./FR-025-retro-store.md).\n- Where `department` is absent, retro SHALL retain the existing configuration,\n  metric/query and output behavior of [FR-024](./FR-024-retro-snapshot.md).\n- The local catalog SHALL register the canonical flow/WIP, dependency,\n  pin/version-churn and review/rework metrics and tag-repin/state-bounce loop\n  detectors with their canonical definitions, units and direction.\n- The collector SHALL preserve every existing cata"
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "The snapshot record containing this envelope uses `schema_version: 2`.\nThe reader SHALL accept existing version-1 records without rewriting or\nsynthesizing department evidence. The reader SHALL reject unsupported record or\nenvelope versions naming the version before appending. This is an explicit\nschema evolution under [FR-025](./FR-025-retro-store.md), not a migration layer."
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "`retro delta --json` SHALL expose department current/previous/baseline comparisons\nusing the record selection rules of [FR-027](./FR-027-retro-delta.md).\nFor new measurements, comparison identity comprises metric definition/catalog,\nnormalized scope, query parameters, window duration and applicable policy\nidentity; observation times advance normally and are not identity changes.\nWhen an identity differs, delta SHALL emit `query_changed` with both identities\nand no numeric trend. When a side lacks department evidence or is unavailable,\ndelta SHALL emit `unavailable` with a reason. Configuration"
    },
    {
      "id": "FR-029-AC-1",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "With department absent, existing snapshot/delta/backfill fixtures retain their metric IDs, values, queries and selection behavior. A department snapshot retains the legacy map and appends one version-2 line with the version-1 envelope; prior store bytes remain identical."
    },
    {
      "id": "FR-029-AC-2",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Two independent fixture teams with changed IDs/capacities serialize their configured scopes, observations, keyed measurements and evidence references; overlapping project membership counts native entities once. Replaying identical inputs/clock yields identical department payloads."
    },
    {
      "id": "FR-029-AC-3",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Missing policy, source timestamp, complete history, completion obligations, reviewer-ready time, lane capacity or dependency reachability produces the affected unavailable/unassessed reason while unrelated measurements remain available. A partial blocker graph or cycle never yields an arbitrarily finite successful chain length."
    },
    {
      "id": "FR-029-AC-4",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged."
    },
    {
      "id": "FR-029-AC-5",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Exact threshold boundary fixtures exercise all four comparators and three comparison bases. Reversed thresholds, wrong units, duplicate IDs, unknown IDs and unsupported department versions fail naming the field, with no source read and no appended line. Missing optional policy/owner produces unassessed/unrouted rather than a fabricated threshold/owner."
    },
    {
      "id": "FR-029-AC-6",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "All ten classes remain in the report with later collectors absent. Red+unknown yields red/incomplete, amber+unknown amber/incomplete, green+unknown unknown/incomplete, all green green/complete and an empty required set unknown/incomplete; optional metrics change neither severity nor completeness. Department rollup uses the same rules."
    },
    {
      "id": "FR-029-AC-7",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Changed scope, formula/catalog identity, parameter, window or applicable assessment policy produces query_changed without numeric trend; reordered equivalent config does not. Missing department evidence in a version-1 baseline yields unavailable for new measurements while compatible old metrics still compare. Delta invokes no source reader."
    },
    {
      "id": "FR-029-AC-8",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set."
    },
    {
      "id": "FR-029-AC-9",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Empty duration populations are unavailable, singleton count/median/p90 agree, zero denominators are unavailable, and partial pages, stale sources and excessive future skew invalidate dependent measurements without turning a class green."
    },
    {
      "id": "FR-029-AC-10",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Version-1 history remains byte-identical when read; unsupported record/envelope versions fail naming the version and leave the store unchanged. Shared JSON round trips preserve keyed values, evidence, comparison states, class completeness and owner/action routing without requiring a live source."
    },
    {
      "id": "FR-029",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "- [US-009](../usecase/US-009-review-work-convergence.md), existing retro intent.\n- [FR-024](./FR-024-retro-snapshot.md) through\n  [FR-028](./FR-028-retro-backfill.md), existing catalog/store/history behavior.\n- [FR-018](./FR-018-agent-activity-verdicts.md), lifecycle evidence remains owned\n  by its existing collector; this requirement does not redefine execution state.\n- [FR-001](ix://agent-ix/dev-team/FR-001) and\n  [FR-002](ix://agent-ix/dev-team/FR-002) are cross-repository contracts, not the\n  unrelated local ready-queue and project-chunk requirements with the same IDs.\n- SWM-163 implements"
    },
    {
      "id": "FR-024",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "context_only",
      "excerpt": "`ix-board retro snapshot --config <file> [--window <duration>] [--json]` SHALL\nread the structured Linear, GitHub and herdr sources named in the retro\nconfiguration, compute the metric catalog below for the configured team, and\nappend one record to the store defined by [FR-025](./FR-025-retro-store.md).\n`--window` takes seconds, minutes, hours or days (`90s`, `20m`, `4h`, `2d`) and\ndefaults to `24h`. The command SHALL measure window metrics over the trailing window\nending at the snapshot time, and state metrics at the snapshot time. The command SHALL NOT mutate Linear, GitHub or any repository"
    },
    {
      "id": "FR-025",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "context_only",
      "excerpt": "The retro commands SHALL keep their records in one append-only JSONL file per\nteam at `$TEAM_RETRO_HOME/<team>/snapshots.jsonl`, defaulting `TEAM_RETRO_HOME`\nto `~/.local/state/team-retro`. Each line SHALL be one record carrying\n`schema_version`, `kind` (`snapshot` or `backfill`), `taken_at` (UTC), `team`,\n`window`, and the metric map of [FR-024](./FR-024-retro-snapshot.md)."
    },
    {
      "id": "FR-026",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "context_only",
      "excerpt": "The retro commands SHALL write each metric in a record with `status` (`ok` or `unavailable`), `value`\n(a number, or null when unavailable), `unit`, `direction`, `source` and `query`."
    },
    {
      "id": "FR-027",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "context_only",
      "excerpt": "`ix-board retro delta --config <file> [--json]` SHALL read the store of the\nconfigured team and, for each metric of the latest record, report the value, the\nchange since the previous record, and the change since the baseline. The latest\nand previous records SHALL be the two records of kind `snapshot` with the\ngreatest `taken_at`. The baseline SHALL be the record named by\n`baseline_taken_at` in the configuration, or the earliest record of either kind\nwhen that field is absent. Backfilled records serve only as baseline and\nhistory. The command SHALL read no Linear or GitHub source."
    },
    {
      "id": "FR-028",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "context_only",
      "excerpt": "`ix-board retro backfill --config <file> --since <date> [--until <date>]\n[--window <duration>] [--json]` (default window `24h`, as for snapshot) SHALL append one record of kind `backfill` per\nUTC day in the range, each with `taken_at` at `23:59:59Z` of that day, using the\nsame sources as [FR-024](./FR-024-retro-snapshot.md). For each record, the command SHALL measure window\nmetrics over the half-open trailing window that ends at the next UTC midnight, so that\nconsecutive days are contiguous. `--until` SHALL default to the last complete UTC day, and\nthe command SHALL refuse a day that has not e"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "context_only",
      "excerpt": "The `ix-board agents <team>... [--idle-after <duration>]` command SHALL report\ncurrent execution, unfinished worker assignments, recent ended activity and\nuncertainty separately so a team leader can decide whether intervention is needed.\nTranscript modification time is an age observation, never proof of a live worker."
    },
    {
      "id": "FR-001",
      "path": "agent-ix/dev-team/spec/functional/FR-001-evidence-contract.md",
      "role": "context_only",
      "excerpt": "The monitoring collector SHALL emit scoped evidence records using the common\nenvelope in [the design](../../docs/department-monitoring.md#evidence-and-comparison-contract)."
    },
    {
      "id": "FR-002",
      "path": "agent-ix/dev-team/spec/functional/FR-002-deterministic-metrics.md",
      "role": "context_only",
      "excerpt": "The ix-board retro collector SHALL compute each measure in the\n[metric catalog](../../docs/department-monitoring.md#metric-catalog) from normalized evidence."
    }
  ]
}
```

## Verdict

CHANGES REQUIRED: one medium-severity shared wire-model gap for later lifecycle evidence.
