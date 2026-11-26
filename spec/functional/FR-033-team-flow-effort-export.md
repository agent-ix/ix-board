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
history. Each scope has a stable operational team/scope identity and explicit
Linear selectors. The exporter SHALL preserve unavailable evidence and
independent assignment, execution and pause facts. A team-only selector covers
all issues in that Linear team and the team's configured repositories; no
release project or relay identity is required.

## Behavior

The exporter reads the shared `relay-work-wire` event contract. Transition IDs,
cursors and derived state commit atomically to a versioned SQLite journal.
Current snapshots establish current bindings without fabricating past starts.
Stage residence is distinct from sampled lifecycle execution. Scope additions,
removals, decomposition, cancellation and reported completion are separate.
Native ticket/PR links bound PR attribution. Overlapping scope effort is unknown
unless attribution is unique. Selector changes do not erase verified closure
events or retained effort history. An explicit pause changes intent without
erasing observed execution.

HTTP serves cached metrics independently of source reads. Numeric labels are
limited to configured team/scope IDs and bounded stage, state, metric and host
categories. Every scope series also carries `team_key`, the configured Linear
team key (empty only for scopes without a team selector). For each named
metric, missing or stale dependent facts omit that numeric series and set its
`ix_team_metric_available` series to zero wherever that availability series is
defined. Metrics without a companion availability series are omitted when their
source is missing or stale. A current empty and fully observed population is
reported as zero for counts; undefined ratios and distributions omit their
numeric series and report availability zero where that series is defined.
Workers share the default coordinated Linear cache and persisted quota gate.
Stale back-off fallback snapshots cannot certify a fresh tracker population;
last-success timestamps retain the actual successful source-sync time.
Snapshot-derived health gauges use the latest stored 1d retro snapshot for the
configured team. A daily snapshot stays current for 25 hours to allow the next
scheduled run and ordinary delay; a future or older snapshot is unavailable
with an explicit reason.
The exporter surface also serves the bounded fleet gauges defined by [FR-038](./FR-038-fleet-tick.md) and [FR-041](./FR-041-fleet-saturation.md) (`ix_fleet_tick_timestamp_seconds`, `ix_fleet_tick_actions{kind}`, `ix_fleet_ready_buffer_deficit`), omitting a value whose source is unavailable.
The initial configuration permits four scopes and 1,000 output series. Individual
evidence is retained for at most seven days within a 64 MiB SQLite file limit.

For QSL, the exporter service has no relay identity; `executing_agents` is
therefore sourced from current Prometheus lane state. `stage_oldest_age` uses
Linear issue state history rather than a retained assignment snapshot. The
effort tile needs the configured gate selector, verified closure evidence and
fresh retained Prometheus lane history; the first-review median needs a
successful configured-repository PR population and at least one covered new
non-draft PR with a valid marker; changes-requested needs covered open
non-draft PRs with Linear review/disposition records. Partial populations
publish with their coverage gauge; the relevant value is withheld at zero
covered observations, except stage age, which requires at least 25% issue
history coverage.

The named series are absent when their source-side conditions are not met.
Effort per verified path closure uses configured gate tickets and Linear's
`path_closed - path_closed_unverified` counts, keyed by issue and closure event.
Changing the gate selector does not reset past closures or effort. Effort uses
retained Prometheus lane-state history over the trailing day and publishes
fresh-host-feed coverage beside the value; no minimum 23h55m threshold applies.
The effort value is withheld only below 50% fresh-host-feed coverage because a
minority of the trailing window cannot represent the team's observed effort;
the coverage gauge remains present with the unavailable reason. Expected hosts
come from the fresh current team pane inventory in
`herdr_pane_info_ratio{workspace_label}`, including hosts with no current lane
state series. At or above 50%, the value uses only covered host-time and is
published with its coverage gauge.
If the window has no verified closure, the value is absent with an explicit
reason. Daily closure counts report every Linear transition into a completed
state for issues in the selected team scope, independent of PR attachment or
gate-path verification. They cover the previous seven UTC calendar days and
include observed-over-expected history coverage. First-review median requires a
successful PR population and at least one covered new PR; its coverage gauge is
published alongside the median even for a partial population. Changes-requested
share comes from the Linear reviewer marker population,
not GitHub review decisions; oldest stage age uses current-state entry times for
open team issues in Linear workflow states of type `started`; executing-agent
count uses current Prometheus lane state for the exact team workspace. Unrelated
incomplete git or process evidence does not invalidate execution. Malformed or
duplicate review records are skipped and counted in
`ix_team_review_records_skipped_24h`. Partial configured repository failures
lower coverage while preserving values computed from covered repositories.
Undefined ratios/distributions and stale values omit their numeric series.

For `executing_agents`, current `ix_fleet_lane_state_ratio` rows whose lane
workspace label exactly matches the configured Linear team define the observed
lane population; the latest timestamp per lane classifies it. Only `working`
counts as executing. If multiple distinct state labels have positive values at
that timestamp, the lane is uncovered instead of selecting an arbitrary row. A
stale or missing feed reduces coverage; a fresh unambiguous state other than
`working` observes a non-executing lane. The partial count
is published over covered lanes whenever at least one lane is covered. At zero
coverage the count is unavailable. The gauge
`ix_team_metric_coverage{team_id,team_key,scope_id,metric}` carries observed over
expected coverage for the named value. Stage age uses one `stage` label per
Linear workflow state of type `started`; the value is the oldest current-state
entry time among open team issues. The label is the lowercase snake-case state
name. Backlog, unstarted, triage and completed states are excluded. Stage
coverage is emitted only for stages with current open issues. State-entry times
come from Linear issue history, cached by issue update time and incrementally
backfilled across refreshes. Each refresh reads a bounded number of changed
histories; failed or not-yet-read histories reduce coverage. The oldest age is
published over covered issues at 25% or greater stage coverage, beside the
coverage gauge. The value is the oldest observed entry among covered issues.
Below 25%, fewer than one in four current issues support this
maximum-age estimate, so the age is withheld with an unavailable reason. No
fixed stage set or synthetic stage population is emitted.

`pr_changes_requested_share` uses open, non-draft PRs whose latest Linear review
or disposition record says `mergeable: false`, or that retain an unresolved
finding in any review artifact, as the numerator. A finding remains unresolved
until a later disposition for that finding marks it fixed, rejected or
accepted-no-change; a later clean review artifact alone does not clear it. The
denominator is open, non-draft PRs with a valid review record. Its coverage
gauge reports reviewed PRs over the full open, non-draft PR population and is
bounded by configured-repository and persisted marker-backfill coverage. During
marker backfill, only PRs whose channels have been read contribute to the value;
`review_marker_backfill` coverage reports the fraction of configured
repository/reviewer channels fully scanned across the retained 90-day update
window. First backfill is bounded to comments updated in that window; the
store retains at most that window and prunes its oldest comment rows before
reaching the 4 MiB limit. An open PR without a retained review record is
uncovered in the changes-requested coverage denominator. This is a
current mergeability share; `review_prs_with_findings_share_24h` is a separate
24-hour share of PRs with findings in the recent review-record population. The
first-review median is emitted over covered new non-draft PRs whenever at
least one is covered; draft PRs are excluded from both its denominator and
numerator, and its coverage gauge accompanies the value. The seven-day gauges
`ix_team_merged_prs_day{team_id,team_key,scope_id,days_ago}` and
`ix_team_closures_day{team_id,team_key,scope_id,days_ago}` have one row per UTC
calendar day with `days_ago` from 0 through 6. Each value is computed over the
successfully covered part of its configured source population and carries a
coverage gauge; zero means no matching event was found in that covered part.
An uncovered source row cannot certify the full cohort as empty, and a value is
withheld only when no row is covered.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-033-AC-1 | Short stage transitions and removals survive replay; duplicate event IDs do not duplicate durations or cursor advancement, and restart restores the atomic checkpoint. | Test |
| FR-033-AC-2 | Assignment reports alone do not count execution. Consecutive verified unique lifecycle observations count execution; gaps, missing attribution and replacement sessions preserve unknowns. A pause does not remove real observed effort. | Test |
| FR-033-AC-3 | Source outages do not advance assignment accounting beyond the confirmed watermark; a delayed earlier completion remains consumable after recovery. Repeated upserts replace worker identity without resetting a known stage start. | Test |
| FR-033-AC-4 | Size and time retention prune evidence using UTF-8 bytes before exceeding the SQLite ceiling; unsupported schemas and a requested ceiling below allocated pages are refused without rewriting history. | Test |
| FR-033-AC-5 | Missing/stale values and current ages are omitted; source last-success timestamps survive failed attempts; window counts remain gauges and native session/ticket identities never become metric labels. | Test |
| FR-033-AC-6 | Added, removed, canceled, split and completed populations remain separate; changing selectors does not erase verified closure events or retained effort history. | Test |
| FR-033-AC-7 | Bootstrap snapshots establish no historical stage start; unreported workers and partial sources remain visible as attribution/data gaps. | Test |
| FR-033-AC-8 | HTTP reads a bounded request line across packet fragments and serves its cached response without querying external sources. | Test |
| FR-033-AC-9 | PR closures distinguish merged and closed-without-merge cohorts; current changes-requested share uses open non-draft PRs with valid Linear review/disposition records as its denominator and reports marker/repository coverage, empty covered denominators are unavailable, and reopen/size measures expose their counting populations. | Test |
| FR-033-AC-10 | Native structured review records deduplicate by native and marker run identity; latest dispositions distinguish confirmed, rejected and pending findings. Malformed, missing-id and duplicate records are skipped and counted while other parseable records remain usable; marker coverage exposes an incomplete source population. Production regressions are not inferred. | Test |
| FR-033-AC-11 | A read-only local quota frontier gauge counts unexpired observations without creating or changing the quota database or sending API traffic; missing, stale or future observations omit the count and report unavailable monitoring. | Test |
| FR-033-AC-12 | Every exporter GitHub read uses shared persistent admission and response caching. A rolling-hour ceiling of 180 sends and 4,500 pessimistically reserved GraphQL points leaves 10% of GitHub's 5,000-point user/hour primary limit for other clients, and survives concurrent workers and restarts. Per-PR REST polling and cache-miss fallbacks are prohibited; historical links use batches of at most 25, with at most 50 older links per repository per collection. Every actual HTTP attempt is admitted once with automatic redirects/retries disabled. Debits and leases use the current admission clock; Retry-After uses the response receipt clock. Server reset/Retry-After cooldowns survive HTTP errors and restart; cooldown or exhausted budget admits zero sends. Unsupported expensive department and commit/check evidence remains explicitly unavailable. | Test |
| FR-033-AC-13 | Repository state uses at most ten 50-row pages per open/recent cohort and a 45-second admission deadline per repository; incomplete, malformed, absent or stale repository evidence lowers coverage and cannot certify a zero, clean or unmerged population for that repository. Values may be published over successfully covered repositories with coverage beside them; zero covered repositories makes them unavailable. Required nullable native fields must be present and PR state/timestamps consistent. Cache representations bind the provider and effective credential; unknown credentials cannot reuse private bodies. Cache hits retain source observation times, conditional 304 responses require an existing valid representation, and stale values are omitted after 15 minutes. Query/output/time and shared SQLite storage limits are enforced: 16 KiB requests, 1 MiB response bodies and 20-second transport deadlines; response storage is at most one eighth of the journal ceiling capped at 4 MiB and 128 entries, retained at most seven days. | Test |
| FR-033-AC-14 | The exporter serves the fleet tick gauges from the last committed tick in the fleet ledger, omits them when it is older than twice the tick interval, serves the ready-buffer deficit with bounded labels, and omits each value whose source is unavailable. | Test |
| FR-033-AC-15 | All scope metrics include `team_key` from the configured Linear selector. Execution counts use fresh classified Prometheus lane status even if unrelated git/process evidence is partial; multiple active state labels at the same latest timestamp make that lane uncovered instead of choosing arbitrarily. Effort uses retained Prometheus lane history and verified path closures; expected hosts come from the fresh current QSL `herdr_pane_info_ratio` inventory, including hosts with no lane samples in the trailing range. Stale state series cannot conflict with a newer state for the same lane and slot. Missing/stale history publishes availability and coverage without fabricating zero. Linear reviewer markers supply changes-requested share; malformed or duplicate records are skipped and counted. | Test |
| FR-033-AC-16 | WIP counts open selected-team issues whose Linear workflow state type is `started`; the gauge is `ix_team_wip_active`. Ready and blocked counts use the selected team issue population. Team panes are read from the current `herdr_pane_status_ratio` joined to the fresh `herdr_pane_info_ratio` inventory without relay identity; the comparison clock is captured after the source read and tolerates at most five seconds of source clock skew. The latest state per exact-team pane classifies `working` as executing and all other fresh states as not executing. A pane with no status in a fresh inventory is uncovered. Coverage reports observed over expected panes. A team-only reported scope matches its ticket slice only when all ticket identifiers carry the selected team key; project/initiative-only slices without team identity remain unattributed. Stage age uses open team issues' current Linear state entry times and emits labels only for measured workflow states of type `started`, in lowercase snake case. At most four issue histories are read per tracker refresh; persisted history permits gradual warmup without blocking the tracker. | Test |
| FR-033-AC-17 | The changes-requested share uses latest Linear review/disposition records for open, non-draft PRs, with numerator not-mergeable or unresolved-finding records and denominator PRs with valid review records; coverage is reviewed PRs over open non-draft PRs, bounded by configured-repository coverage. The separate marker-backfill gauge reports the fraction of channels fully scanned in the retained 90-day window and prevents partial results from being presented as complete. Unresolved findings survive later clean reviews until a terminal disposition closes them. Marker backfill persists per-channel page cursors across refreshes and source failures; first backfill is limited to comments updated in the prior 90 days; subsequent reads filter on native comment update time after a persisted watermark. The marker store prunes its oldest comments before reaching the 4 MiB limit. At most one marker page is read per tracker refresh; together with bounded daily-closure history reads this keeps warmed steady-state Linear request use below 150 requests per hour at the configured tracker cadence. The share and first-review median are published over any nonempty covered population, including partial coverage; draft PRs are excluded from first-review coverage. | Test |
| FR-033-AC-18 | The exporter emits seven daily gauges for merged PRs and selected-team Linear issues that transition into a completed state, with the same bounded team labels and `days_ago` 0..6. Configured GitHub repositories are queried over the full seven-day window so every merged day is represented. At most two selected issue histories are read per tracker refresh for the daily transition cohort; persisted cursors permit gradual warmup. Partial source failures lower each daily series coverage while preserving values computed from covered repositories and issue histories; zero is emitted only when the covered part of a source has no matching events. | Test |

## Dependencies

- [FR-029](./FR-029-department-retro-contract.md) owns the shared retro envelope.
- [FR-030](./FR-030-incremental-linear-cache.md) owns coordinated incremental reads and freshness provenance.
- [FR-031](./FR-031-shared-linear-quota.md) owns the shared request budget and cooldown.
- [Relay work history](ix://agent-ix/ix-relay/FR-066) owns transition identities and coverage.
