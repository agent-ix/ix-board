# SR-6151 — SWM-252 gap analysis

Redacted review record. Live fleet examples, machine-specific paths and raw values are
omitted.

Reviewed head: `f25d380`.

## Accepted gaps tracked in this fix

- Tag tests to the criteria they assert and implementations to the criteria they
  implement; correct inaccurate tags.
- Exercise schema migrations with preserved rows and prove view results with fixtures,
  not only fresh-ledger view existence.
- Use label-aware Prometheus fixtures for the live measurement reader.
- Add live-source coverage for ready-queue rules, demand, capacity-window production,
  attribution and drain behavior.
- Ensure the PR body names criteria, related code checked and focused tests run.

Raw examples and reviewer-local paths were excluded from this public copy.
