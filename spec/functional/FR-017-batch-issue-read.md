---
id: FR-017
title: Read multiple exact Linear issues in one request
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
---
# FR-017: Read multiple exact Linear issues

## Description

`ix-board issues <identifier>...` SHALL resolve each supplied Linear issue
identifier or UUID against one cached workspace snapshot and return the exact
structured issue records. It SHALL preserve request order, return an issue
only once when a selector is repeated, and preserve Linear's exact workflow
state display name. An unknown identifier SHALL be reported in a `missing`
list while found issues remain available. An ambiguous identifier SHALL
produce an explicit error without partial output.

The terminal view SHALL show each issue identifier, exact state name, and
title and a `Missing` line when applicable. JSON SHALL return an `IssueBatch`
with `issues`, `missing`, and `complete` fields without deriving state from
ticket prose. A batch with at least one found issue SHALL exit successfully;
a batch where every selector is missing SHALL still print the missing list
and exit non-zero.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-017-AC-1 | A batch of identifiers reads one workspace snapshot and returns exact issues in request order with duplicates removed. | Test |
| FR-017-AC-2 | Unknown selectors are reported once in request order while found issues are returned; an ambiguous selector fails without partial output. | Test |
| FR-017-AC-3 | Text shows exact state names and missing selectors; JSON retains the structured issue records and completeness flag. | Inspection |
| FR-017-AC-4 | When every selector is missing, the CLI emits the missing list and exits non-zero; a partial result exits successfully. | Test |
