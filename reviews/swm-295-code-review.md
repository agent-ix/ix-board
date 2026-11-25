---
id: SR-6120
title: "Code review - ix-board unavailable references and roadmap ids"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@97a7efd7e4e3e3118cb7ab69da2a597484ba860b; src/lib.rs, src/reference_warnings.rs, src/scope.rs, src/named_focus.rs, src/program_review.rs, src/main.rs, src/model.rs, src/hygiene.rs, src/linear.rs, src/linear_summary.rs, src/summary_cache.rs, tests/, docs/, plugin/skills/"
review_set: subset
---

# SR-6120: ix-board unavailable references and roadmap ids

## Summary

Ticket: SWM-295 (also SWM-296). PR agent-ix/ix-board#97 at 97a7efd, diffed
against main at 08812f6, with the Rust lane (rust-review, rust-style). Focus
counts, project rows plus outside_projects, roadmap leaf counts and the merge
with SWM-297 hold. Two structured results still present an issue with an
unavailable reference as clean.

## Verdict

**CONDITIONAL** - no high finding; two medium and three low.

Gates: `make ci` exit 0 per target/make-ci.log (649 passed, 0 failed, 4
ignored; licenses and unsafe audit pass). Not re-run by this reviewer. No CI
workflow file is in the diff.

FND-001 and FND-002 were reproduced with a scratch program linked against the
built library, outside the worktree.

Consumers: ix-relay origin/main pins an earlier ix-board revision, builds none
of the changed public structs, deserializes no ix-board CLI JSON, and still
compiles its exhaustive error match because no Error variant was added or
removed. factory-panel reads selected keys of `roadmap --json` and
`focus status`, so the added members do not break it.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | An issue with an unavailable blocker looks unblocked in the dependency fields. `project-chunk` filters the unknown id out of `open_tickets[].blocked_by` (empty list) and puts the issue in blocker layer 0 beside unblocked work; `plan-chunk` gives it dependency_layer 0, which sorts first, so `--size 1` returns only the blocked issue. Before this change both reads failed. Only `warnings` (and plan-chunk `ready: false`) reveal it. | src/lib.rs:1577-1582 |
| FND-002 | medium | The named batch reports a selection and its team union as `complete` with `counts_availability: complete` and no unknown fact while its ready and blocked counts or leaf set depend on an unavailable blocker or parent. FR-012 says such counts are never complete (FR-012-AC-17). Warnings are batch-wide and name no selection or team. | src/named_focus.rs:390-401, src/named_focus.rs:901 |
| FND-003 | low | Warnings are lost or bypass diagnostics outside the focus reads. `agents` calls `next`, which discards them; a missing blocker used to surface as `ready_queue_error` and now silently removes work from `next_ready`. The `next` command prints them with raw `eprintln!`, ignoring `--diagnostics-format`. | src/main.rs:983, src/main.rs:1119-1124 |
| FND-004 | low | The poisoned summary row in the partition test is still keyed `summary:v1` while the reader now uses `summary:v2`, so the summary half of that test can no longer fail if the namespace stopped partitioning summaries. | tests/delta_cache.rs:268 |
| FND-005 | low | The shipped plugin skills do not mention `warnings` or `outside_projects`. An agent following focus-status reports project and milestone rows only, which can total less than the focus, with no warning shown. | plugin/skills/focus-status/SKILL.md:37-42 |
