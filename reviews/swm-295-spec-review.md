---
id: SR-6122
title: "Spec review - FR-046 and amended requirements"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@97a7efd7e4e3e3118cb7ab69da2a597484ba860b; spec/spec.md, spec/functional/FR-002, FR-010, FR-011, FR-012, FR-014, FR-020, FR-045, FR-046"
review_set: base
---

# SR-6122: FR-046 and amended requirements

## Summary

Ticket: SWM-295 (also SWM-296). Base review of the spec diff of
agent-ix/ix-board#97. The renumber to FR-046 is complete. Two neighbouring
statements were not reconciled with FR-046. The FR-012 conflict is recorded
once, as a code finding in SR-6120.

## Verdict

**CONDITIONAL** - two low findings.

Sub-analyses (ears-conformance, integrity, dependency and the rest) were not
run as separate documents for this urgent review. `quire validate` on FR-020
and FR-046 reported grammar warnings only, on FR-020.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | FR-014-AC-8 gives layer zero only to a node with no blocker prerequisites, while FR-046 gives an unavailable prerequisite no layer row, so the implementation puts its dependent in layer zero. FR-014-AC-8 and FR-002 layer ordering state no exception. | spec/functional/FR-014-plan-chunk.md:129 |
| FND-002 | low | FR-045 defines activity as events on focus issues; FR-046 limits it to issues of project rows without amending FR-045, so events of outside_projects issues are omitted although FR-045 still requires them. | spec/functional/FR-045-focus-activity.md:43, spec/functional/FR-046-dangling-references.md:53 |
