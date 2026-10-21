---
id: SR-027
title: "Code review — ix-board reconcile SWM-104"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; plugin/skills/ix-board-planning/SKILL.md, src/lib.rs, src/main.rs, src/reconcile.rs, tests/fixtures/reconcile/*"
review_set: subset
---

## Summary

Disposition recheck of SWM-104 at `37cc3184edf6a6445ec54eebf3347830804f3e14` against base `699080d633f58c51c0bfa965d900c3be76ea29f0`, including CLI wiring, reconciliation logic, fixtures, output, and live GitHub evidence. One attachment-completeness edge case can produce a false positive.

## Verdict

**CONDITIONAL** — one medium finding: the all-merged conclusion can be made from incomplete Linear attachment evidence.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | All linked PRs merged is reported from a partial attachment list | src/reconcile.rs:278-280 |

## Verification

- `cargo fmt --check`: passed (stable rustfmt emitted only the repository configuration warnings).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo test -p ix-board`: passed; 136 passed, 2 ignored across lib, CLI, integration, and fixture suites.
- `cargo deny check`: failed on existing locked dependency advisories and source policy (`rustls`/`webpki` advisories and git source `ix-cli-kit`); no dependency or lockfile changes are in this PR.
- `bash scripts/check_unsafe_comments.sh`: passed; 0 unsafe blocks.
- Live `ix-board reconcile "Repeatable Program Review" --json`: passed with all five groups empty and no warnings. The command paginated current PR lists and branch lists across `agent-ix/dev-team`, `agent-ix/ix-board`, `agent-ix/ix-relay`, and `agent-ix/quire-research`; live PR totals were 16, 54, 26, and 11.
- PR #52 exact head is `2864aea6ef591834bd523bcc13e05b82a4e7fa97`; base is `peter/swm-102-103` at `8b83413bde585a7b483e355ed301eac0715f74a3`; GitHub reports OPEN and MERGEABLE.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The merged-PR/open-ticket conclusion now requires `attachments_complete`; the regression fixture verifies 50 returned merged PRs plus an unseen open 51st PR do not produce the all-merged finding. |

After excerpt: `if issue.attachments_complete && !ticket_prs.is_empty() && merged.len() == ticket_prs.len() && !is_terminal(issue.state_type)`
