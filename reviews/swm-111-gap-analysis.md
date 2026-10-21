---
id: SR-028
title: "Gap analysis — ix-board blocker chain"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@f5c825f32dbe925886f0567422a059e5e1bc24cf; SWM-111 chain behavior; src/chain.rs, src/main.rs, src/linear.rs, tests/fixtures/chain/linear-issues.json"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-002"
    type: reviews
---

# SR-028: ix-board blocker chain gap analysis

## Summary

Ticket: SWM-111. Planless gap analysis of the new `chain` CLI surface, using `quoin matrix`, `quire matrix`, and `quire coverage`. The existing FR-002 requirement covers blocker closure for `project-chunk` but does not own the new cross-team `chain` command or its owner, age, scope, and cycle-reporting behavior. Plan completion: not assessed.

## Verdict

**FAIL** — the new user-visible command has no owning formal requirement, so its behavior has no specification source against which its tests can be traced.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The new user-visible `ix-board chain` command and its cross-team ownership, age, root-blocker, cycle, and output behavior have no owning FR/US in `spec/`; FR-002 only specifies project-chunk blocker layers. | src/main.rs:140-147, src/chain.rs:117-118 |

## Coverage

Plan completion: not assessed.

`quoin matrix --repo . --json` ran with no run evidence recorded for any criterion. `quire matrix --scope . --format json` computed the static matrix: 121 criteria total, 65 tagged, 50 untagged, and 6 `method-without-symbol`. `quire coverage --scope . --json` reported 24 untracked trace-tag symbols and 14 diagnostics. The untagged criteria and untracked symbols are existing repository baseline outside the SWM-111 diff; the reverse-gap finding above is the newly changed command's missing owner. No chain requirement exists to receive test trace tags. Semantic intent review was not requested and was skipped.


## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-002 | high | The new chain requirement reuses FR-019, which already identifies the program-scope-check requirement; matrix trace tags for unrelated program-scope tests now bind to the chain criteria. | spec/functional/FR-019-blocker-chain.md, spec/functional/FR-019-program-scope-check.md, src/program_review.rs:636-712 |
| FND-003 | medium | The new FR artifact fails structural validation because its required Dependencies section is missing. | spec/functional/FR-019-blocker-chain.md |

## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | still-open | A chain requirement file was added, but it reuses the existing FR-019 identity, so it does not provide a unique formal requirement and causes unrelated trace tags to bind to its criteria. |

```yaml
dispositions:
  - fnd: FND-001
    outcome: still-open
    reason: The chain requirement reuses the existing FR-019 identity, so it does not provide a unique formal requirement and causes unrelated trace tags to bind to its criteria.
```

```yaml
new_findings:
  - fnd: FND-002
    severity: high
    confidence: high
    method: gap-analysis
    check: integrity
    artifact_id: spec/functional/FR-019-blocker-chain.md
    related_ids: [FR-019]
    path: spec/functional/FR-019-blocker-chain.md
    lines: "2-3"
    excerpt: |-
      id: FR-019
      title: Expand cross-team blocker chains
    finding: "The new chain requirement reuses FR-019, which already identifies `FR-019-program-scope-check.md`. `quire matrix` now maps existing program-scope tests in `src/program_review.rs` to the new chain AC-1 and AC-2, so the formal requirement identity and its trace evidence are ambiguous."
  - fnd: FND-003
    severity: medium
    confidence: high
    method: gap-analysis
    check: integrity
    artifact_id: spec/functional/FR-019-blocker-chain.md
    related_ids: []
    path: spec/functional/FR-019-blocker-chain.md
    lines: "1-36"
    excerpt: |-
      # FR-019: Expand cross-team blocker chains
    finding: "The owning FR does not satisfy the repository's required FR structure: `quire validate --scope . spec/functional/FR-019-blocker-chain.md` reports the required `Dependencies` section missing."
```


### Round 2

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | FR-022 now owns the chain command under a unique requirement ID, and its changed acceptance criteria are linked to chain tests. |
| FND-002 | fixed | The chain requirement uses FR-022, while FR-019 remains the program-scope-check requirement; quire matrix now binds chain criteria to chain tests only. |
| FND-003 | fixed | The new FR-022 includes the required Dependencies section and validates structurally. |

```yaml
dispositions:
  - fnd: FND-001
    outcome: fixed
    after_excerpt: |-
      # FR-022: Expand cross-team blocker chains

      ## Description

      `ix-board chain <issue>` SHALL resolve an exact Linear issue identifier or ID
  - fnd: FND-002
    outcome: fixed
    after_excerpt: |-
      id: FR-022
      title: Expand cross-team blocker chains
      type: FR
  - fnd: FND-003
    outcome: fixed
    after_excerpt: |-
      ## Dependencies

      - **Upstream**: [FR-005](FR-005-structured-data-boundary.md) structured Linear
        issue records; [FR-019](FR-019-program-scope-check.md) program scope files
        when the `--scope` selector is used.
      - **Downstream**: No dependent requirement is currently declared.
```
