---
id: FR-016
title: Produce an ix-board-owned focus record for shared storage
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-007"
    type: implements
  - target: "ix://agent-ix/ix-board/FR-011"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-012"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-010"
    type: depends_on
---
# FR-016: Produce a versioned focus record

## Description

`ix-board --json focus report` SHALL emit one `FocusRecord` with an explicit
schema version, typed canonical Linear-ID initiative, project, milestone, and
issue members, the report time, and the complete ix-board focus status.

The library SHALL expose the same Rust type and a generated JSON Schema.

The ix-board library SHALL compute the record from one workspace snapshot, so
its selector resolution and status refer to the same source data.

The report command SHALL be read-only.

The report command SHALL NOT assume a transport or storage service.

## Schema versions

**Version 2 (existing, unchanged).** `FocusRecord` with `schema_version` 2 has
exactly the members `schema_version`, `selection` (a list of `{kind, id}`
members with kind `initiative`, `project`, `milestone`, or `issue`),
`reported_at`, and `status`. It carries no team, no exclusions, no milestone
project qualification, and no empty-selection intent, and unknown members are
rejected. `focus report` keeps emitting version 2 for inline selections, and its
bytes, its empty-selection failure, and its schema do not change.

**Version 3 (additive named selection).** A separate versioned document, the
**named selection**, carries the FR-010 named selection for storage by another
service. It has exactly these members, all required:

- `schema_version`: the number 3.
- `team`: one Linear team key.
- `include` and `exclude`: lists of `{kind, id}` members. The `kind` value is
  exactly one of the four literals `initiative`, `project`, `milestone`, or
  `issue`, the same spellings as version 2; the FR-010 ticket member is encoded
  as `issue`, and `ticket` is not a wire value. Any other `kind` value is an
  invalid document and the generated schema enumerates exactly these four. A
  `milestone` member
  also requires `project_id`, and no other kind carries it. An empty `include`
  list is the explicit empty selection. A missing `include` or `exclude` is a
  decode error and never an empty list.

The document holds canonical Linear IDs only, with no labels, expanded issue IDs,
or status. It is encoded in one canonical order (kind, then ID, with the
milestone project as a tie-break) with no duplicate member in a list, so the
same selection always encodes to the same bytes. A member that is in both lists
is kept in both; the exclusion wins at evaluation. The library exposes the Rust
type and a generated JSON Schema for it, and `focus report` and its schema are
unaffected.

**Version handling.** This contract applies only to the additive
named-selection decoder entry point. That decoder accepts exactly version 3 and
checks the version before the other members. Any other version, older or newer,
is a typed unsupported-version error that names the version found and the
version supported. The library never converts, upgrades, or fills in a missing
team, exclusion list, or intent. In particular a version 2 record presented to
the named-selection decoder is refused as unsupported and is never read as a
named selection. The existing version 2 `FocusRecord` Rust deserialization and
its schema are unchanged: this requirement adds no version-first check, no new
error type, and no new refusal to them. A malformed document of version 3, including an
unknown member, a missing required member, a milestone without `project_id`, or
a duplicate member, is a distinct typed invalid-document error, so a consumer can
tell an unsupported version from an undecodable document.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-016-AC-1 | A name-based selection produces canonical ID selectors, a version, a report timestamp, and the same status facts as `focus status` over that snapshot. | Test |
| FR-016-AC-2 | `FocusRecord` has an exportable structural JSON Schema. | Test |
| FR-016-AC-3 | An empty or unresolved selection fails without a partial record. | Test |
| FR-016-AC-4 | Version 2 encodes each focus member as `{kind,id}`; repeated reads of the same snapshot and selection produce the same ordered member list and issue set. Qualified milestone members select assigned issues even when names collide across projects. | Test |
| FR-016-AC-5 | After the named selection exists, `focus report` for an inline selection still emits version 2 with the same members and bytes, an empty or unresolved inline selection still fails with no partial record, and the exported `FocusRecord` schema is unchanged. | Test |
| FR-016-AC-6 | A version 3 named selection with team, include, exclude, and a milestone carrying `project_id` round-trips through encode and decode unchanged; the same selection built in any member order encodes to identical bytes in canonical order; a member present in both lists stays in both. | Test |
| FR-016-AC-7 | A version 3 document with `include: []` decodes as the explicit empty selection; a document with `include` or `exclude` or `team` missing, a milestone without `project_id`, a non-milestone with `project_id`, a duplicate member in one list, or an unknown member is refused as an invalid document and is never read as empty or as a whole-team selection. | Test |
| FR-016-AC-8 | A version 2 `FocusRecord` presented to the named-selection decoder is refused as unsupported, naming versions 2 and 3, with no team, exclusion, or intent supplied and no conversion; versions 0, 1, and 4 are refused the same way; the existing version 2 `FocusRecord` deserialization behaves exactly as before the named selection existed, including for a version 3 document. | Test |
| FR-016-AC-9 | An unsupported version is reported as a different typed error from an invalid document of version 3, and the version is checked before the other members, so a version 4 document with unknown members reports the version. | Test |
| FR-016-AC-10 | The named selection has an exportable structural JSON Schema in which `team`, `include`, and `exclude` are required, a milestone member requires `project_id`, member `kind` is limited to exactly `initiative`, `project`, `milestone`, and `issue`, and unknown members are not allowed; a document with kind `ticket` or any other value is refused as an invalid document; the version 2 schema remains exported separately. | Test |
| FR-016-AC-11 | Canonicalizing named string inputs yields canonical IDs and the milestone's owning project ID, and fails with no partial document for a missing, ambiguous, other-team, or project-mismatched member; the stored document contains no label, expanded issue ID, or status. | Test |

## Dependencies

- **Upstream**: [FR-010](FR-010-explicit-work-scope.md) for named-selection
  membership and outcomes, [FR-011](FR-011-inline-planner-focus.md),
  [FR-012](FR-012-focus-status-and-ready-queue.md), and
  [US-007](../usecase/US-007-share-focus-record.md).
