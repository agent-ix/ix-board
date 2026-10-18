---
id: SR-024
title: ix-board agents activity detector code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@31eeb4a242162545494da9aa223886ac814d55a0; src/agents/{mod,probe,harness,herdr}.rs, src/agents/probe.sh, src/main.rs, src/lib.rs, tests/agents.rs, tests/fixtures/agents/*"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-018"
    type: reviews
---

# SR-024: ix-board agents activity detector code and Rust review

## Summary

Ticket: SWM-95. PR agent-ix/ix-board#49. Code review with the rust-review lane
folded in, scoped to `git diff origin/main...HEAD`. Major problems only, per
the owner's ruling; mediums and lows are recorded, not routed for fixing.

Real-machine shapes checked read-only: terra Claude session dirs
(`~/.claude/projects/<proj>/<id>.jsonl`, `<id>/subagents/agent-*.jsonl`),
terra and luna Codex rollouts (`session_meta` first line carries
`"parent_thread_id":"<id>"` with no whitespace; `task_complete.last_agent_message`
and `response_item` assistant `output_text` present), `herdr agent list`,
`herdr --machine luna agent list`, `herdr machine list --json` (bare array),
and the `HERDR_PANE_ID` environ values (match herdr pane ids such as `wE:p7`).

## Verdict

Not mergeable. One high finding: the `@@PS` parser splits on single
whitespace characters, but `ps -eo pid=,ppid=,comm=,args=` pads its columns,
so on real output `comm` is always empty and the child-process signal (rule 3)
never fires. The tests use single-spaced fixture lines and cannot catch it.
A fix must not just switch to `split_whitespace`: `comm` itself can contain
spaces (terra runs MCP servers whose comm is `npm exec prism-`), which would
then parse as `npm` and mark every Claude pane WORKING. Read `comm` from
`/proc/<pid>/comm` or emit a delimiter, and add a real-shaped padded fixture.

What holds: transcript locations and JSON shapes match real Claude and Codex
files; the transcript read is bounded to the last 256 KiB; ssh arguments are
single-quoted and session ids are filtered to `[A-Za-z0-9_-]`; team names never
reach a shell; a failed machine adds an error without dropping other rows; no
production `unwrap` on untrusted input.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | `@@PS` parsing uses `splitn(4, char::is_whitespace)` on column-padded `ps` output, so `comm` is always empty and child build/gate processes are never detected; tests use single-spaced fixtures. | src/agents/probe.rs:177-186, src/agents/probe.sh:81 |
| FND-002 | medium | No overall timeout on `ssh` command execution or `herdr --machine`; only `ConnectTimeout=8`. A remote that connects then hangs blocks the whole report. | src/agents/probe.rs:236-267, src/agents/probe.rs:297-303 |
| FND-003 | medium | When `herdr` reports a waiting-for-background-agents pane as `working`, rule 2 returns WORKING and the tail is never read, so WAITING-STUCK is unreachable for that case; unverified against a live stuck pane. | src/agents/mod.rs:153-168, src/agents/mod.rs:454 |
| FND-004 | low | `WORK_COMMANDS` includes `kani`, which FR-018 does not list. | src/agents/mod.rs:39-41 |
| FND-005 | low | Codex guardian-review child rollouts (`thread_source: guardian_review`) count as live subagents. | src/agents/probe.sh:40-44 |
| FND-006 | low | `missing_transcript_is_partial_not_dropped` sets `scenario.turn_age = 0`, which the overriding probe never reads. | tests/agents.rs:332-334 |

## Gates

Not run. Free disk on `/` was 8.8 GiB, under the 20 GiB floor for builds, so
no cargo build was started. FND-001 was confirmed from real `ps` output on
terra (`1       0 systemd         /lib/systemd/...`) against the Rust
`splitn` semantics; `quire validate` and `quoin matrix` were run.

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-007 | high | Fix commit 3e1c077 does not compile: `impl Trait` in a closure parameter (E0562) in the pipe-drain closure; `make ci` on titan stops at lint. | src/agents/probe.rs:250 |

## Dispositions

| FND | Outcome | sha/reason |
| --- | --- | --- |
| FND-001 | fixed | 3e1c077 (logic verified; probe.sh snippet run on terra emits tab-delimited records that keep `npm exec prism-` whole) |
| FND-002 | fixed | 3e1c077 (60 s limit on every herdr and probe call, kill on expiry, unit test) |
| FND-003 | accepted-no-change | owner ruling (one reviewer, major bugs only): mediums and lows are recorded, not fixed in this PR |
| FND-004 | accepted-no-change | owner ruling (one reviewer, major bugs only): mediums and lows are recorded, not fixed in this PR |
| FND-005 | accepted-no-change | owner ruling (one reviewer, major bugs only): mediums and lows are recorded, not fixed in this PR |
| FND-006 | accepted-no-change | owner ruling (one reviewer, major bugs only): mediums and lows are recorded, not fixed in this PR |
| FND-007 | still-open | does not compile at 3e1c077; new head expected |
| FND-007 | fixed | c8f8798 (pipe drain is now a generic fn; `make ci` on titan at c8f8798 exit 0, all suites ok) |
