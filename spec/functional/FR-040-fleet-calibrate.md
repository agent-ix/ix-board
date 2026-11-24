---
id: FR-040
title: Calibrate fleet detectors against hook-event ground truth
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
---
# FR-040: Calibrate fleet detectors against hook-event ground truth

## Description

The `ix-board fleet calibrate [--since <time>] [--json]` command SHALL report the
precision and recall of each detector against ground truth derived from hook
events, plus the error rate of the fused `fleet status` classes against sampled
audits. A detector SHALL never be graded against evidence it consumed.

## Behavior

Detectors: herdr agent status, pane-text classification, the FR-018 `agents`
verdict, and lane self-report. Self-report is a state a lane's own relay message
claims (such as working or idle). Ground truth is derived only from hook events:
at a sample time `t` a lane is *active* when a tool-use or user-prompt event
exists in `[t-W, t+W]` and *inactive* when the latest event before `t` is a stop or
idle-prompt and no tool-use event follows within `W` (`W` default 5m). Samples
without a determinable truth are excluded and counted. A detector may not consume
hook events; `fleet status` classes consume them and are therefore graded only
against audits.

Audits: the director records sampled pane-read observations (lane, time, observed
class, observer) as structured input; `calibrate audit add` validates and stores
them ([FR-039](./FR-039-fleet-ledger.md)). `fleet status` WORKING precision is
computed over audits in the window. Output per detector: sample count, excluded
count, confusion counts, precision and recall per class, and error rate. Where the
sample count is below a configured minimum (default 30) the result is
`insufficient_samples`, never a rate. Where any detector or the fused status has an
error rate above 20% over a sufficient sample, the command emits a `defect_ticket`
record (title, evidence, rates, window) for the caller to file; ix-board writes no
tracker data. The command reports whether the 24-hour WORKING-precision target
(error under 10%) is met. Calibration output also reports ground-truth coverage
per host so a host without hook events is visible.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-040-AC-1 | A hook-event fixture with known detector outputs yields the hand-computed precision, recall and confusion counts for each of the four detectors. | Test |
| FR-040-AC-2 | Samples with no determinable ground truth are excluded and counted; a detector's own input is never used as its ground truth (a test fails if a detector reads hook data). | Test |
| FR-040-AC-3 | Fewer samples than the minimum yields `insufficient_samples` and no defect record; the minimum is configurable. | Test |
| FR-040-AC-4 | An error rate of exactly 20% emits no defect record and above 20% emits one carrying the evidence; no Linear write occurs. | Test |
| FR-040-AC-5 | Audit samples added with `calibrate audit add` are validated, stored, and drive the fused WORKING precision; a malformed audit is refused. | Test |
| FR-040-AC-6 | The report states whether WORKING error is under 10% over 24h and shows per-host ground-truth coverage. | Test |
| FR-040-AC-7 | Boundary fixtures at `W` seconds around the sample time classify truth as specified. | Test |

## Dependencies

- Hook events with stop and idle-prompt kinds from ix-relay SWM-201 (assumed). Defect filing is performed by the executing agent, not by ix-board.
