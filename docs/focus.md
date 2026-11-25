# Focus

A focus is the part of a workspace a report covers. You select it on the command
line with initiatives, projects, milestones and issues. ix-board resolves the
selection to a set of issues and the blockers they depend on, then builds a
report from that set.

## Selectors

Every focus command accepts the same repeatable flags.

| Flag | Accepts | Selects |
| --- | --- | --- |
| `--initiative` | UUID, identifier such as `I-10`, or exact name | The initiative's projects |
| `--project` | UUID, identifier, or exact name | All issues in the project, with or without a milestone |
| `--milestone` | UUID, globally unique name, or `<project>/<milestone>` | Issues assigned to that milestone |
| `--issue` | UUID or identifier | The issue and its descendants |

Selectors are combined as a union and deduplicated. Names match without regard
to case. A project UUID or identifier takes precedence over a project that has
the same name. A milestone name that occurs in more than one project fails and
lists the candidates; qualify it as `<project identifier or name>/<milestone
name>` to choose one. A slash may appear inside either name. A selector that
matches nothing, or an empty focus, is an error and produces no partial report.

Parent and child issues are related by parent links, which are separate from
blocker relations. A selected issue includes its descendants.

## Blocker layers

For every issue in the focus, ix-board follows the full transitive `blocked-by`
graph. The closure includes blockers from other projects and teams, and blockers
that are completed or canceled. Layer 0 holds issues without blockers. Every
other issue is one layer above its deepest blocker. Blockers that are not part of
the focus stay visible and are marked as blocker-only. A blocker without a record
has no row (see [unavailable references](#unavailable-references)). A dependency
cycle is an error.

## Unavailable references

A snapshot can hold an issue that points at a record the snapshot does not hold,
such as an archived parent or blocker, or a milestone that its project does not
list. The read succeeds, keeps the issue in its counts, and adds one warning for
each such reference.

| Warning kind | Effect on the issue |
|--------------|---------------------|
| `unavailable_milestone` | Counted at project level, with no milestone |
| `unavailable_project` | Not ready; counted outside every project row |
| `unavailable_parent` | Ancestry ends at the issue |
| `unavailable_blocker` | The issue and its dependents are not ready |

```json
"warnings": [
  {
    "kind": "unavailable_milestone",
    "issue": "ENG-44",
    "milestone": "1b6c…",
    "project": "8f20…"
  }
]
```

The `warnings` member is omitted when there is nothing to report, and terminal
output lists the same entries under `Warnings:`. `status` counts the selected
issues that no project row holds, including issues without a project, in
`outside_projects`, so project rows plus that group equal the whole focus.
`next` prints its warnings to standard error. A parent cycle or a dependency
cycle is still an error.

A milestone created recently can be reported as unavailable until the next full
reconciliation of the cache, because an incremental refresh reads only the
projects that Linear marks as updated.

## Work milestones

`--work-milestone` narrows the work that `focus status` and `focus plan-chunk`
consider, without changing which issues are in the focus. It accepts an exact
milestone ID or name among the selected projects and can be repeated. Blocker
roots and ready issues are limited to issues in those milestones. The full
prerequisite closure of those issues stays visible, and progress counts keep the
whole focus as their denominator. A value that matches no milestone in the
focus fails before any report is printed.

Use `--milestone` when the milestone belongs in the saved focus. Use
`--work-milestone` for a temporary view of a broader focus.

## Reports

### status

```bash
ix-board focus status --initiative "Initiative name"
```

For each selected project and each milestone, `status` reports the target date,
leaf progress, the open issue count by state, and the issues in progress. It
then lists blocker layers with each blocker's state and the focus issues that
depend on it, followed by the ready queue for the focus.

- Progress counts leaf tasks. Completed leaves count as done, canceled and
  duplicate leaves are excluded, and parent issues are not counted. A group with
  no leaves has a null percentage.
- Open counts by state include parent issues.
- A project with some of its issues outside the focus is labeled
  `(selected focus only)`.
- The ready queue applies the [readiness rules](how-it-works.md#readiness) to
  focus issues only.
- `--closed-since <duration>` (default `48h`; units s, m, h) sets the window for
  recent activity. Each project, milestone and no-milestone group lists
  `recently_closed` issues (completed inside the window, newest first, at most
  50), and the top-level `activity` list holds merged, closed and opened events
  (newest first, at most 200, one per kind, issue and pull request). Every
  focus issue carries `assignee`, `started_at`, `completed_at` and `prs`: the
  GitHub pull requests Linear has attached, with `repo`, `number`, `status`
  (`open`, `draft`, `merged`, `closed`, or `unknown` when Linear gave no
  metadata), `draft`, `created_at` and `merged_at`. A ticket whose attachments
  Linear truncated still reports the pull requests it returned. `focus report`
  accepts the same flag.

Example output, with invented names:

```text
Focus snapshot fetched at 2026-10-28T09:12:03Z (2 min ago)
Project ENG-P1 — Billing rewrite
  Target date: 2026-11-30
  Progress: 6/14 leaf tasks, excluding parents (42.9%)
  Open by state, including parents:
    Backlog: 3
    Coding: 2
  In progress:
    ENG-41 [Coding] Export invoices
  Milestone M1 Ledger — 1b6c…
    Target date: 2026-11-01
    Progress: 5/8 leaf tasks, excluding parents (62.5%)
Blocker layers:
  0:
    ENG-17 [Coding / Open] roots: ENG-41 — Ledger schema
Ready focus issues:
  ENG-44 — Tax rules (project ENG-P1 is Started; …)
```

### hygiene

```bash
ix-board focus hygiene --project "Project name" --stale-days 30
```

`hygiene` checks every issue in the focus, including completed ones, and prints
five groups of findings and a diff.

| Group | Finding |
| --- | --- |
| Missing scope | An issue with no project, no milestone, or neither |
| Overdue milestones | A milestone whose target date has passed and that still has open issues |
| Pull requests | `done_with_open_pr` or `open_with_merged_pr`, from GitHub pull requests linked by Linear attachments |
| Stale issues | An open issue not updated for more than `--stale-days` days (default 30) |
| Near-duplicate titles | Issues whose titles share most of their content words |

The near-duplicate check is a review signal. It lowercases titles, drops a fixed
list of stop words, and groups issues that share at least three words and at
least 75% of the smaller title's words. It never changes an issue.

Each group prints `checked` or `incomplete`. A group is incomplete when a source
it needs is missing, for example when `GITHUB_TOKEN` is not set for the pull
request check. An incomplete group never means that nothing was found.

The diff compares the focus with the previous successful report for the same
workspace and selectors. It lists issues that entered or left the focus, were
created or closed, and changed state, project, milestone or blockers. The first
run has no baseline, so the diff is `unavailable` with reason `no_prior_report`.
ix-board keeps only the latest baseline and replaces it only after a complete
read.

### plan-chunk

```bash
ix-board focus plan-chunk --initiative "Initiative name" --size 5
```

`plan-chunk` lists the open issues of the focus in dependency order, for handing
a block of work to the next lead. Rows are ordered by blocker layer, then project
order, then milestone order within the project (issues without a milestone
last), then issue identifier. `--size` limits the number of rows; without it the
chunk holds every open focus issue.

Each row shows the issue, its layer, its milestone, whether it is ready and why,
the open focus issues it unblocks, and an acceptance-criteria excerpt. The
excerpt is the first 280 characters under the first heading named `Acceptance
Criteria`, or null when there is none. Rows for issues that are not ready, such
as one in `Todo`, say so and give the reason.

### report

```bash
ix-board --json focus report --initiative "Initiative name"
```

`report` prints a versioned JSON record (`FocusRecord`) with the canonical
selection as typed Linear IDs, the report time, and the full `focus status`
result, all computed from one snapshot. Another client can validate and store
the record. The command does not send the record anywhere.

### expand

`focus expand` prints the resolved projects, milestones, issues and blocker
layers without status or readiness, which is useful for checking what a set of
selectors covers.

Library consumers that store selections with exclusions can use named
selections and bounded reads; see [named selections](named-focus.md).
