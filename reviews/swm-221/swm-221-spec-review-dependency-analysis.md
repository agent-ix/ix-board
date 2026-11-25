---
id: SR-6714
title: "SWM-221 FR-047 dependency analysis"
type: SpecReview
analysis: dependency
scope: "agent-ix/ix-board@7d500013413471112318d4e8b8727089b54d8ed6; spec/functional/FR-047-retro-eval.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-047"
    type: reviews
---

# SR-6714: SWM-221 FR-047 dependency analysis

## Summary

Ticket: SWM-221. Dependency analysis of the FR-047 relationships and Dependencies section.

## Verdict

**CONDITIONAL** - low findings only. The stack itself depends on unmerged #102 and #103 (see the code review, ledger schema finding).

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | Frontmatter relationships omit FR-026 (unavailable semantics, named in Dependencies) and FR-041, which defines the ix_fleet_ready_buffer_deficit series the evaluator queries; ledger views it depends on are on unmerged PRs and no edge or text says which schema version is required. | spec/functional/FR-047-retro-eval.md:4-17 |

## Scope

- FR-047/relationships (examined): spec/functional/FR-047-retro-eval.md
- FR-047/Dependencies (examined): spec/functional/FR-047-retro-eval.md

## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | Relationships now include FR-026 and FR-041, and Dependencies state the ledger schema ownership and accepted versions. |
