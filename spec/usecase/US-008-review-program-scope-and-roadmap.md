---
id: US-008
title: Review a program scope and live roadmap
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-019"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-020"
    type: traces_to
provenance:
  source: SWM-102, SWM-103
---
# US-008: Review a program scope and live roadmap

## Story

**As a** program reviewer
**I want** to validate the declared program boundary and view project progress
from live Linear and GitHub data
**So that** I can run a repeatable review without maintaining hand-written
scripts or stored tallies.

## Acceptance Examples (Illustrative)

### US-008-EX-1: Check a program boundary

- **Given** a program scope file with initiatives, projects, and repositories
- **When** I run `ix-board scope check`
- **Then** ix-board reports missing and unplaced entries using live catalogs
  and identifies repository checks that could not be verified

### US-008-EX-2: Review live program progress

- **Given** an optional scope file and one complete Linear workspace snapshot
- **When** I run `ix-board roadmap`
- **Then** ix-board groups projects under initiatives and derives project
  progress from the snapshot's issue records
