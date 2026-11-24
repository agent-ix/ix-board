---
id: FR-043
title: Replace shell and Python fleet monitoring with Rust commands
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-040"
    type: depends_on
---
# FR-043: Replace shell and Python fleet monitoring with Rust commands

## Description

Every fleet monitoring function SHALL be implemented in Rust. The `_lead-bin`
scripts `monitor.sh`, `calibrate.sh`, `util.sh`, probes, `lane-status.sh`,
`hosts-status.sh`, `host-probe.sh`, `watchdog.sh` and `qsl-lane-check` SHALL be
removed only after their replacement is live. ix-board SHALL NOT contain Python or
logic-bearing shell for fleet behavior.

## Behavior

Replacement map: `monitor.sh` and `qsl-lane-check` become `fleet status` and
`fleet tick`; `calibrate.sh` becomes `fleet calibrate`; `watchdog.sh` becomes the
tick metric of [FR-038](./FR-038-fleet-tick.md) with an external alert; `util.sh`, probes,
`lane-status.sh`, `hosts-status.sh` and `host-probe.sh` become the ix-relay
relay-watcher collector (ix-relay SWM-201) read through [FR-034](./FR-034-fleet-lane-status.md).
The stopgap `monitor.sh` is stopped only after `fleet status` has produced a
complete report for the configured lanes and its ticks are observed by the tick
metric. Makefile targets call Rust binaries only.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-043-AC-1 | Each listed script has a documented replacement command or collector, checked against the replacement map. | Inspection |
| FR-043-AC-2 | The ix-board repository contains no `.py` file and no shell script with conditionals or loops reachable from fleet commands or Makefile targets. | Inspection |
| FR-043-AC-3 | The runbook states `monitor.sh` stops only after one complete `fleet status` report and a tick metric sample exist. | Inspection |
| FR-043-AC-4 | The replacement map is a fixture-checked table in the docs and every row names a command that exists in the CLI help. | Test |

## Dependencies

- ix-relay SWM-201 collector (assumed) for host and pane evidence.
