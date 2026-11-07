# Retro snapshot

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
| `thresholds` | `pr_idle_hours`, `pr_oversize_lines`, `in_progress_stale_hours`. |
| `main_check` | `{"repo": "owner/name", "branch": "main"}`; check-runs define main status. |
| `local_paths` | Directories read for prunable worktrees and disk bytes. |
| `slot_process`, `orphan_slot_hours` | Process-name substring of build-slot wrappers, and the age after which an orphaned wrapper counts. |
| `gate_log` | Path to the gate JSONL log. |
| `relay_leads`, `message_stale_minutes` | Relay queue names and the age after which a pending message counts as undelivered. |
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
`fix_rounds_max`, `fix_rounds_total`, `head_shas_per_pr_max`, `done_reopened`,
`main_red_incidents`, `main_red_minutes`, `issues_without_project`, `issues_without_milestone`,
`in_progress_stale`, `source_count_disagreement`, `agents_working_share`,
`agents_idle_minutes_max`, `worktrees_prunable`, `disk_bytes`, `orphan_slots`, `full_gates_max`,
`repins_total`, `build_lock_wait_minutes`, `messages_undelivered`, `decisions_open` and
`decision_oldest_hours`. The definitions are in the FR-024 requirement.

The ix-relay queue is read with `redis-cli LRANGE` using `IX_RELAY_URL` and `IX_RELAY_PREFIX`
(defaults `redis://127.0.0.1:6379` and `ixr:v3:`), so `redis-cli` must be installed for
`messages_undelivered`.
