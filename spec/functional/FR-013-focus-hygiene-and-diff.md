---
id: FR-013
title: Report focus hygiene and changes since the prior report
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-005"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-011"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-004"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-005"
    type: depends_on
provenance:
  source: SWM-35
description: The ix-board focus hygiene report SHALL identify project, milestone, PR, stale issue, and duplicate-title findings and compare the focus with its most recent successful report.
---
# FR-013: Report focus hygiene and changes since the prior report

## Description

`ix-board focus hygiene` SHALL report actionable hygiene findings for the
deduplicated focus issue set resolved by
[FR-011](FR-011-inline-planner-focus.md), plus project milestone metadata and
linked pull request status. It SHALL also report the changes between the
current focus snapshot and the most recent successful hygiene report for the
same Linear workspace and canonical inline focus. The report SHALL use
structured Linear data and GitHub pull request metadata only; it SHALL NOT
read or write GitHub Issues, interpret ticket prose, or mutate Linear or
GitHub.

## Inputs

- The inline initiative, project, and issue selectors from FR-011.
- The report time, captured once as an RFC 3339 UTC timestamp.
- `stale_days`, a positive whole number of days, defaulting to 30; the CLI
  SHALL permit an explicit positive `--stale-days` override.
- The complete structured Linear workspace snapshot needed to resolve the
  focus and retrieve current fields for previously focused issues that no
  longer match its selectors. It includes issue state, project,
  project-milestone, blocker, created, and updated fields; milestone target
  dates; and structured Linear attachment URLs.
- GitHub pull request records resolved from supported URLs attached to Linear
  issues. A PR URL SHALL identify a GitHub repository and pull-request number.
- The most recent successful report snapshot, if one exists, for the same
  workspace and canonical focus.

## Behavior

1. The command SHALL accept the same inline selectors as `focus expand`,
   resolve them under FR-010 and FR-011, and reject an empty focus. It SHALL
   inspect every resolved focus issue regardless of workflow state, so
   completed examples remain available for duplicate detection and diffing.
2. An issue SHALL produce a `missing_scope` finding if it has no project, no
   project milestone, or both. The finding SHALL list each missing field.
   ix-board SHALL check milestone absence independently of project absence.
3. A project milestone SHALL produce an `overdue_milestone` finding when its
   target date is before the report's UTC calendar date and it has at least one
   focus issue whose workflow category is neither completed nor canceled. The
   finding SHALL include the milestone, target date, and sorted open issue IDs.
4. ix-board SHALL identify a linked PR only from a structured Linear
   attachment URL matching a GitHub pull-request URL. ix-board SHALL query the PR record by
   repository and number. ix-board SHALL report `done_with_open_pr` for each
   linked PR whose GitHub state is open when its focus issue has a completed
   workflow category. ix-board SHALL report `open_with_merged_pr` when a focus
   issue has a workflow category other than completed or canceled and its
   linked PR has a non-null merged timestamp. ix-board SHALL not classify
   closed-but-unmerged PRs as either finding. ix-board SHALL evaluate multiple
   issue-to-PR links separately. ix-board SHALL retain each finding with its
   source issue IDs. An unrecognized URL, inaccessible PR,
   or incomplete GitHub response SHALL make the relevant PR check unavailable
   with a reason; it SHALL NOT be treated as evidence that no linked PR exists.
5. An issue SHALL produce a `stale_issue` finding when it is open (not in a
   completed or canceled workflow category) and its `updatedAt` is strictly
   earlier than `report_time - stale_days`. The finding SHALL include the
   `updatedAt`, report time, configured threshold, and elapsed whole days.
6. ix-board SHALL run near-duplicate matching only over the issues in the
   resolved focus set. ix-board SHALL lowercase titles and split each title
   into tokens at every character that is not a Unicode letter or number. The
   fixed stop-word set is:
   `a`, `an`, `as`, `at`, `by`, `for`, `from`, `in`, `into`, `of`, `on`,
   `or`, `the`, `to`, `with`, `agent`, `antipattern`, `doc`, `docs`,
   `instructions`, `removal`, `slated`, `tracking`. The remaining
   unique tokens form the normalized content-token set. ix-board SHALL build a
   deterministic inverted index from content token to sorted issue IDs. For
   each token in ascending order, ix-board SHALL enumerate every unordered
   pair of IDs in that token's posting list in ascending ID order. The
   all-candidate set SHALL contain each unique issue pair sharing at least
   one content token, with a count of shared tokens for each pair. The
   default maximum all-candidate-pair count is 100,000, before applying the
   three-token, minimum-token, or overlap match thresholds. If adding another
   unique pair would exceed this limit, ix-board SHALL immediately return an
   explicit `resource_limit` error with no partial hygiene report and preserve
   the existing diff baseline. After generating all candidates
   within the limit, ix-board SHALL sort pairs lexicographically by
   `(lower_issue_id, higher_issue_id)` and SHALL match a pair only when it
   shares at least three content tokens, each token set has at least four
   tokens, and `shared-token-count / smaller-set-size >= 0.75`.
   ix-board SHALL group matching issue pairs into connected components of
   this pairwise relation. Each component is one `near_duplicate_titles`
   finding with issue IDs sorted ascending; ix-board SHALL order groups by
   their sorted issue-ID tuples and SHALL report original titles. This
   deterministic heuristic can include false matches; the output is a review
   signal and SHALL NOT merge, close, or otherwise change issues.
7. For the diff, ix-board SHALL compare the union of issue IDs in the current
   resolved focus and the most recent successful snapshot for the same Linear
   workspace and canonical selector key (the deduplicated canonical selector
   IDs, sorted by selector kind and ID). ix-board SHALL retrieve current
   records for every union member from the complete current Linear workspace
   snapshot, including issues that no longer match the focus selectors. If a
   current record for a union member is unavailable, ix-board SHALL mark the
   diff unavailable and SHALL NOT replace its baseline. For each union member,
   ix-board SHALL compare focus membership, workflow state, project ID,
   milestone ID, and sorted blocker-ID set. ix-board SHALL report old and
   current values for each changed field. ix-board SHALL label membership
   changes `entered_focus` or `left_focus`. ix-board SHALL report issues absent from
   the prior snapshot and created since its report time as `created`; it SHALL
   report issues that changed from a non-completed/non-canceled state to a
   completed or canceled state as `closed`. ix-board SHALL report an issue as
   both created and closed when both conditions hold. ix-board SHALL emit at
   most one entry per issue and change kind in a report.
8. ix-board SHALL persist at most the most recent successful diff baseline per
   workspace and canonical focus in the local SQLite store. ix-board SHALL
   store the canonical selector key, report time, and for each currently
   focused issue its stable ID, focus membership, workflow state/category,
   project ID, milestone ID, and sorted blocker IDs. ix-board SHALL replace
   the baseline atomically only after it obtains a complete Linear snapshot
   and successfully constructs the diff. When a read is incomplete,
   ix-board SHALL leave the prior baseline intact. No older
   snapshots are required. When no prior baseline exists, the diff SHALL be
   unavailable with reason `no_prior_report`; it SHALL NOT be reported as an
   empty diff. When a prior snapshot is missing, unreadable, or lacks fields
   required by this contract, the diff SHALL be unavailable with the reason.
9. Every report group SHALL include an availability value and reasons. A
   check SHALL be unavailable when any source needed for that check is
   incomplete or inaccessible; already-proven findings MAY remain visible,
   but an unavailable check SHALL NOT imply that no matching issues exist.
   An incomplete Linear snapshot SHALL prevent baseline replacement. A
   completed report with an unavailable GitHub PR lookup MAY still update the
   Linear diff baseline if the Linear snapshot and diff are complete.
10. For identical source records, canonical focus, configuration, and report
    time, the report SHALL produce the same finding membership and ordering.
    ix-board SHALL sort findings by kind and canonical issue ID. ix-board SHALL
    sort PR findings by repository and PR number. ix-board SHALL sort duplicate
    groups by their sorted issue-ID tuple.

## Outputs

- A structured report containing report time, canonical selector key, the
  `stale_days` threshold, five hygiene groups (`missing_scope`,
  `overdue_milestone`, PR-status mismatch, `stale_issue`, and
  `near_duplicate_titles`), and a diff group.
- An `issue_refs` map keyed by the stable UUIDs referenced in findings. Each
  available entry SHALL contain the current Linear identifier and title.
  Finding IDs remain unchanged. Terminal output SHALL print the identifier and
  title for each referenced issue, falling back to the UUID when no current
  issue record is available.
- Each group SHALL expose `available` or `unavailable`, a list of missing
  source reasons, and any findings proven by available records.
- The diff SHALL expose focus-membership changes (`entered_focus` and
  `left_focus`) alongside changed state, project, milestone, and blocker
  values, including current values for issues that left the resolved focus.
- PR-status mismatch findings SHALL distinguish `done_with_open_pr` from
  `open_with_merged_pr` and include the issue ID, repository, PR number, URL,
  PR state, and merged timestamp when present.
- A deterministic text rendering for terminal use. It SHALL retain finding
  kinds and actionable issue/PR identifiers and state when a group is
  unavailable. JSON SHALL remain machine-readable for unavailable groups.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-013-AC-1 | The report identifies each focus issue with a missing project, missing milestone, or both, and lists the missing fields; completed and canceled issues remain eligible for scope and duplicate checks. | Test |
| FR-013-AC-2 | A milestone with a target date before report date and at least one open focus issue appears as overdue with its open issues; a milestone due today, with no target date, or with only completed/canceled issues does not. | Test |
| FR-013-AC-3 | Linked GitHub PR fixtures produce `done_with_open_pr` and `open_with_merged_pr` as specified; closed-unmerged PRs do not; multiple links remain individually attributable; unavailable URLs/responses produce unavailable status instead of a clean result; no GitHub Issues request occurs. | Test |
| FR-013-AC-4 | An open issue last updated strictly more than 30 days before report time is stale by default, and one at the exact 30-day boundary is not; a positive CLI override changes only the stale threshold, and completed/canceled issues are excluded. | Test |
| FR-013-AC-5 | The cited IR-387..IR-407 title family forms one near-duplicate connected component under the normative token rule, including “Mark hash/digest/pin antipattern as slated for removal”, “Mark hash/digest/pin as slated for removal in agent docs”, and “Document hash/digest/pin tracking as slated for removal”; a fixture below any minimum-token, shared-token-count, or overlap boundary does not match. | Test |
| FR-013-AC-6 | A prior successful report for the same workspace and canonical focus yields changes for differing state, project, milestone, and blocker fields with old/current values; issues created since the baseline and transitions to completed/canceled appear once, and an issue created and then completed appears under both kinds. A different canonical focus does not reuse the baseline. | Test |
| FR-013-AC-7 | No prior snapshot yields an unavailable diff with `no_prior_report`; incomplete Linear reads, failed diff construction, and unreadable/insufficient snapshots do not advance the baseline. A complete report atomically replaces only the matching focus baseline; an unavailable GitHub PR lookup does not prevent baseline advancement when the Linear diff is complete. | Test |
| FR-013-AC-8 | Identical complete records, focus, configuration, and report time yield stable JSON and text findings and ordering; incomplete source evidence marks the affected group unavailable without presenting its findings as complete. | Test |
| FR-013-AC-9 | When a previously focused issue moves to a project outside the current focus, the diff still reports its `left_focus` membership change and old/current project IDs from the union of prior focus IDs and current focus IDs. A changed issue whose current record cannot be fetched makes the diff unavailable and does not advance the baseline. | Test |
| FR-013-AC-10 | The inverted index generates each unique issue pair within the resolved focus that shares at least one normalized content token, counts shared tokens, excludes out-of-focus issues, and applies the three-token and title-overlap thresholds only after candidate enumeration; candidate pairs, connected-component groups, and group members use the specified stable ordering. | Test |
| FR-013-AC-11 | A fixture of 448 focused titles each normalizing to the same single content token generates 100,128 all-candidate pairs and returns `resource_limit` before title-match filtering, with no partial report and no baseline update; 447 such titles yield 99,681 candidates and no duplicate group because the match threshold is not met. | Test |
| FR-013-AC-12 | JSON `issue_refs` contains identifier and title for available issues referenced by scope, overdue, PR, stale, duplicate, and diff findings while retaining their stable UUID fields. Terminal findings use those references and fall back to UUIDs when a current record is unavailable. | Test |
| FR-013-AC-13 | Repeated links to one pull request share one lookup and one unavailable reason; terminal headings state whether each check was complete and how many findings it produced. | Test |

## Dependencies

- **Focus expansion**: [FR-011](FR-011-inline-planner-focus.md)
- **Cache and structured source records**: [FR-004](FR-004-cache-polling.md),
  [FR-005](FR-005-structured-data-boundary.md)
- Linear issue, project milestone, and attachment records; linked GitHub PR
  metadata; local SQLite persistence for one diff baseline per workspace and
  canonical focus.
