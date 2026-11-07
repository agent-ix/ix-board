---
id: SR-6033
title: "ix-board retro delta and backfill code and Rust review (SWM-143)"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@5a0d69446967df5bb0140711fdf715fba3f8dec2; src/retro/{delta,backfill,metrics,mod}.rs, src/main.rs, tests/retro.rs, docs/retro.md, README.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-027"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-028"
    type: reviews
---

# SR-6033: ix-board retro delta and backfill code and Rust review (SWM-143)

## Summary

Ticket: SWM-143. PR agent-ix/ix-board#62, head 5a0d694. Code review with the
rust-review lane folded in (dev-tools:rust-style loaded), scoped to
`git diff origin/main...HEAD`. `cargo test --test retro` ran on the PR
checkout: 66 passed, 0 failed, which includes every existing snapshot test.

Mutation testing ran in a scratch copy (`/private/tmp/claude-501/rv143-mut`),
never in the PR checkout. 26 mutants were applied one at a time, each followed
by `cargo test --test retro`. 3 did not compile and were rewritten. Of the
rest, 18 were killed by a tagged test. These covered: regressing needing two
moves; unavailable vs comparable; query_changed; baseline default, override
and error; one snapshot giving no delta; the store's taken_at ordering; the
snapshot-in-range refusal and its half-open end; the idempotent skip;
not-reconstructible overriding computed values; empty population; the
per-record window; the end-of-day boundary (both the frame end and the
taken_at); the future-day refusal; a GitHub read per day; and a swallowed
GitHub page error. Six survived. Three were equivalent mutants: the neutral
guard is redundant because `moved_against` returns false for `Neutral`, and
moving the baseline lookup after the no-delta return changes nothing because
the `?` has already run. The other three are real gaps: a Linear history
cache reset per day and a Linear workspace read per day (FND-002 below), and
every day measured at the last day's frame (SR-6034 FND-001). A source process
spawned by `retro delta` and then ignored also survives (SR-6034 FND-002).

## Verdict

Mergeable on behavior. No high finding and no wrong result was found. The
only medium is a test gap, recorded in SR-6034.

What holds: delta takes only `(config, store)`, so the library cannot read a
source. The store returns records sorted by `taken_at`, so the default
baseline is the earliest record of either kind. Unavailable is never 0: the
change is `None` unless both sides are `ok` with values. Backfill reads
GitHub once per run through `read_all_pages`, which errors at the page limit
instead of returning a shorter list, so an incomplete read makes the flow
metrics unavailable for every day. The seven FR-028 metrics are the only ones
kept, and the not-reconstructible loop overwrites every other catalog id. The
store path is checked before any source starts. Fixtures are synthetic.

The snapshot refactor does not change behavior. `window_progress` holds the
same prefetch, `path_closed`, `intake_new` and `net_closed_minus_new` code in
the same order. `path_remaining`, `gates_remaining` and
`path_closed_unverified` now come after it, but they read nothing it writes,
and metrics go into a BTreeMap, so insertion order does not show.
`list_pull_requests` now takes `&dyn GithubSource`, a pure narrowing of its
argument. No compatibility layer, pin or hash was added.

Author choices not yet in the spec are judged in SR-6034 FND-003 and FND-004.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | Each backfill day's window is (D-1 23:59:59, D 23:59:59], which is one second earlier than the UTC day. `timestamp_epoch` keeps fractional seconds and Linear timestamps carry milliseconds, so an issue created or completed at D 23:59:59.500Z is counted on D+1, and one at D-1 23:59:59.500Z is counted on D. Nothing is lost or counted twice, but the window is not the UTC day the docs describe. | src/retro/backfill.rs:51, src/retro/backfill.rs:94, src/retro/metrics.rs:41-43 |
| FND-002 | low | docs/retro.md says Linear is read once per run, but no test checks it. Clearing the history cache per day, or re-reading the workspace for each day, passes the suite: every value test covers a single day, and only the GitHub read is counted. | src/retro/metrics.rs:1278-1295, tests/retro.rs:2468-2490, docs/retro.md:108 |
| FND-003 | low | `epoch` narrows an f64 parsed from a persisted `taken_at` with a bare `as u64`. That truncates fractional seconds, so `...:00.9Z` matches a baseline of `...:00Z`, and it saturates pre-1970 times to 0. Rust style asks for a checked conversion at persistence boundaries. | src/retro/delta.rs:172-176 |
| FND-004 | low | When `--until` is omitted and `--since` is today, the default `--until` (yesterday) falls before `--since` and the error says "--until is before --since", naming a flag the user did not pass. The real cause is that today has not ended. | src/main.rs:759, src/retro/mod.rs:47-48 |

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-005 | low | The new `end_exclusive` flag picks the window semantics, but no snapshot test pins the snapshot side: switching `run_snapshot` to `end_exclusive: true` (half-open `[now - window, now)`) passes the whole suite. Reading the code confirms snapshots pass `false` and keep `(now - window, now]`, so behavior is unchanged; only the oracle is missing. | src/retro/mod.rs:69-73, src/retro/metrics.rs:44-50 |

## Dispositions

Disposition pass 1, at head f6ac51d. Re-checked by mutation in a scratch copy, one `cargo test --test retro` per mutant; the PR checkout passes 68 of 68.

| FND | Outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | f6ac51d: a backfill frame now ends at the next UTC midnight with `end_exclusive: true`, so a day is `[midnight - window, midnight)`. The new PR fixture holds merges at end+0.5 s (inside) and at the day's start minus 0.5 s (outside). Making the start exclusive or the end inclusive each fails `pull_request_metrics_equal_the_known_values_of_the_trailing_window`. |
| FND-002 | fixed | f6ac51d: `linear_and_github_are_read_once_per_backfill_run_however_many_days` counts one workspace read and one PR read over four days, with no issue history read twice. Clearing the history cache per day, re-reading the workspace per day, or re-reading PRs per day each fail it. |
| FND-003 | fixed | f6ac51d: `epoch` now returns `Option<f64>` with no cast, and baselines match as exact instants. Flooring to the second fails `a_baseline_matching_no_record_stops_with_an_error_naming_it` (new `.900Z` case). |
| FND-004 | fixed | f6ac51d: `--until` is now optional in `run_backfill`, and both ends of the range are checked against `now` first, so `--since` today with no `--until` reports FutureDay naming the next midnight. Checking only the last day, or defaulting `--until` to today, fails `a_three_day_range_appends_three_records_at_the_end_of_each_utc_day`. |
