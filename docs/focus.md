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
the focus stay visible and are marked as blocker-only. A missing blocker record
or a dependency cycle is an error.

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
