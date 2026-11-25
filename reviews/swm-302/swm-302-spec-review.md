---
id: SR-6142
title: "SWM-302 FR-027 base spec review"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@1eecb5141742e61ee43dc8ab28d66d01bf79a96d; spec/functional/FR-027-retro-delta.md"
review_set: subset
---

# SR-6142: SWM-302 FR-027 base spec review

## Summary

Ticket: SWM-302. PR agent-ix/ix-board#100. Base checklist over the changed paragraph and FR-027-AC-9.
No AssuranceProfile was found to apply.

## Verdict

**CONDITIONAL**: one low finding. No hash, digest, pin or tracking-record antipattern in the diff.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | FR-027-AC-9 joins three obligations (snapshot record content, delta output, legacy decoding) and puts the snapshot record obligation in the read-only delta requirement; FR-024, which owns the snapshot output and lists its `--json` fields in AC-14, does not mention the classes. | FR-027-AC-9 |

## New findings (disposition pass 1)

Head a08cbe8d22a89747a362e1181f0be032d31f81f3; fix diff only.

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-002 | low | FR-027 and AC-9 condition resolution on stored metrics that omit class metadata and never say records do not store it; a stored metric cannot carry it, and a record with `class` is rejected as an unknown field. | FR-027-AC-9 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Delta output and legacy decoding are now separate criteria and the snapshot record obligation is removed from the delta requirement. |
| FND-002 | fixed | FR-027 and AC-9 now state that store records never contain the fields and AC-9 drops the condition; the description keeps one now-vacuous sentence beginning `When a stored metric has no class metadata`. |

## New findings (disposition pass 2)

Head f2e972f4099055b1946355247f0d2dc3aa0e15f8; this round's diff only.

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-003 | low | The rule that store records never contain the class fields is now stated in both FR-027 (description and AC-9) and FR-024-AC-14, and AC-9 again joins a store obligation to a delta output criterion. | FR-027-AC-9 |
