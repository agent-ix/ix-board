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
  fleet          Fleet lane commands
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
      --closed-since <CLOSED_SINCE>  Report closures and PR events inside this window (for example 90m, 48h) [default: 48h]
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
      --closed-since <CLOSED_SINCE>
          Report closures and PR events inside this window (for example 90m, 48h) [default: 48h]
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

Exit status: zero when every machine was read. When some machines are missing (they time out after 45 seconds or cannot be read) but other machines returned rows, the command prints those rows, lists the missing machines in `missing_hosts` with reason and elapsed time, and exits with the partial code. When no row could be read it exits with the internal-error code.

## ix-board fleet status

```text
Classify every registered agent lane from relay, Prometheus and order evidence

Usage: ix-board fleet status [OPTIONS]
```

Emits one row per registered lane with one class: `WORKING`, `IDLE`, `BLOCKED(reason)`, `LOOP`, `STUCK`, `STALE-ORDER`, `DEAD` or `UNREACHABLE` (FR-034). Relay data (registry, activity, receipts) is read from Redis through relay-core using the standard ix-relay settings; watcher series are read from the Prometheus HTTP API at `IX_PROMETHEUS_URL` or `[fleet] prometheus_url`. The working window, `max_tool_runtime` and `no_events_after` come from the relay `ix.fleet.config` series and cannot be set locally. Without a Prometheus URL, or when it cannot be read, hook evidence still classes the lanes with the documented relay defaults (working window 10m, `max_tool_runtime` 2h, `no_events_after` 30m); those rows carry `thresholds_defaulted` and one defect names the remedy. The output also lists lanes total, working and not working (UNREACHABLE counts as not working) per host. Weak signals from herdr are not wired yet (SWM-264). `no_events` fires only for a lane whose newest event is not `stop`, `idle`, `session_end` or `blocked`. A defect `observed_ms` is the run time; `first_seen_ms` is the relay time of the run that first saw it, read from the fleet ledger (see below), and is absent when the ledger could not be written or the defect was first seen in a run whose relay could not be read. `[fleet]` also accepts `lanes`, `hosts`, `loop_stops`, `dead_after_secs`, `max_clock_skew_secs`, `push_interval_secs`, `host_timeout_secs` and the ledger keys listed under `ix-board fleet ledger query`.

Exit status: 0 when no source defect exists. Any defect (`missing_lane`, `missing_host`, `no_events`, `stale_source`, `clock_skew`, `unreadable_source`, `stale_host_registry`, `relay_activity_absent`, `lane_registry_absent`, `host_lane_list_absent` (the host's lanes are UNREACHABLE), `host_hook_events_absent`, `series_absent`, `series_naming_unverified`, `ledger_unwritable`) sets `complete` to false and exits 1 (ix-cli-kit partial). When no row could be produced it exits 4 (internal error). A refused `[fleet]` configuration exits 3. `STALE-ORDER` reads the relay's expired un-acted orders (`Relay::expired_orders`, ix-relay FR-067, FR-071), per lane address; the relay applies the order deadline and says whether a `Delivered` receipt came before expiry, so an order without one is `timed_out`. A list that cannot be read is an `unreadable_source` defect on `relay.receipts`, as is an expired order whose recipient has no registered lane address (one defect per recipient, listing the order ids), a `truncated` list (qualifying orders may be missing), rows the relay `skipped` because it could not decode them, and an expiry index the relay has not started. An expiry index the relay has not started (no `indexed_since_ms`) is the same kind of defect.

## ix-board fleet tick

```text
Evaluate the fleet once and emit the minder's action list; records the tick in the ledger

Usage: ix-board fleet tick [OPTIONS]

Options:
      --session <SESSION>  Address of the session running the tick, recorded with it
```

`fleet tick --json` reads `fleet status` internally and prints a deterministic, ordered action list (FR-038): `push_next_ticket`, `answer_dialog`, `escalate_block`, `escalate_stuck`, `resend_order`, `add_lane`, `escalate_stale_pr`, `act_on_undetermined`, `ladder_rung`, `escalate_drain`, `fix_source`, `route_dead_lane` and `route_idle_lane`, each with `id`, `target`, `reason`, `severity`, `due_by`, `condition_started_at`, evidence and kind-specific `detail`, sorted by severity, subject and kind. An action id stays the same while its condition persists. Each tick commits one `tick` row, one `lane_tick` row per lane (source `tick`) and the `action` rows to the ledger; it makes no relay, Linear or GitHub write. A tick that cannot evaluate (the relay cannot be read) prints an error, exits 4 and commits nothing; a tick with source defects exits 1 after printing the list.

The in-fleet ladder: a condition's own action is rung 0, sent in the tick that first emits it. While it persists the tick emits `ladder_rung` actions to the worker (rung 1), the lane's lead (rung 2) and the orchestrator (rung 3) in the first tick at or after 1, 2 and 3 tick intervals. A lane in BLOCKED never gets rung 1. A lane with no configured lead makes the orchestrator the owner (reason `lead_unresolved`) and rung 3 goes out with rung 2's time. The tick never addresses the owner.

Settings come from the `[fleet_tick]` table: `stale_pr_after_secs` (default 3h), `drain_cap_secs` (4h), `orchestrator`, `minder`, `standing_orders_path`, `default_busy_target` (0.85), and the tables `leads` (lane to lead address), `busy_targets` and `lane_capacity` (host to value). `tick_interval_secs` stays under `[fleet]`.

Sources: the FR-072 assignment lists are read through relay-core; a list that fails to read is a source defect (`tick.assignments`) routed as `fix_source` in the same tick. Host busy ratio (SWM-281), lane pull-request activity (SWM-282) and ready unassigned work (SWM-212) have no reader yet; each prints under `unavailable` as a named seam, raises no defect, and its actions (`add_lane`, `escalate_stale_pr`) are not emitted. `need_ready_tickets` is deferred to SWM-217. A source defect becomes `fix_source` in the tick in which it first appears. `act_on_undetermined` goes to the lead, not the lane, when the lane is BLOCKED. A DEAD lane and an IDLE lane with an empty FR-072 list emit `route_dead_lane` and `route_idle_lane` to the orchestrator. Without `--session` the caller is resolved through the relay from `HERDR_PANE_ID` and this machine's registration. Each tick first completes the pending `fleet assign` rows, the same step `fleet status` takes, and sets `executed_at` of a `push_next_ticket` when the lane works with its derived focus on the pushed ticket. A defect belongs to the run that reads its source: `fleet status` never closes the `tick.*` defects a tick raises. `answer_dialog` depends on the SWM-226 standing-order file: its machine-readable format is not specified, so a readable file lets the tick emit `answer_dialog` for a BLOCKED(dialog) or a `queued_question` with the relay reason and no order id, and a missing or unreadable file emits `escalate_block`. The exporter (`ix-team-exporter`) serves `ix_fleet_tick_timestamp_seconds` and `ix_fleet_tick_actions{kind}` from the last committed tick when its configuration sets `fleet_ledger`, and omits them when that tick is older than twice `fleet_tick_interval_seconds`.

## ix-board fleet assign

```text
Send a lane its ordered tickets as a relay order, store the list, and wait for the outcome

Usage: ix-board fleet assign [OPTIONS] <LANE> <TICKETS>...

Arguments:
  <LANE>        Lane address server:workspace:tab
  <TICKETS>...  Ticket identifiers in order; the first is the expected ticket

Options:
      --deadline <DEADLINE>  Acknowledgement deadline (for example 90s, 5m) [default: 5m]
      --json                 Emit machine-readable JSON on stdout instead of human-readable text
  -h, --help                 Print help
```

Reads each ticket (identifier, title, state, description with its acceptance criteria) from the Linear cache, sends the exact text to the lane as one relay order with the acknowledgement deadline and `--expect-ticket` set to the first ticket, and only after the relay accepts the order writes the ordered list for the lane address through the ix-relay assignment list (FR-072), carrying the order id. A later `assign` replaces the list. Nothing is written to `PLAN-*.md`.

The caller needs the orchestrator role: `[assign] orchestrator = true` in the ix-relay `config.toml`. The command refuses, before any send: tickets that are unreadable, Done, Canceled, Duplicate, merged or named twice; a ticket another lane already holds (the lane is named); the caller's own lane; an unknown lane; a lane on a host whose registry lease expired (`host_registry_stale`); a lane without exactly one registered session (`no_recipient`); a first ticket id over 64 bytes; an order body over the relay limit of 4096 bytes (assign fewer tickets) or holding an unsafe character.

The command waits until the deadline and prints one result. `confirmed`: the lane's focus, derived from the branch or worktree directory of its hook events after the order was queued (FR-037), is the first ticket before the deadline. `acted_not_focused`: the session acted but its focus is another ticket. `ack_expired`: delivered, no action by the deadline. `timed_out`: never delivered. The report carries `order_id`, `expect_ticket`, `sent_at_ms`, `deadline_at_ms`, `result`, `result_at_ms`, `confirm_latency_s` (whole seconds; absent unless confirmed), `focus_at_deadline`, `list_version`, `previous_tickets`, `list_write_error` and `defects`. Relay reads that fail while waiting are retried and reported as a `relay_read_errors` defect.

Each assignment is one `assignment_history` row in the fleet ledger: written `pending` before the send, completed with the order, and completed with the result. `fleet status` completes rows left `pending` by a killed command and fills `focus_at_deadline` of rows confirmed before their deadline. `[fleet]` accepts `team_keys` (Linear team keys read from branch and directory names; the keys of the assigned tickets are always read) and `focus_window_secs` (default 600).

Exit status: 0 for `confirmed`. 1 for `acted_not_focused`, `ack_expired` or `timed_out`. 3 for a refusal of the request or configuration. 4 for an operational failure: the relay cannot be connected, the send fails, the list write fails after the accepted send (the report names the order), or a ledger write fails (`ledger_unwritable`).

## ix-board fleet ledger query

```text
Print the rows of one ledger view without writing

Usage: ix-board fleet ledger query [OPTIONS] <VIEW>

Arguments:
  <VIEW>  View name, for example idle_minutes or defect_rows

Options:
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --since <SINCE>                Only rows within this window before the newest run (for example 90s, 20m, 4h)
```

`fleet status` appends each run to a schema-versioned SQLite ledger (FR-039): a run record, one `lane_tick` row per lane (class, reason, evidence, relay time of the run), per-host lane counts, and the defects with their first-seen and close times. `fleet ledger query` reads it without writing, locking or migrating. Queryable views: `idle_minutes` (lane, host, day, minutes per non-WORKING class and minutes while ready work existed), `idle_lane_with_ready_work_ticks`, `lanes_with_ready_work`, `idle_lane_episodes_with_ready_work`, `idle_lane_episode_with_ready_work_max` and `defect_rows`. `--json` prints `{"view", "since_ms", "rows"}`; text prints one `column=value` line per row. `--since` is measured back from the newest run on the relay clock, never the local clock, and views without a time column (`idle_lane_episode_with_ready_work_max`) refuse it. Ready-work columns stay empty until `fleet tick` writes them. All stored times are relay server time; a run whose relay could not be read stores the host clock in `host_ms` with `relay_unavailable`, writes no lane rows, closes no defects and prunes nothing.

The ledger lives at `IX_BOARD_LEDGER_PATH`, else `[fleet] ledger_path`, else `$XDG_STATE_HOME/ix-board/fleet-ledger.sqlite3` (`~/.local/state/ix-board/fleet-ledger.sqlite3`). A path inside a git work tree, or on a network filesystem (NFS, SMB/CIFS, 9p, a WSL `/mnt/<drive>` path, a Windows UNC share), is refused. Writers take an exclusive lock on `<ledger>.lock` and wait up to `ledger_lock_wait_secs`; on Windows a lock file whose process has exited is broken. A ledger of a newer schema version is refused without a change; a version 1 ledger migrates to version 2 in one transaction. `[fleet]` ledger keys: `ledger_path`, `ledger_retention_days` (default 30; closed and point-in-time rows older than this are pruned, open rows never), `ledger_lock_wait_secs` (default 10), `tick_interval_secs` (default 300), `cycle_interval_secs` (default 600) and `grace_secs` (default cycle plus tick). The tick and cycle intervals and grace are stored on each run, so a changed value applies to later runs only.

When the ledger cannot be opened or written, `fleet status` still prints the report and adds one `ledger_unwritable` defect carrying the reason (exit 1, or 4 when no row was produced). An unknown view, a missing ledger or `--since` on an untimed view exits 3.

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

## ix-board retro snapshot

Measures a team's convergence and appends one record to its retro store. See
[retro snapshot](retro.md) for the configuration file and the metric catalog.

```text
ix-board retro snapshot [OPTIONS] --config <CONFIG>

Options:
      --config <CONFIG>  JSON retro configuration file
      --window <WINDOW>  Trailing window: seconds, minutes, hours or days (90s, 20m, 4h, 2d) [default: 24h]
      --json             Emit the stored record as JSON on stdout
```

The command exits zero when it appends a record, even if some metrics are unavailable; their
ids are listed on stderr. It exits non-zero and appends nothing when the configuration is
invalid or the store path is refused.

## ix-board retro delta

Compares the latest snapshot with the previous one and the baseline, reading only the store. See
[delta](retro.md#delta).

```text
ix-board retro delta [OPTIONS] --config <CONFIG>

Options:
      --config <CONFIG>              JSON retro configuration file
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

With fewer than two snapshots the command reports that no delta exists and exits zero. A
`baseline_taken_at` that matches no record stops it with an error naming the value.

## ix-board retro backfill

Appends one record per UTC day from Linear and GitHub history. See [backfill](retro.md#backfill).

```text
ix-board retro backfill [OPTIONS] --config <CONFIG> --since <SINCE>

Options:
      --config <CONFIG>              JSON retro configuration file
      --since <SINCE>                First UTC day, YYYY-MM-DD
      --until <UNTIL>                Last UTC day, YYYY-MM-DD; defaults to the last complete UTC day
      --window <WINDOW>              Trailing window for window metrics: seconds, minutes, hours or days (90s, 20m, 4h, 2d) [default: 24h]
      --json                         Emit machine-readable JSON on stdout instead of human-readable text
      --diagnostics-format <FORMAT>  Diagnostic stream format on stderr [default: human]
      --color <WHEN>                 Colorize human diagnostics on stderr: auto (TTY only), always, or never [default: auto]
  -h, --help                         Print help
```

The command refuses a range that contains a `snapshot` record, skips days it already wrote, and
refuses a day that has not ended.

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
