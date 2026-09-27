---
type: master-requirements
name: ix-board
org: agent-ix
component_type: rust-cli-library
implementation_language: rust
tags:
  - linear
  - github
  - deterministic-read-tooling
depends_on: []
relationships: []
security_critical: false
---
# Master Requirements Specification

## Purpose

This specification defines the SWM-2 read behavior of ix-board for team leaders
who need a deterministic view of ready work, project context, and search results.
It governs the CLI and library behavior for these reads and the cache that serves
them.

## Scope

### In Scope

- The `next --team <T>`, `project-chunk <project>`, and `search <query>` read
  commands and their corresponding library behavior.
- Ready-queue eligibility, blocker layers, reasons, and ordering.
- Structured Linear data reads and the SQLite cache with a five-minute poll.
- Fixture-backed verification of queue layering, ready-queue rules, and cache
  invalidation.
- The warm-cache latency target and its measurement.
- Team-leader skill guidance that uses these commands and removal of the named
  Python exporter when the implementation is delivered.

### Out of Scope

- Drift detection and reporting, which is a later feature.
- Webhook-based cache refresh; SWM-2 uses polling.
- LLM-based interpretation or actions based on ticket or comment prose.
- Mutating Linear or GitHub data.
- Compatibility layers, legacy readers, or migration shims.
- Copying or vendoring artifacts from another repository.

## System Overview

### System Description

ix-board is a deterministic Linear/GitHub CLI and library. SWM-2 adds fast,
cached Linear reads for a team leader: a reasoned ready queue, a project summary,
and search across tickets and projects. The library obtains structured source
records through Linear GraphQL and caches read results locally.

### Intended Users

- Team leaders use the CLI and team-leader skill to inspect team and project work.
- Library consumers rely on the same deterministic read results and queue rules.

## Requirements Architecture

The use case US-001 traces to the functional requirements FR-001 through FR-006
and the performance constraint NFR-001. IT-001 describes fixture-backed
verification at the Linear read/cache boundary. MP-001 defines the reproducible
warm-cache latency observation for NFR-001.

## Open Questions

- Which exact Linear project status value means “running” for queue eligibility?
- Which spec states map to each “next step,” and what ordering applies when
  several tickets are eligible at the same step?
- What should `next` do when a blocker is outside the requested team or its
  project is not running?
- Does “open ticket” exclude canceled tickets, and which Linear status values
  define open?
- Which structured ticket and project fields are searchable, and how are equal
  matches ordered?
- Does the five-minute poll mean a fixed periodic poll or a maximum cache-entry
  age? What result should commands return after a poll fails?
- What SQLite location and cache scope are required across users, processes, and
  worktrees?
- Which checked-in or installed file is the authoritative team-leader skill, and
  where is the named Python exporter located in the delivery environment?
- What representative team/project size, host, and latency statistic govern the
  one-second warm-cache target?
- Is a 30-run maximum the desired measurement protocol, or should another
  repetition count or summary statistic be used?

## References

- SWM-2, “ix-board: next, project-chunk and search with cache” (Linear).
- The ix-board README and source repository.
