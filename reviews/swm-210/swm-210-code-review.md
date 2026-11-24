---
id: SR-6106
title: ix-board agents done-idle, Codex tail read and host isolation code and Rust review
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@931a90a88df3055152d8d88bf2c9c1658cc20651; src/agents/{collect,mod,probe}.rs, src/main.rs, docs/agents-and-drift.md, docs/schema/agents-report.schema.json, tests/agents.rs, tests/retro.rs; consumers src/retro/{live,metrics}.rs, src/team_metrics/collect.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-018"
    type: reviews
---

# SR-6106: ix-board agents done-idle, Codex tail read and host isolation code and Rust review

## Summary

Ticket: SWM-210, PR ix-board#87 at 931a90a. Code review with the rust-review
lane folded in, scoped to `git diff origin/main...HEAD`, against FR-018-AC-27,
FR-018-AC-28 and FR-018-AC-29. Findings 1 and 2 were reproduced with a scratch
binary calling the public `collect_probe` against generated Codex rollouts.

## Verdict

Request changes. The herdr `done` rule and the per-host timeout are correct.
The Codex tail read is not. It replays a window that begins after the
`session_meta` header and inside a turn. For any Codex rollout over 1 MiB,
worker identity is lost and spurious lifecycle issues appear. Both effects
turn rows back into UNKNOWN or partial, which is the opposite of what SWM-210
is meant to do.

Checked and clean:
- The three edited assertions (`shared_classifier...`, `writes_do_not_prove...`
  and the `("running", Empty, "done", 3600)` table row) change only to follow
  FR-018-AC-27. Nothing else in those tests was weakened.
- Changing `Host` to `Send + Sync` and `Box` to `Arc` is source-only. The
  probe protocol is untouched (PROBE_VERSION 2, same `Probe` types), and
  `__agent-probe` calls `serve_probe`, not `Host`.
- Abandoned host threads are bounded. A `SystemHost` stops every command at a
  60 s deadline counted from construction, so an abandoned thread lives at most
  about 15 s past the 45 s cutoff. With the exporter's 10-60 s interval, that
  is at most about two stray threads per hung host. There is no unbounded
  growth.
- Exit status does not regress. A failed host already put text in `errors`,
  which already gave a non-zero exit on main. A host that hangs used to fail
  at the 60 s call timeout. FR-018 requires a non-zero exit.
- Memory in the tail read is bounded: one record buffer of at most
  `MAX_RECORD + 1` bytes and a `take` limit on each window.
- `cargo clippy --all-targets -D warnings` is clean, and `tests/agents.rs`
  and `tests/retro.rs` pass.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Codex tail replay skips the session_meta header for any rollout over 1 MiB. Each such child gets `header_changed_during_collection` and loses parent_id, agent_path, role and declared_depth. The parent never fills child_owners. Reproduced. | src/agents/collect.rs:284-311, src/agents/collect.rs:826-829, src/agents/harness.rs:483 |
| FND-002 | high | A tail window that starts mid-turn replays collab_waiting and other execution records with no assignment, which raises `execution_without_active_assignment`. A later lifecycle record in the same window ends the search with that issue still set, so decide() returns UNKNOWN. expected_children and pending calls are also limited to the window. Reproduced. | src/agents/collect.rs:296-299, src/agents/harness.rs:300-317 |
| FND-003 | medium | Retro snapshot and ix-team-exporter wrap build_report in a 25 s AGENTS_DEADLINE, which is below HOST_TIMEOUT (45 s). A host that takes 25-45 s (luna took 26 s) still drops every host's rows there. metrics.rs checks only `errors`, so a timed-out host would not mark the population unresolved. | src/retro/live.rs:607, src/retro/metrics.rs:1098, src/agents/mod.rs:408 |
| FND-004 | low | The timeout arm records the host only in missing_hosts, while the list-failure and panic arms also push to `errors`. Consumers that read `errors` (retro metrics, the minder's machine-error check) miss timeouts. | src/agents/mod.rs:478-481 |
| FND-005 | low | A missing host maps to Outcome::Internal, the same code as a total failure. Outcome::Partial exists and would let callers keep healthy rows. | src/main.rs:494-496 |
| FND-006 | low | The first-line skip throws away a complete record when the window starts exactly on a record boundary. In the final 64 MiB window this can drop the only lifecycle record. | src/agents/collect.rs:247-266 |
| FND-007 | low | Growing windows re-read overlapping ranges. One transcript can be charged 85 MiB (170 MiB with the retry) against the 256 MiB collection budget, so a few large rollouts can starve every other session. | src/agents/collect.rs:290-304 |
