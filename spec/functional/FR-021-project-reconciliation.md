---
id: FR-021
title: Reconcile project issues with linked GitHub pull requests
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
---
# FR-021: Reconcile project issues with linked GitHub pull requests

## Description

`ix-board reconcile <project> [--scope <file>] [--stale-days <days>]` SHALL
resolve a Linear project by identifier, UUID, or exact case-insensitive name
and compare its structured issue states and attachment URLs with GitHub pull
request and branch records. ix-board SHALL enumerate GitHub pull-request
records across every page for each searched repository. Reconciliation SHALL
report merged pull requests whose ticket is not terminal, completed tickets
with no merged pull request, stale Coding or In Progress tickets with no
linked pull request or matching branch, stale open pull requests, and open
draft pull requests attached to terminal tickets. A project scope file adds
repositories to the branch search; repositories found in structured
pull-request attachments are also searched.

An issue's Linear attachment list is complete only when its attachments
connection reports no next page. Reconciliation SHALL warn when attachment
data is incomplete. Reconciliation SHALL omit absence-of-PR findings.
Reconciliation SHALL NOT claim that all linked pull requests are merged from
an incomplete attachment list.

The command SHALL render the same five finding groups in terminal and JSON
output. It SHALL use a positive whole-number stale threshold in days, defaulting
to 14. It SHALL perform read-only Linear and GitHub operations.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-021-AC-1 | Given structured Linear issues, pull requests, and branches for a project, reconciliation emits the applicable five finding groups deterministically and resolves project identity without substituting an unknown or ambiguous project. | Test |
| FR-021-AC-2 | When the Linear attachment connection has another page after its 50 returned URLs, the issue is marked incomplete, a warning is emitted, and no all-linked-pull-requests-merged conclusion is emitted; the fixture includes 50 attached merged PRs and an unseen open PR. | Test |
| FR-021-AC-3 | The CLI accepts the project selector, optional scope, and positive stale-day threshold; terminal and JSON output expose the same five finding groups. | Test |

## Dependencies

- **Upstream**: [FR-005](FR-005-structured-data-boundary.md) structured
  Linear records; [FR-010](FR-010-explicit-work-scope.md) program scope files.
- **Downstream**: No dependent requirement is currently declared.
