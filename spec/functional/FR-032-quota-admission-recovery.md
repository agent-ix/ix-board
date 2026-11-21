---
id: FR-032
title: Explain quota admission and recover from expired evidence
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-010"
    type: satisfies
provenance:
  source: SWM-185
---
# FR-032: Explain quota admission and recover from expired evidence

## Description

The shared quota gate SHALL evaluate admission and recovery using explicit typed
states, preserving the conservative accounting and actual-server backoff in
[FR-031](FR-031-shared-linear-quota.md). The evaluator SHALL distinguish a local
allocation hold from a server remaining-quota reserve hold, even when local usage
is zero. The evaluator SHALL expire each server observation with its own evidence
rather than attach an older remaining value to a later observation's reset.

## Behavior

### Admission states

The gate SHALL derive a decision for a specified request estimate and endpoint
from one atomic view of persisted state, configuration, and current clock time.
Admission SHALL re-evaluate and create the durable reservation in the same shared
transaction; an earlier status result is explanatory evidence, not permission to
send. Status inspection SHALL perform no network request or credential lookup.

The decision SHALL expose a typed state with these meanings:

| State | Meaning | Next transition |
|-------|---------|-----------------|
| Available | The requested operation fits every applicable guard after pending charges and debt. Server headroom can still be unknown. | A reservation, observation, settlement, configuration change, or deadline can change eligibility. |
| LocalWindowHeld | The requested operation exceeds the configured local request or complexity allocation. | The fixed local window deadline, unless another independent hold remains. |
| ServerReserveHeld | A live request, complexity, or endpoint observation leaves insufficient remaining quota after reserve and outstanding charges. | The owning observation's expiry or admissible new evidence, unless another independent hold remains. |
| ServerBackoff | An actual HTTP 429 or GraphQL RATELIMITED cooldown is active. | The persisted latest applicable retry-until. |
| EvidenceUnavailable | Required persisted state or observation provenance is invalid and cannot be conservatively recovered. | Repair or replacement of that evidence; no invented retry deadline. |
| RequestRejected | The request estimate or configuration is invalid, or a single request exceeds its allowed ceiling. | A valid request or configuration; waiting alone does not fix it. |

The evaluator SHALL retain all applicable blockers with typed limiting dimensions
(`requests`, `complexity`, `endpoint_requests`, or a nondimensional evidence/input
failure). The primary state SHALL prefer ServerBackoff, then EvidenceUnavailable,
then RequestRejected, then ServerReserveHeld, then LocalWindowHeld. Available SHALL
have no blockers. Each timed blocker SHALL carry its own absolute deadline; the
decision SHALL expose the earliest next reevaluation time rather than promise
admission at that time. An unknown transition SHALL remain unknown.

Each dimensional blocker SHALL expose the applicable capacity, reserve, estimated
request charge, local usage, outstanding charge/debt, and effective remaining
evidence where known, with explicit unknown values. Local-window deadlines and
generations SHALL remain distinct from server observation deadlines and
generations. The evaluator SHALL NOT infer remote availability from zero local
usage or from expiry of one blocker.

### Paired observations and bounded recovery

For each request, complexity, and endpoint dimension, the gate SHALL retain the
remaining value together with its response/request provenance, observation time,
reset ownership, and expiry. A reset from a different response SHALL NOT extend
the lifetime of an earlier remaining value. A reset-only or limit-only response
SHALL NOT refresh an older remaining observation. A remaining-only response SHALL
use a fixed expiry measured from that observation, bounded by the configured
fallback quota-window duration (one hour by default); subsequent unrelated
responses SHALL NOT slide that expiry.

Until its expiry, each accepted restrictive observation SHALL constrain admission
conservatively with subsequent reservations and unsettled charges. A higher
remaining value or later reset, by itself, SHALL NOT erase a live lower constraint
or prove a new server generation. Independent valid observations MAY coexist;
their effective constraint SHALL be at least as restrictive as every unexpired
applicable observation. The persisted representation SHALL be bounded without
extending the expiry of any retained restrictive constraint or losing its charge
ownership. This contract does not require storing every response.

When the owning deadline is reached, the gate SHALL retire only that expired
constraint. A newer valid observation SHALL retain its own remaining value and
deadline after the older constraint expires. The gate SHALL NOT carry the old
minimum into that observation's later window. Out-of-order completion SHALL NOT
resurrect expired evidence, move a local deadline, or refund a newer generation.
Equal-time observations SHALL resolve conservatively and deterministically.

Missing or malformed headers SHALL NOT create positive server headroom. A parsed
reset already at or before observation time SHALL NOT retain a remaining hold
past that reset. Invalid arithmetic, corrupt ownership, or unusable provenance
SHALL produce EvidenceUnavailable when conservative recovery is impossible.
Expiry of valid observations SHALL yield unknown server headroom and reuse only
the existing configured fallback/local allocation; it SHALL NOT create a new
local window, replenish usage, cancel outstanding charges, or assert a recovered
server limit. A clock moving backwards SHALL NOT clear a hold early.

While an actual-server cooldown is active, the transport SHALL issue zero requests
through every path, including force refresh and any recovery attempt. Independent
observations SHALL NOT shorten that cooldown. This change SHALL introduce no
background retry, sleep loop, quota-reset operation, reserve reduction, or special
probe request. Recovery SHALL occur on the next ordinary protected operation or
status evaluation after evidence expires.

The gate SHALL preserve durable single-use reservation ownership, actual-cost
settlement, pending charges, excess-cost debt, conservative abandoned-reservation
charges, replay rejection, wrong-store rejection, and separation of local and
server generations. Restart and migration SHALL NOT reset those obligations or
convert ambiguous legacy evidence into known headroom. Legacy unpaired evidence
SHALL retain its existing restrictive deadline without extending it, or fail with
EvidenceUnavailable when that deadline cannot be recovered safely.

### Consumer and stale-read contract

The cached-read SDK SHALL expose a typed refresh outcome/accessor containing the
quota decision captured when refresh was refused. It SHALL distinguish quota
refusal, transport/source failure, refresh-lock refusal, successful refresh, and
cache reuse without classifying human-readable messages. A snapshot with absent
legacy outcome metadata SHALL expose an unknown outcome rather than infer one
from its failure text. The outcome SHALL describe the read attempt; a later live
status evaluation SHALL NOT rewrite that attempt's provenance.

The SDK SHALL preserve existing read signatures and public struct-literal
construction contracts, including CacheFreshness and existing report structs.
New typed information SHALL use an additive accessor or separately introduced
envelope that does not require callers to add fields or adopt a new constructor.
Existing serialized fields SHALL retain their meanings; additive outcome data
SHALL have explicit absent-field behavior for older stored/output data.

Ordinary stale fallback SHALL preserve the last complete compatible snapshot and
its successful age under [FR-004](FR-004-cache-polling.md). Its typed outcome SHALL
retain the actual quota refusal and transition information. The CLI SHALL render
a local-window hold as a local allocation hold and a server reserve hold as a
locally enforced request, complexity, or endpoint reserve hold, with the owning
deadline when known. Only actual ServerBackoff SHALL be described as Linear rate
limited or populate an active actual-server `rate_limit_until`. Generic network
failure text SHALL NOT replace an available typed quota reason. Bounded reads
SHALL retain their strict no-stale-fallback contract.

`cache status` SHALL expose the evaluated typed decision and dimensional evidence
in JSON and equivalent labeled text, alongside existing counters. With local
requests and complexity both zero and observed request remaining below reserve,
status SHALL identify ServerReserveHeld rather than LocalWindowHeld or actual
server cooldown. Status SHALL label the estimate/endpoint it evaluates so its
Available result cannot be mistaken for eligibility of every possible query.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-032-AC-1 | With request remaining 250, request reserve 500, local requests/complexity zero, no cooldown, and a future paired reset, real persisted admission sends zero calls and returns ServerReserveHeld with requests dimension and the paired deadline. SDK fallback and cache-status JSON/text expose that reason; rate_limit_until has no active cooldown. | Test |
| FR-032-AC-2 | Fixture decisions independently exercise every state, simultaneous local/server/backoff holds, both capacity dimensions, endpoint constraints, unknown evidence, and an oversized single request. Primary-state precedence and every blocker/deadline match the contract; the earliest transition is not labeled a guarantee of admission. | Test |
| FR-032-AC-3 | With a 501-request admission threshold, remaining 250 expiring at T1 followed by remaining 1000 expiring at T2 blocks before T1 and retires only the old constraint exactly at T1, without carrying 250 to T2. Replacing 1000 with 400 keeps the newer hold until T2. Reversing arrival order never bypasses a live constraint or resurrects it after expiry. | Test |
| FR-032-AC-4 | Reset-only, remaining-only, limit-only, missing, malformed, past-reset, equal-time, and out-of-order fixture headers never splice one response's low remaining onto another's later reset. Specifically, remaining 250 with reset T1 arriving after T1 never becomes a new fallback-hour hold. Repeated moving resets cannot extend a retained constraint; missing-reset evidence expires at its fixed fallback bound. Retired evidence is unknown, not invented positive remaining. | Test |
| FR-032-AC-5 | A deterministic clock-driven sequence spans at least three local windows and three server observation expiries while requests, varying actual-cost settlements, and moving reset headers continue. Local usage resets only at fixed local deadlines; every admission satisfies all applicable live constraints; expired low evidence does not cause an indefinite hold, and local zero never proves remote availability. | Test |
| FR-032-AC-6 | Real SQLite reopen/crash-recovery and concurrent-gate fixtures preserve active holds, paired observation ownership, pending/debt charges and single-use reservations. Late settlement across each kind of generation boundary, duplicated settlement, wrong-store tokens, legacy unpaired rows, corrupt provenance, and backward clock movement cannot refund or admit incorrectly. | Test |
| FR-032-AC-7 | Local HTTP fixtures return HTTP 429 and GraphQL RATELIMITED with retry metadata. Until the persisted latest retry-until, independently opened gates and every normal/direct/history/force path make exactly zero further calls even across local resets, newer high remaining observations, and restart. At expiry only a normal otherwise-eligible protected request is admitted; no automatic retry or probe runs. | Test |
| FR-032-AC-8 | A WorkspaceSource/local-HTTP integration returns a complete compatible stale snapshot on each typed quota refusal, preserving source-fetch age and successful sync markers. A source error and lock refusal remain distinct outcomes; a bounded read returns its typed refusal without stale data; absent legacy outcome metadata is unknown without message parsing. | Test |
| FR-032-AC-9 | An external consumer fixture compiles unchanged legacy public struct literals and existing read calls while another consumer obtains typed outcomes through the additive API. Changing human failure-message wording does not change either consumer's typed classification; old serialized metadata without the new outcome retains documented unknown behavior. | Test |
| FR-032-AC-10 | A deterministic sequence/model test drives production admission, persistence reopen, observations, reservations, and settlement using a controlled clock and real storage. Its independent oracle asserts the safety and bounded-expiry properties above after each event, records reproducible failing sequences, and includes generated or enumerated interleavings rather than merely echoing the production evaluator. All tests use fixture sources/local HTTP and make no real Linear API calls. | Test |

## Dependencies

- [US-010](../usecase/US-010-read-within-linear-quota.md) owns useful evidence with visible freshness and quota availability.
- [FR-031](FR-031-shared-linear-quota.md) owns shared budgets, reserves, and strict actual-server backoff; this requirement refines the previously combined local refusal diagnostic.
- [FR-004](FR-004-cache-polling.md) owns compatible stale fallback and strict bounded reads.
- [FR-030](FR-030-incremental-linear-cache.md) owns dataset partition and successful refresh publication.
