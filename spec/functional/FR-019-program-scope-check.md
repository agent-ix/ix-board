---
id: FR-019
title: Check a program scope against live catalogs
type: FR
provenance:
  source: SWM-102
---
# FR-019: Check a program scope against live catalogs

## Description

The `scope check` command SHALL validate a TOML program scope file against
structured Linear initiative and project catalogs and GitHub repository
availability. It SHALL report missing initiatives, missing projects, missing
repositories, and projects that are not placed in the declared scope. It SHALL
make no changes to Linear or GitHub.

The scope file SHALL describe program membership and ownership only. It SHALL
not persist counts or other values that can be derived from live source data.
GitHub repository probes SHALL have a bounded request timeout and report a
timeout as an error naming the affected repository.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-019-AC-1 | The scope loader accepts the documented TOML program, initiative, project placement, repository, lane, and out-of-scope fields; rejects unknown fields and invalid or duplicate placements; and does not accept stored tallies. | Test |
| FR-019-AC-2 | Given the scope and live Linear catalogs, the check reports missing initiatives, missing projects, and unplaced projects; given GitHub results, it reports missing repositories and treats an unauthenticated 404 as unverifiable rather than asserting absence. | Test |
| FR-019-AC-3 | Every GitHub repository request has a finite timeout; a timed-out request returns an error identifying the repository and does not produce a successful check report. | Test |
| FR-019-AC-4 | Scope checking is read-only with respect to Linear and GitHub. | Inspection |

## Dependencies

- Structured Linear initiative and project catalog reads.
- GitHub repository API reads.
