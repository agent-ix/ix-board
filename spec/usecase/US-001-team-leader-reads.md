---
id: US-001
title: Inspect ready work and project context
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-001"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-002"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-003"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-004"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-005"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-006"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-007"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-015"
    type: traces_to
  - target: "ix://agent-ix/ix-board/NFR-001"
    type: traces_to
---
# US-001: Inspect ready work and project context

## Story

**As a** team leader coordinating work across a team and its projects
**I want** a deterministic view of ready work, project context, and matching
records, with controls for project run state and queue priority
**So that** I can decide what the team should work on using current Linear data.

## Context

Today, the team leader needs one view that explains readiness and blockers,
summarizes project goals and milestones, and finds relevant tickets and projects.
The experience must use structured source data and remain useful on repeated
reads through a warm cache.

## Acceptance Examples (Illustrative)

### US-001-EX-1: Identify the next ready item

- **Given** a running project with tickets at different dependency layers
- **When** the team leader requests the next queue for that team
- **Then** eligible work is ordered by project priority and project manual
  order, then by ticket priority and ticket manual order, with stable project
  and ticket identifiers as final tie-breakers; each row includes its applicable
  spec step and explains its readiness or position

### US-001-EX-2: Inspect a project

- **Given** a project with a goal, milestones, open tickets, and blockers
- **When** the team leader requests its project chunk
- **Then** those items are available in one command result

### US-001-EX-3: Search current records

- **Given** matching ticket and project records
- **When** the team leader searches for a query
- **Then** matching tickets and projects are returned from structured data

### US-001-EX-4: Check a handoff's ticket states in one read

- **Given** a lead has a list of ticket identifiers and one identifier is stale
- **When** the lead reads those issues together
- **Then** the found tickets retain their exact Linear states and the missing
  identifier is called out without hiding the found records

### US-001-EX-5: Detect an entirely stale ticket list in a script

- **Given** every requested issue identifier is unknown to the current snapshot
- **When** a script reads them together
- **Then** ix-board prints the missing identifiers and exits non-zero

## Notes (Informative)

The MVP uses the Linear status, queue, search, and cache defaults stated in
FR-001 through FR-004. Reads use structured fields and do not interpret prose.
