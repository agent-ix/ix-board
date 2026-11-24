---
id: US-011
title: Keep every agent lane working and fed from outside any agent session
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-035"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-036"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-037"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-038"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-039"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-040"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-041"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-042"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-043"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-044"
    type: traces_to
---
# US-011: Keep every agent lane working and fed from outside any agent session

## Story

**As a** program owner running agent lanes across several hosts
**I want** a deterministic, evidence-based statement of what every lane is doing,
what it should do next and whether enough ready work exists to keep the hosts busy
**So that** an idle, blocked, dead or unfed lane is found and acted on within
minutes by a process that does not depend on any agent's own judgement, instead
of being discovered hours later.

## Acceptance Examples (Illustrative)

### US-011-EX-1: Find the idle lane

- **Given** a registered lane whose last hook event is two hours old
- **When** a minder runs `ix-board fleet tick --json`
- **Then** the output contains a "push next ticket" action naming that lane and
  the next ticket in its assignment, with the ticket text

### US-011-EX-2: A lane is never silently absent

- **Given** a registered lane on a host whose probe failed
- **When** I run `ix-board fleet status --json`
- **Then** the lane still appears with a class and a source-defect row naming the
  failed probe, and no "all hosts in target" conclusion is possible

### US-011-EX-3: Planning is told it is short

- **Given** four lanes that need eight ready tickets and only three exist
- **When** the tick runs
- **Then** it emits "need 5 more ready tickets" routed to the owning planners
