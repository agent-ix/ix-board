---
id: FR-033
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
# FR-033: Export bounded operational-team flow and effort metrics

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
categories. Every scope series also carries `team_key`, the configured Linear
team key (empty only for scopes without a team selector). Missing or stale
dependent facts omit current numeric values and set the corresponding
`ix_team_metric_available` series to zero. A current empty population is
reported as zero for counts and ages; undefined ratios and distributions omit
their numeric series and report availability zero.
Workers share the default coordinated Linear cache and persisted quota gate.
Stale back-off fallback snapshots cannot certify a fresh tracker population;
last-success timestamps retain the actual successful source-sync time.
The exporter surface also serves the bounded fleet gauges defined by [FR-038](./FR-038-fleet-tick.md) and [FR-041](./FR-041-fleet-saturation.md) (`ix_fleet_tick_timestamp_seconds`, `ix_fleet_tick_actions{kind}`, `ix_fleet_ready_buffer_deficit`), omitting a value whose source is unavailable.
The initial configuration permits four scopes and 1,000 output series. Individual
evidence is retained for at most seven days within a 64 MiB SQLite file limit.

The named series are absent when their source-side conditions are not met:
effort requires configured gate tickets, a verified closure, fresh GitHub evidence,
and at least 86,400 seconds of valid observation coverage in the trailing day;
first-review median requires a successful PR population and at least one eligible
new PR; changes-requested share comes from the Linear reviewer marker population,
not GitHub review decisions; oldest stage age requires retained assignments with
known starts, except that known starts remain usable alongside unknown starts; and
executing-agent count requires fresh pane and transcript evidence that classifies
the agent. Unrelated incomplete git or process evidence does not invalidate an
execution verdict. Malformed or duplicate review records are skipped and counted
in `ix_team_review_records_skipped_24h`. Open and recent-closed GitHub PR
connections are batched per repository, successful repositories remain usable
when another repository fails, and an ordinary request failure does not impose a
shared one-hour cooldown. A stage age of zero is emitted only when the fresh lead
population is known and has no retained assignment in that stage; failed or stale
lead snapshots omit the value. The exporter reports source availability per
metric; undefined ratios/distributions and stale values omit their numeric series.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-033-AC-1 | Short stage transitions and removals survive replay; duplicate event IDs do not duplicate durations or cursor advancement, and restart restores the atomic checkpoint. | Test |
| FR-033-AC-2 | Assignment reports alone do not count execution. Consecutive verified unique lifecycle observations count execution; gaps, missing attribution and replacement sessions preserve unknowns. A pause does not remove real observed effort. | Test |
| FR-033-AC-3 | Source outages do not advance assignment accounting beyond the confirmed watermark; a delayed earlier completion remains consumable after recovery. Repeated upserts replace worker identity without resetting a known stage start. | Test |
| FR-033-AC-4 | Size and time retention prune evidence using UTF-8 bytes before exceeding the SQLite ceiling; unsupported schemas and a requested ceiling below allocated pages are refused without rewriting history. | Test |
| FR-033-AC-5 | Missing/stale values and current ages are omitted; source last-success timestamps survive failed attempts; window counts remain gauges and native session/ticket identities never become metric labels. | Test |
| FR-033-AC-6 | Added, removed, canceled, split and completed populations remain separate; changing selectors invalidates old comparable effort points before another ratio is emitted. | Test |
| FR-033-AC-7 | Bootstrap snapshots establish no historical stage start; unreported workers and partial sources remain visible as attribution/data gaps. | Test |
| FR-033-AC-8 | HTTP reads a bounded request line across packet fragments and serves its cached response without querying external sources. | Test |
| FR-033-AC-9 | PR closures distinguish merged and closed-without-merge cohorts; current changes-requested share has a decisive-review denominator, empty denominators are unavailable, and reopen/size measures expose their counting populations. | Test |
| FR-033-AC-10 | Native structured review records deduplicate by comment/artifact identity, latest dispositions distinguish confirmed, rejected and pending findings, malformed or missing evidence remains unavailable, and production regressions are not inferred. | Test |
| FR-033-AC-11 | A read-only local quota frontier gauge counts unexpired observations without creating or changing the quota database or sending API traffic; missing, stale or future observations omit the count and report unavailable monitoring. | Test |
| FR-033-AC-12 | Every exporter GitHub read uses shared persistent admission and response caching. A rolling-hour hard ceiling of 40 sends and 1,000 pessimistically reserved GraphQL points survives concurrent workers and restarts. Per-PR REST polling and cache-miss fallbacks are prohibited; historical links use batches of at most 25, with at most 50 older links per repository per collection. Every actual HTTP attempt is admitted once with automatic redirects/retries disabled. Debits and leases use the current admission clock; Retry-After uses the response receipt clock. Server reset/Retry-After cooldowns survive HTTP errors and restart; cooldown or exhausted budget admits zero sends. Unsupported expensive department and commit/check evidence remains explicitly unavailable. | Test |
| FR-033-AC-13 | Repository state uses at most four 50-row pages per open/recent cohort; incomplete, malformed, absent or stale evidence cannot certify an empty, clean or unmerged population. Required nullable native fields must be present and PR state/timestamps consistent. Cache representations bind the provider and effective credential; unknown credentials cannot reuse private bodies. Cache hits retain source observation times, conditional 304 responses require an existing valid representation, and stale values are omitted after 15 minutes. Query/output/time and shared SQLite storage limits are enforced: 16 KiB requests, 1 MiB response bodies, 20-second transport deadlines and a 45-second collection admission deadline; response storage is at most one eighth of the journal ceiling capped at 4 MiB and 128 entries, retained at most seven days. | Test |
| FR-033-AC-14 | The exporter serves the fleet tick gauges from the last committed tick in the fleet ledger, omits them when it is older than twice the tick interval, serves the ready-buffer deficit with bounded labels, and omits each value whose source is unavailable. | Test |
| FR-033-AC-15 | All scope metrics include `team_key` from the configured Linear selector. Execution counts use classified pane/transcript evidence even if unrelated git/process evidence is partial. Fresh verified empty execution and stage populations emit zero; unknown/stale populations omit the numeric value with availability 0. A known active stage start remains present alongside an unknown start. Linear reviewer markers supply changes-requested share; malformed or duplicate records are skipped and counted. | Test |

## Dependencies

- [FR-029](./FR-029-department-retro-contract.md) owns the shared retro envelope.
- [FR-030](./FR-030-incremental-linear-cache.md) owns coordinated incremental reads and freshness provenance.
- [FR-031](./FR-031-shared-linear-quota.md) owns the shared request budget and cooldown.
- [Relay work history](ix://agent-ix/ix-relay/FR-066) owns transition identities and coverage.
