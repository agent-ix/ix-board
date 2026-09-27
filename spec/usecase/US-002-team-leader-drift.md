---
id: US-002
title: See whether team leads are staying in scope and landing work
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-008"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-009"
    type: traces_to
---
# US-002: See whether team leads are staying in scope and landing work

## Story

**As a** team leader coordinating several leads
**I want** a short, evidence-backed status for each lead
**So that** I can notice work outside the running project queue and work that
is active but not landing.

## Context

The agents share one GitHub account, so account identity cannot establish which
lead owns a PR or commit. A ticket identifier can connect a change to a team
and project. The report needs to make missing attribution visible rather than
guessing a lead.

## Acceptance Examples (Illustrative)

### US-002-EX-1: See work outside the run queue

- **Given** a lead with a team whose running projects have a known order
- **When** a PR references a ticket in a paused project
- **Then** the report identifies the ticket and project as outside that lead's
  running scope

### US-002-EX-2: See activity without delivery

- **Given** a lead with busy activity and no new PR in the reporting window
- **When** the report is requested
- **Then** the lead's line explains the activity and absent PR

### US-002-EX-3: Inspect data in scripts and at a glance

- **Given** the same source events, configuration, and report time
- **When** the report is requested as JSON or text
- **Then** both forms state the same lead status and reasons, while the text
  form uses one line per lead

### US-002-EX-4: Keep historical delivery visible

- **Given** a PR opened before the reporting window and merged inside it
- **When** the report counts landed work
- **Then** the PR contributes to delivery without being treated as a new PR

### US-002-EX-5: Recognize incomplete evidence

- **Given** a missing source page or historical queue state for a Coding move
- **When** the report is requested
- **Then** the affected signal explains that its evidence is incomplete,
  without presenting a clean status based on the partial view

## Notes (Informative)

The shared GitHub account cannot establish which lead owns a PR without a
ticket ID. The report retains the PR as a global unassigned finding with its
details so a team leader can investigate important work rather than lose it.
