---
id: US-009
title: Review whether a team's work is converging
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-024"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-025"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-026"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-027"
    type: traces_to
  - target: "ix://agent-ix/ix-board/FR-028"
    type: traces_to
---
# US-009: Review whether a team's work is converging

## Story

**As a** program reviewer or independent retro agent
**I want** a repeatable, stored measurement of a team's flow, rework, scope
growth and progress toward its gate tickets, with a baseline to compare against
**So that** I can tell work that is landing on the critical path from activity
that only looks busy, and see whether a change in process improved it.

## Acceptance Examples (Illustrative)

### US-009-EX-1: Take a snapshot

- **Given** a retro configuration naming a team, its gate tickets and its repositories
- **When** I run `ix-board retro snapshot --config <file>`
- **Then** ix-board appends one record of the metric catalog to a store outside
  every git work tree, and marks each metric it could not read as unavailable
  with the reason

### US-009-EX-2: Compare with the baseline

- **Given** a store holding a baseline and several snapshots
- **When** I run `ix-board retro delta --config <file>`
- **Then** ix-board shows each metric's change since the previous snapshot and
  since the baseline, and flags a metric that moved the wrong way twice in a row

### US-009-EX-3: Reconstruct a baseline from history

- **Given** a past window with merged pull requests and Linear state history
- **When** I run `ix-board retro backfill --config <file> --since <date>`
- **Then** ix-board appends one backfilled record per day for the metrics that
  history can reproduce and marks the rest unavailable
