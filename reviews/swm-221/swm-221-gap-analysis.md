---
id: SR-6711
title: "SWM-221 retro eval gap analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@7d500013413471112318d4e8b8727089b54d8ed6; spec/functional/FR-047-retro-eval.md, src/retro/eval.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-047"
    type: reviews
---

# SR-6711: SWM-221 retro eval gap analysis

## Summary

Ticket: SWM-221. Planless gap analysis of FR-047 and the ticket criteria against the head, using quire matrix, quire trace, a run of the command over the six dev-team role specs (origin/main cfb9225) and source inspection. Of 104 deterministic rules, 15 reach a source adapter, 13 route to a retro adapter that cannot find them and 76 have none. Semantic review of every test was not run.

## Verdict

**FAIL** - seven of eight acceptance criteria are untagged and the ticket criteria for per-session rows, objective metrics and a producer for every rule are unmet. Plan completion: not assessed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | 76 of 104 deterministic rules have no producer: 31 ledger, 20 transcript, 11 relay, 9 linear, 4 github, 1 prom. 65 return a generic `<source> evidence is unavailable for this evaluation window` reason (11 relay rules say no registered adapter), which reads as a passing outage. None carries an exact reason it can never have a producer; the only intended not_applicable row is overridden. Views already queryable and not registered include ORCH-R09 and MIND-R02; the 14 views #102 adds are not registered either. No transcript query source exists (required by the SWM-221 comment). The ticket requires every evaluation to be run. | src/retro/eval.rs:2344-2354 |
| FND-002 | high | Session rows come only from the ledger session_turn table, which nothing writes on this head, #103 or #102. `sessions` is always empty in production, so the ticket criterion `Per session: a row per rule` is unmet. The hooks.role_turns adapter is never reached because no deterministic rule uses that reference (it is the trigger column). | src/retro/eval.rs:209-227 |
| FND-003 | high | Only the `## Evaluations` table is parsed. The role specs `## Metrics` tables are never read and the JSON has no metric rows, so the ticket criterion `plus objective metrics` is unmet. Eleven adapters written for references no deterministic rule uses (reviewed_prs, review_markers, disposition_rounds, review_findings, gate_records, gate_log_runs, merged_prs, lane_orders_due, need_ready_tickets_due, ready_buffer_deficit_mean, role_turns) never run. | src/retro/eval.rs:335-345 |
| FND-004 | high | All 25 tests in eval.rs use the path-qualified `#[ix_trace_rs::trace(..)]`, which the tools do not read. quire matrix reports FR-047-AC-1 to AC-6 untagged and quire trace lists each tag as evidence_near_miss. Only AC-7 (main.rs, bare `#[trace]`) is bound. Use `use ix_trace_rs::trace;` and the bare attribute. | src/retro/eval.rs:2441-2443 |
| FND-005 | medium | FR-047-AC-8 (read-only evaluation) has no tagged test and no implements marker. | spec/functional/FR-047-retro-eval.md:88-88 |
| FND-006 | medium | No test runs the composed adapter assembly in run_retro_eval, where extend order decides the result. This test passes on declared_adapters alone while the command reports pass for the same rule; the retro stored-snapshot test likewise covers an adapter the command always overrides. | src/retro/eval.rs:2629-2652 |
| FND-007 | medium | Ticket criterion 3 says the matching _lead-bin shell scripts are replaced. The PR names no script, removes or replaces none and does not mention the criterion. | src/retro/eval.rs:1-4 |
| FND-008 | low | implements markers do not match behaviour: gate-record functions claim AC-6 (registry resolution), adapters that return wrong values claim AC-1, and the new query functions read_reviewed_prs, pr_methods_count, read_merged_prs, gate_log_runs_count and ready_buffer_mean_from_response carry none. | src/retro/eval.rs:1483-1485 |

## Scope

- FR-047-AC-1 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-2 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-3 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-4 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-5 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-6 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-7 (examined): spec/functional/FR-047-retro-eval.md
- FR-047-AC-8 (examined): spec/functional/FR-047-retro-eval.md
- build_pause_metric_is_not_applicable_without_a_producer (examined): src/retro/eval.rs

## Coverage

Plan completion: not assessed. Matrix: FR-047-AC-7 tagged; AC-1 to AC-6 and AC-8 untagged. Semantic review of every test was not run.

## Dispositions

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | still-open | Relay and transcript reducers are implemented and tested in isolation, but `run_retro_eval` never invokes them, so rules cannot read those sources. |
| FND-002 | fixed | Session rows now read event-window `session_turn` records produced by the fleet status path. |
| FND-003 | fixed | Role/session results now populate `objective_metrics` from each role spec's Metrics table. |
| FND-004 | fixed | The prior path-qualified trace attributes were replaced by bare imports and attributes; matrix shows AC-1 through AC-7 and AC-9 tagged. |
| FND-005 | still-open | FR-047-AC-8 remains untagged; the matrix reports zero binders. |
| FND-006 | still-open | Tests exercise reducers and serialization, but no test executes the composed adapter assembly in `run_retro_eval`. |
| FND-007 | still-open | The ticket criterion to replace the matching `_lead-bin` shell scripts is not represented by a script replacement in this PR. |
| FND-008 | fixed | Current test attributes and implementation markers use the bare trace form and their corresponding acceptance criteria. |

### Round 2

| FND | Outcome | Reason |
| --- | --- | --- |
| FND-001 | still-open | Relay and transcript adapters remain absent from the command assembly. |
| FND-005 | still-open | AC-8 remains untagged. |
| FND-006 | still-open | No test exercises the composed command adapter assembly. |
| FND-007 | still-open | The requested `_lead-bin` shell-script replacement remains outside this diff. |
