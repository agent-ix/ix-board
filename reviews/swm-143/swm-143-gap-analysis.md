---
id: SR-6034
title: "ix-board retro delta and backfill gap analysis (SWM-143)"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@5a0d69446967df5bb0140711fdf715fba3f8dec2; FR-027 AC-1..AC-8, FR-028 AC-1..AC-7, tests/retro.rs, src/retro/**"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-027"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-028"
    type: reviews
---

# SR-6034: ix-board retro delta and backfill gap analysis (SWM-143)

## Summary

Ticket: SWM-143. Planless gap analysis against `quoin matrix` (quoin 0.29.4)
and the tagged tests in tests/retro.rs. Plan completion: not assessed. All 15
criteria are `tagged`. As in the rest of the repository, no run evidence is
bound. Each oracle was checked with real mutants in a scratch copy, one
`cargo test --test retro` per mutant (see SR-6033 Summary).

## Verdict

FR-027-AC-1..8 and FR-028-AC-2..7 each have a test that fails when the clause
breaks, confirmed by mutation. FR-028-AC-1 and the FR-028 per-record window
rule have no multi-day value oracle (FND-001, medium). FR-027-AC-6 is
satisfied structurally, since `run_delta` has no source parameter, but the
test does not use readers that fail when invoked (FND-002). Two low findings
list author decisions that the spec should state.

Author choices, judged:
- taken_at at 23:59:59Z with window (taken_at-window, taken_at]: acceptable;
  the spec needs this sentence (see SR-6033 FND-001 on the one-second shift).
- `--until` defaulting to the last complete UTC day, and a day that has not
  ended being refused: good; the spec needs this sentence.
- Snapshot inside the range meaning since 00:00 <= at < until+1 day: correct
  reading of "inside the range"; stating it as a half-open interval helps.
- baseline_taken_at matched to the second, and checked before the no-delta
  message: good; the spec needs this sentence.
- regressing computed over snapshot records only, so it needs three
  comparable snapshots: consistent with "backfilled records serve only as
  baseline and history"; the spec needs this sentence.
- Backfill path_closed using today's path: acceptable and documented. The
  docs do not say that intake_new, net_closed_minus_new and done_reopened
  also use today's project membership. The spec needs both sentences.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-028 says each record measures over the trailing window ending at its own taken_at, but no test checks values across more than one day. AC-1 checks only taken_at, and AC-5 and AC-6 each use a one-day range. Measuring every day at the last day's frame (`now: pending.last()`) passes the suite, so a multi-day backfill could write the same numbers on every day and still be green. | src/retro/backfill.rs:91-97, tests/retro.rs:2310-2344, tests/retro.rs:2468-2490 |
| FND-002 | low | FR-027-AC-6 asks for source readers that fail the test if invoked. The test runs the binary with `env_clear()`, which only makes reads fail quietly. A `retro delta` that spawns `gh` and ignores the error passes it. The library signature prevents reads today, so behavior is safe; the oracle is weaker than the AC. | tests/retro.rs:2212-2238, src/main.rs:739-743 |
| FND-003 | low | The FR-028 statement does not define "end of that day" (23:59:59Z and the window (taken_at-window, taken_at]), the `--until` default (last complete UTC day), the refusal of a day that has not ended, the half-open snapshot range, or that path and project membership are today's. The code and docs decide all of these. | spec/functional/FR-028-retro-backfill.md:14-30, docs/retro.md:95-115 |
| FND-004 | low | The FR-027 statement does not say that baseline_taken_at matches to the second, that the baseline error comes before the no-delta message, or that regressing uses snapshot records only and so needs three snapshots. The code and docs decide all of these. | spec/functional/FR-027-retro-delta.md:14-35, docs/retro.md:77-93 |
