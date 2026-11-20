---
id: SR-6046
title: "SWM-154 relation pagination code review"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; src/linear.rs, src/lib.rs, tests/fixtures/reconcile/linear-oversubscribed-issue-attachments.json; SWM-154"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-011", type: references}
  - {target: "ix://agent-ix/ix-board/FR-004", type: references}
---

## Summary

Code review with the Rust review lane found no runtime or implementation defect in the bounded change. The separate gap-analysis records rejected trace bindings.

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

PASS for implementation review. Merge still requires gap-analysis findings to be resolved and the lead's final gate.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Evidence

Both connection directions are fully followed before any snapshot is returned. Endpoint IDs, rather than current issue position or direction, determine blocker orientation. BTreeSet deduplicates first-page/continuation and inverse duplicates and orders IDs; existing focus layering orders stable identifiers. A three-issue fixture checks outgoing and incoming continuation edges in an exact prerequisite/hub/dependent closure. A sparse one-issue fixture checks 102 incoming blockers without pretending its absent endpoints form a usable focus snapshot.

The continuation cursor is required only when hasNextPage=true; missing, whitespace-only, repeated, malformed, missing-issue, missing-connection and GraphQL failures propagate before cache replacement. The loop allows 1000 total pages and refuses another page honestly. Existing request HTTP handling propagates network/schema failures through the same request path; no new later-page network fixture was added. Cache v12 changes only the namespaced source key and deliberately refreshes older incomplete relationship data; persisted snapshot shape is unchanged and no migration is needed.

Issue query size remains 50, project size 20; both initial relation directions use ten, continuation queries fifty. Coder-supplied live records show API acceptance but are excluded as approved credential-boundary validation: the lead reports that the user denied the new binary's password prompt and a prior token retry was not authorized. This review made no credential-backed CLI reads or key injection. Fixtures establish query shape, not a numeric complexity cost oracle. Attachments retain their separate completeness flag. No query, cache, or returned result is skipped or truncated by this change. No duplicate implementation, stub, gate weakening, protected lifecycle/schema change, pin, or version edit was found.

## Checks

Independent cached unit-test binary: 22 Linear tests passed, exit 0; cached focus integration binary: 10 tests passed, exit 0; no compilation. make fmt-check passed with existing stable rustfmt warnings about nightly-only import settings. Coder durable logs record make lint and focused integration tests passing. Final aggregate gate and supply-chain checks belong to the lead and were not run here. Optional mutation checks were not performed because compilation is not admitted. Source stayed frozen.

