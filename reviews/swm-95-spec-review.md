---
id: SR-026
title: FR-018 agent activity verdicts spec review
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@31eeb4a242162545494da9aa223886ac814d55a0; spec/functional/FR-018-agent-activity-verdicts.md, spec/spec.md, spec/usecase/US-001-team-leader-reads.md"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-018"
    type: reviews
---

# SR-026: FR-018 agent activity verdicts spec review

## Summary

Ticket: SWM-95. Spec review of the new FR-018 against design section 5a
(`ix-board agents` must tell, in minutes, whether each agent pane is working,
for Claude Code and Codex). Integrity and EARS sub-checks folded in.

## Verdict

FR-018 matches the design intent: the signal table, the verdict list, the
rule order and both required harnesses are specified, and every AC is
testable. The "recent turn → WORKING" rule matches the design's IDLE
definition (idle only after `--idle-after`). No major defects.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | FR-018 fails `quire validate` structurally: required `## Dependencies` section is missing (FR-017 on main has the same gap). | spec/functional/FR-018-agent-activity-verdicts.md |
| FND-002 | low | Four agentless passive SHALL clauses (`shall be listed`, `shall be measured`, `shall be reported`) flagged by quire grammar checks. | spec/functional/FR-018-agent-activity-verdicts.md:19-27,69 |
| FND-003 | low | The Description packs many SHALL obligations into one FR statement (command, machines, rows, signals, rules, output, exit code). | spec/functional/FR-018-agent-activity-verdicts.md:13-82 |
