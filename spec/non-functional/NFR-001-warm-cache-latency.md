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

## Measurement and Evaluation

| Metric | Target | Threshold | Method |
|--------|--------|-----------|--------|
| Warm-cache `next` elapsed time | Less than 1.0 second | 1.0 second | Lightweight repeated CLI benchmark on a warmed checked-in fixture; every run must meet the target. |

## Verification

The benchmark reports elapsed time for each warm-cache run and verifies that
every run meets the target.
