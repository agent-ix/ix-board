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
  and the queue data defined by [FR-001](FR-001-ready-queue.md).
- GitHub PR creation and merge events plus branch/title ticket identifiers;
  commit events with ticket identifiers; per-lead herdr busy state and OTel
  spending events.

## Behavior

1. For each lead, ix-board SHALL treat only that lead's team and its `Started`
   projects as running scope. It SHALL order those projects by the project
   priority, manual order, and stable identifiers in
   [FR-001](FR-001-ready-queue.md). It SHALL preserve that order in signal
   details. `Paused` and other non-running projects SHALL remain out of scope.
2. For every PR created in the reporting window, ix-board SHALL extract ticket
   identifiers from its branch and title. When exactly one distinct identifier
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
   report queue veering if the ticket is outside its lead's running projects,
   or if a higher-ranked unblocked ticket in the same lead's running queue was
   ready immediately before that transition. The finding SHALL name the moved ticket and,
   for a priority inversion, at least the highest-ranked skipped ticket.
   ix-board SHALL use the [FR-001](FR-001-ready-queue.md) queue rank and
   blocker rules immediately before the transition. When transition history is missing,
   ix-board SHALL report the veering signal as unavailable rather than infer
   it from current state.
5. While a ticket remains in `Coding` longer than the configured positive
   `stall_hours`, ix-board SHALL report a stall if it finds no ticket-attributed
   commit or PR in the trailing `stall_hours` before report time. ix-board SHALL
   include the ticket ID, elapsed Coding hours, and the last observed
   commit or PR time when one exists. Attribution SHALL use ticket IDs in
   structured commit/PR metadata or their associated branch/title.
6. When herdr reports a lead busy at report time **or** that lead's OTel spend
   during the reporting window reaches the configured `spend_threshold`,
   ix-board SHALL report busy-without-PR if that lead has no attributed PR
   created in the window. The finding SHALL state which activity source met
   the condition and the observed spend when applicable.
7. For each lead's team and running projects, ix-board SHALL count tickets
   created, tickets transitioned to `Completed`, and attributed PRs merged in
   the reporting window. It SHALL report the three counts and the net ticket
   growth (`created - completed`) as the ticket-churn signal. ix-board SHALL
   count a ticket or PR once for each applicable event, using its stable ID.
8. If a source needed to decide a signal is unavailable, ix-board SHALL report
   that signal as unavailable with its missing source. For the busy-or-spend
   condition, an observed true input SHALL establish activity even if the
   other input is unavailable; a false condition requires both inputs known
   and false. ix-board SHALL not replace missing observations with zero,
   classify unknown PR scope as in scope, or call a lead on track from
   incomplete evidence.

## Outputs

- Five evidence-bearing signal groups for each configured lead, in configured
  lead order, with running projects in queue order.
- A global unassigned PR group, including PRs with no ticket ID, with PR
  details and a reason attribution could not be completed.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-008-AC-1 | Fixture PRs with one ticket ID in branch or title classify in scope for a running project and out of scope for a paused or other non-running project; conflicting, absent, unresolved, or unconfigured-team IDs appear as global unassigned findings with PR details and reason, without attribution by GitHub user. | Test |
| FR-008-AC-2 | Fixture Coding transitions into a non-running project or ahead of a higher-ranked unblocked ticket yield veering findings naming the ticket and skipped ticket where applicable; a correctly ordered transition does not. | Test |
| FR-008-AC-3 | A fixture ticket in Coding for longer than `stall_hours` with no ticket-attributed commit or PR in the trailing `stall_hours` yields a stall; an attributed commit or PR in that trailing interval prevents it. | Test |
| FR-008-AC-4 | Fixture herdr busy state or OTel spend at the configured threshold with no attributed new PR yields busy-without-PR; an attributed new PR prevents it. | Test |
| FR-008-AC-5 | Fixture creation, Completed transition, and merged-PR events produce correct per-lead counts and net ticket growth, with repeated event records deduplicated by stable event identity. | Test |
| FR-008-AC-6 | Missing transition, GitHub, herdr, OTel, or Linear evidence marks each affected signal unavailable rather than producing a zero or on-track conclusion. | Test |
| FR-008-AC-7 | Given identical source records, configuration, and report time, repeated reads return the same signal groups and ordering without invoking an LLM or writing source records. | Test |

## Dependencies

- **Use case**: [US-002](../usecase/US-002-team-leader-drift.md)
- **Queue rules**: [FR-001](FR-001-ready-queue.md)
- **Structured-data boundary**: [FR-005](FR-005-structured-data-boundary.md)
