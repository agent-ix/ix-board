---
id: SR-6101
title: "spec-review/spec-integrity-analysis review of PR78 and SWM-187"
type: SpecReview
analysis: integrity
scope: "agent-ix/ix-board; spec/functional/FR-033-team-flow-effort-export.md; original exporter PR78"
review_set: subset
---

## Summary

FAIL: 1 finding(s) require disposition. Inspected frozen6ec90c446c97dbe595de4877227cbe1694e70baa against origin/main. Original featureSWM181, incident/fix recordSWM187. No production source edits or aggregate gate by reviewer.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | A blank line terminates the acceptance table after AC-8, so the computed schema/matrix never mint AC-9 through AC-13, including the provider safety criteria. | FR-033; spec/functional/FR-033-team-flow-effort-export.md:56-62 |

## Scope

- FR-033 (examined), spec/functional/FR-033-team-flow-effort-export.md: When operational scopes are configured, ix-board SHALL export cached flow and effort measurements using the existing retro calculations and native work history.
- FR-033-AC-1 (examined), spec/functional/FR-033-team-flow-effort-export.md: Short stage transitions and removals survive replay; duplicate event IDs do not duplicate durations or cursor advancement, and restart restores the atomic checkpoint.
- FR-033-AC-2 (examined), spec/functional/FR-033-team-flow-effort-export.md: Assignment reports alone do not count execution. Consecutive verified unique lifecycle observations count execution; gaps, missing attribution and replacement sessions preserve unknowns. A pause does not remove real observed effort.
- FR-033-AC-3 (examined), spec/functional/FR-033-team-flow-effort-export.md: Source outages do not advance assignment accounting beyond the confirmed watermark; a delayed earlier completion remains consumable after recovery. Repeated upserts replace worker identity without resetting a known stage start.
- FR-033-AC-4 (examined), spec/functional/FR-033-team-flow-effort-export.md: Size and time retention prune evidence using UTF-8 bytes before exceeding the SQLite ceiling; unsupported schemas and a requested ceiling below allocated pages are refused without rewriting history.
- FR-033-AC-5 (examined), spec/functional/FR-033-team-flow-effort-export.md: Missing/stale values and current ages are omitted; source last-success timestamps survive failed attempts; window counts remain gauges and native session/ticket identities never become metric labels.
- FR-033-AC-6 (examined), spec/functional/FR-033-team-flow-effort-export.md: Added, removed, canceled, split and completed populations remain separate; changing selectors invalidates old comparable effort points before another ratio is emitted.
- FR-033-AC-7 (examined), spec/functional/FR-033-team-flow-effort-export.md: Bootstrap snapshots establish no historical stage start; unreported workers and partial sources remain visible as attribution/data gaps.
- FR-033-AC-8 (examined), spec/functional/FR-033-team-flow-effort-export.md: HTTP reads a bounded request line across packet fragments and serves its cached response without querying external sources.
- FR-033-AC-9 (examined), spec/functional/FR-033-team-flow-effort-export.md: PR closures distinguish merged and closed-without-merge cohorts; current changes-requested share has a decisive-review denominator, empty denominators are unavailable, and reopen/size measures expose their counting populations.
- FR-033-AC-10 (examined), spec/functional/FR-033-team-flow-effort-export.md: Native structured review records deduplicate by comment/artifact identity, latest dispositions distinguish confirmed, rejected and pending findings, malformed or missing evidence remains unavailable, and production regressions are not inferred.
- FR-033-AC-11 (examined), spec/functional/FR-033-team-flow-effort-export.md: A read-only local quota frontier gauge counts unexpired observations without creating or changing the quota database or sending API traffic; missing, stale or future observations omit the count and report unavailable monitoring.
- FR-033-AC-12 (examined), spec/functional/FR-033-team-flow-effort-export.md: Every exporter GitHub read uses shared persistent admission and response caching. A rolling-hour hard ceiling of 40 sends and 1,000 pessimistically reserved GraphQL points survives concurrent workers and restarts. Per-PR REST polling and cache-miss fallbacks are prohibited; historical links use batches of at most 25, with at most 50 older links per repository per collection. Server reset/Retry-After cooldowns survive nonzero command exits and restart; cooldown or exhausted budget admits zero sends. Unsupported expensive department and commit/check evidence remains explicitly unavailable.
- FR-033-AC-13 (examined), spec/functional/FR-033-team-flow-effort-export.md: Repository state uses at most four 50-row pages per open/recent cohort; incomplete, malformed, absent or stale evidence cannot certify an empty, clean or unmerged population. Cache hits retain source observation times, conditional 304 responses require an existing valid representation, and stale values are omitted after 15 minutes. Query/output/time and shared SQLite storage limits are enforced: 16 KiB requests, 1 MiB response bodies, 20-second transport deadlines and a 45-second collection admission deadline; response storage is at most one eighth of the journal ceiling capped at 4 MiB and 12

## Evidence and verdict

FR033's feature core is coherent and independently observable; current identity is distinct from session history, and pause does not imply observed inactivity. New resource criteria specify40 sends/1000 reserved points, bounded pages/bytes/deadlines/cache and unknown data. However the blank line at57 ends the only acceptance table; the following five pipe rows lack a new header and disappear from the computed spec. quire validation reported0/8 rather than13 criteria; the computed matrix contains onlyAC1..8. This silently excludes the most important incident safety criteria from traceability. Keep all13 rows in a single contiguous table and verify exact enumeration. No requirement rewrite or new assurance layer is required.

The existing profile inventory found no AssuranceProfile. Dependencies point to existing coordinated Linear/retro contracts and the external accepted relay wire owner. This pass does not demand new stories or stakeholder artifacts for a bounded operational change.

Review custody path is .agent/reviews/swm-187; the lead must retain the six SR artifacts under reviews/ when landing. Exporter remains stopped; this review does not authorize restart.

## Dispositions

Round1 inspected frozen73c0c8d727f490cb80fa976f5b3a473d9359f627. Original findings and comments remain unchanged. Independent offline library tests:52 passed,2 subprocess fixtures ignored; fragmented HTTP bin test:1 passed. The independent matrix enumerates13/13 FR033 criteria tagged, and the spec validates. No live GitHub API, real credential read, production start or aggregate gate was run.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The blank separator afterAC8 was removed, so all13 acceptance rows form one table and are minted by the matrix. Independent spec validation reports13 criteria. |

Latest verdict: PASS for this scoped method; no new findings.
