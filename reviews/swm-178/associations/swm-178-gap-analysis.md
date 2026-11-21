---
id: SR-6081
title: "Gap analysis — SWM-178 resolved-input associations"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; repository matrix, source/test surface and stub inventory; PR 77 association API, tests and guide; FR-004 and FR-016"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
---

## Summary

Ticket: SWM-178. Planless repository-driven audit at 55e33220bad48e2f93996bef8a583826762f146f against accepted 2d8c2a8d6c2720066930355135cf6b1e75cf0a48. No new association-specific trace, reverse-gap or stub defect was found; the standing 180 legacy untagged criteria remain separately accepted outside PR 77.

## Verdict

**PASS for the correction-specific gap review. Repository-wide assurance remains FAIL** because the computed matrix still contains the accepted 180 legacy untagged criteria. This does not approve PR 77 while SR-6080 findings remain. No clean-repository assurance, prior PR re-review or final aggregate is claimed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Scope examined

```yaml
scope:
  - {id: FR-016-AC-11, path: spec/functional/FR-016-focus-record.md, role: examined, excerpt: "Canonicalizing named string inputs yields canonical IDs and the milestone's owning project ID, and fails with no partial document for a missing, ambiguous, other-team, or project-mismatched member; the stored document contains no label, expanded issue ID, or status."}
  - {id: FR-016-AC-6, path: spec/functional/FR-016-focus-record.md, role: examined, excerpt: "A version 3 named selection with team, include, exclude, and a milestone carrying `project_id` round-trips through encode and decode unchanged; the same selection built in any member order encodes to identical bytes in canonical order; a member present in both lists stays in both."}
  - {id: FR-004-AC-14, path: spec/functional/FR-004-cache-polling.md, role: examined, excerpt: "Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds."}
  - {id: FR-004-AC-16, path: spec/functional/FR-004-cache-polling.md, role: examined, excerpt: "After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker."}
```

All four criteria are engine-tagged. New bindings were inspected against their assertions:

```yaml
bindings:
  - {test_id: every_input_occurrence_keeps_its_list_index_and_exact_member, ac_id: FR-016-AC-11, trace: correct}
  - {test_id: every_input_occurrence_keeps_its_list_index_and_exact_member, ac_id: FR-016-AC-6, trace: correct}
  - {test_id: a_caller_attaches_its_own_labels_to_aliases_and_repeats_without_resolving_again, ac_id: FR-016-AC-11, trace: correct}
  - {test_id: the_same_member_in_both_lists_is_one_occurrence_per_list, ac_id: FR-016-AC-11, trace: correct}
  - {test_id: the_selection_and_its_wire_bytes_equal_the_canonical_forms_and_carry_no_association, ac_id: FR-016-AC-11, trace: correct}
  - {test_id: the_selection_and_its_wire_bytes_equal_the_canonical_forms_and_carry_no_association, ac_id: FR-016-AC-6, trace: correct}
  - {test_id: cold_warm_and_changed_sources_resolve_from_one_snapshot_each, ac_id: FR-016-AC-11, trace: correct}
  - {test_id: cold_warm_and_changed_sources_resolve_from_one_snapshot_each, ac_id: FR-004-AC-14, trace: correct}
  - {test_id: a_good_member_then_a_bad_one_returns_no_selection_and_no_associations, ac_id: FR-016-AC-11, trace: correct}
  - {test_id: a_good_member_then_a_bad_one_returns_no_selection_and_no_associations, ac_id: FR-004-AC-16, trace: correct}
  - {test_id: capturing_associations_spends_the_same_one_budget_as_resolution_and_ordering, ac_id: FR-004-AC-14, trace: correct}
  - {test_id: capturing_associations_spends_the_same_one_budget_as_resolution_and_ordering, ac_id: FR-016-AC-11, trace: correct}
```

These are supporting bindings, not assertions that each test exhausts every clause of its AC. The independent expected vectors, alias/repeat metadata joins, typed error checks and capture-phase budget oracle exercise production behavior. No internal replacement or new production test hook is used. SR-6080 records the code and docs findings separately, without duplicating them here.

## Coverage

- Plan completion: not assessed
- Reconciliation: quire matrix --scope /Users/peter/dev/ix-board-swm-178 --format json, exit 0; quire 0.52.0 (engine 0.62.2). Static reconciliation: no run evidence read; no grep fallback.
- Computed matrix: 273 criteria; tagged 84, untagged 180, method-without-symbol 9; ignored-only, qualified-record and retired each 0. The baseline remains accepted separately, not repaired or relabeled clean.
- quire coverage --scope /Users/peter/dev/ix-board-swm-178 --json, exit 0, reports its distinct 272 reference-row denominator, 84 backed and 9 no-symbol rows. Rust census: 398 candidates, 336 tagged, 173 bound. Untracked symbols 0; unmatched tags 225. None of the seven new tests has an unmatched tag. These are tool-defined counts, not an invented percent-complete claim.
- Diagnostics: 274 trace-id-outside-marker; two absent optional archetypes (StR and TestMatrix); one NFR-001 Acceptance Criteria heading mismatch; one uncatalogued verification method; one catch-all-universal classification. Coverage stderr also records unchanged cfg-alternative symbol collisions in cache.rs and lib.rs. Raw matrix.json/matrix.stderr and coverage.json/coverage.stderr retain those distinctions. No diagnostic was silently interpreted as a new missing test.
- Reverse-gap inventory: 43 src files, public exports/methods and src/main.rs command tree checked against existing behavior families FR-001 through FR-029: queue/chunks/search, cache/structured boundaries, controls, drift, focus, credentials, issues, agents, scope/roadmap/reconcile/chain/orphans and retro. surface-stub-inventory.txt retains the symbol scan. Inventory does not claim a new semantic audit of every historical symbol.
- One additive public behavior: preserving final validated per-input associations alongside a bounded canonical selection, owned by FR-016-AC-11 and FR-004 whole-operation bounds, with the accepted FR-061 consumer label requirement as context. Three result/domain types and the lookup helper support that behavior. No new CLI, config, wire version, storage or source reader. New untraced behavior count 0. The duplicate list type is a code-review finding, not a new unspecified business behavior.
- Whole-source/test scan found no todo! or unimplemented! body and no required-work TODO/FIXME/XXX; the sole TODO text is a test issue identifier IR-TODO. New source stubs 0; new test stubs 0. No line coverage or coverage inflation claim was used as evidence.
- Optional whole-repository semantic review skipped. Targeted changed behavior/AC/test agreement was requested and checked; no extra spec-analysis campaign, plan audit or child agent was run.
- Root actual execution logs show all seven new tests and the existing bounded nested-scan cases passing, with focused counts and limits recorded in SR-6080. Reviewer ran no Cargo or runtime gate. Root owns the one final PR 77 aggregate after fixes and dispositions.

## Provenance and custody

quoin 0.36.7; spec-artifacts-process@v0.29.2. The installed authoring pack, skeleton and schema were read. SR-6081 was verified unused in the repository/scratchpad/tracker before allocation. Exact runtime model ID unavailable; run A559CD0A-6F84-4184-90DD-B26406F34287. This artifact is outside the frozen worktree for later verbatim custody. Standing historical findings and comments remain unchanged.

## Tracker record

Linear SWM-178 comment cac85d5a-5812-4f51-a670-449de8ec5497. Read-back passed: exact body, parsed YAML, scope and source excerpts, zero non-placeholder findings and clean flag. The clean flag describes new correction findings only; accepted repository gaps remain explicit above.

## Narrow fix confirmation

Disposition round 1 at 9fa5ea1dd087ab1752485dfa0408aeddb3540ad6: inspected the full diff from 55e33220bad48e2f93996bef8a583826762f146f. It removes the redundant NamedList enum, uses the existing ListKind in the same association API/tests/guide, and splits the label-policy sentence. No value assertion, association resolution/deadline behavior, wire/schema, spec, test trace marker or business behavior changed. Correction-specific gap verdict remains **PASS**; unchanged standing 180 legacy untagged criteria still prevent a repository-wide assurance PASS. The matrix/coverage counts above remain their original recorded snapshot, not a fresh run. Plan completion: not assessed.

SR-6080 records both fixes and root's focused checks. No gap finding required disposition, so no duplicate gap-analysis/dispositions comment was posted. No new test, runtime, matrix, broad semantic or spec campaign was run for this enum reuse. Root owns latest verbatim artifact custody and one final aggregate after the code-review disposition PASS.
