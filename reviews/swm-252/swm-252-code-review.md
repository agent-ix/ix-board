---
id: SR-6150
title: "Code review (with rust-review lane) of ix-board PR 102 capacity history"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; src/fleet/capacity.rs, src/fleet/ledger/capacity.rs, src/fleet/ledger/capacity/tests.rs, src/fleet/ledger/mod.rs, src/fleet/ledger/schema.rs, src/fleet/ledger/tests.rs, src/fleet/mod.rs, src/fleet/config.rs, src/main.rs, src/bin/ix-team-exporter.rs, docs/cli-reference.md"
review_set: subset
---

## Summary

Ticket: SWM-252. PR 102, head 31f2fa12dbae323729db69775d7749796a19d5e4, base 5075d92. The change adds ledger tables, a measurement trait and three write commands, but a real run can record no capacity step at all: nothing in a shipped binary implements or calls the measurement path.

## Verdict

**FAIL** - high findings FND-001 to FND-010 and FND-012 to FND-014. Gates run: cargo fmt check pass; cargo clippy all-targets deny warnings pass; cargo test lib 260 passed 3 ignored; tests fleet 37, fleet_tick 30, schema 3 passed; unsafe comment audit pass. Not run: fleet_assign and fleet_receipts_redis (Docker wedged on this host), the rest of the integration suite, cargo deny. CI workflow files are unchanged. No AssuranceProfile applies. Behaviour was reproduced with the built CLI and a scratch Rust harness against the public API; a copy of the live titan ledger (schema 2) migrated to 4 with run, defect, lane_tick, host_tick and tick rows unchanged and integrity_check ok.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | No production MeasurementSource exists and no command calls measure_open_step or record_window; only tests do. Every capacity measurement field has no producer, so a real run records zero steps. | src/fleet/capacity.rs:164 |
| FND-002 | high | fleet capacity step opens a hold that no command can close. end, step and start are then refused for the host forever and fleet tick suppresses add_lane while the run stays open; only manual SQL recovers. | src/fleet/ledger/capacity.rs:354 |
| FND-003 | high | The build_slots_busy and build_slots_sustainable values are discarded. capacity_step has no column for them and FieldBasis has no value, so the ledger cannot report build-slot concurrency for any step. | src/fleet/ledger/capacity.rs:1212 |
| FND-004 | high | The documented fleet.capacity_limits.HOST table is rejected as an unknown key, so adding it makes fleet status, ledger query, capacity and tick fail. No reserve or limit value or source is read or stored. | src/fleet/config.rs:163 |
| FND-005 | high | end with reason knee records the lane count of the knee step itself (3) instead of stepping back one lane (2), so the level that showed no gain or over 5 percent throttle is recorded as sustainable. | src/fleet/ledger/capacity.rs:550 |
| FND-006 | high | An invalid window cannot be repeated with the same step_no. open_step ignores repeat_of and takes MAX plus 1, and the new unique index rejects a second row with that step_no. Repeats are then judged-without-demand violations and use up the 8 step bound. repeat_of is not validated. | src/fleet/ledger/capacity.rs:364 |
| FND-007 | high | History is pruned after ledger_retention (30 days): runs, steps including the current level's evidence steps, superseded levels and events are deleted, and discovery_state falls back to none. | src/fleet/ledger/schema.rs:301 |
| FND-008 | high | start enforces only one open run. The two runs per host per day cap and the step-down cool-off are not checked; four runs of one host at the same time were accepted. | src/fleet/ledger/capacity.rs:241 |
| FND-009 | high | None of the role-spec capacity views exists. fleet ledger query refuses discovery_steps, discovery_steps_violating_ramp, discovery_steps_judged_without_demand, discovery_raised_past_knee, discovery_runs_without_recorded_level and unrecorded_hosts_without_discovery_run, so ORCH-R16 to R20 are not measurable. | src/fleet/ledger/schema.rs:267 |
| FND-010 | high | Draining marks, drain completion, step-downs, lane events and host output samples have no production writer (no drain or remove command, record_event only called by a test), yet the exporter publishes ix_fleet_capacity_draining_lanes 0 for every host: zero for absent data. | src/fleet/ledger/capacity.rs:995 |
| FND-011 | medium | The exporter omits ix_fleet_capacity_throttled_at_level, ix_fleet_capacity_throttled_without_step_down and ix_fleet_capacity_overload_events_total required by FR-041-AC-16. | src/fleet/ledger/capacity.rs:973 |
| FND-012 | high | A step whose output, throttle and headroom are all unavailable is still valid on saturation samples alone, stores knee_signal false with basis available and coverage 1.0, and can set the recorded level through end step_bound or build_slots_saturated. | src/fleet/ledger/capacity.rs:1059 |
| FND-013 | high | The knee compares host_output_per_hour across different output bases. 12 commits per hour followed by 6 tickets plus PRs per hour raises an output_gain knee and stops discovery on a unit change. | src/fleet/ledger/capacity.rs:1189 |
| FND-014 | high | slots_busy_equals_sustainable is taken from the source without checking that both counts are available and equal. With busy unavailable and sustainable 8, end build_slots_saturated was accepted and recorded level 2. | src/fleet/ledger/capacity.rs:1210 |
| FND-015 | medium | measure_open_step commits the step and deletes the open window in separate transactions with a second lock acquisition. A lock timeout or crash between them leaves both; every retry fails on the unique index and end is refused. | src/fleet/ledger/capacity.rs:470 |
| FND-016 | medium | Host identity is case-sensitive on write and case-insensitive on read: titan and Titan get two open runs, step titan attached its hold to the Titan run, supersede would leave two current levels, and any misspelled host is accepted. | src/fleet/ledger/capacity.rs:241 |
| FND-017 | medium | No window contract is enforced: ramp-up of zero is accepted, the window length is unchecked (one saturation sample covers any window shorter than a tick, giving coverage 1.0), and end accepts a time before the run start. | src/fleet/ledger/capacity.rs:334 |
| FND-018 | medium | Reads require the exact new version: after install, capacity status, ledger query and exporter reads refuse the live version 2 ledger until a writer migrates it; once migrated, the installed 0.10.0 binaries refuse it, so their tick and status writes fail. Both builds report 0.10.0. | src/fleet/ledger/capacity.rs:637 |
| FND-019 | medium | record_event stores the evidence_kind its caller passes after only a vocabulary check; FR-041 requires ix-board to derive it from live series and never take it from the command. | src/fleet/ledger/capacity.rs:287 |
| FND-020 | low | Two draining marks on one lane give draining_lanes two identical entries and inflate the gauge. | src/fleet/ledger/capacity.rs:858 |
| FND-021 | low | Text output of capacity status prints only per-host state and row counts, not the runs, steps and level history the command help promises. | src/main.rs:1594 |
| FND-022 | low | end build_slots_saturated on a step whose slot comparison is NULL fails with a SQLite column type error instead of the typed refusal. | src/fleet/ledger/capacity.rs:582 |
