---
id: SR-6121
title: "Gap analysis - FR-046 and the FR-020 roadmap additions"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@97a7efd7e4e3e3118cb7ab69da2a597484ba860b; spec/functional/FR-046-dangling-references.md, spec/functional/FR-020-program-roadmap.md, tests/, src/"
review_set: subset
---

# SR-6121: FR-046 and FR-020 additions

## Summary

Ticket: SWM-295 (also SWM-296). Planless gap analysis limited to the PR diff of
agent-ix/ix-board#97. `quire matrix` reports FR-046-AC-1 to AC-8, FR-020-AC-7,
FR-020-AC-8 and FR-002-AC-7 tagged. Two tagged criteria are only partly able
to fail.

## Verdict

**CONDITIONAL** - two low findings; every criterion added by this PR is tagged.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | Nothing tests the CLI wiring of roadmap counts. FR-020-AC-8 tests call `count_from_issues` and `counts_unavailable` directly, so removing the `count_roadmap_from_cache` call, or breaking its no-snapshot and unreadable-snapshot branches, leaves the suite green. | src/main.rs:741, src/main.rs:842-857 |
| FND-002 | low | FR-046-AC-6 requires terminal warnings in sorted order without duplicates, but the terminal test renders one warning and no test builds an input that would yield a duplicate or an out-of-order pair in text output. | src/main.rs:2345, tests/dangling_references.rs:510 |

## Coverage

Plan completion: not assessed

Matrix: `quire matrix --scope . --format json` (engine 0.62.2). FR-010-AC-3,
FR-012-AC-6 and FR-014-AC-6, whose text this PR edits, show untagged because
their tests use unmarked `Trace:` comments, the same form as on main; this is
pre-existing and not raised. Reverse gaps: new public items
(`next_with_warnings`, `cached_workspace_snapshot`, `count_from_issues`,
`counts_unavailable`, `ReferenceWarning`) map to FR-046 or FR-020. No stubs
found. Semantic review: not run as a separate pass (no opt-in); tagged tests
were read against their criteria by hand. Evidence binding (`quoin matrix` run
evidence) is not assessed.
