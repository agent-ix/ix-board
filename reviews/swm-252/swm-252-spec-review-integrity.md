---
id: SR-6154
title: "Integrity analysis of SWM-252 spec changes"
type: SpecReview
analysis: integrity
scope: "agent-ix/ix-board; spec/functional/FR-039-fleet-ledger.md, spec/functional/FR-041-fleet-saturation.md"
review_set: subset
---

## Summary

Ticket: SWM-252. Checked FR-041 against itself, FR-039, ix-relay FR-058 and the dev-team orchestrator role spec on origin/main.

## Verdict

**FAIL** - sustainable build slots are sourced from a value that is not a build slot count.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Sustainable slots are now server-capacity.max_agents, an operator-entered agent count. The same FR calls the interim source server-capacity build slots separate from lane capacity, ix-relay FR-058 has no build-slot field, and the role spec projects the recorded lane count into server capacity, so slot saturation compares build slots with agents. | spec/functional/FR-041-fleet-saturation.md:50 |
| FND-002 | medium | AC-20 expects empty evidence for a thermal drain with no admission block or aborted outcome, and build_abort after such a change, but the evidence rule gives thermal_abort from temperature alone and admission_block for admission growth. | spec/functional/FR-041-fleet-saturation.md:215 |
| FND-003 | medium | FR-041 says history is never deleted while FR-039 retention prunes the capacity tables by age; neither exempts capacity history. | spec/functional/FR-041-fleet-saturation.md:128 |
| FND-004 | medium | Run end never states that a knee records the previous level, while the role spec says to step back one lane; found_level is therefore open to recording the knee level. | spec/functional/FR-041-fleet-saturation.md:128 |
| FND-005 | low | The dev-team role spec still names ix_build_slots_sustainable, ix_build_pause_seconds_total and ix_build_probe_value, which FR-041 now replaces; no follow-up is recorded. | spec/functional/FR-041-fleet-saturation.md:224 |
