---
id: SR-6044
title: "SWM-158 code review of lifecycle collection and verdicts"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; PR 65; Cargo.toml, Cargo.lock, src/agents/, src/main.rs, src/retro/metrics.rs, tests/agents.rs, tests/retro.rs, tests/schema.rs, examples/export_schema.rs, docs/agents-and-drift.md, docs/examples/agents-v2.json, docs/schema/"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board/FR-018", type: references}
---

## Summary

Ticket SWM-158, frozen CODE PR ix-board#65, reviewed SHA d201872100e4fc8b6e8ebdf0bf5098d93cd01e3b. Code-review with its Rust lane found two consequential classification defects and one terminal-generation edge defect.

## Verdict

**FAIL** — fix all three findings in this PR, then return this reviewer for dispositions. No merge approval.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Claude meta messages are treated as assignments and invalidate known terminal metadata. | src/agents/harness.rs:140 |
| FND-002 | high | A returned Codex wait remains pending after subsequent tool execution. | src/agents/harness.rs:368 |
| FND-003 | medium | A late Codex waiting-end event reopens a terminal generation without a new assignment. | src/agents/harness.rs:249 |

## Evidence and required fixes

FND-001: src/agents/harness.rs treats any string user message except two text patterns as an assignment, ignoring isMeta. src/agents/collect.rs then attaches metadata_assignment_order_unresolved whenever assignment_starts > 1. A production-library reproduction with finished=true and one assignment yields direct=Some(0), nested=Some(0), state=Completed, no issues. Adding an isMeta=true injected string message yields direct=None with the terminal state unchanged. A sanitized projection of the originally captured completed direct worker a0b5092961bed3483 confirms a user/isMeta/system-reminder record and finished=true metadata. The same shape is common in the historical root. Distinguish assignment/start evidence from injected meta/system/skill records; retain true ordering uncertainty for real later assignments. Reproduction: /tmp/swm158-code-review/repro.rs and repro.log.

FND-002: call() sets Waiting on wait_agent/wait; result() removes the correlated call but never clears the pending wait; later ordinary function calls also leave Waiting unchanged. A start → wait call → correlated JSON result → functions.exec sequence remains Waiting with no issue. The live parent has wait_agent JSON results (message/timed_out keys), with no collab_waiting_begin/end events, so those synthetic events cannot be required to clear the wait. decide() then prefers the stale wait over Herdr working, producing WAITING with active workers or UNKNOWN without workers. Correlate wait completion and subsequent execution while retaining genuine pending waits. Reproduction: codex-repro.rs and codex-repro.log.

FND-003: start g1 → task_complete g1 → collab_waiting_end g1 changes Completed to Running, clears execution_ended_at, and reports no uncertainty. collab_waiting_begin can likewise change terminal state to active Waiting. Only a confirmed new assignment/start may reopen terminal work. Guard generation/state and retain uncertainty for conflicting late events. Reproduction: codex-repro.rs and codex-repro.log.

## Validation

make fmt-check and make lint passed. cargo test --test agents --test retro --test schema passed 19 + 70 + 2 tests. cargo test --lib agents:: passed 7 tests. These commands use the assigned worktree/target. Logs: fmt.log, lint.log, tests.log and units.log in /tmp/swm158-code-review.

No aggregate make ci, full crate suite, deny or unsafe aggregate lane ran during this focused review; the lead owns the once-only final gate. No CI file changes exist. No applicable AssuranceProfile was found. No implementation changes, commits, pushes, new worktrees or target directories were made. The authorized nullable alias is accepted.

## Limits and collection assessment

The shared 256 MiB read budget explicitly reports exhaustion and keeps prior observations. The supplied live output's 71 collection_byte_limit cases are honest uncertainty; this review found no evidence that a larger cap would be a correct fix. Replaying full historical transcripts before small metadata can exhaust that budget, so retain boundedness and add fair/metadata-conscious collection if practical. That observation alone is not a finding. Parent transcript ordering uncertainty is reported independently.

Protocol, schema/example parity, null equality, independent counts, excluded internal observations, current/recent separation, clocks and retro provenance were examined. Linux smoke is being collected independently by the lead; its result and the final aggregate gate are not claimed here. Minder implementation is in the separately delivered consumer repository.

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

