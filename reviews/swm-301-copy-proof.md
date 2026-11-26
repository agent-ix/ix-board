# SWM-301 QSL acceptance proof

## Isolated run

- Copied the live Linear SQLite cache with SQLite `.backup` into `/Users/peter/dev/ix-board/.tmp/swm301-proof-final/linear.sqlite3`; the source cache was read-only for the copy. The exporter state and `TEAM_RETRO_HOME` were separate temporary paths. The live exporter config and service were not changed.
- Created the fresh 24-hour QSL snapshot with installed `ix-board 0.11.0`, using the isolated cache and `/private/tmp/swm301-proof-final/config-QSL.json`. The run took 296 seconds (17:41:04–17:46:00 PST) and produced 10,712 bytes of JSON.
- Ran the branch-built v0.12.0 exporter with the copied cache, that temporary QSL retro home/config, state `/private/tmp/swm301-proof-final/state/events.sqlite`, `IX_PROMETHEUS_URL=http://prometheus.luna.ix`, and port `127.0.0.1:19168`. `PATH` matched `/Users/peter/Library/LaunchAgents/com.agent-ix.ix-team-exporter.plist`. The command explicitly unset all `HERDR_*` context variables; no relay identity variables were present in the run environment.
- Scraped `capture-final.prom` at 2026-11-25 17:51:03 PST. The complete raw scrape is committed as `reviews/swm-301-copy-exporter.prom`.

## QSL panel values

The scope labels are `team_id="fsl",team_key="QSL",scope_id="completion"`.

| Tile | Observed value | Coverage / note |
| --- | ---: | --- |
| WIP | 50 | `ix_team_wip_active` |
| Ready / blocked | 48 / 40 | `ix_team_ready_issues` / `ix_team_blocked_issues` |
| Executing agents | 15 | 100% of current QSL lanes observed from Prometheus |
| Oldest `in_progress` stage age | 50,891.0 sec | 30.61% history coverage; oldest observed age over covered issues |
| Oldest `review` stage age | unavailable | 0% matching current-state history; below the documented 25% meaningful-coverage minimum |
| Leaf path remaining | 23 | `ix_team_path_remaining` |
| Gates remaining | 1 | Five configured gate tickets |
| Ready, unmerged PRs | 0 | `ix_team_prs_ready_unmerged` |
| PR cycle median / p90 | 2,048 / 19,444 sec | 24-hour QSL repository population |
| First-review median | 635.651 sec | 83 covered new PRs; 38.97% coverage |
| Changes-requested share | 0.60 | 26.32% coverage; computed from Linear review/disposition markers |
| Confirmed high / medium / low / unknown findings | 0 / 0 / 0 / 0 | Pending 217; rejected 0 |
| Verified closures per UTC day, `days_ago=0..6` | 1, 4, 1, 0, 2, 0, 0 | Team QSL path and configured gates QSL-2844, QSL-2845, QSL-2846, QSL-2856, QSL-2918 |
| Merged PRs per UTC day, `days_ago=0..6` | 8, 207, 26, 23, 1, 0, 0 | All three configured QSL repositories covered |

The separate effort series also emitted: six verified closures in the trailing 24 hours, 43,880 seconds per verified closure, and 58.84% fresh-host-time coverage. It remains in the exporter although the accepted R1 panel removed that tile.

The review-stage age is the only remaining panel value withheld in this capture. Its gauge and availability/reason series are present; the copied state-history cache contained no matching current-state transition for open issues in that stage. The exporter does not substitute issue creation or assignment time for the missing state-entry evidence.

## Request budget

`ix_team_linear_requests_last_refresh` reported 3 requests on the completed steady-state 300-second tracker refresh: 36 requests/hour at the configured cadence, below the 150/hour ceiling. A cold restart refresh reported 16 once; persisted marker cursors and stage-history entries were reused on the measured steady-state refresh. The five-minute retro snapshot is a separate CLI job and is not run by exporter refresh.

## Scope and live config

The proof covers the approved QSL panel scope after the R1 tile reduction. General exporter metrics outside the QSL panel also carry explicit availability/reason series when their producer is unconfigured. The proposed live config diff remains at `/Users/peter/dev/_lead-bin/waves/factory-panel-100/swm-301-config-proposal.diff`; it retains `listen=192.168.50.153:9167` and has not been applied.
