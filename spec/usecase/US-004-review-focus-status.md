---
id: US-004
title: Review status and ready work for a focus
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-010"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-011"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-012"
    type: traces_to
---
# US-004: Review status and ready work for a focus

## Story

**As a** team lead
**I want** to inspect status, blockers, and ready work for an inline focus
**So that** I can see project and milestone progress and choose eligible work.

## Acceptance Examples (Illustrative)

### US-004-EX-1: Check a focus before planning

- **Given** a focus containing projects, milestones, focus issues, and
  cross-team blockers
- **When** the team lead requests focus status
- **Then** ix-board shows project and milestone progress, target dates,
  open issues by state, in-progress issues, each focus issue's blocker states,
  and the focus issues eligible for the ready queue

## Notes (Informative)

FR-010 and FR-011 own focus selection, expansion, and blocker-layer membership.
FR-001 and FR-002 own ready eligibility, ordering, blocker closure, and error
behavior. FR-012 composes those results into one focus status view and defines
the per-project and per-milestone progress presentation.
