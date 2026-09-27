---
id: MP-001
title: ix-board warm-cache next latency
type: MeasurementPlan
status: proposed
owner: ix-board-maintainer
metric: next_warm_cache_elapsed_seconds
definition_version: swm2.next-warm-cache.v1
stage: baseline
statistical_design:
  population: Repeated invocations of `ix-board next --team <T>` after the required team data has been cached.
  sampling: Record each invocation's elapsed wall-clock time from command start until the complete result is available.
  repetitions: 30
  estimator: Maximum elapsed time across the recorded invocations.
  error_model: Host load, process scheduling, filesystem state, and variation in team/project graph size.
  uncertainty: Preserve every observed invocation and document host and fixture/team size with the result.
  decision_rule: The maximum observed elapsed time is less than 1.0 second.
relationships:
  - target: "ix://agent-ix/ix-board/NFR-001"
    type: measures
---
# MP-001: ix-board warm-cache next latency

## Decision Use

This observation provides evidence for NFR-001's warm-cache latency target. It
does not claim the target is met until measurements are collected under an
agreed representative host and team/project population.

## Population

The population is a run of repeated `next --team <T>` invocations after an
initial invocation has populated the required cache. Record team size, project
size, host, build identity, cache state, and every measured elapsed time. The
representative team and project sizes remain a product decision captured in
`spec/spec.md`.

## Collection Procedure

1. Record the implementation build identity and host details.
2. Run `ix-board next --team <T>` once to populate the required warm cache.
3. Run the same command 30 times, measuring wall-clock time from process start
   through complete result availability.
4. Preserve all 30 observations and evaluate the maximum against the strict
   threshold of 1.0 second.

## Interpretation

The result supports the NFR only for the recorded host and team/project size.
Do not compare runs with different warm-cache definitions or populations as one
series. The product decision about a representative population and release
summary statistic remains open. The 30-run maximum in this proposed plan is a
provisional collection choice and must be confirmed before the plan becomes
active or the result is used as a release gate.
