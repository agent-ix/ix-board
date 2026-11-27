---
id: FR-050
title: Check host authentication and local build capacity
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-048"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-049"
    type: depends_on
---
# FR-050: Check host authentication and local build capacity

## Description

`ix-board host check` SHALL run without arguments as a deterministic local-host
check suitable for a cadence job. It SHALL report the host identity, GitHub and
Linear authentication health, the local build-host metrics available from
locked-build, and suspicious quire, quoin or ripgrep processes whose current
working directory is inside the user's `dev` directory. It SHALL never print
credential values. Its process status SHALL be 0 when all required checks pass
and 1 when any check fails or is unknown.

## Behavior

- The host label is resolved from explicit ix-board host configuration or the
  current Herdr server label; an unresolved identity is an unknown finding.
- The check runs GitHub's authenticated status probe and validates the Linear
  API credential with a minimal authenticated request. Missing credentials,
  locked credential stores, HTTP 401 responses, failed probes, and commands
  absent from the non-login `PATH` have distinct stable statuses.
- Secret values and subprocess output that could contain secrets are discarded;
  only the stable status and safe source label are reported.
- Local capacity includes load per core, memory, swap, locked-build queue depth,
  running leases, and admission eligibility from locked-build metrics/status.
  Missing telemetry is unknown and fails the command.
- The command accepts injected process, command, credential, HTTP, and metric
  evidence in tests; tests do not inspect live host state, use real credentials,
  or send notifications.

## Acceptance Criteria

| ID | Criteria | Verification |
| --- | --- | --- |
| FR-050-AC-1 | The no-argument command reports the resolved host and stable GitHub and Linear auth states, returns 0 for healthy probes and 1 for failed or unknown checks, and does not print credential values. | Test |
| FR-050-AC-2 | Injected keyring-locked, missing-key, HTTP 401, command-failure and non-login-PATH evidence produces distinct findings without exposing subprocess output or secret values. | Test |
| FR-050-AC-3 | Local locked-build metrics report load per core, memory, swap, queue depth, running leases and admission eligibility; missing or stale required metrics fail the host check. | Test |
| FR-050-AC-4 | An injected process table reports quire, quoin or rg with cwd under `~/dev` and does not flag other commands or directories. | Test |
| FR-050-AC-5 | A cadence manifest accepts and runs the no-argument host-check job, recording its exit outcome. | Test |

## Traceability

SWM-343, SWM-328/334 and PLAT-1950 part 3.

## Dependencies

- [FR-048](./FR-048-build-host-hygiene-and-status.md) defines host process and
  locked-build evidence.
- [FR-049](./FR-049-cadence.md) defines recurring host-check jobs.
