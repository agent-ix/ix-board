---
id: SR-6080
title: "Code review — SWM-178 resolved-input associations"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; PR 77 src/lib.rs, src/named_selection.rs, tests/bounded_read.rs, docs/named-focus.md; existing ListKind, read_bound, resolver and canonical wire contracts"
review_set: subset
---

## Summary

Ticket: SWM-178. Independent code review with the Rust and docs-style lanes at frozen 55e33220bad48e2f93996bef8a583826762f146f, base 2d8c2a8d6c2720066930355135cf6b1e75cf0a48, PR ix-board#77. Two findings: one duplicate public domain type and one prose-rule violation. The association algorithm preserves each occurrence and uses the existing resolver and deadline correctly.

## Verdict

**FAIL** — FND-001 is high under code-review's mandatory duplication rubric; FND-002 is low. Both belong in the same PR 77. This is a review verdict, not the final aggregate gate.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | NamedList restates the existing public ListKind include/exclude domain as a second incompatible enum. | src/named_selection.rs:272; src/named_focus.rs:65; src/lib.rs:23 |
| FND-002 | low | The label-policy sentence combines SDK behavior and caller conflict policy in more than 30 words, violating docs-style sentence-too-long. | docs/named-focus.md:248 |

## Required corrections

FND-001: `ListKind` already means which selection list a member came from, has exactly `Include` and `Exclude`, and derives every trait required by the new code, including Hash and Ord. Both are root-exported. `ResolvedNamedInput.list` and `MemberReport.list` now identify the same include/exclude concept using incompatible public types. Code-review requires one home for a domain type, and rust-style implementation requires keeping a fact in one place. Reuse the existing ListKind for associations and lookup, tests and docs. If a new public spelling is retained, make it a re-export of the same type, not another enum or a handwritten conversion. Keep existing ListKind API and wire bytes unchanged. The severity comes from the skill's explicit duplication rule, not a claim that current resolution returns the wrong member.

FND-002: before: “The SDK does not truncate them, choose between the labels of aliases of one member, or store them: if two aliases carry different labels, which one to keep (first, last, or both) is your decision, and you have every association to decide with.” Suggested rewrite: “The SDK returns each association with its input and resolved member. Keep your labels alongside the inputs. When aliases have different labels, choose whether to keep the first, last, or all labels.” This preserves caller ownership while applying the literal, one-fact-per-sentence rule. No other docs change is requested.

## Scope examined

```yaml
scope:
  - {id: FR-016-AC-11, path: spec/functional/FR-016-focus-record.md, role: examined, excerpt: "Canonicalizing named string inputs yields canonical IDs and the milestone's owning project ID, and fails with no partial document for a missing, ambiguous, other-team, or project-mismatched member; the stored document contains no label, expanded issue ID, or status."}
  - {id: FR-016-AC-6, path: spec/functional/FR-016-focus-record.md, role: examined, excerpt: "A version 3 named selection with team, include, exclude, and a milestone carrying `project_id` round-trips through encode and decode unchanged; the same selection built in any member order encodes to identical bytes in canonical order; a member present in both lists stays in both."}
  - {id: FR-004-AC-14, path: spec/functional/FR-004-cache-polling.md, role: examined, excerpt: "Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated scheduler and clock-precision tolerance of 250 milliseconds, and never D plus a further legacy request timeout; with a controlled clock the elapsed time is exactly D; no page or nested read begins after D, and each request is issued with a timeout no greater than the time remaining, so a request begun with 5 seconds remaining carries a timeout of at most 5 seconds."}
  - {id: FR-004-AC-16, path: spec/functional/FR-004-cache-polling.md, role: examined, excerpt: "After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry changes neither the cache nor any later result; the fake observes zero requests after the call returns and exactly one attempt per request issued, and the implementation spawns no detached worker."}
  - {id: NamedList, path: src/named_selection.rs, role: examined, excerpt: "pub enum NamedList {"}
  - {id: ListKind, path: src/named_focus.rs, role: examined, excerpt: "pub enum ListKind {"}
  - {id: label-policy-sentence, path: docs/named-focus.md, role: examined, excerpt: "The SDK does not truncate them, choose between the labels of aliases of one member, or store them: if two aliases carry different labels, which one to keep (first, last, or both) is your decision, and you have every association to decide with."}
  - {id: FR-061-revision-labels, path: /Users/peter/dev/.ix-board-lifecycle-delivery/relay-fr061-authphase-accepted.md, role: context_only, excerpt: "Each member keeps a display-name label of at most 64 bytes (cut on a UTF-8 boundary), used only for display."}
```

The FR-004 tags contribute whole-operation evidence; the association tests do not individually exhaust page, transport or post-return assertions. The full unchanged FR-004/FR-016 contracts were read for context; no new spec review campaign was run. Consumer SR-114/FND-002 and the accepted FR-061 label obligation explain the intended API consumer; SDK labels, truncation and alias-conflict policy remain caller-owned.

## Rust lane and behavior evidence

API: the root exports make all new result types and the bounded method externally usable. Public items have documentation. The result preserves (list, zero-based index, exact input, typed canonical member), include before exclude; alias and identical repeated inputs remain separate. The milestone member retains its owner. Capturing by zipping the original lists with unsorted resolved vectors is justified by the private one-for-one Result collection, not guessed canonical positions. All four canonical variants survive unchanged; cross-kind equal IDs remain distinct by typed enum cloning and ordering, although no new collision-specific fixture is claimed.

Implementation: one resolve_members pass serves both old and new APIs; canonical_list_with remains the one canonical ordering algorithm. No new label/wire/schema field, reader, metric engine, persistence, authorization path or thread. Existing method signatures and standalone/unbounded wrappers are preserved. The new service wrapper admits the policy/source before reading, starts the one deadline before cache lookup, uses one snapshot, and passes that deadline through resolution, per-occurrence capture, ordering and final publication. Errors return no result. Association capture checks the budget for each occurrence. No new unsafe, panic, catch-all conversion, error swallowing or lint allowance was found.

Tests: all seven new public-method tests use the real ReadService with fake source/clock/temp cache. The exact per-occurrence and canonical vectors are independently specified; label joins use caller-owned metadata; failure tests check typed variants. The byte-equivalence comparisons share the resolver, so they were assessed together with those independent member oracles and the unchanged wire implementation, not treated as independent correctness proof by themselves. The controlled-clock differential test makes capture checks observable and checks refusal with a deadline that still fits the legacy method. Existing nested-scan tests remain execution evidence for unchanged resolver loops. The source-behavior/cache test changes source availability, not the fixture's resolved IDs; no changed-ID oracle is claimed.

Docs: manually checked the new section against public APIs and the label-join test. The guide's Rust fragments depend on surrounding application variables and are not standalone doctests. No runnable-doctest or rendered-page claim is made. No Python docs lint was run, per the explicit no-Python instruction; FND-002 is a manual rule check. No new figure, layout or navigation structure was introduced.

## Execution and limits

Reviewer ran no Cargo, formatter, runtime tests, source edits, commit, worktree creation, authentication or product command. Root's actual logs were read: swm178-associations-clippy.log finishes successfully (all-targets, -D warnings); focused-tests.log reports library 172, bounded_read 37, named_focus 20, named_selection 5, schema 3 passed; rustdoc.log finishes successfully with -D warnings; fmt-check.log has the existing stable-rustfmt configuration warnings and root records exit 0. Checks ran on the exact formatted source bytes committed as 55e3322; the subsequent formatter commit changes only layout. The final aggregate, cargo-deny and unsafe audit for PR 77 are root-owned and not claimed here. No source mutation/behavioral RED was run. No applicable AssuranceProfile was found under spec/.

Tool provenance: quoin 0.36.7; quire 0.52.0 (engine 0.62.2); spec-artifacts-process@v0.29.2. GitHub confirms agent-ix/ix-board is public and PR 77 is ready at the exact reviewed head. Exact runtime model ID is unavailable to this harness; none is invented. Review run A559CD0A-6F84-4184-90DD-B26406F34287. SR-6080 and SR-6081 were absent from repository/scratchpad review IDs and the SWM-178 tracker markers before allocation. Existing has-review label was present. Artifacts remain outside the frozen worktree for root custody.

## Tracker record

Linear SWM-178 comment 74f30871-2a5e-45ce-871b-f521a8e37d7e. Read-back passed: exact body, parsed YAML, scope and source excerpts, finding IDs/summaries/fields and clean flag. No findings posted to GitHub. Both findings await fixes and this same reviewer's dispositions.

## Dispositions

Round 1 at frozen PR 77 HEAD 9fa5ea1dd087ab1752485dfa0408aeddb3540ad6, compared directly with the reviewed 55e33220bad48e2f93996bef8a583826762f146f. Original findings, scope excerpts and comments above remain unchanged as historical evidence.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | NamedList and its re-export are removed. The association field, lookup argument, capture loop, guide and tests directly reuse the existing ListKind. No alias or conversion was introduced; named_focus.rs is unchanged, preserving the existing ListKind type, wire representation and schema. |
| FND-002 | fixed | The label-policy bullet now uses the suggested short sentences, retaining caller ownership and the first/last/all alias-label decision. |

### Fixed excerpts

FND-001, src/named_selection.rs:280:

```rust
    pub list: ListKind,
```

FND-001, src/named_selection.rs:307:

```rust
    pub fn association(&self, list: ListKind, index: usize) -> Option<&ResolvedNamedInput> {
```

FND-002, docs/named-focus.md:248-250:

```text
- You own the labels. The SDK returns each association with its input and resolved
  member. Keep your labels alongside the inputs. When aliases have different labels,
  choose whether to keep the first, last, or all labels.
```

## Current verdict after disposition round 1

**PASS** — both findings fixed; no new finding in the bounded fix diff. Direct inspection confirms the change removes the duplicate enum and changes its uses to ListKind, with the documentation rewrite. Test value assertions, resolver/capture/deadline behavior and wire code are unchanged. No new test is required for this enum reuse. GitHub confirms the ready PR points to 9fa5ea1dd087ab1752485dfa0408aeddb3540ad6 and the sole worktree is clean.

Root actual check-results and logs were read: format check exit 0 with no diff, Clippy all-targets -D warnings exit 0 (5.24 s), bounded_read 37 and named_focus 20 passing, rustdoc -D warnings exit 0 (1.68 s). Earlier library 172 / named_selection 5 / schema 3 execution evidence applies to the unchanged core; no fresh run of those suites is claimed here. Reviewer ran no Cargo, runtime, source edit, Python, authentication or product command. Root still owns one final aggregate for the final source plus custody commit; no merge/closure or final-gate claim is made.

SR-6081 remains correction-specific PASS with the separately accepted 180 legacy untagged criteria preventing repository-wide assurance. No new matrix or broad semantic/spec review was run for this narrow fix. Exact runtime model ID remains unavailable. Disposition invocation DE04B89F-0AB9-48E0-89A5-6103AA035173.

Disposition tracker comment: 3a5f89d8-8ac7-4475-8c02-4fd1cdfdd779 on SWM-178. Read-back passed: exact body, real YAML parse, both finding IDs and fixed outcomes, non-empty source-verified after_excerpt values, and matching artifact disposition rows. Original review comments were compared byte-for-byte and remain unchanged. No GitHub findings were posted.
