---
id: SR-6022
title: Explicit Linear CLI credential recovery code review
type: SpecReview
analysis: base
scope: ix-board tooling recovery changes
review_set: base
---

## Summary

Self-review of the bounded tooling recovery change using the Rust review checklist where applicable. This is not an independent reviewer approval.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No unresolved implementation finding in the changed scope | Changed files below |

## Review Coverage

Shared resolution for status and live reads, explicit CLI selection without Keychain access, default denial propagation, unknown selectors, secret handling, and diagnostic recovery instructions.

- `README.md`
- `setup.md`
- `spec/functional/FR-015-protected-credential-value.md`
- `src/lib.rs`
- `src/linear.rs`
- `src/main.rs`

## Verification

make test and make lint passed; license and unsafe checks passed. The release build linked against the local ix-cli-kit fix was tested with a live Linear read using IX_BOARD_CREDENTIAL_SOURCE=linear-cli. Native Keychain access remains interaction-required in this agent context.

Runtime verification used Quire 0.36.2 (engine 0.50.2) and ix-flow 0.2.3 where applicable. No human acceptance token was acknowledged.

## Verdict

The local recovery behavior is verified. Release promotion still requires normal owner review and the verification limitations above remain visible.
