---
id: FR-045
title: Report recently closed tickets and linked pull requests for a focus
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-004"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-012"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-016"
    type: depends_on
provenance:
  source: SWM-297
description: Focus status reports tickets completed inside a time window and the GitHub pull requests Linear has attached, so a dashboard can show recent merges without calling GitHub.
dependencies:
  - FR-012
  - FR-016
---
# FR-045: Report recently closed tickets and linked pull requests for a focus

## Description

`focus status`, and therefore `focus report`, SHALL report which focus tickets completed recently and which GitHub pull requests Linear has linked to focus tickets. A dashboard reads merges within a focus from the Linear snapshot alone and makes no GitHub call.

## Behavior

### Window

`--closed-since <duration>` sets the window, using the duration syntax of `agents --idle-after` (seconds, minutes or hours, for example `90m` or `48h`). The default is `48h`. The window ends at the time of the report and starts that duration earlier. A timestamp is inside the window when it is not earlier than the start and not later than the end.

### Issue facts

Every `FocusIssue` carries `assignee` (the assignee name or null), `started_at` and `completed_at` (the Linear timestamps or null) and `prs`, always present and empty when the issue has none.

A `prs` entry has `url`, `repo` (the repository name without the owner), `number`, `status` (`open`, `draft`, `merged`, `closed`, or `unknown`), `draft`, `created_at` and `merged_at`. Only GitHub pull-request attachments produce entries. `status`, `draft`, `created_at`, `merged_at`, `number` and `repo` come from the attachment metadata Linear stores; when the metadata is absent or does not name them, `repo` and `number` are parsed from the pull-request URL and `status` is `unknown`. An attachment list Linear truncated still produces the entries it returned; the truncation only keeps the existing incomplete-attachment warnings.

### Recently closed

Each project, each milestone and the no-milestone group carries `recently_closed`: the focus issues of that group whose workflow-state type is `completed` and whose `completed_at` is inside the window, newest completion first, at most 50.

### Activity

`FocusStatus` carries `activity`, a list of events on focus issues, newest first, at most 200. A `merged` event is a linked pull request's `merged_at`, an `opened` event is its `created_at`, and a `closed` event is a completed issue's `completed_at`; each is included only when inside the window. An event has `at`, `kind`, the issue `identifier` and `title`, the `project` name, and `pr` (`url`, `repo`, `number`) for `merged` and `opened` events and null for `closed`. Events with the same kind, issue identifier and pull-request URL appear once.

### Output

Terminal output lists, under each project, a "Closed in window" section with each closed issue's identifier and title, and `repo#number` of its merged pull request.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-045-AC-1 | `recently_closed` for a project, a milestone and the no-milestone group holds exactly the completed issues of that group with `completed_at` inside the window; issues outside it, not completed, or canceled are omitted, and a narrower `--closed-since` shrinks the list. | Test |
| FR-045-AC-2 | `recently_closed` is ordered by `completed_at`, newest first. | Test |
| FR-045-AC-3 | With more than 50 qualifying issues `recently_closed` holds the 50 newest, and `activity` holds at most 200 events. | Test |
| FR-045-AC-4 | `activity` holds `merged`, `opened` and `closed` events only when their timestamp is inside the window, newest first, with the project name (the FR-046 text for an issue outside every project row), and `pr` null for `closed`. | Test |
| FR-045-AC-5 | Duplicate attachments of one pull request, and any other repeat of the same kind, issue and pull-request URL, produce one event. | Test |
| FR-045-AC-6 | Each `FocusIssue` serializes `assignee`, `started_at`, `completed_at` and `prs`, with `prs` an empty array when there are none. | Test |
| FR-045-AC-7 | A GitHub pull attachment's metadata sets `status` (`merged`, `draft`, `open`), `draft`, `created_at`, `merged_at`, `number` and `repo`. | Test |
| FR-045-AC-8 | A pull-request attachment without metadata gives `repo` and `number` from its URL and status `unknown`; non-pull GitHub URLs and non-GitHub attachments give no entry. | Test |
| FR-045-AC-9 | An issue whose attachments are truncated (`attachments_complete` false) still lists the pull requests it returned. | Test |
| FR-045-AC-10 | `--closed-since` defaults to 48 hours on `focus status` and `focus report`, accepts a duration, and the terminal output lists closed issues with the merged `repo#number`. | Test |

## Dependencies

- [FR-012](./FR-012-focus-status-and-ready-queue.md) focus status; [FR-016](./FR-016-focus-record.md) focus record, which embeds this status.
