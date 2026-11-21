---
id: FR-032
title: Export bounded operational-team flow and effort metrics
type: FR
relationships:
  - target: ix://agent-ix/ix-board/FR-029
    type: depends_on
  - target: ix://agent-ix/ix-board/FR-030
    type: depends_on
  - target: ix://agent-ix/ix-board/FR-031
    type: depends_on
  - target: ix://agent-ix/ix-relay/FR-066
    type: depends_on
---
# FR-032: Export bounded operational-team flow and effort metrics

## Description

When operational scopes are configured, ix-board SHALL export cached flow and
effort measurements using the existing retro calculations and native work
history. Each scope has a stable operational team/scope identity, explicit
Linear selectors and verified lead addresses. The exporter SHALL preserve
unavailable evidence and independent assignment, execution and pause facts.

## Behavior

The exporter reads the shared `relay-work-wire` event contract. Transition IDs,
cursors and derived state commit atomically to a versioned SQLite journal.
Current snapshots establish current bindings without fabricating past starts.
Stage residence is distinct from sampled lifecycle execution. Scope additions,
removals, decomposition, cancellation and reported completion are separate.
Native ticket/PR links bound PR attribution. Overlapping scope effort is unknown
unless attribution is unique. Selector changes invalidate comparable effort
history. An explicit pause changes intent without erasing observed execution.

HTTP serves cached metrics independently of source reads. Numeric labels are
limited to configured team/scope IDs and bounded stage, state, metric and host
categories. Missing or stale dependent facts omit current numeric values.
Workers share the default coordinated Linear cache and persisted quota gate.
Stale back-off fallback snapshots cannot certify a fresh tracker population;
last-success timestamps retain the actual successful source-sync time.
The initial configuration permits four scopes and 1,000 output series. Individual
evidence is retained for at most seven days within a 64 MiB SQLite file limit.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-032-AC-1 | Short stage transitions and removals survive replay; duplicate event IDs do not duplicate durations or cursor advancement, and restart restores the atomic checkpoint. | Test |
| FR-032-AC-2 | Assignment reports alone do not count execution. Consecutive verified unique lifecycle observations count execution; gaps, missing attribution and replacement sessions preserve unknowns. A pause does not remove real observed effort. | Test |
| FR-032-AC-3 | Source outages do not advance assignment accounting beyond the confirmed watermark; a delayed earlier completion remains consumable after recovery. Repeated upserts replace worker identity without resetting a known stage start. | Test |
| FR-032-AC-4 | Size and time retention prune evidence using UTF-8 bytes before exceeding the SQLite ceiling; unsupported schemas and a requested ceiling below allocated pages are refused without rewriting history. | Test |
| FR-032-AC-5 | Missing/stale values and current ages are omitted; source last-success timestamps survive failed attempts; window counts remain gauges and native session/ticket identities never become metric labels. | Test |
| FR-032-AC-6 | Added, removed, canceled, split and completed populations remain separate; changing selectors invalidates old comparable effort points before another ratio is emitted. | Test |
| FR-032-AC-7 | Bootstrap snapshots establish no historical stage start; unreported workers and partial sources remain visible as attribution/data gaps. | Test |
| FR-032-AC-8 | HTTP reads a bounded request line across packet fragments and serves its cached response without querying external sources. | Test |

| FR-032-AC-9 | PR closures distinguish merged and closed-without-merge cohorts; current changes-requested share has a decisive-review denominator, empty denominators are unavailable, and reopen/size measures expose their counting populations. | Test |
| FR-032-AC-10 | Native structured review records deduplicate by comment/artifact identity, latest dispositions distinguish confirmed, rejected and pending findings, malformed or missing evidence remains unavailable, and production regressions are not inferred. | Test |

## Dependencies

- [FR-029](./FR-029-department-retro-contract.md) owns the shared retro envelope.
- [FR-030](./FR-030-incremental-linear-cache.md) owns coordinated incremental reads and freshness provenance.
- [FR-031](./FR-031-shared-linear-quota.md) owns the shared request budget and cooldown.
- [Relay work history](ix://agent-ix/ix-relay/FR-066) owns transition identities and coverage.
