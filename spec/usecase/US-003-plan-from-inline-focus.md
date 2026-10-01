---
id: US-003
title: Plan work from an inline focus
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-010"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-011"
    type: traces_to
---
# US-003: Plan work from an inline focus

## Story

**As a** team planner
**I want** to define an inline focus from initiatives, projects, and tickets
and expand it to their issues and blocker layers
**So that** planning queries can use a consistent, current view of the work I
am responsible for.

## Acceptance Examples (Illustrative)

### US-003-EX-1: Expand a mixed focus

- **Given** an initiative, project, and explicit tickets with overlapping issue
  membership
- **When** the planner expands those inline selectors
- **Then** ix-board returns the deduplicated Linear issue set and complete
  blocker layers in deterministic order

## Notes (Informative)

FR-010 owns selector membership and ticket-descendant behavior. FR-001 and
FR-002 own blocker evaluation and blocker-layer traversal semantics. FR-011
combines those existing behaviors into the inline focus-expansion result.
