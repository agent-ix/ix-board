---
type: reference
---
# Agents and drift

`agents` tells whether each coding agent in a team's Herdr workspaces is working.
`drift` checks whether each configured lead is working on the right tickets and
delivering. Both are read-only.

## Usage: agents

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

Each row distinguishes execution, unfinished worker assignments and ended activity.
The first established rule determines its verdict.

| Verdict | Evidence |
| --- | --- |
| `PROMPT` | Outstanding approval dialog. |
| `ASKING` | Latest assistant response asks for input. |
| `UNKNOWN` | Contradictory or insufficient evidence prevents classification. |
| `WAITING` | Lead awaits active workers, or inactive lead has active direct workers. |
| `WAITING-STUCK` | Confirmed wait, complete evidence, no viable work or progress for 15 minutes. |
| `WORKING` | Positive current lead execution without unresolved contradiction or pending wait. |
| `RECENT` | Execution ended within the idle threshold; no active workers remain. |
| `IDLE` | Established quiet reaches the idle threshold. |

`--idle-after` accepts positive seconds, minutes or hours, such as `90s`, `20m`
or `2h`. It defaults to `20m`. `IDLE` rows show the team's next ready ticket.
Queue failures appear once and leave verdicts unchanged.

### Worker counts and coverage

`active_direct_workers` counts unfinished assigned workers whose immediate
parent is the pane's lead. `active_nested_workers` counts active descendants at
depth two or greater. Running and waiting assignments count as active.
Completed, stopped and internal approval sessions remain visible in `workers`.

Each count is nullable. Missing children, ambiguous ancestry, unsupported records
and conflicting observations make the affected count unknown. Complete graphs
with no active workers yield zero. `live_subagents` is a deprecated nullable alias
of `active_direct_workers`.

`availability` reports lifecycle, transcript, process, pane and git coverage separately.
Any unavailable source makes the row `partial`. Known evidence remains visible,
and independent positive execution can still establish `WORKING`.

### Signals and clocks

Claude companion metadata and structured Codex task records establish assignment
state. Completion and cancellation immediately remove workers from active counts.
A quiet running assignment remains active. Resume or inspection preserves a
terminal assignment until a new assignment or task start is established.

`last_turn_age_secs`, `subagent_age_secs` and git ages describe file or commit
ages. `recent_activity_at` describes structured execution activity. Each row
carries `host_now` and collection start/end timestamps in the owning host's clock
domain. Missing lifecycle timestamps remain null with a reason.

Pane tails are read for every state, including `working`, to detect approvals and
pending waits. Linux process collection attributes supported build/gate commands
through `HERDR_PANE_ID`. Unsupported platforms and permission failures have
explicit availability reasons.

### Probe protocol

Local and remote reads use the same read-only Rust probe. Remote hosts need a
compatible installed `ix-board`; failures preserve healthy rows and produce a
nonzero command exit. A machine has a 60-second read budget.

The hidden command `ix-board __agent-probe` accepts a JSON request on stdin:

```json
{"protocol_version":2,"request":{"sessions":[],"cwds":[]}}
```

Its JSON response contains `protocol_version: 2` and `probe`. Public agents
reports carry `schema_version: 2`. Schemas are available through:

```bash
cargo run --example export_schema -- AgentsReport
cargo run --example export_schema -- ProbeResponse
```

Requests allow 128 sessions and 128 working directories. File discovery stops
after 20,000 entries. Each record/header is limited to 1 MiB, and each transcript
to 64 MiB. Aggregate file reads stop at 256 MiB. The request retains at most
1,024 workers and accepts/emits at most 16 MiB of protocol data. Collection retries a changing file once. Repeated changes and
limit exhaustion produce explicit unavailable evidence.

Claude metadata prioritizes unfinished assignments before old terminal transcripts.
Small identity records remain visible when full replay exceeds the budget.
Unread transcript references keep descendant counts uncertain.

## Examples

```bash
ix-board agents "Team name" --json
```

The [agents example](examples/agents-v2.json) shows nullable worker counts and
source availability. [JSON Schema](schema/agents-report.schema.json) describes
the complete report contract.

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
