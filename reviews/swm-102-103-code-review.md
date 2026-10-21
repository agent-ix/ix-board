---
id: SR-027
title: "Code review — ix-board SWM-102/103"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; src/program_review.rs, src/linear.rs, src/main.rs, tests, plugin/skills/ix-board-planning/SKILL.md; SWM-102 and SWM-103 acceptance criteria"
review_set: subset
---

## Summary

The initial review of head `8b83413bde585a7b483e355ed301eac0715f74a3` found an unbounded GitHub request. On head `ddc9d45`, the 20-second request timeout remains in place and the timeout test now synchronizes request arrival and response release through channels; the focused test passes.

## Verdict

**PASS** — FND-001 and FND-002 are resolved at head `ddc9d45`.

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
| FND-002 | fixed | The server signals after receiving the request and waits on a response channel; the client timeout is then observed through a result channel before the server is released. |

The original code-review finding table above is unchanged. FND-002 was introduced in disposition pass 1.

### FND-001 after excerpt

```rust
let client =
    github_client(GITHUB_REQUEST_TIMEOUT).map_err(|source| ProgramReviewError::Github {
        repository: "<client initialization>".to_owned(),
        source,
    })?;
```

### FND-002 after excerpt

```rust
request_ready_rx
    .recv_timeout(Duration::from_secs(2))
    .expect("server accepted the repository request");
let (diagnostic, timed_out) = result_rx
    .recv_timeout(Duration::from_secs(2))
    .expect("client completed the bounded request");
respond_tx.send(()).unwrap();
```
