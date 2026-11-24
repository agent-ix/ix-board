---
id: FR-042
title: Replay fleet classification over recorded evidence
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-034"
    type: depends_on
---
# FR-042: Replay fleet classification over recorded evidence

## Description

The `ix-board fleet replay <evidence-dir> [--cadence <duration>]` command SHALL run
the `fleet status` classifier over recorded evidence at a fixed cadence (default
1m) in recorded time and report, per lane and host, the first time each non-WORKING
class was emitted. Replay uses the same classifier as live status and reads no live source.

## Behavior

Evidence is a directory of normalized event files (hook-equivalent events derived
from agent transcripts, host idle observations from the earlier monitor ledger, and
herdr pane records) plus an expectations file listing `{lane or host, expected class,
begins_at, max_detection_delay}`. A replay adapter converts transcripts and the
`monitor-ledger.jsonl` format into events; conversion records anything it cannot map
as unavailable evidence. The report states for each expectation whether the class
was emitted within its delay and the measured delay. The command exits non-zero if
any expectation fails. The recorded-night expectations (2026-11-23/24) are:
terra host idle, lanes wE:p28 and p1Z IDLE, and titan lanes wB:pJ, wB:pM and wB:pN
BLOCKED, each within 10 minutes of `begins_at`. The result is recorded on the ticket.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-042-AC-1 | A synthetic evidence directory with a lane going idle at T is reported IDLE within the cadence, and an expectation with a shorter delay fails the run. | Test |
| FR-042-AC-2 | Replay uses the production classifier and no live source (a test fails on any network, relay or herdr access). | Test |
| FR-042-AC-3 | Transcript and `monitor-ledger.jsonl` fixtures convert to events; unmappable records are listed as unavailable evidence. | Test |
| FR-042-AC-4 | Replay over the captured 2026-11-23/24 evidence flags terra host idle, wE:p28 and p1Z idle, and titan wB:pJ, wB:pM, wB:pN blocked, each within 10 minutes of its start. | Demonstration |
| FR-042-AC-5 | A missing lane in the evidence yields a `missing_lane` defect and a failed expectation, not a pass. | Test |

## Dependencies

- Captured night evidence supplied by the lead; conversion fixtures derived from it SHALL contain no credentials or prompt text beyond classification evidence.
