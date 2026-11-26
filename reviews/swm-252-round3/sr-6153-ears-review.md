# SR-6153 — EARS conformance review

Redacted review record. Live fleet data and local paths are omitted.

Reviewed head: `f25d380`.

## Accepted findings tracked in this fix

- Split multi-clause capacity statements where necessary.
- Define coverage consistently for counters, gauges, configuration values and
  histogram inputs.

The public specification now describes push-interval coverage and avoids counting
duplicate series or repeated samples more than once.
