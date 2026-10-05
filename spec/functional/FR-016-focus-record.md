---
id: FR-016
title: Produce an ix-board-owned focus record for shared storage
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/FR-011"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-012"
    type: depends_on
---
# FR-016: Produce a versioned focus record

## Description

`ix-board --json focus report` SHALL emit one `FocusRecord` with an explicit
schema version, canonical Linear-ID selectors, the report time, and the
complete ix-board focus status. The library SHALL expose the same Rust type
and a generated JSON Schema. The record SHALL be computed from one workspace
snapshot, so its selector resolution and status refer to the same source data.
The report command SHALL be read-only and SHALL NOT assume a transport or
storage service.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-016-AC-1 | A name-based selection produces canonical ID selectors, a version, a report timestamp, and the same status facts as `focus status` over that snapshot. | Test |
| FR-016-AC-2 | `FocusRecord` has an exportable structural JSON Schema. | Test |
| FR-016-AC-3 | An empty or unresolved selection fails without a partial record. | Test |
