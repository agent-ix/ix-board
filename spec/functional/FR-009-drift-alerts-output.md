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

## Outputs

- A structured JSON drift report for machine consumers.
- A one-line-per-lead text report for team leaders.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-009-AC-1 | An out-of-scope PR or a queue-veering fixture produces `out_of_scope` for its attributed lead, with the source ticket/PR and scope or skipped-ticket reason. | Test |
| FR-009-AC-2 | A Coding stall, busy-without-PR finding, or net growth above the configured limit with merged PR count below its configured minimum produces `churning`; boundary values do not cross strict thresholds. | Test |
| FR-009-AC-3 | Valid threshold changes alter only the corresponding alert decision; invalid values or duplicate team ownership produce a configuration error. | Test |
| FR-009-AC-4 | A fixture with no alerts and complete signals yields `on_track`; one with missing required evidence and no proven alert yields `unknown`; a proven alert yields `flagged` with reasons even if another signal is unavailable. | Test |
| FR-009-AC-5 | The same multi-lead fixture renders valid JSON and exactly one text line per lead, in configured order, with matching status and alert reasons; JSON includes all signal evidence and unassigned PR details, and each text line identifies unassigned PR references and reasons. | Test |
| FR-009-AC-6 | Repeated output from identical fixture data, configuration, and report time is byte-for-byte stable for each format. | Test |

## Dependencies

- **Use case**: [US-002](../usecase/US-002-team-leader-drift.md)
- **Signal classification**: [FR-008](FR-008-drift-signals.md)
