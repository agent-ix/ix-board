---
id: SR-6057
title: "SWM-163 gap-analysis of department implementation"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; SWM-163; src/linear.rs, src/retro/, tests/retro.rs; FR-029 and FR-024..028 compatibility"
review_set: subset
relationships:
  - {target: ix://agent-ix/ix-board/FR-029, type: references}
---

## Summary

Ticket: SWM-163. Independent review of the frozen implementation; exact model ID unavailable. Findings require the original coder's fix round. Artifact custody remains external until the lead/coder retains this exact file in reviews/.

## Verdict

**FAIL** — new behavioral or completion gaps remain. Repository-wide assurance remains INCOMPLETE for the recorded baseline limits.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Required SWM-163 measurements have calculators but no production source path that can supply their facts. workspace_evidence always sets claim/movement/blocked/merged/obligations evidence absent; LiveGithub always sets reviewer-ready/review counts/diff attribution/full gates absent and emits no pin-event or check facts. No native-to-normalized injectable adapter for these histories/classifications exists beyond tests constructing Fact values. Thus claims_without_movement, merged_open, oldest_gated_hours, idle_with_ready, reviewer age, repin_only_prs, tag_repin_events/tag_repin_loop, focused/workspace checks and gates_per_pr remain permanently unavailable even when their real source data is available. Honest unavailable output protects results but does not fulfill this ticket's collection obligations. Add real read-only normalization paths with injectable HTTP/log boundaries and fixture tests; keep genuinely missing or denied history unavailable. | src/retro/live.rs:118-165,441-506 |

## Coverage

Reconciliation: quoin matrix --repo . --json exited 0 using quoin 0.36.7 and quire 0.52.0 (engine 0.62.2). Full repository: 229 criteria, 41 tagged, 180 untagged, 8 method-without-symbol; all 12 FR-029 ACs tagged. No bound run evidence exists. quire matrix --scope . --strict exited 1: 180 untagged criteria and 30 design gaps. These older source-verification gaps are retained as baseline limitations, not represented as newly introduced defects or silently repaired. Full repository assurance result: INCOMPLETE/FAIL; no full audit PASS is asserted.

Reverse inventory: 41 Rust files across src/tests; inventory public-inventory.txt covers the full source tree. Existing public behavior groups map to FR-001..028 (ready queue, project/search/cache, handoff, drift/focus, credential boundary, agent lifecycle, program/reconcile/chain/orphans, retro). New department configuration, normalized evidence, metric catalog, policy, loops, persistence and comparisons map to FR-029 and its canonical external contracts. No new unowned behavior identified. Full detailed legacy reverse/body audit is INCOMPLETE; changed 17-file surface examined in detail. Source/test stub scan is recorded in stub-scan.txt: intentional unavailable later collectors are explicit; required native collector incompleteness is FND-001. Existing ignored subprocess helper and NFR benchmark retain reasons. No new ignored tests, lowered threshold, vendored canonical spec or assurance scaffolding detected. No line coverage was collected; coverage inflation cannot be claimed absent merely from green tests.

Plan completion: not assessed

Optional gap semantic fan-out: skipped (no explicit opt-in); required code-review spec/code/test checks were performed separately.

## Examined scope and bindings

All FR-029 AC-1..12 examined; FR-024..028 and external dev-team FR-001/FR-002 read as context only. Each correct binding means the real test exercises part of the AC, not that every obligation is satisfied; FNDs identify missing edges.

```json
{
  "bindings": [
    {
      "ac_id": "FR-029-AC-1",
      "test_id": "department_snapshot_appends_v2_preserves_v1_bytes_and_delta_reads_no_collectors",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-1",
      "test_id": "department_backfill_keeps_current_gauges_unavailable_without_invented_history",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-2",
      "test_id": "native_identity_deduplicates_populations_and_replays_two_independent_teams",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-3",
      "test_id": "fresh_restarted_and_moved_claims_are_excluded_and_unknown_history_is_isolated",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-3",
      "test_id": "dependency_depth_fanin_cancelled_partial_and_cyclic_graphs_are_exact",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-3",
      "test_id": "department_backfill_keeps_current_gauges_unavailable_without_invented_history",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "fresh_restarted_and_moved_claims_are_excluded_and_unknown_history_is_isolated",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "dependency_depth_fanin_cancelled_partial_and_cyclic_graphs_are_exact",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "state_exit_boundaries_and_singleton_seven_hour_distributions_match_oracles",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "completion_review_ready_diff_attribution_and_round_policy_use_explicit_evidence",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "injected_http_lock_and_reachability_boundary_keeps_unrelated_revision_counts_available",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "revision_divergence_checks_and_gate_scope_use_native_attributed_populations",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "independent_same_time_entities_need_no_cross_entity_order_and_same_state_updates_do_not_exit",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-5",
      "test_id": "four_comparators_and_three_bases_respect_exact_threshold_boundaries",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-5",
      "test_id": "malformed_versions_units_statistics_keys_and_duplicates_name_policy_field",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-5",
      "test_id": "invalid_department_config_or_store_versions_refuse_before_reads_and_appends",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-6",
      "test_id": "class_and_department_severity_preserve_breaches_and_independent_completeness",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-7",
      "test_id": "normalized_configuration_identity_ignores_order_but_detects_semantic_changes",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-7",
      "test_id": "department_snapshot_appends_v2_preserves_v1_bytes_and_delta_reads_no_collectors",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-8",
      "test_id": "tag_repin_and_state_bounce_deduplicate_ordered_native_events_and_refuse_unknown_order",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-8",
      "test_id": "independent_same_time_entities_need_no_cross_entity_order_and_same_state_updates_do_not_exit",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-9",
      "test_id": "state_exit_boundaries_and_singleton_seven_hour_distributions_match_oracles",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-9",
      "test_id": "partial_pages_missing_timestamps_staleness_skew_and_zero_capacity_never_become_green",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-9",
      "test_id": "revision_divergence_checks_and_gate_scope_use_native_attributed_populations",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-10",
      "test_id": "categorical_roundtrip_is_unassessed_and_compares_equality_without_numeric_trends",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-10",
      "test_id": "department_snapshot_appends_v2_preserves_v1_bytes_and_delta_reads_no_collectors",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-10",
      "test_id": "invalid_department_config_or_store_versions_refuse_before_reads_and_appends",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-11",
      "test_id": "malformed_versions_units_statistics_keys_and_duplicates_name_policy_field",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-11",
      "test_id": "keyed_policy_selects_count_median_or_p90_per_expected_key_without_averaging",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-12",
      "test_id": "categorical_roundtrip_is_unassessed_and_compares_equality_without_numeric_trends",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-12",
      "test_id": "categorical_lifecycle_keeps_partial_source_counts_and_provenance_without_transcript_text",
      "trace": "correct"
    }
  ],
  "scope": [
    {
      "excerpt": "With department absent, existing snapshot/delta/backfill fixtures retain their metric IDs, values, queries and selection behavior. A department snapshot retains the legacy map and appends one version-2 line with the version-1 envelope; prior store bytes remain identical.",
      "id": "FR-029-AC-1",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Two independent fixture teams with changed IDs/capacities serialize their configured scopes, observations, keyed measurements and evidence references; overlapping project membership counts native entities once. Replaying identical inputs/clock yields identical department payloads.",
      "id": "FR-029-AC-2",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Missing policy, source timestamp, complete history, completion obligations, reviewer-ready time, lane capacity or dependency reachability produces the affected unavailable/unassessed reason while unrelated measurements remain available. A partial blocker graph or cycle never yields an arbitrarily finite successful chain length.",
      "id": "FR-029-AC-3",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged.",
      "id": "FR-029-AC-4",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Exact threshold boundary fixtures exercise all four comparators and three comparison bases. Reversed thresholds, wrong units, duplicate IDs, unknown IDs and unsupported department versions fail naming the field, with no source read and no appended line. Missing optional policy/owner produces unassessed/unrouted rather than a fabricated threshold/owner.",
      "id": "FR-029-AC-5",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "All ten classes remain in the report with later collectors absent. Red+unknown yields red/incomplete, amber+unknown amber/incomplete, green+unknown unknown/incomplete, all green green/complete and an empty required set unknown/incomplete; optional metrics change neither severity nor completeness. Department rollup uses the same rules.",
      "id": "FR-029-AC-6",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Changed scope, formula/catalog identity, parameter, window or applicable assessment policy produces query_changed without numeric trend; reordered equivalent config does not. Missing department evidence in a version-1 baseline yields unavailable for new measurements while compatible old metrics still compare. Delta invokes no source reader.",
      "id": "FR-029-AC-7",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set.",
      "id": "FR-029-AC-8",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Empty duration populations are unavailable. For the singleton [7 hours], count=1, median=7 hours and p90=7 hours. Zero denominators are unavailable, and partial pages, stale sources and excessive future skew invalidate dependent measurements without turning a class green.",
      "id": "FR-029-AC-9",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Version-1 history remains byte-identical when read; unsupported record/envelope versions fail naming the version and leave the store unchanged. Shared JSON round trips preserve keyed values, evidence, comparison states, class completeness and owner/action routing without requiring a live source.",
      "id": "FR-029-AC-10",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "For expected state keys A/B/C, fixture A has median=1h/p90=10h, B median=1h/p90=2h, and C is missing. With gte amber=3h/red=5h and p90 selected, key severities are red/green/unknown and the metric is red/incomplete with C's missing-key reason. Selecting median yields green/green/unknown and unknown/incomplete; adding C median=1h/p90=1h yields green/complete for the median policy. Unselected key D with p90=100h changes neither result. Missing statistic/key reduction, empty expected key set, invalid component/unit or numeric policy on categorical evidence fails configuration validation. Changing",
      "id": "FR-029-AC-11",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "An offline lane_execution_state round trip preserves an FR-018 WORKING row and an UNKNOWN/partial row including null timestamp/count reasons, lifecycle provenance and source availability. No numeric state code or invented timestamp appears. Categories remain unassessed with categorical-evidence reason; a required categorical input makes its class unknown/incomplete. Compatible WORKING-to-WORKING and WORKING-to-UNKNOWN observations produce same and changed equality respectively without numeric trend; unavailable evidence produces unavailable and a changed source schema produces query_changed.",
      "id": "FR-029-AC-12",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "`ix-board retro snapshot --config <file> [--window <duration>] [--json]` SHALL\nread the structured Linear, GitHub and herdr sources named in the retro\nconfiguration, compute the metric catalog below for the configured team, and\nappend one record to the store defined by [FR-025](./FR-025-retro-store.md).\n`--window` takes seconds, minutes, hours or days (`90s`, `20m`, `4h`, `2d`) and\ndefaults to `24h`. The command SHALL measure window metrics over the trailing window\nending at the snapshot time, and state metrics at the snapshot time. The command SHALL NOT mutate Linear, GitHub or any repository",
      "id": "FR-024",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "context_only"
    },
    {
      "excerpt": "The retro commands SHALL keep their records in one append-only JSONL file per\nteam at `$TEAM_RETRO_HOME/<team>/snapshots.jsonl`, defaulting `TEAM_RETRO_HOME`\nto `~/.local/state/team-retro`. Each line SHALL be one record carrying\n`schema_version`, `kind` (`snapshot` or `backfill`), `taken_at` (UTC), `team`,\n`window`, and the metric map of [FR-024](./FR-024-retro-snapshot.md).\n\nWhen a retro command appends, it SHALL write the whole line with a single write to a\nfile opened in append mode, and SHALL create the directory when absent. The retro\ncommand SHALL NOT rewrite or truncate an existing line",
      "id": "FR-025",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "context_only"
    },
    {
      "excerpt": "The retro commands SHALL write each metric in a record with `status` (`ok` or `unavailable`), `value`\n(a number, or null when unavailable), `unit`, `direction`, `source` and `query`.\n\nIf a source cannot be read, is incomplete, or is not named in the\nconfiguration (a metric that needs a threshold or a paired field counts as not named when it is absent), then the command SHALL set `status` to `unavailable`, set `value` to\nnull, and record a `reason` naming the failed or missing source.\n\nThe command SHALL NOT report zero or a partial count for an unavailable metric.\n\nIf a paginated\nread ends befo",
      "id": "FR-026",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "context_only"
    },
    {
      "excerpt": "`ix-board retro delta --config <file> [--json]` SHALL read the store of the\nconfigured team and, for each metric of the latest record, report the value, the\nchange since the previous record, and the change since the baseline. The latest\nand previous records SHALL be the two records of kind `snapshot` with the\ngreatest `taken_at`. The baseline SHALL be the record named by\n`baseline_taken_at` in the configuration, or the earliest record of either kind\nwhen that field is absent. Backfilled records serve only as baseline and\nhistory. The command SHALL read no Linear or GitHub source.\n\n`baseline_ta",
      "id": "FR-027",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "context_only"
    },
    {
      "excerpt": "`ix-board retro backfill --config <file> --since <date> [--until <date>]\n[--window <duration>] [--json]` (default window `24h`, as for snapshot) SHALL append one record of kind `backfill` per\nUTC day in the range, each with `taken_at` at `23:59:59Z` of that day, using the\nsame sources as [FR-024](./FR-024-retro-snapshot.md). For each record, the command SHALL measure window\nmetrics over the half-open trailing window that ends at the next UTC midnight, so that\nconsecutive days are contiguous. `--until` SHALL default to the last complete UTC day, and\nthe command SHALL refuse a day that has not e",
      "id": "FR-028",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "context_only"
    }
  ]
}
```

## Dispositions

Round 1 at frozen head 8552acdc698794c628723a29c5dd2e57bb32cf25 (runtime fix 0f43e6e4ab4a5d2406841fd74c7aa883a1f34060; final commit artifact custody only). Original Findings and prior review conclusions remain unchanged as history.

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | Required collectors now have real native HTTP projections and versioned configured source-file normalization invoked by run_snapshot. Native Started/attachment joins, review/diff reads, source coverage/channel/watermark/attribution checks, reviewer marker joins and native/export fixture tests establish production paths. Original missing-source defect is fixed; new behavioral defects in those paths are separately recorded in SR-6056. |


## Disposition pass 1 verdict

**Original finding fixed. Repository assurance INCOMPLETE.** No new gap-method defect identified in the fix. Shared code remains blocked by SR-6056; this is not a full repository audit PASS.

Focused reviewer checks: make fmt-check exit 0; cargo test --lib retro::department exit 0 (27 tests); cargo test --lib department_native_state_http exit 0 (1 test); cargo test --test retro exit 0 (75 tests). Behavior probes and source-export probes compile/execute exit 0 against frozen production library and reproduce the remaining defects. Logs and Rust sources are beside this artifact. Baseline full-repository assurance remains INCOMPLETE: 180 older untagged criteria and no bound run evidence; unchanged baseline matrix not repeated. No optional semantic fan-out, full workspace gate, live source or credential operation was run. Exact model ID unavailable.

The source-adapter boundary, native initial state context, created/restarted claims, complete attachment merge joins, GH paging and dependency classification, export ordering/coverage, source attribution and actual snapshot/file integration were inspected. No new spec/plan diff requires a spec-review method.

Plan completion: not assessed

## Round 1 reviewer data

```json
{
  "bindings": [
    {
      "ac_id": "FR-029-AC-3",
      "test_id": "review_regression_reachable_external_blockers_require_valid_evidence_and_are_persisted",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-8",
      "test_id": "review_regression_state_bounce_retains_boundary_context_and_counts_inside_returns",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-12",
      "test_id": "review_regression_observed_unknown_preserves_other_lanes_and_compares_categorically",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "configured_native_http_and_export_file_flow_through_snapshot_store_and_missing_file_is_isolated",
      "trace": "correct"
    }
  ],
  "dispositions": [
    {
      "after_excerpt": "department::read_source_logs(department, frame, &mut batches, |path| {\n            std::fs::read_to_string(path).map_err(|e| format!(\"{}: {e}\", path.display()))\n        });\n        let mut report = department::collect(department, frame, &batches, &metrics);\n        let previous = stored\n            .iter()\n            .rev()\n            .find(|r| r.kind == RecordKind::Snapshot)\n            .and_then(|r| r.department.as_ref());\n        let baseline = config\n            .baseline_taken_at\n            .as_ref()\n            .and_then(|at| stored.it",
      "fnd": "FND-001",
      "outcome": "fixed",
      "reason": "Required collectors now have real native HTTP projections and versioned configured source-file normalization invoked by run_snapshot. Native Started/attachment joins, review/diff reads, source coverage/channel/watermark/attribution checks, reviewer marker joins and native/export fixture tests establish production paths. Original missing-source defect is fixed; new behavioral defects in those paths are separately recorded in SR-6056."
    }
  ],
  "new_findings": [],
  "scope": [
    {
      "excerpt": "With department absent, existing snapshot/delta/backfill fixtures retain their metric IDs, values, queries and selection behavior. A department snapshot retains the legacy map and appends one version-2 line with the version-1 envelope; prior store bytes remain identical.",
      "id": "FR-029-AC-1",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Two independent fixture teams with changed IDs/capacities serialize their configured scopes, observations, keyed measurements and evidence references; overlapping project membership counts native entities once. Replaying identical inputs/clock yields identical department payloads.",
      "id": "FR-029-AC-2",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Missing policy, source timestamp, complete history, completion obligations, reviewer-ready time, lane capacity or dependency reachability produces the affected unavailable/unassessed reason while unrelated measurements remain available. A partial blocker graph or cycle never yields an arbitrarily finite successful chain length.",
      "id": "FR-029-AC-3",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged.",
      "id": "FR-029-AC-4",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Exact threshold boundary fixtures exercise all four comparators and three comparison bases. Reversed thresholds, wrong units, duplicate IDs, unknown IDs and unsupported department versions fail naming the field, with no source read and no appended line. Missing optional policy/owner produces unassessed/unrouted rather than a fabricated threshold/owner.",
      "id": "FR-029-AC-5",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "All ten classes remain in the report with later collectors absent. Red+unknown yields red/incomplete, amber+unknown amber/incomplete, green+unknown unknown/incomplete, all green green/complete and an empty required set unknown/incomplete; optional metrics change neither severity nor completeness. Department rollup uses the same rules.",
      "id": "FR-029-AC-6",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Changed scope, formula/catalog identity, parameter, window or applicable assessment policy produces query_changed without numeric trend; reordered equivalent config does not. Missing department evidence in a version-1 baseline yields unavailable for new measurements while compatible old metrics still compare. Delta invokes no source reader.",
      "id": "FR-029-AC-7",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set.",
      "id": "FR-029-AC-8",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Empty duration populations are unavailable. For the singleton [7 hours], count=1, median=7 hours and p90=7 hours. Zero denominators are unavailable, and partial pages, stale sources and excessive future skew invalidate dependent measurements without turning a class green.",
      "id": "FR-029-AC-9",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Version-1 history remains byte-identical when read; unsupported record/envelope versions fail naming the version and leave the store unchanged. Shared JSON round trips preserve keyed values, evidence, comparison states, class completeness and owner/action routing without requiring a live source.",
      "id": "FR-029-AC-10",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "For expected state keys A/B/C, fixture A has median=1h/p90=10h, B median=1h/p90=2h, and C is missing. With gte amber=3h/red=5h and p90 selected, key severities are red/green/unknown and the metric is red/incomplete with C's missing-key reason. Selecting median yields green/green/unknown and unknown/incomplete; adding C median=1h/p90=1h yields green/complete for the median policy. Unselected key D with p90=100h changes neither result. Missing statistic/key reduction, empty expected key set, invalid component/unit or numeric policy on categorical evidence fails configuration validation. Changing",
      "id": "FR-029-AC-11",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "An offline lane_execution_state round trip preserves an FR-018 WORKING row and an UNKNOWN/partial row including null timestamp/count reasons, lifecycle provenance and source availability. No numeric state code or invented timestamp appears. Categories remain unassessed with categorical-evidence reason; a required categorical input makes its class unknown/incomplete. Compatible WORKING-to-WORKING and WORKING-to-UNKNOWN observations produce same and changed equality respectively without numeric trend; unavailable evidence produces unavailable and a changed source schema produces query_changed.",
      "id": "FR-029-AC-12",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "`ix-board retro snapshot --config <file> [--window <duration>] [--json]` SHALL\nread the structured Linear, GitHub and herdr sources named in the retro\nconfiguration, compute the metric catalog below for the configured team, and\nappend one record to the store defined by [FR-025](./FR-025-retro-store.md).\n`--window` takes seconds, minutes, hours or days (`90s`, `20m`, `4h`, `2d`) and\ndefaults to `24h`. The command SHALL measure window metrics over the trailing window\nending at the snapshot time, and state metrics at the snapshot time. The command SHALL NOT mutate Linear, GitHub or any repository",
      "id": "FR-024",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "context_only"
    },
    {
      "excerpt": "The retro commands SHALL keep their records in one append-only JSONL file per\nteam at `$TEAM_RETRO_HOME/<team>/snapshots.jsonl`, defaulting `TEAM_RETRO_HOME`\nto `~/.local/state/team-retro`. Each line SHALL be one record carrying\n`schema_version`, `kind` (`snapshot` or `backfill`), `taken_at` (UTC), `team`,\n`window`, and the metric map of [FR-024](./FR-024-retro-snapshot.md).\n\nWhen a retro command appends, it SHALL write the whole line with a single write to a\nfile opened in append mode, and SHALL create the directory when absent. The retro\ncommand SHALL NOT rewrite or truncate an existing line",
      "id": "FR-025",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "context_only"
    },
    {
      "excerpt": "The retro commands SHALL write each metric in a record with `status` (`ok` or `unavailable`), `value`\n(a number, or null when unavailable), `unit`, `direction`, `source` and `query`.\n\nIf a source cannot be read, is incomplete, or is not named in the\nconfiguration (a metric that needs a threshold or a paired field counts as not named when it is absent), then the command SHALL set `status` to `unavailable`, set `value` to\nnull, and record a `reason` naming the failed or missing source.\n\nThe command SHALL NOT report zero or a partial count for an unavailable metric.\n\nIf a paginated\nread ends befo",
      "id": "FR-026",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "context_only"
    },
    {
      "excerpt": "`ix-board retro delta --config <file> [--json]` SHALL read the store of the\nconfigured team and, for each metric of the latest record, report the value, the\nchange since the previous record, and the change since the baseline. The latest\nand previous records SHALL be the two records of kind `snapshot` with the\ngreatest `taken_at`. The baseline SHALL be the record named by\n`baseline_taken_at` in the configuration, or the earliest record of either kind\nwhen that field is absent. Backfilled records serve only as baseline and\nhistory. The command SHALL read no Linear or GitHub source.\n\n`baseline_ta",
      "id": "FR-027",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "context_only"
    },
    {
      "excerpt": "`ix-board retro backfill --config <file> --since <date> [--until <date>]\n[--window <duration>] [--json]` (default window `24h`, as for snapshot) SHALL append one record of kind `backfill` per\nUTC day in the range, each with `taken_at` at `23:59:59Z` of that day, using the\nsame sources as [FR-024](./FR-024-retro-snapshot.md). For each record, the command SHALL measure window\nmetrics over the half-open trailing window that ends at the next UTC midnight, so that\nconsecutive days are contiguous. `--until` SHALL default to the last complete UTC day, and\nthe command SHALL refuse a day that has not e",
      "id": "FR-028",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "context_only"
    }
  ]
}
```
