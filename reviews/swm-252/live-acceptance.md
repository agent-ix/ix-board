# SWM-252 Amendment 2 live acceptance run

`acceptance-output.txt` records the real collector/status JSON and the historical draco/titan capacity-window evidence. The collector ran from a temporary config and scratch ledger with live Prometheus, Linear, GitHub and relay work-event reads. The scheduled collector remained on its five-minute interval; the final collection at 2026-11-25 23:22 UTC wrote only to `/private/tmp/ix-board-swm252-acceptance`.

The capacity scenarios used the actual 60-minute interval 2026-11-25 22:14:24.050–23:14:24.050 UTC, a 30-minute ramp-up, and historical Prometheus lane counts of draco=25 and titan=13. Each host had 17 complete demand samples whose interval union covered 98.17% of the window. The samples did not cover demand at the measured lane counts, so both windows correctly recorded `valid=false`, `inconclusive_reason=demand_short`, `deficit_coverage=0.0`. No sustainable level was recorded.

Host output and ticket/PR/commit attribution, interim sustainable slots, memory reserve readings and thermal readings were measured. `ix_build_slots_busy`, `ix_build_queue_wait_seconds`, and `ix_build_admission_blocked_seconds_total` remain `series_missing` pending observe adoption. Each scratch run has one invalid window, so the `inconclusive_no_demand` end guard correctly requires another measured window; neither run was ended with an unsupported reason. No live config, ledger, relay registration or lane state changed.

Focused tests after Amendment 2: saturation unit tests 19/19; capacity ledger tests 19/19; fleet tick integration tests 30/30. `quire trace --file src/fleet/saturation.rs` lists the changed implementations and tagged tests.
