---
id: SR-6079
title: "Gap analysis — SWM-178 canonicalization correction"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; repository matrix and surface inventory; correction PR 76 src/, tests/bounded_read.rs, docs/named-focus.md; FR-004, FR-010, FR-016"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
---

## Summary

Ticket: SWM-178. Planless, repository-driven matrix and surface audit at 1b0941ae1d758580cadb3151fa11f4bdacb5b314, with detailed reverse-gap/stub/test inspection proportionate to correction PR 76 against accepted 53606425728d94f24d1c5acca9068c76c78be1d2. The correction has a missing adverse oracle for nested scans; the standing 180 legacy untagged criteria remain separately accepted outside this PR.

## Verdict

**FAIL** as a repository-wide assurance claim because the computed matrix still has 180 untagged criteria. The correction-specific test gap is **CONDITIONAL**, FND-001 below, and must be fixed in the same PR. No clean-repository assurance is claimed and no legacy repair campaign is requested.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | Composed deadline tests exercise many outer projects but lack a phase-targeted oracle for large nested initiative, milestone, owner and team-membership scans. | tests/bounded_read.rs:1556; tests/bounded_read.rs:1614; src/scope.rs:72; src/scope.rs:174; src/named_selection.rs:396 |

## Evidence and required correction

many_names(30) gives each project one initiative, one milestone and one team. The heavy/light oracle correctly holds the snapshot/read/decode cost constant, and shows that many-project resolution consumes the remaining budget. The separate five-second read-cost test checks sharing rather than granting resolution a second budget. Those are useful public-API tests, not early-timeout substitutes. However, ManualClock advances only on monotonic_now calls: every unchecked inner traversal consumes zero controlled time. Removing every inner check from a future correct implementation, leaving project-level checks intact, would still satisfy the current fixtures and assertions. The missing checks at SR-6078/FND-001 already demonstrate this blind spot with green root tests.

Add public-method, fake-source/clock adverse tests with fixed outer project/read/cache cost and varied nested cardinality. Place expiry inside initiative lookup and membership, milestone exact/name/qualified lookup and owner validation, and team membership as applicable; show light success, heavy typed timeout, bounded cooperative stopping and no document, on cold and warm paths. The oracle must fail when the intended nested phase stops checking, rather than expiring at an earlier outer/read check. Preserve positive resolution and legacy semantics. No source mutation or behavioral-red run was performed by this reviewer.

## Scope examined

| Unit | Role | Excerpt |
| --- | --- | --- |
| FR-004, Deadline clause | examined | The deadline is the bound on the whole operation, measured from the start below, with no additional allowance of one request timeout. |
| FR-016-AC-11 | examined | Canonicalizing named string inputs yields canonical IDs and the milestone's owning project ID, and fails with no partial document for a missing, ambiguous, other-team, or project-mismatched member; the stored document contains no label, expanded issue ID, or status. |
| FR-010, Qualified milestones | examined | A string input keeps the existing grammar, including slashes inside either name, and canonicalizes to the milestone ID and its owning project ID. |
| FR-004-AC-12 | context_only | Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds or more is refused as an invalid policy with no source read. |
| FR-004-AC-14 | context_only | Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds. |
| FR-004-AC-16 | context_only | After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker. |

New-test FR-016-AC-11 bindings on composed success and failure tests are correct. Deadline tests provide supporting evidence for FR-004's whole-operation obligation; their tags are not a claim that they individually exhaust the request/page-specific AC-14. The three relevant criteria are engine-tagged; no new untracked test tag or unowned behavior was found in the correction.

## Coverage

- Plan completion: not assessed
- Reconciliation: quoin matrix --repo /Users/peter/dev/ix-board-swm-178 --json, quoin 0.36.7, quire 0.52.0 (engine 0.62.2), exit 0. No grep fallback.
- Full computed matrix: 273 criteria: tagged 84, untagged 180, method-without-symbol 9, ignored-only 0, qualified-record 0, retired 0. All 273 report no run evidence in the store. Root test logs are separate execution evidence, not evidence-store bindings.
- quire coverage --scope /Users/peter/dev/ix-board-swm-178 --json exited 0. Its distinct denominator is 272 reference rows (84 backed, 9 no-symbol rows), not the matrix's 273 criteria. Rust binding census: 381 candidates, 319 tagged, 156 bound. Untracked symbols: 0; unmatched tags: 225. These are full-repository baseline measures, not new correction defects.
- Baseline diagnostics: 274 trace-id-outside-marker, two absent optional archetypes (StR/TestMatrix), one missing NFR-001 Acceptance Criteria heading, one uncatalogued NFR benchmark method and one catch-all-universal classification. Coverage stderr additionally reports cfg-alternative symbol ID collisions in cache.rs and lib.rs; these do not concern the changed functions. Exact raw results remain in swm178-canonicalization-{matrix,coverage}.json. Existing 180 gaps remain standing accepted; no new tickets, spec edits or baseline cleanup requested.
- Reverse-gap inventory spans repository public exports, src/main.rs commands, src/ production modules and tests. Existing behavior families map to FR-001 through FR-029 (queue/chunks/search/cache/boundaries/controls/drift/focus/credentials/issues/agents/program scope/roadmap/reconcile/chain/orphans/retro). One new public behavior was added: bounded canonicalization, owned by FR-004 plus FR-016-AC-11 and FR-010 qualification semantics. Four budgeted resolver adapters, the canonicalizer and its team/membership/order helpers are implementation of that existing behavior, not unowned API. No new CLI/config/protocol/store/service was added. The inventory is not a claim that every unchanged historical public symbol received a fresh semantic audit.
- Whole-source/test stub search for todo!, unimplemented!, TODO, FIXME and placeholder found only a test ticket identifier IR-TODO, not a hollow implementation. New source stubs 0; new test stubs 0; new untraced behaviors 0. No coverage percentage or line-coverage completion claim is made.
- Optional whole-repository semantic review: skipped. Targeted requirement/test/code agreement for the changed public composition was explicitly requested and assessed here and in SR-6078; no additional spec-method campaign ran.
- Actual execution evidence and restrictions are recorded in SR-6078: root's focused tests and Clippy/fmt/rustdoc checks passed; reviewer ran no Cargo or final gate. One final correction aggregate remains root-owned after fixes/dispositions. Prior PR 75 is immutable history.

## Dispositions

Round 1 at frozen 0e80afdb43d744b0487b6cddc4f6960ca88b62d2. The original finding and repository baseline verdict above remain unchanged as history.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Eight new public-method tests fix outer project count at one and use exactly the same snapshot payload for light/heavy selectors. They isolate initiative lookup/membership, milestone exact/whole-name/qualified/owner scans and team-known/holds-team-work scans. Each exercises cold and warm paths, positive light success, standalone-equivalent heavy success with generous time, heavy typed timeout/no document at an intermediate budget, and bounded stopping. Root observed exactly eight assertion failures before source fixes and all eight pass afterward; two ordering guards also pass. |

### Fixed excerpt

tests/bounded_read.rs:

```rust
        assert!(
            heavy_used >= light_used + WORK_STEP * (NESTED / 2),
            "{phase} (cache {from_cache}): the nested traversal must cost controlled time, so \
             it checks the deadline inside the list: {heavy_used:?} against {light_used:?}"
        );
```

## Current verdict after disposition round 1

**PASS for the correction-specific gap review** — FND-001 fixed, no new correction findings. **Repository-wide assurance remains FAIL against the unchanged standing 180 legacy untagged criteria**, separately accepted outside this PR. No clean-repository or final aggregate claim is made, and no historical trace repair campaign was run.

The public phase oracle is adverse in the relevant phase, not merely early read expiry: initiative lookup and exact milestone lookup compare no member with a first-record member, whose subsequent membership/owner search terminates immediately; initiative membership and milestone ownership compare first versus last records after identical full lookup passes; whole-name compares exact ID versus name after the common exact pass; qualification adds the slash-qualified pass after common exact/name passes; team-known has no issues so first/last team keys are the only differing scan; holds-team-work makes both teams known from two issues before comparing first/last project team keys. The intermediate deadline therefore falls in the extra nested work. Every new test uses the actual public composed method and same read/cache/decode payload, with no internal-method substitution or new production clock hook. Removing a targeted inner check removes roughly NESTED clock steps and fails its differential cost assertion even if surrounding checks remain. The two unit guards cover exact canonical ordering/deduplication and bounded insertion refusal.

Root RED/GREEN logs were read, not inferred: RED9b802ee compiled, exactly8 nested assertion failures and2 ordering passes; GREEN0e80afd library172/bounded30/named_selection5/scope6/integration30+1ignored, including all eight new cases. Root fmt/Clippy/rustdoc checks passed; reviewer did not execute Cargo or mutate source. No new spec/public/wire/CLI behavior was introduced by the fix; existing FR-004/FR-010/FR-016 ownership remains. No broad fresh matrix or optional whole-repository semantic campaign was rerun for dispositions; historical full matrix baseline remains precisely the earlier recorded snapshot. Plan completion: not assessed.
