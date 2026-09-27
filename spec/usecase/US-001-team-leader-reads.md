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
  - target: "ix://agent-ix/ix-board/NFR-001"
    type: traces_to
---
# US-001: Inspect ready work and project context

## Story

**As a** team leader coordinating work across a team and its projects
**I want** a deterministic view of ready work, project context, and matching
records
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
- **Then** eligible work is ordered by its applicable spec step and each row
  explains its readiness or position

### US-001-EX-2: Inspect a project

- **Given** a project with a goal, milestones, open tickets, and blockers
- **When** the team leader requests its project chunk
- **Then** those items are available in one command result

### US-001-EX-3: Search current records

- **Given** matching ticket and project records
- **When** the team leader searches for a query
- **Then** matching tickets and projects are returned from structured data

## Notes (Informative)

Policy details not stated by the request remain open in the master specification:
status-to-step mapping, tie ordering, search fields and ranking, cache scope and
poll-failure behavior, and the latency measurement population.
