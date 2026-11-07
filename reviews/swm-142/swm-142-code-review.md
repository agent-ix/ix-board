---
id: SR-6030
title: "ix-board retro snapshot code and Rust review (SWM-142)"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@585c3f2c42c3dbffe68da3e07af6176fa8377c01; src/retro/{mod,catalog,config,sources,store,metrics,live}.rs, src/linear.rs, src/main.rs, src/agents/mod.rs, src/lib.rs, src/drift.rs, tests/retro.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-024"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-025"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-026"
    type: reviews
---

# SR-6030: ix-board retro snapshot code and Rust review (SWM-142)

## Summary

Ticket: SWM-142. PR agent-ix/ix-board#61, head 585c3f2. Code review with the
rust-review lane folded in (dev-tools:rust-style loaded), scoped to
`git diff origin/main...HEAD`. FR-027 and FR-028 are out of scope.

Checked against the owner's hard constraints: store outside git (symlinks
and dangling links followed), read-only sources, unavailable never 0, no
partial read reported as a smaller number, no hashes or pins, no
compatibility layers, no new crates (Cargo.toml unchanged; `redis-cli`
binary), relay password kept out of argv, no credentials in the store.

Metrics spot-checked against the FR-024 table with fixture reasoning:
path_remaining, path_closed, intake_new, net_closed_minus_new,
gates_remaining, path_closed_unverified, prs_merged_per_hour,
pr_cycle_median/p90 (nearest rank, even count), prs_idle (at-least
boundary), prs_oversize (strict), prs_ready_unmerged, fix_rounds_*
(window filter, first-line markers), head_shas_per_pr_max, done_reopened,
main_red_incidents/minutes (pending skipped, fail-first not an incident,
still failing to now), in_progress_stale, agents_*, orphan_slots (strict),
full_gates_max/repins_total/build_lock_wait_minutes, messages_undelivered,
decisions_*.

## Verdict

Not mergeable as is. One high finding: `done_reopened` silently drops the
path issues when the blocker expansion is truncated, so a partial read gives
a smaller number, which the owner's constraint and FR-026 forbid (FND-001).
Fix: use `scope.path_ok()?` as `net_closed_minus_new` already does.

The rest holds: the store does one `write_all` to an append-mode file after
the final-newline check, re-checks the git refusal at append time, and
creates nothing on refusal. `resolve_links` follows dangling links. Every
GitHub/Linear paginated read goes through `read_all_pages` or `gh
--paginate`, and errors become reasons. The merged-PR cutoff
(`reached_cutoff`) is correct because merged_at <= updated_at. The herdr
25 s deadline is measured from start and still collects a result that is
already waiting. `parallel_map` reports a read that never ran as an error.
`REDISCLI_AUTH` carries the password, and no reason string includes the URL.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | `done_reopened` uses `scope.path_ok().unwrap_or_default()`, so when the blocker expansion is truncated or partial it counts only project issues and reports a smaller number with status ok instead of `unavailable`. | src/retro/metrics.rs:565-581 |
| FND-002 | medium | Relay addresses without colons skip the registry and read `inbox:<text>` directly. A missing key returns an empty list, so a mistyped or unregistered address gives 0, not `unavailable`. FR-024 makes an unregistered address `unavailable`, and this extra address form is a compatibility path the spec does not define. | src/retro/live.rs:628-634, src/retro/live.rs:715-734 |
| FND-003 | medium | The workspace comes from the five-minute cache, and history reads are skipped when the cached `updated_at` is before the window start. An issue that completes or reopens after the cache was filled is missed, so path_closed, net_closed_minus_new and done_reopened undercount with status ok. | src/retro/metrics.rs:360-372, src/retro/metrics.rs:404-410, src/retro/live.rs:89-93 |
| FND-004 | medium | `head_shas_per_pr_max` counts every `PULL_REQUEST_COMMIT` timeline item as a head commit. A PR whose 10 commits arrived in one push reports 10 heads, so the rework metric over-counts. | src/retro/live.rs:157-163, src/retro/live.rs:289-297 |
| FND-005 | medium | `in_progress_stale` sees an attached PR's update only if the PR is in the `repos` listing (open, or merged and updated since the cutoff). An attached PR in another repository, or a closed-unmerged PR, that was updated recently is invisible, so the issue counts as stale with status ok. | src/retro/metrics.rs:695-702 |
| FND-006 | low | The agent verdicts use a hard-coded `idle_after` of 20 minutes, and the `agents_working_share` query does not record it, so the threshold that sets WORKING is not visible in the record (FR-026 query rule). | src/retro/live.rs:462, src/retro/metrics.rs:194 |
| FND-007 | low | `parse_redis_url` drops the `/db` path. With a non-zero db, the registry read is empty and a colon address becomes unavailable, but a bare-name address reads db 0 and gives 0. | src/retro/live.rs:597 |
| FND-008 | low | `run_snapshot` says a refused store path costs no reads, but `main` starts the herdr thread (spawning herdr/ssh processes) before `check_writable` runs. | src/main.rs:681-682, src/retro/mod.rs:38-49 |
| FND-009 | low | GraphQL wire types whose selection the code controls (`RawPullRequest`, `Timeline`, `TimelineNode`, `RelayMessage`) do not use `deny_unknown_fields`, which rust-style requires for typed payloads. | src/retro/live.rs:171-245, src/retro/live.rs:664-667 |
| FND-010 | low | `row.age_secs > hours * 3600` can overflow u64 for a very large `orphan_slot_hours` from the config (caller-controlled data), which panics in debug builds and wraps in release. | src/retro/metrics.rs:1170 |

## Gates

`cargo test retro` as briefed runs 0 tests: the filter matches no test name,
and the tests in tests/retro.rs are not named "retro". `cargo test --test
retro` (CARGO_TARGET_DIR=/private/tmp/claude-501/rv142-target): 40 passed,
0 failed. `make ci` was not re-run; nothing suggested the author's run at
this head was wrong.

## Dispositions

Disposition pass 1, at head 640f710. `cargo test --test retro`: 48 passed, 0 failed.

| FND | Outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 640f710: `union.extend(scope.path_ok()?...)`. The partial-expansion test now asserts `done_reopened` (and path_closed, net_closed_minus_new, path_closed_unverified) is unavailable. |
| FND-002 | fixed | 640f710: `queue_names_for_address` returns an error for anything that is not `server:workspace:tab` or has no registration. The bare-name path is gone; the test covers `peter`, `luna:IR`, `a:b:c:d` and the empty string. |
| FND-003 | fixed | 640f710: `LiveLinear::workspace(since)` reads `id updatedAt` of every issue updated since the window (or stale cutoff) and uses the cache only when `snapshot_is_current` finds the same raw update string for each one; otherwise `force_refresh_workspace`. Both queries return Linear's raw `updatedAt` string, so a completion after the cache fill changes the string and forces a refetch. `snapshot_is_current` is tested (see SR-6031 FND-008 for the untested wiring). |
| FND-004 | fixed | 640f710: `head_updates = 1 + force-push totalCount`, with the FR-024 definition rewritten to the same lower bound, so the over-count is gone (see SR-6032 FND-003 on the new definition). |
| FND-005 | fixed | 640f710: `stale_count` reads each attached PR with `pull_request(repo, number)` in any repository and state, and an unreadable one makes the metric unavailable; tested with a closed PR in another repo at 1 h and 200 h, and an unreadable one. |
| FND-006 | fixed | 640f710: `AGENTS_IDLE_AFTER_SECS` is shared with the herdr read and recorded as `idle_after=20m` in both agent queries; tested. |
| FND-007 | fixed | 640f710: `parse_redis_url` keeps `/db`, `redis()` passes `-n <db>`, and a non-numeric db is an error; tested. |
| FND-008 | fixed | 640f710: `run_retro_snapshot` calls `store.check_writable()` before building the Linear reader or starting the herdr thread. |
| FND-009 | fixed | 640f710: the GraphQL selection types and Record/Metric use `deny_unknown_fields`; the REST and relay types say why they do not. A test checks that an unknown field fails. |
| FND-010 | fixed | 640f710: `span_secs` and `checked_mul` make an oversize threshold unavailable with "too large to measure"; tested with u64::MAX for three thresholds. |
