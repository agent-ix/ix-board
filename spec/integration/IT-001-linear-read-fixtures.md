---
id: IT-001
title: Verify Linear reads, queue rules, and cache invalidation with fixtures
type: IT
relationships:
  - target: "ix://agent-ix/ix-board/FR-001"
    type: verifies
  - target: "ix://agent-ix/ix-board/FR-004"
    type: verifies
---
# IT-001: Verify Linear reads, queue rules, and cache invalidation with fixtures

## Objective

Verify that structured Linear read fixtures produce the specified queue layers
and ready decisions, and that a changed source fixture invalidates the cached
record before a later read returns it.

## Target Integration

The test exercises the boundary between the ix-board Linear GraphQL read client,
the local SQLite cache, and the queue-building library. Tests use controlled
fixture responses and do not require a live Linear service.

## Preconditions

- The fixture set contains a running and a non-running project, tickets across
  multiple `blocked-by` layers, and resolved and unresolved prerequisites.
- A SQLite cache can be created in an isolated test location.
- The test can advance or control the cache clock and provide a changed source
  fixture for a later successful poll.

## Inputs

- Structured GraphQL-shaped project, ticket, spec-state, and dependency fixtures.
- Ticket/comment prose fixtures that conflict with structured fields.
- An unchanged source fixture followed by a changed source fixture.

## Test Procedure

1. Load the initial structured fixtures into the read boundary.
   - IT-001-SC-01: the fixture response is consumed without a live Linear call.
2. Request the ready queue with both running and non-running projects and
   multiple dependency layers.
   - IT-001-SC-02: non-running and transitively blocked tickets are excluded;
     eligible tickets appear at their spec step with a reason.
3. Repeat the same request with unchanged source fixtures.
   - IT-001-SC-03: results and order remain identical while the warm cache is
     reused.
4. Advance the controlled poll interval and supply a fixture where one source
   record has changed.
   - IT-001-SC-04: the old cached value is invalidated and the next command
     returns the changed record.
5. Supply prose that conflicts with the structured readiness fields.
   - IT-001-SC-05: readiness and ordering follow structured fields and are not
     changed by the prose.

## Expected Results

All five success criteria hold. Queue membership, dependency layers, step
reasons, and cache replacement match the structured fixtures. Repeated identical
inputs yield identical results.

## Open Questions

The project-status mapping, spec-state step mapping, and cache refresh behavior
after a failed poll must be decided before the fixture expectations can become
fully concrete.
