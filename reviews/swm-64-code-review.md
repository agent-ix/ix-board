---
id: SR-6020
title: "Code review — ix-board priority-goal reporting"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; README.md, plugin/skills/ix-board-planning/SKILL.md, src/drift.rs, src/linear.rs; FR-008-AC-11..16, FR-009-AC-8..10"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-008", type: references}
  - {target: "ix://agent-ix/ix-board/FR-009", type: references}
---

## Summary

Reviewed PR #57 at `d3be5b030350b703257f5adb96fbc7beb6df3a8e` against base `73d54c5392ff2ae16f831fa9b8bc8e18af83d37b`, including the Rust producer/evaluator, tests, user-facing docs, and the priority-goal output contract. The change has three behavior defects: the text output does not implement the specified format, live collection omits required cross-team/history evidence, and evaluator crowding includes events outside the specified transition-only interval.

## Verdict

**FAIL** — the live report can omit qualifying crowding work and both output formatting and event selection disagree with FR-009/FR-008.

## Scope

- `FR-008-AC-11` through `FR-008-AC-16`: distinct workers, progress timing, crowding, incomplete evidence, and terminal goals.
- `FR-009-AC-8` through `FR-009-AC-10`: JSON/text output fields, missing-history output, and QSL L2 fixture.
- `src/drift.rs`: `evaluate_priority_goals`, `render_drift_text`, QSL L2 and blocker tests, incomplete-evidence test.
- `src/linear.rs`: `collect_drift_evidence`, priority goal/blocker selection, and history query selection.
- `README.md` and `plugin/skills/ix-board-planning/SKILL.md`: priority-goal consumer guidance.
- Repo conventions and gates: `CLAUDE.md`, `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, and `deny.toml`; no gate configuration changed.

## Examined Requirements

- **FR-008-AC-11** (`spec/functional/FR-008-drift-signals.md`; examined): A fixture with two active goal issues assigned to the same Linear assignee and a third unassigned issue reports one active worker; an open blocker assigned to a second assignee increases the count to two, while another blocker assigned to the first assignee does not. Additional issues assigned to existing assignees do not increase the count. Only Coding/In Progress (including equivalent `started` custom states) contribute, and goal issues plus their open blockers are counted together by distinct assignee ID.
- **FR-008-AC-12** (`spec/functional/FR-008-drift-signals.md`; examined): A fixture with a Completed transition and a later state transition reports both timestamps and computes elapsed hours from the later transition; a linked commit/PR or metadata-only edit does not reset the goal clock. With no transitions, the earliest known goal issue creation time is the baseline; without a complete baseline the elapsed value is unavailable.
- **FR-008-AC-13** (`spec/functional/FR-008-drift-signals.md`; examined): The QSL L2 replay at `2026-10-05T18:00:00Z` with a 12-hour threshold and minimum of one reports 13 hours since the 05:00Z goal transition, preserves the 06:00Z prior-day completion, reports two distinct active assignees against the minimum of one, and includes `IR-626`, `QSL-635`, and `QSL-641` in the IR-path `crowding_out` group; it flags the stale goal.
- **FR-008-AC-14** (`spec/functional/FR-008-drift-signals.md`; examined): A fixture with complete history groups all advancing issues from other projects by team and project in `crowding_out`, including all ticket IDs across teams; cross-team tickets assigned to a counted goal worker also appear in `out_of_team_activity`. Goal issues and blockers are excluded, and grouping/order is deterministic.
- **FR-008-AC-15** (`spec/functional/FR-008-drift-signals.md`; examined): Missing current issue/assignee, goal-membership, blocker, or relevant history pages make dependent counts/timestamps unavailable and produce explicit missing-source reasons; status is `unknown` unless a positive finding is already proven, in which case it is `flagged`. Partial crowding evidence is retained, but partial evidence never yields `on_track`, `flagged=false`, or a complete empty list.
- **FR-008-AC-16** (`spec/functional/FR-008-drift-signals.md`; examined): With complete evidence showing every goal issue is terminal, the report has no staffing-shortfall finding and returns `on_track`; with an open goal issue, the configured worker minimum applies.
- **FR-009-AC-8** (`spec/functional/FR-009-drift-alerts-output.md`; examined): A priority-goal fixture serializes every required JSON field with the specified type, enum, ordering, and nullability; `active_workers` is a distinct-assignee count and not an issue count. The text form emits exactly one stable line per goal with the specified keys and values, and matches the JSON assessment.
- **FR-009-AC-9** (`spec/functional/FR-009-drift-alerts-output.md`; examined): With complete evidence and no recorded completion or transition, both completion/transition timestamps are `null` and hours use the earliest known goal-issue creation baseline; with missing history or baseline, dependent values are `null`, `trusted=false`, and `missing_sources` names the gap. Status is `unknown` unless a positive finding is independently proven, in which case it is `flagged`; proven crowding groups remain present without implying completeness.
- **FR-009-AC-10** (`spec/functional/FR-009-drift-alerts-output.md`; examined): The QSL L2 fixture renders the 12-hour threshold, 13 hours since progress, two distinct active assignees against the configured minimum of one, the latest completion and transition timestamps, and crowding tickets `IR-626`, `QSL-635`, and `QSL-641`; JSON and text identify the same stale/crowding status and ticket groups.

## Examined Implementation and Tests

- `src/linear.rs:377-389` (examined): selects history issue IDs using `window_start` and the goal-team/current-worker filter.
- `src/linear.rs:427-449` (examined): queries issue history only for the selected ID set.
- `src/drift.rs:1204-1308` (examined): derives the goal stale interval and groups transitions, PRs, and commits.
- `src/drift.rs:2142-2165` (examined): renders the priority-goal text line.
- `src/drift.rs:2667-3093` (examined): QSL L2, blocker, serialization, and incomplete-evidence unit tests.
- `README.md` and `plugin/skills/ix-board-planning/SKILL.md` (examined): consumer-facing explanation of active-worker counts and evidence semantics.
- No applicable `AssuranceProfile` was found under `spec/`.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | `render_drift_text` emits a different wire shape from FR-009-AC-9: it prints an unquoted goal after `priority_goal `, uses `active_workers=` instead of `workers=`, renders lists as ad-hoc strings (including `none`), and uses `missing=` instead of `missing_sources=`. It also prints absent timestamps as `unknown` even when complete evidence establishes no such event, where the contract requires `null`. The new test checks only the `active_workers=2/1` substring, so this mismatch passes. | src/drift.rs:2142-2165; src/drift.rs:2869; spec/functional/FR-009-drift-alerts-output.md:75-82 |
| FND-002 | high | Live history collection only requests histories for recently updated issues within the general `window_hours` and only when they share a goal team or are assigned to a counted worker. A qualifying transition on another team by another worker, or a same-team transition within `no_progress_hours` but outside `window_hours`, is never loaded. The evaluator then receives no transition and the collector records no missing-history reason, so `crowding_out` omits required tickets despite FR-008 requiring every other project in the goal-specific interval. | src/linear.rs:377-389; src/linear.rs:427-449; spec/functional/FR-008-drift-signals.md:148-155 |
| FND-003 | medium | The evaluator adds linked PR and commit activity directly to `crowding_out`, although FR-008-AC-13 defines qualifying work as issues that transitioned in `[report_time - no_progress_hours, report_time)`. Its transition predicate also includes an event exactly at `report_time` (`at > report_epoch` skips only later events), contrary to the exclusive upper bound. A recent PR/commit without a transition, or a transition exactly at report time, can therefore create crowding and flag a goal without a qualifying event. | src/drift.rs:1204-1209; src/drift.rs:1239-1308; spec/functional/FR-008-drift-signals.md:148-155 |

## Review Notes

The changed tests cover repeated goal assignees, blank assignees, one distinct direct blocker worker, transitive blocker traversal, serialized JSON fields, and missing Linear issue evidence. I inspected tests and matrix bindings but did not execute the Rust test suite or other CI gates in this review.
