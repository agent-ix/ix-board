---
id: SR-6024
title: "Spec dependency review — ix-board SWM-141 retro"
type: SpecReview
analysis: dependency
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The dependency lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**FAIL** — the history reads duplicate SWM-132's scope with no reference to it, and several new collectors needed as groundwork are presented as reuse. FR-025 and FR-027 dependencies are sound.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-028 and FR-024 read Linear state history and blocker edges over a window. That is SWM-132's collector scope, but neither FR references SWM-132 or names the existing drift IssueHistory reader. | spec/functional/FR-028-retro-backfill.md:16-19 |
| FND-002 | medium | Three metric groups need readers that do not exist yet. The spec lists them as reuse instead of naming the groundwork: Linear comments (fix_rounds, in_progress_stale), per-PR size, and issue milestones. | spec/functional/FR-024-retro-snapshot.md:39-44 |
| FND-003 | low | Dependency sections disagree. FR-024 Downstream omits FR-026, although FR-026 lists FR-024 upstream and FR-024-AC-6 depends on FR-026's field set. FR-025 Upstream omits FR-024, whose metric map it stores, and its Downstream omits FR-026. | spec/functional/FR-024-retro-snapshot.md:64-67 |

## Finding detail

- **FND-001** (FR-028): FR-024 lines 32 and 42 (`path_closed`, `done_reopened`) and FR-028 lines 16-19 cite 'Linear state history' generically. Failure: the implementation builds a second paginated state-transition collector next to SWM-132's, which breaks Peter's no-duplication constraint and gives two answers for the same window. Fix: name the FR-008 history reader and state that retro consumes SWM-132's evidence (or explicitly sequence SWM-132 first).
- **FND-002** (FR-024): Line 66 claims reuse of FR-018/021/022/023 readers. But src/ has no comment reader. `prs_oversize` needs additions and deletions, which the PR list response used by reconcile does not carry, so each PR needs a detail read. FR-023 has no milestone field. Failure: the code ticket is scoped as wiring and turns into three new collectors, with no FR stating their pagination or incompleteness rules.
- **FND-003** (FR-024): Failure: blast-radius queries over Dependencies miss FR-026 when FR-024's catalog changes.

## Review evidence

- SWM-132 (same project, milestone 'Evidence and metrics') owns 'complete paginated issue state transitions, blocker edges, linked PRs and commits for an explicit reporting window; reuse existing collectors' and persisted versioned evidence. SWM-141 Scope: 'Reuse existing readers ... and the attributed evidence from SWM-132; no duplicate collectors.'
- Existing readers: IssueHistory (src/linear.rs:82, drift/FR-008), GitHub PR list with created/merged timestamps (src/reconcile.rs:49, src/drift.rs:196). No comment reader exists, and the PR list payload used by reconcile has no additions/deletions.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The upstream entry now names the drift issue-history reader (FR-008) and states that retro reuses SWM-132's reader and does not duplicate it; FR-028:45 repeats this. Proof: spec/functional/FR-024-retro-snapshot.md:99-99. |
| FND-002 | fixed | The new readers are now listed explicitly as groundwork: comment markers, PR size, milestones, PR timeline, check-runs, and local git and disk reads. Proof: spec/functional/FR-024-retro-snapshot.md:99-99. |
| FND-003 | still-open | FR-025's upstream (now includes FR-024) and downstream (now includes FR-026) are fixed. FR-024's Downstream (line 100) still lists only FR-025, FR-027 and FR-028, although FR-026 lists FR-024 upstream and FR-024-AC-13 and AC-14 depend on FR-026. Proof: spec/functional/FR-024-retro-snapshot.md:100. |

FND-001 after_excerpt:

> - **Upstream**: [US-009](../usecase/US-009-review-work-convergence.md); [FR-005](./FR-005-structured-data-boundary.md); the readers behind [FR-018](./FR-018-agent-activity-verdicts.md), [FR-021](./FR-021-project-reconciliation.md), [FR-022](./FR-022-blocker-chain.md) and [FR-023](./FR-023-unprojected-issues.md); the issue-history reader that drift uses ([FR-008](./FR-008-drift-signals.md)). New readers this feature adds are limited to Linear comment markers, pull-request size, issue milestones, pull-request timeline, check-runs and local git/disk reads. Linear ticket SWM-132 collects attrib...

FND-002 after_excerpt:

> - **Upstream**: [US-009](../usecase/US-009-review-work-convergence.md); [FR-005](./FR-005-structured-data-boundary.md); the readers behind [FR-018](./FR-018-agent-activity-verdicts.md), [FR-021](./FR-021-project-reconciliation.md), [FR-022](./FR-022-blocker-chain.md) and [FR-023](./FR-023-unprojected-issues.md); the issue-history reader that drift uses ([FR-008](./FR-008-drift-signals.md)). New readers this feature adds are limited to Linear comment markers, pull-request size, issue milestones, pull-request timeline, check-runs and local git/disk reads. Linear ticket SWM-132 collects attrib...

## New findings (disposition pass 2)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-004 | medium | FR-024 invents the `gate_log` JSONL format, but no requirement, ticket or existing tool produces it. `full_gates_max`, `repins_total` and `build_lock_wait_minutes` therefore have no real source and are `not configured` in practice. | spec/functional/FR-024-retro-snapshot.md:42 |

- **FND-004** (FR-024): The plan's Deliverable 3 sources these from gate logs and a lock log that lanes keep ad hoc. Failure: no lane writes this exact schema, so the three metrics never become `ok` and are dropped in practice, or each lane writes a variant that ix-board rejects. Fix: name the producer (an existing gate or lock script and its real output format, read as it is) or file the producer as a tracked dependency, with project, milestone and relation.

## Dispositions (round 2)

| FND | outcome | reason |
| --- | --- | --- |
| FND-003 | fixed | FR-024 Downstream now lists FR-026. Proof: spec/functional/FR-024-retro-snapshot.md:111-111. |

FND-003 after_excerpt (round 2):

> - **Downstream**: [FR-025](./FR-025-retro-store.md), [FR-026](./FR-026-retro-unavailable-not-zero.md), [FR-027](./FR-027-retro-delta.md), [FR-028](./FR-028-retro-backfill.md).

## Dispositions (round 3)

| FND | outcome | reason |
| --- | --- | --- |
| FND-004 | fixed | The gate_log producer is named as the team gate script under tracked Linear ticket SWM-145 (project Planner-Lead Focus Monitoring, milestone Evidence and metrics, blocks SWM-144, related to SWM-141; verified with linear issue view/relation list). Its line format matches FR-024:42. Proof: spec/functional/FR-024-retro-snapshot.md:42-42. |

FND-004 after_excerpt (round 3):

> | `gate_log` | Optional path to the JSONL gate log that the team gate script emits (Linear SWM-145) whose lines carry `pr`, `kind` (`gate` or `repin`), `requested_lock_at`, `acquired_lock_at`, `started_at` and `finished_at`. |

