---
id: FR-015
title: Read multiple exact Linear issues in one request
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
---
# FR-015: Read multiple exact Linear issues

## Description

`ix-board issues <identifier>...` SHALL resolve each supplied Linear issue
identifier or UUID against one cached workspace snapshot and return the exact
structured issue records. It SHALL preserve request order, return an issue
only once when a selector is repeated, and preserve Linear's exact workflow
state display name. An unknown or ambiguous identifier SHALL produce an
explicit error and no partial output.

The terminal view SHALL show each issue identifier, exact state name, and
title. JSON SHALL return the public `Issue` records without deriving state
from ticket prose.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-015-AC-1 | A batch of identifiers reads one workspace snapshot and returns exact issues in request order with duplicates removed. | Test |
| FR-015-AC-2 | An unknown or ambiguous issue selector fails without partial output. | Test |
| FR-015-AC-3 | Text shows exact state names and JSON retains the structured issue records. | Inspection |
