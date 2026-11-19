---
id: SR-6045
title: "SWM-158 repository gap analysis"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; repository spec/, src/, tests/; changed lifecycle/probe/schema/retro implementation; PR 65"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-018", type: references}
---

## Summary

Planless repository matrix and surface audit at d201872100e4fc8b6e8ebdf0bf5098d93cd01e3b. New compiled trace attributes bind FR-018-AC-1 through AC-24, but the eight-verdict tests bypass adapter paths and existing repository trace debt prevents a repository assurance PASS.

## Verdict

**FAIL** for repository-wide traceability. The 183 unrelated untagged criteria are existing debt, not introduced by PR 65, and should not expand this coder's scope. The two new fixture gaps require fixes in SWM-158.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Repository matrix retains 183 untagged criteria outside FR-018; repository-wide assurance cannot pass. | spec/functional/FR-001-ready-queue.md:52 |
| FND-002 | medium | Claude eight-verdict coverage bypasses record decoding for working and pending-wait cases. | tests/agents.rs:128 |
| FND-003 | medium | Codex eight-verdict coverage bypasses record decoding for working and pending-wait cases. | tests/agents.rs:128 |

## Evidence

FND-001: quoin matrix --repo . --json reports 217 criteria: 24 tagged, 185 untagged, 8 method-without-symbol. Removing FR-018's two cross-repository/manual criteria leaves 183 unrelated untagged criteria. For example FR-001-AC-1 has no binder. The changed files do not modify these requirement/test surfaces. Coverage diagnostics also include existing source symbol collisions and 256 trace-id-outside-marker records. No optional architecture/security absence was treated as a finding.

FND-002 and FND-003: tests/agents.rs::all_eight_verdicts_thresholds_and_pending_wait_precedence decodes only done/asking fixtures. It creates a Lifecycle struct for running, mutates state to Waiting, and creates workers directly for other verdicts. This verifies shared classifier branches, but cannot detect the actual Claude meta-message or Codex returned-wait bugs in SR-6044. Add sanitized harness-specific record sequences through production adapters/collector for the eight verdicts and their precedence. These two bindings are partial, not wrong-AC tags.

## Coverage

Reconciliation: quoin matrix 0.36.7, quire 0.52.0 (engine 0.62.2), spec-artifacts-process@v0.29.2. Full matrix is matrix.json; coverage.json holds diagnostics, including previously known cache/platform symbol collisions. All criteria carry no run evidence in the Quoin store; focused Cargo execution is reported separately, without claiming imported run bindings.

Plan completion: not assessed

FR-018: 24 tagged Test criteria; AC-25 is untagged locally because minder consumers belong to the companion dev-team delivery; AC-26 is a Demonstration with macOS evidence supplied and native Linux smoke pending the lead's report. Neither missing local binder is presented as newly missing ix-board code. All 24 tagged criteria's real Cargo trace attributes compiled during the focused tests. binders are extracted from the computed matrix, not a handwritten matrix.

Repository surface inventory covered CLI families auth, next, project-chunk, search, issues, agents, drift, retro, focus, project, scope, roadmap, reconcile, chain, orphans and snapshot, with their corresponding source modules and governing FR-001 through FR-028/master scope. The added hidden probe is owned by FR-018. No newly unspecified behavior, live source stub, TODO/unimplemented placeholder, or replacement of the implementation under test was found. This inventory is not a fresh semantic acceptance test of unchanged modules.

Focused execution: 19 agents, 70 retro, 2 schema and 7 agent unit tests passed; make fmt-check/make lint passed. No aggregate gate ran. New canonical ix-trace-rs v0.1.1 attributes compile and computed matrix bindings exist, including the all-eight-verdicts test to AC-2/3 and lifecycle_working_share_excludes_waiting_recent_and_refuses_unresolved_population to AC-24.

Optional gap-analysis semantic fan-out was not invoked; ordinary code review still checked concrete behavior and test assertions. No formal plan artifact was supplied for plan-completeness assessment. No applicable AssuranceProfile found.

## Examined scope

```json
{
  "scope": [
    {
      "id": "FR-018-AC-1",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "One invocation returns one row per agent pane across local/enabled machines, matching named workspaces case-insensitively; missing teams and unreadable machines are explicit and healthy rows survive."
    },
    {
      "id": "FR-018-AC-2",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Claude real-record fixtures exercise all eight verdicts and precedence; ended execution younger than idle-after is RECENT, established quiet exactly at the threshold is IDLE, and a pending worker wait overrides herdr working."
    },
    {
      "id": "FR-018-AC-3",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Codex real-record fixtures exercise the same verdicts, threshold and precedence, deriving direct and nested parentage from structured records rather than rollout count."
    },
    {
      "id": "FR-018-AC-4",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Missing adapters/transcripts retain herdr, tail and available attributed-command evidence, mark partial, expose availability reasons and leave unprovable counts null."
    },
    {
      "id": "FR-018-AC-5",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Injectable machine-read fixtures prove local and remote host-clock ages and identical typed protocol results without network access."
    },
    {
      "id": "FR-018-AC-6",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "IDLE rows include the next ready ticket when readable; an unreadable queue is reported once without changing verdicts."
    },
    {
      "id": "FR-018-AC-7",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Text and versioned JSON report the same rows/counts, including null; idle-after accepts positive s/m/h values and rejects unsupported, zero or negative values."
    },
    {
      "id": "FR-018-AC-8",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The captured eight-file topology (two active direct, two completed direct, two completed nested fork, two stopped direct) yields active_direct_workers=2, active_nested_workers=0 and live_subagents=2."
    },
    {
      "id": "FR-018-AC-9",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Completion and user cancellation immediately remove a worker from active counts despite a fresh transcript mtime; an assigned running/waiting worker remains active after more than ten minutes without writes."
    },
    {
      "id": "FR-018-AC-10",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "A completed/stopped worker stays terminal after reopen or incidental writes, restarts after a confirmed later assignment/start, and an end_turn alone does not complete an unfinished assignment."
    },
    {
      "id": "FR-018-AC-11",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Claude fixtures reconcile finished/stoppedByUser/parentAgentId/spawnDepth: both terminal flags mean stopped, resume alone does not reopen finished work, parentUuid never supplies ancestry, missing transition timestamps stay null, and unorderable conflicting evidence yields uncertainty."
    },
    {
      "id": "FR-018-AC-12",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Codex fixtures distinguish task/turn completion/interruption, successful follow-ups and reopening; list_agents snapshots require matching call_id, string/object statuses normalize without result text, and guardian_review/internal records remain visible but excluded."
    },
    {
      "id": "FR-018-AC-13",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "One active direct worker and one active depth-two worker produce separate counts of one each; internal/approval sessions add zero, and duplicate identical identities never increase either count."
    },
    {
      "id": "FR-018-AC-14",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Ambiguous ancestry, conflicting duplicate identities, unknown role/state, missing discovered children and unsupported lifecycle formats yield explicit uncertainty/null in affected counts; complete evidence with no qualifying active workers yields exact zero, tested with empty, nonempty all-terminal (completed/stopped), and excluded-only graphs."
    },
    {
      "id": "FR-018-AC-15",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Normalized output includes identity, immediate parent, role/state, structured activity, transition/observation timestamps and provenance, including excluded records and unavailable timestamp reasons."
    },
    {
      "id": "FR-018-AC-16",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "A fake third harness adapter produces lifecycle observations and correct counts/verdicts using the unchanged shared classifier."
    },
    {
      "id": "FR-018-AC-17",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "WAITING is emitted for a lead awaiting active workers and for an inactive lead with active direct workers; neither state becomes WORKING merely because worker files are recent."
    },
    {
      "id": "FR-018-AC-18",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "WAITING-STUCK requires a confirmed pending wait, complete evidence of no viable worker/executing work and 15 minutes without progress; 14m59s, quiet active workers and incomplete evidence cannot satisfy it."
    },
    {
      "id": "FR-018-AC-19",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Recent transcript/git writes or old nonterminal records without positive current execution never yield WORKING; source sample time/current-state uncertainty is preserved, contradictory execution/terminal evidence yields UNKNOWN, and explicit approval/question fixtures retain PROMPT/ASKING precedence."
    },
    {
      "id": "FR-018-AC-20",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Quiet discovered children are enumerated without an mtime cutoff; a changing file is retried once, repeated change yields uncertainty, and enumeration/read/state/output limits produce bounded responses with explicit reasons."
    },
    {
      "id": "FR-018-AC-21",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Missing/incompatible remote probe versions, malformed/truncated protocol responses, permission failures, unavailable platform sources and a 60-second timeout retain independent healthy rows and explicit error/coverage results without heuristic fallback."
    },
    {
      "id": "FR-018-AC-22",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Skewed host clocks and collection start/end fixtures keep documented ages in the owning host's clock domain and expose inconsistent/unknown timing instead of fabricating current execution."
    },
    {
      "id": "FR-018-AC-23",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Schema/examples expose schema/protocol versions, nullable counts, deprecated live_subagents equality including null, normalized workers, recent activity and per-source availability."
    },
    {
      "id": "FR-018-AC-24",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Retro working-share counts only WORKING, becomes unavailable for unresolved relevant rows/population, and changes query provenance so comparison with the previous definition reports definition change rather than a numeric delta."
    },
    {
      "id": "FR-018-AC-25",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "context_only",
      "excerpt": "Minder fixtures observe WAITING/RECENT without nudging, accept null corrected counts/alias, and preserve failed/partial command status, errors and coverage."
    },
    {
      "id": "FR-018-AC-26",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "context_only",
      "excerpt": "macOS and Linux smoke runs demonstrate the shared Rust probe and supported lifecycle collection; inaccessible remote/platform sources are recorded as unavailable with reasons rather than claimed successful."
    }
  ]
}
```

