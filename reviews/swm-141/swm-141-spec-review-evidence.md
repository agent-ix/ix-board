---
id: SR-6025
title: "Spec evidence review — ix-board SWM-141 retro"
type: SpecReview
analysis: evidence
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The evidence lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**FAIL** — FR-028-AC-5, the pilot reproduction criterion, cannot be evaluated against the records FR-028 produces and has no stated tolerance. All other criteria name Test or Inspection appropriately; most are fixture-testable.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-028-AC-5 checks a whole-window median and p90 against backfill output, but FR-028 writes only per-day records. No record carries the 7-day values, and the tolerance is left to 'the fixture'. The criterion has nothing to compare and no bound that can fail. | spec/functional/FR-028-retro-backfill.md:34 |
| FND-002 | medium | FR-026-AC-4 needs 'changing the counting rule between two runs', but counting rules are compiled in, so no test can change one between runs. The criterion is untestable unless `query` is defined to include the configured thresholds and window. | spec/functional/FR-026-retro-unavailable-not-zero.md:32 |
| FND-003 | low | FR-024-AC-5 (no writes, no prose reads) and FR-027-AC-6 (no network) are Inspection. A Test with refusing or recording source traits, as FR-018-AC-5 already uses, would fail on regression. | spec/functional/FR-024-retro-snapshot.md:61 |

## Finding detail

- **FND-001** (FR-028-AC-5): A median over 7 days cannot be derived from 7 daily medians. The tolerance 'stated in the fixture' lets the fixture author make the criterion pass. The recorded 86 PRs at 3.5 per hour spans about 25 hours, so the recorded figures probably do not describe the stated window or the configured repository set (confidence medium on that point). Failure: AC-5 passes for any implementation, or fails for a correct one. Fix: state the comparison unit (a whole-range aggregate, or the sum of daily `prs_merged`), the repositories, the window bounds and a numeric tolerance in the AC.
- **FND-002** (FR-026-AC-4): FR-026:19-21 says `query` states the counting rule but not which inputs it includes. Failure: a test cannot exercise AC-4 without two builds, so the criterion is tagged but never actually checked. Fix: define `query` as rule id plus window plus thresholds, and change a configured threshold in the test.
- **FND-003** (FR-024-AC-5): Inspection is not re-run when the code changes. Failure: a later change adds a Linear mutation or a network call in delta, and no criterion fails.

## Review evidence

- Plan Deliverable 3 records '86 PRs merged / median 0.6h / p90 4.1h / 3.5 per h'; 86 / 3.5 per hour ≈ 24.6 h, not the 168 h of 2026-10-31..11-07.
- FR-028 emits one record per UTC day; FR-024 percentiles are computed per record window.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | AC-5 now compares one per-day record against a synthetic fixture with exact equality inside a stated 24h trailing window; no tolerance, no real data. Proof: spec/functional/FR-028-retro-backfill.md:39-39. |
| FND-002 | fixed | `query` now includes the window and thresholds, and AC-4 (line 36) varies `--window` or a threshold between runs, which a test can do. Proof: spec/functional/FR-026-retro-unavailable-not-zero.md:23-25. |
| FND-003 | fixed | FR-024-AC-12 is now a Test with recording source doubles; FR-027-AC-6 (FR-027:44) is a Test with readers that fail if invoked. Proof: spec/functional/FR-024-retro-snapshot.md:93-93. |

FND-001 after_excerpt:

> | FR-028-AC-5 | For a synthetic fixture of pull-request timestamps with a known count, median and 90th percentile inside a 24h trailing window, the backfilled record for that day equals those values exactly; the fixture holds no real project data. | Test |

FND-002 after_excerpt:

> `query` SHALL state the counting rule used together with the window and the
> thresholds applied, so that two records computed by different rules or windows
> are distinguishable. One unavailable metric SHALL NOT stop the other metrics

FND-003 after_excerpt:

> | FR-024-AC-12 | The command makes no Linear mutation, no GitHub write and no repository write; test source doubles that record writes see none. | Test |

