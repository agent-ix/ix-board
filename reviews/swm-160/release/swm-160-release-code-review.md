---
id: SR-6062
title: "Code review — ix-board 0.6.0 release metadata"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; SWM-160 release; Cargo.toml, Cargo.lock and static version consumers"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
---
## Summary

Independent metadata-only review of PR72 at de1ee66ba485ffefc1c8d5969e4245fd9476b68c, based on accepted functional source8381a6014e70e833756d04d8d495f5cdda8430bc. The only changes are the matching root package version replacements from0.5.1 to0.6.0 in Cargo.toml and Cargo.lock.

## Verdict

**PASS** — no in-scope findings. The manifest and root lock package agree, and dependencies, functional source, schemas, tests and CI are unchanged. Version0.6.0 represents the accepted department feature release; artifact verification and installation remain the release owner's later phase.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Coverage

Methods: code-review with rust-review/rust-style lane. Repository AGENTS/CLAUDE, Cargo, clippy, deny and formatter settings were read. There is no applicable AssuranceProfile or specification/plan diff; spec-review sub-analyses are inapplicable. Gap-analysis is separately recorded in SR-6063.

The frozen diff contains exactly two version-line replacements, git diff --check exits0, and source/test/spec/example/workflow diffs from the accepted base are empty. No dependency resolution, public API, metric, credential, wire format, safety, concurrency, vendoring, test oracle or required gate is changed. Root package name, edition, MSRV and license remain intact. Static inspection confirms clap derives the CLI version from package metadata; searches of src/tests found no hardcoded0.5.1 assertion requiring correction. US-003 delivery notes require a runnable --version command; its actual artifact smoke remains pending, not claimed by static inspection.

Explicit heavy admission remains held for dev-team's TUI artifact phase. No compiler, build, lint, tests, cargo network or aggregate gate ran in this review. The unchanged functional source's retained gate reports322passing tests and one ignored benchmark; that gate is context, not a review rerun. Artifact version/ABI/protocol checks must follow admission. No real credential store, ix-board credential-backed command, token access, ACL/signing change, source edit, extra worktree, push, release or install occurred.

Exact reviewer model ID is unavailable; invocation3BABAA20-C6B7-4C94-865E-6036E70BD877. TicketSWM160 was explicitly supplied by the lead and validated against its real Linear record; the PR body already references it. The reviewer does not change parent or SWM167 completion state.

## Examined units

- package-version (examined), Cargo.toml:3: version = "0.6.0"
- locked-package-version (examined), Cargo.lock:1083: version = "0.6.0"
- cli-version-consumer (examined), src/main.rs:38: version,
- delivery-version-smoke (context_only), spec/usecase/US-003-plan-from-inline-focus.md:49: `ix-board --version` exits successfully.
