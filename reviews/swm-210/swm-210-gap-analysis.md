---
id: SR-6107
title: ix-board agents done-idle, Codex tail read and host isolation gap analysis
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@931a90a88df3055152d8d88bf2c9c1658cc20651; FR-018-AC-27..AC-29, tests/agents.rs, src/agents/**, src/main.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-018"
    type: reviews
---

# SR-6107: ix-board agents done-idle, Codex tail read and host isolation gap analysis

## Summary

Ticket: SWM-210, PR ix-board#87. Planless gap analysis of the new FR-018
criteria, based on the computed matrix (`quire matrix --scope .`) and the
tagged tests. The 257 untagged criteria elsewhere in the repository existed
before this PR and are out of scope.

## Verdict

CONDITIONAL. FR-018-AC-27, FR-018-AC-28 and FR-018-AC-29 are each tagged by
real tests that call the production `decide`, `collect_probe` and
`build_report_with_timeout`. The diff adds no stubs and no code without an
owning requirement. Three gaps remain: the exit-status branch of AC-29 has no
test, the AC-28 tests cover only one window shape, and the AC-27 test does not
check its own preconditions.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | No test covers the "exit status is non-zero" clause of FR-018-AC-29. No output_outcome test uses a report where missing_hosts is set and errors is empty. | src/main.rs:494, tests/agents.rs:1359 |
| FND-002 | medium | The FR-018-AC-28 tests use only a parent rollout whose lifecycle record is in the first window. No test covers a child rollout over 1 MiB or a window that starts mid-turn, which is why SR-6106 FND-001 and FND-002 pass the suite. | tests/agents.rs:1302 |
| FND-003 | low | The FR-018-AC-27 test does not cover its exclusions. A done pane with an active worker or an attributed process should not be IDLE, and that is not tested. | tests/agents.rs:1239 |

## Coverage

- Criteria in scope: 3, tagged 3, untagged 0 (computed matrix).
- Reverse gaps: none. HOST_TIMEOUT, MissingHost and the tail read all trace to FR-018 (Evidence completeness).
- Stubs and coverage inflation: none in the diff.
- Semantic review: skipped. It was not requested by the dispatch.
- Plan completion: not assessed
