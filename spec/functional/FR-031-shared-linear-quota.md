---
id: FR-031
title: Coordinate Linear quota across processes
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-010"
    type: satisfies
provenance:
  source: SWM-184
---
# FR-031: Coordinate Linear quota across processes

## Description

The Linear transport SHALL persist server cooldown and local request-budget state
in the shared SQLite cache without storing credentials. The quota gate SHALL be
shared conservatively across credentials on the same machine/cache so two API keys
for one authenticated user cannot multiply its allowance. Every Linear request, including direct, history, update probes,
and forced refresh paths, SHALL consult and reserve against that state before send.

When HTTP 429 or a GraphQL `RATELIMITED` error occurs, the transport SHALL persist
the latest applicable retry-until from Retry-After, exhausted request/complexity
reset headers, or structured error retry metadata before returning the error.
Non-JSON HTTP 429 bodies SHALL retain rate-limit classification. When no usable
retry metadata exists, the transport SHALL use a documented conservative 60-second
cooldown. Concurrent observations SHALL never shorten an active cooldown.

While server cooldown is active, all processes sharing this cache SHALL issue zero
Linear requests. The CLI SHALL print exactly `Linear rate limited; retry in <N>s`
on stderr, with N the ceiling of remaining seconds. No automatic sleep or retry
SHALL occur. Ordinary snapshot reads use the fallback in
[FR-004](FR-004-cache-polling.md); empty-cache reads fail locally. Bounded reads
retain their strict error contract.

The transport SHALL enforce a configurable conservative request and estimated
complexity budget per time window across those processes, atomically reserving
before send and honoring available remaining-quota/reset headers with a reserve.
Configuration SHALL accept plan tiers `free`, `basic`, `business`, and `enterprise`;
the default SHALL be free/basic. Tier fallback SHALL apply multipliers 1, 1, 2,
and 5 respectively to API-key 2,500 requests/3,000,000 complexity per hour or
OAuth 5,000 requests/2,000,000 complexity per hour, with a 10,000 single-query
complexity ceiling. Actual Limit/Remaining/Reset headers SHALL override fallback
capacity. The transport SHALL apply the configured share to both request and
complexity capacities. Configuration validation SHALL accept only finite shares
in the range `0 < share <= 0.5`, including explicit overrides, leaving at least
50 percent for other tools before applying any additional reserve. Defaults SHALL
remain within that range. Business SHALL be selected
for the owner's installation through configuration, not a global default.
A local budget rejection SHALL make no
request, use the same stale fallback eligibility, and emit a distinct
`Linear local budget exhausted; retry in <N>s` diagnostic. Unknown complexity
SHALL use a documented conservative estimate, never zero.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-031-AC-1 | A fixture HTTP 429 with a non-JSON body and Retry-After, and a GraphQL RATELIMITED fixture with exhausted request/complexity resets, persist the latest applicable retry-until; two observations never shorten it; absent usable metadata yields 60 seconds. | Test |
| FR-031-AC-2 | After one process records cooldown, a separately launched process using the same cache attempts workspace, direct issue, history, probe, and force-refresh reads and sends zero requests until expiry; after expiry one eligible attempt is permitted. | Test |
| FR-031-AC-3 | With 2.1 seconds remaining the actual-server diagnostic is exactly `Linear rate limited; retry in 3s`; no retry or sleep occurs, and an empty cache fails without network. | Test |
| FR-031-AC-4 | With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct diagnostic. | Test |
| FR-031-AC-5 | Counters record requests attempted, returned complexity when available, estimated complexity otherwise, duration, mode, and success/failure for each refresh; partial failed refresh costs remain counted and are never described as a successful sync. | Test |
| FR-031-AC-6 | `ix-board cache status` returns cache path, dataset coverage, snapshot age/staleness, last successful sync/full reconciliation, `rate_limit_until`, local budget/reset and latest refresh counters in JSON and text without resolving credentials or making any network request; unknown values are explicit. | Test |
| FR-031-AC-7 | Tier fixtures produce the documented API-key/OAuth fallback limits and multipliers, default to free/basic with at most 50 percent share, and honor lower actual header capacity; explicit shares 0.1 and 0.5 apply to both request and complexity capacities, while zero, negative, above-0.5, non-finite, or malformed shares fail before network access. Two API keys sharing the cache consume one allowance and a query exceeding 10,000 estimated complexity is refused locally. | Test |

## Dependencies

- [FR-004](FR-004-cache-polling.md) defines freshness, single-flight, fallback, and bounded reads.
- Tests use fixture transports and controlled clocks, never the real Linear API.
