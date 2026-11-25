---
id: FR-046
title: Tolerate unavailable Linear references in reads
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-004"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-012"
    type: depends_on
provenance:
  source: SWM-295
description: The ix-board library and CLI SHALL complete a read that meets a reference to an unavailable Linear record and SHALL report that reference as a typed warning.
dependencies:
  - FR-001
  - FR-002
  - FR-010
  - FR-011
  - FR-012
  - FR-013
  - FR-014
---
# FR-046: Tolerate unavailable Linear references in reads

## Description

A Linear snapshot can hold an issue that references a record the snapshot does
not hold, for example an archived parent or blocker, or a milestone that its
project no longer lists. Where such a reference exists, `focus status`,
`focus report`, `focus expand`, `focus plan-chunk`, `focus hygiene`,
`project-chunk`, `next`, `select_scope` and the named-selection batch SHALL
complete the read.

ix-board SHALL keep every selected issue in the result and in its leaf counts,
so that counts equal the `select_scope` counts for the same selection.

ix-board SHALL report each unavailable reference as one typed warning of kind
`unavailable_milestone`, `unavailable_project`, `unavailable_parent` or
`unavailable_blocker`. A warning names the human-readable identifier of the
issue that holds the reference and the referenced Linear UUID; a milestone
warning also names the issue's project, or null when the issue has none.
Warnings are sorted and unique. The `warnings` member is omitted from JSON when
it is empty. Terminal output lists them under `Warnings:`.

Each reference degrades as follows.

- **Milestone.** An issue whose milestone its project does not hold is counted
  at project level: in `project_level_issues` of an expansion, in
  `unmilestoned` of a status row, and with a null milestone in a plan chunk.
- **Project.** An issue whose project has no record is not ready, and its plan
  reason names the unavailable project. A status result counts every selected
  issue that no project row holds, whether it references an unavailable project
  or has none, in `outside_projects`, with their recently closed tickets
  (FR-045); the member is omitted when no such issue exists. The activity list
  of FR-045 covers the issues of project rows.
- **Parent.** Ancestry ends at the issue that holds the reference. The issue is
  a leaf when no snapshot issue names it as parent.
- **Blocker.** The unavailable prerequisite is not `Completed`, so every issue
  whose blocker closure reaches it is not ready and, in a named batch, counts as
  blocked. It has no row in any blocker layer; the available prerequisites keep
  their layers.

The `next` command's JSON is a list of ready rows. It SHALL print the warnings
of its candidates to standard error; the library returns them with the queue.

A parent cycle and a dependency cycle are not unavailable references and remain
errors.

The Linear workflow state and team of an issue are required members of the
source record, and each project's initiative links carry the initiative's
identity, so a snapshot cannot hold an unavailable state, team or initiative
reference.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-046-AC-1 | Given an issue whose milestone its project does not hold, `focus expand`, `focus status`, `focus report` and `focus plan-chunk` succeed for a project and for an initiative selection, count the issue at project level with a null plan milestone, report project and total leaf counts equal to `select_scope`, and carry an `unavailable_milestone` warning naming the issue, milestone and project. | Test |
| FR-046-AC-2 | Given an issue that has a milestone and no project, a ticket-selected focus succeeds, counts it in `outside_projects`, and warns with a null project. | Test |
| FR-046-AC-3 | Given selected issues that reference an unavailable project or have no project, status row totals plus `outside_projects` equal the `select_scope` counts and `outside_projects` lists their recently closed tickets; an unavailable project is warned, its issue is not ready with a reason naming the project, and `next` excludes it and returns the warning. | Test |
| FR-046-AC-4 | Given an issue whose parent has no record, `select_scope`, the focus reads and the named batch succeed, count the issue as a leaf, and the reads carry an `unavailable_parent` warning. | Test |
| FR-046-AC-5 | Given a blocker with no record, direct or behind an available blocker, the focus reads, `project-chunk` and the named batch succeed, warn on the issue that holds the reference, report the dependent as not ready and blocked, keep the available blocker layers, and `next` excludes the dependent and returns the warning. | Test |
| FR-046-AC-6 | A result without unavailable references has no `warnings` or `outside_projects` member in its JSON; a result with them lists the warnings in terminal output in sorted order without duplicates. | Test |
| FR-046-AC-7 | `focus hygiene` over a focus with unavailable references succeeds and carries the same warnings. | Test |
| FR-046-AC-8 | A parent cycle and a dependency cycle still return their explicit errors. | Test |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md),
  [FR-002](FR-002-project-chunk.md),
  [FR-010](FR-010-explicit-work-scope.md),
  [FR-011](FR-011-inline-planner-focus.md),
  [FR-012](FR-012-focus-status-and-ready-queue.md),
  [FR-013](FR-013-focus-hygiene-and-diff.md),
  [FR-014](FR-014-plan-chunk.md)
