---
id: FR-009
title: Explain drift alerts in JSON and one-line lead summaries
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-002"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-008"
    type: depends_on
provenance:
  source: SWM-4
description: The ix-board drift command SHALL derive out-of-scope and churning alerts from configurable thresholds and render per-lead status and reasons as JSON and one-line text.
---
# FR-009: Explain drift alerts in JSON and one-line lead summaries

## Description

The `ix-board drift` command SHALL turn the signals in
[FR-008](FR-008-drift-signals.md) into the two named alerts `out_of_scope` and
`churning`, and SHALL return a status and evidence-based reasons for each lead.
All thresholds SHALL be plain configuration values, not learned or inferred by
an LLM.

## Inputs

- Signal groups produced under [FR-008](FR-008-drift-signals.md).
- Configured positive `window_hours`, positive `stall_hours`, nonnegative
  `spend_threshold`, nonnegative `max_net_ticket_growth`, and nonnegative
  `min_merged_prs_for_churn`. Configuration SHALL reject missing, invalid, or
  duplicate lead/team mappings and invalid threshold values with a clear error.
  A `spend_threshold` of zero means any observed positive spend; observed zero
  spend does not satisfy the spending predicate.

## Behavior

1. The `out_of_scope` alert SHALL be present for a lead when an attributed PR
   for that lead is out of running scope or the lead has a veering finding.
2. The `churning` alert SHALL be present for a lead when it has a Coding stall
   or busy-without-PR finding, or when net ticket growth exceeds
   `max_net_ticket_growth` **and** merged PR count is below
   `min_merged_prs_for_churn` in the reporting window.
3. A lead SHALL be `flagged` when either alert is present. A lead SHALL be
   `on_track` only when neither alert is present and every signal needed for
   both alert decisions is available. Otherwise its status SHALL be
   `unknown`, with unavailable-signal reasons.
4. Every alert SHALL carry a reason naming the source signal and its ticket,
   PR, count, and threshold evidence as applicable. PRs without attributable
   ticket IDs SHALL be visible in the global unassigned group with their PR
   details and reason, but SHALL NOT create a per-lead alert without
   ticket-to-lead attribution.
5. The `ix-board drift --json` form SHALL include report time, window,
   configured lead/team IDs, ordered running projects, each lead's status,
   alerts, reasons, all five signal groups with evidence and availability,
   and global unassigned PRs with their details and reasons. The text form SHALL
   emit exactly one line per configured lead in the same order, including
   lead ID, status, alert names, concise reasons, and the count, repository/
   number references, and reasons of global unassigned PRs. For an empty lead
   set, ix-board SHALL return an empty lead array in JSON and no text lines.
6. For the same source records, configuration, and report time, ix-board SHALL
   express the same lead statuses and alert reasons in a deterministic
   order. JSON SHALL remain machine-readable even when a signal is unavailable.
7. When an event source returns only a subset of required pages or partitions,
   ix-board SHALL keep any alert supported by received evidence, mark dependent
   signal counts unavailable, and return `unknown` unless another proven alert
   makes the lead `flagged`.

## Outputs

- A structured JSON drift report for machine consumers.
- A one-line-per-lead text report for team leaders.

### JSON field contract

The JSON object SHALL contain the following required fields. `timestamp`
means an RFC 3339 UTC string; `integer` means a JSON integer; `number` means a
finite JSON number. Nullable values SHALL be JSON `null` when their observation
is unavailable. Empty arrays SHALL mean no observed items only when the
corresponding signal is available; with an unavailable signal, an array MAY
hold already proven findings and SHALL NOT imply completeness.

| Field | Type and value shape |
|-------|----------------------|
| `report_time`, `window_start`, `window_end` | `timestamp`; the reporting interval is `[window_start, window_end)` and `window_end = report_time`. |
| `leads` | Array of lead objects in configured order. |
| `unassigned_prs` | Array of global unassigned PR objects, ordered by repository then PR number. |
| `leads[].lead_id`, `leads[].team_key` | Nonempty strings. |
| `leads[].running_projects` | Array of `{id: string, identifier: string or null}` in [FR-001](FR-001-ready-queue.md) queue order. |
| `leads[].status` | Exactly `on_track`, `flagged`, or `unknown`. |
| `leads[].alerts` | Array of `{kind: "out_of_scope" or "churning", reasons: array of nonempty strings}`; each kind occurs at most once. |
| `leads[].reasons` | Array of nonempty strings explaining lead status, empty only for `on_track`. |
| `leads[].signals` | Object with exactly the five required keys `pr_scope`, `veering`, `stall`, `busy_without_pr`, and `ticket_churn`. |
| `leads[].signals.<signal>.availability` | Exactly `available` or `unavailable`. |
| `leads[].signals.<signal>.missing_sources` | Array of source/partition reason strings; empty if available, nonempty if unavailable. |
| `leads[].signals.pr_scope.prs` | Array of `{repository: string, number: integer, url: string, ticket_id: string, project_id: string or null, classification: "in_scope" or "out_of_scope"}`. |
| `leads[].signals.veering.tickets` | Array of `{ticket_id: string, transitioned_at: timestamp, reason: "outside_running_project" or "priority_inversion", skipped_ticket_id: string or null}`. |
| `leads[].signals.stall.tickets` | Array of `{ticket_id: string, coding_since: timestamp, elapsed_hours: number, last_activity_at: timestamp or null}`. |
| `leads[].signals.busy_without_pr` | In addition to common fields: `{finding: boolean or null, herdr_busy: boolean or null, otel_spend: number or null, new_pr_count: integer or null}`. |
| `leads[].signals.ticket_churn` | In addition to common fields: `{created: integer or null, completed: integer or null, merged_prs: integer or null, net_growth: integer or null}`; counts are nonnegative when known, while net growth may be negative. |
| `unassigned_prs[]` | `{repository: string, number: integer, url: string, title: string, branch: string, created_at: timestamp, merged_at: timestamp or null, ticket_ids: array of strings, reason: "missing_ticket_id" or "conflicting_ticket_ids" or "unresolved_ticket_id" or "unconfigured_team"}`. |

Each signal's common `availability` and `missing_sources` fields SHALL appear
even when its evidence array is empty. ix-board SHALL order PR arrays by
repository and PR number, ticket arrays by ticket ID and event time, alerts by
`out_of_scope` then `churning`, and missing-source and reason arrays
lexicographically. The text form MAY abbreviate details but SHALL preserve the
same status, alert kinds, and actionable PR references/reasons as JSON.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-009-AC-1 | An out-of-scope PR or a queue-veering fixture produces `out_of_scope` for its attributed lead, with the source ticket/PR and scope or skipped-ticket reason. | Test |
| FR-009-AC-2 | A Coding stall, busy-without-PR finding, or net growth above the configured limit with merged PR count below its configured minimum produces `churning`; boundary values do not cross strict thresholds. | Test |
| FR-009-AC-3 | Valid threshold changes alter only the corresponding alert decision; `spend_threshold = 0` accepts positive observed spend but not observed zero, while invalid values or duplicate team ownership produce a configuration error. | Test |
| FR-009-AC-4 | A fixture with no alerts and complete signals yields `on_track`; one with missing required evidence and no proven alert yields `unknown`; a proven alert yields `flagged` with reasons even if another signal is unavailable. | Test |
| FR-009-AC-5 | The same multi-lead fixture renders valid JSON and exactly one text line per lead, in configured order, with matching status and alert reasons; JSON assertions check every required field name, value type, enum, nullability, and unassigned PR detail, and each text line identifies unassigned PR references and reasons. | Test |
| FR-009-AC-6 | Repeated output from identical fixture data, configuration, and report time is byte-for-byte stable for each format. | Test |
| FR-009-AC-7 | A fixture with an incomplete PR or Linear history page marks dependent counts `null` and the signal unavailable; it cannot yield `on_track` or an absent alert from that partial evidence, while a separately proven alert still yields `flagged`. | Test |

## Dependencies

- **Use case**: [US-002](../usecase/US-002-team-leader-drift.md)
- **Signal classification**: [FR-008](FR-008-drift-signals.md)
