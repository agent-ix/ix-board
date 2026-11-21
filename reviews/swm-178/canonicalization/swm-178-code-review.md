---
id: SR-6078
title: "Code and Rust review — SWM-178 bounded canonicalization correction"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; correction PR 76; src/lib.rs, src/named_selection.rs, src/scope.rs, tests/bounded_read.rs, docs/named-focus.md; FR-004, FR-010, FR-016"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-004", type: references}
  - {target: "ix://agent-ix/ix-board/FR-016", type: references}
---

## Summary

Ticket: SWM-178. Independently reviewed correction PR 76 at frozen 1b0941ae1d758580cadb3151fa11f4bdacb5b314 against accepted 53606425728d94f24d1c5acca9068c76c78be1d2, in the sole /Users/peter/dev/ix-board-swm-178 worktree. Code-review includes its Rust-review lane. Two actionable defects remain in this correction.

## Verdict

**FAIL** — nested resolution work can run without cooperative deadline checks; canonical member ordering has two implementations. Fix both in PR 76 before the final aggregate gate.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Nested initiative, milestone and team-membership scans can outrun the shared deadline because only the enclosing project loop checks it. | src/scope.rs:72; src/scope.rs:157; src/scope.rs:174; src/scope.rs:215; src/named_selection.rs:303; src/named_selection.rs:318; src/named_selection.rs:357; src/named_selection.rs:396 |
| FND-002 | high | canonical_list_within duplicates canonical_list's canonical ordering and deduplication rule instead of sharing one implementation. | src/named_selection.rs:128; src/named_selection.rs:278 |

## Evidence and required correction

FND-001: resolve_initiative_id_within checks once per project, then ids.extend traverses every initiative without checking. resolve_milestone_id_within likewise traverses every milestone in its exact-ID pass, whole-name pass and qualified-name pass without checking inside those loops. canonical_member's initiative-membership and milestone-owner .any scans, and team_is_known/holds_team_work's team_keys.contains scans have the same gap. Project.initiatives, Project.milestones and Project.team_keys are unconstrained Vec fields. One project can therefore contain enough records to overrun D after the preceding project-level check; the next outer/final check only refuses after all that work. For a missing milestone, the final resolver error can propagate directly through ? without the public success-only final check. This is a boundedness defect even though no successful document escapes after the final check. Check the same budget at bounded intervals inside each nested collection traversal, preserving existing resolution precedence and typed errors. Include the final ordered-list traversal and other variable-length scans in that audit. No new deadline or detached worker is needed.

FND-002: searched src/ for canonical_list, BTreeSet<NamedMember> and ordered.insert. Existing canonical_list at line 128 creates a BTreeSet and collects it; the newly added canonical_list_within independently creates a BTreeSet, inserts and collects. NamedSelection::new/canonical/encode use the former while both canonicalizers now use the latter. A change to normalization or duplicate handling must therefore edit two implementations to preserve the newly documented equality. The code-review skill's within-repository duplication rule requires one implementation and grades duplication high. Make both callers share the budget-capable implementation (or a common ordering core), with a deliberate infallible unbounded adapter; keep public signatures unchanged.

## Scope examined

Every unit below has been read; examined means checked against the correction, not full historical acceptance.

| Unit | Role | Excerpt |
| --- | --- | --- |
| FR-004, Deadline clause | examined | The deadline is the bound on the whole operation, measured from the start below, with no additional allowance of one request timeout. |
| FR-016-AC-11 | examined | Canonicalizing named string inputs yields canonical IDs and the milestone's owning project ID, and fails with no partial document for a missing, ambiguous, other-team, or project-mismatched member; the stored document contains no label, expanded issue ID, or status. |
| FR-010, Qualified milestones | examined | A string input keeps the existing grammar, including slashes inside either name, and canonicalizes to the milestone ID and its owning project ID. |
| FR-004-AC-12 | context_only | Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds or more is refused as an invalid policy with no source read. |
| FR-004-AC-14 | context_only | Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds. |
| FR-004-AC-16 | context_only | After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker. |
| canonical_list | examined | fn canonical_list(members: Vec<NamedMember>) -> Vec<NamedMember> |
| canonical_list_within | examined | fn canonical_list_within( |
| Public composed API | examined | pub fn canonicalize_named_selection_bounded( |

The accepted consumer FR-061/FR-063 copies at authphase 9a804538a0db533b8972d9b9aef908d9dd49619a were context: create/update accepts names and identifiers, resolves credentials first, then requires one read-plus-validation budget before caller publication. No consumer spec/API was invented. The new public method and its input/output types are externally reachable; tests import them through ix_board. It admits policy/support before reads, creates one Deadline before read_within/cache, passes the same deadline into canonicalize_within and checks successful completion. No auth invocation, reset budget, raw reader, publisher, store, provenance requirement or caller-thread workaround is introduced. Returning NamedSelection is sufficient because read_within already enforces maximum age.

## Checks and limits

Read all five changed files and relevant unchanged read_within, Budget, NamedSelection, standalone/resolver entry points, public exports, fixture clock/source, scope/selection tests and accepted source requirements. The legacy resolver functions delegate with Budget::unbounded; ID/name precedence, case matching, slash split grammar, duplicate handling, ownership checks and v2/v3 wire definitions otherwise retain their prior semantics. No source/spec/CI/Makefile/manifest/lockfile change beyond the five-file correction; no weakened gate, unsafe addition, new dependency, panic path, retry or spawned worker found. No applicable AssuranceProfile or repository Rust skill was found. Tokio review is not applicable to these synchronous changes.

Root-owned logs were inspected: swm178-canonicalization-clippy.log completed all-target Clippy with -D warnings; focused-tests.log reports library 170, bounded_read 22 (four new composed cases), named_selection 5, scope 6 and integration 30 passed plus 1 ignored; rustdoc.log completed with root's -D warnings setting; fmt-check.log and root check-results record exit 0. Root performed those checks; this reviewer ran no Cargo, formatter or aggregate gate, and claims no behavioral red or mutation execution. The source/format custody handoffs reconcile the single import-line formatting commit with the exact frozen head. Root's one final correction make ci, including deny/unsafe checks, remains pending after findings/dispositions. Prior PR 75's historical gate is not acceptance for this correction.

Reviewer methods: dev-team reviewer 0.4.1, dev-tools code-review/rust-review/rust-style 0.3.2 with API, implementation, safety, wire, testing and detailed review references. Exact runtime model ID is unavailable; no model identifier is inferred. No children, code edits, commits, extra worktree/target, Python, live ix-board/auth/Keychain, signing or real endpoint were used.

## Dispositions

Round 1, independently inspected at frozen 0e80afdb43d744b0487b6cddc4f6960ca88b62d2, against the original reviewed 1b0941ae1d758580cadb3151fa11f4bdacb5b314. Original findings, verdict and review-pass evidence above are retained unchanged as history.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Initiative lookup and all three milestone passes now check each inner record. Budget::any checks before each predicate; Budget::contains delegates to it. Both team-key scans, initiative membership and milestone ownership now use those helpers with the existing same Budget. No new deadline, public signature, lookup precedence, error shape or wire semantics. Root RED/GREEN evidence and eight phase-specific public adverse tests support the fix. |
| FND-002 | fixed | canonical_list_with is the single BTreeSet ordering/deduplication implementation. canonical_list_within supplies its existing budget check; canonical_list supplies an Infallible check and destructures an irrefutable Result. No duplicate ordering core, panic, lint allowance or discarded error remains. Two unit guards verify exact ordering across all four kinds, duplicate elimination, milestone project tie-breaks and bounded refusal. |

### Fixed excerpts

FND-001, src/scope.rs:

```rust
        for initiative in &project.initiatives {
            budget.check()?;
            if initiative.id == value
                || initiative.identifier.eq_ignore_ascii_case(value)
                || initiative.name.eq_ignore_ascii_case(value)
            {
                ids.insert(initiative.id.clone());
            }
        }
```

FND-002, src/named_selection.rs:

```rust
fn canonical_list_with<E>(
    members: Vec<NamedMember>,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<Vec<NamedMember>, E> {
    let mut ordered = BTreeSet::new();
    for member in members {
        check()?;
        ordered.insert(member);
    }
    Ok(ordered.into_iter().collect())
}
```

## Current verdict after disposition round 1

**PASS for the correction review** — both findings fixed; no new findings in the bounded fix diff. Root must still run the one final correction aggregate after taking custody of these latest artifacts. This does not claim that the aggregate has already passed.

The same public composition remains available to credential-ready consumers. The helper changes preserve short-circuit membership semantics and milestone whole-name/project-qualified candidate collection. Unbounded callers remain infallible adapters. Final result collection/formatting consumes records already visited under checks; it adds no new matching pass or independent ordering policy. Successful public return still checks the original deadline. This is cooperative deadline enforcement, with no separate guarantee that a primitive comparison/allocation is preemptible. No broadened timeout/error precedence requirement was introduced.

Additional units examined: Budget::any and Budget::contains; collect_named_milestones; canonical_list_with and both adapters; the eight one_project_with_many public tests; the two named_selection unit guards. FR-016-AC-6 was examined for the ordering guard: "A version 3 named selection with team, include, exclude, and a milestone carrying `project_id` round-trips through encode and decode unchanged; the same selection built in any member order encodes to identical bytes in canonical order; a member present in both lists stays in both." The guard supports the ordering part of that criterion, not an assertion that it independently tests the whole round trip.

Inspected actual root logs: tests-only RED 9b802ee3fbc46bccf42a9e0b1635b03a5bcf96e8 compiled and produced exactly eight nested-cost assertion failures (0 passed, 8 failed, 22 filtered), not compilation/fixture failures; its two ordering guards passed. GREEN 0e80afd logs show all eight public tests pass, library172/bounded30/named_selection5/scope6/integration30+1ignored. Root reports fmt exit0 and all-target Clippy exit0; logs show Clippy completion in7.08s and rustdoc completion in2.14s with the requested -D warnings policy. No formatting source change was necessary. Root's actual checks supersede the coder's honest not-run disclaimer. Reviewer ran no Cargo, source mutation, formatting or aggregate. Original PR75 checks remain historical. New checks are recorded in swm178-canonicalization-review-{red,red-ordering,green-focused-tests,green-clippy,green-fmt-check,green-rustdoc}.log outside the tree.
