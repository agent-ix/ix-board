---
id: SR-6086
title: "SWM-184 gap-analysis implementation review"
type: SpecReview
analysis: gap-analysis
review_set: subset
scope: "agent-ix/ix-board; SWM-184; PR80; implementation diff origin/main...8eab311590845aebf0797cf44ea0192321afebd6"
---

## Summary

Ticket: SWM-184. Reviewed frozen implementation head 8eab311590845aebf0797cf44ea0192321afebd6, base ee9aa71. Repository matrix, reverse-gap and stub procedures ran without a plan. No applicable AssuranceProfile. Unchanged specification review is outside this implementation review. Explicit delegated decisions on credential-bound partitions, deadline precedence, and fresh detail JSON compatibility were honored.

## Verdict

FAIL. 11 defects require correction in the same PR. No merge or release performed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-004-AC-20 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/cache_coordination.rs:174-225 |
| FND-002 | high | FR-004-AC-21 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/cache_status.rs:27-67 |
| FND-003 | high | FR-019-AC-5 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/summary_cache.rs:113-150 |
| FND-004 | high | FR-020-AC-2 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/summary_cache.rs:113-150 |
| FND-005 | high | FR-030-AC-1 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/delta_cache.rs:148-290 |
| FND-006 | high | FR-030-AC-2 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/delta_cache.rs:294-397 |
| FND-007 | high | FR-030-AC-3 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/delta_cache.rs:294-397 |
| FND-008 | high | FR-030-AC-4 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/summary_cache.rs:285-418 |
| FND-009 | high | FR-030-AC-5 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior, including a two-retro-worker fixture proving central reuse and metered histories. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | src/retro/live.rs:199-204 |
| FND-010 | high | FR-030-AC-6 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/delta_cache.rs:294-397 |
| FND-011 | high | FR-031-AC-6 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken. | tests/cache_status.rs:27-67 |

## Coverage

Plan completion: not assessed

Reconciliation: quoin matrix (quoin 0.36.7, quire 0.52.0 / engine 0.62.2), corroborated with quire matrix and quire coverage. Whole repository: 290 matrix criteria, 92 tagged, 189 untagged, 9 method-without-symbol. No ignored-only criteria. No run evidence in quoin store; external focused execution logs are recorded separately, not fabricated as engine-bound evidence. Coverage reports zero untracked symbols. Existing spec PR artifact recorded 196 untagged before implementation; current count is 189, a reduction of seven. Findings identify the eleven newly implemented/revised SWM-184 obligations still lacking bindings; remaining historical gaps are retained as repository limitations, outside this ticket's repair scope.

Full source/CLI inventory mapped existing queue, scope/focus, hygiene, drift, program review, project mutations, agents and retro behavior to FR001-031; new cache/summary/transport/settings behavior maps FR004/019/020/030/031. Inventoried 31 requirement-owned behavior families. New untraced behavior families: 0. Confirmed source stubs: 0; test stubs: 0; coverage inflation: 0. Existing ignored subprocess helper and explicit NFR benchmark retain tracked reasons. This is a repository audit with findings bounded for this PR; it does not certify historical repository coverage. Semantic review as a separate optional fanout was not requested and was skipped; ordinary code-review source/test faithfulness checks ran. No plan was selected or authored. Existing full-spec validation failures reported in merged SR6083 concern unchanged FR017 Dependencies and untyped roadmap campaign prose; neither is represented as a new implementation defect.

## Examined scope

```yaml
{
  "clean": false,
  "scope": [
    {
      "id": "FR-001-AC-1",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "Given fixture projects in `Started` and `Paused`, `next` includes eligible tickets only from `Started` projects."
    },
    {
      "id": "FR-001-AC-2",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "Given Linear workflow states with display names `Done`/type `completed` and `Canceled`/type `canceled`, plus other issue states, the first two map to closed `Co"
    },
    {
      "id": "FR-001-AC-3",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "Given fixture tickets in `Backlog`, `Specify`, and `Specced`, their next steps are Specify, Specify, and Coding respectively."
    },
    {
      "id": "FR-001-AC-4",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "A ticket remains excluded while any prerequisite in any recursive `blocked-by` layer, including a cross-team prerequisite, is not `Completed`; a canceled prereq"
    },
    {
      "id": "FR-001-AC-5",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "Eligible tickets are ordered by project priority/manual order, then ticket priority/manual order, with identifiers as final tie-breakers; each row explains its "
    },
    {
      "id": "FR-001-AC-6",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "Given identical structured fixture data, repeated calls return the same rows, order, and reasons."
    },
    {
      "id": "FR-002-AC-1",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given a project fixture containing a goal, milestones, open tickets, and blocker dependencies, one `project-chunk` result contains all four categories; `Complet"
    },
    {
      "id": "FR-002-AC-2",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given a project with no milestones or no open tickets, the result represents the empty category and still returns the other available categories."
    },
    {
      "id": "FR-002-AC-3",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given an unknown project identifier, the command reports not found and does not substitute another project's data."
    },
    {
      "id": "FR-002-AC-4",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given identical structured project input, repeated calls return the same categories and dependency relationships."
    },
    {
      "id": "FR-002-AC-5",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given open issues in the requested project, `blocker_layers` contains their full transitive `blocked_by` closure, including prerequisites in other projects or t"
    },
    {
      "id": "FR-002-AC-6",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given a dependency graph for the closure, layers place prerequisites before dependents and order entries within each layer by stable Linear issue identifier."
    },
    {
      "id": "FR-002-AC-7",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given a `blocked_by` reference in the closure to an issue absent from the structured source data, `project-chunk` reports a missing-dependency error."
    },
    {
      "id": "FR-002-AC-8",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "Given a cycle in the dependency closure, `project-chunk` reports a dependency-cycle error."
    },
    {
      "id": "FR-002-AC-9",
      "path": "spec/functional/FR-002-project-chunk.md",
      "role": "examined",
      "excerpt": "An exact project name resolves case-insensitively to the same chunk as its identifier; duplicate matching names fail explicitly rather than choosing one."
    },
    {
      "id": "FR-003-AC-1",
      "path": "spec/functional/FR-003-search.md",
      "role": "examined",
      "excerpt": "Given a query matching structured project and issue IDs, names, titles, or descriptions with different casing, search returns both record kinds and identifies e"
    },
    {
      "id": "FR-003-AC-2",
      "path": "spec/functional/FR-003-search.md",
      "role": "examined",
      "excerpt": "Given a query with no match, search returns an empty result without fabricating records."
    },
    {
      "id": "FR-003-AC-3",
      "path": "spec/functional/FR-003-search.md",
      "role": "examined",
      "excerpt": "Given a missing or empty query, search reports invalid usage and performs no Linear read."
    },
    {
      "id": "FR-003-AC-4",
      "path": "spec/functional/FR-003-search.md",
      "role": "examined",
      "excerpt": "Given a query that appears only in a comment, search does not return that record."
    },
    {
      "id": "FR-003-AC-5",
      "path": "spec/functional/FR-003-search.md",
      "role": "examined",
      "excerpt": "Given identical structured input and query, repeated searches return identical results ordered by kind and identifier."
    },
    {
      "id": "FR-004-AC-1",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an unexpired cache entry, repeated fixture reads use the cached value and return identical results without another source read."
    },
    {
      "id": "FR-004-AC-2",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an expired entry and a changed fixture source, the next read refreshes and returns the changed value."
    },
    {
      "id": "FR-004-AC-3",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an expired complete entry and a failed or rate-limited fixture refresh, an ordinary read returns that entry labeled stale with its unchanged actual age an"
    },
    {
      "id": "FR-004-AC-4",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given identical cache state and fixture source, repeated reads return identical results."
    },
    {
      "id": "FR-004-AC-5",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given an absolute `XDG_CACHE_HOME` on Unix with `HOME` unset or set, the CLI and `ReadService::open_default` both select `<XDG_CACHE_HOME>/ix-board/linear.sqlit"
    },
    {
      "id": "FR-004-AC-6",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a non-absolute or absent `XDG_CACHE_HOME`, the library falls back to the available home directory; given neither an absolute XDG cache directory nor a hom"
    },
    {
      "id": "FR-004-AC-7",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Project queries use a page size of 20 records."
    },
    {
      "id": "FR-004-AC-8",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Issue queries use a page size of 50 records."
    },
    {
      "id": "FR-004-AC-9",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A structured RATELIMITED response produces a typed LinearRateLimited error."
    },
    {
      "id": "FR-004-AC-10",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A rate-limit error carries the cooldown derived from exhausted quota resets."
    },
    {
      "id": "FR-004-AC-11",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Without a read policy, existing method signatures require no new argument and preserve the 30-second per-request timeout; TTL is 300 seconds by default, explici"
    },
    {
      "id": "FR-004-AC-12",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a bounded read with maximum age 240 seconds, an entry whose source read began 200 seconds earlier is returned with no source read, and an entry whose sour"
    },
    {
      "id": "FR-004-AC-13",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a snapshot whose source read began 250 seconds ago but whose cache write or report occurred 10 seconds ago, a bounded read with maximum age 240 seconds re"
    },
    {
      "id": "FR-004-AC-14",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a fake source whose pages never complete, a bounded read with deadline D returns the typed timeout with an elapsed time of at most D plus a stated schedul"
    },
    {
      "id": "FR-004-AC-15",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a source whose credential resolution takes longer than the deadline before it becomes ready, a bounded read still performs its full budgeted read after re"
    },
    {
      "id": "FR-004-AC-16",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "After a timeout or failed bounded read, the caller receives no snapshot and the cache holds no partial entry as fresh; a fake source that completes after expiry"
    },
    {
      "id": "FR-004-AC-17",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A source that cannot honor a bound returns the explicit unsupported-bound error before any source read; its legacy read method still works unchanged."
    },
    {
      "id": "FR-004-AC-18",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A structured RATELIMITED response during a bounded read returns the existing typed rate-limit error with its cooldown, not a timeout, and the library does not r"
    },
    {
      "id": "FR-004-AC-19",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Given a cache that is locked for longer than the deadline, a bounded read returns the typed timeout within the deadline and does not read the source; a refreshe"
    },
    {
      "id": "FR-004-AC-20",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "Two separately launched processes read the same expired dataset concurrently; exactly one source refresh occurs and the waiter reuses it after rechecking. A hel"
    },
    {
      "id": "FR-004-AC-21",
      "path": "spec/functional/FR-004-cache-polling.md",
      "role": "examined",
      "excerpt": "A summary refresh, failed refresh, or fallback never advances detail freshness; pre-provenance snapshots have unknown age and fail bounded freshness checks. Fal"
    },
    {
      "id": "FR-005-AC-1",
      "path": "spec/functional/FR-005-structured-data-boundary.md",
      "role": "examined",
      "excerpt": "Given ticket or comment prose that requests a different command action, a fixture-backed read produces the same decision as when that prose is absent, if struct"
    },
    {
      "id": "FR-005-AC-2",
      "path": "spec/functional/FR-005-structured-data-boundary.md",
      "role": "examined",
      "excerpt": "Given structured fields that determine readiness and conflicting prose, the command follows the structured fields."
    },
    {
      "id": "FR-005-AC-3",
      "path": "spec/functional/FR-005-structured-data-boundary.md",
      "role": "examined",
      "excerpt": "The read commands complete without invoking an LLM service."
    },
    {
      "id": "FR-006-AC-1",
      "path": "spec/functional/FR-006-team-leader-handoff.md",
      "role": "examined",
      "excerpt": "The team-leader skill names all three commands and describes when to use each."
    },
    {
      "id": "FR-006-AC-2",
      "path": "spec/functional/FR-006-team-leader-handoff.md",
      "role": "examined",
      "excerpt": "The named Python exporter remains until the three replacement commands work; after they work, delivery removes `/home/peter/dev/linear-initiative-export.py`."
    },
    {
      "id": "FR-007-AC-1",
      "path": "spec/functional/FR-007-project-queue-controls.md",
      "role": "examined",
      "excerpt": "Given a project in `Started`, setting its status to `Paused` writes only its status, and a subsequent `next` excludes every ticket in that project."
    },
    {
      "id": "FR-007-AC-2",
      "path": "spec/functional/FR-007-project-queue-controls.md",
      "role": "examined",
      "excerpt": "Given a project in `Paused`, setting its status to `Started` writes only its status, and its otherwise eligible tickets can appear in `next` under FR-001 rules."
    },
    {
      "id": "FR-007-AC-3",
      "path": "spec/functional/FR-007-project-queue-controls.md",
      "role": "examined",
      "excerpt": "Given projects with different priorities, changing one project's priority changes queue ordering according to FR-001 while preserving its manual order and all t"
    },
    {
      "id": "FR-007-AC-4",
      "path": "spec/functional/FR-007-project-queue-controls.md",
      "role": "examined",
      "excerpt": "Given an unknown project, unsupported status, or unsupported priority, the command reports an error and no project field is changed."
    },
    {
      "id": "FR-007-AC-5",
      "path": "spec/functional/FR-007-project-queue-controls.md",
      "role": "examined",
      "excerpt": "Given any ix-board project mutation, only the requested project's status or priority field is written; no other Linear field is changed."
    },
    {
      "id": "FR-007-AC-6",
      "path": "spec/functional/FR-007-project-queue-controls.md",
      "role": "examined",
      "excerpt": "Given `project set-priority` values outside the exact tokens `0`, `1`, `2`, `3`, and `4` (including negative, greater-than-four, decimal, or named values), the "
    },
    {
      "id": "FR-008-AC-1",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Fixture PRs with one ticket ID in branch or title classify in scope for a running project and out of scope for a paused or other non-running project; conflictin"
    },
    {
      "id": "FR-008-AC-2",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Fixture Coding transitions into a non-running project or ahead of a higher-ranked unblocked ticket yield veering findings naming the ticket and skipped ticket w"
    },
    {
      "id": "FR-008-AC-3",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "A fixture ticket in Coding for longer than `stall_hours` with no ticket-attributed commit or PR in the trailing `stall_hours` yields a stall; an attributed comm"
    },
    {
      "id": "FR-008-AC-4",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Fixture herdr busy state or positive OTel spend at or above the configured threshold with no attributed new PR yields busy-without-PR; an attributed new PR prev"
    },
    {
      "id": "FR-008-AC-5",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Fixture creation, Completed transition, and merged-PR events produce correct per-lead counts and net ticket growth, with repeated event records deduplicated by "
    },
    {
      "id": "FR-008-AC-6",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Missing historical project status/priority/order, ticket state/spec state/priority/order, or blocker evidence marks the veering decision unavailable with the mi"
    },
    {
      "id": "FR-008-AC-7",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Given identical source records, configuration, and report time, repeated reads return the same signal groups and ordering without invoking an LLM or writing sou"
    },
    {
      "id": "FR-008-AC-8",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "A fixture omitting a GitHub PR/commit or Linear issue/history page or partition marks affected signals unavailable, retains any proven finding, and never yields"
    },
    {
      "id": "FR-008-AC-9",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Missing herdr, OTel, GitHub, or Linear observations mark each affected signal unavailable rather than replacing missing values with zero; one observed true busy"
    },
    {
      "id": "FR-008-AC-10",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "A configuration fixture accepts a project, milestone, or explicit issue-ID goal with a positive integer minimum and positive finite no-progress threshold; it re"
    },
    {
      "id": "FR-008-AC-11",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "A fixture with two active goal issues assigned to the same Linear assignee and a third unassigned issue reports one active worker; an open blocker assigned to a"
    },
    {
      "id": "FR-008-AC-12",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "A fixture with a Completed transition and a later state transition reports both timestamps and computes elapsed hours from the later transition; a linked commit"
    },
    {
      "id": "FR-008-AC-13",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "The QSL L2 replay at `2026-10-05T18:00:00Z` with a 12-hour threshold and minimum of one reports 13 hours since the 05:00Z goal transition, preserves the 06:00Z "
    },
    {
      "id": "FR-008-AC-14",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "A fixture with complete history groups all advancing issues from other projects by team and project in `crowding_out`, including all ticket IDs across teams; cr"
    },
    {
      "id": "FR-008-AC-15",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "Missing current issue/assignee, goal-membership, blocker, or relevant history pages make dependent counts/timestamps unavailable and produce explicit missing-so"
    },
    {
      "id": "FR-008-AC-16",
      "path": "spec/functional/FR-008-drift-signals.md",
      "role": "examined",
      "excerpt": "With complete evidence showing every goal issue is terminal, the report has no staffing-shortfall finding and returns `on_track`; with an open goal issue, the c"
    },
    {
      "id": "FR-009-AC-1",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "An out-of-scope PR or a queue-veering fixture produces `out_of_scope` for its attributed lead, with the source ticket/PR and scope or skipped-ticket reason."
    },
    {
      "id": "FR-009-AC-2",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "A Coding stall, busy-without-PR finding, or net growth above the configured limit with merged PR count below its configured minimum produces `churning`; boundar"
    },
    {
      "id": "FR-009-AC-3",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "Valid threshold changes alter only the corresponding alert decision; `spend_threshold = 0` accepts positive observed spend but not observed zero, while invalid "
    },
    {
      "id": "FR-009-AC-4",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "A fixture with no alerts and complete signals yields `on_track`; one with missing required evidence and no proven alert yields `unknown`; a proven alert yields "
    },
    {
      "id": "FR-009-AC-5",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "The same multi-lead fixture renders valid JSON and exactly one text line per lead, in configured order, with matching status and alert reasons; JSON assertions "
    },
    {
      "id": "FR-009-AC-6",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "Repeated output from identical fixture data, configuration, and report time is byte-for-byte stable for each format."
    },
    {
      "id": "FR-009-AC-7",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "A fixture with an incomplete PR or Linear history page marks dependent counts `null` and the signal unavailable; it cannot yield `on_track` or an absent alert f"
    },
    {
      "id": "FR-009-AC-8",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "A priority-goal fixture serializes every required JSON field with the specified type, enum, ordering, and nullability; `active_workers` is a distinct-assignee c"
    },
    {
      "id": "FR-009-AC-9",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "With complete evidence and no recorded completion or transition, both completion/transition timestamps are `null` and hours use the earliest known goal-issue cr"
    },
    {
      "id": "FR-009-AC-10",
      "path": "spec/functional/FR-009-drift-alerts-output.md",
      "role": "examined",
      "excerpt": "The QSL L2 fixture renders the 12-hour threshold, 13 hours since progress, two distinct active assignees against the configured minimum of one, the latest compl"
    },
    {
      "id": "FR-010-AC-1",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Initiative, project and ticket selectors resolve from structured relationships; overlapping selectors count a ticket only once."
    },
    {
      "id": "FR-010-AC-2",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Progress excludes parents, canceled and duplicate leaves and groups remaining leaves by workflow stage."
    },
    {
      "id": "FR-010-AC-3",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Missing or ambiguous selectors, cyclic ancestry and missing ancestors return explicit errors."
    },
    {
      "id": "FR-010-AC-4",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Transport retains initiative identities, parent IDs and workflow categories; cached reads preserve actual fetch age and forced refresh replaces it."
    },
    {
      "id": "FR-010-AC-5",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "A team boundary selects only tickets owned by that team, including unprojected tickets when no narrower selectors exist; project, initiative and ticket selector"
    },
    {
      "id": "FR-010-AC-6",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Project and initiative membership follows assigned project IDs without expanding into descendants in other projects. Explicit ticket selection includes descenda"
    },
    {
      "id": "FR-010-AC-7",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "A Linear initiative identifier resolves to the same project and issue set as its UUID or exact name; missing and ambiguous selectors still fail explicitly."
    },
    {
      "id": "FR-010-AC-8",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "A milestone UUID, unique name, or project-qualified name selects only its assigned issues, contributes its containing project for reporting, and unions without "
    },
    {
      "id": "FR-010-AC-9",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Given a named selection including initiative I (projects P1 and P2) and excluding P2, the effective leaves are P1's; a P1 leaf excluded by ticket is absent; a P"
    },
    {
      "id": "FR-010-AC-10",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Excluding an initiative removes the leaves of its current projects; excluding a project removes its assigned leaves; excluding a milestone removes only its assi"
    },
    {
      "id": "FR-010-AC-11",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Given selections A (includes P) and B (includes P, excludes ticket T in P) of one team, A keeps T, B does not, and the combined set contains T once; given A (in"
    },
    {
      "id": "FR-010-AC-12",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Given a parent with two children, excluding one child leaves the parent a non-leaf and uncounted; excluding the parent while including both children counts neit"
    },
    {
      "id": "FR-010-AC-13",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "A named selection with an empty include list returns the explicit-empty state with an empty issue set, project set, and leaf set, with and without exclusions an"
    },
    {
      "id": "FR-010-AC-14",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "Across two snapshots in which project P2 joins and then leaves initiative I, the same selection keeps unchanged bytes and its effective set follows each snapsho"
    },
    {
      "id": "FR-010-AC-15",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "A milestone member whose recorded project owns it selects its assigned issues; the same milestone ID recorded with another project reports `ProjectMismatch` and"
    },
    {
      "id": "FR-010-AC-16",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "For include and exclude members that are resolved, proven absent by enumeration evidence, a ticket of another team, a milestone with the wrong project, and not "
    },
    {
      "id": "FR-010-AC-17",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "An initiative with no project association in the snapshot and no catalog evidence is `Unavailable`, not `Missing`; a team that appears in no project or issue gi"
    },
    {
      "id": "FR-010-AC-18",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "An unresolved include member leaves its selection incomplete and contributes no leaves; an unresolved exclude member removes nothing, is reported, and leaves it"
    },
    {
      "id": "FR-010-AC-19",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "A selection of team X that includes an initiative holding X and Y tasks counts only X's leaves and reports the team-boundary reason; a ticket member of team Y i"
    },
    {
      "id": "FR-010-AC-20",
      "path": "spec/functional/FR-010-explicit-work-scope.md",
      "role": "examined",
      "excerpt": "All existing FR-010 fixtures for `ScopeSelection` and `FocusSelection` (AC-1 through AC-8) return identical results and errors after the named selection exists."
    },
    {
      "id": "FR-011-AC-1",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Given combined inline initiative, project, and issue selectors, expansion follows FR-010 membership rules, returns the union once per issue, and distinguishes f"
    },
    {
      "id": "FR-011-AC-2",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Given an initiative with assigned projects, each project's milestones and issues assigned to those projects are present; issues assigned to a milestone are grou"
    },
    {
      "id": "FR-011-AC-3",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Given an explicit issue with descendants, the issue and its descendants are included under FR-010 rules; every resolved focus issue roots a complete FR-002 bloc"
    },
    {
      "id": "FR-011-AC-4",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Empty selector input or any missing or ambiguous selector produces an explicit error and no partial expansion."
    },
    {
      "id": "FR-011-AC-5",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "An incomplete snapshot, missing blocker record, or dependency cycle returns an explicit error and no partial expansion; repeated expansion of identical complete"
    },
    {
      "id": "FR-011-AC-6",
      "path": "spec/functional/FR-011-inline-planner-focus.md",
      "role": "examined",
      "excerpt": "Recorded Linear project, initiative, milestone, issue, project-milestone association, and relation fixtures produce the expansion without a GitHub Issues reques"
    },
    {
      "id": "FR-012-AC-1",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given selected projects and milestones, `focus status` reports each group's target date, open issue counts grouped by workflow-state display name, and in-progre"
    },
    {
      "id": "FR-012-AC-2",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given project and milestone issue trees with completed, open, canceled, duplicate, and parent issues, progress uses FR-010 leaf classification, reports done/tot"
    },
    {
      "id": "FR-012-AC-3",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given two focus issues with a shared transitive blocker and other blockers across projects and teams in open, completed, and canceled states, aggregate layers i"
    },
    {
      "id": "FR-012-AC-4",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given focus issues with different project, spec, and blocker eligibility, the ready queue includes exactly the focus issues eligible under FR-001, applies FR-00"
    },
    {
      "id": "FR-012-AC-5",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given identical complete Linear records, repeated `focus status` calls return identical project and milestone groups, issue lists, blocker layers, ready queue o"
    },
    {
      "id": "FR-012-AC-6",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "An empty focus, incomplete source snapshot, missing blocker record, or dependency cycle returns an explicit error and no partial status result."
    },
    {
      "id": "FR-012-AC-7",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Project focus issues without a milestone appear in `unmilestoned` with open-state, in-progress, and leaf-progress facts; parent issues contribute to open-state "
    },
    {
      "id": "FR-012-AC-8",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "A work-milestone selector keeps full project and milestone progress but restricts blocker roots and ready issues to matching milestones, preserving cross-milest"
    },
    {
      "id": "FR-012-AC-9",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "An empty filtered ready queue reports that it is empty and counts otherwise-ready issues excluded by the selected milestones."
    },
    {
      "id": "FR-012-AC-10",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "A project reached only through milestone or issue focus is marked `partial_focus`, shows only selected or member-bearing milestones, and labels its project tota"
    },
    {
      "id": "FR-012-AC-11",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given three named selections of two teams and one snapshot, one batch call returns each selection's result with its caller key, one combined result per team, an"
    },
    {
      "id": "FR-012-AC-12",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given a selection with open, completed, canceled, duplicate and parent issues, the per-selection done, total, remaining-by-stage, ready, and blocked counts equa"
    },
    {
      "id": "FR-012-AC-13",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given two selections of one team that share project P and one leaf placed under two initiatives, the combined result counts each leaf once, each per-selection r"
    },
    {
      "id": "FR-012-AC-14",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given selections of teams X and Y, the results form separate groups and no total, union, or readiness spans teams."
    },
    {
      "id": "FR-012-AC-15",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Partial reasons are reported on initiative rows and project rows for a milestone-only inclusion, a ticket-only inclusion, an exclusion, and a team boundary that"
    },
    {
      "id": "FR-012-AC-16",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given an included leaf whose only open prerequisite is excluded by the selection, the leaf is not ready and is counted blocked, the prerequisite appears in the "
    },
    {
      "id": "FR-012-AC-17",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "An explicit empty selection returns state `empty` with no rows and a valid zero leaf count; a batch with zero selections returns `none` with no totals at all; a"
    },
    {
      "id": "FR-012-AC-18",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "A project whose native status category is `Unknown` is reported with that classification and its row is not reported as complete or healthy; the existing status"
    },
    {
      "id": "FR-012-AC-19",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "A parent cycle, a missing parent or blocker record, a dependency cycle, or an incomplete snapshot fails the whole batch with the existing explicit error and no "
    },
    {
      "id": "FR-012-AC-20",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Inspection confirms the batch evaluation reuses the FR-010 resolvers and leaf classification, the FR-001 readiness evaluation, the FR-002 closure, and the exist"
    },
    {
      "id": "FR-012-AC-21",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given a batch of team X with a `complete` selection A and a selection B whose exclude member is unresolved, team X's union is `incomplete`, is not reported comp"
    },
    {
      "id": "FR-012-AC-22",
      "path": "spec/functional/FR-012-focus-status-and-ready-queue.md",
      "role": "examined",
      "excerpt": "Given a selection that includes only project P directly and no initiative, the result contains a standalone project row for P with initiative-link status `none "
    },
    {
      "id": "FR-013-AC-1",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "The report identifies each focus issue with a missing project, missing milestone, or both, and lists the missing fields; completed and canceled issues remain el"
    },
    {
      "id": "FR-013-AC-2",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "A milestone with a target date before report date and at least one open focus issue appears as overdue with its open issues; a milestone due today, with no targ"
    },
    {
      "id": "FR-013-AC-3",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "Linked GitHub PR fixtures produce `done_with_open_pr` and `open_with_merged_pr` as specified; closed-unmerged PRs do not; multiple links remain individually att"
    },
    {
      "id": "FR-013-AC-4",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "An open issue last updated strictly more than 30 days before report time is stale by default, and one at the exact 30-day boundary is not; a positive CLI overri"
    },
    {
      "id": "FR-013-AC-5",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "The cited IR-387..IR-407 title family forms one near-duplicate connected component under the normative token rule, including “Mark hash/digest/pin antipattern a"
    },
    {
      "id": "FR-013-AC-6",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "A prior successful report for the same workspace and canonical focus yields changes for differing state, project, milestone, and blocker fields with old/current"
    },
    {
      "id": "FR-013-AC-7",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "No prior snapshot yields an unavailable diff with `no_prior_report`; incomplete Linear reads, failed diff construction, and unreadable/insufficient snapshots do"
    },
    {
      "id": "FR-013-AC-8",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "Identical complete records, focus, configuration, and report time yield stable JSON and text findings and ordering; incomplete source evidence marks the affecte"
    },
    {
      "id": "FR-013-AC-9",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "When a previously focused issue moves to a project outside the current focus, the diff still reports its `left_focus` membership change and old/current project "
    },
    {
      "id": "FR-013-AC-10",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "The inverted index generates each unique issue pair within the resolved focus that shares at least one normalized content token, counts shared tokens, excludes "
    },
    {
      "id": "FR-013-AC-11",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "A fixture of 448 focused titles each normalizing to the same single content token generates 100,128 all-candidate pairs and returns `resource_limit` before titl"
    },
    {
      "id": "FR-013-AC-12",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "JSON `issue_refs` contains identifier and title for available issues referenced by scope, overdue, PR, stale, duplicate, and diff findings while retaining their"
    },
    {
      "id": "FR-013-AC-13",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "Repeated links to one pull request share one lookup and one unavailable reason; terminal headings state whether each check was complete and how many findings it"
    },
    {
      "id": "FR-013-AC-14",
      "path": "spec/functional/FR-013-focus-hygiene-and-diff.md",
      "role": "examined",
      "excerpt": "An initiative identifier such as `I-10` selects the same hygiene focus, findings, and canonical baseline key as its UUID or exact name."
    },
    {
      "id": "FR-014-AC-1",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "The returned chunk includes only open focus issues and orders them by the complete tuple of dependency layer, FR-001 project order, milestone order within the p"
    },
    {
      "id": "FR-014-AC-2",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "The handoff candidate set includes every open focus issue, including issues that are ready, blocked, in a paused project, in Coding or Review, or have an unmapp"
    },
    {
      "id": "FR-014-AC-3",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given `--size N`, exactly the first min(N, candidate-count) issue rows appear after deterministic ordering; blocker-only nodes and completed/canceled focus issu"
    },
    {
      "id": "FR-014-AC-4",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given a focus prerequisite with multiple transitive open focus dependents, its `unblocks` lists each dependent once in stable identifier order; unrelated and bl"
    },
    {
      "id": "FR-014-AC-5",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given descriptions with a matching Acceptance Criteria Markdown heading, the excerpt is the trimmed section body through the next heading, limited to 280 Unicod"
    },
    {
      "id": "FR-014-AC-6",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given identical complete Linear records and size, repeated calls return identical rows, readiness reasons, unblocks, excerpts, ordering, and source fetch timest"
    },
    {
      "id": "FR-014-AC-7",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "The operation uses only structured Linear reads and makes no GitHub Issues request or mutation."
    },
    {
      "id": "FR-014-AC-8",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given one focus issue with prerequisite paths of unequal lengths in its complete blocker closure and another focus issue on a shorter path, each node receives l"
    },
    {
      "id": "FR-014-AC-9",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given unmilestoned and milestone-assigned open focus issues across projects, dependency layer sorts first, FR-001 project priority/manual order/project identifi"
    },
    {
      "id": "FR-014-AC-10",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "Given a work-milestone filter, the chunk includes selected open work and open focus-member prerequisites from its blocker closure, excludes unrelated later work"
    },
    {
      "id": "FR-014-AC-11",
      "path": "spec/functional/FR-014-plan-chunk.md",
      "role": "examined",
      "excerpt": "An open `Todo` issue with a running project and completed blockers remains in the handoff with `ready=false`; its reasons call it open, identify that Todo is ou"
    },
    {
      "id": "FR-015-AC-1",
      "path": "spec/functional/FR-015-protected-credential-value.md",
      "role": "examined",
      "excerpt": "A synthetic credential is retained as SecretValue; its Debug representation is redacted and does not contain the fixture text."
    },
    {
      "id": "FR-015-AC-2",
      "path": "spec/functional/FR-015-protected-credential-value.md",
      "role": "examined",
      "excerpt": "The existing injected HTTP request receives the unchanged fixture Authorization header and the existing cache namespace remains stable."
    },
    {
      "id": "FR-015-AC-3",
      "path": "spec/functional/FR-015-protected-credential-value.md",
      "role": "examined",
      "excerpt": "Empty and whitespace-only constructor inputs still return MissingApiKey."
    },
    {
      "id": "FR-016-AC-1",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "A name-based selection produces canonical ID selectors, a version, a report timestamp, and the same status facts as `focus status` over that snapshot."
    },
    {
      "id": "FR-016-AC-2",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "`FocusRecord` has an exportable structural JSON Schema."
    },
    {
      "id": "FR-016-AC-3",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "An empty or unresolved selection fails without a partial record."
    },
    {
      "id": "FR-016-AC-4",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "Version 2 encodes each focus member as `{kind,id}`; repeated reads of the same snapshot and selection produce the same ordered member list and issue set. Qualif"
    },
    {
      "id": "FR-016-AC-5",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "After the named selection exists, `focus report` for an inline selection still emits version 2 with the same members and bytes, an empty or unresolved inline se"
    },
    {
      "id": "FR-016-AC-6",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "A version 3 named selection with team, include, exclude, and a milestone carrying `project_id` round-trips through encode and decode unchanged; the same selecti"
    },
    {
      "id": "FR-016-AC-7",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "A version 3 document with `include: []` decodes as the explicit empty selection; a document with `include` or `exclude` or `team` missing, a milestone without `"
    },
    {
      "id": "FR-016-AC-8",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "A version 2 `FocusRecord` presented to the named-selection decoder is refused as unsupported, naming versions 2 and 3, with no team, exclusion, or intent suppli"
    },
    {
      "id": "FR-016-AC-9",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "An unsupported version is reported as a different typed error from an invalid document of version 3, and the version is checked before the other members, so a v"
    },
    {
      "id": "FR-016-AC-10",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "The named selection has an exportable structural JSON Schema in which `team`, `include`, and `exclude` are required, a milestone member requires `project_id`, m"
    },
    {
      "id": "FR-016-AC-11",
      "path": "spec/functional/FR-016-focus-record.md",
      "role": "examined",
      "excerpt": "Canonicalizing named string inputs yields canonical IDs and the milestone's owning project ID, and fails with no partial document for a missing, ambiguous, othe"
    },
    {
      "id": "FR-017-AC-1",
      "path": "spec/functional/FR-017-batch-issue-read.md",
      "role": "examined",
      "excerpt": "A batch of identifiers reads one workspace snapshot and returns exact issues in request order with duplicates removed."
    },
    {
      "id": "FR-017-AC-2",
      "path": "spec/functional/FR-017-batch-issue-read.md",
      "role": "examined",
      "excerpt": "Unknown selectors are reported once in request order while found issues are returned; an ambiguous selector fails without partial output."
    },
    {
      "id": "FR-017-AC-3",
      "path": "spec/functional/FR-017-batch-issue-read.md",
      "role": "examined",
      "excerpt": "Text shows exact state names and missing selectors; JSON retains the structured issue records and completeness flag."
    },
    {
      "id": "FR-017-AC-4",
      "path": "spec/functional/FR-017-batch-issue-read.md",
      "role": "examined",
      "excerpt": "When every selector is missing, the CLI emits the missing list and exits non-zero; a partial result exits successfully."
    },
    {
      "id": "FR-018-AC-1",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "One invocation returns one row per agent pane across local/enabled machines, matching named workspaces case-insensitively; missing teams and unreadable machines"
    },
    {
      "id": "FR-018-AC-2",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Claude real-record fixtures exercise all eight verdicts and precedence; ended execution younger than idle-after is RECENT, established quiet exactly at the thre"
    },
    {
      "id": "FR-018-AC-3",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Codex real-record fixtures exercise the same verdicts, threshold and precedence, deriving direct and nested parentage from structured records rather than rollou"
    },
    {
      "id": "FR-018-AC-4",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Missing adapters/transcripts retain herdr, tail and available attributed-command evidence, mark partial, expose availability reasons and leave unprovable counts"
    },
    {
      "id": "FR-018-AC-5",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Injectable machine-read fixtures prove local and remote host-clock ages and identical typed protocol results without network access."
    },
    {
      "id": "FR-018-AC-6",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "IDLE rows include the next ready ticket when readable; an unreadable queue is reported once without changing verdicts."
    },
    {
      "id": "FR-018-AC-7",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Text and versioned JSON report the same rows/counts, including null; idle-after accepts positive s/m/h values and rejects unsupported, zero or negative values."
    },
    {
      "id": "FR-018-AC-8",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The captured eight-file topology (two active direct, two completed direct, two completed nested fork, two stopped direct) yields active_direct_workers=2, active"
    },
    {
      "id": "FR-018-AC-9",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Completion and user cancellation immediately remove a worker from active counts despite a fresh transcript mtime; an assigned running/waiting worker remains act"
    },
    {
      "id": "FR-018-AC-10",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "A completed/stopped worker stays terminal after reopen or incidental writes, restarts after a confirmed later assignment/start, and an end_turn alone does not c"
    },
    {
      "id": "FR-018-AC-11",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Claude fixtures reconcile finished/stoppedByUser/parentAgentId/spawnDepth: both terminal flags mean stopped, resume alone does not reopen finished work, parentU"
    },
    {
      "id": "FR-018-AC-12",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Codex fixtures distinguish task/turn completion/interruption, successful follow-ups and reopening; list_agents snapshots require matching call_id, string/object"
    },
    {
      "id": "FR-018-AC-13",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "One active direct worker and one active depth-two worker produce separate counts of one each; internal/approval sessions add zero, and duplicate identical ident"
    },
    {
      "id": "FR-018-AC-14",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Ambiguous ancestry, conflicting duplicate identities, unknown role/state, missing discovered children and unsupported lifecycle formats yield explicit uncertain"
    },
    {
      "id": "FR-018-AC-15",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Normalized output includes identity, immediate parent, role/state, structured activity, transition/observation timestamps and provenance, including excluded rec"
    },
    {
      "id": "FR-018-AC-16",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "A fake third harness adapter produces lifecycle observations and correct counts/verdicts using the unchanged shared classifier."
    },
    {
      "id": "FR-018-AC-17",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "WAITING is emitted for a lead awaiting active workers and for an inactive lead with active direct workers; neither state becomes WORKING merely because worker f"
    },
    {
      "id": "FR-018-AC-18",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "WAITING-STUCK requires a confirmed pending wait, complete evidence of no viable worker/executing work and 15 minutes without progress; 14m59s, quiet active work"
    },
    {
      "id": "FR-018-AC-19",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Recent transcript/git writes or old nonterminal records without positive current execution never yield WORKING; source sample time/current-state uncertainty is "
    },
    {
      "id": "FR-018-AC-20",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Quiet discovered children are enumerated without an mtime cutoff; a changing file is retried once, repeated change yields uncertainty, and enumeration/read/stat"
    },
    {
      "id": "FR-018-AC-21",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Missing/incompatible remote probe versions, malformed/truncated protocol responses, permission failures, unavailable platform sources and a 60-second timeout re"
    },
    {
      "id": "FR-018-AC-22",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Skewed host clocks and collection start/end fixtures keep documented ages in the owning host's clock domain and expose inconsistent/unknown timing instead of fa"
    },
    {
      "id": "FR-018-AC-23",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Schema/examples expose schema/protocol versions, nullable counts, deprecated live_subagents equality including null, normalized workers, recent activity and per"
    },
    {
      "id": "FR-018-AC-24",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Retro working-share counts only WORKING, becomes unavailable for unresolved relevant rows/population, and changes query provenance so comparison with the previo"
    },
    {
      "id": "FR-018-AC-25",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Minder fixtures observe WAITING/RECENT without nudging, accept null corrected counts/alias, and preserve failed/partial command status, errors and coverage."
    },
    {
      "id": "FR-018-AC-26",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "macOS and Linux smoke runs demonstrate the shared Rust probe and supported lifecycle collection; inaccessible remote/platform sources are recorded as unavailabl"
    },
    {
      "id": "FR-019-AC-1",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "The scope loader accepts the documented TOML program, initiative, project placement, repository, lane, and out-of-scope fields; rejects unknown fields and inval"
    },
    {
      "id": "FR-019-AC-2",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Given the scope and live Linear catalogs, the check reports missing initiatives, missing projects, and unplaced projects; given GitHub results, it reports missi"
    },
    {
      "id": "FR-019-AC-3",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Every GitHub repository request has a finite timeout; a timed-out request returns an error identifying the repository and does not produce a successful check re"
    },
    {
      "id": "FR-019-AC-4",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Scope checking is read-only with respect to Linear and GitHub."
    },
    {
      "id": "FR-019-AC-5",
      "path": "spec/functional/FR-019-program-scope-check.md",
      "role": "examined",
      "excerpt": "Cold and warm scope checks fetch no issue/history pages, reuse summary catalogs, and expose stale/age/cooldown metadata when using a last complete catalog."
    },
    {
      "id": "FR-020-AC-1",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "The roadmap groups projects under their initiative memberships, places a shared project exactly once under its declared primary initiative when scoped (or deter"
    },
    {
      "id": "FR-020-AC-2",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "A cold roadmap performs no issue/history enumeration, reports native progress with `linear_native` provenance and observation time, and distinguishes it from ti"
    },
    {
      "id": "FR-020-AC-3",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "Scoped output contains only scope-declared initiatives and projects; the roadmap reports declared initiatives without projects and projects in multiple initiati"
    },
    {
      "id": "FR-020-AC-4",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "Initiative, project, and diagnostic rows, including each `also_in` membership list, are ordered case-insensitively by name, with original spelling as a determin"
    },
    {
      "id": "FR-020-AC-5",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "The roadmap uses a structured Linear snapshot and performs no Linear or GitHub writes."
    },
    {
      "id": "FR-020-AC-6",
      "path": "spec/functional/FR-020-program-roadmap.md",
      "role": "examined",
      "excerpt": "A scoped roadmap reports scope-declared initiatives and projects absent from Linear in `missing_scope_initiatives` and `missing_scope_projects`; reports Linear "
    },
    {
      "id": "FR-021-AC-1",
      "path": "spec/functional/FR-021-project-reconciliation.md",
      "role": "examined",
      "excerpt": "Given structured Linear issues, pull requests, and branches for a project, reconciliation emits the applicable five finding groups deterministically and resolve"
    },
    {
      "id": "FR-021-AC-2",
      "path": "spec/functional/FR-021-project-reconciliation.md",
      "role": "examined",
      "excerpt": "When the Linear attachment connection has another page after its 50 returned URLs, the issue is marked incomplete, a warning is emitted, and no all-linked-pull-"
    },
    {
      "id": "FR-021-AC-3",
      "path": "spec/functional/FR-021-project-reconciliation.md",
      "role": "examined",
      "excerpt": "The CLI accepts the project selector, optional scope, and positive stale-day threshold; terminal and JSON output expose the same five finding groups."
    },
    {
      "id": "FR-022-AC-1",
      "path": "spec/functional/FR-022-blocker-chain.md",
      "role": "examined",
      "excerpt": "An issue selector expands the cross-team transitive blocker graph and returns issue metadata, root blockers, unowned roots, paths, cycles, and warnings in human"
    },
    {
      "id": "FR-022-AC-2",
      "path": "spec/functional/FR-022-blocker-chain.md",
      "role": "examined",
      "excerpt": "A complete linear chain yields its ordered path and terminal root; a dependency cycle is reported without recursion failure and is not reported as a root."
    },
    {
      "id": "FR-022-AC-3",
      "path": "spec/functional/FR-022-blocker-chain.md",
      "role": "examined",
      "excerpt": "When one issue references an unavailable blocker and one available blocker, output includes a partial path ending at that issue, continues through the available"
    },
    {
      "id": "FR-022-AC-4",
      "path": "spec/functional/FR-022-blocker-chain.md",
      "role": "examined",
      "excerpt": "A dense cycle graph produces no more than 2,000 cycles and bounded traversal; output includes a warning when a cycle or traversal limit is reached."
    },
    {
      "id": "FR-022-AC-5",
      "path": "spec/functional/FR-022-blocker-chain.md",
      "role": "examined",
      "excerpt": "A long acyclic blocker chain is processed iteratively without exhausting the call stack."
    },
    {
      "id": "FR-023-AC-1",
      "path": "spec/functional/FR-023-unprojected-issues.md",
      "role": "examined",
      "excerpt": "The command requires a scope file, selects the named projects case-insensitively, reports only unprojected issues for their teams, separates open and completed "
    },
    {
      "id": "FR-023-AC-2",
      "path": "spec/functional/FR-023-unprojected-issues.md",
      "role": "examined",
      "excerpt": "A title-prefix match is preferred over team association; absent a prefix match, suggestions follow team association. Every suggestion includes a reason and a te"
    },
    {
      "id": "FR-023-AC-3",
      "path": "spec/functional/FR-023-unprojected-issues.md",
      "role": "examined",
      "excerpt": "Text output presents open and completed groups with suggestions, and JSON exposes the same groups and structured suggestion fields."
    },
    {
      "id": "FR-024-AC-1",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "Given fixture Linear, GitHub and herdr records, the command appends exactly one record containing every catalog id, with the fixture's expected value or unavail"
    },
    {
      "id": "FR-024-AC-2",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`path_remaining` equals the open transitive blockers of the gate tickets and excludes issues that block no gate; it is `unavailable` when the blocker expansion "
    },
    {
      "id": "FR-024-AC-3",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`net_closed_minus_new` is negative for a fixture window with more new issues than completions, counts a completed path issue once, and `intake_new` counts only "
    },
    {
      "id": "FR-024-AC-4",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`prs_merged_per_hour` equals merged count divided by window hours, and `pr_cycle_median_hours` and `pr_cycle_p90_hours` match the nearest-rank values for a fixt"
    },
    {
      "id": "FR-024-AC-5",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`prs_open`, `prs_idle`, `prs_oversize` and `prs_ready_unmerged` match the fixture for pull requests that straddle each threshold, a draft, and an approved and a"
    },
    {
      "id": "FR-024-AC-6",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`fix_rounds_max` and `fix_rounds_total` count only pull requests that are open or were merged inside the window, and are computed from marker first lines only: "
    },
    {
      "id": "FR-024-AC-7",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`head_force_pushes_max` (force-push events only, so a pull request pushed once counts zero), `done_reopened` and `main_red_incidents` match the fixture's timeli"
    },
    {
      "id": "FR-024-AC-8",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`issues_without_project`, `issues_without_milestone`, `in_progress_stale` and `source_count_disagreement` match the fixture, and a stale issue with a recently u"
    },
    {
      "id": "FR-024-AC-9",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`agents_working_share` and `agents_idle_minutes_max` match the fixture rows, and an empty agent list yields `unavailable` with reason `empty population`."
    },
    {
      "id": "FR-024-AC-10",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`worktrees_prunable` and `disk_bytes` match a fixture directory with a prunable worktree, and are `unavailable` with reason `not configured` when `local_paths` "
    },
    {
      "id": "FR-024-AC-11",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`full_gates_max`, `repins_total`, `build_lock_wait_minutes`, `orphan_slots`, `messages_undelivered`, `decisions_open` and `decision_oldest_hours` match the fixt"
    },
    {
      "id": "FR-024-AC-15",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`main_red_incidents` and `main_red_minutes` count a fail-then-pass sequence across three commits inside the window as one incident with the minutes between the "
    },
    {
      "id": "FR-024-AC-16",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`gates_remaining` counts gate tickets not completed, and `path_closed_unverified` counts a path issue completed inside the window with no merged pull request at"
    },
    {
      "id": "FR-024-AC-17",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "A configuration file that is missing, unreadable, not valid JSON, or lacks `team`, `gate_tickets`, `projects` or `repos` stops the command with a non-zero exit "
    },
    {
      "id": "FR-024-AC-18",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "A missing threshold makes only the metrics that use it `unavailable` with reason `not configured`, and `slot_process` without `orphan_slot_hours`, or `relay_lea"
    },
    {
      "id": "FR-024-AC-12",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "The command makes no Linear mutation, no GitHub write and no repository write; test source doubles that record writes see none."
    },
    {
      "id": "FR-024-AC-13",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "Each source is read with one paginated pass per run, and a source read that does not reach its last page makes dependent metrics unavailable ([FR-026](./FR-026-"
    },
    {
      "id": "FR-024-AC-14",
      "path": "spec/functional/FR-024-retro-snapshot.md",
      "role": "examined",
      "excerpt": "`--json` output exposes every metric with id, value, unit, direction, status, source and query; text output prints one line per metric."
    },
    {
      "id": "FR-025-AC-1",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "With `TEAM_RETRO_HOME` unset, the store path is `~/.local/state/team-retro/<team>/snapshots.jsonl`; with it set, the path is under the given directory."
    },
    {
      "id": "FR-025-AC-2",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "Two consecutive snapshots add two lines and leave the first line byte-identical."
    },
    {
      "id": "FR-025-AC-3",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "A store path inside a git work tree, including through a symbolic link, is refused with a message naming the path, a non-zero exit, and no file or directory cre"
    },
    {
      "id": "FR-025-AC-4",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "A record with a newer `schema_version` stops the command with an error naming that version; a record with an older version is read and left unchanged."
    },
    {
      "id": "FR-025-AC-5",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "Every record carries `schema_version`, `kind`, `taken_at`, `team`, `window` and a metric map, and no line contains a credential value."
    },
    {
      "id": "FR-025-AC-6",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "A store with an unparsable line makes read commands stop naming that line number, and a store whose last byte is not a newline makes the append command refuse; "
    },
    {
      "id": "FR-025-AC-7",
      "path": "spec/functional/FR-025-retro-store.md",
      "role": "examined",
      "excerpt": "Records appended out of `taken_at` order are read back in `taken_at` order, with file order breaking ties."
    },
    {
      "id": "FR-026-AC-1",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "examined",
      "excerpt": "With the GitHub source failing, every flow metric is `unavailable` with a reason naming GitHub, `value` is null rather than 0, and the progress metrics are stil"
    },
    {
      "id": "FR-026-AC-2",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "examined",
      "excerpt": "A pull-request listing that stops before its last page makes the dependent metrics `unavailable`, not a smaller count."
    },
    {
      "id": "FR-026-AC-3",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "examined",
      "excerpt": "A metric whose source the configuration does not name is `unavailable` with reason `not configured`."
    },
    {
      "id": "FR-026-AC-4",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "examined",
      "excerpt": "Two runs that differ only in `--window`, or only in a threshold, produce different `query` strings for the affected metrics."
    },
    {
      "id": "FR-026-AC-5",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "examined",
      "excerpt": "The command exits zero and appends the record when some metrics are unavailable, and prints the unavailable ids on stderr."
    },
    {
      "id": "FR-026-AC-6",
      "path": "spec/functional/FR-026-retro-unavailable-not-zero.md",
      "role": "examined",
      "excerpt": "A median over zero merged pull requests is `unavailable` with reason `empty population`, while a merge rate over zero merges is the value 0."
    },
    {
      "id": "FR-027-AC-1",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "For a store of three snapshots, each metric shows its value, the change from the previous snapshot and the change from the baseline, with the baseline defaultin"
    },
    {
      "id": "FR-027-AC-2",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "A `down`-direction metric that rose in each of the last two comparisons is `regressing`; one that rose once and fell once is not."
    },
    {
      "id": "FR-027-AC-3",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "A metric unavailable on either side reports `unavailable`, and a metric whose `query` differs reports `query_changed` with both queries; neither reports a numer"
    },
    {
      "id": "FR-027-AC-4",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "A `neutral` metric is never `regressing`."
    },
    {
      "id": "FR-027-AC-5",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "With one snapshot the command reports that no delta exists and exits zero; output contains no combined score."
    },
    {
      "id": "FR-027-AC-6",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "The command makes no network request: it is run with source readers that fail the test if invoked, and still succeeds from the store alone."
    },
    {
      "id": "FR-027-AC-7",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "A backfill run after a snapshot leaves the snapshot as the latest record, and a backfilled record can serve as the baseline."
    },
    {
      "id": "FR-027-AC-8",
      "path": "spec/functional/FR-027-retro-delta.md",
      "role": "examined",
      "excerpt": "A `baseline_taken_at` matching no record stops the command with an error naming the value."
    },
    {
      "id": "FR-028-AC-1",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "A three-day range appends three `backfill` records with `taken_at` at the end of each UTC day."
    },
    {
      "id": "FR-028-AC-2",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "Running the same range twice appends nothing the second time and reports each skipped day."
    },
    {
      "id": "FR-028-AC-3",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "Metrics needing live state (`path_remaining`, agent verdicts, in-progress staleness, open pull-request counts) are `unavailable` with reason `not reconstructibl"
    },
    {
      "id": "FR-028-AC-4",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "A range containing a `snapshot` record's `taken_at` is refused with a message naming that record, and nothing is appended."
    },
    {
      "id": "FR-028-AC-5",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "For a synthetic fixture of pull-request timestamps with a known count, median and 90th percentile inside a 24h trailing window, the backfilled record for that d"
    },
    {
      "id": "FR-028-AC-6",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "The per-day record for a day with no merged pull requests has `prs_merged_per_hour` equal to 0 and both cycle metrics `unavailable` with reason `empty populatio"
    },
    {
      "id": "FR-028-AC-7",
      "path": "spec/functional/FR-028-retro-backfill.md",
      "role": "examined",
      "excerpt": "A backfill record's `query` strings include its window, and a delta between it and a snapshot taken with a different window reports `query_changed` ([FR-027](./"
    },
    {
      "id": "FR-029-AC-1",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "With department absent, existing snapshot/delta/backfill fixtures retain their metric IDs, values, queries and selection behavior. A department snapshot retains"
    },
    {
      "id": "FR-029-AC-2",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Two independent fixture teams with changed IDs/capacities serialize their configured scopes, observations, keyed measurements and evidence references; overlappi"
    },
    {
      "id": "FR-029-AC-3",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Missing policy, source timestamp, complete history, completion obligations, reviewer-ready time, lane capacity or dependency reachability produces the affected "
    },
    {
      "id": "FR-029-AC-4",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "The locally registered metrics match canonical fixture oracles for fresh/restarted/stale claims, state exits on window boundaries, merged issues with unmet obli"
    },
    {
      "id": "FR-029-AC-5",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Exact threshold boundary fixtures exercise all four comparators and three comparison bases. Reversed thresholds, wrong units, duplicate IDs, unknown IDs and uns"
    },
    {
      "id": "FR-029-AC-6",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "All ten classes remain in the report with later collectors absent. Red+unknown yields red/incomplete, amber+unknown amber/incomplete, green+unknown unknown/inco"
    },
    {
      "id": "FR-029-AC-7",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Changed scope, formula/catalog identity, parameter, window or applicable assessment policy produces query_changed without numeric trend; reordered equivalent co"
    },
    {
      "id": "FR-029-AC-8",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Duplicate events and equal timestamps with known source sequence produce exact canonical tag-repin/state-bounce findings once with ordered IDs and deduplication"
    },
    {
      "id": "FR-029-AC-9",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Empty duration populations are unavailable. For the singleton [7 hours], count=1, median=7 hours and p90=7 hours. Zero denominators are unavailable, and partial"
    },
    {
      "id": "FR-029-AC-10",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "Version-1 history remains byte-identical when read; unsupported record/envelope versions fail naming the version and leave the store unchanged. Shared JSON roun"
    },
    {
      "id": "FR-029-AC-11",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "For expected state keys A/B/C, fixture A has median=1h/p90=10h, B median=1h/p90=2h, and C is missing. With gte amber=3h/red=5h and p90 selected, key severities "
    },
    {
      "id": "FR-029-AC-12",
      "path": "spec/functional/FR-029-department-retro-contract.md",
      "role": "examined",
      "excerpt": "An offline lane_execution_state round trip preserves an FR-018 WORKING row and an UNKNOWN/partial row including null timestamp/count reasons, lifecycle provenan"
    },
    {
      "id": "FR-030-AC-1",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Given a baseline, a fixture delta adds and updates records, removes observed archive/trash entries and obsolete memberships, preserves unchanged IDs, and produc"
    },
    {
      "id": "FR-030-AC-2",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "A failed or rate-limited later delta page leaves the entire dataset, high-water mark, and successful fetch provenance unchanged; the next attempt uses the previ"
    },
    {
      "id": "FR-030-AC-3",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source c"
    },
    {
      "id": "FR-030-AC-4",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Cold roadmap and scope-check fixtures request no issue pages or histories; requesting a ticket-dependent report loads detail, subsequent reports reuse it, and a"
    },
    {
      "id": "FR-030-AC-5",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Two retro workers sharing a fixture cache reuse the centrally coordinated snapshot/delta without each probing and forcing a full workspace fetch; ticket/history"
    },
    {
      "id": "FR-030-AC-6",
      "path": "spec/functional/FR-030-incremental-linear-cache.md",
      "role": "examined",
      "excerpt": "Fixtures independently change source endpoint, workspace, credential-derived authorization identity, cache-model/schema version, and dataset coverage while reta"
    },
    {
      "id": "FR-031-AC-1",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "A fixture HTTP 429 with a non-JSON body and Retry-After, and a GraphQL RATELIMITED fixture with exhausted request/complexity resets, persist the latest applicab"
    },
    {
      "id": "FR-031-AC-2",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "After one process records cooldown, a separately launched process using the same cache attempts workspace, direct issue, history, probe, and force-refresh reads"
    },
    {
      "id": "FR-031-AC-3",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "With 2.1 seconds remaining the actual-server diagnostic is exactly `Linear rate limited; retry in 3s`; no retry or sleep occurs, and an empty cache fails withou"
    },
    {
      "id": "FR-031-AC-4",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknow"
    },
    {
      "id": "FR-031-AC-5",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "Counters record requests attempted, returned complexity when available, estimated complexity otherwise, duration, mode, and success/failure for each refresh; pa"
    },
    {
      "id": "FR-031-AC-6",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "`ix-board cache status` returns cache path, dataset coverage, snapshot age/staleness, last successful sync/full reconciliation, `rate_limit_until`, local budget"
    },
    {
      "id": "FR-031-AC-7",
      "path": "spec/functional/FR-031-shared-linear-quota.md",
      "role": "examined",
      "excerpt": "Tier fixtures produce the documented API-key/OAuth fallback limits and multipliers, default to free/basic with at most 50 percent share, and honor lower actual "
    },
    {
      "id": "NFR-001-M-1",
      "path": "spec/non-functional/NFR-001-warm-cache-latency.md",
      "role": "examined",
      "excerpt": "Warm-cache `next` elapsed time"
    },
    {
      "id": "NFR-001-M-2",
      "path": "spec/non-functional/NFR-001-warm-cache-latency.md",
      "role": "examined",
      "excerpt": "Warm-cache Linear requests"
    }
  ],
  "findings": [
    {
      "fnd": "FND-001",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-004-AC-20",
      "related_ids": [],
      "path": "tests/cache_coordination.rs",
      "lines": "174-225",
      "excerpt": "Two separately launched processes read the same expired dataset concurrently; exactly one source refresh occurs and the waiter reuses it after rechecking. A held lock causes ordinary fallback/error within five seconds, and terminating the owner releases the OS lock without a stale lockfile preventing recovery.",
      "finding": "FR-004-AC-20 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-002",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-004-AC-21",
      "related_ids": [],
      "path": "tests/cache_status.rs",
      "lines": "27-67",
      "excerpt": "A summary refresh, failed refresh, or fallback never advances detail freshness; pre-provenance snapshots have unknown age and fail bounded freshness checks. Fallback preserves unavailable components and does not invent zero values.",
      "finding": "FR-004-AC-21 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-003",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-019-AC-5",
      "related_ids": [],
      "path": "tests/summary_cache.rs",
      "lines": "113-150",
      "excerpt": "Cold and warm scope checks fetch no issue/history pages, reuse summary catalogs, and expose stale/age/cooldown metadata when using a last complete catalog.",
      "finding": "FR-019-AC-5 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-004",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-020-AC-2",
      "related_ids": [],
      "path": "tests/summary_cache.rs",
      "lines": "113-150",
      "excerpt": "A cold roadmap performs no issue/history enumeration, reports native progress with `linear_native` provenance and observation time, and distinguishes it from ticket completion; unsupported count/activity fields are present as null with unavailable reasons in JSON and text. Project update time is never mislabeled issue activity and no scope tally is used.",
      "finding": "FR-020-AC-2 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-005",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-030-AC-1",
      "related_ids": [],
      "path": "tests/delta_cache.rs",
      "lines": "148-290",
      "excerpt": "Given a baseline, a fixture delta adds and updates records, removes observed archive/trash entries and obsolete memberships, preserves unchanged IDs, and produces the same result when replayed; all three entity queries use the last successful sync minus 60 seconds and include archived records.",
      "finding": "FR-030-AC-1 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-006",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-030-AC-2",
      "related_ids": [],
      "path": "tests/delta_cache.rs",
      "lines": "294-397",
      "excerpt": "A failed or rate-limited later delta page leaves the entire dataset, high-water mark, and successful fetch provenance unchanged; the next attempt uses the previous successful high-water mark.",
      "finding": "FR-030-AC-2 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-007",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-030-AC-3",
      "related_ids": [],
      "path": "tests/delta_cache.rs",
      "lines": "294-397",
      "excerpt": "With no baseline, incompatible baseline, or a full reconciliation 24 hours old, the next applicable refresh performs a full scan; an ID absent from its source catalog is removed. An ordinary intervening refresh uses only deltas, and output exposes the last full reconciliation time.",
      "finding": "FR-030-AC-3 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-008",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-030-AC-4",
      "related_ids": [],
      "path": "tests/summary_cache.rs",
      "lines": "285-418",
      "excerpt": "Cold roadmap and scope-check fixtures request no issue pages or histories; requesting a ticket-dependent report loads detail, subsequent reports reuse it, and a summary refresh never advances detail freshness.",
      "finding": "FR-030-AC-4 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-009",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-030-AC-5",
      "related_ids": [],
      "path": "src/retro/live.rs",
      "lines": "199-204",
      "excerpt": "Two retro workers sharing a fixture cache reuse the centrally coordinated snapshot/delta without each probing and forcing a full workspace fetch; ticket/history requests remain covered by the shared quota meter.",
      "finding": "FR-030-AC-5 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior, including a two-retro-worker fixture proving central reuse and metered histories. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-010",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-030-AC-6",
      "related_ids": [],
      "path": "tests/delta_cache.rs",
      "lines": "294-397",
      "excerpt": "Fixtures independently change source endpoint, workspace, credential-derived authorization identity, cache-model/schema version, and dataset coverage while retaining overlapping stable record IDs; each mismatch (or absent compatibility metadata) requires a full baseline, never merges or returns the old partition as fallback, and a failed/blocked baseline exposes no old-partition records. An exact partition match permits incremental reuse; changing credentials does not reset the shared quota allowance.",
      "finding": "FR-030-AC-6 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    },
    {
      "fnd": "FND-011",
      "severity": "high",
      "confidence": "high",
      "method": "gap-analysis",
      "check": "coverage",
      "artifact_id": "FR-031-AC-6",
      "related_ids": [],
      "path": "tests/cache_status.rs",
      "lines": "27-67",
      "excerpt": "`ix-board cache status` returns cache path, dataset coverage, snapshot age/staleness, last successful sync/full reconciliation, `rate_limit_until`, local budget/reset and latest refresh counters in JSON and text without resolving credentials or making any network request; unknown values are explicit.",
      "finding": "FR-031-AC-6 remains untagged in the installed engine matrix. New tests use nonbinding doc comments or omit this criterion; the active model binds Rust #[trace(...)] attributes. Add a real criterion attribute to tests asserting this exact behavior. Oracle: quire matrix reports this criterion tagged with a nonignored behavioral test, and that test fails when the required behavior is broken."
    }
  ],
  "bindings": []
}
```

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-002 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-003 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-004 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-005 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-006 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-007 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-008 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-009 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-010 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |
| FND-011 | fixed | Verified against pushed 1d4e44432247003a3559b878fd50e1fe727617df; exact after excerpt below. |

## Disposition verdict (round 1)

PASS for the eleven SWM-184 review findings: each has a nonignored engine binding and inspected behavioral assertions. All eleven missing bindings are fixed, including two independent concurrent real LiveLinear adapters sharing baseline/delta and charging both history requests. The repository-wide historical matrix remains incomplete (177 untagged, 104 tagged, 9 inspection-only), so this disposition does not claim whole-repository assurance PASS.

Inspected all fix source and new tests at 1d4e44432247003a3559b878fd50e1fe727617df. Reviewer confined rerun passed 12 tests across retro_shared_cache (1), roadmap_api_compat (2), transport_redirect (1), and summary_cache (8); log /tmp/SWM-184-code-review/disposition-tests.log. Coder final focused logs show 324 affected tests passing, zero failures/ignored, including native text renderer; final check/fmt/lint logs pass. Fresh independent matrix /tmp/SWM-184-code-review/disposition-matrix.json confirms all eleven bindings. No real Linear API or aggregate CI run. Existing spec artifacts, including SR6082, remain untouched. Custody destination: reviews/swm-184-implementation-gap-analysis.md; copy this scratch file byte-identically. Exact runtime model id remains unavailable.

## Disposition evidence (round 1)

```yaml
{
  "dispositions": [
    {
      "fnd": "FND-001",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-004-AC-1\", \"FR-004-AC-2\", \"FR-004-AC-20\")]\n#[test]\nfn separate_process_waiter_reuses_one_completed_refresh_of_expired_snapshot() {"
    },
    {
      "fnd": "FND-002",
      "outcome": "fixed",
      "after_excerpt": "    #[trace(\"FR-004-AC-3\", \"FR-004-AC-21\")]\n    #[test]\n    fn fallback_json_and_text_preserve_unknown_age_cooldown_and_failure() {"
    },
    {
      "fnd": "FND-003",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-019-AC-5\", \"FR-030-AC-4\", \"FR-004-AC-21\")]\n#[test]\nfn cold_and_warm_scope_and_roadmap_reuse_summaries_without_advancing_real_detail_reads() {"
    },
    {
      "fnd": "FND-004",
      "outcome": "fixed",
      "after_excerpt": "    #[trace(\"FR-020-AC-2\")]\n    #[test]\n    fn native_roadmap_text_reports_complete_provenance_for_placed_and_unplaced_projects() {"
    },
    {
      "fnd": "FND-005",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-030-AC-1\")]\n#[test]\nfn detailed_deltas_preserve_unchanged_records_and_update_blocking_edges_on_unchanged_dependents() {"
    },
    {
      "fnd": "FND-006",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-030-AC-2\", \"FR-030-AC-3\", \"FR-030-AC-6\")]\n#[test]\nfn failed_detail_page_preserves_watermark_records_and_costs_then_retry_and_daily_full_are_correct()"
    },
    {
      "fnd": "FND-007",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-030-AC-2\", \"FR-030-AC-3\", \"FR-030-AC-6\")]\n#[test]\nfn failed_detail_page_preserves_watermark_records_and_costs_then_retry_and_daily_full_are_correct()"
    },
    {
      "fnd": "FND-008",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-019-AC-5\", \"FR-030-AC-4\", \"FR-004-AC-21\")]\n#[test]\nfn cold_and_warm_scope_and_roadmap_reuse_summaries_without_advancing_real_detail_reads() {"
    },
    {
      "fnd": "FND-009",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-030-AC-5\", \"FR-031-AC-4\")]\n#[test]\nfn two_live_retro_workers_reuse_full_and_delta_publications_and_meter_histories() {"
    },
    {
      "fnd": "FND-010",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-030-AC-2\", \"FR-030-AC-3\", \"FR-030-AC-6\")]\n#[test]\nfn failed_detail_page_preserves_watermark_records_and_costs_then_retry_and_daily_full_are_correct()"
    },
    {
      "fnd": "FND-011",
      "outcome": "fixed",
      "after_excerpt": "#[trace(\"FR-031-AC-6\", \"FR-004-AC-21\")]\n#[test]\nfn offline_status_accepts_empty_and_legacy_databases_without_credentials() {"
    }
  ]
}
```
