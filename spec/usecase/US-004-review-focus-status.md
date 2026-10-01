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
  open issues by state and in-progress issues for resolved focus members,
  each focus issue's blocker states with roots attributed to each shared
  blocker, and the focus issues eligible for the ready queue

### US-004-EX-2: Verify the live IR focus against Linear

- **Given** the current live IR focus and a separate expected result obtained
  from an independent `linear api` read
- **When** the delivery acceptance check compares that expected result with
  `focus status`
- **Then** the check confirms that IR-320, IR-321, and IR-322 appear in the
  ready queue and that QSL-20's blockers are listed, and records the query,
  comparison result, verification time, and reviewed commit in the SWM-34
  Linear ticket
- **And** these identifiers denote the ticket's example data at verification
  time; they are neither product-contract identifiers nor permanent fixture
  identifiers

## Notes (Informative)

FR-010 and FR-011 own focus selection, expansion, and blocker-layer membership.
FR-001 and FR-002 own ready eligibility, ordering, blocker closure, and error
behavior. FR-012 composes those results into one focus status view and defines
the per-project and per-milestone progress presentation. The live IR/QSL
comparison is a delivery verification against current independent Linear
records; it does not freeze those issue IDs as product behavior.
