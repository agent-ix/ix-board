---
id: SR-001
title: "Code review — ix-board SWM-2 implementation"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@456679207e32d312ca0bb6020e5238327f411836; PR #2 changed files: Cargo.lock, Cargo.toml, README.md, deny.toml, src/cache.rs, src/linear.rs, src/lib.rs, src/main.rs, src/model.rs, tests/integration.rs; acceptance context: spec/spec.md, spec/functional/*.md, spec/non-functional/*.md, spec/usecase/*.md"
review_set: subset
---

## Summary

Reviewed the frozen SWM-2 implementation at `456679207e32d312ca0bb6020e5238327f411836` against its changed code, tests, CI configuration, and accepted specification. Three cache hazards remain; an earlier blocker-edge finding was corrected before this PR head.

## Verdict

**FAIL** — the cache can serve one Linear account's data to another, and its database may expose private tracker records to other local users.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The single workspace cache key can return a prior account's snapshot under different Linear credentials | src/cache.rs:44-47,58 |
| FND-002 | medium | Fresh cached reads still fail when `LINEAR_API_KEY` is unset | src/main.rs:122-125 |
| FND-003 | medium | The SQLite cache does not restrict access to private tracker data | src/cache.rs:39-42 |
| FND-004 | high | Earlier blocker-direction finding; corrected before this reviewed SHA | src/linear.rs:229-238 |

## Prior Review Context

The preliminary working-tree review at base `094933f` found that `A blocks B` had been recorded as `A blocked_by B`. Commit `9ae969e` changed the mapping to put `A` in `B.blocked_by` and added transport-level fixture coverage. That correction is present at the reviewed SHA; FND-004 is historical and is not an open defect at this SHA.

## Dispositions

| FND | outcome | sha/reason |
| --- | --- | --- |
| FND-004 | fixed | 9ae969e |
| FND-001 | fixed | c892b85f76c2122e38a52e3747d30d5aa5da1d75 |
| FND-002 | still-open | The CLI still constructs `LinearGraphQlSource` before opening/reading the cache, so an unexpired cache cannot be used without `LINEAR_API_KEY`. |
| FND-003 | fixed | c892b85f76c2122e38a52e3747d30d5aa5da1d75 |
| FND-002 | accepted-no-change | Requiring credentials establishes the account namespace before reading private cached data; returning the previous account's snapshot without credentials could expose it. |

FND-004 after_excerpt: `blocked_by.entry(relation.related_issue.id.clone()).or_default().insert(relation.issue.id.clone());`
FND-001 after_excerpt: `cache_namespace = format!("linear:sha256:{:x}", Sha256::digest(api_key.as_bytes()));`
FND-003 after_excerpt: `file.set_permissions(std::fs::Permissions::from_mode(0o600))`
FND-002 round-2 reason: `LINEAR_API_KEY` is required to establish which account the cache belongs to; using cached data without credentials could return another account's snapshot. The README documents the credential requirement, so this behavior is accepted for the prerelease.

## Gate Results

- `cargo fmt --all -- --check`: passed (stable rustfmt warned that two configured import options require nightly).
- `cargo clippy --all-targets -- -D warnings`: not run to completion; the sandbox rejected writes to the existing target directory, and the elevated retry was interrupted.
- `cargo test`: not run.
- `cargo deny check`: not run.
