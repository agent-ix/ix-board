---
id: SR-023
title: Shared protected credential value Rust diff review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@cabf2c8901dfb6aa0a8aee368b20326478d23ab2; Cargo.toml, Cargo.lock, src/linear.rs, FR-015, SR-022; workflow diff empty"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-015"
    type: reviews
---

# SR-023: Shared protected credential value Rust diff review

## Summary

Review the private credential field replacement and injected HTTP acceptance tests.

## Verdict

Pass. The API key is retained in the authoritative shared redacted,
non-serializable SecretValue. The constructor's blank-input check and cache
namespace calculation are unchanged. HTTP receives the unchanged secret only
at its authentication boundary. Environment lookup and credential identities
are unchanged; no OS backend is constructed and no credential read/write occurs.
The added keyring graph is the kit's opt-in secrets feature, not copied code.
The local loopback server verifies the actual Authorization header. Tests use
fictional strings; no private data enters review artifacts or stdout.
No workflows or runtime resource limits changed. New tests carry FR-015 tags.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder). | Reviewed diff at cabf2c8 |

## Gates

Full local `make ci` exited 0: fmt-check, all-target clippy -D warnings,
unit/integration/doc tests, license check and unsafe audit. All credential tests
ran through the injected loopback seam. Existing unmatched-license warnings
remain warnings; no policy was weakened.
