---
id: SR-027
title: "Code review — ix-board blocker chain"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@f5c825f32dbe925886f0567422a059e5e1bc24cf; SWM-111 acceptance; src/chain.rs, src/main.rs, src/linear.rs, src/model.rs, tests/fixtures/chain/linear-issues.json"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-002"
    type: reviews
---

# SR-027: ix-board blocker chain code review

## Summary

Ticket: SWM-111. Reviewed the PR #53 implementation at `f5c825f` against the ticket's four acceptance bullets and the existing FR-002 blocker-closure semantics. The cycle-safe traversal handles the fixture cases, but partial dependency data can hide known branches and dense cycles bypass the path-output bound.

## Verdict

**CONDITIONAL** — two medium findings need disposition before merge.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | When an issue has both a missing blocker reference and known blocker edges, path traversal stops at that issue and omits every known downstream branch; the warning does not restore the missing paths. | src/chain.rs:341-344 |
| FND-002 | medium | The 2,000-path limit does not bound cycle enumeration: a cyclic graph with no sink paths keeps `paths.len()` at zero while the DFS enumerates and stores every simple cycle, allowing dense cycles to consume excessive CPU and memory. | src/chain.rs:333-356 |

## Coverage

Reviewed Rust code and tests for the new command, GraphQL assignee data, scope selection, traversal, human rendering, and JSON serialization. `make ci` first could not create the configured shared Cargo target lock because it is read-only; it was rerun with writable Cargo home and target paths. See gate result in the handoff.


## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | The traversal retains the partial path and continues through available blocker edges. |
| FND-002 | fixed | Cycle output and edge traversal now have independent finite limits with warnings. |

```yaml
dispositions:
  - fnd: FND-001
    outcome: fixed
    after_excerpt: |-
      if unresolved.contains(issue_id) {
          // Keep the known partial route visible while also exploring its known branches.
          paths.insert(traversal.path.clone());
      }
  - fnd: FND-002
    outcome: fixed
    after_excerpt: |-
      const CYCLE_LIMIT: usize = 2_000;
      const TRAVERSAL_STEP_LIMIT: usize = 100_000;
```

## Post-fix code review

At `acda4af19470b139c2ee16bea62ccb97ea5961ed`, rechecked both original findings against the fix commit: FND-001 and FND-002 are fixed. No new code findings were identified. At `cc36aece6b5767fc566833e9eb99e0a3df702728`, the `src/chain.rs` and `src/main.rs` delta only changes trace IDs from FR-019 to FR-022 to match the uniquely numbered chain requirement; the reviewed traversal implementation is unchanged. No additional code-review finding or Linear comment is needed for this head.
