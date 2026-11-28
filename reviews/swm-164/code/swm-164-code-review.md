---
id: SR-6058
title: "SWM-164 code-review of operational collectors and shared reports"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; SWM-164; all 11 changed files in src/retro and tests/retro.rs; FR-029; canonical dev-team operational contracts"
review_set: subset
relationships:
  - {target: ix://agent-ix/ix-board/FR-029, type: references}
---

## Summary

Ticket: SWM-164. Independent frozen-head review at 9a425ffb30ca1a386d38e403ab6fef384ed1f03b, accepted base 4213e89eb6d4474d5496f838c9c1be13fe2c44f1. Exact model ID is unavailable. Reviewer-only artifacts remain outside the branch for lead/coder custody.

## Verdict

**FAIL — the original-send deduplication can erase a real order-flip finding; four additional behavioral edge defects remain.**

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Deduplicating orders before chronological sorting retains the lexicographically first event ID rather than the original send. For the same send_id, adding a later redelivery named a-redelivery ahead of z-original changes a valid two-reversal chain from one finding to an ok empty result. Select the original/earliest send and preserve its attribution/order context before deduplication; reject conflicting original-send provenance. | src/retro/department/operations.rs:1436-1448 |
| FND-002 | medium | order_flip_loop discards the predecessor outside lookback before evaluating in-window reversals. With o1 at -4000s, o2 reversing o1 at -3000s, o3 reversing o2 at -2000s, lookback=3600s and repeat=2, both reversal events are in the lookback but the detector returns ok with no finding. Retain the preceding order as context and count only qualifying reversal events inside lookback. | src/retro/department/operations.rs:1488-1497 |
| FND-003 | medium | Message deduplication discards a later authoritative classification for an already observed original message. An a-raw row with correction=null followed by z-classified with correction=true and valid attribution for the same message_id yields unavailable correction_rate and classified=0/total=1. The advertised classification-export input must merge usable attributed labels by native send identity, preserve the original sender/time, and reject conflicts; event-ID lexical order must not choose the classification. | src/retro/department/operations.rs:1261-1280 |
| FND-004 | medium | Point-in-time operational coverage must extend through frame.now even when the export is within its configured freshness limit. A host export observed 10 seconds ago with complete coverage through its observation, a 60-second freshness limit and 2-second clock tolerance is rejected as missing host coverage. This effectively caps point-in-time export freshness at clock tolerance and makes normal saved lifecycle/host/instruction snapshots unavailable. Validate gauge coverage at its observation and apply the configured age policy; retain end-of-window completeness for event populations. | src/retro/department/operations.rs:1791-1816 |
| FND-005 | medium | A malformed event timestamp in one configured operational family discards every observation and marks every family unavailable. A single export with a valid host row and malformed instruction event timestamp loses host_slots_used=1 and returns the instruction timestamp error for that host metric. Retain validated independent facts and scope normalization errors to their dependent capability instead of replacing the whole multi-family Evidence batch. | src/retro/department/operations.rs:1693-1707 |

## Coverage

Methods: code-review, including rust-review/rust-style; quoin:gap-analysis as the installed agent skill. No spec/plan changes, therefore no spec-review sub-analyses apply. No applicable AssuranceProfile found. AGENTS.md, CLAUDE.md, Cargo/Make/lint/formatter configuration and Rust API, implementation, boundary, safety, testing and applicable runtime guidance were read. Source/CI gates were not weakened. Exact reviewer model ID unavailable; run C8449AAC-9D1F-46A0-963A-DC8D5E64B821.

All 11 diff paths were read at frozen HEAD. Existing core integration, Fact dispatch, optional absent-field compatibility, snapshot composition, single writer append, strict DeltaReport decoding, saved Record JSON/JSONL/Delta selection, version refusal and input preservation were examined. The actual source-only TUI consumer in ix-relay-swm-165 uses read_saved_report and collect_snapshot; no second model/validator was introduced. Existing no-department/legacy fixtures and native lifecycle UNKNOWN/partial preservation pass through the real shared APIs. The operational public DTOs, file adapters and 30 metric paths plus order_flip_loop map to FR-029 and the canonical dev-team contracts. No new unowned behavior, copied canonical formula/specification, scheduler implementation, parallel lifecycle classifier or helper duplicate was found. Native wire DTOs are decode-only boundary records under the explicit authorized design.

Released-native verification: independently inspected dev-tools commit a44628f0c911e508f793e31a9b6b616d22b3da49, tools/ix-build/src/runtime.rs Report/RequestReport/status/phase_name, model.rs Event/RequestId, and config.rs HostSample. Fields, transparent request IDs, phase strings, u32 occupancy, u64 millisecond timestamps/durations and optional termination metadata match this consumer. No installed scheduler, state file, host probe or activation command was invoked. Missing relay persistent receipts/history remain genuine unknowns owned by SWM-45/SWM-174; source text is not used as an instruction or semantic classifier. Host capacity comes from supplied native data; reserves and policy are configured.

Reviewer checks in the sole retained target /Users/peter/dev/worktrees/ix-board-swm-164/target: make fmt-check exit 0; make lint exit 0 (all targets, -D warnings); cargo test --lib retro:: exit 0, 53 passed; cargo test --test retro exit 0, all 77 passed. Logs: fmt.log, lint.log, retro-lib.log, retro-integration.log under this scratch directory. Three existing source-free CLI cases used only malformed configuration/offline delta and synthetic tripwires, under the root's explicit clarification. No credential-backed CLI, key access, real environment key, alternate authentication or live collection was used. git diff --check exit 0. Root owns one final aggregate make ci; deny/aggregate were not repeated by this reviewer. All compiler/test processes finished; QUIESCENT. Target retained unchanged in location; no cleanup/new worktree/source mutation/push.

Offline Rust probes compiled against the frozen shared library and executed exit 0, recording actual wrong results rather than asserting a passing suite: redelivery changes one order-flip finding to an ok empty result; two in-window reversals with preceding context outside lookback also return an ok empty result; later attributed correction is dropped after a raw send; a 10-second-old host snapshot is rejected despite 60-second freshness; one malformed instruction timestamp erases valid independent host occupancy. probes.rs/probes.log contain the minimal reproductions. An initial scratch compile failed on a private timestamp helper; corrected to the existing chrono public API, then final compile and execution both exited 0. No reviewed branch file was edited.

Reconciliation: quoin matrix --repo /Users/peter/dev/worktrees/ix-board-swm-164 --json exit 0 (quoin 0.36.7; quire 0.52.0, engine 0.62.2). Computed repository matrix: 229 criteria; 41 tagged; 180 untagged; 8 method-without-symbol; 0 tagged-by-ignored-test. All FR-029 AC-1..12 are tagged. All 229 criteria have no run evidence bound. These unchanged legacy gaps remain baseline limitations, not a request for this ticket to repair the whole repository. Full repository assurance remains INCOMPLETE/FAIL, never a whole-repository PASS. matrix.json retains individual rows and binders.

quire coverage --scope /Users/peter/dev/worktrees/ix-board-swm-164 --json exit 0: 316 Rust candidates, 262 tagged, 98 bound, 0 untracked_symbols. Existing extraction diagnostics include cfg-related symbol collisions, legacy marker forms, NFR-001's unmatched Acceptance Criteria heading and absent optional archetypes; coverage.json retains the actual census and diagnostics. An engine tag proves a binding, not that every compound obligation passed. Code-review checked exact operational/native/saved-reader oracles; the new decision-waiter calculators have no nonzero fixture or DecisionFact test construction, recorded separately by gap-analysis.

Reverse inventory covers all src/tests paths (70 files, including 48 Rust files) and public surface groups: ready/project/search/cache, writes/handoff, drift/focus, credentials, lifecycle, program/chain/orphans/reconciliation and legacy retro map to FR-001..028; new department APIs/evidence/config/measurements/assessment/persistence map to FR-029 and linked canonical contracts. public-inventory.txt records public retro declarations; new surface was inspected in detail. The complete legacy body-by-body reverse audit remains INCOMPLETE. New unowned behavior: 0 identified; confirmed new source stubs: 0; confirmed new test stubs: 0. stub-scan.txt retains source/test TODO/ignore/placeholder candidates; deliberate compatibility re-exports and honest unavailable source results are not stubs. No new ignored tests or lowered coverage gates. No line coverage was collected; do not infer absence of coverage inflation from green tests.

Plan completion: not assessed

Optional semantic fan-out: skipped (no separate opt-in); required code-review specification/code/test checks were performed in this independent review.


## Examined scope and bindings

Each excerpt is a verbatim prefix up to 600 characters of its named unit. All listed examined units include clean portions; correct bindings verify partial obligations rather than proving the entire compound criterion. External dev-team identifiers refer to their explicit external paths, not local ix-board requirements with the same IDs.

```json
{
  "bindings": [
    {
      "ac_id": "FR-029-AC-2",
      "test_id": "capacity_load_and_signed_disk_headroom_use_configured_native_hosts",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-3",
      "test_id": "missing_remote_host_capacity_reserve_and_samples_do_not_fabricate_zero",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "scheduler_distributions_separate_completed_samples_from_current_ages_and_window_start",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-8",
      "test_id": "relay_enqueue_delivery_ack_and_redelivery_are_distinct_original_message_counts",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-9",
      "test_id": "denial_duration_unions_overlaps_intersects_window_and_requires_attribution",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-4",
      "test_id": "attributed_messages_deduplicate_redelivery_and_unknown_semantics_do_not_become_healthy",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-8",
      "test_id": "explicit_order_reversals_have_exact_evidence_and_changed_authority_suppresses_pairs",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-1",
      "test_id": "read_only_collection_uses_operational_exports_and_writer_appends_exactly_once",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-10",
      "test_id": "saved_json_jsonl_and_delta_share_version_validation_without_sources_or_writes",
      "trace": "correct"
    },
    {
      "ac_id": "FR-029-AC-12",
      "test_id": "categorical_lifecycle_keeps_partial_source_counts_and_provenance_without_transcript_text",
      "trace": "correct"
    }
  ],
  "clean": false,
  "findings": [
    {
      "artifact_id": "FR-029-AC-8",
      "check": "code-bug",
      "confidence": "high",
      "excerpt": "Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set.",
      "finding": "Deduplicating orders before chronological sorting retains the lexicographically first event ID rather than the original send. For the same send_id, adding a later redelivery named a-redelivery ahead of z-original changes a valid two-reversal chain from one finding to an ok empty result. Select the original/earliest send and preserve its attribution/order context before deduplication; reject conflicting original-send provenance.",
      "fnd": "FND-001",
      "lines": "1436-1448",
      "method": "code-review",
      "path": "src/retro/department/operations.rs",
      "related_ids": [],
      "severity": "high"
    },
    {
      "artifact_id": "FR-029-AC-8",
      "check": "code-bug",
      "confidence": "high",
      "excerpt": "Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication keys. Missing ordering history produces unavailable, not an empty successful finding set.",
      "finding": "order_flip_loop discards the predecessor outside lookback before evaluating in-window reversals. With o1 at -4000s, o2 reversing o1 at -3000s, o3 reversing o2 at -2000s, lookback=3600s and repeat=2, both reversal events are in the lookback but the detector returns ok with no finding. Retain the preceding order as context and count only qualifying reversal events inside lookback.",
      "fnd": "FND-002",
      "lines": "1488-1497",
      "method": "code-review",
      "path": "src/retro/department/operations.rs",
      "related_ids": [],
      "severity": "medium"
    },
    {
      "artifact_id": "FR-029-AC-4",
      "check": "code-bug",
      "confidence": "high",
      "excerpt": "The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obligations, reviewer age, fan-in, pin reachability and attributed diff/check classification. Missing attribution yields unavailable; all old metric oracles remain unchanged.",
      "finding": "Message deduplication discards a later authoritative classification for an already observed original message. An a-raw row with correction=null followed by z-classified with correction=true and valid attribution for the same message_id yields unavailable correction_rate and classified=0/total=1. The advertised classification-export input must merge usable attributed labels by native send identity, preserve the original sender/time, and reject conflicts; event-ID lexical order must not choose the classification.",
      "fnd": "FND-003",
      "lines": "1261-1280",
      "method": "code-review",
      "path": "src/retro/department/operations.rs",
      "related_ids": [],
      "severity": "medium"
    },
    {
      "artifact_id": "FR-029-AC-9",
      "check": "code-bug",
      "confidence": "high",
      "excerpt": "Empty duration populations are unavailable. For the singleton [7 hours], count=1, median=7 hours and p90=7 hours. Zero denominators are unavailable, and partial pages, stale sources and excessive future skew invalidate dependent measurements without turning a class green.",
      "finding": "Point-in-time operational coverage must extend through frame.now even when the export is within its configured freshness limit. A host export observed 10 seconds ago with complete coverage through its observation, a 60-second freshness limit and 2-second clock tolerance is rejected as missing host coverage. This effectively caps point-in-time export freshness at clock tolerance and makes normal saved lifecycle/host/instruction snapshots unavailable. Validate gauge coverage at its observation and apply the configured age policy; retain end-of-window completeness for event populations.",
      "fnd": "FND-004",
      "lines": "1791-1816",
      "method": "code-review",
      "path": "src/retro/department/operations.rs",
      "related_ids": [],
      "severity": "medium"
    },
    {
      "artifact_id": "FR-029-AC-3",
      "check": "code-bug",
      "confidence": "high",
      "excerpt": "Missing policy, source timestamp, complete history, completion obligations, reviewer-ready time, lane capacity or dependency reachability produces the affected unavailable/unassessed reason while unrelated measurements remain available. A partial blocker graph or cycle never yields an arbitrarily finite successful chain length.",
      "finding": "A malformed event timestamp in one configured operational family discards every observation and marks every family unavailable. A single export with a valid host row and malformed instruction event timestamp loses host_slots_used=1 and returns the instruction timestamp error for that host metric. Retain validated independent facts and scope normalization errors to their dependent capability instead of replacing the whole multi-family Evidence batch.",
      "fnd": "FND-005",
      "lines": "1693-1707",
      "method": "code-review",
      "path": "src/retro/department/operations.rs",
      "related_ids": [],
      "severity": "medium"
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
      "excerpt": "A report from two configured teams contains their separate populations and one deduplicated department rollup; no out-of-scope issue or remote lane is silently included.",
      "id": "FR-001-AC-1",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-001-evidence-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Removing a required page, host, source timestamp or capacity config makes dependent values null/unavailable with source and reason; unaffected values remain visible and no unavailable class is green.",
      "id": "FR-001-AC-2",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-001-evidence-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Every value exposes metric ID/version, scope, observation/window timestamps, unit, population/denominator, source references, query/config identity and status; classified events also expose attribution and classification version.",
      "id": "FR-001-AC-3",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-001-evidence-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "A relay enqueue does not count as delivery or acknowledgement; an ambiguous pane/session association is unknown rather than assigned to the focused pane.",
      "id": "FR-001-AC-4",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-001-evidence-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "An existing metric fixture produces unchanged values after catalog extension; changed formulas use a new metric ID.",
      "id": "FR-001-AC-5",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-001-evidence-contract.md",
      "role": "examined"
    },
    {
      "excerpt": "Fixed fixtures compute every catalog entry with exact expected count, age, ratio or nearest-rank duration percentile; replaying the same evidence and clock yields identical metric payloads.",
      "id": "FR-002-AC-1",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "examined"
    },
    {
      "excerpt": "Boundary fixtures on either side of configured stale, repin-window, WIP, wait, hold, refusal and message thresholds produce the documented assessment; absent thresholds produce unassessed values, never green.",
      "id": "FR-002-AC-2",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "context_only"
    },
    {
      "excerpt": "Tag→repin, state-bounce and reversed-order fixtures emit the exact matching event IDs and participants once per qualifying sequence; duplicate events, legitimate new evidence and unrelated entities do not create a loop.",
      "id": "FR-002-AC-3",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "examined"
    },
    {
      "excerpt": "Missing state history, truncated relations, unknown message classification and an unavailable remote host make the corresponding metrics unavailable without lowering counts or denominators.",
      "id": "FR-002-AC-4",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "examined"
    },
    {
      "excerpt": "Previous/baseline comparisons include absolute changes and direction; incompatible query/config/catalog identity emits query_changed. Empty and singleton duration populations follow the catalog rules.",
      "id": "FR-002-AC-5",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "examined"
    },
    {
      "excerpt": "A department scorecard lists all ten problem classes, source age, available trend, threshold and one configured owner/action for each red; missing owner configuration is visible as unrouted rather than guessed.",
      "id": "FR-002-AC-6",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "context_only"
    },
    {
      "excerpt": "A newly started claim without movement is excluded from claims_without_movement until its latest start/claim age exceeds the configured stale interval. An older claim without subsequent movement counts; recent movement or a restarted claim excludes it; missing claim history makes the metric unavailable.",
      "id": "FR-002-AC-7",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "context_only"
    },
    {
      "excerpt": "Required metric mixes red+unknown and amber+unknown produce red and amber with complete=false and missing IDs; green+unknown produces unknown; all-green produces green with complete=true; an empty required set produces unknown. Optional metric statuses do not alter these results.",
      "id": "FR-002-AC-8",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-002-deterministic-metrics.md",
      "role": "context_only"
    },
    {
      "excerpt": "Equal-priority eligible requests run in enqueue order; repeated high-priority arrivals cannot pass an eligible request after its configured reservation threshold. A deterministic-clock test proves this under sustained arrivals.",
      "id": "FR-005-AC-1",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "Higher-priority waiting work gets the next available capacity before aging reservation, while an already running child continues; no new grant exceeds current capacity or duplicates leased units. If capacity shrinks from two units to one while two units are running, both continue, overcommit is reported, and new admission waits for sufficient release.",
      "id": "FR-005-AC-2",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "Capacity shrink/growth, unavailable/stale host telemetry, insufficient disk reserve and oversized requests follow the defined admission behavior without a hardcoded slot count.",
      "id": "FR-005-AC-3",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "Timeout, cancellation, child crash, scheduler restart, stale heartbeat and PID-reuse fixtures reclaim leases only after verified termination of the owned child group; unrelated children are never terminated.",
      "id": "FR-005-AC-4",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "Each queued/running request exposes wait/hold age, owner, resource units, host capacity and termination reason through JSON events consumed by ix-board; enqueue/start/end calculations match the actual transitions.",
      "id": "FR-005-AC-5",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "examined"
    },
    {
      "excerpt": "Logic is Rust in a shared tool with a thin Make invocation; consumers migrate from project wrappers rather than copying shell/Python arbitration into Rust repositories.",
      "id": "FR-005-AC-6",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "A child returning 75 is invoked once and its status is propagated; a signaled child returns non-success. Neither outcome re-enters acquisition contention or silently reruns the build.",
      "id": "FR-005-AC-7",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "On a host with total capacity two units, a queued two-unit request reaches the reservation threshold while one-unit jobs keep arriving. The request is reservation-eligible even with fewer than two free units; no newer job consumes subsequently released units, and the reserved request starts after both units are free.",
      "id": "FR-005-AC-8",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/spec/functional/FR-005-fair-build-scheduler.md",
      "role": "context_only"
    },
    {
      "excerpt": "| Capacity/resources | `host_slots_used`, `host_slots_free`: occupied units and max(0, capacity-used) per host at end; `host_overcommit_units`: max(0, used-capacity); `queue_wait_minutes`: request start-enqueue for starts in window plus current ages for queued requests, separately labeled; `lock_hold_minutes`: end-start for finishes in window plus current ages for running requests, separately labeled | Scheduler events/verified live/config capacity; no wrapper-count heuristic is presented as a lease count | Build-tool/host owner: fair queue, bound holds and recover suspect leases |",
      "id": "department-monitoring.md:L90",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Capacity/resources | `host_load_per_slot`: host load1 / configured effective host build capacity; `disk_headroom_bytes`: available filesystem bytes minus configured reserve; `disk_available_bytes`: actual free bytes, separate from **existing** disk_bytes directory usage | Host telemetry with observation age, filesystem identity, reserve/capacity policy; unknown capacity or reserve unknown ratio/headroom | Host owner: stop new admission below reserve, clean owned leftovers and rebalance within scope |",
      "id": "department-monitoring.md:L91",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Liveness | `lane_execution_state`: lifecycle-based pane report with evidence timestamps and uncertainty; `login_blocked_lanes`: verified login/auth-screen panes; `order_unacked_age_minutes`: age since delivery (or enqueue if delivery unknown, separately labeled) for orders lacking acknowledgement; `lane_no_movement_hours`: time since most recent verified token, PR-head, commit or worktree movement | Pane agent list/lifecycle, token telemetry, VCS, relay event chain; remote hosts included by explicit lane config, omitted host unavailable. Tokens indicate activity, not acceptance or progress |",
      "id": "department-monitoring.md:L92",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Authority/policy | `refusals_by_lane`: attributed denied order events in window; `frozen_denial_hours`: union of denial-to-resolution intervals intersected with window per lane; `repeated_orders`: order IDs resent without changed evidence/authority, count sends after first | Explicit order/denial/resolution IDs and actor/classifier attribution; transcript interpretation absent = unavailable. Unresolved denial extends to end; overlaps count once | Planner/owner: supply scoped standing authority or missing allow-rule; no permission bypass |",
      "id": "department-monitoring.md:L93",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Signal integrity | `reconciliation_agreement_ratio`: matching tool/ground-truth entities / fully paired entities; `reconciliation_unpaired`: missing-side entity count, separate; `source_age_hours`; `instruction_version_mismatches`: active instruction release claims inconsistent with resolved executable/plugin versions | Time-aligned native identity observations and claimed/actual versions; no complete paired population = unknown; timestamp skew invalidates pair | Source/tool owner: verify discrepancy, file bug and report stale facts |",
      "id": "department-monitoring.md:L94",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Signal integrity | `relay_queued`, `relay_delivered`, `relay_acknowledged`: distinct event counts in window; `prompt_timeout_unconfirmed`: timed-out delivery attempts lacking later receipt evidence | Read-only relay log and receipt/transcript evidence; missing delivery instrumentation unknown, not failed | Relay/Herdr owner: inspect evidence before retry; no duplicate send on queued success |",
      "id": "department-monitoring.md:L95",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Decision latency | `decisions_open`, `decision_oldest_hours` **existing**; `ruling_waiting_issues`: distinct open issues structurally blocked by configured decision/ruling issues; `spec_merge_waiting_issues`: open issues structurally blocked by unmerged owned spec deliverables | Complete labels, dependency graph and PR evidence; absent decision classification unavailable | Planner/architecture owner: decide critical ruling, separate ruler/coder, authorize branch-head work only where suitable |",
      "id": "department-monitoring.md:L97",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    },
    {
      "excerpt": "| Comms overhead | `messages_per_sender`: sends by canonical sender in window; `correction_rate`: explicitly classified correction sends / all classifiable sends, alongside classified/all coverage; `status_only_messages`: attributed status-only sends; `planner_ad_hoc_assignments`: assignments outside published wave/plan revision; `planner_scope_violations`: assignments outside authorized lane scope; `planner_unbatched_sends`: actionable sends after first to same lane/tick absent new blocking evidence | Relay sends/order records, published plan revision, assignment events, authority scope and a",
      "id": "department-monitoring.md:L98",
      "path": "/Users/peter/dev/worktrees/dev-team-swm-161/docs/department-monitoring.md",
      "role": "examined"
    }
  ]
}
```


## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Round 1: Chronological ordering now precedes send deduplication, preserving the earliest original event and refusing conflicting fact/entity/classifier provenance. Original independent probe remains one finding with redelivery; the new fixture also checks reversed input order and conflicting actor refusal. |
| FND-002 | fixed | Round 1: All prior orders remain context; only reversal endpoints after the open lookback boundary increment the count. The original two in-window reversals now yield one finding, and the boundary fixture excludes a reversal exactly at the start. |
| FND-003 | fixed | Round 1: Native message identity now merges attributable correction/status labels while original canonical sender and sent_at remain immutable. The original raw-then-classified probe returns correction_rate=1, classified=1/total=1; the new fixture checks order independence, label conflict and send-time conflict. |
| FND-004 | fixed | Round 1: Gauge coverage is evaluated at source observation while configured batch/row freshness remains enforced. The original 10-second snapshot is now available under its 60-second limit; the new fixture still refuses a 61-second stale snapshot and short event-window coverage. |
| FND-005 | fixed | Round 1: Malformed rows and coverage intervals are recorded against their known family and independently valid observations are retained. Original malformed-instruction probe now preserves host_slots_used; the fixture also covers malformed instruction coverage without losing valid host data. |

## Disposition pass 1 verification and current verdict

Frozen disposition head: 56312d49a35302635443d3fcd7b38fb6300d6bd8; fixes 4630029, 4692952, 56312d4. Every original finding in this artifact had no prior outcome and was checked in round 1. Original finding rows, scope and historical verdict are preserved byte-for-byte above; this appended section records the current outcome. No new findings. Exact reviewer model ID unavailable; run E4493D4F-DAA3-4B22-9E54-D57BACEDE46A.

Reviewer-run checks: make fmt-check exit 0; make lint exit 0 (all targets, -D warnings); cargo test --lib retro:: exit 0 (59 passed); cargo test --test retro exit 0 (77 passed, 0 filtered). Original five independent Rust probes recompiled against the frozen shared library, compile exit 0 and execution exit 0: redelivery leaves the finding intact, preceding context yields the expected loop, classification merges to rate 1/coverage 1 of 1, fresh gauge succeeds, malformed instruction preserves host occupancy. Logs: code-disposition1/{fmt.log,lint.log,retro-lib.log,retro-integration.log,probes-compile.log,probes.log}. No aggregate gate repeated; root owns final make ci. Shared saved-reader/collect_snapshot source APIs and version rules are unchanged; their integration tests passed again. No spec/plan changes or additional review method applies. Prior computed matrix limitations remain unchanged: 180 legacy untagged criteria, no bound run evidence, full legacy reverse audit INCOMPLETE; no redundant matrix scan or whole-repository PASS claim. Plan completion: not assessed.

All six original findings across SR-6058/SR-6059 are fixed; this scoped review is clear for the lead's final exact-head gate and merge. Overall repository assurance remains INCOMPLETE for the recorded legacy baseline. No source changes, push, new worktree, credentials/live collection, host/scheduler calls or cache deletion. All own compiler/test processes completed: QUIESCENT; sole target retained.

```json
{
  "dispositions": [
    {
      "after_excerpt": "        candidates.sort_by(|a, b| {\n            event_time(a.0)\n                .unwrap_or(0.0)\n                .total_cmp(&event_time(b.0).unwrap_or(0.0))\n                .then(a.0.sequence.cmp(&b.0.sequence))\n        });\n        let mut rows = Vec::new();",
      "artifact_id": "FR-029-AC-8",
      "fnd": "FND-001",
      "original_finding": "Deduplicating orders before chronological sorting retains the lexicographically first event ID rather than the original send. For the same send_id, adding a later redelivery named a-redelivery ahead of z-original changes a valid two-reversal chain from one finding to an ok empty result. Select the original/earliest send and preserve its attribution/order context before deduplication; reject conflicting original-send provenance.",
      "outcome": "fixed",
      "path": "src/retro/department/operations.rs",
      "verification": "Chronological ordering now precedes send deduplication, preserving the earliest original event and refusing conflicting fact/entity/classifier provenance. Original independent probe remains one finding with redelivery; the new fixture also checks reversed input order and conflicting actor refusal."
    },
    {
      "after_excerpt": "            if let Some((old_row, old)) = previous.get(&key)\n                && event_time(row)? > self.frame.now - lookback as f64\n                && f.supersedes_order_id.as_deref() == Some(&old.order_id)\n                && f.reverses_order_id.as_deref() == Some(&old.order_id)\n                && f.direction != old.direction\n                && f.evidence_revision == old.evidence_revision",
      "artifact_id": "FR-029-AC-8",
      "fnd": "FND-002",
      "original_finding": "order_flip_loop discards the predecessor outside lookback before evaluating in-window reversals. With o1 at -4000s, o2 reversing o1 at -3000s, o3 reversing o2 at -2000s, lookback=3600s and repeat=2, both reversal events are in the lookback but the detector returns ok with no finding. Retain the preceding order as context and count only qualifying reversal events inside lookback.",
      "outcome": "fixed",
      "path": "src/retro/department/operations.rs",
      "verification": "All prior orders remain context; only reversal endpoints after the open lookback boundary increment the count. The original two in-window reversals now yield one finding, and the boundary fixture excludes a reversal exactly at the start."
    },
    {
      "after_excerpt": "            if send.sender != sender || send.sent_at != sent_at {\n                return Err(\"conflicting original message sender/time provenance\".into());\n            }\n            if attributed(row).is_ok() {\n                merge_message_label(&mut send.correction, f.correction);\n                merge_message_label(&mut send.status_only, f.status_only);\n            }",
      "artifact_id": "FR-029-AC-4",
      "fnd": "FND-003",
      "original_finding": "Message deduplication discards a later authoritative classification for an already observed original message. An a-raw row with correction=null followed by z-classified with correction=true and valid attribution for the same message_id yields unavailable correction_rate and classified=0/total=1. The advertised classification-export input must merge usable attributed labels by native send identity, preserve the original sender/time, and reject conflicts; event-ID lexical order must not choose the classification.",
      "outcome": "fixed",
      "path": "src/retro/department/operations.rs",
      "verification": "Native message identity now merges attributable correction/status labels while original canonical sender and sent_at remain immutable. The original raw-then-classified probe returns correction_rate=1, classified=1/total=1; the new fixture checks order independence, label conflict and send-time conflict."
    },
    {
      "after_excerpt": "        let required_until = if gauge { observed } else { frame.now };\n        let from = if gauge {\n            observed\n        } else {\n            frame.now - frame.window_secs as f64\n        };\n        let origin = matches!(family.as_str(), \"order\" | \"denial\" | \"relay_event\");",
      "artifact_id": "FR-029-AC-9",
      "fnd": "FND-004",
      "original_finding": "Point-in-time operational coverage must extend through frame.now even when the export is within its configured freshness limit. A host export observed 10 seconds ago with complete coverage through its observation, a 60-second freshness limit and 2-second clock tolerance is rejected as missing host coverage. This effectively caps point-in-time export freshness at clock tolerance and makes normal saved lifecycle/host/instruction snapshots unavailable. Validate gauge coverage at its observation and apply the configured age policy; retain end-of-window completeness for event populations.",
      "outcome": "fixed",
      "path": "src/retro/department/operations.rs",
      "verification": "Gauge coverage is evaluated at source observation while configured batch/row freshness remains enforced. The original 10-second snapshot is now available under its 60-second limit; the new fixture still refuses a 61-second stale snapshot and short event-window coverage."
    },
    {
      "after_excerpt": "        match validation {\n            Ok(()) => rows.push(row),\n            Err(reason) => {\n                errors\n                    .entry(family.into())\n                    .or_default()\n                    .insert(format!(\"{}: {reason}\", row.event_id));\n            }\n        }\n    }\n    evidence.observations = rows;",
      "artifact_id": "FR-029-AC-3",
      "fnd": "FND-005",
      "original_finding": "A malformed event timestamp in one configured operational family discards every observation and marks every family unavailable. A single export with a valid host row and malformed instruction event timestamp loses host_slots_used=1 and returns the instruction timestamp error for that host metric. Retain validated independent facts and scope normalization errors to their dependent capability instead of replacing the whole multi-family Evidence batch.",
      "outcome": "fixed",
      "path": "src/retro/department/operations.rs",
      "verification": "Malformed rows and coverage intervals are recorded against their known family and independently valid observations are retained. Original malformed-instruction probe now preserves host_slots_used; the fixture also covers malformed instruction coverage without losing valid host data."
    }
  ]
}
```
