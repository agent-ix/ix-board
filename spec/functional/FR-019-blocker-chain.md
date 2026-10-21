---
id: FR-019
title: Expand cross-team blocker chains
type: FR
---
# FR-019: Expand cross-team blocker chains

## Description

`ix-board chain <issue>` SHALL resolve an exact Linear issue identifier or ID
and expand its transitive blockers across teams. With `--scope <file>` and no
issue argument, it SHALL start from every open issue in the listed projects.
The result SHALL include each discovered issue's state, team, assignee, age,
blockers, and scope lane when available; terminal roots and unowned roots;
distinct paths; detected cycles; and warnings for unavailable blocker records.
The command SHALL provide equivalent human-readable and JSON reports.

For an issue whose blocker list contains both unavailable references and
available references, the report SHALL preserve the path ending at that issue
as partial evidence and continue to expand every available blocker branch.
It SHALL not classify that issue as a root blocker.

Traversal SHALL be iterative and bounded. It SHALL emit at most 2,000 distinct
paths and 2,000 distinct cycles, and inspect at most 100,000 dependency edges
per command. Reaching a bound SHALL add a warning identifying the limited
output; it SHALL never report an incomplete traversal as exhaustive.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-019-AC-1 | An issue selector expands the cross-team transitive blocker graph and returns issue metadata, root blockers, unowned roots, paths, cycles, and warnings in human and JSON output; a scope selector starts from the open issues in its listed projects. | Test |
| FR-019-AC-2 | A complete linear chain yields its ordered path and terminal root; a dependency cycle is reported without recursion failure and is not reported as a root. | Test |
| FR-019-AC-3 | When one issue references an unavailable blocker and one available blocker, output includes a partial path ending at that issue, continues through the available branch, reports the unavailable reference, and does not mark the issue as a root. | Test |
| FR-019-AC-4 | A dense cycle graph produces no more than 2,000 cycles and bounded traversal; output includes a warning when a cycle or traversal limit is reached. | Test |
| FR-019-AC-5 | A long acyclic blocker chain is processed iteratively without exhausting the call stack. | Test |
