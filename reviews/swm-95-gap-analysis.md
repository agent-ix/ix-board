---
id: SR-025
title: ix-board agents activity detector gap analysis
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@31eeb4a242162545494da9aa223886ac814d55a0; FR-018 AC-1..AC-7, tests/agents.rs, src/agents/**"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-018"
    type: reviews
---

# SR-025: ix-board agents activity detector gap analysis

## Summary

Ticket: SWM-95. Planless gap analysis of FR-018 against the computed matrix
(`quoin matrix`) and the tagged tests. Plan completion: not assessed.

## Verdict

All seven FR-018 ACs are tagged by tests in `tests/agents.rs`. Each verdict
test drives the real `decide` rule order through `build_report` for both
Claude and Codex fixtures, and the idle threshold is tested at exactly
`--idle-after` and one second below. No tautological verdict assertions.

One high gap: AC-2 and AC-3 claim the child-process path for both adapters,
but the fixture `@@PS` lines are single-spaced, unlike real `ps` output, so the
tests pass while the production path never detects a process (see SR-024
FND-001). AC-5's parser test shares the same unrealistic fixture.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-018-AC-2/AC-3 child-process coverage uses single-spaced `@@PS` fixtures; real `ps` pads columns, so the tested path differs from production and the tests cannot fail on the real defect. | tests/agents.rs:134-137, tests/agents.rs:392-396 |

## Dispositions

| FND | Outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 3e1c077 (`ps` parsing removed; parser test now uses real-shaped tab records including a comm with spaces) |
