---
id: SR-6113
title: ix-board fleet status gap analysis against FR-034
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board@3af5b3dec92173262660063327861391b63c8e52; spec/functional/FR-034-fleet-lane-status.md, src/fleet/*.rs, src/main.rs, tests/fleet.rs"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-034"
    type: reviews
---

# SR-6113: ix-board fleet status gap analysis against FR-034

## Summary

Ticket: SWM-209. PR agent-ix/ix-board#89. Planless gap analysis. Plan
completion: not assessed. `quoin matrix --json` (quoin 0.36.10) reports all
16 FR-034 criteria (AC-1 to AC-16) as `tagged`; every tag is on a test in
`tests/fleet.rs`, and all 20 tests pass. No stubs, `todo!` or
`unimplemented!` were found in `src/fleet`. Reverse gap: every new module
traces to FR-034. The criteria were then read against what the tests assert
and what the live command reads.

## Verdict

Coverage is complete on paper and mostly real. The gaps are in the inputs
FR-034 lists that the live command does not read, and in two criteria whose
tags overstate what the tests prove. None of them blocks this slice on its
own; the blocking defects are in SR-6112.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-034 Inputs lists herdr state and title, pane text, git/GitHub PR heads and commits, the FR-018 verdict, host busy ratio and capacity. The live command reads none of them: `NoWeakSignals` is wired, so AC-3 holds only against a fake. | src/main.rs:796-801, src/fleet/live.rs:258-266 |
| FND-002 | medium | AC-9 requires an UNREACHABLE lane to count as not working "in capacity figures", but the report has no capacity figure and no test checks one. | tests/fleet.rs:386-426 |
| FND-003 | medium | The `no_events` defect fires for every lane silent past `no_events_after`, including quiet idle lanes whose last event is `stop` or `idle`. The relay's own `ix.fleet.lane.no_events` (ix-relay FR-045) excludes quiet lanes. In steady state most idle lanes will carry the defect, and every run exits partial. This is a FR-034 spec/relay divergence, not only a code gap. | src/fleet/report.rs:540-552 |
| FND-004 | low | AC-12 names the order deadline among its boundary fixtures, but receipts are faked as already expired, so no test exercises a deadline boundary. | tests/fleet.rs:342-372 |
| FND-005 | low | AC-15 is tested by serialising the struct and rendering text in-process; no test runs the CLI with `--json`. | tests/fleet.rs:575-620 |
