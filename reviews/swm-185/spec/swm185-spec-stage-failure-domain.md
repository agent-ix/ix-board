---
id: SR-6091
title: "SWM-185 spec stage failure-domain review"
type: SpecReview
analysis: failure-domain
scope: "agent-ix/ix-board; SWM-185; spec/functional/FR-032-quota-admission-recovery.md, spec/functional/FR-031-shared-linear-quota.md, spec/usecase/US-010-read-within-linear-quota.md, spec/spec.md"
review_set: subset
---

## Summary

The selected failure-domain lens covers identity and ownership of observations/reservations, atomic admission, stale-outcome provenance, invalid evidence, concurrency, clock reversal, restart and migration. PASS: independent guards and charge ownership survive expiry, late completion and reopen; no callback or graph traversal extension is introduced.

Ticket: SWM-185. PR ix-board#82; frozen head 3339082d844f7cadc6a84e195cd9d278fdaa211c; base c816c18.

## Verdict

**PASS** — no defects found in this method's examined specification scope. Remote availability remains unknown; passing this review authorizes no quota bypass.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

## Examined scope

```json
[
  {
    "id": "FR-032-statement-1",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The shared quota gate SHALL evaluate admission and recovery using explicit typed\nstates, preserving the conservative accounting and actual-server backoff in\n[FR-031](FR-031-shared-linear-quota.md). The evaluator SHALL distinguish a local\nallocation hold from a server remaining-quota reserve hold, even when local usage\nis zero. The evaluator SHALL expire each server observation with its own evidence\nrather than attach an older remaining value to a later observation's reset."
  },
  {
    "id": "FR-032-statement-2",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The gate SHALL derive a decision for a specified request estimate and endpoint\nfrom one atomic view of persisted state, configuration, and current clock time.\nAdmission SHALL re-evaluate and create the durable reservation in the same shared\ntransaction; an earlier status result is explanatory evidence, not permission to\nsend. Status inspection SHALL perform no network request or credential lookup."
  },
  {
    "id": "FR-032-statement-3",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The decision SHALL expose a typed state with these meanings:"
  },
  {
    "id": "FR-032-statement-4",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The evaluator SHALL retain all applicable blockers with typed limiting dimensions\n(`requests`, `complexity`, `endpoint_requests`, or a nondimensional evidence/input\nfailure). The primary state SHALL prefer ServerBackoff, then EvidenceUnavailable,\nthen RequestRejected, then ServerReserveHeld, then LocalWindowHeld. Available SHALL\nhave no blockers. Each timed blocker SHALL carry its own absolute deadline; the\ndecision SHALL expose the earliest next reevaluation time rather than promise\nadmission at that time. An unknown transition SHALL remain unknown."
  },
  {
    "id": "FR-032-statement-5",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Each dimensional blocker SHALL expose the applicable capacity, reserve, estimated\nrequest charge, local usage, outstanding charge/debt, and effective remaining\nevidence where known, with explicit unknown values. Local-window deadlines and\ngenerations SHALL remain distinct from server observation deadlines and\ngenerations. The evaluator SHALL NOT infer remote availability from zero local\nusage or from expiry of one blocker."
  },
  {
    "id": "FR-032-statement-6",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "For each request, complexity, and endpoint dimension, the gate SHALL retain the\nremaining value together with its response/request provenance, observation time,\nreset ownership, and expiry. A reset from a different response SHALL NOT extend\nthe lifetime of an earlier remaining value. A reset-only or limit-only response\nSHALL NOT refresh an older remaining observation. A remaining-only response SHALL\nuse a fixed expiry measured from that observation, bounded by the configured\nfallback quota-window duration (one hour by default); subsequent unrelated\nresponses SHALL NOT slide that expiry."
  },
  {
    "id": "FR-032-statement-7",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Until its expiry, each accepted restrictive observation SHALL constrain admission\nconservatively with subsequent reservations and unsettled charges. A higher\nremaining value or later reset, by itself, SHALL NOT erase a live lower constraint\nor prove a new server generation. Independent valid observations MAY coexist;\ntheir effective constraint SHALL be at least as restrictive as every unexpired\napplicable observation. The persisted representation SHALL be bounded without\nextending the expiry of any retained restrictive constraint or losing its charge\nownership. This contract does not require s"
  },
  {
    "id": "FR-032-statement-8",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "When the owning deadline is reached, the gate SHALL retire only that expired\nconstraint. A newer valid observation SHALL retain its own remaining value and\ndeadline after the older constraint expires. The gate SHALL NOT carry the old\nminimum into that observation's later window. Out-of-order completion SHALL NOT\nresurrect expired evidence, move a local deadline, or refund a newer generation.\nEqual-time observations SHALL resolve conservatively and deterministically."
  },
  {
    "id": "FR-032-statement-9",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Missing or malformed headers SHALL NOT create positive server headroom. A parsed\nreset already at or before observation time SHALL NOT retain a remaining hold\npast that reset. Invalid arithmetic, corrupt ownership, or unusable provenance\nSHALL produce EvidenceUnavailable when conservative recovery is impossible.\nExpiry of valid observations SHALL yield unknown server headroom and reuse only\nthe existing configured fallback/local allocation; it SHALL NOT create a new\nlocal window, replenish usage, cancel outstanding charges, or assert a recovered\nserver limit. A clock moving backwards SHALL NOT"
  },
  {
    "id": "FR-032-statement-10",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "While an actual-server cooldown is active, the transport SHALL issue zero requests\nthrough every path, including force refresh and any recovery attempt. Independent\nobservations SHALL NOT shorten that cooldown. This change SHALL introduce no\nbackground retry, sleep loop, quota-reset operation, reserve reduction, or special\nprobe request. Recovery SHALL occur on the next ordinary protected operation or\nstatus evaluation after evidence expires."
  },
  {
    "id": "FR-032-statement-11",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The gate SHALL preserve durable single-use reservation ownership, actual-cost\nsettlement, pending charges, excess-cost debt, conservative abandoned-reservation\ncharges, replay rejection, wrong-store rejection, and separation of local and\nserver generations. Restart and migration SHALL NOT reset those obligations or\nconvert ambiguous legacy evidence into known headroom. Legacy unpaired evidence\nSHALL retain its existing restrictive deadline without extending it, or fail with\nEvidenceUnavailable when that deadline cannot be recovered safely."
  },
  {
    "id": "FR-032-statement-12",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The cached-read SDK SHALL expose a typed refresh outcome/accessor containing the\nquota decision captured when refresh was refused. It SHALL distinguish quota\nrefusal, transport/source failure, refresh-lock refusal, successful refresh, and\ncache reuse without classifying human-readable messages. A snapshot with absent\nlegacy outcome metadata SHALL expose an unknown outcome rather than infer one\nfrom its failure text. The outcome SHALL describe the read attempt; a later live\nstatus evaluation SHALL NOT rewrite that attempt's provenance."
  },
  {
    "id": "FR-032-statement-13",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "The SDK SHALL preserve existing read signatures and public struct-literal\nconstruction contracts, including CacheFreshness and existing report structs.\nNew typed information SHALL use an additive accessor or separately introduced\nenvelope that does not require callers to add fields or adopt a new constructor.\nExisting serialized fields SHALL retain their meanings; additive outcome data\nSHALL have explicit absent-field behavior for older stored/output data."
  },
  {
    "id": "FR-032-statement-14",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Ordinary stale fallback SHALL preserve the last complete compatible snapshot and\nits successful age under [FR-004](FR-004-cache-polling.md). Its typed outcome SHALL\nretain the actual quota refusal and transition information. The CLI SHALL render\na local-window hold as a local allocation hold and a server reserve hold as a\nlocally enforced request, complexity, or endpoint reserve hold, with the owning\ndeadline when known. Only actual ServerBackoff SHALL be described as Linear rate\nlimited or populate an active actual-server `rate_limit_until`. Generic network\nfailure text SHALL NOT replace an a"
  },
  {
    "id": "FR-032-statement-15",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "`cache status` SHALL expose the evaluated typed decision and dimensional evidence\nin JSON and equivalent labeled text, alongside existing counters. With local\nrequests and complexity both zero and observed request remaining below reserve,\nstatus SHALL identify ServerReserveHeld rather than LocalWindowHeld or actual\nserver cooldown. Status SHALL label the estimate/endpoint it evaluates so its\nAvailable result cannot be mistaken for eligibility of every possible query."
  },
  {
    "id": "FR-032-AC-1",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "With request remaining 250, request reserve 500, local requests/complexity zero, no cooldown, and a future paired reset, real persisted admission sends zero calls and returns ServerReserveHeld with requests dimension and the paired deadline. SDK fallback and cache-status JSON/text expose that reason; rate_limit_until has no active cooldown."
  },
  {
    "id": "FR-032-AC-2",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Fixture decisions independently exercise every state, simultaneous local/server/backoff holds, both capacity dimensions, endpoint constraints, unknown evidence, and an oversized single request. Primary-state precedence and every blocker/deadline match the contract; the earliest transition is not labeled a guarantee of admission."
  },
  {
    "id": "FR-032-AC-3",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "With a 501-request admission threshold, remaining 250 expiring at T1 followed by remaining 1000 expiring at T2 blocks before T1 and retires only the old constraint exactly at T1, without carrying 250 to T2. Replacing 1000 with 400 keeps the newer hold until T2. Reversing arrival order never bypasses a live constraint or resurrects it after expiry."
  },
  {
    "id": "FR-032-AC-4",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Reset-only, remaining-only, limit-only, missing, malformed, past-reset, equal-time, and out-of-order fixture headers never splice one response's low remaining onto another's later reset. Specifically, remaining 250 with reset T1 arriving after T1 never becomes a new fallback-hour hold. Repeated moving resets cannot extend a retained constraint; missing-reset evidence expires at its fixed fallback bound. Retired evidence is unknown, not invented positive remaining."
  },
  {
    "id": "FR-032-AC-5",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "A deterministic clock-driven sequence spans at least three local windows and three server observation expiries while requests, varying actual-cost settlements, and moving reset headers continue. Local usage resets only at fixed local deadlines; every admission satisfies all applicable live constraints; expired low evidence does not cause an indefinite hold, and local zero never proves remote availability."
  },
  {
    "id": "FR-032-AC-6",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Real SQLite reopen/crash-recovery and concurrent-gate fixtures preserve active holds, paired observation ownership, pending/debt charges and single-use reservations. Late settlement across each kind of generation boundary, duplicated settlement, wrong-store tokens, legacy unpaired rows, corrupt provenance, and backward clock movement cannot refund or admit incorrectly."
  },
  {
    "id": "FR-032-AC-7",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "Local HTTP fixtures return HTTP 429 and GraphQL RATELIMITED with retry metadata. Until the persisted latest retry-until, independently opened gates and every normal/direct/history/force path make exactly zero further calls even across local resets, newer high remaining observations, and restart. At expiry only a normal otherwise-eligible protected request is admitted; no automatic retry or probe runs."
  },
  {
    "id": "FR-032-AC-8",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "A WorkspaceSource/local-HTTP integration returns a complete compatible stale snapshot on each typed quota refusal, preserving source-fetch age and successful sync markers. A source error and lock refusal remain distinct outcomes; a bounded read returns its typed refusal without stale data; absent legacy outcome metadata is unknown without message parsing."
  },
  {
    "id": "FR-032-AC-9",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "An external consumer fixture compiles unchanged legacy public struct literals and existing read calls while another consumer obtains typed outcomes through the additive API. Changing human failure-message wording does not change either consumer's typed classification; old serialized metadata without the new outcome retains documented unknown behavior."
  },
  {
    "id": "FR-032-AC-10",
    "path": "spec/functional/FR-032-quota-admission-recovery.md",
    "role": "examined",
    "excerpt": "A deterministic sequence/model test drives production admission, persistence reopen, observations, reservations, and settlement using a controlled clock and real storage. Its independent oracle asserts the safety and bounded-expiry properties above after each event, records reproducible failing sequences, and includes generated or enumerated interleavings rather than merely echoing the production evaluator. All tests use fixture sources/local HTTP and make no real Linear API calls."
  },
  {
    "id": "FR-031-local-refusal",
    "path": "spec/functional/FR-031-shared-linear-quota.md",
    "role": "examined",
    "excerpt": "Local-window exhaustion,\nserver reserve holds, and actual-server cooldown SHALL have the distinct typed\nstates and truthful diagnostics defined by\n[FR-032](FR-032-quota-admission-recovery.md)."
  },
  {
    "id": "FR-031-AC-4",
    "path": "spec/functional/FR-031-shared-linear-quota.md",
    "role": "examined",
    "excerpt": "With fixture request/complexity budgets and two competing processes, aggregate sends never exceed either budget; every direct/history request is counted, unknown cost uses the documented estimate, quota reserve prevents the next send, and local refusal has its distinct typed diagnostic under [FR-032](FR-032-quota-admission-recovery.md)."
  },
  {
    "id": "US-010-FR-032",
    "path": "spec/usecase/US-010-read-within-linear-quota.md",
    "role": "examined",
    "excerpt": "- target: \"ix://agent-ix/ix-board/FR-032\"\n    type: traces_to"
  },
  {
    "id": "spec-architecture-SWM-185",
    "path": "spec/spec.md",
    "role": "examined",
    "excerpt": "SWM-185 adds [FR-032](./functional/FR-032-quota-admission-recovery.md) to\ndistinguish quota admission states, expire paired server observations, and expose\ntyped refresh outcomes without changing existing public struct construction."
  }
]
```

## Validation and limitations

No applicable AssuranceProfile exists under spec/. The selected direct subset review requires no enforced review run; no human acceptance or implementation gate is claimed. Quoin 0.36.7; quire 0.52.0 (engine 0.62.2); spec-artifacts-process@v0.29.2. Changed four-document validation passes with unchanged FR-031 advisory grammar warnings. Full-spec validation exits 3 with two failing documents: FR-017 lacks Dependencies; roadmap/ears-campaign-stories.md lacks type frontmatter. Both are unchanged from the base. Computed matrix lists FR-032-AC-1 through AC-10 untagged: expected pending implementation for this spec-only stage, not evidence of delivered behavior. Implementation must use the installed active imported #[trace] grammar. No builds, gates, real Linear API calls or review worktrees were used.

