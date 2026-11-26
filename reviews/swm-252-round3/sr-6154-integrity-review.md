# SR-6154 — FR-041 integrity review

Redacted review record. Host names, role-spec local paths and raw values are omitted.

Reviewed head: `f25d380`.

## Accepted findings tracked in this fix

- Sustainable slots come from explicit build-slot configuration until the build
  scheduler producer is adopted; lane capacity remains optional.
- Keep the admission-only throttled-share basis distinct from build-abort outcomes.
- Reconcile capacity-history retention with general ledger retention.
- State that a knee records the preceding sustainable lane count.
- Record the follow-up needed for superseded build-scheduler series in the role spec.

Raw examples and private cross-repository paths were excluded from this public copy.
