---
id: FR-020
title: Build a live program roadmap
type: FR
provenance:
  source: SWM-103
---
# FR-020: Build a live program roadmap

## Description

The `roadmap` command SHALL build a deterministic program view from one
structured Linear workspace snapshot, with an optional program scope file.
It SHALL group projects under initiatives, place a shared project once, show
other initiative memberships, identify projects without an initiative, and
identify initiatives without projects. A scoped roadmap SHALL include only
the initiatives and projects declared by that scope.

Each project row SHALL derive open, completed, canceled, progress percentage,
and last activity from current issue records. These values SHALL NOT be read
from or written to a stored tally. Projects and initiatives SHALL have stable,
deterministic ordering.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-020-AC-1 | The roadmap groups projects under their initiative memberships, places a shared project exactly once under its declared primary initiative when scoped (or deterministic primary membership when unscoped), and reports remaining memberships as `also_in`. | Test |
| FR-020-AC-2 | Given a workspace snapshot, project open, completed, canceled, progress percentage, and last activity values are derived from its live issue records; no persisted tally is used. | Test |
| FR-020-AC-3 | Scoped output contains only scope-declared initiatives and projects; the roadmap reports declared initiatives without projects and projects in multiple initiatives, and an unscoped roadmap reports projects with no initiative membership as unplaced. | Test |
| FR-020-AC-4 | Initiative, project, and diagnostic rows have deterministic case-insensitive name ordering for identical input records. | Test |
| FR-020-AC-5 | The roadmap uses a structured Linear snapshot and performs no Linear or GitHub writes. | Inspection |
| FR-020-AC-6 | For scoped output, a project assigned in Linear to a scope-declared initiative but absent from the scope file is reported as missing from scope; each scope-listed project with differing declared and Linear initiative memberships is reported with declared memberships absent from Linear as `missing_from_linear` and Linear memberships absent from the scope file as `missing_from_scope`. | Test |

## Dependencies

- Structured Linear initiative, project, and issue records from one workspace snapshot.
- Optional program scope file conforming to FR-019.
