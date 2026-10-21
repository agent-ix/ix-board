---
id: FR-023
title: Report unprojected issues in a program scope
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-008"
    type: implements
---
# FR-023: Report unprojected issues in a program scope

## Description

`ix-board orphans --scope <file>` SHALL read the named program scope and one
Linear workspace snapshot, then report unprojected issues belonging to the
teams associated with projects in that scope. It SHALL separate open issues
from completed issues, exclude canceled and duplicate issues, and return an
error when a named scope project is absent or ambiguous in the snapshot. For
each issue, it SHALL suggest scoped projects by matching the issue title to a
project identifier or name prefix; when no prefix matches, it SHALL suggest
projects associated with the issue's team. Each suggestion SHALL include its
reason and whether its project is associated with a different team. The
command SHALL be read-only.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-023-AC-1 | The command requires a scope file, selects the named projects case-insensitively, reports only unprojected issues for their teams, separates open and completed issues, excludes canceled and duplicate issues, and fails when a scoped project is absent or ambiguous. | Test |
| FR-023-AC-2 | A title-prefix match is preferred over team association; absent a prefix match, suggestions follow team association. Every suggestion includes a reason and a team-mismatch flag. | Test |
| FR-023-AC-3 | Text output presents open and completed groups with suggestions, and JSON exposes the same groups and structured suggestion fields. | Test |

## Dependencies

- The program-scope file format and structured Linear workspace snapshot.
