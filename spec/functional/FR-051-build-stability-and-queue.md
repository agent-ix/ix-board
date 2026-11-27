---
id: FR-051
title: Central build stability and queue analysis
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/FR-050"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-027"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-049"
    type: depends_on
---
# FR-051: Central build stability and queue analysis

## Description

When an operator runs `ix-board host check`, ix-board SHALL report rolling build
stability and queue evidence for draco, luna, titan and terra from locked-build's
redacted retained status and transition events.

## Inputs

The producer contract is dev-tools' shared `locked_build::wire` projection:
request and event lifecycle timestamps, stable diagnostic codes and limits,
queue depth at enqueue, position, cumulative state timings and blocker
observations, separate requested/granted hard caps and admission reservations,
and status-only needed/free-budget memory. The consumer accepts additive fields.
Legacy status supplies `state`, `enqueued_at_ms`, `started_at_ms`, `ended_at_ms`,
`wait_ms`, `reason`, `signal` and `exit_code` until the producer is installed.

## Behavior

- The view SHALL report `(now - 1h, now]` and `(now - 24h, now]` windows per host.
- Requests count enqueue attempts, including event-only pre-enqueue refusals.
  Completed counts terminal attempts finishing within the window; success rate
  is successful attempts divided by completed attempts. Empty rates are null.
  A succeeded lifecycle with observed exit zero remains successful when a
  supervision-loss diagnostic is present; the loss is still counted as a diagnostic.
- Terminations by code include all abnormal terminal/refused attempts, including
  command exits. Kill rate counts RSS cap, OOM, hold timeout, supervision loss,
  cancellation and external signal outcomes divided by completed attempts;
  known successful attempts do not enter the kill numerator.
- Stable codes take precedence over legacy evidence. Signal 9 alone is an external
  signal, never proof of OOM. Ordinary nonzero exits are command failures.
- The top violated limit is the most frequent resource/hold/refusal bound, with
  deterministic lexical limit-key tie breaking and the latest measured/limit values.
- Queue waits use completed admission waits (nearest-rank p50/p95), independently
  per host and class. Open waits do not enter the percentile population. Precise
  admitted timestamps take precedence over legacy started timestamps.
- Queue depth is a step series reconstructed from enqueue to admission or terminal
  cancellation, clipped to each window and including a baseline. Position and
  enqueue-depth observations are retained when present. Memory values are per
  request, not measured RSS; missing legacy fields remain null. Status-only
  Existing `requested_memory_bytes` and `granted_memory_bytes` remain hard-cap
  aliases. Separate `requested_memory_reservation_bytes` and
  `granted_memory_reservation_bytes` retain admission reservations; missing
  reservation evidence stays null. `memory_needed_bytes` and
  `memory_free_budget_bytes` show the reservation needed and
  currently unreserved budget without substituting granted bytes.
- Blocked seconds sum cumulative per-request observations, allowing overlapping
  reasons. A request spanning the start boundary needs a cumulative observation
  at that boundary; otherwise its blocked duration is unavailable. Legacy reason
  labels are retained without attributing an entire wait to the last reason.
- Event snapshots SHALL NOT count a retained request twice. A correction SHALL
  retain the original request identity and SHALL NOT create another attempt.
  Retained status is authoritative; event-only attempts use the latest snapshot,
  with producer event order breaking equal timestamp ties. Refused attempts that
  exist only in events, ix-board SHALL include them once per request id.
- Malformed, stale (over 120 seconds), future (over five seconds), wrong-host,
  duplicate-identity, incompatible-version and pruned history SHALL be unavailable
  with a safe reason rather than zero. Freshness uses the actual collection clock,
  independently of the requested window end and preceding slow source reads.
  Raw command, owner, environment, message
  and credential data SHALL NOT appear in the view or errors.
- The retro catalog SHALL add per-host build kill rate, OOM count, supervision
  loss count and queue wait p95 seconds ids without changing existing ids. These
  metrics use a fixed rolling 24-hour window; missing evidence is unavailable.
- A rise from a previous available metric SHALL enter the existing retro delta
  defect path and use its explicit `--notify` / `alert_recipient` route. Default
  cadence SHALL include host checks and retro snapshots without extra flags or
  scheduled notification.

## Acceptance Criteria

| ID | Criteria | Verification |
| --- | --- | --- |
| FR-051-AC-1 | Hand-written status and event evidence produces per-host 1h/24h requests, completions, success rates and termination counts with exact boundary inclusion and one count per attempt, including event-only refusals and historical corrections sharing a timestamp with the original outcome. | Test |
| FR-051-AC-2 | Stable codes override legacy reasons; legacy RSS, timeout, supervision, refusal, cancellation, external signal and command exits map correctly; successful degraded supervision retains its diagnostic without becoming a failed or killed attempt; top violated limit carries limit and measured evidence. | Test |
| FR-051-AC-3 | Per-host/class queue p50/p95, depth steps, blocker seconds and separate requested/granted hard caps, reservations and needed/free-budget memory match injected values; empty waits and incomplete blocking history remain null. | Test |
| FR-051-AC-4 | Additive retro ids for all four hosts collect injected stability values and rising values become existing delta defects; missing hosts stay unavailable and notifications require explicit request through the existing recipient. | Test |
| FR-051-AC-5 | No-argument host check includes all four host rows through bounded read-only collection, rejects stale/malformed/wrong-host/pruned history without leaking raw payloads, and default cadence includes the view and retro metrics without notification flags. | Test |

## Dependencies

- [FR-050](./FR-050-host-check.md) owns local host checks.
- [FR-027](./FR-027-retro-delta.md) owns retro delta defects and notifications.
- [FR-049](./FR-049-cadence.md) owns the default cadence manifest.
