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

SWM-2 gives team leaders deterministic Linear reads for ready work, project
context, and search through the ix-board CLI and library. SWM-3 adds explicit
project status and priority writes so team leaders can control the run queue.

## Scope

### In Scope

- The `next --team <T>`, `project-chunk <project>`, and `search <query>` read
  commands and their corresponding library behavior.
- Ready-queue eligibility, blocker layers, reasons, and ordering.
- Structured Linear data reads and the SQLite cache with a five-minute poll.
- Fixture-backed checks for queue layering, readiness, and cache invalidation.
- The warm-cache latency target and team-leader skill handoff.
- `project set-status` for `Started`/`Paused` and `project set-priority` for
  supported Linear project priority values; these are ix-board's only writes.

### Out of Scope

- Drift detection and reporting, which is a later feature.
- Webhook-based cache refresh; SWM-2 uses polling.
- LLM-based interpretation or actions based on ticket or comment prose.
- Mutating Linear fields other than project status and priority, or mutating
  GitHub data.
- Compatibility layers, legacy readers, or migration shims.
- Copying or vendoring artifacts from another repository.

## System Overview

### System Description

ix-board is a deterministic Linear/GitHub CLI and library. SWM-2 adds fast,
cached Linear reads for a team leader: a reasoned ready queue, a project summary,
and search across tickets and projects. SWM-3 adds narrow project controls:
`Started` runs a project and `Paused` holds it; project priority participates in
the queue order defined by FR-001. The library obtains structured source
records through Linear GraphQL and caches read results locally.

### Intended Users

- Team leaders use the CLI and team-leader skill to inspect team and project work.
- Library consumers rely on the same deterministic read results and queue rules.

## Requirements Architecture

The use case US-001 traces to FR-001 through FR-007 and NFR-001. Functional
acceptance criteria include fixture checks for queue rules and cache behavior;
NFR-001 specifies a lightweight warm-cache fixture benchmark.

## References

- SWM-2, “ix-board: next, project-chunk and search with cache” (Linear).
- The ix-board README and source repository.
