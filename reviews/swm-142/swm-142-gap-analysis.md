---
id: SR-6031
title: "ix-board retro snapshot gap analysis (SWM-142)"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@585c3f2c42c3dbffe68da3e07af6176fa8377c01; FR-024 AC-1..AC-18, FR-025 AC-1..AC-7, FR-026 AC-1..AC-6, US-009-EX-1, tests/retro.rs, src/retro/**"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-024"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-025"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-026"
    type: reviews
---

# SR-6031: ix-board retro snapshot gap analysis (SWM-142)

## Summary

Ticket: SWM-142. Planless gap analysis against the computed matrix
(`quoin matrix`, quoin 0.29.4, quire 0.38.3) and the tagged tests in
tests/retro.rs. Plan completion: not assessed. FR-027 and FR-028 are out of
scope. Each test was read and checked by mutating the code in my head:
would the test fail if the clause it claims broke?

## Verdict

Every FR-024, FR-025 and FR-026 criterion is tagged (`tagged` in the matrix;
no run evidence is bound, as for the rest of the repo). Most oracles are
strong. Examples: the 24 h idle boundary at exactly the threshold, 1000 vs
1001 oversize lines, the 7200 vs 7201 s orphan age, the dedup of path issues
in `net_closed_minus_new` (it fails without the dedup: 3 instead of 1),
byte-identical first line, the symlink and dangling-link refusals, and the
torn final line.

Two AC clauses have no failing oracle, because the fixture never sets up the
case the clause excludes. In both, the mutation survives the whole suite.
US-009-EX-1 is covered by FR-024-AC-1 together with FR-025-AC-3.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-024-AC-2 "path_closed counts only completions inside the window" has no failing oracle. No path issue has a completion outside the window while being updated inside it, so deleting `frame.contains(change.at)` in `changed_in_window` passes every test. SWM-8, the only completion outside the window, is not on the path and is skipped by its `updated_at` anyway. | tests/retro.rs:534-542, src/retro/metrics.rs:412-416 |
| FND-002 | medium | FR-024-AC-6 "count only pull requests that are open or were merged inside the window" has no failing oracle. The out-of-window PR (x#500) is left out of the listing fixture, so replacing `relevant_prs` with all listed PRs passes. The live listing does include PRs merged before the window but updated inside it. The same gap applies to `head_shas_per_pr_max`. | tests/retro.rs:1217-1237, src/retro/metrics.rs:761-765 |
| FND-003 | low | FR-024-AC-6's human-comment and non-first-line cases are removed by the fixture's own `starts_with(prefix)` filter before `marker_fields` sees them, so a looser `marker_fields` (for example `contains("<!--")`) would not be caught. | tests/retro.rs:130-136, tests/retro.rs:650-656 |
| FND-004 | low | The FR-024 main_red definition says a failing run with no earlier passing commit is not an incident and pending commits are skipped. No fixture starts with Fail or contains Pending, so neither rule is checked (it is not AC text; AC-15 passes). | tests/retro.rs:893-937, src/retro/metrics.rs:994-1022 |
| FND-005 | low | FR-026-AC-2 builds the PR listing failure with `read_all_pages` in the test, which is the same as a plain GitHub error (FR-026-AC-1). The live early stop in `LiveGithub::list` (`reached_cutoff`) has no test. | tests/retro.rs:1538-1558, src/retro/live.rs:273-285 |
| FND-006 | low | FR-024-AC-2's partial-expansion test checks only `path_remaining` and `intake_new`, not the other path-dependent metrics, which lets SR-6030 FND-001 (`done_reopened` computed from a partial path) pass. | tests/retro.rs:544-556 |
