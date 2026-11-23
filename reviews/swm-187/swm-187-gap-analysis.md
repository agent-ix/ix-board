---
id: SR-6098
title: "gap-analysis review of PR78 and SWM-187"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; spec/functional/FR-033-team-flow-effort-export.md; original exporter PR78"
review_set: subset
---

## Summary

FAIL: 1 finding(s) require disposition. Inspected frozen6ec90c446c97dbe595de4877227cbe1694e70baa against origin/main. Original featureSWM181, incident/fix recordSWM187. No production source edits or aggregate gate by reviewer.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The feature tests use Trace doc comments rejected by the active Rust trace grammar; every minted FR-033 criterion is untagged. | FR-033-AC-1; src/team_metrics/tests.rs:104-105 |

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

Plan completion: not assessed

The whole-repository quoin matrix --repo . --json ran successfully and found34 requirements. FR033 mints only8 criteria because its acceptance table ends after AC8; SR6101 records that independent structure defect. Every one of the8 minted criteria is untagged; new tests claim Trace doc comments, while the active Rust form requires #[trace]. Once the table is repaired, bind all13 feature criteria with recognized source markers and recompute. Inherited untagged criteria remain outside this bounded PR pass and are not certified clean.

Reverse mapping of changed production code: shared wire/history/checkpoint/sampling/rendering belongs to FR033; native activity reporting extends existing agent contracts; quota frontier monitoring belongs to FR033AC11 and reuses FR031's store; new GitHub admission belongs to FR033AC12/13. Existing Linear adapter remains FR030/031. No changed stub or perPR REST fallback found. Shared query selection is referenced from retro rather than copied into a second owner.

The40 passing offline focused tests cover the named normal and error paths; new edge failures are recorded in SR6097, including actual Collector public-API reproduction. No aggregate gate was run. Matrix static bindings are distinct from test execution and retained run evidence. Optional automated semantic assessment was not invoked.

Review custody path is .agent/reviews/swm-187; the lead must retain the six SR artifacts under reviews/ when landing. Exporter remains stopped; this review does not authorize restart.

## Dispositions

Round1 inspected frozen73c0c8d727f490cb80fa976f5b3a473d9359f627. Original findings and comments remain unchanged. Independent offline library tests:52 passed,2 subprocess fixtures ignored; fragmented HTTP bin test:1 passed. The independent matrix enumerates13/13 FR033 criteria tagged, and the spec validates. No live GitHub API, real credential read, production start or aggregate gate was run.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Feature tests now use declared Rust trace attributes. The independent computed matrix enumerates13 FR033 criteria and every one is tagged; inherited repository gaps remain outside this disposition. |

Latest verdict: PASS for this scoped method; no new findings.
