# SR-6180 — Fleet saturation and `need_ready_tickets` review

Redacted review record. Raw host names, project names, lane addresses, ticket IDs,
timestamps, metric values and local paths are omitted.

Reviewed head: `f25d380`.

## Accepted findings tracked in this fix

- Normalize planner keys against configured project identifiers and names.
- Derive readiness from the ready-queue contract and include idle unassigned lanes in
  project shares.
- Exclude under-covered lanes from mean duty; an idle or stale host cannot suppress
  other hosts' actions.
- Resolve ready-buffer actions only when the corresponding deficit clears.
- Allocate host ready shares so host buffers and deficits reconcile with the global
  totals.
- Treat an absent lane cap as unbounded; preserve zero-lane bootstrap eligibility only
  with current lease and complete demand evidence.
- Use assigned tickets alone for supply; discard stale work events as supply evidence.
- Key tickets by the canonical issue ID rather than parsing free-text references.
- Keep live read tests and CLI documentation aligned with the acceptance criteria.
