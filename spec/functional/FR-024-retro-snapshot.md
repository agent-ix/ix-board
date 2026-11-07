---
id: FR-024
title: Snapshot the team convergence metric catalog
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-009"
    type: implements
---
# FR-024: Snapshot the team convergence metric catalog

## Description

`ix-board retro snapshot --config <file> [--window <duration>] [--json]` SHALL
read the structured Linear, GitHub and herdr sources named in the retro
configuration, compute the metric catalog below for the configured team, and
append one record to the store defined by [FR-025](./FR-025-retro-store.md).
`--window` takes seconds, minutes, hours or days (`90s`, `20m`, `4h`, `2d`) and
defaults to `24h`. The command SHALL measure window metrics over the trailing window
ending at the snapshot time, and state metrics at the snapshot time. The command SHALL NOT mutate Linear, GitHub or any repository. The
command SHALL NOT derive a value from ticket or comment prose; the only comment
text it reads is the first-line `reviewer` and `reviewer-dispositions` markers
that `/reviewer` posts, parsed as the key=value fields defined below.

### Configuration

The retro configuration is a JSON file with these fields. `team` is the store
name and the default Linear team key; the other names are explicit so that one
word never has to serve as Linear key, workspace label and directory.

| Field | Meaning |
|---|---|
| `team` | Store directory name and default Linear team key. |
| `linear_teams` | Linear team keys whose open issues count toward scope metrics. |
| `projects` | Linear project names whose issues count toward progress metrics. |
| `gate_tickets` | Issue ids whose transitive blockers define the path. |
| `repos` | `owner/name` repositories whose pull requests belong to the team. |
| `herdr_teams` | Herdr workspace labels read for utilization metrics. |
| `thresholds` | `pr_idle_hours`, `pr_oversize_lines`, `in_progress_stale_hours`. |
| `main_check` | Optional repository and branch; the check-runs of every commit on that branch inside the window define main status. |
| `local_paths` | Optional directories read for prunable worktrees and disk bytes. |
| `slot_process` | Optional process-name substring identifying build-slot wrappers, and `orphan_slot_hours`, the age after which a wrapper whose parent has exited counts as an orphan. |
| `gate_log` | Optional path to the JSONL gate log that the team gate script emits (Linear SWM-145) whose lines carry `pr`, `kind` (`gate` or `repin`), `requested_lock_at`, `acquired_lock_at`, `started_at` and `finished_at`. |
| `relay_leads` | Optional relay addresses (`server:workspace:tab`) and `message_stale_minutes`, read for undelivered messages. |
| `decision_label` | Optional Linear label name marking issues that wait on an owner decision. |
| `baseline_taken_at` | Optional UTC time naming the baseline record ([FR-027](./FR-027-retro-delta.md)). |

### Metric catalog

The catalog is a fixed list of stable metric ids. Each id SHALL have one
definition, one counting rule, one unit and one good direction (`up`, `down` or
`neutral`), and SHALL be reported only from the source named for it. When a
population is empty, the percentile and ratio metrics SHALL be `unavailable`
with reason `empty population`.

| Family | Metric id | Definition and source |
|---|---|---|
| progress | `path_remaining` | Open issues that transitively block a gate ticket, from the blocker-chain expansion of [FR-022](./FR-022-blocker-chain.md); `unavailable` when that expansion is truncated or partial. |
| progress | `path_closed` | Path issues whose state history shows a move to completed inside the window. |
| progress | `intake_new` | Issues created inside the window in the configured projects. |
| progress | `net_closed_minus_new` | Issues in the configured projects completed inside the window (path issues counted once) minus `intake_new`. |
| progress | `gates_remaining` | Gate tickets that are not completed. |
| progress | `path_closed_unverified` | Path issues completed inside the window with no merged pull request attached, per [FR-021](./FR-021-project-reconciliation.md). |
| flow | `prs_merged_per_hour` | Pull requests merged inside the window in `repos`, divided by window hours. |
| flow | `pr_cycle_median_hours`, `pr_cycle_p90_hours` | Median and 90th percentile (nearest rank) of creation-to-merge hours for those pull requests. |
| flow | `prs_open`, `prs_idle`, `prs_oversize` | Open pull requests; those with no update for `pr_idle_hours`; those whose added plus deleted lines exceed `pr_oversize_lines`. |
| flow | `prs_ready_unmerged` | Open non-draft pull requests whose GitHub review decision is `APPROVED`. |
| rework | `fix_rounds_max`, `fix_rounds_total` | For each pull request, the highest `round=` among `reviewer-dispositions` markers whose `id=` matches a `reviewer` marker carrying that `pr=`; the maximum and the sum over pull requests. |
| rework | `head_shas_per_pr_max` | The largest count of distinct head commits for one pull request that is open or was merged inside the window, from that pull request's paginated timeline. |
| rework | `done_reopened` | Issues whose state history shows completed followed by a non-completed state inside the window. |
| stability | `main_red_incidents`, `main_red_minutes` | Transitions of the `main_check` branch from passing to failing across the commits inside the window, and the total minutes from each failing commit to the next passing one (to the snapshot time while still failing), from check-run conclusions; `unavailable` when `main_check` is absent. |
| scope | `issues_without_project`, `issues_without_milestone` | Open issues of `linear_teams` lacking a project, or lacking a milestone in the structured issue record. |
| scope | `in_progress_stale` | Started issues with no state change and no attached pull request update inside `in_progress_stale_hours`. |
| scope | `source_count_disagreement` | Absolute difference between the open-issue count of the configured projects from the issue snapshot and from the project-chunk read. |
| utilization | `agents_working_share` | Agents whose [FR-018](./FR-018-agent-activity-verdicts.md) verdict is `WORKING` divided by agents reported for `herdr_teams`. |
| utilization | `agents_idle_minutes_max` | The largest minutes-idle among those agents. |
| resource | `worktrees_prunable`, `disk_bytes` | Prunable worktrees reported by git, and total bytes, for each directory in `local_paths`. |
| resource | `orphan_slots` | Processes whose name contains `slot_process`, whose parent has exited and whose age exceeds `orphan_slot_hours`, from the local process table. |
| rework | `full_gates_max`, `repins_total` | From `gate_log` lines finished inside the window: the largest number of `gate` lines for one `pr`, and the count of `repin` lines. |
| resource | `build_lock_wait_minutes` | Sum of `acquired_lock_at` minus `requested_lock_at` over `gate_log` lines finished inside the window. |
| utilization | `messages_undelivered` | Messages pending longer than `message_stale_minutes` in the ix-relay bus queues of the addresses in `relay_leads`, read without dequeuing. |
| scope | `decisions_open`, `decision_oldest_hours` | Open issues carrying `decision_label`, and the age in hours of the oldest. |

A later catalog version adds ids only; it SHALL NOT change the definition of an
existing id (see [FR-025](./FR-025-retro-store.md)).

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-024-AC-1 | Given fixture Linear, GitHub and herdr records, the command appends exactly one record containing every catalog id, with the fixture's expected value or unavailable status for each. | Test |
| FR-024-AC-2 | `path_remaining` equals the open transitive blockers of the gate tickets and excludes issues that block no gate; it is `unavailable` when the blocker expansion is truncated. `path_closed` counts only completions inside the window. | Test |
| FR-024-AC-3 | `net_closed_minus_new` is negative for a fixture window with more new issues than completions, counts a completed path issue once, and `intake_new` counts only issues created inside the window. | Test |
| FR-024-AC-4 | `prs_merged_per_hour` equals merged count divided by window hours, and `pr_cycle_median_hours` and `pr_cycle_p90_hours` match the nearest-rank values for a fixture with an even count and an outlier. | Test |
| FR-024-AC-5 | `prs_open`, `prs_idle`, `prs_oversize` and `prs_ready_unmerged` match the fixture for pull requests that straddle each threshold, a draft, and an approved and an unapproved pull request. | Test |
| FR-024-AC-6 | `fix_rounds_max` and `fix_rounds_total` are computed from marker first lines only: a comment containing the marker text in its body but not on its first line, and a human comment, change neither value; a disposition marker whose `id=` matches no `reviewer` marker is ignored. | Test |
| FR-024-AC-7 | `head_shas_per_pr_max`, `done_reopened` and `main_red_incidents` match the fixture's timeline, state history and check-runs; `main_red_incidents` is `unavailable` with reason `not configured` when `main_check` is absent. | Test |
| FR-024-AC-8 | `issues_without_project`, `issues_without_milestone`, `in_progress_stale` and `source_count_disagreement` match the fixture, and a stale issue with a recently updated attached pull request is not counted. | Test |
| FR-024-AC-9 | `agents_working_share` and `agents_idle_minutes_max` match the fixture rows, and an empty agent list yields `unavailable` with reason `empty population`. | Test |
| FR-024-AC-10 | `worktrees_prunable` and `disk_bytes` match a fixture directory with a prunable worktree, and are `unavailable` with reason `not configured` when `local_paths` is absent. | Test |
| FR-024-AC-11 | `full_gates_max`, `repins_total`, `build_lock_wait_minutes`, `orphan_slots`, `messages_undelivered`, `decisions_open` and `decision_oldest_hours` match the fixture gate log, process table, relay queue and labeled issues, and each is `unavailable` with reason `not configured` when its configuration field is absent. | Test |
| FR-024-AC-15 | `main_red_incidents` and `main_red_minutes` count a fail-then-pass sequence across three commits inside the window as one incident with the minutes between the failing and the next passing commit, and count a still-failing branch to the snapshot time. | Test |
| FR-024-AC-16 | `gates_remaining` counts gate tickets not completed, and `path_closed_unverified` counts a path issue completed inside the window with no merged pull request attached and not one with a merged pull request. | Test |
| FR-024-AC-17 | A configuration file that is missing, unreadable, not valid JSON, or lacks `team`, `gate_tickets`, `projects` or `repos` stops the command with a non-zero exit and a message naming the file and the field, and appends nothing. | Test |
| FR-024-AC-12 | The command makes no Linear mutation, no GitHub write and no repository write; test source doubles that record writes see none. | Test |
| FR-024-AC-13 | Each source is read with one paginated pass per run, and a source read that does not reach its last page makes dependent metrics unavailable ([FR-026](./FR-026-retro-unavailable-not-zero.md)). | Test |
| FR-024-AC-14 | `--json` output exposes every metric with id, value, unit, direction, status, source and query; text output prints one line per metric. | Test |

## Dependencies

- **Upstream**: [US-009](../usecase/US-009-review-work-convergence.md); [FR-005](./FR-005-structured-data-boundary.md); the readers behind [FR-018](./FR-018-agent-activity-verdicts.md), [FR-021](./FR-021-project-reconciliation.md), [FR-022](./FR-022-blocker-chain.md) and [FR-023](./FR-023-unprojected-issues.md); the issue-history reader that drift uses ([FR-008](./FR-008-drift-signals.md)). New readers this feature adds are limited to Linear comment markers, pull-request size, issue milestones and labels, pull-request timeline, check-runs, the gate log, the ix-relay queue and local git, disk and process reads. Linear ticket SWM-132 collects attributed history for the dashboard; the retro commands reuse its reader once it lands and do not duplicate it.
- **Downstream**: [FR-025](./FR-025-retro-store.md), [FR-026](./FR-026-retro-unavailable-not-zero.md), [FR-027](./FR-027-retro-delta.md), [FR-028](./FR-028-retro-backfill.md).
