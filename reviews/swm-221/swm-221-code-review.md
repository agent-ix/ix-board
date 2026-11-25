---
id: SR-6710
title: "SWM-221 retro eval code and Rust review"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@7d500013413471112318d4e8b8727089b54d8ed6; src/linear.rs, src/main.rs, src/retro/eval.rs, src/retro/live.rs, src/retro/metrics.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-047"
    type: reviews
---

# SR-6710: SWM-221 retro eval code and Rust review

## Summary

Ticket: SWM-221. Code review with the rust-review lane folded in, of PR agent-ix/ix-board#104 at 7d500013413471112318d4e8b8727089b54d8ed6 against base swm-261-role-metric-producers (90f15ec). Read-only. Checked: 25 `cargo test --lib retro::eval` tests pass (binary included a then-uncommitted request counter in src/linear.rs); each changed file passes rustfmt at the head; clippy was not verified because the author lane had uncommitted edits in the worktree that did not compile. A bounded instrumented run on isolated copies of config, cache and ledger and an offline run against an empty ledger with network blocked reproduced FND-001, FND-004, FND-006 and FND-012. Clean in what was examined: pass-rate denominator, matched-rule trend arithmetic, unknown-query rows, JSON report wrapper. Not examined for lack of a producing rule: Prometheus mean arithmetic against a live source.

## Verdict

**FAIL** - not mergeable. The command does not complete inside its consumer limit, several rules report pass from missing data or read the wrong value, and 13 rules can never read their source.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The command can exceed the panel contract (5 min). It runs the whole retro snapshot twice (current and previous window) before any adapter, with no evaluator-wide deadline, no progress output and JSON written only at the end. A bounded run on isolated copies was stopped at the five-minute limit before producing output, while still in the first snapshot. It was processing PR lists, state histories and metrics that no selected role rule reads; per-window adapters and all-history marker reads had not started. | src/main.rs:1254-1255 |
| FND-002 | high | Each run spends shared source budget on metrics no selected rule reads. A full run needs both snapshots plus all-history reviewer comment pagination per repository and window, so one evaluation can exhaust the shared Linear budget. GitHub calls go straight to `gh` without a timeout instead of using the shared budget guard in team_metrics/github.rs. | src/main.rs:1442-1462 |
| FND-003 | high | A `reviewer` data block is accepted only when `scope` is a string. dev-team FR-017 records and the reviewer skill write `scope` as a list, and real comments on SWM-261, SWM-252 and SWM-302 are lists. So every real review comment is invalid: REV-R13 counts all of them (false fail), and `continue` at line 977 drops their findings, so REV-R10 and review_findings read 0 (REV-R10 false pass). The test fixture writes `scope: full`, encoding the wrong shape. Line 968 also counts missing first-line marker fields into REV-R13, which FR-017 defines over the data block only (double count with REV-R05). | src/retro/eval.rs:1129-1141 |
| FND-004 | high | ORCH-R11 must be not_applicable with reason exactly `no pause producer`. declared_adapters sets that, then ledger_adapters (extended later) overrides it: host_build_pause_interval has no writer, the view is empty, no row carries an `available` column, so the result is Value 0 and `<= 10` PASSES. Reproduced with an empty v3 ledger and no network: ORCH-R11-E1 pass, value 0. The existing test checks declared_adapters alone. | src/retro/eval.rs:1754-1796 |
| FND-005 | high | read_ledger_view returns the number of distinct id values. For idle_lane_minutes_during_build_pause the id field is `lane` and the rule is in minutes with target <= 10. The `minutes` column is never read: one lane idle 500 minutes (or on several days) evaluates to 1 and passes. | src/retro/eval.rs:2078-2100 |
| FND-006 | high | No ledger adapter checks coverage, so absence of data is a pass. No code on this head emits a `need_ready_tickets` action or writes action.project/requested_count (the emitter exists only on unmerged #102), so PLAN-R14 reads 0 whatever happened. A new, empty or stale ledger also gives CODE-R13 value 0. Reproduced: empty ledger, zero runs, no network gives coder, orchestrator and planner pass_rate 1.0 with trend 0.0. FR-047 says partial coverage is unavailable. | src/retro/eval.rs:1911-1951 |
| FND-007 | medium | planner.md counts ACTIONS where fewer tickets than requested entered the ready state. The code sums the missing tickets (requested minus ready), so one action asking for 3 with none delivered reads 3 instead of 1, and the unit does not match the rule. | src/retro/eval.rs:2038-2038 |
| FND-008 | medium | The lane-order rows are not filtered by team, lane or session. Evaluating one team counts every lane in the fleet ledger, so another team's missed order fails this team's coder rule. | src/retro/eval.rs:1779-1792 |
| FND-009 | high | 13 rules name ix-board FR-029 department measurements (and `_max` of keyed values, and a loop detector count). Both retro adapters read only record.metrics, the FR-024 catalog; record.department is never read. All 13 always return `retro catalog has no metric`, even with a department configured. | src/retro/eval.rs:679-709 |
| FND-010 | high | retro_record_adapters is extended after retro_adapters and returns an entry for every retro query, so stored snapshots are always overridden (that adapter is dead). The previous window is then a live collection run today with a past `now`: prs_oversize, in_progress_stale, issues_without_project and issues_without_milestone are computed from current state. previous_score and trend report today's values as yesterday's, giving a fabricated flat trend (CODE-R03, PLAN-R07, R12, R13). | src/main.rs:1263-1280 |
| FND-011 | high | Any `reviewer` comment ever counts, including one posted after the merge. FR-017: a merge is judged with comments created at or before it. A pull request merged unreviewed and reviewed later reads 0, passing both rules. The previous window is likewise changed by later comments. | src/retro/eval.rs:1703-1713 |
| FND-012 | high | eval.json is mandatory and needs `role_spec_dir`, a key FR-017 does not define. dev-team FR-017-AC-6 requires an absent file to evaluate with defaults. No team has an eval.json today, so the panel command exits 3 for every team. Reproduced: absent file gives `No such file or directory`; a file with only FR-017 keys gives `missing field role_spec_dir`. | src/retro/eval.rs:88-98 |
| FND-013 | medium | Findings added under `new_findings` in a disposition comment are counted as findings but never checked for path, lines or finding text, so a locationless one passes REV-R10. Keys are (repo, SR id, fnd) although SR ids collide across pull requests, which merges distinct findings and can attribute a disposition round to the wrong pull request (line 938 overwrites). | src/retro/eval.rs:1033-1042 |
| FND-014 | medium | FR-017 joins gate log lines to gate records on finished_at. The code also requires the log's `pr` string to equal `owner/repo#n`. The gate log format does not fix that spelling (a bare number is accepted by the reader), so every run logged as `repo#n` or `n` counts as unrecorded and fails the rule. | src/retro/eval.rs:1664-1668 |
| FND-015 | medium | Records and markers are fetched over all history and one malformed old entry (missing finished, head or exit; a mismatched `pr`; for pr_methods, any old reviewer marker without a valid pr, line 1383-1386) fails the whole query for every window permanently, even when the entry is far outside the window. | src/retro/eval.rs:1560-1594 |
| FND-016 | medium | Runs are counted over all comments with no upper time bound. A second run posted after a window's end changes that window's value, so the previous window sees the future and the trend is distorted. | src/retro/eval.rs:1375-1400 |
| FND-017 | high | The stacked ledger contract reaches schema version 6 in #102; this stack's query accepts only 3 and returns unsupported version, so all three ledger rules are unavailable until the schema migration sequence is reconciled. #102 and #103 each define their own V3 and `git merge-tree` of #102 with this head conflicts in schema.rs, ledger/mod.rs and main.rs. `after #102 and #103` needs renumbered migrations first. | src/retro/eval.rs:1878-1884 |
| FND-018 | medium | --team changes the settings directory, snapshot store and report label only. collect_eval_retro_record still uses config.team, linear_teams, projects and repos, so a report labelled team X carries values measured for the configured team. | src/main.rs:1224-1224 |
| FND-019 | low | Text output glues the reason to the status (`UnavailableGitHub ...`, no separator) and omits sessions and scores, so text and JSON do not carry the same values. | src/retro/eval.rs:2415-2434 |
| FND-020 | low | An unknown --role empties the spec list and returns exit 0 with `roles: []`, indistinguishable from a team with nothing to evaluate. The retain also runs before adapters, so --role changes which sources are read although FR-047 calls it presentation only. | src/main.rs:1244-1246 |

## Scope

- run_retro_eval (examined): src/main.rs
- collect_eval_retro_record (examined): src/main.rs
- encode_output_json (examined): src/main.rs
- evaluate_rules (examined): src/retro/eval.rs
- score (examined): src/retro/eval.rs
- trend_delta (examined): src/retro/eval.rs
- parse_role_spec (examined): src/retro/eval.rs
- evaluate_sessions (examined): src/retro/eval.rs
- retro_record_adapters (examined): src/retro/eval.rs
- prometheus_adapters (examined): src/retro/eval.rs
- parse_linear_marker_metrics (examined): src/retro/eval.rs
- marker_data_complete (examined): src/retro/eval.rs
- yaml_data_block (examined): src/retro/eval.rs
- read_linear_gate_record_metrics (examined): src/retro/eval.rs
- merged_prs_without_review_count (examined): src/retro/eval.rs
- ledger_adapters (examined): src/retro/eval.rs
- read_need_ready_tickets_unanswered (examined): src/retro/eval.rs
- fetch_named_state_changes (examined): src/linear.rs
- main_commits (context_only): src/retro/live.rs
- fix_rounds (context_only): src/retro/metrics.rs

## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | still-open | The full duplicate retro snapshots were removed and stages report timings, but the command has no overall deadline bounding aggregate source work. |
| FND-002 | still-open | GitHub now uses the shared guard, but the Linear marker and PR-history queries still have no evaluator-wide or shared request budget; source volume can grow with repositories and pagination. |
| FND-003 | fixed | The marker reader accepts the real list-valued scope and validates marker fields separately; live-shape regression coverage exists in `src/retro/eval/linear.rs`. |
| FND-004 | fixed | The composed ledger path preserves the required `no pause producer` not-applicable result. |
| FND-005 | fixed | The reducer now sums the `minutes` column for minute-valued views. |
| FND-006 | fixed | Ledger rules require collector and capacity event-time coverage before returning values; missing coverage is unavailable. |
| FND-007 | fixed | Need-ready action counts use distinct action ids rather than missing-ticket totals. |
| FND-008 | still-open | Lane-order rows are still reduced without filtering to the evaluated team, lane, or session. |
| FND-009 | fixed | The retro adapters now read the configured department metric values and keyed metric reductions. |
| FND-010 | fixed | Previous-window adapters use stored retro records only; current live state is collected only for the current window. |
| FND-011 | fixed | Merge/review matching uses native merge time and excludes comments posted after the merge. |
| FND-012 | fixed | Missing evaluator settings default successfully and `role_spec_dir` has a documented default. |
| FND-013 | fixed | Disposition `new_findings` require location and finding text; keys include the pull-request identity. |
| FND-014 | fixed | Gate-log joins use the exact pull-request key and finish-time match represented by the current source readers. |
| FND-015 | fixed | Historical marker/record parsing is window bounded so malformed old entries outside the window do not poison the current query. |
| FND-016 | fixed | Reviewer-method runs are selected within the requested event-time window. |
| FND-017 | fixed | The read-only evaluator accepts supported schema versions 3 through 8, including the stacked fleet ledger schema. |
| FND-018 | fixed | `run_retro_eval` assigns the explicit team to the loaded retro config before collection. |
| FND-019 | fixed | Text rendering now includes scores, sessions, objective metrics and source counts with separated fields. |
| FND-020 | still-open | Unknown roles now error, but filtering specs before adapter construction changes which sources are requested; the specification says filtering is presentation-only. |

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-021 | high | The `relay` and `transcript` adapter modules are not called from `run_retro_eval`; configured relay-receipt, hook-event and transcript rules therefore remain unavailable even when their source data exists. | src/main.rs:1749-1825 |
| FND-022 | high | The required `cargo clippy --workspace --all-targets --all-features -- -D warnings` gate fails on this head with dead-code and clippy diagnostics in changed production and test code. | src/retro/eval/ledger.rs:82-437 |

### Round 2

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | still-open | Source collection is staged, but there is still no evaluator-wide deadline bounding aggregate source calls. |
| FND-002 | still-open | GitHub is guarded, but Linear history pagination still has no shared/evaluator request budget. |
| FND-008 | still-open | Lane-order rows remain unfiltered to the evaluated team, lane or session. |
| FND-020 | fixed | `source_specs` is now derived before presentation filters, so `--role` does not reduce source selection. |
| FND-021 | still-open | This change does not wire relay or transcript reducers into `run_retro_eval`; their rules remain unavailable even with source data. |
| FND-022 | fixed | `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes on this head. |
