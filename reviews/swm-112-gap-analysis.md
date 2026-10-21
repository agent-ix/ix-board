---
id: SR-2028
title: "Gap analysis — ix-board SWM-112 orphans"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; SWM-112 changed behavior, spec/"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/spec/spec"
    type: reviews
---

## Summary

Manually compared the SWM-112 acceptance criteria, new command and report code, fixture tests, and the repository's computed Test Matrix. The command's behavior has no owning formal functional requirement, so the new tests have no criterion to trace to.

## Verdict

**CONDITIONAL** — implementation evidence covers the ticket's acceptance criteria, but formal requirement ownership and criterion-level traceability are missing for this new user-visible command.

## Findings

| ID      | Severity | Summary                                                                 | Refs                                      | Escape Cause        |
| ------- | -------- | ----------------------------------------------------------------------- | ----------------------------------------- | ------------------- |
| FND-001 | medium   | The new `orphans` command has no owning functional requirement or ACs. | src/orphans.rs:56, src/main.rs:621        | missing-requirement |

## Coverage

- Compared all four SWM-112 acceptance bullets with `find_orphans`, CLI dispatch/rendering, and recorded fixture/output tests.
- `quire matrix --scope . --format json` completed over 19 requirement documents. No criterion covers `orphans`; the repository has 50 untagged criteria overall, which include pre-existing gaps outside this PR. The two new `orphans` unit tests have no trace tags because no owning FR/AC exists.
- `quoin gap-analysis` is unavailable as a command in installed Quoin 0.28.3 (`quoin gap-analysis --help` returned “command gap-analysis not found”). Manual acceptance-criteria-to-test/code comparison was used. No repository end-to-end semantic gap-analysis method ran.
- Plan completion: not assessed

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | FR-023 now owns the `orphans` command and its three acceptance criteria are all tagged by tests in the computed matrix. |

FND-001 after_excerpt:

> `ix-board orphans --scope <file>` SHALL read the named program scope and one Linear workspace snapshot, then report unprojected issues belonging to the teams associated with projects in that scope.

