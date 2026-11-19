---
id: FR-018
title: Report per-agent lifecycle activity verdicts for herdr workspaces
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-024"
    type: constrains
  - target: "ix://agent-ix/ix-board/FR-026"
    type: constrains
  - target: "ix://agent-ix/ix-board/FR-027"
    type: constrains
---
# FR-018: Report per-agent lifecycle activity verdicts

## Description

The `ix-board agents <team>... [--idle-after <duration>]` command SHALL report
current execution, unfinished worker assignments, recent ended activity and
uncertainty separately so a team leader can decide whether intervention is needed.
Transcript modification time is an age observation, never proof of a live worker.

The command SHALL emit one row per agent pane in each named herdr workspace
across the local session and every enabled machine from `herdr machine list`.
Workspace labels match case-insensitively. An unmatched team is listed as missing.
An unreadable machine is listed as an error without suppressing healthy rows.
The command SHALL exit non-zero when any machine cannot be read.
`--idle-after` accepts positive durations in seconds, minutes or hours (`90s`,
`20m`, `2h`) and defaults to `20m`.

## Inputs

The command SHALL obtain herdr state and session identity from `herdr agent list`
and inspect `herdr agent read --lines 30` for explicit approvals, questions and
pending worker waits even when herdr reports `working`.

The command SHALL locate Claude Code records in
`~/.claude/projects/*/<id>.jsonl`, companion metadata and
`<id>/subagents/agent-*.jsonl`, and Codex records in
`~/.codex/sessions/**/rollout-*-<id>.jsonl` with structured parent/thread records.
Discovery SHALL include quiet children without a transcript-mtime cutoff.

The command SHALL collect supported process evidence for build or gate commands
(`cargo`, `rustc`, `make`, `gh`, `flock`, `pytest`, `npm`, `pnpm`,
`locked-build.sh`) whose environment attributes them to the pane's
`HERDR_PANE_ID`. The command SHALL report process collection as unavailable on
unsupported platforms or permission failure, rather than an empty process list.
Git ages describe the last commit and newest uncommitted file in the pane's
working directory; they do not prove execution.

## Outputs

The report SHALL carry a schema version and, per row, team, machine, pane,
harness, herdr state, verdict, excerpts, child processes, git ages and `partial`.
It SHALL carry nullable `active_direct_workers` and `active_nested_workers`,
normalized worker observations (including excluded internal observations),
separate recent activity, per-source availability with reasons, and host clock,
collection start/end and evidence provenance.

Each worker observation SHALL carry harness-scoped identity, immediate parent,
role (`worker`, `internal`, `unknown`), lifecycle state (`running`, `waiting`,
`idle`, `completed`, `stopped`, `unknown`), structured last activity, transition
timestamp, observation time and provenance. Unavailable timestamps are null
with a reason, never synthesized from a file modification time.

The deprecated `live_subagents` field SHALL equal `active_direct_workers`,
including null. This specific nullable alias is retained for existing consumers.
Exact zero requires a complete discovered lifecycle graph for the relevant count;
missing evidence is null, never fabricated zero. Known observations remain visible
when a count is null. Any relevant source unavailability makes the row `partial`;
uncertainty in one count need not erase an independently complete count.

The command SHALL retain documented age fields, including last-turn age and
last-subagent-write age, measured against the clock of the host holding the files.
File-write ages remain identified as such. Recent structured execution activity
and its timestamp are separate from these ages.

The terminal table SHALL show the same rows and nullable counts as `--json`,
with an indented excerpt for `PROMPT`, `ASKING` and `UNKNOWN`, next ready ticket
for `IDLE`, and missing-team and machine-error lines. Unknown counts render as
unknown, not `0`. JSON schema and examples SHALL describe the version, nullable
alias/counts, lifecycle observations, source availability and new verdicts.

## Behavior

### Lifecycle and identity

The lifecycle reducer SHALL count an identity as active only when it has an
unfinished assigned task in `running` or `waiting` state. A direct worker has
the pane's lead session as immediate parent; a nested worker has proven ancestry
at depth two or greater. The reducer SHALL deduplicate identities and exclude
internal/guardian/approval sessions from both counts.

When completion or user cancellation is confirmed, the reducer SHALL retain
`completed` or `stopped` until a later confirmed assignment/start for that same
identity. Reopening a transcript, resuming inspection or writing a recent file
does not restart work. A successful follow-up assigning new work can restart it.
An assistant `end_turn` alone does not establish worker completion.

The Claude adapter SHALL reconcile `finished`, `stoppedByUser`, `parentAgentId`
and `spawnDepth` companion metadata with child and parent lifecycle records.
When both `stoppedByUser` and `finished` are true, the adapter SHALL classify
the terminal state as `stopped`; this combination is not itself a conflict.
Companion metadata supplies no lifecycle timestamp, and its mtime SHALL NOT
supply one. A resume alone does not reopen an assignment even if `finished`
remains true. `parentUuid` links messages, never worker ancestry.
The Codex adapter SHALL reconcile structured session headers, task/turn starts,
completions, interruptions, successful follow-ups and parent child snapshots,
excluding `guardian_review` and other internal sources.
Codex child snapshots SHALL be correlated with the originating `list_agents`
call through `call_id`, rather than accepted from arbitrary agent-shaped output.
An `agent_status` object such as `{completed: "result text"}` is normalized by
its state discriminator; result text SHALL NOT be retained in observations.
Both adapters SHALL produce the shared observation contract from sanitized real
record fixtures. Unknown lifecycle formats are explicitly unsupported.
An old nonterminal record alone does not prove current lead execution; the
adapter SHALL retain source sample time and any uncertainty about current state.

When identity, ancestry, role or state is unresolved, records conflict without
a provable later transition, a discovered child is unreadable, or a relevant
enumeration is incomplete, the reducer SHALL preserve uncertainty in affected
counts and classifications. A parent reference to an absent child is incomplete
evidence. Duplicate identical observations do not create extra workers;
contradictory duplicates do not silently choose one.

The classifier SHALL consume normalized observations through a harness adapter
interface, so a third adapter can be added without rewriting verdict rules.

### Verdicts

The classifier SHALL evaluate the following rules in order, reading all evidence
needed to distinguish an explicit wait from an apparent execution signal:

1. `PROMPT`: explicit outstanding approval/dialog evidence; include its text.
2. `ASKING`: an outstanding question or permission request in the latest assistant
   response, with no later response or execution superseding it; include its text.
3. `UNKNOWN`: contradictory evidence or missing/unsupported evidence prevents
   choosing among the remaining verdicts; include the reason and raw herdr fields
   when its state is unknown. Independent positive evidence may resolve a
   classification despite an unrelated unavailable source.
4. `WAITING`: the lead explicitly awaits active workers, or the lead is inactive
   while at least one direct worker has confirmed unfinished assigned work.
   An explicit pending worker wait takes precedence over herdr `working`.
5. `WAITING-STUCK`: a confirmed pending worker wait has lasted at least 15 minutes,
   complete evidence proves no viable active worker or executing work, and no
   progress has occurred during that interval. Quiet files alone never qualify.
6. `WORKING`: positive current lead execution is established by herdr, lifecycle
   or a verified attributed gate process, without an unresolved contradiction or
   explicit pending worker wait. Recent writes and active workers alone do not
   qualify the lead as working.
7. `RECENT`: execution has ended, no active workers remain, and the last structured
   execution activity is younger than `--idle-after`.
8. `IDLE`: quiet is established for at least `--idle-after`, with no outstanding
   prompt, question, active workers, current execution or unresolved wait.
9. `UNKNOWN`: no preceding rule can be established from available evidence.

An `IDLE` row SHALL include the team's next ready ticket from the existing ready
queue when readable. A queue failure is reported once and does not change verdicts.

### Collection protocol

The command SHALL use the same read-only Rust probe locally and remotely through
a hidden ix-board command with typed JSON request/response and explicit protocol
version. Remote collection requires a compatible installed binary; absent or
incompatible binaries produce unavailable evidence, never the old shell/mtime
heuristic. One batched SSH probe per enabled remote machine is combined with
`herdr --machine <label>` reads behind injectable machine-read traits.

The collector SHALL enforce a 60-second per-machine timeout and finite limits
on file enumeration, bytes read, record size, retained state and response size.
Limits SHALL be documented constants or configuration and enforced before an
unbounded allocation/read. On reaching a limit, affected evidence is unavailable
or uncertain with a reason; healthy rows remain present. Stored identity metadata
does not retain full transcript headers, prompts, credentials or instructions.

When a file changes during collection, the collector SHALL retry it once and
report uncertainty if it changes again. Host clocks and start/end timestamps
SHALL accompany evidence so remote clock skew cannot fabricate activity ages
or a consistent snapshot. Missing permissions and unsupported platform sources
remain explicit availability results.

### Consumers

The working-share metric in [FR-024](./FR-024-retro-snapshot.md) SHALL count only
`WORKING` rows, excluding `WAITING` and `RECENT`. When a relevant classification
or population is unresolved, it SHALL be unavailable under
[FR-026](./FR-026-retro-unavailable-not-zero.md). Query provenance SHALL identify
this lifecycle definition so [FR-027](./FR-027-retro-delta.md) reports a definition
change rather than a numeric old/new comparison.

Minder consumers SHALL treat `WAITING` and `RECENT` as observation-only, with no
nudge triggered by either verdict. Consumers SHALL accept nullable counts and
the deprecated alias while preserving command exit status, errors and coverage
instead of treating a partial or failed command as a complete zero-worker report.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-018-AC-1 | One invocation returns one row per agent pane across local/enabled machines, matching named workspaces case-insensitively; missing teams and unreadable machines are explicit and healthy rows survive. | Test |
| FR-018-AC-2 | Claude real-record fixtures exercise all eight verdicts and precedence; ended execution younger than idle-after is RECENT, established quiet exactly at the threshold is IDLE, and a pending worker wait overrides herdr working. | Test |
| FR-018-AC-3 | Codex real-record fixtures exercise the same verdicts, threshold and precedence, deriving direct and nested parentage from structured records rather than rollout count. | Test |
| FR-018-AC-4 | Missing adapters/transcripts retain herdr, tail and available attributed-command evidence, mark partial, expose availability reasons and leave unprovable counts null. | Test |
| FR-018-AC-5 | Injectable machine-read fixtures prove local and remote host-clock ages and identical typed protocol results without network access. | Test |
| FR-018-AC-6 | IDLE rows include the next ready ticket when readable; an unreadable queue is reported once without changing verdicts. | Test |
| FR-018-AC-7 | Text and versioned JSON report the same rows/counts, including null; idle-after accepts positive s/m/h values and rejects unsupported, zero or negative values. | Test |
| FR-018-AC-8 | The captured eight-file topology (two active direct, two completed direct, two completed nested fork, two stopped direct) yields active_direct_workers=2, active_nested_workers=0 and live_subagents=2. | Test |
| FR-018-AC-9 | Completion and user cancellation immediately remove a worker from active counts despite a fresh transcript mtime; an assigned running/waiting worker remains active after more than ten minutes without writes. | Test |
| FR-018-AC-10 | A completed/stopped worker stays terminal after reopen or incidental writes, restarts after a confirmed later assignment/start, and an end_turn alone does not complete an unfinished assignment. | Test |
| FR-018-AC-11 | Claude fixtures reconcile finished/stoppedByUser/parentAgentId/spawnDepth: both terminal flags mean stopped, resume alone does not reopen finished work, parentUuid never supplies ancestry, missing transition timestamps stay null, and unorderable conflicting evidence yields uncertainty. | Test |
| FR-018-AC-12 | Codex fixtures distinguish task/turn completion/interruption, successful follow-ups and reopening; list_agents snapshots require matching call_id, string/object statuses normalize without result text, and guardian_review/internal records remain visible but excluded. | Test |
| FR-018-AC-13 | One active direct worker and one active depth-two worker produce separate counts of one each; internal/approval sessions add zero, and duplicate identical identities never increase either count. | Test |
| FR-018-AC-14 | Ambiguous ancestry, conflicting duplicate identities, unknown role/state, missing discovered children and unsupported lifecycle formats yield explicit uncertainty/null in affected counts; a complete empty graph alone yields exact zero. | Test |
| FR-018-AC-15 | Normalized output includes identity, immediate parent, role/state, structured activity, transition/observation timestamps and provenance, including excluded records and unavailable timestamp reasons. | Test |
| FR-018-AC-16 | A fake third harness adapter produces lifecycle observations and correct counts/verdicts using the unchanged shared classifier. | Test |
| FR-018-AC-17 | WAITING is emitted for a lead awaiting active workers and for an inactive lead with active direct workers; neither state becomes WORKING merely because worker files are recent. | Test |
| FR-018-AC-18 | WAITING-STUCK requires a confirmed pending wait, complete evidence of no viable worker/executing work and 15 minutes without progress; 14m59s, quiet active workers and incomplete evidence cannot satisfy it. | Test |
| FR-018-AC-19 | Recent transcript/git writes or old nonterminal records without positive current execution never yield WORKING; source sample time/current-state uncertainty is preserved, contradictory execution/terminal evidence yields UNKNOWN, and explicit approval/question fixtures retain PROMPT/ASKING precedence. | Test |
| FR-018-AC-20 | Quiet discovered children are enumerated without an mtime cutoff; a changing file is retried once, repeated change yields uncertainty, and enumeration/read/state/output limits produce bounded responses with explicit reasons. | Test |
| FR-018-AC-21 | Missing/incompatible remote probe versions, malformed/truncated protocol responses, permission failures, unavailable platform sources and a 60-second timeout retain independent healthy rows and explicit error/coverage results without heuristic fallback. | Test |
| FR-018-AC-22 | Skewed host clocks and collection start/end fixtures keep documented ages in the owning host's clock domain and expose inconsistent/unknown timing instead of fabricating current execution. | Test |
| FR-018-AC-23 | Schema/examples expose schema/protocol versions, nullable counts, deprecated live_subagents equality including null, normalized workers, recent activity and per-source availability. | Test |
| FR-018-AC-24 | Retro working-share counts only WORKING, becomes unavailable for unresolved relevant rows/population, and changes query provenance so comparison with the previous definition reports definition change rather than a numeric delta. | Test |
| FR-018-AC-25 | Minder fixtures observe WAITING/RECENT without nudging, accept null corrected counts/alias, and preserve failed/partial command status, errors and coverage. | Test |
| FR-018-AC-26 | macOS and Linux smoke runs demonstrate the shared Rust probe and supported lifecycle collection; inaccessible remote/platform sources are recorded as unavailable with reasons rather than claimed successful. | Demonstration |

## Dependencies

- **Intent**: [US-001](../usecase/US-001-team-leader-reads.md), team-leader inspection of ready work and project context. SWM-158 corrects lifecycle evidence before it drives intervention.
- **Consumers**: [FR-024](./FR-024-retro-snapshot.md), [FR-026](./FR-026-retro-unavailable-not-zero.md), and [FR-027](./FR-027-retro-delta.md); companion dev-team minder rules consume the versioned agents output.
- **Verification**: the computed Quire matrix derives criterion coverage from test trace tags. Existing tests in `tests/agents.rs` must be revised for corrected AC-2/AC-3 behavior; lifecycle/protocol/consumer tests must bind the discriminating criteria above. This spec change does not claim those implementation tests pass.
