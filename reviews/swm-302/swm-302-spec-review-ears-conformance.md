---
id: SR-6143
title: "SWM-302 FR-027 EARS conformance review"
type: SpecReview
analysis: ears-conformance
scope: "agent-ix/ix-board@1eecb5141742e61ee43dc8ab28d66d01bf79a96d; spec/functional/FR-027-retro-delta.md"
review_set: subset
---

# SR-6143: SWM-302 FR-027 EARS conformance review

## Summary

Ticket: SWM-302. The engine reports no grammar warning on FR-027; the semantic pass over the three new
statements found one low defect in their subjects.

## Verdict

**CONDITIONAL**: one low finding.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | Two new statements name no component that can act: `The snapshot records used by delta SHALL persist` and `Readers SHALL accept`. It is unclear which commands write the fields (snapshot only, or backfill too) and which readers are bound. | FR-027 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Each statement names `ix-board retro delta` or the `decode_saved_report` reader; the persistence statement is removed. The engine still emits advisory missing-subject and unclassifiable warnings on line 32, where the subject is a bare code span. |
