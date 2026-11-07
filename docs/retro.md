# Retro: snapshot, delta and backfill

`ix-board retro` is a group of three commands that measure whether a team's work is converging on its gate tickets. `retro snapshot` takes a measurement, `retro delta` compares it with the previous one and a baseline, and `retro backfill` builds a baseline from history. The dev-team `team-retro` skill runs these on a schedule and scores the team's roles from the numbers.

## Typical use

```bash
export TEAM_RETRO_HOME=~/.local/state/team-retro     # default; must be outside any git work tree
ix-board retro backfill --config retro.json --since 2026-10-31 --until 2026-11-06   # once, for a baseline
ix-board retro snapshot --config retro.json          # every few hours
ix-board retro delta    --config retro.json          # after each snapshot
```

Use the same `--window` for every snapshot and any backfill; records taken with different windows or thresholds compare as `query_changed`. A snapshot measured 15 to 60 seconds against a live workspace; most of the upper end is the 25-second wait on Herdr when a machine is unreachable.

`ix-board retro snapshot` reads Linear, GitHub, Herdr and a few local sources, computes a fixed
catalog of metrics for one team, and appends one record to that team's store. It never writes to
Linear, GitHub or any repository.

## Configuration

A JSON file. `team`, `gate_tickets`, `projects` and `repos` are required; a missing or invalid
file stops the command and names the file and field.

| Field | Meaning |
|---|---|
| `team` | Store directory name and default Linear team key. |
| `linear_teams` | Linear team keys whose open issues count toward scope metrics (default `[team]`). |
| `projects` | Linear project names whose issues count toward progress metrics. |
| `gate_tickets` | Issue ids whose transitive blockers define the path. |
| `repos` | `owner/name` repositories whose pull requests belong to the team. |
| `herdr_teams` | Herdr workspace labels read for utilization metrics. |
| `thresholds` | `pr_idle_hours`, `pr_oversize_lines`, `in_progress_stale_hours`. Each is optional with no default; a metric whose threshold is absent is `unavailable: not configured`. |
| `main_check` | `{"repo": "owner/name", "branch": "main"}`; check-runs define main status. |
| `local_paths` | Directories read for prunable worktrees and disk bytes. |
| `slot_process`, `orphan_slot_hours` | Sibling top-level fields: process-name substring of build-slot wrappers, and the age in hours after which an orphaned wrapper counts. `orphan_slots` needs both. |
| `gate_log` | Path to the gate JSONL log. The dev-team `team-gate` wrapper writes it, one line per gate run and re-pin. |
| `relay_leads`, `message_stale_minutes` | Sibling top-level fields: relay addresses (`server:workspace:tab`) and the age in minutes after which a pending message counts as undelivered. `messages_undelivered` needs both. |
| `decision_label` | Linear label marking issues that wait on an owner decision. |
| `baseline_taken_at` | UTC time naming the baseline record. |

A metric that needs an absent field is `unavailable` with reason `not configured`.

## Store

Records go to `$TEAM_RETRO_HOME/<team>/snapshots.jsonl`, defaulting to
`~/.local/state/team-retro`. Each line is one record with `schema_version`, `kind`, `taken_at`,
`team`, `window` and a `metrics` map. The command appends with a single write, never rewrites a
line, and refuses a path that resolves (through symbolic links) inside a git work tree or below a
directory containing `.git`. It also refuses to append to a file that does not end in a newline,
and a reader stops at a line that does not parse or a `schema_version` newer than it understands.

## Metrics

Each metric has `status` (`ok` or `unavailable`), `value` (null when unavailable), `unit`,
`direction`, `source`, `query` and, when unavailable, `reason`. `query` states the counting rule
with the window and thresholds applied. A source that stops before its last page makes the
metrics that depend on it unavailable rather than smaller.

The ids are `path_remaining`, `path_closed`, `intake_new`, `net_closed_minus_new`,
`gates_remaining`, `path_closed_unverified`, `prs_merged_per_hour`, `pr_cycle_median_hours`,
`pr_cycle_p90_hours`, `prs_open`, `prs_idle`, `prs_oversize`, `prs_ready_unmerged`,
`fix_rounds_max`, `fix_rounds_total`, `head_force_pushes_max`, `done_reopened`,
`main_red_incidents`, `main_red_minutes`, `issues_without_project`, `issues_without_milestone`,
`in_progress_stale`, `source_count_disagreement`, `agents_working_share`,
`agents_idle_minutes_max`, `worktrees_prunable`, `disk_bytes`, `orphan_slots`, `full_gates_max`,
`repins_total`, `build_lock_wait_minutes`, `messages_undelivered`, `decisions_open` and
`decision_oldest_hours`. The definitions are in the FR-024 requirement.

Boundaries: a pull request is idle at or beyond `pr_idle_hours`; oversize and orphan age are
strictly above their limits. `fix_rounds_*` count only pull requests that are open or were merged
inside the window, keyed by the marker's `pr=` value; a disposition counts only when its `repo=` and `id=` both match a `reviewer` marker whose `repo=` is in `repos`. `head_force_pushes_max` is the largest number of force-push events on one pull request open or merged in the window; ordinary pushes are not counted and no commit hash is stored. A main-branch incident
is a pass-to-fail transition between consecutive checked commits in the window; commits with
pending or no check-runs are skipped, and minutes run from the first failing commit to the next
passing one, or to the snapshot time. `agents_idle_minutes_max` is the largest time since an
agent's last turn. `net_closed_minus_new` counts the union of project and path issues once.

A relay address must be `server:workspace:tab`; anything else, or an address with no registration, makes the metric unavailable. The ix-relay queues are keyed by registered session name, so each relay address is resolved
read-only through the bus registry (`HGETALL <prefix>agents`) by machine, workspace and tab, and
the inbox lists of every matching registration are read with `LRANGE`. The bus URL and prefix
come from `IX_RELAY_URL` and `IX_RELAY_PREFIX`, then `~/.ix/relay/config.toml`, then the relay
defaults (`redis://127.0.0.1:6379`, `ixr:v3:`). `redis-cli` must be installed; the password is
passed in `REDISCLI_AUTH`, never on the command line.

Speed: the Linear workspace comes from the shared read cache (five-minute TTL) only when a fresh read of the recently updated issues proves it current, otherwise it is refetched; issue histories
are read only for issues updated inside the window and concurrently, reviewer markers are read
per pull request, and the herdr read runs alongside the others with a 25-second limit, after
which the agent metrics are `unavailable`.

## Delta

`ix-board retro delta --config retro.json [--json]` reads only the store; it makes no Linear,
GitHub, Herdr or local request. The latest and previous records are the two `snapshot` records
with the greatest `taken_at`. The baseline is the record whose `taken_at` equals
`baseline_taken_at`, or the earliest record of either kind when that field is absent; a value that
matches no record (compared as the same instant) stops the command with an error naming it, before any no-delta message. Backfilled records serve only as
baseline and history. With fewer than two snapshots the command prints that no delta exists and
exits zero.

For each metric of the latest record it prints the value, the change since the previous snapshot
and the change since the baseline. A change is reported only when both sides are `ok` and their
`query` strings are equal; when either side is `unavailable` the comparison is `unavailable`, and
when the queries differ it is `query_changed` with both queries shown. `regressing` is set for a
metric with direction `up` or `down` when the change moved against the direction in each of the
latest two consecutive comparisons between snapshots (so it needs three snapshots with comparable
queries); `neutral` metrics never regress. There is no combined score.

## Backfill

`ix-board retro backfill --config retro.json --since 2026-11-01 [--until 2026-11-06]
[--window 24h] [--json]` appends one `backfill` record per UTC day with `taken_at` at
`23:59:59Z` of that day, measuring window metrics over the half-open window ending at the next UTC
midnight (`[midnight - window, midnight)`), so consecutive 24h days are contiguous to the
millisecond. `--until` defaults to the last complete UTC day; a day that has not ended is refused.

Only these metrics are reconstructed: `prs_merged_per_hour`, `pr_cycle_median_hours` and
`pr_cycle_p90_hours` from pull-request creation and merge timestamps, and `path_closed`,
`intake_new`, `net_closed_minus_new` and `done_reopened` from Linear issue creation and state
history. They use the same functions as `retro snapshot`, fed historical frames. Every other metric
is `unavailable` with reason `not reconstructible`, including `path_remaining`: the blocker graph
at a past time is not recorded. The path is the current set of transitive blockers of the gate
tickets and project membership is each issue's current project, so the four Linear metrics count
today's path and today's project issues as they were on that day. Linear and
GitHub are each read once per run; an incomplete read makes the metrics that depend on it
unavailable for every day.

A day that already has a backfill record is skipped and reported. A range containing the
`taken_at` of any `snapshot` record (from 00:00:00 of the first day up to, not including, 00:00:00
after the last day) is refused, naming that record, and nothing is appended. The
store rules are those of the snapshot. Linear and GitHub only are read; Herdr, the relay and the
local sources are not started.
