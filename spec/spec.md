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
not landing. SWM-102 adds program scope validation against live Linear and
GitHub catalogs; SWM-103 adds a live program roadmap with derived project
progress. SWM-104 adds read-only reconciliation between Linear project issue
state and linked GitHub pull requests (FR-021). SWM-112 adds a read-only report
of unprojected issues in a program scope (FR-023). SWM-33 adds inline planner
focus inputs that expand initiatives, projects, and issues into Linear issue
sets and blocker layers. SWM-34 adds focus status, blocker-state, and
ready-queue reporting. SWM-35 adds a focus hygiene report and a comparison
with the last successful report for that focus.

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
- Focus status by project and milestone, blocker-state details, and the ready
  queue restricted to focus issues (FR-012).
- Focus hygiene findings for missing project/milestone assignment, overdue
  milestones, linked GitHub pull request status, stale issues, and duplicate
  title groups, plus a diff from the most recent successful report (FR-013).
- Dependency-ordered handoff chunks from a focus, with per-issue readiness and
  acceptance excerpts (FR-014).
- A versioned JSON focus record with canonical selectors and status facts for
  clients that choose their own storage or transport (FR-016).
- `agents` reports a per-agent activity verdict (working, waiting-stuck, prompt,
  asking, idle, unknown) for the herdr workspaces of named teams on every
  machine, measured in minutes from transcripts, subagents and processes (FR-018).
- `orphans` reports unprojected open and completed issues for the teams
  associated with projects in a program scope, with read-only project
  suggestions (FR-023).
- `drift` reports per-lead scope and churn signals from structured Linear,
  GitHub, herdr, and OpenTelemetry records, with configurable thresholds,
  a stable JSON field contract, and one text line per lead. Its time-based
  decisions require complete event enumeration and contemporaneous queue
  evidence; incomplete reads produce unavailable signal results.
- Program scope files are checked against live Linear and GitHub catalogs, and
  program roadmaps derive project progress from live issue records (FR-019 and
  FR-020).
- `chain` expands cross-team blockers from an issue or all open issues in a
  declared project scope, retaining known paths under partial data and bounding
  cycle and path enumeration (FR-022).
- `reconcile` compares a Linear project's issue state and structured pull
  request attachments with paginated GitHub pull-request and branch records;
  incomplete attachment data suppresses conclusions that require a complete
  attachment inventory (FR-021).

### Out of Scope

- Webhook-based cache refresh; SWM-2 uses polling.
- LLM-based interpretation or actions based on ticket or comment prose.
- Mutating Linear fields other than project status and priority, or mutating
  GitHub data.
- Reading or writing GitHub Issues as part of planner focus resolution.
- Reading or writing GitHub Issues as part of focus hygiene; GitHub is queried
  only for pull requests linked by structured Linear attachments.
- Compatibility layers, legacy readers, or migration shims.
- Copying or vendoring artifacts from another repository.

## System Overview

### System Description

ix-board is a deterministic Linear/GitHub CLI and library. SWM-2 adds fast,
cached Linear reads for a team leader: a reasoned ready queue, a project summary,
and search across tickets and projects. SWM-3 adds narrow project controls:
`Started` runs a project and `Paused` holds it; project priority participates in
the queue order defined by FR-001. SWM-4 adds a read-only drift report for the
configured leads. SWM-102 and SWM-103 add scope validation and live roadmap
reports. SWM-104 adds read-only project-to-pull-request reconciliation through
FR-021. SWM-33 adds inline focus expansion from structured Linear
records; SWM-34 composes that focus with project and milestone status, blocker
states, and the FR-001 ready queue. The library obtains structured source
records through Linear GraphQL and caches read results locally.

### Intended Users

- Team leaders use the CLI and team-leader skill to inspect team and project work.
- Team planners use inline focus selectors to resolve their planned
  initiatives, projects, issues, and blockers.
- Team leads use `focus status` to inspect progress, blockers, and ready work
  within the selected focus.
- Team planners use inline focus selectors and hygiene reports to inspect
  planned work, correct tracking gaps, and see changes since a prior report.
- Team leads use `focus plan-chunk` to hand off an ordered slice of open focus
  issues with their readiness and dependency context.
- Program reviewers use `scope check` to validate a declared program boundary
  and `roadmap` to report initiative and project progress from live records;
  `orphans` reports unprojected issues and scoped project suggestions.
- Library consumers rely on the same deterministic read results and queue rules.

## Requirements Architecture

The use case US-001 traces to FR-001 through FR-007 and NFR-001. US-002 traces
to FR-008 and FR-009, which depend on the queue and structured-data boundary.
US-003 traces to FR-010 and FR-011, reusing FR-001/FR-002 blocker semantics.
US-004 traces to FR-010 through FR-012, composing the existing focus expansion
and queue rules into a status result.
US-005 traces to FR-013, which uses the canonical focus from FR-011 and the
structured-source boundary in FR-005.
US-006 traces to FR-010, FR-011, FR-012, and FR-014, composing focus
membership, blocker layers, and readiness into an ordered handoff slice.
US-007 traces to FR-016, which packages a canonical focus and status for
shared clients without choosing a storage service. US-008 traces to FR-019
and FR-020 for validating a declared program scope and building its live
roadmap. US-001 also traces to FR-021, which reconciles a selected project's
Linear issues and attachments with GitHub pull-request and branch evidence.
FR-023 specifies the read-only orphan report over a declared program scope.
Functional acceptance criteria include fixture checks for each drift signal,
queue rules, and cache behavior; NFR-001 specifies a lightweight warm-cache
fixture benchmark.

## References

- SWM-2, “ix-board: next, project-chunk and search with cache” (Linear).
- SWM-4, “ix-board drift: out-of-scope and churn alerts” (Linear).
- The ix-board README and source repository.
