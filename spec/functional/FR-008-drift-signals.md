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
description: The ix-board drift read SHALL classify PR scope, queue veering, Coding stalls, activity without a PR, and ticket churn for configured leads from structured source evidence.
---
# FR-008: Classify drift signals from attributed work and activity

## Description

The `ix-board drift` read SHALL produce five signal groups for every configured
lead: PR-to-ticket scope, queue veering, Coding stall, busy or spending without
a PR, and ticket churn. It SHALL use structured Linear, GitHub, herdr, and
OpenTelemetry records, without an LLM or prose interpretation. The read SHALL
not mutate any source.

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

## Outputs

- Five evidence-bearing signal groups for each configured lead, in configured
  lead order, with running projects in queue order.
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

## Dependencies

- **Use case**: [US-002](../usecase/US-002-team-leader-drift.md)
- **Queue rules**: [FR-001](FR-001-ready-queue.md)
- **Structured-data boundary**: [FR-005](FR-005-structured-data-boundary.md)
