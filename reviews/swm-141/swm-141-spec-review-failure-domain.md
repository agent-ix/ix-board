---
id: SR-6028
title: "Spec failure-domain review — ix-board SWM-141 retro"
type: SpecReview
analysis: failure-domain
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The failure-domain lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**FAIL** — record ordering after a backfill, window and threshold changes, malformed lines and empty populations all produce wrong deltas, and the spec has no rule for any of them. The git-tree guard, the newer-schema stop and the per-metric unavailable isolation are well specified.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-027's 'latest', 'previous' and 'earliest' record are undefined as file order or `taken_at` order. Backfill appends older records after newer snapshots, so file order puts the store out of time order. | spec/functional/FR-027-retro-delta.md:13-17 |
| FND-002 | medium | `query` is not required to include the window or the configured thresholds, so FR-027 compares a 4 h snapshot with a 24 h backfill record, or idle-threshold 2 h with 4 h, as a real change and can flag `regressing`. | spec/functional/FR-026-retro-unavailable-not-zero.md:19-21 |
| FND-003 | medium | FR-025 does not say how a truncated or unparsable line is handled (crash or full disk mid-append), and the append-only rule forbids repairing it. | spec/functional/FR-025-retro-store.md:18-20 |
| FND-004 | medium | FR-026 allows a null `value` only when `unavailable`. Empty populations (zero PRs merged gives no median/p90; zero agents reported gives 0/0 for `agents_working_share`) have no defined status or value. | spec/functional/FR-026-retro-unavailable-not-zero.md:13-14 |
| FND-005 | low | Only GitHub PR pagination has a criterion. A bounded or partial FR-022 traversal (limit hit, unavailable blocker) has no criterion requiring `path_remaining` to become unavailable. | spec/functional/FR-026-retro-unavailable-not-zero.md:30 |
| FND-006 | low | A configured baseline `taken_at` that matches no record has no error path. | spec/functional/FR-027-retro-delta.md:16-17 |
| FND-007 | low | 'A range overlapping a snapshot record' is undefined: does the snapshot's `taken_at` or its `window` count? | spec/functional/FR-028-retro-backfill.md:33 |

## Finding detail

- **FND-001** (FR-027): Failure (scenario above): by file order, the 'latest' record is the 11-06 backfill and the 'baseline' is the 11-07 snapshot. Delta reports a backfilled day as current and inverts every baseline change. US-009-EX-3 is exactly this flow.
- **FND-002** (FR-026): FR-024:19-21 makes window per run, FR-028 records are day windows, and the thresholds are configuration. FR-027:22-24 suppresses a change only on `unavailable` or a `query` difference. Failure: `prs_merged_per_hour` holds, but `path_closed` and `intake_new` drop about sixfold between a 24 h and a 4 h record and are reported regressing.
- **FND-003** (FR-025): Failure: one torn final line makes every later `retro delta` fail to parse forever, or an implementer silently skips it, and the two implementations disagree. Fix: a reader rule (for example, report the line number and treat that record as absent) plus a criterion for it.
- **FND-004** (FR-026): Failure: an implementer writes 0 for the median of an empty set, which violates 'never zero', or panics on 0/0. A quiet overnight window is normal (plan finding 1: 0 deletion PRs overnight).
- **FND-005** (FR-026-AC-2): FR-022 says reaching a bound never reports exhaustive. Failure: path_remaining reports a truncated count as `ok` and drops a gate's blockers from the progress metric.
- **FND-006** (FR-027): Failure: the implementation silently falls back to the earliest record, or picks the nearest record, and the baseline is not the one the user chose.
- **FND-007** (FR-028-AC-4): Failure: a backfill ending on the day a 24 h-window snapshot starts is refused by one implementation and accepted by another, leaving a duplicate window.

## Review evidence

- Scenario traced: snapshot on 11-07, then `retro backfill --since 10-31 --until 11-06` appends 7 older records after it (no overlap with the snapshot's day, so FR-028-AC-4 does not refuse).

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-008 | medium | `main_check` is defined by 'latest commit check-runs', but `main_red_incidents` (FR-024:66) counts pass-to-fail transitions inside the window, which needs the check-runs of every main commit in the window. | spec/functional/FR-024-retro-snapshot.md:39-39 |
| FND-009 | low | FR-028 adds `--window` to backfill but states no default, while FR-024's snapshot default is 24h. | spec/functional/FR-028-retro-backfill.md:13-17 |

- **FND-008** (FR-024): Failure: an implementer who reads only the latest commit, as the config row says, reports at most one incident per window, so a main branch that went red twice and recovered reports 0 or 1. AC-7 does not say how many commits the fixture holds.
- **FND-009** (FR-028): Failure: if backfill defaults to the UTC day and snapshot to 24h, the results match. Any other default makes every metric compared with a backfilled baseline report `query_changed`, so the backfill baseline never yields a delta.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Latest and previous are now the two snapshot records with the greatest `taken_at`. FR-025:37-38 orders the store by `taken_at`, and FR-027-AC-7 covers backfill appended after a snapshot. Proof: spec/functional/FR-027-retro-delta.md:15-20. |
| FND-002 | fixed | `query` now carries window and thresholds; FR-028-AC-7 checks that a window mismatch gives `query_changed`. Proof: spec/functional/FR-026-retro-unavailable-not-zero.md:23-25. |
| FND-003 | fixed | An unparsable line stops the read with its line number, and a missing trailing newline refuses the append; AC-6 covers both. Proof: spec/functional/FR-025-retro-store.md:28-30. |
| FND-004 | fixed | Empty populations are now `unavailable` with reason `empty population`; FR-026-AC-6 and FR-024-AC-9 cover it. Proof: spec/functional/FR-024-retro-snapshot.md:45-49. |
| FND-005 | fixed | `path_remaining` is now `unavailable` when the FR-022 expansion is truncated or partial; FR-024-AC-2 covers it. Proof: spec/functional/FR-024-retro-snapshot.md:53-53. |
| FND-006 | fixed | An unmatched `baseline_taken_at` now stops the command with an error naming the value; FR-027-AC-8 covers it. Proof: spec/functional/FR-027-retro-delta.md:22-23. |
| FND-007 | fixed | Overlap is now defined by a snapshot's `taken_at` inside the requested range; FR-028-AC-4 matches. Proof: spec/functional/FR-028-retro-backfill.md:27-28. |

FND-001 after_excerpt:

> change since the previous record, and the change since the baseline. The latest
> and previous records SHALL be the two records of kind `snapshot` with the
> greatest `taken_at`. The baseline SHALL be the record named by
> `baseline_taken_at` in the configuration, or the earliest record of either kind
> when that field is absent. Backfilled records serve only as baseline and
> history. The command SHALL read no Linear or GitHub source.

FND-002 after_excerpt:

> `query` SHALL state the counting rule used together with the window and the
> thresholds applied, so that two records computed by different rules or windows
> are distinguishable. One unavailable metric SHALL NOT stop the other metrics

FND-003 after_excerpt:

> When a command reads a line that does not parse, it SHALL stop with an error
> naming the line number and exit non-zero. If the final byte of the file is not
> a newline, the append command SHALL refuse to write and name the file.

FND-004 after_excerpt:

> The catalog is a fixed list of stable metric ids. Each id SHALL have one
> definition, one counting rule, one unit and one good direction (`up`, `down` or
> `neutral`), and SHALL be reported only from the source named for it. When a
> population is empty, the percentile and ratio metrics SHALL be `unavailable`
> with reason `empty population`.

FND-005 after_excerpt:

> | progress | `path_remaining` | Open issues that transitively block a gate ticket, from the blocker-chain expansion of [FR-022](./FR-022-blocker-chain.md); `unavailable` when that expansion is truncated or partial. |

FND-006 after_excerpt:

> When `baseline_taken_at` matches no record, the command SHALL stop with an error
> naming the value. When fewer than two snapshot records exist, the command SHALL

FND-007 after_excerpt:

> report it. When any `snapshot` record has a `taken_at` inside the requested
> range, the command SHALL refuse the range, name that record and append nothing.

## Dispositions (round 2)

| FND | outcome | reason |
| --- | --- | --- |
| FND-008 | fixed | `main_check` is now defined over the check-runs of every commit on the branch inside the window, and AC-15 covers a fail-then-pass sequence across three commits. Proof: spec/functional/FR-024-retro-snapshot.md:39-39. |
| FND-009 | fixed | Backfill now states a default window of 24h, the same as snapshot. Proof: spec/functional/FR-028-retro-backfill.md:13-14. |

FND-008 after_excerpt (round 2):

> | `main_check` | Optional repository and branch; the check-runs of every commit on that branch inside the window define main status. |

FND-009 after_excerpt (round 2):

> `ix-board retro backfill --config <file> --since <date> [--until <date>]
> [--window <duration>] [--json]` (default window `24h`, as for snapshot) SHALL append one record of kind `backfill` per

