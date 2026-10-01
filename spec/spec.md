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
SWM-4 reports evidence of work outside that queue and work that is active but
not landing. SWM-33 adds inline planner focus inputs that expand initiatives,
projects, and issues into Linear issue sets and blocker layers.

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
- Inline planner focus inputs that expand selected Linear initiatives,
  projects, and issues into a deduplicated issue set and blocker layers (FR-011).
- `drift` reports per-lead scope and churn signals from structured Linear,
  GitHub, herdr, and OpenTelemetry records, with configurable thresholds,
  a stable JSON field contract, and one text line per lead. Its time-based
  decisions require complete event enumeration and contemporaneous queue
  evidence; incomplete reads produce unavailable signal results.

### Out of Scope

- Webhook-based cache refresh; SWM-2 uses polling.
- LLM-based interpretation or actions based on ticket or comment prose.
- Mutating Linear fields other than project status and priority, or mutating
  GitHub data.
- Reading or writing GitHub Issues as part of planner focus resolution.
- Compatibility layers, legacy readers, or migration shims.
- Copying or vendoring artifacts from another repository.

## System Overview

### System Description

ix-board is a deterministic Linear/GitHub CLI and library. SWM-2 adds fast,
cached Linear reads for a team leader: a reasoned ready queue, a project summary,
and search across tickets and projects. SWM-3 adds narrow project controls:
`Started` runs a project and `Paused` holds it; project priority participates in
the queue order defined by FR-001. SWM-4 adds a read-only drift report for the
configured leads. SWM-33 adds inline focus expansion from structured Linear
records. The library obtains structured source records through Linear GraphQL
and caches read results locally.

### Intended Users

- Team leaders use the CLI and team-leader skill to inspect team and project work.
- Team planners use inline focus selectors to resolve their planned
  initiatives, projects, issues, and blockers.
- Library consumers rely on the same deterministic read results and queue rules.

## Requirements Architecture

The use case US-001 traces to FR-001 through FR-007 and NFR-001. US-002 traces
to FR-008 and FR-009, which depend on the queue and structured-data boundary.
US-003 traces to FR-010 and FR-011, reusing FR-001/FR-002 blocker semantics.
Functional acceptance criteria include fixture checks for each drift signal,
queue rules, and cache behavior; NFR-001 specifies a lightweight warm-cache
fixture benchmark.

## References

- SWM-2, “ix-board: next, project-chunk and search with cache” (Linear).
- SWM-4, “ix-board drift: out-of-scope and churn alerts” (Linear).
- The ix-board README and source repository.
