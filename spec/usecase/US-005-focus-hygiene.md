---
id: US-005
title: Keep an inline focus accurate and see what changed
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-013"
    type: traces_to
---
# US-005: Keep an inline focus accurate and see what changed

## Story

**As a** team planner
**I want** hygiene findings and a diff for my inline focus
**So that** I can correct stale or inconsistent tracking and see what changed
since I last reviewed it.

## Acceptance Examples (Illustrative)

### US-005-EX-1: Find inaccurate focus tracking

- **Given** a focus containing issues without project or milestone, an overdue
  milestone, a linked PR whose state disagrees with its issue, stale work, and
  related issue titles
- **When** the planner runs focus hygiene
- **Then** ix-board lists each evidence-backed finding and leaves source data
  unchanged

### US-005-EX-2: See changes since the prior report

- **Given** a successful report for the same canonical focus
- **When** the planner runs focus hygiene again after issue fields or blocker
  relations change
- **Then** ix-board reports those changes against the stored prior snapshot

### US-005-EX-3: Distinguish unavailable PR checks from clean results

- **Given** repeated links to the same PR and no available GitHub token
- **When** the planner runs focus hygiene
- **Then** the PR check reports one unavailable reason and marks
  the section incomplete instead of listing duplicate reasons or zero findings

## Notes (Informative)

PR state is looked up from a GitHub pull request URL attached to a Linear
issue. Ticket and comment prose is not interpreted as a source of instructions
or PR attribution.
