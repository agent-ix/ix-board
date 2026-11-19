---
id: SR-6037
title: "spec-review/integrity review of SWM-158 lifecycle specification"
type: SpecReview
analysis: integrity
scope: "agent-ix/ix-board; spec/functional/FR-018-agent-activity-verdicts.md; spec/spec.md; SWM-158"
review_set: subset
---

## Summary

Ticket: SWM-158. Reviewed frozen spec-only PR ix-board#64 at 73367fee91adda8ef5108c94bc76af7ecd93169b. The ordered verdict and terminal/resume policies are coherent, but AC-14 leaves a real zero-count interpretation conflict with the topology example and terminal-worker exclusion.

## Verdict

**CONDITIONAL** — Clarify the zero-count criterion before merging.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | AC-14 restricts exact zero to a complete empty graph, although a complete nonempty graph containing only terminal or excluded workers must also yield zero. Clarify that completeness and absence of qualifying active workers determine zero, and add a nonempty all-terminal fixture. | spec/functional/FR-018-agent-activity-verdicts.md:214 |

## Review scope

```yaml
{
  "scope": [
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The `ix-board agents <team>... [--idle-after <duration>]` command SHALL report"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "current execution, unfinished worker assignments, recent ended activity and"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "uncertainty separately so a team leader can decide whether intervention is needed."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Transcript modification time is an age observation, never proof of a live worker."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL emit one row per agent pane in each named herdr workspace"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "across the local session and every enabled machine from `herdr machine list`."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Workspace labels match case-insensitively. An unmatched team is listed as missing."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "An unreadable machine is listed as an error without suppressing healthy rows."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL exit non-zero when any machine cannot be read."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`--idle-after` accepts positive durations in seconds, minutes or hours (`90s`,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`20m`, `2h`) and defaults to `20m`."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL obtain herdr state and session identity from `herdr agent list`"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "and inspect `herdr agent read --lines 30` for explicit approvals, questions and"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "pending worker waits even when herdr reports `working`."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL locate Claude Code records in"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`~/.claude/projects/*/<id>.jsonl`, companion metadata and"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`<id>/subagents/agent-*.jsonl`, and Codex records in"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`~/.codex/sessions/**/rollout-*-<id>.jsonl` with structured parent/thread records."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Discovery SHALL include quiet children without a transcript-mtime cutoff."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL collect supported process evidence for build or gate commands"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "(`cargo`, `rustc`, `make`, `gh`, `flock`, `pytest`, `npm`, `pnpm`,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`locked-build.sh`) whose environment attributes them to the pane's"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`HERDR_PANE_ID`. The command SHALL report process collection as unavailable on"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "unsupported platforms or permission failure, rather than an empty process list."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Git ages describe the last commit and newest uncommitted file in the pane's"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "working directory; they do not prove execution."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The report SHALL carry a schema version and, per row, team, machine, pane,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "harness, herdr state, verdict, excerpts, child processes, git ages and `partial`."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "It SHALL carry nullable `active_direct_workers` and `active_nested_workers`,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "normalized worker observations (including excluded internal observations),"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "separate recent activity, per-source availability with reasons, and host clock,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "collection start/end and evidence provenance."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Each worker observation SHALL carry harness-scoped identity, immediate parent,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "role (`worker`, `internal`, `unknown`), lifecycle state (`running`, `waiting`,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`idle`, `completed`, `stopped`, `unknown`), structured last activity, transition"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "timestamp, observation time and provenance. Unavailable timestamps are null"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "with a reason, never synthesized from a file modification time."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The deprecated `live_subagents` field SHALL equal `active_direct_workers`,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "including null. This specific nullable alias is retained for existing consumers."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Exact zero requires a complete discovered lifecycle graph for the relevant count;"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "missing evidence is null, never fabricated zero. Known observations remain visible"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "when a count is null. Any relevant source unavailability makes the row `partial`;"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "uncertainty in one count need not erase an independently complete count."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL retain documented age fields, including last-turn age and"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "last-subagent-write age, measured against the clock of the host holding the files."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "File-write ages remain identified as such. Recent structured execution activity"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "and its timestamp are separate from these ages."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The terminal table SHALL show the same rows and nullable counts as `--json`,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "with an indented excerpt for `PROMPT`, `ASKING` and `UNKNOWN`, next ready ticket"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "for `IDLE`, and missing-team and machine-error lines. Unknown counts render as"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "unknown, not `0`. JSON schema and examples SHALL describe the version, nullable"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "alias/counts, lifecycle observations, source availability and new verdicts."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The lifecycle reducer SHALL count an identity as active only when it has an"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "unfinished assigned task in `running` or `waiting` state. A direct worker has"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "the pane's lead session as immediate parent; a nested worker has proven ancestry"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "at depth two or greater. The reducer SHALL deduplicate identities and exclude"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "internal/guardian/approval sessions from both counts."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "When completion or user cancellation is confirmed, the reducer SHALL retain"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`completed` or `stopped` until a later confirmed assignment/start for that same"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "identity. Reopening a transcript, resuming inspection or writing a recent file"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "does not restart work. A successful follow-up assigning new work can restart it."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "An assistant `end_turn` alone does not establish worker completion."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The Claude adapter SHALL reconcile `finished`, `stoppedByUser`, `parentAgentId`"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "and `spawnDepth` companion metadata with child and parent lifecycle records."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "When both `stoppedByUser` and `finished` are true, the adapter SHALL classify"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "the terminal state as `stopped`; this combination is not itself a conflict."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Companion metadata supplies no lifecycle timestamp, and its mtime SHALL NOT"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "supply one. A resume alone does not reopen an assignment even if `finished`"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "remains true. `parentUuid` links messages, never worker ancestry."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The Codex adapter SHALL reconcile structured session headers, task/turn starts,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "completions, interruptions, successful follow-ups and parent child snapshots,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "excluding `guardian_review` and other internal sources."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Codex child snapshots SHALL be correlated with the originating `list_agents`"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "call through `call_id`, rather than accepted from arbitrary agent-shaped output."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "An `agent_status` object such as `{completed: \"result text\"}` is normalized by"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "its state discriminator; result text SHALL NOT be retained in observations."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Both adapters SHALL produce the shared observation contract from sanitized real"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "record fixtures. Unknown lifecycle formats are explicitly unsupported."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "An old nonterminal record alone does not prove current lead execution; the"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "adapter SHALL retain source sample time and any uncertainty about current state."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "When identity, ancestry, role or state is unresolved, records conflict without"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "a provable later transition, a discovered child is unreadable, or a relevant"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "enumeration is incomplete, the reducer SHALL preserve uncertainty in affected"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "counts and classifications. A parent reference to an absent child is incomplete"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "evidence. Duplicate identical observations do not create extra workers;"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "contradictory duplicates do not silently choose one."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The classifier SHALL consume normalized observations through a harness adapter"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "interface, so a third adapter can be added without rewriting verdict rules."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The classifier SHALL evaluate the following rules in order, reading all evidence"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "needed to distinguish an explicit wait from an apparent execution signal:"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "1. `PROMPT`: explicit outstanding approval/dialog evidence; include its text."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "2. `ASKING`: an outstanding question or permission request in the latest assistant"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   response, with no later response or execution superseding it; include its text."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "3. `UNKNOWN`: contradictory evidence or missing/unsupported evidence prevents"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   choosing among the remaining verdicts; include the reason and raw herdr fields"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   when its state is unknown. Independent positive evidence may resolve a"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   classification despite an unrelated unavailable source."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "4. `WAITING`: the lead explicitly awaits active workers, or the lead is inactive"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   while at least one direct worker has confirmed unfinished assigned work."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   An explicit pending worker wait takes precedence over herdr `working`."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "5. `WAITING-STUCK`: a confirmed pending worker wait has lasted at least 15 minutes,"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   complete evidence proves no viable active worker or executing work, and no"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   progress has occurred during that interval. Quiet files alone never qualify."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "6. `WORKING`: positive current lead execution is established by herdr, lifecycle"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   or a verified attributed gate process, without an unresolved contradiction or"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   explicit pending worker wait. Recent writes and active workers alone do not"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   qualify the lead as working."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "7. `RECENT`: execution has ended, no active workers remain, and the last structured"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   execution activity is younger than `--idle-after`."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "8. `IDLE`: quiet is established for at least `--idle-after`, with no outstanding"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "   prompt, question, active workers, current execution or unresolved wait."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "9. `UNKNOWN`: no preceding rule can be established from available evidence."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "An `IDLE` row SHALL include the team's next ready ticket from the existing ready"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "queue when readable. A queue failure is reported once and does not change verdicts."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The command SHALL use the same read-only Rust probe locally and remotely through"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "a hidden ix-board command with typed JSON request/response and explicit protocol"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "version. Remote collection requires a compatible installed binary; absent or"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "incompatible binaries produce unavailable evidence, never the old shell/mtime"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "heuristic. One batched SSH probe per enabled remote machine is combined with"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`herdr --machine <label>` reads behind injectable machine-read traits."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The collector SHALL enforce a 60-second per-machine timeout and finite limits"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "on file enumeration, bytes read, record size, retained state and response size."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Limits SHALL be documented constants or configuration and enforced before an"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "unbounded allocation/read. On reaching a limit, affected evidence is unavailable"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "or uncertain with a reason; healthy rows remain present. Stored identity metadata"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "does not retain full transcript headers, prompts, credentials or instructions."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "When a file changes during collection, the collector SHALL retry it once and"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "report uncertainty if it changes again. Host clocks and start/end timestamps"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "SHALL accompany evidence so remote clock skew cannot fabricate activity ages"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "or a consistent snapshot. Missing permissions and unsupported platform sources"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "remain explicit availability results."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "The working-share metric in [FR-024](./FR-024-retro-snapshot.md) SHALL count only"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "`WORKING` rows, excluding `WAITING` and `RECENT`. When a relevant classification"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "or population is unresolved, it SHALL be unavailable under"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "[FR-026](./FR-026-retro-unavailable-not-zero.md). Query provenance SHALL identify"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "this lifecycle definition so [FR-027](./FR-027-retro-delta.md) reports a definition"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "change rather than a numeric old/new comparison."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "Minder consumers SHALL treat `WAITING` and `RECENT` as observation-only, with no"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "nudge triggered by either verdict. Consumers SHALL accept nullable counts and"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "the deprecated alias while preserving command exit status, errors and coverage"
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "instead of treating a partial or failed command as a complete zero-worker report."
    },
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
      "excerpt": "Ambiguous ancestry, conflicting duplicate identities, unknown role/state, missing discovered children and unsupported lifecycle formats yield explicit uncertainty/null in affected counts; a complete empty graph alone yields exact zero."
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
      "role": "examined",
      "excerpt": "Minder fixtures observe WAITING/RECENT without nudging, accept null corrected counts/alias, and preserve failed/partial command status, errors and coverage."
    },
    {
      "id": "FR-018-AC-26",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "macOS and Linux smoke runs demonstrate the shared Rust probe and supported lifecycle collection; inaccessible remote/platform sources are recorded as unavailable with reasons rather than claimed successful."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "- **Intent**: [US-001](../usecase/US-001-team-leader-reads.md), team-leader inspection of ready work and project context. SWM-158 corrects lifecycle evidence before it drives intervention."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "- **Consumers**: [FR-024](./FR-024-retro-snapshot.md), [FR-026](./FR-026-retro-unavailable-not-zero.md), and [FR-027](./FR-027-retro-delta.md); companion dev-team minder rules consume the versioned agents output."
    },
    {
      "id": "FR-018",
      "path": "spec/functional/FR-018-agent-activity-verdicts.md",
      "role": "examined",
      "excerpt": "- **Verification**: the computed Quire matrix derives criterion coverage from test trace tags. Existing tests in `tests/agents.rs` must be revised for corrected AC-2/AC-3 behavior; lifecycle/protocol/consumer tests must bind the discriminating criteria above. This spec change does not claim those implementation tests pass."
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "- `agents` reports lifecycle activity verdicts (working, waiting, waiting-stuck,"
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "  recent, prompt, asking, idle, unknown), nullable direct/nested active-worker"
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "  counts and source uncertainty for named herdr workspaces on every machine"
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "  ([FR-018](./functional/FR-018-agent-activity-verdicts.md), SWM-158)."
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "- Compatibility layers, legacy readers, or migration shims, except the explicitly"
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "  retained deprecated nullable `live_subagents` alias in"
    },
    {
      "id": "spec/spec.md",
      "path": "spec/spec.md",
      "role": "examined",
      "excerpt": "  [FR-018](./functional/FR-018-agent-activity-verdicts.md)."
    }
  ]
}
```

## Evidence and limitations

No applicable AssuranceProfile exists under spec/. Direct subset review authorized by the dispatch. No enforced ix-flow run or human acceptance is claimed. Changed documents pass quire validation; full-repository validation has existing structural failures in FR-004, FR-016, FR-017 and roadmap/ears-campaign-stories.md. The computed matrix does not bind the existing Rust /// Trace comments under the installed grammar; draft Rust files in the shared worktree can appear in its diagnostics, so this is context only, not a frozen-code coverage verdict. No new lifecycle tests are implemented by this spec PR. No aggregate gate was run. Installed tools: quoin 0.36.7, quire 0.52.0 (engine 0.62.2), spec-artifacts-process@v0.29.2.

Rust-review, React review and gap-analysis are skipped because no production code changes are committed. Criterion-strength is not selected; no calibrated Jev claim is made. Object review is not selected: no standalone domain-object artifact is added or edited; the existing command's lifecycle observation/output is assessed through integrity and boundary lenses. The approved nullable alias, observation provenance and WAITING/RECENT choices are binding user decisions.
