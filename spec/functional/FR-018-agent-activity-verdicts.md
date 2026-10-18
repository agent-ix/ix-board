---
id: FR-018
title: Report per-agent activity verdicts for herdr workspaces
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-001"
    type: implements
---
# FR-018: Report per-agent activity verdicts

## Description

`ix-board agents <team>... [--idle-after <duration>]` SHALL answer, in one
invocation and measured in minutes, whether each coding agent in the named
teams is working right now. A team is the label of a herdr workspace. The
command SHALL read the local herdr session and every enabled machine from
`herdr machine list`, and SHALL emit one row for every agent pane in a
workspace whose label equals a named team (case-insensitive). A named team
that matches no workspace SHALL be listed as missing; a machine that cannot be
read SHALL be listed as an error and SHALL NOT hide the rows of other
machines. `--idle-after` takes seconds, minutes or hours (`90s`, `20m`, `2h`)
and defaults to `20m`.

Each row SHALL carry the team, machine, pane, harness, herdr state, verdict,
minutes since the last turn, minutes since the last subagent write, live
subagent count, child processes of the pane, git ages of the pane working
directory, and whether the row is `partial`. Ages SHALL be measured against
the clock of the machine that holds the files.

Signals:

- herdr state from `herdr agent list`: `working`, `idle`, `done`, `blocked`,
  `unknown`.
- Last turn: modification time of the harness transcript located from herdr's
  `agent_session` id, and the last assistant message in it. Claude Code reads
  `~/.claude/projects/*/<id>.jsonl`; its subagents are
  `<id>/subagents/agent-*.jsonl`. Codex reads
  `~/.codex/sessions/**/rollout-*-<id>.jsonl`; its child threads are rollouts
  whose `parent_thread_id` is the session id.
- Child processes: processes whose environment carries the pane's
  `HERDR_PANE_ID` and whose command is a build or gate tool (`cargo`, `rustc`,
  `make`, `gh`, `flock`, `pytest`, `npm`, `pnpm`, `locked-build.sh`).
- Pane tail: `herdr agent read --lines 30`, for panes whose herdr state is not
  `working`.
- Git: age of the last commit and of the newest uncommitted file in the pane
  working directory.

A subagent is live when its transcript was written in the last 10 minutes.
Remote machines are read with one batched `ssh` per machine for transcripts,
processes and git, plus `herdr --machine <label>` for herdr state. Those reads
SHALL sit behind a trait so tests supply fixtures.

Verdicts, evaluated in this order, first match wins:

1. `PROMPT`: herdr state `blocked`; carries the dialog text from the tail.
2. `WORKING`: herdr state `working`.
3. `WORKING`: a live subagent or a child process exists.
4. `WAITING-STUCK`: the tail says it is waiting for background agents and
   neither the last turn nor any subagent write is younger than 15 minutes.
5. `UNKNOWN`: herdr state `unknown`; carries the raw herdr fields.
6. `ASKING`: herdr state `idle` or `done` and the last assistant message ends
   with a question or asks for permission; carries the excerpt.
7. `WORKING`: the last turn or the last subagent write is younger than
   `--idle-after`.
8. `IDLE`: otherwise.

An `IDLE` row SHALL carry the team's next ready ticket from the existing ready
queue (the team name read as a Linear team key) when the queue can be read; a
queue that cannot be read SHALL be reported once in the result and SHALL NOT
change any verdict.

Harness adapters sit behind a `Harness` trait chosen from herdr's `agent`
field. Claude Code and Codex are supported. A pane whose harness has no
adapter, or whose transcript cannot be found, still receives its herdr state,
tail and child processes and is marked `partial` with no turn ages; it is never
dropped.

The terminal view SHALL be a table with one row per agent, an indented excerpt
line for `PROMPT`, `ASKING` and `UNKNOWN`, the next ready ticket for `IDLE`,
and lines for missing teams and machine errors. `--json` SHALL return the same
rows as structured data. The command SHALL exit non-zero when any machine
could not be read.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-018-AC-1 | One invocation returns one row per agent pane in the named workspaces across the local session and every enabled machine, matching labels case-insensitively, listing unmatched teams as missing and unreadable machines as errors without dropping other rows. | Test |
| FR-018-AC-2 | The Claude Code adapter yields a verdict through the full rule order for every verdict (`WORKING`, `WAITING-STUCK`, `PROMPT`, `ASKING`, `IDLE`, `UNKNOWN`) from fixture transcripts, and an agent idle for exactly `--idle-after` is `IDLE` while one idle for less is `WORKING`. | Test |
| FR-018-AC-3 | The Codex adapter does the same for every verdict and the idle-age threshold, counting rollouts spawned by the session as subagents. | Test |
| FR-018-AC-4 | A pane with no adapter or no transcript is reported with herdr state, tail and child processes and marked `partial`; it is not dropped. | Test |
| FR-018-AC-5 | Machine reads (herdr and the batched probe) go through a trait; the probe output parser produces ages from the remote clock, and no test touches the network. | Test |
| FR-018-AC-6 | `IDLE` rows carry the team's next ready ticket when the queue is readable; an unreadable queue is reported once and leaves verdicts unchanged. | Test |
| FR-018-AC-7 | Text and JSON output carry the same rows; `--idle-after` accepts `s`, `m` and `h` suffixes and refuses other values. | Test |
