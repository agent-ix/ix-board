---
id: SR-6047
title: "SWM-154 bounded relation pagination gap analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; relation pagination in src/linear.rs, src/lib.rs and owning FR-011/FR-004 criteria; SWM-154"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-011", type: references}
  - {target: "ix://agent-ix/ix-board/FR-004", type: references}
---

## Summary

The installed matrix successfully extracted the owning criteria but rejected the newly added doc-comment trace claims. Runtime fixtures exercise real HTTP and cache paths, but these criteria have no engine-bound tests. The bounded audit does not assess unrelated repository criteria.

Frozen head: 2e3d4f9e6fcb8b75a3001bcd412f7d1a8c62e030; base: 4cb243b4ee0bb6632a01903ce198e0883ee3d5e4. Ticket: SWM-154. This review is bounded to relation pagination and its owning criteria; it makes no whole-repository assurance claim. No applicable AssuranceProfile was found. Runtime model identity is unavailable to this harness; marker model=unavailable records that limitation.

## Scope

```json
{
  "scope": [
    {
      "id": "FR-004-AC-3",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only",
      "excerpt": "Given an expired entry and a failed fixture refresh, the read returns an error and does not return the stale value."
    },
    {
      "id": "FR-004-AC-7",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "context_only",
      "excerpt": "Project queries use a page size of 20 records."
    },
    {
      "id": "FR-004-AC-8",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Issue queries use a page size of 50 records."
    },
    {
      "id": "FR-011-AC-3",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Given an explicit issue with descendants, the issue and its descendants are included under FR-010 rules; every resolved focus issue roots a complete FR-002 blocker closure, including cross-project, cross-team, completed, and canceled prerequisites. Layers order prerequisites before dependents and entries by stable Linear issue identifier."
    },
    {
      "id": "FR-011-AC-5",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "An incomplete snapshot, missing blocker record, or dependency cycle returns an explicit error and no partial expansion; repeated expansion of identical complete Linear data returns identical output and ordering."
    },
    {
      "id": "FR-011-AC-6",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Recorded Linear project, initiative, milestone, issue, project-milestone association, and relation fixtures produce the expansion without a GitHub Issues request; transport evidence shows separate paginated project and issue queries, client-side relationship joins, and each issued query remains below 10,000 complexity."
    }
  ]
}
```

## Verdict

FAIL: four examined criteria are untagged under the declared model. These are trace-binding findings, not proof of missing runtime tests or a tool failure.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-011-AC-3 has no bound test: the new relation regression uses a /// Trace comment rejected by the active trace model as trace-id-outside-marker; use the existing ix_trace_rs::trace attribute. | src/linear.rs:1928, FR-011-AC-3 |
| FND-002 | high | FR-011-AC-5 has no bound test: the new relation regression uses a /// Trace comment rejected by the active trace model as trace-id-outside-marker; use the existing ix_trace_rs::trace attribute. | src/linear.rs:1928, FR-011-AC-5 |
| FND-003 | high | FR-011-AC-6 has no bound test: the new relation regression uses a /// Trace comment rejected by the active trace model as trace-id-outside-marker; use the existing ix_trace_rs::trace attribute. | src/linear.rs:1928, FR-011-AC-6 |
| FND-004 | high | FR-004-AC-8 has no bound test: the new relation regression uses a /// Trace comment rejected by the active trace model as trace-id-outside-marker; use the existing ix_trace_rs::trace attribute. | src/linear.rs:1928, FR-004-AC-8 |

## Coverage

Reconciliation: quire matrix (quire 0.52.0, engine 0.62.2) — no run evidence read. The active spec-artifacts-process v0.29.2 trace model requires Rust attributes; existing ix-trace-rs v0.1.1 is already a dev-dependency and tests/agents.rs demonstrates canonical usage. No grep fallback was used. Scoped examined criteria: tagged 0, untagged 4, tagged-by-ignored-test 0, method-without-symbol 0, qualified-record 0, retired 0. FR-004-AC-3 and AC-7 were read as context and also report untagged in the full matrix; no new whole-repository claim is made. The captured matrix and coverage JSON retain diagnostics, including unrelated baseline warnings and cfg-specific symbol collisions.

Plan completion: not assessed

Reverse-gap inventory: three changed behaviors (complete relation continuation, directional endpoint join/deduplication, atomic cache refresh/refusal) map to FR-011 and FR-004; zero unowned behaviors and zero source/test stubs found in this bounded surface. Tests execute the real HTTP client and SQLite cache; mocks are only the external HTTP boundary. Sparse fixture endpoints are not asserted as a complete focus result. Stable transitive closure is separately checked using three present issue records. Optional semantic audit was skipped; ordinary code-test alignment was assessed by the Rust code-review lane. No whole-tree semantic or assurance audit is claimed.

## Reproduction

On the current macOS workspace, resolved executable /Users/peter/.nvm/versions/node/v24.15.0/bin/quire reports quire 0.52.0 (engine 0.62.2); quoin at the same bin directory reports 0.36.7. Both static commands exit 0:

- quire matrix --scope /Users/peter/dev/worktrees/ix-board-swm-154 --format json
- quire coverage --scope /Users/peter/dev/worktrees/ix-board-swm-154 --json

Expected bound claims from relation regressions; actual matrix reports status=untagged, binders=[] and unbacked_claims.reason=unmarked-trace-id. Coverage reports src/linear.rs:1930, :1990 and :2057 as trace-id-outside-marker: a comment never binds; write #[trace(...)] on the test instead. Retain doc prose if useful, but add the canonical attributes to tests that actually assert these units. Existing baseline comment-only tags are affected too and are not silently counted as coverage.


## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Round 1: FR-011-AC-3 now has test_function binders at src/linear.rs:1933 and :2060 through canonical trace attributes. |
| FND-002 | fixed | Round 1: FR-011-AC-5 now has test_function binders at src/linear.rs:1933, :1993 and :2060 through canonical trace attributes. |
| FND-003 | fixed | Round 1: FR-011-AC-6 now has test_function binders at src/linear.rs:1933 and :2060 through canonical trace attributes. |
| FND-004 | fixed | Round 1: FR-004-AC-8 now has a test_function binder at src/linear.rs:1933 through the canonical trace attribute. |

## Disposition round 1 evidence

Fix head ce2bee607a957dab38ca0305fd8b56e925cd0927 was independently inspected against original frozen head 2e3d4f9e6fcb8b75a3001bcd412f7d1a8c62e030. Production code and assertions are unchanged. The source diff imports existing ix_trace_rs::trace and converts exactly the three newly added comment-only trace claims to attributes; no dependency, pin, version or schema changes were made. SR-6046 and original SR-6047 were copied into reviews/ without changing their findings.

Independent quire matrix and coverage commands at the fix head both exited 0. All four examined rows now have status=tagged and nonempty test_function binders. The new relation tests have zero coverage diagnostics. Other existing comment-only trace claims still appear as unbacked_claims on these rows; they do not erase the new valid binders and are unchanged baseline context. No fallback, whole-repository PASS, execution-evidence coverage claim or optional semantic audit was used.

The cached Linear test binary ran independently: 22 tests passed, exit 0. No compilation was run. Author format/lint/client-test logs were read as supplied evidence; the lead retains final aggregate gate ownership. No credential-backed CLI reads, retries or key injection occurred.

Current bounded verdict: PASS; all four original findings are fixed, no new findings. Original Summary, Verdict and Findings above remain historical and unedited; these latest dispositions determine current status. SR-6046 remains clean and requires no duplicate disposition comment. GitHub reported the fix head MERGEABLE. Source remained frozen and the worktree clean.

