---
id: SR-6026
title: "Spec risk and complexity review — ix-board SWM-141 retro"
type: SpecReview
analysis: risk-complexity
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The risk-complexity lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**PASS with medium risk** — the main volatility is in deciding which metrics backfill can reconstruct and in the cost of each tick. FR-025, FR-026 and FR-027 are small, deterministic and low-risk.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-028 groups reconstructibility by family ('progress and rework metrics from ... state history'). Per metric id it does not say which ids can be reproduced; path_remaining, for one, needs the historical blocker edges and gate set. | spec/functional/FR-028-retro-backfill.md:17-20 |
| FND-002 | low | Snapshot has no request budget or runtime bound (history, comments and per-PR detail per tick across repositories), and no NFR covers it. | spec/functional/FR-024-retro-snapshot.md:13-16 |

## Finding detail

- **FND-001** (FR-028): Linear state history does not give the blocker edges or project membership of a past day. AC-3 names only agent verdicts and in-progress staleness as not reconstructible. Failure: one implementer computes past path_remaining from today's blocker graph, which yields a fabricated baseline. Another marks it unavailable. The baselines differ and delta-vs-baseline is wrong for one of them. Fix: list `reconstructible: yes/no` per metric id.
- **FND-002** (FR-024): Failure: on a large team, Linear or GitHub rate limits hit every few ticks, and FR-026 then marks whole families unavailable on every tick, so the deltas are empty without anything signalling the cause.

## Review evidence

- Backfill runs over history for every issue in the configured projects. Snapshot reads history, comments and per-PR detail on each 2-4 h cron tick (plan Deliverable 1, light tick).

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-003 | low | `head_shas_per_pr_max` covers 'open or merged' pull requests with no window bound, so each tick reads the paginated timeline of every pull request ever merged in `repos`. | spec/functional/FR-024-retro-snapshot.md:64-64 |

- **FND-003** (FR-024): Failure: the per-tick request count grows without bound as history grows and runs into the GitHub rate limit, and with AC-13's single-pass rule the metric then becomes permanently `unavailable`. Fix: bound the population to PRs open at the snapshot time or merged inside the window.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The reconstructible metric ids are now listed one by one, and `path_remaining` is explicitly not reconstructible. Proof: spec/functional/FR-028-retro-backfill.md:19-24. |
| FND-002 | fixed | AC-13 bounds each source to one paginated pass per run, and FR-026 names the failing source in `reason`. Proof: spec/functional/FR-024-retro-snapshot.md:94-94. |

FND-001 after_excerpt:

> The command SHALL reconstruct only these metrics from history: `prs_merged_per_hour`,
> `pr_cycle_median_hours`, `pr_cycle_p90_hours` (from pull-request creation and
> merge timestamps), and `path_closed`, `intake_new`, `net_closed_minus_new` and
> `done_reopened` (from Linear issue creation and state history). Every other
> metric SHALL be `unavailable` with reason `not reconstructible`, including
> `path_remaining`, because the blocker graph at a past time is not recorded.

FND-002 after_excerpt:

> | FR-024-AC-13 | Each source is read with one paginated pass per run, and a source read that does not reach its last page makes dependent metrics unavailable ([FR-026](./FR-026-retro-unavailable-not-zero.md)). | Test |

## Dispositions (round 2)

| FND | outcome | reason |
| --- | --- | --- |
| FND-003 | fixed | `head_shas_per_pr_max` is now bounded to PRs open at the snapshot time or merged inside the window. Proof: spec/functional/FR-024-retro-snapshot.md:68-68. |

FND-003 after_excerpt (round 2):

> | rework | `head_shas_per_pr_max` | The largest count of distinct head commits for one pull request that is open or was merged inside the window, from that pull request's paginated timeline. |

