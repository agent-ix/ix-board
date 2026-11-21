---
id: US-010
title: Read planning evidence within Linear quota
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-004"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-020"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-031"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-030"
    type: traces_to
provenance:
  source: SWM-184
---
# US-010: Read planning evidence within Linear quota

## Story

**As a** team lead sharing Linear access with other reporting processes
**I want** useful planning evidence with visible freshness and quota availability
**So that** repeated reports remain usable without exhausting our shared access.

## Acceptance Examples (Illustrative)

### US-010-EX-1: Continue during backoff

- **Given** a previous successful report and a Linear cooldown
- **When** another process requests the report
- **Then** it sees the last evidence with its age and cooldown without another Linear request

### US-010-EX-2: Inspect a program economically

- **Given** a program containing many tickets
- **When** I request a roadmap or scope check
- **Then** I receive initiative and project summaries without downloading every ticket

### US-010-EX-3: Require fresh evidence explicitly

- **Given** a named-focus operation requiring a strict freshness bound
- **When** fresh evidence cannot be obtained within its deadline
- **Then** it fails without publishing stale evidence
