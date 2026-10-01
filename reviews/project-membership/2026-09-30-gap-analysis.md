---
id: SR-021
title: "Project membership gap-analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@4585654a05d8384042f914973a4c4f4e9d794492; src/scope.rs, tests/scope.rs, spec/functional/FR-010-explicit-work-scope.md; diff against 2b6ab972a1648d3d1f04fcb60d2cd7e81e633748"
review_set: subset
---
## Summary

FR-010-AC-6 is exercised by a public API fixture containing a parent and child inside the focused project and a grandchild in a different project outside the initiative. Project and initiative selection exclude that grandchild. Adding an explicit ticket root includes it despite overlap with already-selected project tickets. Whole-team scope still includes all owned tickets. Existing team, deduplication, terminal-state and ancestry refusal tests pass.

## Verdict

**PASS** — no findings in the reviewed diff.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Evidence and limits

Four focused scope tests pass. Full gate results are recorded in the PR. No applicable AssuranceProfile exists. Installed quire has no matrix command; no strict computed trace verdict is claimed. Full spec validation retains the preexisting FR-004 structural diagnostics; changed FR-010 and these reviews validate separately. The consumer owns binding and display.
