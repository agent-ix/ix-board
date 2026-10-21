---
id: SR-6021
title: "Gap analysis — ix-board priority-goal reporting"
type: SpecReview
analysis: gap-analysis
scope: "spec/, src/, tests/, README.md, plugin/skills/ix-board-planning/SKILL.md"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-008", type: references}
  - {target: "ix://agent-ix/ix-board/FR-009", type: references}
---

## Summary

Planless repository audit of ix-board's computed requirement matrix, source, and tests for the priority-goal behavior changed by PR #57. All nine priority-goal criteria in FR-008-AC-11..16 and FR-009-AC-8..10 are untagged in the computed matrix; plan completion was not assessed.

## Verdict

**FAIL** — the computed matrix has untagged criteria, including every criterion this PR implements and tests.

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
| FND-001 | high | The computed matrix reports all nine priority-goal criteria FR-008-AC-11..16 and FR-009-AC-8..10 as `untagged`, with no test binders, even though matching unit tests exist in `src/drift.rs`. Those tests therefore do not discharge the requirements in the repository's traceability model; a regression in distinct worker counting, history boundaries, incomplete evidence, or output shape can leave the matrix unchanged. | spec/functional/FR-008-drift-signals.md:205-210; spec/functional/FR-009-drift-alerts-output.md:160-162; src/drift.rs:2667-3093 |

## Coverage

- Reconciliation: `quoin matrix --repo <worktree> --json` (quoin 0.28.3, quire 0.36.2); `quire matrix --scope <worktree> --format json` — no run evidence was read. The computed matrix has 152 criteria: 84 tagged, 60 untagged, 8 method-without-symbol. All nine priority-goal criteria in scope are untagged.
- Plan completion: not assessed
- Criteria: tagged 84 · untagged 60 · tagged-by-ignored-test 0 · method-without-symbol 8
- Evidence: no run evidence (152 criteria)
- Untraced behaviors / stubs: 9 untagged priority-goal criteria; no TODO/FIXME/XXX/todo!/unimplemented! found in the changed source files. Changed behavior maps to FR-008 and FR-009.
- Semantic review: skipped (not opted in)

## Scope

- `FR-008-AC-11` through `FR-008-AC-16`, `spec/functional/FR-008-drift-signals.md:205-210` — each criterion is marked `untagged` by the computed matrix.
- `FR-009-AC-8` through `FR-009-AC-10`, `spec/functional/FR-009-drift-alerts-output.md:160-162` — each criterion is marked `untagged` by the computed matrix.
- `src/drift.rs:2667-3093` — in-memory evaluator tests exist but have no trace bindings for those criteria.
- `src/linear.rs:206-465` — live evidence collection implementation; owned by FR-008/FR-009 and reviewed in SR-6020.
- No explicit implementation plan was supplied; repository-driven planless mode was used.
