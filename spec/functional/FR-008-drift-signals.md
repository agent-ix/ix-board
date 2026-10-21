---
id: FR-008
title: Classify drift signals from attributed work and activity
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-002"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-001"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-005"
    type: depends_on
provenance:
  source: SWM-4
description: The ix-board drift read SHALL classify PR scope, queue veering, Coding stalls, activity without a PR, ticket churn, and configured priority-goal staffing/crowding from structured source evidence.
---
# FR-008: Classify drift signals from attributed work and activity

## Description

The `ix-board drift` read SHALL produce five signal groups for every configured
lead: PR-to-ticket scope, queue veering, Coding stall, busy or spending without
a PR, and ticket churn. It SHALL also assess every configured priority goal.
It SHALL use structured Linear, GitHub, herdr, and OpenTelemetry records,
without an LLM or prose interpretation. The read SHALL not mutate any source.

## Inputs

- A configured lead identifier and its Linear team key. A team SHALL map to at
  most one lead within a report; duplicate ownership is a configuration error.
- A report time and a configured, positive reporting window. Events in the
  half-open interval `[report time - window, report time)` belong to the report.
  Elapsed-hour comparisons use UTC timestamps.
- Linear project and issue records, issue state-transition and creation times,
  and the queue data defined by [FR-001](FR-001-ready-queue.md). For each
  Coding transition in the reporting window, the historical queue evidence
  SHALL include the immediately preceding project run state, priority and
  manual order and team associations; ticket team/project association,
  workflow/spec state, priority and manual order; and blocker edges and
  prerequisite completion states, including cross-team prerequisites. Event
  histories or a timestamped snapshot MAY supply this
  evidence only when they cover every input needed to reconstruct that queue.
- GitHub PR creation and merge events, including PRs opened before the window
  and merged inside it, plus branch/title ticket identifiers; commit events
  with ticket identifiers; per-lead herdr busy state and OTel spending events.
- Zero or more `priority_goals`, each identifying a Linear project or
  milestone by its exact identifier/name, or explicit issue IDs when no
  project or milestone identifies the goal. Each goal SHALL have a positive
  integer `minimum_workers` and a positive finite `no_progress_hours`. The
  no-progress threshold SHALL be independent of the reporting window and
  Coding-stall threshold.

## Behavior

1. At report time, ix-board SHALL treat only each lead's team and its `Started`
   projects as that lead's running scope. It SHALL order those projects by the project
   priority, manual order, and stable identifiers in
   [FR-001](FR-001-ready-queue.md). It SHALL preserve that order in signal
   details. `Paused` and other non-running projects SHALL remain out of scope.
2. For every PR created **or merged** in the reporting window, ix-board SHALL
   extract ticket identifiers from its branch and title, including for a PR
   opened before the window and merged inside it. When exactly one distinct identifier
   exists, ix-board SHALL attribute the PR through that ticket's Linear team
   and project, never the shared GitHub account. When no identifier exists,
   identifiers conflict, or an identifier does not resolve to a Linear issue,
   ix-board SHALL emit a global unassigned PR finding with the reason. Each
   unassigned finding SHALL retain the PR's repository, number, URL, title,
   branch, creation time, and any extracted ticket IDs for investigation.
3. For an attributed new PR, ix-board SHALL classify it as `in_scope` when its
   ticket belongs to the lead's team and a running project, and `out_of_scope`
   when the ticket belongs to that lead's team but its project is not running.
   For a PR ticket in another configured lead's team, ix-board SHALL assign
   the PR to that other lead. For a ticket in an unconfigured team, ix-board SHALL
   emit a global unassigned finding with the known ticket ID and PR details.
4. When a ticket enters `Coding` during the reporting window, ix-board SHALL
   report queue veering if the ticket was outside its lead's running projects
   immediately before that transition,
   or if a higher-ranked unblocked ticket in the same lead's running queue was
   ready immediately before that transition. The finding SHALL name the moved ticket and,
   for a priority inversion, at least the highest-ranked skipped ticket.
   ix-board SHALL reconstruct the [FR-001](FR-001-ready-queue.md) queue
   immediately before the transition from contemporaneous evidence for every
   queue input listed above. If any required historical state or ordering
   evidence cannot be reconstructed, ix-board SHALL mark that transition's
   veering decision unavailable with the missing input as reason. The current
   project or ticket snapshot SHALL NOT stand in for the historical queue.
5. While a ticket remains in `Coding` longer than the configured positive
   `stall_hours`, ix-board SHALL report a stall if it finds no ticket-attributed
   commit or PR in the trailing `stall_hours` before report time. ix-board SHALL
   include the ticket ID, elapsed Coding hours, and the last observed
   commit or PR time when one exists. Attribution SHALL use ticket IDs in
   structured commit/PR metadata or their associated branch/title.
6. When herdr reports a lead busy at report time **or** that lead's observed
   OTel spend during the reporting window is greater than zero **and** reaches
   the configured `spend_threshold`,
   ix-board SHALL report busy-without-PR if that lead has no attributed PR
   created in the window. The finding SHALL state which activity source met
   the condition and the observed spend when applicable.
7. For each lead's team and running projects, ix-board SHALL count tickets
   created, tickets transitioned to `Completed`, and attributed PRs merged in
   the reporting window, regardless of when those PRs were opened. When a PR
   opened before the window merges inside it, ix-board SHALL count it in merged-PR
   churn after ticket-ID attribution, but not in new-PR scope or
   busy-without-new-PR counts. ix-board SHALL report the three counts and
   the net ticket growth (`created - completed`) as the ticket-churn signal.
   ix-board SHALL count a ticket or PR once for each applicable event, using
   its stable ID.
8. If a source needed to decide a signal is unavailable, ix-board SHALL report
   that signal as unavailable with its missing source. For the busy-or-spend
   condition, an observed true input SHALL establish activity even if the
   other input is unavailable; a false condition requires both inputs known
   and false. ix-board SHALL not replace missing observations with zero,
   classify unknown PR scope as in scope, or call a lead on track from
   incomplete evidence.
9. For each source query, ix-board SHALL determine whether all pages and
   partitions needed for the reporting interval and historical queue have
   been enumerated. If a GitHub PR/commit or Linear issue/history page or
   partition is missing, truncated, or has an unresolved next-page cursor,
   ix-board SHALL mark each affected signal unavailable with the incomplete
   source and page/partition reason. It SHALL retain findings already proven
   by received records but SHALL NOT treat partial counts as complete or use
   them to conclude that a lead is `on_track` or has no alert.
10. When `ix-board drift` is invoked, ix-board SHALL assess each configured priority
    goal using its own `no_progress_hours`; this value is a stale threshold,
    not a polling interval. ix-board SHALL resolve a goal using exact Linear
    project or milestone membership, or its explicit issue IDs when neither
    membership is available. It SHALL include open direct and transitive
    blockers in goal-related activity and worker coverage. It SHALL NOT infer
    goal membership from prose.
11. ix-board SHALL count `active_workers` as the number of distinct, nonempty
    Linear assignee IDs on goal issues or their open blockers in `Coding` or
    `In Progress` (including equivalent custom states categorized by Linear
    as `started`). An assignee SHALL count once regardless of the number of
    qualifying issues. An unassigned issue SHALL add no worker. The ix-board
    drift read SHALL derive this count from assignee identities, not the number
    of issues or recent activity events.
12. For each goal, ix-board SHALL report the latest goal issue completion time
    and latest goal issue state-transition time at or before report time.
    ix-board SHALL calculate `hours_since_progress` as elapsed UTC hours since
    the later of those two events. If neither event exists, ix-board SHALL use
    the earliest known creation time of a goal issue as the baseline; without
    a complete baseline, ix-board SHALL mark elapsed hours unavailable.
    Linked commits, PR activity, assignee changes, and issue edits without a
    state transition SHALL NOT reset this clock.
13. When the elapsed time is greater than or equal to `no_progress_hours`,
    ix-board SHALL find other issues that transitioned in
    `[report_time - no_progress_hours, report_time)`. It SHALL group
    advancing tickets from every other project by team and project, naming
    every qualifying ticket in `crowding_out` regardless of team. Goal issues
    and their blockers SHALL NOT appear as crowding-out work. It SHALL also
    report cross-team advances in `out_of_team_activity` when a ticket is
    assigned to a worker counted for the goal. The groups and ticket IDs SHALL
    be deterministically ordered.
14. ix-board SHALL flag a goal when its complete active-worker count is below
    `minimum_workers`, or when its no-progress threshold is reached and at
    least one other project has qualifying advancing tickets. When complete
    evidence shows no open goal issues remain, ix-board SHALL report
    `on_track` and SHALL NOT report a staffing shortfall. A goal whose open
    issues are all blocked SHALL have status `blocked`; otherwise a positive
    finding SHALL have status `flagged`, a complete assessment with neither
    condition SHALL have status `on_track`, and an assessment lacking required
    evidence SHALL have status `unknown`.
15. When unrelated evidence is unavailable and a positive finding is proven,
    ix-board MAY report the priority goal as `flagged`.
16. A priority-goal assessment SHALL require complete current Linear issue,
    assignee, goal-membership, blocker, and relevant state-history evidence.
    When an input is unavailable or its pages are incomplete, ix-board SHALL
    name the missing source/reason and SHALL mark dependent counts and elapsed
    hours unavailable. It SHALL NOT emit `on_track` or `flagged=false` from
    incomplete evidence, or imply that an empty crowding list is complete.
    ix-board SHALL preserve findings and advancing-ticket groups already
    proven by received records.
17. Given the deterministic QSL L2 replay at
    `2026-10-05T18:00:00Z`, a 12-hour no-progress threshold, and the supplied
    issue history, ix-board SHALL report 13 hours since the latest goal state
    transition, preserve the latest completion and transition timestamps,
    report distinct active assignees versus the configured minimum, and name
    IR-path tickets `IR-626`, `QSL-635`, and `QSL-641` in deterministic order.
    It SHALL flag the goal for stale progress with competing work regardless
    of whether its assigned-worker minimum is met.

## Outputs

- Five evidence-bearing signal groups for each configured lead, in configured
  lead order, with running projects in queue order.
- One assessment for each configured priority goal, in configuration order,
  with progress timestamps, elapsed hours, distinct active-worker count and
  minimum, status/availability, and competing projects/tickets.
- A global unassigned PR group, including PRs with no ticket ID, with PR
  details and a reason attribution could not be completed.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-008-AC-1 | Fixture PRs with one ticket ID in branch or title classify in scope for a running project and out of scope for a paused or other non-running project; conflicting, absent, unresolved, or unconfigured-team IDs appear as global unassigned findings with PR details and reason, without attribution by GitHub user. | Test |
| FR-008-AC-2 | Fixture Coding transitions into a non-running project or ahead of a higher-ranked unblocked ticket yield veering findings naming the ticket and skipped ticket where applicable; a correctly ordered transition does not. The fixture varies historical project run state/priority/order, ticket workflow/spec state/priority/order, and blocker completion while holding the current snapshot fixed; decisions follow the pre-transition evidence. | Test |
| FR-008-AC-3 | A fixture ticket in Coding for longer than `stall_hours` with no ticket-attributed commit or PR in the trailing `stall_hours` yields a stall; an attributed commit or PR in that trailing interval prevents it. | Test |
| FR-008-AC-4 | Fixture herdr busy state or positive OTel spend at or above the configured threshold with no attributed new PR yields busy-without-PR; an attributed new PR prevents it. With threshold zero, observed spend zero alone does not yield a finding, while positive spend does. | Test |
| FR-008-AC-5 | Fixture creation, Completed transition, and merged-PR events produce correct per-lead counts and net ticket growth, with repeated event records deduplicated by stable event identity; a PR opened before the window but merged inside it counts as landed work without counting as a new PR. | Test |
| FR-008-AC-6 | Missing historical project status/priority/order, ticket state/spec state/priority/order, or blocker evidence marks the veering decision unavailable with the missing input as reason; changing today's snapshot alone cannot change a decision based on complete pre-transition evidence. | Test |
| FR-008-AC-7 | Given identical source records, configuration, and report time, repeated reads return the same signal groups and ordering without invoking an LLM or writing source records. | Test |
| FR-008-AC-8 | A fixture omitting a GitHub PR/commit or Linear issue/history page or partition marks affected signals unavailable, retains any proven finding, and never yields a clean/no-alert conclusion from partial counts; complete pagination yields the expected counts and alert decision. | Test |
| FR-008-AC-9 | Missing herdr, OTel, GitHub, or Linear observations mark each affected signal unavailable rather than replacing missing values with zero; one observed true busy/spend input may still prove activity when the other is unavailable. | Test |
| FR-008-AC-10 | A configuration fixture accepts a project, milestone, or explicit issue-ID goal with a positive integer minimum and positive finite no-progress threshold; it rejects blank/duplicate goals, a zero minimum, and nonpositive or non-finite thresholds. The no-progress threshold is independent of `window_hours` and `stall_hours`. | Test |
| FR-008-AC-11 | A fixture with two active goal issues assigned to the same Linear assignee and a third unassigned issue reports one active worker; adding a second distinct assignee reports two, while more issues assigned to existing assignees do not increase the count. Only Coding/In Progress (including equivalent `started` custom states) contribute. | Test |
| FR-008-AC-12 | A fixture with a Completed transition and a later state transition reports both timestamps and computes elapsed hours from the later transition; a linked commit/PR or metadata-only edit does not reset the goal clock. With no transitions, the earliest known goal issue creation time is the baseline; without a complete baseline the elapsed value is unavailable. | Test |
| FR-008-AC-13 | The QSL L2 replay at `2026-10-05T18:00:00Z` with a 12-hour threshold reports 13 hours since the 05:00Z goal transition, preserves the 06:00Z prior-day completion, and includes `IR-626`, `QSL-635`, and `QSL-641` in the IR-path `crowding_out` group; it flags the stale goal regardless of whether the distinct-worker minimum is met. | Test |
| FR-008-AC-14 | A fixture with complete history groups all advancing issues from other projects by team and project in `crowding_out`, including all ticket IDs across teams; cross-team tickets assigned to a counted goal worker also appear in `out_of_team_activity`. Goal issues and blockers are excluded, and grouping/order is deterministic. | Test |
| FR-008-AC-15 | Missing current issue/assignee, goal-membership, blocker, or relevant history pages make dependent counts/timestamps unavailable and produce explicit missing-source reasons; status is `unknown` unless a positive finding is already proven, in which case it is `flagged`. Partial crowding evidence is retained, but partial evidence never yields `on_track`, `flagged=false`, or a complete empty list. | Test |
| FR-008-AC-16 | With complete evidence showing every goal issue is terminal, the report has no staffing-shortfall finding and returns `on_track`; with an open goal issue, the configured worker minimum applies. | Test |

## Dependencies

- **Use case**: [US-002](../usecase/US-002-team-leader-drift.md)
- **Queue rules**: [FR-001](FR-001-ready-queue.md)
- **Structured-data boundary**: [FR-005](FR-005-structured-data-boundary.md)
