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
- The explicit `IX_BOARD_CREDENTIAL_SOURCE=linear-cli` selection SHALL skip the
  OS store and environment key and use the configured Linear CLI integration.
- The default `auto` selection SHALL preserve environment, store, CLI precedence
  and SHALL propagate store access failures rather than silently falling back.
- Unknown source values SHALL fail before any credential source is accessed.
- Credential status and live reads SHALL share the same source resolution.

## Acceptance Criteria

| ID | Criteria | Verification |
| --- | --- | --- |
| FR-015-AC-1 | A synthetic credential is retained as SecretValue; its Debug representation is redacted and does not contain the fixture text. | Test |
| FR-015-AC-2 | The existing injected HTTP request receives the unchanged fixture Authorization header and the existing cache namespace remains stable. | Test |
| FR-015-AC-3 | Empty and whitespace-only constructor inputs still return MissingApiKey. | Test |
| FR-015-AC-4 | Explicit linear-cli selection resolves the configured CLI credential even when the store would deny access, without consulting that store. | Test |
| FR-015-AC-5 | Automatic selection preserves store errors; unsupported selections read no credential source. | Test |

## Dependencies

- The authoritative ix-cli-kit secrets value type.
- The existing Linear transport and injected HTTP test seam.
