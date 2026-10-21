---
id: SR-027
title: "Code review — ix-board SWM-102/103"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; src/program_review.rs, src/linear.rs, src/main.rs, tests, plugin/skills/ix-board-planning/SKILL.md; SWM-102 and SWM-103 acceptance criteria"
review_set: subset
---

## Summary

The initial review of head `8b83413bde585a7b483e355ed301eac0715f74a3` found an unbounded GitHub request. On head `459133268c1d3304d3ff92fe1bf8e51084d2deb4`, the request has a 20-second timeout and a timeout test; that test introduces a sleep-based timing risk.

## Verdict

**CONDITIONAL** — FND-001 is fixed; the new timeout test has a medium reliability finding.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | GitHub repository probes have no timeout, so `scope check` can hang indefinitely when `api.github.com` stops responding. | src/program_review.rs:185-203 |


## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-002 | medium | The timeout regression test relies on a 200 ms sleep before asserting a 30 ms request timeout; under scheduler delay the response may be ready before the client handles the expired timer, making the test flaky. | src/program_review.rs:851-868 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The production GitHub client now applies a finite timeout and maps failures to the repository-specific error. |

The code-review finding table above is unchanged from the original review pass.

### FND-001 after excerpt

```rust
let client =
    github_client(GITHUB_REQUEST_TIMEOUT).map_err(|source| ProgramReviewError::Github {
        repository: "<client initialization>".to_owned(),
        source,
    })?;
```
