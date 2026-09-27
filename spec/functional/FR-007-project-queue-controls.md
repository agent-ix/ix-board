---
id: FR-007
title: Control project run state and queue priority
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
provenance:
  source: SWM-3
description: The ix-board CLI SHALL update only a Linear project's status or priority, allowing team leaders to pause work and order the ready queue.
dependencies:
  - "ix://agent-ix/ix-board/FR-001"
---
# FR-007: Control project run state and queue priority

## Description

The ix-board CLI SHALL expose two explicit project mutations:

- `project set-status <project> <Started|Paused>` sets the requested project's
  Linear status to `Started` (run) or `Paused` (hold).
- `project set-priority <project> <priority>` sets its Linear project priority
  to a supported Linear priority value.

These are the only Linear mutations ix-board may perform. In particular,
ix-board does not write project manual order, issue state, issue priority,
project metadata, or any other Linear field. Manual order remains an input to
the queue contract in FR-001. A successful command reports the project and the
field value written. An unknown project or invalid status/priority is reported
as an error and does not update another field or project.

## Inputs

- A Linear project identifier.
- Either one of the allowed run statuses (`Started` or `Paused`) or a priority
  value supported by Linear.

## Outputs

- Success: the project identifier and resulting status or priority.
- Error: a clear invalid-input or project-not-found result; no partial or
  unrelated field update.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-007-AC-1 | Given a project in `Started`, setting its status to `Paused` writes only its status, and a subsequent `next` excludes every ticket in that project. | Test |
| FR-007-AC-2 | Given a project in `Paused`, setting its status to `Started` writes only its status, and its otherwise eligible tickets can appear in `next` under FR-001 rules. | Test |
| FR-007-AC-3 | Given projects with different priorities, changing one project's priority changes queue ordering according to FR-001 while preserving its manual order and all ticket fields. | Test |
| FR-007-AC-4 | Given an unknown project, unsupported status, or unsupported priority, the command reports an error and no project field is changed. | Test |
| FR-007-AC-5 | Given any ix-board project mutation, only the requested project's status or priority field is written; no other Linear field is changed. | Inspection |

## Dependencies

- **Upstream**: [FR-001](FR-001-ready-queue.md)
- **Use case**: [US-001](../usecase/US-001-team-leader-reads.md)
