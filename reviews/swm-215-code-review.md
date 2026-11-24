---
id: SR-6116
title: ix-board fleet ledger code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@1cf3cfe979407f308a7b5abf82b0f7d97482708e; src/fleet/config.rs, src/fleet/ledger/mod.rs, src/fleet/ledger/schema.rs, src/fleet/ledger/tests.rs, src/fleet/mod.rs, src/fleet/render.rs, src/fleet/report.rs, src/main.rs, tests/fleet_cli.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-039"
    type: reviews
---

# SR-6116: ix-board fleet ledger code and Rust review

## Summary

Ticket: SWM-215. PR agent-ix/ix-board#93 at 1cf3cfe. Code review with the
rust-review lane folded in, scoped to `git diff origin/main...HEAD`. Probes
were run against real SQLite 3.51 and the PR's debug binary, outside the
worktree.

Checked and clean:

- Migration atomicity under a real crash. A v1 ledger with 50,001 rows was
  migrated with the V2 statements plus a full-table UPDATE in
  `journal_mode=DELETE`, `synchronous=FULL`, a 10-page cache (so dirty pages
  spilled to the database file), then the process was killed with SIGKILL
  before COMMIT. On reopen the hot journal rolled back: `user_version` 1,
  6 `defect` columns, no `lane_tick`, `integrity_check` ok, and the file was
  byte-identical to the pre-migration copy. A read-only open of the
  crashed file also read version 1. Transactional DDL and the
  `user_version` pragma are covered by the rollback journal.
- Writer lock. `flock(LOCK_EX|LOCK_NB)` polled every 20 ms up to
  `ledger_lock_wait` (default 10 s); measured: with the lock held by another
  process, `fleet status` returned after 10.01 s with `ledger_unwritable`.
  The lock file is never removed on Unix, which is correct for flock (unlink
  would let two writers lock different inodes). The run sequence is
  `AUTOINCREMENT` assigned inside the lock.
- Retention deletes run inside the writer transaction (`prune(&tx, ..)`
  before `tx.commit()`), so a crash leaves both the run and the prune undone.
- `open_cycles_sql` with the defaults evaluates 4, 24 and 25 minutes to 1,
  26 to 2 and 36 to 3, matching FR-039-AC-10's examples exactly; integer
  division truncation toward zero is absorbed by `max(1, ..)`.
- `ledger_unwritable` never suppresses the status output: `record_status`
  returns `()`, errors become one defect, rows stay. The output is delayed by
  at most the lock bound (10 s) plus rusqlite's default 5 s busy timeout per
  blocked statement.
- Placeholder tables (`calibration_run`, `audit_sample`,
  `ground_truth_sample`, `saturation_sample`, the FR-041/FR-044 tables) are
  empty, have no reader and no view; they are harmless apart from their
  retention rule (FND-003).
- `make ci` exit 0 at 1cf3cfe; the 15 ledger unit tests and 3 `fleet_cli`
  tests pass.

## Verdict

Request changes. The storage core is sound: migrations and runs are atomic
under a real kill, the lock is bounded, and retention runs in the writer
transaction. But a run whose relay read failed is written with the host
clock and closes every open defect, the retention list deletes rows that are
still open, the branch conflicts with main after #90 (textually and at
compile time), and the CLI reference does not document the new command.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | A run whose relay read failed is written with the host clock. `build_report` returns `finish(local_now_ms, ..)` when the relay is unreadable, and `record_status` writes that `run_ms` as `run.relay_ms`, as `defect.first_seen_at`, as the `closed_at` of other defects, as the retention cutoff and as the `--since` anchor (`MAX(relay_ms)`). Reproduced: with the relay down, `run` held 1795553590748, the host time. FR-039 requires all times on relay server time. The CLI test asserts this host-clock `first_seen_ms`. | src/fleet/report.rs:571, src/fleet/ledger/mod.rs:526-538, src/fleet/ledger/mod.rs:380-384, tests/fleet_cli.rs:65-69 |
| FND-002 | medium | A relay-unreadable run closes every open defect. When the relay is down the report carries only the relay defect, so `reconcile_defects` sets `closed_at` on every other open row (Prometheus, lane and host defects) though nothing was observed about them, and the next good run reopens them with a new `first_seen_at`. One relay blip resets every first-seen time. | src/fleet/ledger/mod.rs:493-500 |
| FND-003 | medium | Retention deletes rows that are still open or current. `PRUNE` keys `plan_coverage_gap` and `drift_finding` on `opened_at`, `action` on `emitted_at` (ignoring `resolved_at`), `cycle_event` on `started_at` and `capacity_level` on `recorded_at` (ignoring `superseded_at`). A gap or finding open longer than retention vanishes, though FR-039 counts an open row by elapsed time, and the current capacity level is deleted after 30 days. Only `defect` uses its close time. | src/fleet/ledger/schema.rs:243-266 |
| FND-004 | medium | The branch conflicts with main after #90. `git merge-tree origin/main HEAD` reports a content conflict in src/fleet/report.rs (`ReceiptsRecordingGap` versus `LedgerUnwritable` in `DefectKind`). After a textual merge it still does not compile: main adds `LaneRow.receipt_gap`, and the `row()` literal in the ledger tests lacks it. | src/fleet/report.rs:190-194, src/fleet/ledger/tests.rs:10-23 |
| FND-005 | medium | docs/cli-reference.md is not updated. It has no `fleet ledger query`, no ledger keys (`ledger_path`, `ledger_retention_days`, `ledger_lock_wait_secs`, `tick_interval_secs`, `cycle_interval_secs`, `grace_secs`), no `IX_BOARD_LEDGER_PATH`, no `ledger_unwritable` in the exit-status defect list, and still says first-seen time "comes with the ledger (SWM-215)". FR-039 requires a documented read-only query command. | docs/cli-reference.md:485-487 |
| FND-006 | low | Retention prune is a full scan of `lane_tick` every run: the only index is `(lane, observed_ms)`, which cannot serve `observed_ms < ?` (EXPLAIN: SCAN lane_tick). At 52 lanes x 8640 runs (5 min, 30 days) the table is 449,280 rows and 155 MB, mostly evidence JSON; a run with prune measured 40-100 ms warm and the `lane_span` window ~1 s. Bounded, but it grows with lanes and retention, and `host_tick` and `run` have no time index either. | src/fleet/ledger/schema.rs:73, src/fleet/ledger/mod.rs:512-520 |
| FND-007 | low | The lock has no stale or network-filesystem handling. On non-Unix the lock is a `create_new` file removed only in `Drop`; after a crash it stays and every later writer times out forever. On NFS or WSL drvfs, flock may be local-only or fail; failure surfaces as `ledger_unwritable`, but a flock that succeeds without cross-client exclusion is silent, and SQLite itself is unsafe on NFS. Neither case is documented or refused. | src/fleet/ledger/mod.rs:197-228 |
| FND-008 | low | `write_meta` overwrites `tick_interval_s`, `cycle_interval_s` and `grace_s` from the current config on every open, and the views read them at query time, so changing the cycle or tick interval silently re-scores every stored episode's `open_cycles` and the `idle_minutes` gap rule over the whole history. | src/fleet/ledger/mod.rs:340-356, src/fleet/ledger/schema.rs:152-160 |

## Dispositions

Round 1, fix head 69af642 (rebased onto main with #90).

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | fixed | `build_report` sets `relay_unavailable`; such a run stores the host clock only in `run.host_ms`, leaves `relay_ms` and `first_seen_at` empty, writes no lane or host rows and skips prune (src/fleet/report.rs:606-608, src/fleet/ledger/mod.rs:306-367); covered by `a_relay_unavailable_run_keeps_host_time_out_of_relay_columns_and_closes_nothing`. |
| FND-002 | fixed | `reconcile_defects` closes absent defects only when the relay was read (src/fleet/ledger/mod.rs:451-461); the same test asserts the Prometheus defect stays open. |
| FND-003 | fixed | PRUNE keys gaps, findings, actions, intervals, cycles, capacity runs and levels and ticks on their close time (src/fleet/ledger/schema.rs:266-290); `retention_never_prunes_open_rows` covers it. |
| FND-004 | fixed | Branch rebased onto origin/main 5af3141; origin/main is an ancestor, GitHub reports MERGEABLE, and `make ci` exits 0 at 69af642 with `LaneRow.receipt_gap` in place. |
| FND-005 | fixed | docs/cli-reference.md documents `fleet ledger query`, the views, `--since`, the ledger path resolution and keys, network refusal, `ledger_unwritable` and `first_seen_ms`. |
| FND-006 | fixed | New indexes `lane_tick_time`, `run_relay_ms`, `host_tick_time` serve the prune predicates (src/fleet/ledger/schema.rs:80-86). |
| FND-007 | fixed | src/fleet/ledger/lock.rs refuses network filesystems (Linux mountinfo and WSL `/mnt/<drive>`, macOS statfs, Windows UNC) and breaks a non-Unix lock whose pid is gone; the Windows branch is not compiled or run on this host. |
| FND-008 | fixed | `meta` is gone; each run stores `tick_interval_s`, `cycle_interval_s` and `grace_s` and `lane_tick_rows` joins them per run; `views_score_history_with_the_config_in_force_for_each_run` covers it. |
