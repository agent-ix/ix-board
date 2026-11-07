---
id: SR-6027
title: "Spec scope boundary review — ix-board SWM-141 retro"
type: SpecReview
analysis: scope-boundary
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The scope-boundary lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**FAIL** — the catalog drops metric families SWM-141 requested, and the pilot fixture would put retro data into a public repository. The boundary against Linear/GitHub writes, and against subjective scoring (kept with the agent per the plan), is drawn correctly.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-024 declares main-branch status, disk usage and undelivered messages outside the catalog, and leaves out other requested metrics without saying so: head SHAs per PR, re-pins, full gates per PR, time-red, prunable worktrees, orphan slots, source count disagreement, decision latency. That drops requested features. | spec/functional/FR-024-retro-snapshot.md:48-51 |
| FND-002 | medium | The FR-028-AC-5 pilot fixture is real QSL L4 retro data (PR timelines and counts). As a test fixture it would be committed to the public ix-board repository, against 'nothing from this feature may be written into any git repo'. | spec/functional/FR-028-retro-backfill.md:34 |

## Finding detail

- **FND-001** (FR-024): Peter's rule: never drop a requested feature; spec the whole feature or ask. SWM-141 names these families explicitly. Failure: the skill's measures.md, built from this spec, cannot flag the 5 h main-red or the resource churn from the plan's findings 8 and 9, which are incidents the retro exists to catch. Fix: add the missing ids with their structured sources. Sources ix-board does not read yet (main status check, gh timeline, git worktree list, du, agent-msg log) become configured sources that report `unavailable: not configured` under FR-026. Leaving them out of the catalog is not an option.
- **FND-002** (FR-028-AC-5): FR-025 keeps retro data out of every repository, but AC-5's fixture is retro data. Failure: QSL PR history ends up in a public repo. Fix: a synthetic fixture with the same statistical shape for the Test, and the pilot replay as an out-of-repo run against the store.

## Review evidence

- SWM-141 Scope requests: rework (head SHAs/PR, re-pins, full gates/PR, false-Done reopens), stability (main-red incidents, time-red), scope health (intake/day, source count disagreement), utilization (undelivered messages), resource (target/clone disk, prunable worktrees, orphan slots), decision latency.
- `gh repo view` reports agent-ix/ix-board visibility PUBLIC.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | still-open | head_shas_per_pr_max, main_red_incidents, source_count_disagreement, worktrees_prunable and disk_bytes are now real catalog ids. Three gaps remain. (a) time-red (detect to green) and orphan slot wrappers from SWM-141's Scope are still absent, under any id. (b) full_gates_per_pr, repins, build_lock_wait_minutes, messages_undelivered and decision_open_age_hours are reserved with no definition, source or counting rule and are permanently `unavailable: not configured` (FR-024:73). That contradicts FR-024:45-47, which requires one definition, one counting rule and one unit for every id, and it is still a drop in effect, because no configuration can ever make them `ok`. (c) messages_undelivered has a structured source the plan names (agent-msg / ix-relay log), as does main-red time from check-runs. Fix: define each reserved id with a source that can be configured, or ask Peter to rule which can be dropped. Proof: spec/functional/FR-024-retro-snapshot.md:66-73. |
| FND-002 | fixed | AC-5 now uses a synthetic fixture that 'holds no real project data'. Proof: spec/functional/FR-028-retro-backfill.md:39-39. |

FND-002 after_excerpt:

> | FR-028-AC-5 | For a synthetic fixture of pull-request timestamps with a known count, median and 90th percentile inside a 24h trailing window, the backfilled record for that day equals those values exactly; the fixture holds no real project data. | Test |

## Dispositions (round 2)

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Every previously dropped metric is now a defined catalog id with a source that can be configured: main_red_minutes (line 70), orphan_slots (77), full_gates_max, repins_total and build_lock_wait_minutes (78-79), messages_undelivered (80), and decisions_open and decision_oldest_hours (81). The reserved-id row is removed, and AC-11 and AC-15 cover the new ids. The gate_log producer gap is raised separately as SR-6024 FND-004. Proof: spec/functional/FR-024-retro-snapshot.md:77-81. |

FND-001 after_excerpt (round 2):

> | resource | `orphan_slots` | Processes whose name contains `slot_process`, whose parent has exited and whose age exceeds `orphan_slot_hours`, from the local process table. |
> | rework | `full_gates_max`, `repins_total` | From `gate_log` lines finished inside the window: the largest number of `gate` lines for one `pr`, and the count of `repin` lines. |
> | resource | `build_lock_wait_minutes` | Sum of `acquired_lock_at` minus `requested_lock_at` over `gate_log` lines finished inside the window. |
> | utilization | `messages_undelivered` | Messages pending longer than `message_stale_minutes` in t...

