---
id: SR-6047
title: "SWM-158 remote probe launch gap analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; repository matrix; deployment delta in PR 67; src/agents/probe.rs, tests/agent_probe_transport.rs, tests/support/probe_transport.rs; FR-018"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-018", type: references}
---

## Summary

Ticket: SWM-158. Planless matrix reconciliation at frozen e003d3733d07345ef4570023b75e234531a8b260 preserves the repository's known assurance failure. The launch correction adds no new blocking gap, unowned behavior or hollow test.

## Verdict

**FAIL for repository-wide assurance**: the unchanged 183 unrelated untagged criteria remain. This baseline was already accepted-no-change under owner decision 6b20e08b in SR-6045; it is not a new blocker for PR 67. The launch delta is clean and mergeable after the lead's final aggregate gate. No repository PASS is claimed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Repository matrix retains 183 untagged criteria outside FR-018; repository-wide assurance cannot pass. | spec/functional/FR-001-ready-queue.md:52 |

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | accepted-no-change | Existing unchanged baseline retained from SR-6045 and owner decision 6b20e08b; explicitly out of scope for this narrow deployment correction. |

## Coverage

Reconciliation: quoin matrix 0.36.7, quire 0.52.0 (engine 0.62.2), spec-artifacts-process@v0.29.2. Independently computed quoin-matrix.json and matrix.json record 217 criteria: 24 tagged, 185 untagged, 8 method-without-symbol; zero ignored-only. The 185 are the 183 unrelated baseline criteria plus FR-018-AC-25 (companion minder consumer) and AC-26 (manual/native demonstration). These two existing cross-delivery criteria are not represented as new missing implementation. Every criterion has no run evidence in the Quoin store; the 24 focused passing Cargo tests are reported independently.

Plan completion: not assessed

Coverage diagnostics remain in coverage.json: zero untracked symbols, 256 trace-id-outside-marker diagnostics, one NFR heading mismatch, two absent optional archetypes, one uncatalogued NFR verification method and one catch-all property diagnostic. Existing platform cache symbol collisions are unchanged. The added Rust trace attribute binds the production transport regression to AC-5 and AC-21. No stale tag or extraction failure affects the new test.

Reverse-gap delta inventory: one changed remote-launch behavior, owned by FR-018's existing shared Rust collection protocol and compatible-binary requirement. New CLI commands/flags, endpoints, configuration knobs and public APIs: zero. Untraced introduced behaviors, source stubs, test stubs and coverage inflation: zero. Unchanged repository family inventory (auth, next, project-chunk, search, issues, agents, drift, retro, focus, project, scope, roadmap, reconcile, chain, orphans and snapshot) retains the FR-001 through FR-028/master-scope ownership documented in SR-6045; this focused follow-up does not claim a fresh semantic audit of every unchanged module.

The fixture compiles a Rust transport executable, runs the production SystemHost path in an isolated subprocess, compares exact typed stdin, and checks selected binary identity and protocol refusal. It therefore has a behavioral failure oracle. Independent 24-test execution passed; the inspected original-command negative control failed with exit 127. No production code was replaced by its own double. Optional semantic fan-out was skipped. No applicable AssuranceProfile found. Live native rollout and final aggregate gate belong to the lead.

## Examined scope

The exact examined units and excerpts are in adjacent scope.json and embedded in the marked Linear comment. FR-018-AC-5/21 were examined for transport behavior, AC-26 is context_only, and FR-001-AC-1 exemplifies the unchanged matrix debt. The full matrix is retained as tool output rather than rewritten as a hand-authored test matrix.

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
