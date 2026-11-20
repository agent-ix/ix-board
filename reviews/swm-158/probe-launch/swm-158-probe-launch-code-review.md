---
id: SR-6046
title: "SWM-158 remote probe launch code review"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; PR 67; Cargo.toml, Cargo.lock, docs/agents-and-drift.md, src/agents/probe.rs, tests/agent_probe_transport.rs, tests/support/probe_transport.rs; FR-018-AC-5/21/26"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-018", type: references}
---

## Summary

Ticket: SWM-158. Reviewed frozen e003d3733d07345ef4570023b75e234531a8b260 against merged 4cb243b. The code-review method includes its rust-review lane. No introduced defects found in this narrow deployment correction.

## Verdict

**PASS** for the reviewed diff. The fixed POSIX exec invocation appends ordinary user installation prefixes after the existing PATH, preserves typed request stdin and the single SSH command argument, and retains explicit protocol-v2 refusal. CLI manifest/lock agree on 0.5.1. No lifecycle or counter behavior changed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Evidence

The new transport test invokes the production SystemHost::probe implementation, not a mirrored command string. An isolated Rust SSH double executes the actual command received through /bin/sh with a restricted noninteractive PATH. Cargo-only discovery returns 33; local-before-cargo returns 22; an existing PATH binary wins with 11; protocol 1 is explicitly refused. HOME and request paths contain spaces, quotes and command-substitution syntax; exact stdin equality and absence of an injected file verify literal handling. SSH options, target and remote-command argument count are asserted at the real transport seam. The same regression fails on the original command with SSH exit 127, as recorded in the coder's durable probe-launch-red.log; the reviewer inspected that negative-control evidence without mutating the frozen source.

Independent checks: make fmt-check, make lint, standalone rustfmt --check for the support double, and cargo test --test agent_probe_transport --test agents all passed (24 tests). Logs: fmt-check.log, lint.log, tests.log in this artifact's directory. Existing harmless stable-rustfmt warnings about nightly import settings remain. No unsafe code, public API additions, duplicated production logic, stubs, dependency churn or CI weakening introduced.

No applicable AssuranceProfile exists under spec/. Native Linux rollout/smoke remains the lead's responsibility; isolated shell behavior is not claimed to be a live deployment. No aggregate make ci, credential-backed ix-board command, token access, remote operation, install, branch edit, push, merge or new worktree occurred.

## Coverage

All six changed files were inspected. FR-018-AC-5 and AC-21 were examined for the transport portion of their obligations; AC-26 was context_only because live macOS/Linux rollout is outside this review. Full lifecycle review was completed in PR 65 and was not repeated. No spec/plan files changed; spec-review and its sub-analyses are inapplicable. Optional gap-analysis semantic fan-out was not invoked.

## Examined scope

The adjacent scope.json records the exact examined units and excerpts; the identical data is embedded in the marked Linear comment. Reviewed source paths and lines are listed in frontmatter and evidence above. The new test's FR-018-AC-5 and AC-21 bindings are correct partial contributions to those broader criteria, not claims that it alone covers every clause.

```json
{
  "scope": [
    {
      "id": "FR-001-AC-1",
      "path": "spec/functional/FR-001-ready-queue.md",
      "role": "examined",
      "excerpt": "Given fixture projects in `Started` and `Paused`, `next` includes eligible tickets only from `Started` projects."
    },
    {
      "id": "FR-018-AC-5",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Injectable machine-read fixtures prove local and remote host-clock ages and identical typed protocol results without network access."
    },
    {
      "id": "FR-018-AC-21",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Missing/incompatible remote probe versions, malformed/truncated protocol responses, permission failures, unavailable platform sources and a 60-second timeout retain independent healthy rows and explicit error/coverage results without heuristic fallback."
    },
    {
      "id": "FR-018-AC-26",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "context_only",
      "excerpt": "macOS and Linux smoke runs demonstrate the shared Rust probe and supported lifecycle collection; inaccessible remote/platform sources are recorded as unavailable with reasons rather than claimed successful."
    },
    {
      "id": "src/agents/probe.rs:398",
      "path": "src/agents/probe.rs",
      "role": "examined",
      "excerpt": "exec env PATH=\"$PATH:$HOME/.local/bin:$HOME/.cargo/bin\" ix-board __agent-probe"
    },
    {
      "id": "Cargo.toml/Cargo.lock",
      "path": "Cargo.toml",
      "role": "examined",
      "excerpt": "version = \"0.5.1\""
    },
    {
      "id": "docs/agents-and-drift.md:83",
      "path": "docs/agents-and-drift.md",
      "role": "examined",
      "excerpt": "For the remote probe process, launch appends `$HOME/.local/bin` and `$HOME/.cargo/bin` to the existing PATH, preserving its preference order."
    },
    {
      "id": "remote_probe_bootstraps_user_bins_preserves_path_and_refuses_skew",
      "path": "tests/agent_probe_transport.rs",
      "role": "examined",
      "excerpt": "fn remote_probe_bootstraps_user_bins_preserves_path_and_refuses_skew()"
    },
    {
      "id": "probe_transport_main",
      "path": "tests/support/probe_transport.rs",
      "role": "examined",
      "excerpt": "//! Standalone Rust SSH/probe double; no network or credential-store access."
    }
  ]
}
```
