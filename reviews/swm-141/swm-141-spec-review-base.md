---
id: SR-6022
title: "Spec review base — ix-board SWM-141 retro"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The base lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**FAIL** — US-009 carries a source-ticket provenance record (checklist P0). The criteria also leave the retro configuration undefined and cover most catalog metrics only through one blanket criterion. ID formats, sequencing, US/FR links and Markdown links are otherwise correct; FR-025's store guard, FR-026's unavailable-not-zero rule and FR-027's no-combined-score rule are well formed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | US-009 stores its source ticket as a `provenance.source: SWM-141` record in frontmatter; remove it. | spec/usecase/US-009-review-work-convergence.md:16-17 |
| FND-002 | medium | The retro configuration file is undefined: no format, field list or error behaviour, and the fields the metrics use (projects, teams, sources, baseline taken_at, herdr workspace) are not among the fields FR-024 says it names. | spec/functional/FR-024-retro-snapshot.md:17-18 |
| FND-003 | medium | FR-024-AC-1 is the only criterion for about 15 of the 24 metric ids, so a wrong counting rule in any one of them cannot be pinned to a criterion. The ticket asks for a criterion per metric. | spec/functional/FR-024-retro-snapshot.md:57 |
| FND-004 | low | US-009-EX-3 runs `ix-board retro backfill --since <date>` without the `--config <file>` that FR-028 requires. | spec/usecase/US-009-review-work-convergence.md:46-51 |

## Finding detail

- **FND-001** (US-009): Checklist Hash/Digest/Pin rule: a provenance record in a requirement artifact is a P0 tracking record. The same pattern was raised as high on FR-023 (SR-2029 FND-002) and removed. Failure: the ticket id becomes a second, unmaintained tracking system that drifts from Linear.
- **FND-002** (FR-024): FR-024:17-18 says the configuration names the team, gate tickets, repositories and thresholds. FR-024:33-34 (`intake_new`, `net_closed_minus_new`) use 'configured projects', FR-024:43 uses 'configured teams' (plural), FR-026:15-16 and AC-3 use 'source not named in the configuration', and FR-027:16-17 uses a configured baseline `taken_at`. 'team' is also one string that has to act as the Linear team key, the herdr workspace label FR-018 matches and the store directory name in FR-025. Failure: one implementer adds a projects field and another derives projects from the team, so `intake_new` differs. A config that leaves out projects makes those metrics permanently `unavailable: not configured`. A missing or malformed config file has no specified exit status.
- **FND-003** (FR-024-AC-1): AC-2..AC-4 cover only path_remaining, path_closed, net_closed_minus_new, prs_merged_per_hour and the cycle percentiles. gates_remaining, intake_new, path_closed_unverified, prs_open, prs_idle_over, prs_oversize, prs_ready_unmerged, fix_rounds_max/total, done_reopened, issues_without_project/milestone, in_progress_stale and agents_* rest only on AC-1's 'value fixed by the fixture', which is compound and names none of their edge cases (idle threshold boundary, size threshold boundary, reopen-then-recomplete). SWM-141 acceptance: 'every metric has an FR with EARS grammar and a TC row'. Failure: a fixture that avoids the threshold boundary passes AC-1 with an off-by-one in prs_idle_over.
- **FND-004** (US-009-EX-3): FR-028:13 makes `--config <file>` mandatory. Failure: a reader following the story runs a command the FR rejects.

## Review evidence

- `quire validate --scope . <the six changed artifacts> spec/spec.md --summary` exits 0; 3/7 docs grammar-clean, 9 advisory grammar warnings (recorded in the ears-conformance SR).
- IDs FR-024..FR-028 and US-009 are new, unique and sequential; AC ids are sequential per FR; every relative link resolves.
- Hash/digest/pin sweep over the diff: the only hit is US-009 `provenance.source` (lines 16-17). `schema_version` and the per-metric `query` field are product data emitted by the feature, not repository bookkeeping, and are not findings.
- No AssuranceProfile exists in the repository; this is a direct review (no enforced ix-flow run).
- `quire matrix` coverage is not assessed: this is a spec-only change ahead of implementation, so all new ACs are untagged by design.

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-005 | low | `gates_remaining` and `path_closed_unverified` still rest only on the blanket FR-024-AC-1; no criterion names either. | spec/functional/FR-024-retro-snapshot.md:57-58 |

- **FND-005** (FR-024-AC-1): AC-2..AC-14 name every other catalog id. Failure: a gate ticket counted as remaining while it is canceled, or a path issue counted as unverified despite an attached merged PR, passes AC-1 when the fixture leaves out that case.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The `provenance` block is removed from the US-009 frontmatter. Proof: spec/usecase/US-009-review-work-convergence.md:1-16. |
| FND-002 | still-open | Format (JSON) and the field list are now defined, and `team`, `linear_teams` and `herdr_teams` are separated (FR-024:24-41). Still missing is the error behaviour the finding named: nothing says what exit status and message a missing, unreadable or malformed config file, or a missing required field, produces, and no AC covers it. Proof: spec/functional/FR-024-retro-snapshot.md:24-41. |
| FND-003 | fixed | AC-1..AC-14 now give per-family criteria with threshold-straddling fixtures (AC-5) and per-metric unavailable cases; residual gap raised as FND-005. Proof: spec/functional/FR-024-retro-snapshot.md:82-95. |
| FND-004 | fixed | US-009-EX-3 now passes `--config <file>`. Proof: spec/usecase/US-009-review-work-convergence.md:47-47. |

FND-001 after_excerpt:

> ---
> id: US-009
> title: Review whether a team's work is converging
> type: US
> relationships:
>   - target: "ix://agent-ix/ix-board/FR-024"
>     type: traces_to
>   - target: "ix://agent-ix/ix-board/FR-025"
>     type: traces_to
>   - target: "ix://agent-ix/ix-board/FR-026"
>     type: traces_to
>   - target: "ix://agent-ix/ix-board/FR-027"
>     type: traces_to
>   - target: "ix://agent-ix/ix-board/FR-028"
>     type: traces_to
> ---

FND-003 after_excerpt:

> | FR-024-AC-1 | Given fixture Linear, GitHub and herdr records, the command appends exactly one record containing every catalog id, with the fixture's expected value or unavailable status for each. | Test |
> | FR-024-AC-2 | `path_remaining` equals the open transitive blockers of the gate tickets and excludes issues that block no gate; it is `unavailable` when the blocker expansion is truncated. `path_closed` counts only completions inside the window. | Test |
> | FR-024-AC-3 | `net_closed_minus_new` is negative for a fixture window with more new issues than completions, counts a completed path...

FND-004 after_excerpt:

> - **When** I run `ix-board retro backfill --config <file> --since <date>`

## New findings (disposition pass 2)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-006 | high | The `gate_log` line format FR-024 defines requires a `head` field, a commit SHA that no metric uses. Remove it. | spec/functional/FR-024-retro-snapshot.md:42 |

- **FND-006** (FR-024): Checklist Hash/Digest/Pin rule. `full_gates_max` keys on `pr`, `repins_total` on `kind`, and `build_lock_wait_minutes` on the lock timestamps (FR-024:78-79), so nothing reads `head`. The usefulness test fails: the field protects no behaviour, yet it obliges whatever writes the log to record a revision SHA on every line. Fix: drop `head` from the field list. It is not a reason to add a use for it.

## Dispositions (round 2)

| FND | outcome | reason |
| --- | --- | --- |
| FND-002 | fixed | FR-024-AC-17 now covers a missing, unreadable or invalid config file and missing required fields, with a non-zero exit, a message naming the file and field, and nothing appended. Proof: spec/functional/FR-024-retro-snapshot.md:103-103. |
| FND-005 | fixed | FR-024-AC-16 now names `gates_remaining` and `path_closed_unverified`, including the merged-PR exclusion. Proof: spec/functional/FR-024-retro-snapshot.md:102-102. |

FND-002 after_excerpt (round 2):

> | FR-024-AC-17 | A configuration file that is missing, unreadable, not valid JSON, or lacks `team`, `gate_tickets`, `projects` or `repos` stops the command with a non-zero exit and a message naming the file and the field, and appends nothing. | Test |

FND-005 after_excerpt (round 2):

> | FR-024-AC-16 | `gates_remaining` counts gate tickets not completed, and `path_closed_unverified` counts a path issue completed inside the window with no merged pull request attached and not one with a merged pull request. | Test |

## Dispositions (round 3)

| FND | outcome | reason |
| --- | --- | --- |
| FND-006 | fixed | `head` is removed from the gate_log field list; the remaining fields are each read by a metric (FR-024:78-79). SWM-145's line format also states 'No commit hashes'. Proof: spec/functional/FR-024-retro-snapshot.md:42-42. |

FND-006 after_excerpt (round 3):

> | `gate_log` | Optional path to the JSONL gate log that the team gate script emits (Linear SWM-145) whose lines carry `pr`, `kind` (`gate` or `repin`), `requested_lock_at`, `acquired_lock_at`, `started_at` and `finished_at`. |

