---
id: SR-018
title: "Team scope code-review"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@aeaf1aec01df8959fe99e34c1d448d0d2777cecc; src/scope.rs, tests/scope.rs, spec/functional/FR-010-explicit-work-scope.md; diff against 3db8ae232b7d328e8f2e4ad97643bd40fcda357a"
review_set: subset
---
## Summary

Reviewed the complete PR diff using repository conventions and the Rust checklist. TeamKey remains typed; missing team selectors return the existing typed selection error. The selector union is intersected with team ownership after descendant traversal. Global ancestry remains available for cycle and missing-parent checks and leaf classification. Whole-team selection includes unprojected tickets. Projects outside the team boundary are pruned; associated team projects with no tasks remain visible. No wire format, cached snapshot, compatibility mechanism, dependency files, or CI gates changed.

## Verdict

**PASS** — no findings in the reviewed diff.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Evidence and limits

Three focused scope tests pass, and full make ci passes with 51 tests and 2 ignored. No applicable AssuranceProfile was found. Installed quire lacks matrix; no strict computed trace matrix result is claimed. Full spec validation retains preexisting FR-004 structural EARS diagnostics, unrelated to this diff; the changed FR-010 and these review artifacts validate separately. Full gates run again before merge.
