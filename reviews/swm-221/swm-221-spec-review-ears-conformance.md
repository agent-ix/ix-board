---
id: SR-6713
title: "SWM-221 FR-047 EARS conformance review"
type: SpecReview
analysis: ears-conformance
scope: "agent-ix/ix-board@7d500013413471112318d4e8b8727089b54d8ed6; spec/functional/FR-047-retro-eval.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-047"
    type: reviews
---

# SR-6713: SWM-221 FR-047 EARS conformance review

## Summary

Ticket: SWM-221. EARS grammar conformance of FR-047 description and behaviour statements.

## Verdict

**CONDITIONAL** - low findings only.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | The description packs five obligations (evaluate, return, show change, read, not infer and not write) into three compound SHALL sentences. | spec/functional/FR-047-retro-eval.md:23-29 |
| FND-002 | low | Behaviour bullets state obligations descriptively (`accepts`, `is`, `are`) without SHALL, and `a bounded duration` names no bound. | spec/functional/FR-047-retro-eval.md:44-47 |

## Scope

- FR-047 (examined): spec/functional/FR-047-retro-eval.md
- FR-047/Behavior (examined): spec/functional/FR-047-retro-eval.md
