# Program review

Program review commands compare a program's plan with the live state of Linear
and GitHub. They share a TOML scope file and are read-only.

## Scope file

The scope file names the initiatives, projects, repositories and lanes of a
program. It holds membership and ownership. It does not hold counts, commit
hashes or versions, because those come from live data.

```toml
program = "Billing"
out_of_scope = ["Mobile"]

[[initiatives]]
name = "Billing Core"

[[projects]]
name = "Billing Engine"
primary_initiative = "Billing Core"
also_in = []

[[repos]]
name = "acme/billing"
purpose = "Invoicing service and API"

[[lanes]]
address = "hub:billing:review"
owns = "cross-lane billing review"
```

| Key | Meaning |
| --- | --- |
| `program` | The program name |
| `out_of_scope` | Names of work that is deliberately excluded |
| `[[initiatives]]` | Initiatives in the program, by name |
| `[[projects]]` | Projects in the program. `primary_initiative` is the initiative that lists the project. `also_in` names other initiatives that include it |
| `[[repos]]` | GitHub repositories as `owner/name`, with a one-line `purpose` |
| `[[lanes]]` | Relay addresses in `server:workspace:tab` form and what each lane owns |

Unknown keys are rejected. A project appears once, under its primary
initiative, and its other memberships are shown beside it. Every project in a
reviewed initiative should be in the file, and every in-scope ticket should
belong to a project.

## scope check

```bash
ix-board scope check --scope program.toml
```

`scope check` confirms that each listed initiative and project exists in Linear
and each repository exists on GitHub. It reports projects in the program's
initiatives that the file has not placed. GitHub probes time out after 20
seconds and the error names the repository. The command exits non-zero when the
scope is invalid.

## roadmap

```bash
ix-board roadmap
ix-board roadmap --scope program.toml
```

`roadmap` lists initiatives with their projects. Each project row shows its
status, progress percentage, open, done and canceled counts, and last activity,
all derived from current issues. A shared project is listed once, under its
primary initiative, with its other initiatives noted. The report also names
projects without an initiative and initiatives without projects. With `--scope`,
it covers only the declared initiatives and projects, and it reports projects
missing from the file and mismatches between the file and Linear's initiative
memberships. Without `--scope`, it covers the whole workspace.

```text
Billing Core
  Billing Engine | Started | 62% | open 31 done 52 canceled 4 | last activity 2026-10-27T16:40:11Z
unplaced project: Scratch | Paused | 0%
```

## orphans

```bash
ix-board orphans --scope program.toml
```

`orphans` lists open and completed tickets that have no project, in the teams of
the scope's projects. Canceled and duplicate tickets are left out. Each ticket
gets a suggested project, chosen by matching the ticket title to a project
identifier or name prefix, or else a project of the same team. A warning is
printed when the suggested project is not associated with the ticket's team,
because Linear rejects that move. The command makes no changes.

## chain

```bash
ix-board chain ENG-123
ix-board chain --scope program.toml
```

`chain` follows blockers across teams. With an issue, it expands that issue's
blockers. With `--scope` and no issue, it starts from every open ticket in the
scope's projects. The report lists root blockers, root blockers that have no
assignee or lane, each distinct blocker path with the state, team, assignee and
age of each ticket, and any cycles. A ticket whose blocker records are partly
unavailable keeps its path as partial evidence, and the remaining branches are
still expanded.

Traversal is bounded to 2,000 paths, 2,000 cycles and 100,000 dependency edges.
When a bound is reached, the report adds a warning and does not present the
result as complete.

## reconcile

```bash
ix-board reconcile "Project name" --scope program.toml --stale-days 14
```

`reconcile` compares a project's tickets with GitHub pull requests and
branches. Pull requests come from structured attachment URLs on the tickets;
`--scope` adds the scope's repositories to the branch search. The report has five
groups:

| Group | Meaning |
| --- | --- |
| Merged PR, ticket still open | A linked pull request merged but the ticket is not closed |
| Done without merged PR | A completed ticket with no merged pull request |
| In Progress without PR or branch | A Coding or In Progress ticket older than `--stale-days` with no pull request or matching branch |
| Stale open PR | An open pull request last updated at least `--stale-days` days ago |
| Draft PR with closed ticket | An open draft pull request attached to a closed ticket |

`--stale-days` is a positive whole number and defaults to 14. When a ticket's
attachment list is incomplete, ix-board prints a warning and leaves out the
findings that depend on a complete list.
