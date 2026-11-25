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
and at least 86,100 seconds of valid observation coverage in the trailing day;
first-review median requires a successful PR population and at least one eligible
new PR; changes-requested share comes from the Linear reviewer marker population,
not GitHub review decisions; oldest stage age requires retained assignments with
known starts, except that known starts remain usable alongside unknown starts; and
executing-agent count requires fresh pane and transcript evidence that classifies
the agent. Unrelated incomplete git or process evidence does not invalidate an
execution verdict. Malformed or duplicate review records are skipped and counted
in `ix_team_review_records_skipped_24h`. Open and recent-closed GitHub PR
connections are batched per repository, the full PR population is withheld if
any selected repository fails, and an ordinary request failure does not impose a
shared one-hour cooldown. A stage age of zero is emitted only when the fresh lead
population is known and has no retained assignment in that stage; failed or stale
lead snapshots omit the value. The exporter reports source availability per
metric; undefined ratios/distributions and stale values omit their numeric series.

For `executing_agents`, the relay directory supplies expected lead addresses and
Herdr snapshots from each address's server determine whether its workspace and
tab has a live pane. A listed address whose tab has no pane is observed as not
running, not as missing coverage. The partial count is published over the
classified live sessions and observed no-pane leads whenever at least one lead
is covered; no percentage cutoff suppresses that observed count because the
gauge discloses how much of the directory it represents. At zero coverage the
count is unavailable. The gauge
`ix_team_metric_coverage{team_id,team_key,scope_id,metric}` carries observed over
expected coverage for the named value. Stage age publishes the oldest known start when at
least one active assignment has a known start and reports known-start coverage.
The QSL current-release selector resolves the unique started QSL project whose
name contains `release`; its open leaf issues are the
gate roots, so the selected path follows the active release project rather than a
fixed ticket list. Effort requires a verified closure and at least 86,100 seconds
(23h55m) of valid execution observation in its 24-hour comparison window.

`pr_changes_requested_share` uses open, non-draft PRs whose latest Linear review
marker says `mergeable: false` or retains an open finding as the numerator, over
open, non-draft PRs with a valid review record as the denominator. Its coverage
gauge reports reviewed PRs over the full open, non-draft PR population. This is a
current mergeability share; `review_prs_with_findings_share_24h` is a separate
24-hour share of PRs with findings in the recent review-record population. The
first-review median is emitted only when at least 95% of new non-draft PRs in the
24-hour cohort have a Linear review record. The seven-day gauges
`ix_team_merged_prs_day{team_id,team_key,scope_id,days_ago}` and
`ix_team_closures_day{team_id,team_key,scope_id,days_ago}` have one row per UTC
calendar day with `days_ago` from 0 through 6. They emit zero only when the full
selected GitHub PR population or Linear state-history population, respectively,
was read successfully for the entire cohort.

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
| FR-033-AC-12 | Every exporter GitHub read uses shared persistent admission and response caching. A rolling-hour ceiling of 180 sends and 4,500 pessimistically reserved GraphQL points leaves 10% of GitHub's 5,000-point user/hour primary limit for other clients, and survives concurrent workers and restarts. Per-PR REST polling and cache-miss fallbacks are prohibited; historical links use batches of at most 25, with at most 50 older links per repository per collection. Every actual HTTP attempt is admitted once with automatic redirects/retries disabled. Debits and leases use the current admission clock; Retry-After uses the response receipt clock. Server reset/Retry-After cooldowns survive HTTP errors and restart; cooldown or exhausted budget admits zero sends. Unsupported expensive department and commit/check evidence remains explicitly unavailable. | Test |
| FR-033-AC-13 | Repository state uses at most ten 100-row pages per open/recent cohort; incomplete, malformed, absent or stale evidence cannot certify an empty, clean or unmerged population. Required nullable native fields must be present and PR state/timestamps consistent. Cache representations bind the provider and effective credential; unknown credentials cannot reuse private bodies. Cache hits retain source observation times, conditional 304 responses require an existing valid representation, and stale values are omitted after 15 minutes. Query/output/time and shared SQLite storage limits are enforced: 16 KiB requests, 1 MiB response bodies, 20-second transport deadlines and a 45-second collection admission deadline; response storage is at most one eighth of the journal ceiling capped at 4 MiB and 128 entries, retained at most seven days. | Test |
| FR-033-AC-14 | The exporter serves the fleet tick gauges from the last committed tick in the fleet ledger, omits them when it is older than twice the tick interval, serves the ready-buffer deficit with bounded labels, and omits each value whose source is unavailable. | Test |
| FR-033-AC-15 | All scope metrics include `team_key` from the configured Linear selector. Execution counts use classified pane/transcript evidence even if unrelated git/process evidence is partial. Fresh verified empty execution and stage populations emit zero; unknown/stale populations omit the numeric value with availability 0. A known active stage start remains present alongside an unknown start. Linear reviewer markers supply changes-requested share; malformed or duplicate records are skipped and counted. | Test |
| FR-033-AC-16 | With relay directory entries for a team, lead addresses are refreshed from that exact workspace and their host's Herdr snapshot distinguishes live tabs from registered addresses with no pane; unavailable host snapshots reduce coverage. `ix_team_metric_coverage{team_id,team_key,scope_id,metric}` reports observed/expected. The executing-agent count is published over covered rows at any positive coverage and withheld at zero coverage; effort accumulation still requires complete lifecycle evidence. The current-release selector chooses the unique started team project whose name contains `release`, and derives gate roots from its open leaf tickets; absent or ambiguous projects withhold path metrics. | Test |
| FR-033-AC-17 | The changes-requested share uses latest Linear review markers for open, non-draft PRs, with numerator not-mergeable or open-finding records and denominator PRs with valid review records; the coverage gauge divides reviewed PRs by all open non-draft PRs. The share is published over any nonempty reviewed denominator, including partial coverage, and differs from the 24-hour finding share. First-review median is emitted only at 95% coverage of new non-draft PRs in the 24-hour cohort. | Test |
| FR-033-AC-18 | The exporter emits seven daily gauges for merged PRs and Linear completions, with the same bounded team labels and `days_ago` 0..6. A zero is emitted only after all selected repo PR pages or all selected ticket histories are read; incomplete source populations omit that daily series and expose zero metric coverage. | Test |

## Dependencies

- [FR-029](./FR-029-department-retro-contract.md) owns the shared retro envelope.
- [FR-030](./FR-030-incremental-linear-cache.md) owns coordinated incremental reads and freshness provenance.
- [FR-031](./FR-031-shared-linear-quota.md) owns the shared request budget and cooldown.
- [Relay work history](ix://agent-ix/ix-relay/FR-066) owns transition identities and coverage.
