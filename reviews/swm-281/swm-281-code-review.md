---
id: SR-6130
title: ix-board live host busy ratio code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@07c69b89183b2960ecb84076ad5147e5157e2e74; git diff 8bf053d...07c69b8: src/fleet/live.rs, src/fleet/report.rs, src/fleet/tick/live.rs, tests/fleet.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-038"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6130: ix-board live host busy ratio code and Rust review

## Summary

Ticket: SWM-281. PR agent-ix/ix-board#98 at head
07c69b89183b2960ecb84076ad5147e5157e2e74 (base 8bf053d). `git range-diff
67655b8..4006aa3 8bf053d..07c69b8` reports the one commit as identical (`=`).
Code review with the rust-review lane folded in, scoped to the diff. Callers
read for context: `build_report` and `read_hosts`, `TickFacts::read`,
`add_lanes`, `commit_tick`, and the ix-relay FR-045, FR-069 and FR-070
contracts.

## Verdict

**FAIL**. FND-001 (high) was reproduced on the live fleet: one host without a
busy series discards every host's ratio, so `add_lane` cannot fire for any host.

What holds, checked read-only against http://prometheus.luna.ix: the series is
`ix_fleet_host_busy_ratio` with labels `host` and `job="ix-relay-watcher"`; the
selector `{host="<name>"}` returns exactly one sample for draco and terra; one
series per host over 14 days; values 0.05 to 1.0, inside the accepted range; the
relay host names (draco, luna, terra, titan) are lower case and match the label
form. luna and titan have no series. An empty vector, a non-finite or
out-of-range value and more than one sample are each refused rather than read
as idle.

Gates on 07c69b8: `cargo fmt --check` clean, `cargo clippy --locked
--all-targets -- -D warnings` clean, `--test fleet` 35 passed, `--test
fleet_tick` 30 passed, `--lib fleet` 25 passed and 1 ignored. `cargo deny` was
not run (no dependency change). A full `--all-targets` run was not completed:
`fleet_assign` and `fleet_receipts_redis` stalled at near-zero CPU on this host
and were stopped; neither touches the changed code.

Live check: the built binary, `fleet tick --json` with a scratch ledger and
`IX_PROMETHEUS_URL=http://prometheus.luna.ix`, returned `host_busy` under
`unavailable` ("returned no busy ratio for host luna") and emitted a
`fix_source` for `tick.host_busy`.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | `host_busy` collects into one `Result`, so the first host with no ratio or a failed read discards every other host's ratio. Live now: luna and titan push no `ix_fleet_host_busy_ratio`, so draco (0.18 to 0.43, below the 0.85 target) and terra are dropped, `facts.host_busy` is `None`, `add_lane` cannot fire for any host even once SWM-212 lands, and every tick raises a standing `tick.host_busy` defect and `fix_source`. A host that is offline or unreachable, which `add_lanes` would skip anyway, disables the whole fleet. Keep the readable hosts and report the unreadable ones per host. | src/fleet/tick/live.rs:53-65 |
| FND-002 | medium | A stale ratio is accepted as current. The instant query returns the last sample for Prometheus's 5 minute lookback (shown read-only: evaluated 240s ahead, draco still returns its value, age 241s; at 330s it is gone). `host_busy` reads no sample age, `sample_age` is computed from the CONFIG series only, and `add_lanes` checks only `StaleHostRegistry`. ix-relay FR-070 omits the busy series while the cpu source fails so that an old value is never read as current; config keeps arriving, so no `stale_source` defect appears and `add_lane` can be emitted from a value up to 5 minutes old (60s is the stale bound used elsewhere, `push_interval * 2`). The same holds when OTLP pushes stop but the relay lease is alive. | src/fleet/tick/live.rs:56-63 |
| FND-003 | medium | An invalid (NaN, outside 0 to 1) or duplicated busy sample returns `Err` from `host_series`, discarding the host's whole read: blocked reasons, probes, relay thresholds and compile seconds. `fleet status` then classes that host's lanes without `ix_fleet_lane_blocked_ratio`, so a BLOCKED lane can class IDLE and the tick sends `push_next_ticket` into a blocked pane. Duplicates are reachable for 5 minutes after any watcher label change (job or instance). Before this PR the busy series could not affect classification. Record a bad busy value as a busy-only defect and leave `busy_ratio` empty. | src/fleet/live.rs:264-280 |
| FND-004 | medium | The tick reads every host's series a second time, sequentially on the main thread, instead of using the ratio `build_report` just read and discarded. Each `host_series` call is 6 HTTP queries each bounded by `host_timeout` (20s default) with no per-host bound, so a Prometheus answering each query just inside the bound can cost 4 hosts x 6 x 20s = 480s, longer than the 300s tick interval, against the FR-034 per-host read bound that `read_hosts` enforces. The two reads can also disagree (rows and defects from one, busy ratios from the other), and `fleet status` itself carries the ratio nowhere. Keep the first read's `busy_ratio` in the report and have `LiveSources` use it. | src/fleet/tick/live.rs:53-57 |
| FND-005 | low | With no Prometheus URL, which is the state of the installed ix-board config on this host, `build_report` already records a `prometheus` defect naming the remedy and `host_busy` now also returns `Failed`, adding a `tick.host_busy` defect: two `fix_source` actions for one missing setting. | src/fleet/tick/live.rs:47-52 |
| FND-006 | low | The `LiveSources` doc comment still says only the FR-072 lists are wired and every other source reports why it is unread; the busy ratio is now wired. | src/fleet/tick/live.rs:20-21 |

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-007 | high | The rule for a usable busy ratio is implemented twice: the `match` in `host_defects` decides which readings raise `prometheus.host_busy`, and a separate `if let` chain in `build_report` decides which enter `host_busy`; the bound `push_interval * 2` is written in both and already exists there as `stale_after`. A change to either needs the other, or a ratio is used while reported unusable, or dropped with no defect. The two agree today. Rated by the code-review duplication rule. Decide once (one function returning the ratio or the defect) and use it for both. | src/fleet/report.rs:453-487, src/fleet/report.rs:917-927 |
| FND-008 | low | The test module was moved above `run()` in the middle of the file and `#[allow(clippy::items_after_test_module)]` added to silence the lint. Put the module at the end of the file and delete the `allow`. | src/fleet/tick/live.rs:109-111 |

## Dispositions

Round 1, head 336d282879937eb6ddb49566bb27b7b0f55679d8.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Only usable readings enter `report.host_busy`; `host_busy()` returns that map. Live run: no `tick.host_busy`, defects for luna and titan only. |
| FND-002 | fixed | The busy sample age is queried separately; a ratio older than `push_interval * 2` or with no age is excluded and raises `stale_source`. |
| FND-003 | fixed | Invalid and duplicate samples set `busy_ratio_error`; the host read continues. |
| FND-004 | fixed | The second read is gone; the tick uses the ratio from the bounded `read_hosts` read. |
| FND-005 | fixed | Live run with no URL: one `prometheus` fix_source, no `tick.host_busy`. |
| FND-006 | fixed | The doc comment describes the busy map. |

Round 2, head ba5546dbe9224ffb67dcbca130089df242ce1a24.

| FND | outcome | reason |
| --- | --- | --- |
| FND-007 | still-open: the staleness bound is still written separately | The predicate is now one function, `usable_busy_ratio`. Both callers still compute `cfg.push_interval.saturating_mul(2)` themselves (src/fleet/report.rs:482 and 917), beside `stale_after = push_interval * 2` at 463. Mutation: changing only the bound at 917 to 3 leaves lib (255), fleet (37) and fleet_tick (30) passing. Define the bound once, for example by passing `FleetConfig` to the function. |
| FND-008 | fixed | The test module is declared last with its body in live_tests.rs; the allow is gone. |
