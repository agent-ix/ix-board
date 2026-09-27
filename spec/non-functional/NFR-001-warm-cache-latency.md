---
id: NFR-001
title: Keep warm-cache next reads under one second
type: NFR
quality_attribute: performance_efficiency
relationships:
  - target: "ix://agent-ix/ix-board/FR-001"
    type: constrains
---
# NFR-001: Keep warm-cache next reads under one second

## Statement

The ix-board CLI SHALL complete `next --team <T>` in less than 1.0 second when
the required team data is available in a warm cache.

## Scope

- Applies to the `next --team <T>` CLI command with its required team data
  already cached.
- Does not characterize cold-cache or remote Linear latency.

## Rationale

Team leaders need a fast repeat read while inspecting and prioritizing work.

## Measurement and Evaluation

| Metric | Target | Threshold | Method |
|--------|--------|-----------|--------|
| Warm-cache `next` elapsed time | Less than 1.0 second | 1.0 second | Repeated CLI measurement under MP-001 |

## Verification

Record the elapsed time for each warm-cache invocation using MP-001. The
performance claim is not verified until the agreed population, environment, and
summary statistic have been fixed and a measured run is available.

## Open Questions

Agree the representative team size, host/environment, warm-cache setup, and
summary statistic (for example, maximum or percentile) before using this
threshold as a release gate.
