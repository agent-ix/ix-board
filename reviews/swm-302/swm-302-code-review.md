---
id: SR-6140
title: "SWM-302 retro metric classes code and Rust review"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@1eecb5141742e61ee43dc8ab28d66d01bf79a96d; src/retro/{catalog,delta,metrics}.rs, src/retro/department/operations_tests.rs, src/team_metrics/tests.rs, tests/retro.rs, spec/functional/FR-027-retro-delta.md"
review_set: subset
---

# SR-6140: SWM-302 retro metric classes code and Rust review

## Summary

Ticket: SWM-302. PR agent-ix/ix-board#100, base 8bf053d, head 1eecb5141742e61ee43dc8ab28d66d01bf79a96d.
Code review with the rust-review lane folded in, over the diff only. Class values are correct for all
34 catalog metrics; the defects are compatibility of the changed wire shapes and one hollow assertion.

## Verdict

**CONDITIONAL**: three medium findings.

Examined and clean: delta output on a copy of the live QSL store (legacy records) gives `class` for all
34 metrics, identical to the FR-024 table and to `CLASS_OF` in factory-panel `hooks/team.tsx` on
origin/main. `department_class` appears on exactly the 8 ids shared with the FR-029 catalog and matches
it. `Out::put` is the one non-test constructor of `Metric`, so snapshot and backfill records carry the
fields. Version-one stores without the fields decode. The live store checksum was unchanged.

Gates at the head: `cargo fmt --check` passed (stable toolchain warns on two nightly-only options);
`cargo clippy --locked --all-targets --all-features -- -D warnings` passed. Test, deny and unsafe-audit
results are given in the reviewer report. No CI workflow file is in the diff.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | Records now carry `class` while `Metric` keeps `deny_unknown_fields` and `schema_version` stays 1 or 2, so one new snapshot makes the whole append-only store unreadable to every older reader. Reproduced with installed 0.10.0 and 0.7.2: `retro delta` exits 3 with `unknown field class`; factory-panel maps exit 3 to `no retro baseline` and shows no warning. | src/retro/catalog.rs:113-118 |
| FND-002 | medium | `MetricDelta.class` is a required field with no serde default, so a delta JSON saved by the previous release no longer decodes through `read_saved_report`. Reproduced: `invalid saved report: missing field class`. | src/retro/delta.rs:58-59 |
| FND-003 | medium | The AC-9 test never reaches the catalog fallback in delta: the latest record is built by `snap`, which already fills both fields, and the legacy record is only the previous one. By code trace, removing both `or_else` fallbacks leaves every assertion true. | tests/retro.rs:2084-2096 |

## New findings (disposition pass 1)

Head a08cbe8d22a89747a362e1181f0be032d31f81f3; fix diff only.

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-004 | low | The fix bumps the two root plugin manifests from 0.5.0 to 0.12.0 with no plugin content change, leaving the nested ones at 0.3.0; the plugin has its own release line (tag `ix-board--v0.5.0`, pinned by the marketplace) that earlier release commits moved in all four files. | .claude-plugin/plugin.json:3 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | `Metric`, the store and the record writers are identical to base 5075d92; classes are resolved only in delta output. |
| FND-002 | fixed | Both `MetricDelta` fields are optional with serde default; the saved 0.10.0-shape delta decodes. |
| FND-003 | fixed | Stored metrics have no class field, so the test's class assertions can only be met by catalog resolution. |
| FND-004 | fixed | Root plugin manifests are back to 0.5.0, identical to base; nested ones unchanged at 0.3.0. |

## New findings (disposition pass 2)

Head f2e972f4099055b1946355247f0d2dc3aa0e15f8; this round's diff only.

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-005 | high | `retro snapshot --json` output can no longer be read by `read_saved_report`: the added metric keys hit `Metric`'s `deny_unknown_fields`. Reproduced offline: base 0.10.0 output decodes, head output fails with `unknown field class`. relay-tui documents this file as a `--report` input. | src/retro/catalog.rs:177-184 |
| FND-006 | low | Public `SnapshotOutput::from_record` panics through `expect` on a metric id outside the compiled catalog, which a decoded record may carry. Reproduced through the library; the CLI caller cannot reach it. | src/retro/catalog.rs:193-195 |
| FND-007 | low | `SnapshotOutput` restates the `Record` envelope field by field without destructuring, so a field added to `Record` compiles and is silently missing from `--json`. | src/retro/catalog.rs:209-218 |
