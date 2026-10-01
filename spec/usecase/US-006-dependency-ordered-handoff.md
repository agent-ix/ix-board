---
id: US-006
title: Prepare a dependency-ordered handoff from a focus
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-010"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-011"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-012"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-014"
    type: traces_to
---
# US-006: Prepare a dependency-ordered handoff from a focus

## Story

**As a** team lead
**I want** a bounded handoff chunk ordered by prerequisites within my focus
**So that** I can give the team work in an order that respects dependencies
and understand which items are ready now.

## Acceptance Examples (Illustrative)

### US-006-EX-1: Put prerequisites before blocked follow-on work

- **Given** an inline focus containing ready prerequisite issues and open
  downstream issues that they block
- **When** the team lead requests a plan chunk
- **Then** the prerequisites appear before their downstream focus issues, and
  each row shows its FR-001 readiness result, milestone, unblocks, and bounded
  acceptance excerpt

### US-006-EX-2: Limit the handoff size

- **Given** more open focus issues than the requested chunk size
- **When** the team lead requests `focus plan-chunk` with `--size N`
- **Then** the command returns at most N issue rows from the deterministic
  dependency and milestone ordering

## Notes (Informative)

FR-010 and FR-011 own inline focus selection and expansion. FR-001 owns ready
eligibility; FR-002 owns transitive blocker closure and layer order; FR-012 owns
the focus status and ready view. FR-014 composes these authorities into an
ordered handoff slice. The slice may include open issues that are not ready so
that a prerequisite can be handed off before a blocked downstream issue.
