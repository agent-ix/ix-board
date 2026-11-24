---
id: SR-6112
title: ix-board fleet status code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@3af5b3dec92173262660063327861391b63c8e52; src/fleet/{mod,classify,config,live,render,report}.rs, src/main.rs, src/lib.rs, Cargo.toml, Cargo.lock, docs/cli-reference.md, tests/fleet.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6112: ix-board fleet status code and Rust review

## Summary

Ticket: SWM-209. PR agent-ix/ix-board#89 at head 3af5b3d. Code review with
the rust-review lane folded in, scoped to `git diff origin/main...HEAD`.
The purpose of the command is that idle and blocked lanes cannot hide: no
UNKNOWN treated as working, no host failing silently, no single weak signal
trusted alone.

Live checks, read-only: `ix-board fleet status --json` was run from the PR
build against the real relay (`ixr:v3:` prefix) with no Prometheus URL and
with `IX_PROMETHEUS_URL=http://prometheus.luna.ix` (503, OOM incident). Both
runs gave 50 rows, all `IDLE`, every row with `last_event_ms: null`, exit 1.
The relay Redis holds no `ixr:v3:activity` stream and no `ixr:v3:fleet-lanes`
hash (`EXISTS` returns 0), and `ixr:v3:agents` holds 91 registrations of
which 41 have no `place`. Series names were checked against ix-relay FR-045
at 0ef4efc.

## Verdict

Not mergeable. Four high findings. The live run shows exactly the failure the
command exists to prevent: the relay has no activity stream and no lane
lists at all, 41 of 91 registrations are dropped, and the report says
nothing about any of it. The only defects are the Prometheus read and the
pending receipts. Separately, an interrupted or denied tool call keeps a
lane WORKING for up to `max_tool_runtime`, because a later `stop`, `idle` or
`blocked` event does not close an open `pre_tool`. `make ci` fails at
`cargo deny` on the new BSL-1.0 dependency.

What holds: class order follows FR-034. WORKING needs hook evidence or two
deduplicated distinct weak sources; weak signals are not wired live, so live
WORKING is hook-only. Relay-owned thresholds are refused locally, and every
age uses relay `TIME`. Host reads are parallel and bounded, and a failed host
keeps the other rows. The exit mapping matches `agents` exactly: Complete 0,
Partial 1, Failed 4, refused config 3 (ix-cli-kit be26cf0: Partial is 1, not
3). The relay event format (`addr`, `server_ms`, `kind`) matches the
relay-core Lua writer. relay-work-wire has no changes between 7252bfa and
0ef4efc, so `agents`, `retro` and `team_metrics` are unaffected; their tests
pass. STALE-ORDER behind `PendingOrderReceipts` is an honest defect, not
silence, and is acceptable until ix-relay#56 merges. A lane known only from a
live host list is never DEAD, which is acceptable because a live herdr list
means a pane exists.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | A missing activity stream or missing lane-list registry gives no defect. Live: no `ixr:v3:activity` and no `ixr:v3:fleet-lanes`, 50/50 rows IDLE with no events, and no defect names either source. `no_events` depends on the Prometheus-supplied `no_events_after`, so with Prometheus down the lane-level defect never fires. | src/fleet/live.rs:48-82, src/fleet/report.rs:540-552 |
| FND-002 | high | Registrations without a resolvable address are dropped by `filter_map`, and `AgentList::skipped` is ignored. Both are silent, although FR-034 says a record without a resolvable address is a source defect. Live: 41 of 91 registrations dropped with no defect. | src/fleet/live.rs:30-41 |
| FND-003 | high | `open_pre_tool` ignores later `stop`, `idle`, `blocked` and `session_start` events. An interrupted or permission-denied tool (no `post_tool`) keeps the lane WORKING for up to `max_tool_runtime` (2h), then STUCK. The relay lane-state rule requires the `pre_tool` to be the newest event. | src/fleet/classify.rs:210-216, src/fleet/classify.rs:297-345 |
| FND-004 | high | `make ci` fails: `cargo deny check licenses` rejects BSL-1.0 on xxhash-rust 0.8.21 (redis 1.8.3, through relay-core). Gate exit 2. | Cargo.toml:27, deny.toml:8 |
| FND-005 | medium | The Prometheus names follow no single OTLP translation. `ix_fleet_lane_compile_seconds_total` assumes suffixes; `ix_fleet_config`, `ix_fleet_lane_blocked` and `ix_fleet_probe_ok` assume none. With suffixes the gauges would be `_seconds` and `_ratio`; without them compile is `ix_fleet_lane_compile_seconds`. A wrong name returns an empty vector with no defect, which silently loses BLOCKED or compile seconds. It cannot be verified live (Prometheus 503). | src/fleet/live.rs:204-240 |
| FND-006 | medium | An unreadable or unconfigured Prometheus discards all hook evidence. `working_window` and `max_tool_runtime` become None, so no lane can be WORKING, STUCK or LOOP(stops). Every row reads IDLE and carries no marker that its class was degraded. No URL gives four identical per-host defects and no single run-level remediation line. | src/fleet/report.rs:491-499, src/fleet/classify.rs:219-221, src/fleet/classify.rs:328-345, src/main.rs:790-795 |
| FND-007 | medium | Registered lanes of a host whose lease lapsed are never UNREACHABLE. relay-core lets a registration override an expired list, and registrations never expire. With `has_pane` true they are never DEAD either, so a dead host's leads read IDLE. | src/fleet/report.rs:483-505, src/fleet/live.rs:56-68 |
| FND-008 | low | When `PromHttp::new` fails, main falls back to `NoPrometheus`, whose defect reason says "no Prometheus URL configured" although a URL was configured. | src/main.rs:790-795, src/fleet/live.rs:145-151 |
| FND-009 | low | `Defect.first_seen_ms` is the current run time, not the first-seen time FR-034 asks for; nothing persists defects across runs. | src/fleet/report.rs:301-317 |

## Gates

`CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=<worktree>/target make ci` exit 2:
fmt-check, clippy and every test pass (20/20 in `tests/fleet.rs`), then
`deny` fails on BSL-1.0 (FND-004).

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-010 | medium | The naming scheme is never verified as such. `ix_fleet_lane_blocked_ratio` is rightly exempt from `series_absent`, because it is empty while no lane is blocked. Nothing then checks, each run, that the always-present series (`ix_fleet_probe_ok_ratio`, `ix_fleet_config_seconds`) exist under the assumed names, or raises one `series_naming_unverified` defect when they do not. A wrong blocked name surfaces only as a side effect of generic `series_absent` rows. Lead ruling: the guard is required. | src/fleet/report.rs:438-485, src/fleet/live.rs:165-181 |
| FND-011 | medium | Per-host absence is still silent once any host has data. A host that never published a lane list is not covered by `lease_lapsed` (it needs an expired list), so its registered lanes read IDLE. Once the activity stream is non-empty, a host whose lanes have no hook event at all (hooks not installed or failing) gets no defect: `no_events` now skips lanes with no event. Only the all-hosts cases (`lane_registry_absent`, `relay_activity_absent`) are reported. | src/fleet/report.rs:488-504, src/fleet/report.rs:700-718, src/fleet/live.rs:52 |
| FND-012 | medium | `order_expired` re-derives expiry from the deadline: "otherwise an elapsed deadline with no Acted counts". Lead ruling: STALE-ORDER uses only the relay-recorded `AckExpired` as the single source of truth, and ix-board must not compute deadline expiry. | src/fleet/report.rs:130-148 |
| FND-013 | medium | FR-034 now contradicts itself. Inputs still says ix-board "SHALL NOT hold its own constant" for the working window and "SHALL hold no local copy" of `max_tool_runtime`/`no_events_after`. The new behaviour paragraph and AC-19 require documented relay defaults, held in `RELAY_DEFAULT_*` in config.rs. | spec/functional/FR-034-fleet-lane-status.md:40-44, src/fleet/config.rs:31-38 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | - |
| FND-002 | fixed | - |
| FND-003 | fixed | - |
| FND-004 | fixed | - |
| FND-005 | fixed | - |
| FND-006 | fixed | - |
| FND-007 | fixed | - |
| FND-008 | fixed | - |
| FND-009 | deferred | Field renamed `observed_ms` and FR-034 now names it observation time; a true first-seen time needs the fleet ledger (SWM-215). |
