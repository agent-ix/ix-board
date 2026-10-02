---
id: FR-015
title: Protect Linear credential values through the shared CLI foundation
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/FR-001"
    type: constrains
provenance:
  source: SWM-41
---

# FR-015: Protect Linear credential values

## Description

The Linear source SHALL retain its authentication value in the shared
`ix-cli-kit::secrets::SecretValue` type, whose Debug output is redacted and
which has no serialization or Display implementation.

## Behavior

- The source SHALL continue accepting an explicit constructor value and reading
  `LINEAR_API_KEY` through its existing environment constructor.
- When the supplied value is blank, the source SHALL return `MissingApiKey`.
- The source SHALL expose authentication bytes only to the downstream HTTP
  authorization operation and its existing cache namespace calculation.
- The source SHALL preserve the existing authentication bytes and cache namespace.
- The source SHALL NOT introduce an OS-store read, credential write, file fallback,
  or storage-identity transition as part of this adoption.

## Acceptance Criteria

| ID | Criterion | Verification |
| --- | --- | --- |
| FR-015-AC-1 | A synthetic credential is retained as SecretValue; its Debug representation is redacted and does not contain the fixture text. | Test |
| FR-015-AC-2 | The existing injected HTTP request receives the unchanged fixture Authorization header and the existing cache namespace remains stable. | Test |
| FR-015-AC-3 | Empty and whitespace-only constructor inputs still return MissingApiKey. | Test |
