---
id: SR-6152
title: "Base spec review of SWM-252 changes to FR-039 and FR-041"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board; spec/functional/FR-039-fleet-ledger.md, spec/functional/FR-041-fleet-saturation.md, spec/spec.md"
review_set: subset
---

## Summary

Ticket: SWM-252. Head 31f2fa12dbae323729db69775d7749796a19d5e4. quire validate passes with two grammar warnings; named series were checked read-only against http://prometheus.luna.ix.

## Verdict

**FAIL** - the named thermal series does not exist for the fleet's Linux and Windows hosts and no producer is allocated.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | node_thermal_temperature_celsius exists only for macOS draco (last sample about 46 hours old); titan reports node_hwmon_temp_celsius, luna node_hwmon_temp_celsius and node_thermal_zone_temp, terra node_thermal_zone_temp and ohm_cpu_celsius under job windows-sensors. draco never has node_memory_MemAvailable_bytes. relay-watcher serves ix_fleet_host_memory_used_bytes and total_bytes for all four hosts. No sensor selection rule is given. | spec/functional/FR-041-fleet-saturation.md:56 |
| FND-002 | high | The requirement hands collection to pluggable measurement sources and says it adds no producers, so no component is required to fill any capacity field and the ticket cannot be met. | spec/functional/FR-041-fleet-saturation.md:224 |
| FND-003 | medium | spec.md states ledger version 3 while FR-039, FR-041 and the code state version 4. | spec/spec.md:193 |
| FND-004 | medium | The FR-041 table list is headed ledger version 4 but omits capacity_window, and the step and window command text does not specify the lane-added and ramp-up inputs the CLI requires. | spec/functional/FR-041-fleet-saturation.md:99 |
| FND-005 | medium | The queue wait histogram of the producer is labelled by priority class; the requirement does not say whether the p90 is over all classes or the gate class. | spec/functional/FR-041-fleet-saturation.md:53 |
