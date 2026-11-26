# SR-6181 — Live lane pull-request reader review

Redacted review record. Raw ticket IDs, lane addresses, API responses and local paths
are omitted.

Reviewed head: `f25d380`.

## Accepted findings tracked in this fix

- Send a GitHub User-Agent.
- Map tickets to lanes from current relay work events and ledger assignment history;
  use Linear attachments as authoritative PR attribution.
- Filter historical work events by latest agent state and freshness.
- Preserve unrelated unattributed PR items without blocking every lane's stale-PR
  resolution.
- Keep a missing lane-ticket mapping unavailable rather than treating it as no PRs.
- Preserve existing escalation actions while their source is unavailable.
- Exercise the production HTTP reader path and document token setup.
