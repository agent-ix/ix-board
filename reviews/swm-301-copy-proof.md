# SWM-301 QSL acceptance proof

## Run setup

- Ran the service-shaped exporter from this branch against an isolated SQLite copy of the live ix-board Linear cache, a temporary QSL retro home, and the copied QSL config. The shared cache and live exporter config were not modified.
- The isolated retro snapshot was created with `ix-board retro snapshot --json --config ... --window 24h`; it completed at 2026-11-26T00:35Z. The exporter used `tracker_interval_seconds=300`, local port 19167, and a separate temporary state database.
- `reviews/swm-301-copy-exporter.prom` is the raw Prometheus scrape from 2026-11-26 00:44:47 UTC. The sample was refreshed from the isolated copy; all remaining QSL dashboard tiles emitted a numeric series.

## Remaining QSL tiles observed

- WIP: `ix_team_wip_active` = 40.
- Ready and blocked: `ix_team_ready_issues` = 43; `ix_team_blocked_issues` = 40.
- Oldest in stage: `ix_team_stage_oldest_age_seconds{stage="in_progress"}` = 46879.7 seconds, coverage 55.3%; `stage="review"` = 20232.4 seconds, coverage 50.0%. Both are started workflow states and meet the 50% minimum evidence rule.
- Leaf remaining: `ix_team_path_remaining` = 16.
- Ready unmerged: `ix_team_prs_ready_unmerged` = 0.
- PR cycle median: `ix_team_pr_cycle_median_seconds_24h` = 1989 seconds.
- First review median: `ix_team_new_pr_first_review_median_seconds_24h` = 635.651 seconds, count 85, coverage 37.95%. The observed covered population is published with its coverage gauge.
- Changes requested: `ix_team_pr_changes_requested_share` = 0.60, coverage 35.71%. It uses Linear review markers and is published over covered PRs while marker backfill is incomplete.
- Findings by severity: confirmed high/medium/low/unknown = 0/0/0/0; pending = 221; rejected = 0. All six series report available in the scrape.
- Verified closures per day, `days_ago=0..6`: 1, 4, 1, 0, 2, 0, 0.
- Merged PRs per day, `days_ago=0..6`: 6, 207, 27, 23, 1, 0, 0.

## Steady-state request measurement

The exporter reported 11 Linear requests on the completed 300-second tracker refresh. At the configured cadence that is 132 requests/hour, under the 150 requests/hour budget. This is the refresh counter (`ix_team_linear_requests_last_refresh`) in the raw scrape. The earlier initial refresh used 3 requests; the 11-request sample is the subsequent full steady-state interval.

The copy proof is limited to the remaining QSL panel tiles after the accepted R1 scope reduction. It does not revalidate the removed tiles. Their existing exporter series remain in the collector.
