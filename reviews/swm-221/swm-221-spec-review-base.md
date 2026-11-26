---
id: SR-6712
title: "SWM-221 FR-047 base spec review"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@7d500013413471112318d4e8b8727089b54d8ed6; spec/functional/FR-047-retro-eval.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-047"
    type: reviews
---

# SR-6712: SWM-221 FR-047 base spec review

## Summary

Ticket: SWM-221. Base spec review of the new FR-047 against the ticket, dev-team FR-017 and the panel contract. The FR-039 edits in the stacked diff belong to #103 and were reviewed there. Structural integrity was not run as a separate artifact; consistency points are recorded here.

## Verdict

**FAIL** - FR-047 omits the ticket's objective metrics.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | SWM-221 requires objective metrics per session alongside rule rows. FR-047 requires no metric anywhere in behaviour, JSON schema or acceptance criteria, so the ticket criterion can be dropped while every AC passes. | spec/functional/FR-047-retro-eval.md:23-29 |
| FND-002 | medium | AC-6 requires every configured reference to resolve to a registered source adapter, but a catch-all that returns unavailable satisfies its second clause, and no requirement asks for an exact never-producer reason (not_applicable). A test cannot tell an unimplemented rule from a source outage. | spec/functional/FR-047-retro-eval.md:86-86 |
| FND-003 | medium | FR-047 never says what an absent eval.json or absent role_spec_dir does, and introduces role_spec_dir, which dev-team FR-017 does not define; FR-017-AC-6 requires an absent file to evaluate with defaults. The two requirements need one stated behaviour. | spec/functional/FR-047-retro-eval.md:84-84 |
| FND-004 | medium | No runtime, source-request or output-size bound is stated although the consumer allows 5 minutes and 4 MiB and the sources share rate budgets. A command that never finishes meets every acceptance criterion. | spec/functional/FR-047-retro-eval.md:67-75 |
| FND-005 | medium | `deterministic status and evidence follow the query registry` names no registry definition: neither this requirement nor an AC binds each query to the role spec counting rule, unit and window, so a producer that counts the wrong thing passes. | spec/functional/FR-047-retro-eval.md:81-81 |

## Scope

- FR-047-AC-1 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-2 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-3 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-4 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-5 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-6 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-7 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-8 (examined): spec/functional/FR-047-retro-eval.md
- FR-047 (examined): spec/functional/FR-047-retro-eval.md

## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | Behavior and JSON requirements now require `objective_metrics` on every role and session row, including null-value reasons. |
| FND-002 | fixed | Unknown references are explicitly unavailable and the no-pause producer has an exact not-applicable reason. |
| FND-003 | fixed | Missing settings defaults and the role-spec directory default are defined. |
| FND-004 | still-open | `--since` still says “bounded duration” without a maximum, and the spec does not define a response-size bound or total runtime limit. |
| FND-005 | still-open | The requirement refers to a query registry but does not define a spec-level mapping of each query to its counting rule, unit and time window. |

### Round 2

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-004 | still-open | The spec still leaves the accepted duration maximum and total runtime/response bounds unstated. |
| FND-005 | still-open | The spec still lacks a query-by-query method, unit and window registry. |

### Round 3

Reviewed head: `81e2852d82a2c128fdb3e5f8c05fc2a7d58f6811`. No spec changes since round 2.

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-004 | still-open | Duration maximum and total runtime/response bounds remain unspecified. |
| FND-005 | still-open | The query/unit/window registry remains undefined in the spec. |
