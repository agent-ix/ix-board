---
id: SR-6153
title: "EARS conformance of SWM-252 statements"
type: SpecReview
analysis: ears-conformance
scope: "agent-ix/ix-board; spec/functional/FR-039-fleet-ledger.md, spec/functional/FR-041-fleet-saturation.md"
review_set: subset
---

## Summary

Ticket: SWM-252. The engine flags one new FR-041 statement twice; one semantic ambiguity was found.

## Verdict

**CONDITIONAL** - medium and low findings only.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | One statement packs two SHALL clauses and uses an agentless passive (ears non-singular, quality agentless-passive). | spec/functional/FR-041-fleet-saturation.md:48 |
| FND-002 | medium | Under 90 percent window coverage is undefined for counts, instant gauges, config values and counters, so two implementers would compute availability differently. | spec/functional/FR-041-fleet-saturation.md:47 |
