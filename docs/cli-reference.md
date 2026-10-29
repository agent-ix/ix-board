# CLI reference

Generated from the `--help` output of `ix-board` 0.4.0. Run `ix-board <command> --help` for the same text.

## ix-board

```text
Deterministic Linear project controls and cached reads for ready work and project context.

Usage: ix-board [OPTIONS] <COMMAND>

Commands:
  auth           Manage the Linear API key in the operating system credential store
  next           Show the deterministic ready queue for one Linear team
  project-chunk  Return a project's goal, milestones, open tickets, and blocker layers
  search         Search structured project and issue fields
  issues         Read multiple exact Linear issues in one cached workspace request
  agents         Say whether each agent in a team's herdr workspaces is working right now
  drift          Render a deterministic drift report from versioned structured evidence
  focus          Resolve an inline Linear initiative/project/issue focus
  project        Update one of the two supported Linear project fields
  scope          Validate a program scope file against Linear and GitHub
  roadmap        List initiatives and projects with current progress and issue counts
  reconcile      Compare a project's Linear state with its linked GitHub pull requests and branches
  chain          Expand blocker chains from one issue or every open ticket in a program scope
  orphans        List unprojected open and completed tickets in the scope's project teams
  help           Print this message or the help of the given subcommand(s)

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
  -V, --version                      Print version
```

## ix-board auth

```text
Manage the Linear API key in the operating system credential store

Usage: ix-board auth [OPTIONS] <COMMAND>

Commands:
  login   Save a Linear API key; prompt with masked feedback by default
  status  Report the active key source without displaying the key
  logout  Remove the saved key from the OS credential store
  help    Print this message or the help of the given subcommand(s)

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board auth login

```text
Save a Linear API key; prompt with masked feedback by default

Usage: ix-board auth login [OPTIONS]

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --stdin                        Read the key from stdin for scripts instead of prompting
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board auth status

```text
Report the active key source without displaying the key

Usage: ix-board auth status [OPTIONS]

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board auth logout

```text
Remove the saved key from the OS credential store

Usage: ix-board auth logout [OPTIONS]

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board next

```text
Show the deterministic ready queue for one Linear team

Usage: ix-board next [OPTIONS] --team <TEAM>

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --team <TEAM>                  Linear team key
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board project-chunk

```text
Return a project's goal, milestones, open tickets, and blocker layers

Usage: ix-board project-chunk [OPTIONS] <PROJECT>

Arguments:
  <PROJECT>  Linear project identifier, UUID, or exact name

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board search

```text
Search structured project and issue fields

Usage: ix-board search [OPTIONS] <QUERY>

Arguments:
  <QUERY>  Case-insensitive substring query

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board issues

```text
Read multiple exact Linear issues in one cached workspace request

Usage: ix-board issues [OPTIONS] <IDENTIFIERS>...

Arguments:
  <IDENTIFIERS>...  Linear issue identifiers or UUIDs, in desired output order

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board focus

```text
Resolve an inline Linear initiative/project/issue focus

Usage: ix-board focus [OPTIONS] <COMMAND>

Commands:
  report      Emit a versioned focus record suitable for shared storage and dashboards
  expand      Expand selectors into focus issues and prerequisite layers
  status      Report project and milestone status, blockers, and ready focus issues
  plan-chunk  Prepare a bounded dependency-ordered handoff chunk for a focus
  hygiene     Report hygiene findings and changes for a focus
  help        Print this message or the help of the given subcommand(s)

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board focus report

```text
Emit a versioned focus record suitable for shared storage and dashboards

Usage: ix-board focus report [OPTIONS]

Options:
      --initiative <INITIATIVES>
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --project <PROJECTS>
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
      --milestone <MILESTONES>
      --issue <ISSUES>
  -h, --help                         Print help
```

## ix-board focus expand

```text
Expand selectors into focus issues and prerequisite layers

Usage: ix-board focus expand [OPTIONS]

Options:
      --initiative <INITIATIVES>     Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --project <PROJECTS>           Linear project UUID, identifier, or exact name; may be repeated
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
      --milestone <MILESTONES>       Linear milestone UUID or exact globally unique name; may be repeated
      --issue <ISSUES>               Linear issue UUID or identifier; may be repeated
  -h, --help                         Print help
```

## ix-board focus status

```text
Report project and milestone status, blockers, and ready focus issues

Usage: ix-board focus status [OPTIONS]

Options:
      --initiative <INITIATIVES>
          Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated
      --json
          Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>
          Diagnostic stream format on stderr [default: human]
      --project <PROJECTS>
          Linear project UUID, identifier, or exact name; may be repeated
      --color <WHEN>
          Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
      --milestone <MILESTONES>
          Linear milestone UUID or exact globally unique name; may be repeated
      --issue <ISSUES>
          Linear issue UUID or identifier; may be repeated
      --work-milestone <WORK_MILESTONES>
          Limit blocker roots and ready issues to these exact milestone IDs or names; repeatable
  -h, --help
          Print help
```

## ix-board focus plan-chunk

```text
Prepare a bounded dependency-ordered handoff chunk for a focus

Usage: ix-board focus plan-chunk [OPTIONS]

Options:
      --initiative <INITIATIVES>
          Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated
      --json
          Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>
          Diagnostic stream format on stderr [default: human]
      --project <PROJECTS>
          Linear project UUID, identifier, or exact name; may be repeated
      --color <WHEN>
          Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
      --milestone <MILESTONES>
          Linear milestone UUID or exact globally unique name; may be repeated
      --issue <ISSUES>
          Linear issue UUID or identifier; may be repeated
      --work-milestone <WORK_MILESTONES>
          Limit handoff roots to these exact milestone IDs or names; repeatable
      --size <SIZE>
          Maximum number of open focus issues to emit
  -h, --help
          Print help
```

## ix-board focus hygiene

```text
Report hygiene findings and changes for a focus

Usage: ix-board focus hygiene [OPTIONS]

Options:
      --initiative <INITIATIVES>     Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --project <PROJECTS>
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
      --milestone <MILESTONES>
      --issue <ISSUES>
      --stale-days <STALE_DAYS>      Stale threshold in positive whole days [default: 30]
  -h, --help                         Print help
```

## ix-board project

```text
Update one of the two supported Linear project fields

Usage: ix-board project [OPTIONS] <COMMAND>

Commands:
  set-status    Set the project run state to Started or Paused
  set-priority  Set the project priority to a Linear priority value
  help          Print this message or the help of the given subcommand(s)

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board project set-status

```text
Set the project run state to Started or Paused

Usage: ix-board project set-status [OPTIONS] <PROJECT> <STATUS>

Arguments:
  <PROJECT>  Linear project identifier or UUID
  <STATUS>   One of: Started, Paused

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board project set-priority

```text
Set the project priority to a Linear priority value

Usage: ix-board project set-priority [OPTIONS] <PROJECT> <PRIORITY>

Arguments:
  <PROJECT>   Linear project identifier or UUID
  <PRIORITY>  0=No priority, 1=Urgent, 2=High, 3=Medium, 4=Low

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board scope

```text
Validate a program scope file against Linear and GitHub

Usage: ix-board scope [OPTIONS] <COMMAND>

Commands:
  check  Check listed records and report unplaced projects
  help   Print this message or the help of the given subcommand(s)

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board scope check

```text
Check listed records and report unplaced projects

Usage: ix-board scope check [OPTIONS] --scope <SCOPE>

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --scope <SCOPE>                TOML program scope file
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board roadmap

```text
List initiatives and projects with current progress and issue counts

Usage: ix-board roadmap [OPTIONS]

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --scope <SCOPE>                Restrict the report to the initiatives and projects in this scope file
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board orphans

```text
List unprojected open and completed tickets in the scope's project teams

Usage: ix-board orphans [OPTIONS] --scope <SCOPE>

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --scope <SCOPE>                TOML program scope file that defines the projects and their teams
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board chain

```text
Expand blocker chains from one issue or every open ticket in a program scope

Usage: ix-board chain [OPTIONS] [ISSUE]

Arguments:
  [ISSUE]  Linear issue identifier or UUID

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --scope <SCOPE>                Program scope file; when issue is omitted, expands all open project tickets in it
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board reconcile

```text
Compare a project's Linear state with its linked GitHub pull requests and branches

Usage: ix-board reconcile [OPTIONS] <PROJECT>

Arguments:
  <PROJECT>  Linear project identifier, UUID, or exact name

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --scope <SCOPE>                Program scope file whose repositories should be searched for branches
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --stale-days <STALE_DAYS>      Staleness threshold in positive whole days [default: 14]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board agents

```text
Say whether each agent in a team's herdr workspaces is working right now

Usage: ix-board agents [OPTIONS] <TEAMS>...

Arguments:
  <TEAMS>...  Team names: herdr workspace labels, on any machine

Options:
      --idle-after <IDLE_AFTER>      Quiet time after which a ready agent counts as idle (for example 90s, 20m, 2h) [default: 20m]
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

## ix-board drift

```text
Render a deterministic drift report from versioned structured evidence

Usage: ix-board drift [OPTIONS] --config <CONFIG>

Options:
      --config <CONFIG>              JSON file containing thresholds and configured leads
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --evidence <EVIDENCE>          JSON file containing source records and completeness attestations
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
      --live                         Collect issue and state-history evidence directly from Linear
  -h, --help                         Print help
```

## JSON Schema export

The public wire types derive JSON Schema from their Rust definitions. Export one
from a clone of the repository:

```bash
cargo run --example export_schema -- <TYPE> > <type>.schema.json
```

`<TYPE>` is one of `Project`, `ProjectChunk`, `ReadyItem`, `DriftReport`,
`FocusStatus`, `FocusRecord`, `FocusHygieneReport` or `FocusPlanChunk`. The
schemas describe the serialized shape, including flattened availability fields
and enum values. Downstream projects can generate TypeScript declarations from
them without copying the Rust models.
