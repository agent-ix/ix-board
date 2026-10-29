# Agents and drift

`agents` tells whether each coding agent in a team's Herdr workspaces is working.
`drift` checks whether each configured lead is working on the right tickets and
delivering. Both are read-only.

## agents

```bash
ix-board agents "Team name" "Other team"
ix-board agents "Team name" --idle-after 90s
```

A team name is the label of a Herdr workspace. `agents` reads the local Herdr
session and every enabled machine from `herdr machine list`, and prints one row
for each agent pane in a workspace whose label equals a team name (case
insensitive). A team name with no workspace is listed as missing. A machine that
cannot be read is listed as an error, and the rows from the other machines are
still printed. The command then exits non-zero.

### Verdicts

Each row has one verdict. ix-board evaluates these rules in order and uses the
first that matches.

| Verdict | Rule |
| --- | --- |
| `PROMPT` | Herdr reports the pane as blocked. The row carries the dialog text. |
| `WORKING` | Herdr reports the pane as working. |
| `WORKING` | A subagent transcript was written in the last 10 minutes, or a build or gate process is running under the pane. |
| `WAITING-STUCK` | The pane says it is waiting for background agents, and neither the last turn nor any subagent write is under 15 minutes old. |
| `UNKNOWN` | Herdr reports the state as unknown. The row carries the raw fields. |
| `ASKING` | Herdr reports the pane as idle or done, and the last assistant message ends with a question or asks for permission. The row carries an excerpt. |
| `WORKING` | The last turn or subagent write is younger than `--idle-after`. |
| `IDLE` | None of the above. |

`--idle-after` takes seconds, minutes or hours, such as `90s`, `20m` or `2h`. The
default is `20m`. An `IDLE` row also shows the team's next ready ticket, reading
the team name as a Linear team key. If the queue cannot be read, the verdicts are
unchanged and the failure is reported once.

### Signals

- **Herdr state** from `herdr agent list`: working, idle, done, blocked or
  unknown.
- **Last turn** from the modification time of the harness transcript, found
  through Herdr's session id. Claude Code transcripts are read from
  `~/.claude/projects` and Codex transcripts from `~/.codex/sessions`.
- **Subagents** from their transcripts, counted as live when written in the last
  10 minutes.
- **Child processes** of the pane that are build or gate tools such as `cargo`,
  `make`, `gh`, `pytest` and `npm`.
- **Pane tail** from `herdr agent read`, used for panes that are not working.
- **Git** age of the last commit and of the newest uncommitted file in the pane's
  working directory.

Remote machines are read with one batched `ssh` per machine. Ages are measured
against the clock of the machine that holds the files. A pane whose harness has no
adapter, or whose transcript cannot be found, still gets its Herdr state, tail
and processes. It is marked `partial` and has no turn ages.

```text
TEAM     MACHINE  PANE     AGENT   VERDICT           TURN     SUB  NOTE
Alpha    local    1-2      claude  WORKING             3m      1m  lead [1 live subagents]
Alpha    local    1-3      codex   IDLE               42m       -  coder
    next ready: ALP-31 Parse config
```

## drift

```bash
ix-board drift --config drift-config.json --evidence drift-evidence.json
ix-board drift --config drift-config.json --live --json
```

`drift` evaluates evidence against a configuration and prints one line per lead
and per priority goal. `--evidence` replays a versioned evidence file, which
gives the same report every time. `--live` collects Linear issues and state
history directly. GitHub, Herdr and OpenTelemetry records can only come from an
evidence file, so those signals are unavailable in live mode and the report says
so. ix-board does not guess attribution from a shared GitHub account and does
not call a language model.

### Configuration

The config is a strict JSON object.

| Field | Meaning |
| --- | --- |
| `report_time` | RFC 3339 time of the report |
| `window_hours` | Length of the reporting window, positive |
| `stall_hours` | Hours in Coding without activity before a ticket counts as stalled, positive |
| `spend_threshold` | Spend that counts as activity, nonnegative |
| `max_net_ticket_growth` | Created minus completed tickets above which churn is flagged |
| `min_merged_prs_for_churn` | Merged pull requests below which growth counts as churn |
| `leads` | Ordered list of `{ "lead_id": "...", "team_key": "..." }`. A team belongs to one lead. |
| `priority_goals` | Optional goals, described below |

### Signals and alerts

For each lead, ix-board computes five signal groups over the reporting window:

| Signal | Meaning |
| --- | --- |
| PR scope | New pull requests attributed to the lead, in scope or out of scope of the lead's `Started` projects |
| Veering | Coding transitions that were outside the ready queue at that time, or that skipped a higher-ranked ready ticket |
| Stall | Tickets in Coding with no pull request or commit for `stall_hours` |
| Busy without PR | Herdr busy or spend above the threshold with no new pull request |
| Ticket churn | Tickets created and completed, merged pull requests, and net growth |

The signals produce two alerts. `out_of_scope` comes from an out-of-scope pull
request or a veering transition. `churning` comes from a stall, from activity
without a pull request, or from net ticket growth above the limit with too few
merged pull requests. A lead's status is `flagged` when an alert is proven,
`on_track` when none is and all evidence was available, and `unknown` when none is
proven but evidence is missing.

### Priority goals

A priority goal reserves a minimum number of workers for a project, milestone or
set of issues.

```json
{
  "goal": "Billing rewrite",
  "ticket_ids": ["ENG-485", "ENG-486"],
  "minimum_workers": 1,
  "no_progress_hours": 12.0
}
```

A goal matches a Linear project or milestone by exact identifier or name. When no
project or milestone applies, `ticket_ids` lists the issues. For each goal the
report gives:

- The last completion and last transition times, and the hours since either.
- `active_workers`: the distinct Linear assignees on active goal issues and their
  open blockers. An assignee counts once, and unassigned or inactive issues add
  nothing. The value is null when issue, membership, blocker or assignee evidence
  is incomplete.
- The current direct and transitive blockers.
- Competing work. Same-team activity in the no-progress interval is reported as
  crowding out. Other-team activity is reported separately, and only when the
  issue is assigned to a worker already counted.

`trusted=false` means a source that could hide relevant work is incomplete, and
the report names that source.

### Evidence file

The evidence file is a versioned JSON envelope.

```json
{
  "schema_version": 1,
  "evidence": {
    "projects": [],
    "tickets": [],
    "transitions": [],
    "pull_requests": [],
    "commits": [],
    "herdr_busy": { "lead-id": false },
    "otel_spend": { "lead-id": 0.0 },
    "missing": []
  }
}
```

Each record is a strict typed struct. The JSON schema for `DriftReport` is
available through [schema export](cli-reference.md#json-schema-export), and the
Rust definitions in `src/drift.rs` describe the input records.

Completeness rules apply to every collector:

- Project and issue snapshots must cover every configured lead.
- Each Coding transition carries a `queue_before` snapshot of the queue at that
  moment: project and ticket order, spec and workflow states and blocker edges.
  `queue_before.missing_inputs` names any field that could not be rebuilt.
- Pull request and commit records must cover every configured repository and the
  whole reporting interval, including pull requests merged in the interval but
  opened earlier. Every page and partition must be present.
- Add a `missing` entry for each source page, partition or history query that is
  incomplete, for example `{"source":"github.prs","reason":"page 3 cursor unresolved"}`.
- Omit a lead from `herdr_busy` or `otel_spend` when that observation is
  unavailable. An observed spend of zero is not activity, even when
  `spend_threshold` is zero.

A signal that depends on missing evidence is `unavailable`, its counts are
null, and the lead cannot be reported `on_track`. The evaluator never reports a
clean result from partial evidence.
