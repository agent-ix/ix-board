---
id: SR-2029
title: "Spec review base — ix-board SWM-112 orphans"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board; spec/functional/FR-023-unprojected-issues.md, spec/usecase/US-008-review-program-scope-and-roadmap.md, spec/spec.md"
review_set: subset
---

## Summary

Reviewed FR-023's identity, acceptance criteria, test coverage, cross-references, and tracking metadata. The requirement is unique, structurally valid, and each criterion is tagged, but it is not linked to its applicable user story, its ambiguous-project error path has no test, and it carries source-ticket provenance metadata prohibited by the requirements review checklist.

## Verdict

**FAIL** — the source provenance record is a high-severity tracking artifact under the checklist; the requirement also lacks its user-story cross-reference and leaves one specified error path untested.

## Findings

| ID      | Severity | Summary                                                                          | Refs                                                |
| ------- | -------- | -------------------------------------------------------------------------------- | --------------------------------------------------- |
| FND-001 | medium   | FR-023 is not linked to the program-review user story US-008.                    | spec/functional/FR-023-unprojected-issues.md:5     |
| FND-002 | high     | FR-023 stores its source ticket as provenance metadata.                          | spec/functional/FR-023-unprojected-issues.md:6-7   |
| FND-003 | medium   | The ambiguous-project error path in FR-023-AC-1 has no test.                     | src/orphans.rs:70-72                               |

## Review evidence

- `FR-023` occurs once in `spec/`; `quire validate --scope . spec/functional/FR-023-unprojected-issues.md` passed.
- `quire matrix --scope . --format json` reports FR-023-AC-1, AC-2, and AC-3 as tagged by the orphan fixture tests and CLI rendering test.
- AC-1's current test verifies a missing project, but not the separate ambiguous-name branch (`OrphansError::ProjectAmbiguous`); a regression that chooses or mishandles duplicate project names would not fail the suite.
- The full `quire validate --scope . 'spec/**/*.md'` run still reports unrelated errors in existing FR-004, FR-016, FR-017, and a roadmap story document; the new FR-023 document validates independently.
- The requirements checklist requires FR-to-US links and prohibits provenance records in requirement artifacts.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | FR-023 now links to US-008, and US-008 has an acceptance example for `ix-board orphans`. |
| FND-002 | fixed | The `provenance.source` field was removed from the FR-023 artifact. |
| FND-003 | fixed | FR-023-AC-1 now names absent and ambiguous projects, and the suite has an ambiguity regression test. |

FND-001 after_excerpt:

> relationships:
>   - target: "ix://agent-ix/ix-board/US-008"
>     type: implements

FND-002 after_excerpt:

> relationships:
>   - target: "ix://agent-ix/ix-board/US-008"
>     type: implements

FND-003 after_excerpt:

> The command requires a scope file, selects the named projects case-insensitively, reports only unprojected issues for their teams, separates open and completed issues, excludes canceled and duplicate issues, and fails when a scoped project is absent or ambiguous.

