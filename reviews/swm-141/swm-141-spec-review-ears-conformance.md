---
id: SR-6029
title: "Spec EARS review — ix-board SWM-141 retro"
type: SpecReview
analysis: ears-conformance
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The ears-conformance lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-025, FR-026, FR-027, FR-028.

## Verdict

**PASS with advisories** — the engine reports 9 advisory grammar warnings across FR-024, FR-025, FR-026 and FR-028; FR-027 is grammar-clean. None of them changes meaning, but each statement maps less cleanly to one AC.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | [quality:agentless-passive] FR-024:27 'SHALL be reported only from the structured source' names no actor; say 'the command SHALL report each id only from...'. | spec/functional/FR-024-retro-snapshot.md:25-27 |
| FND-002 | low | [ears:non-singular] FR-025:18 packs three SHALLs (append with a single write, create the directory, not rewrite). [ears:non-canonical-trigger] line 22 'Before every write' should be 'When the command writes...'. | spec/functional/FR-025-retro-store.md:17-22 |
| FND-003 | low | [ears:unclassifiable][ears:missing-subject] FR-026:19 'A paginated read that ends before its last page SHALL make...' has no system subject. It and line 15 'When a source cannot be read' describe unwanted conditions, so both should be 'If ..., then the command SHALL ...'. | spec/functional/FR-026-retro-unavailable-not-zero.md:15-19 |
| FND-004 | low | [ears:non-singular][ears:missing-subject][ears:unclassifiable][quality:agentless-passive] FR-028:21 'SHALL be idempotent: a day that already has a backfill record SHALL be skipped' is two statements, the second with no subject; say 'If a day already has a backfill record, then the command SHALL skip it and report it'. | spec/functional/FR-028-retro-backfill.md:20-22 |

## Finding detail

- **FND-001** (FR-024): Engine warning at line 27.
- **FND-002** (FR-025): Engine warnings at lines 18 and 22.
- **FND-003** (FR-026): Engine warnings at line 19; the semantic check (unwanted condition written as When) applies to line 15.
- **FND-004** (FR-028): Engine warnings at line 21.

## Review evidence

- `quire validate --scope . <changed artifacts> --summary`: 3/7 docs grammar-clean; ears:missing-subject=2, ears:non-canonical-trigger=1, ears:non-singular=2, ears:unclassifiable=2, quality:agentless-passive=2.

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-005 | low | Splitting FR-025 introduced subject-less statements: line 21 'It SHALL NOT rewrite' and line 26 'A refusal SHALL name' ([ears:unclassifiable][ears:missing-subject]). | spec/functional/FR-025-retro-store.md:19-26 |

- **FND-005** (FR-025): Engine warnings at FR-025:21 and :26. Fix: name the retro command as the subject.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | FR-024 has no grammar warnings now. Proof: spec/functional/FR-024-retro-snapshot.md:45-47. |
| FND-002 | still-open | The non-singular statement is split (FR-025:19-21), but line 23 still opens with the non-canonical trigger 'Before a retro command writes'. Engine: [ears:non-canonical-trigger] line 23. Proof: spec/functional/FR-025-retro-store.md:23. |
| FND-003 | still-open | The engine still reports [ears:unclassifiable][ears:missing-subject] at FR-026:19. Line 16 'When a source cannot be read' still states an unwanted condition with `When` instead of `If ..., then`. Proof: spec/functional/FR-026-retro-unavailable-not-zero.md:16-21. |
| FND-004 | fixed | FR-028 has no grammar warnings now; the idempotence statement is an event-driven `When ...` with a subject. Proof: spec/functional/FR-028-retro-backfill.md:26-28. |

FND-001 after_excerpt:

> The catalog is a fixed list of stable metric ids. Each id SHALL have one
> definition, one counting rule, one unit and one good direction (`up`, `down` or
> `neutral`), and SHALL be reported only from the source named for it. When a

FND-004 after_excerpt:

> When a day already has a backfill record, the command SHALL skip that day and
> report it. When any `snapshot` record has a `taken_at` inside the requested
> range, the command SHALL refuse the range, name that record and append nothing.

## Dispositions (round 2)

| FND | outcome | reason |
| --- | --- | --- |
| FND-002 | fixed | The grammar check now passes FR-025: the trigger is 'While ...' and the refusal is 'If ..., then the retro command SHALL ...'. Proof: spec/functional/FR-025-retro-store.md:23-25. |
| FND-003 | still-open | Line 16 is now 'If ..., then the command SHALL', so the semantic part is fixed. The grammar check still reports [ears:unclassifiable][ears:missing-subject] at FR-026:19, the line where 'The command SHALL NOT report zero or a partial count for such a metric.' and 'If a paginated read ...' share a line. It is advisory only and does not fail `quire validate`. Proof: spec/functional/FR-026-retro-unavailable-not-zero.md:18-21. |
| FND-005 | fixed | Line 21 now names 'The retro command', and the refusal sentence names its subject. The grammar check reports no FR-025 warnings. Proof: spec/functional/FR-025-retro-store.md:19-21. |

FND-002 after_excerpt (round 2):

> While a retro command is about to write, it SHALL resolve the store path, following
> symbolic links, and SHALL refuse the write if the resolved path lies inside a
> git work tree or below a directory that contains a `.git` entry. If the path is refused, then the retro command SHALL name the offending path, exit non-zero and create no file.

FND-005 after_excerpt (round 2):

> When a retro command appends, it SHALL write the whole line with a single write to a
> file opened in append mode, and SHALL create the directory when absent. The retro
> command SHALL NOT rewrite or truncate an existing line.

## Dispositions (round 3)

| FND | outcome | reason |
| --- | --- | --- |
| FND-003 | fixed | The line-19 statement is split into single-obligation statements, each with a subject; the grammar check reports 7/7 docs grammar-clean and 0 grammar findings. Proof: spec/functional/FR-026-retro-unavailable-not-zero.md:20-24. |

FND-003 after_excerpt (round 3):

> The command SHALL NOT report zero or a partial count for an unavailable metric.
> 
> If a paginated
> read ends before its last page, then the command SHALL mark every metric that
> depends on it `unavailable`.

