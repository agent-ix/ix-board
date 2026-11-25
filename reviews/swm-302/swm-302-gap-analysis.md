---
id: SR-6141
title: "SWM-302 retro metric classes gap analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@1eecb5141742e61ee43dc8ab28d66d01bf79a96d; spec/functional/FR-027-retro-delta.md, spec/functional/FR-024-retro-snapshot.md, tests/retro.rs, src/retro/{catalog,delta,metrics}.rs"
review_set: subset
---

# SR-6141: SWM-302 retro metric classes gap analysis

## Summary

Ticket: SWM-302. PR agent-ix/ix-board#100 at head 1eecb5141742e61ee43dc8ab28d66d01bf79a96d. Matrix
verification and reverse-gap check limited to the diff. The behaviour has an owning criterion
(FR-027-AC-9) but the computed matrix binds no test to it.

## Verdict

**FAIL**: FR-027-AC-9, added by this change, is untagged in the computed matrix.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-027-AC-9 is `untagged` in `quire matrix`: the new test uses a `/// Trace:` comment, which `quire coverage` reports never binds. The file already uses `#[trace(...)]` from `ix_trace_rs`. The ticket requires tests that trace the new ACs. | tests/retro.rs:2054 |
| FND-002 | low | The same tag names FR-024-AC-19, which does not exist; FR-024 criteria end at AC-18 and the diff does not change FR-024. | tests/retro.rs:2054 |

## Coverage

Plan completion: not assessed.

Engine: quire 0.52.1 (engine 0.62.2), quoin 0.36.10. FR-027-AC-1 to AC-8 are also untagged for the same
comment form; they predate this diff and are outside its scope. No code without an owning requirement
was found in the diff. Semantic review was not run as a separate pass.

## New findings (disposition pass 1)

Head a08cbe8d22a89747a362e1181f0be032d31f81f3; fix diff only.

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-003 | medium | Ticket acceptance criterion `retro snapshot records carry the same fields` is no longer met: the fix drops them from records and from `retro snapshot --json`, the ticket is unchanged with no recorded decision, and the PR body still says the classes are persisted. | src/retro/catalog.rs:110 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The test carries `#[trace(...)]`; `quire matrix` reports FR-027-AC-9 and AC-10 tagged. |
| FND-002 | fixed | The FR-024-AC-19 tag is removed. |
| FND-003 | fixed | `retro snapshot --json` carries both fields on output, stored lines stay classless and older records decode; the PR body and FR-024-AC-14 say so. |

## New findings (disposition pass 2)

Head f2e972f4099055b1946355247f0d2dc3aa0e15f8; this round's diff only.

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-004 | low | FR-024-AC-14 is bound only by a test that checks two class values; nothing asserts `class` on every metric or that `department_class` is omitted where undefined, and the older field test serializes `Record`, which `--json` no longer emits. | tests/retro.rs:2061-2072 |
