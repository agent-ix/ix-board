---
id: SR-6097
title: "code-review review of PR78 and SWM-187"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; spec/functional/FR-033-team-flow-effort-export.md; original exporter PR78"
review_set: subset
---

## Summary

FAIL: 5 finding(s) require disposition. Inspected frozen6ec90c446c97dbe595de4877227cbe1694e70baa against origin/main. Original featureSWM181, incident/fix recordSWM187. No production source edits or aggregate gate by reviewer.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | A valid empty history page that advances to the relay retention floor is rejected, so a new or expired stream cannot establish healthy history. | FR-033-AC-1; src/team_metrics/collect.rs:93-94 |
| FND-002 | medium | Admission debits and Retry-After cooldowns use the collection-start clock for later requests, so the claimed rolling-hour and response-relative cooldown boundaries expire early. | FR-033-AC-12; src/team_metrics/github.rs:199-240,458 |
| FND-003 | medium | Missing historical mergedAt is decoded as None and returned as merged=false instead of unavailable; inconsistent listed PR state and merge timestamps are also accepted. | FR-033-AC-13; src/team_metrics/github.rs:608-644 |
| FND-004 | medium | Persistent response keys omit credential/account identity, so a changed or missing credential reuses the previous principal's fresh cached repository evidence. | FR-033-AC-13; src/team_metrics/github.rs:153-155,176-194 |
| FND-005 | medium | The hard request ceiling counts gh processes while gh follows HTTP redirects outside admission, so one debit can cause multiple HTTP sends. | FR-033-AC-12; src/team_metrics/github.rs:43-48,214-230 |

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

The full original PR diff contains26 files and5468 insertions. This pass rechecked the new provider and its integration with the previously reviewed exporter: all runtime GithubSource callbacks, retro attribution, new source provenance, subprocess runner, HTTP cache path, journal interaction, execution identity and scope boundaries. No applicable AssuranceProfile exists under spec/. Accepted Linear quota/cache code remains its owner; no exporter callback delegates to LiveGithub. Unsupported department enrichment and main commit/check reads return unavailable without sends.

FND001 was reproduced with the actual public Collector::history API in the offline review fixture series-repro.rs. A version1 empty page with observed_at_ms700000000, available_from_ms95200000, next_cursor95200000-0 and has_more=false returns Err(Invalid("history cursor did not match page")). Relay's retained-floor empty cursor is legitimate, including at cold start. Accept and checkpoint verified empty advancement while retaining strict event cursor/order checks; also ensure failures cannot retain healthy evidence.

FND002: SelectedGithub::read always passes self.now, captured at collection construction, into Guard::request. All later debits share that earlier instant; old debits can leave a rolling hour before the actual last HTTP send is one hour old. For a long Retry-After, the delay is added to the pre-request instant instead of receipt time. Sample actual admission/receipt times through an injectable clock, preserve debit-before-send, and fail closed for backward/future persisted clocks. Test an hour boundary with delayed transport and two workers, plus a long response-relative Retry-After.

FND003: RawInfo.merged_at is Option<String>. Serde permits an absent mergedAt as None, so a batch field {"updatedAt":"2026-11-22T00:00:00Z"} parses into PullRequestInfo{merged:false}. Native mergedAt is nullable, but absent does not mean unmerged. Require field presence separately from explicit null. The listing path also accepts state MERGED with mergedAt=null and derives merged=false from that null; validate state/merge/close consistency and future/native timestamp bounds before certification.

FND004: The digest includes only request.args. Warm-cache lookup occurs without resolving the effective local credential; switching gh accounts/tokens, removing GH_TOKEN, or a restarted worker with a different active principal can reuse a private repository body from the former principal. Namespace successful representations by a redacted opaque effective credential/principal identity and provider identity. Resolve credentials inside the binary using its proper local provider, never exposing token bytes through logs, commands issued by the assistant, files or telemetry. Keep shared request accounting conservative across any namespace change.

FND005: guard inserts one send before Gh::send spawns gh api. The installed gh2.92.0 HTTP factory leaves CheckRedirect unset; its API helper sends via the ordinary client. The underlying Go HTTP client follows redirects by default, up to ten consecutive requests. Thus a redirect on the40th admitted invocation may send a41st HTTP request with no new debit. Disable implicit redirects/retries or pessimistically account for a proved maximum at the actual HTTP boundary. Use offline redirect fixtures; do not test against GitHub.

Primary-source evidence for FND005: [gh2.92 HTTP factory](https://github.com/cli/cli/blob/v2.92.0/api/http_client.go), [gh2.92 API HTTP helper](https://github.com/cli/cli/blob/v2.92.0/pkg/cmd/api/http.go), [Go Client redirect policy](https://pkg.go.dev/net/http#Client). The inference is from the factory's nil policy plus the documented default, not a live API experiment. No undocumented automatic retry behavior is asserted.

Independent focused offline suite:40 passed,2 subprocess fixtures ignored. Those fixtures are invoked by wrapper tests. Cold6000-link batching, warm/restart cache reuse, TTL re-fetch, shared lease, cooldown, bounds and unsupported callbacks pass. Existing budget tests count Fake::send invocations; they cannot establish an HTTP redirect bound. Four valid scopes with32 lead hosts each and populated scalar/stage/PR/review measurements rendered802 series in the offline review fixture, below1000; the hard renderer ceiling remains.

No live GitHub API/rate-limit/auth probes, token reads, production starts or installations were run. Public primary-source web pages were read to verify CLI transport guarantees. GraphQL metadata only confirmed the frozen head and textual mergeability. Local merge-tree is conflict-free, but these findings make the candidate not acceptable for restart or merge.

Review custody path is .agent/reviews/swm-187; the lead must retain the six SR artifacts under reviews/ when landing. Exporter remains stopped; this review does not authorize restart.

## New findings (disposition pass 1)

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-006 | high | The direct HTTP transport sends owner/name/after at the JSON root instead of GraphQL variables, so all native repository queries fail. | FR-033; src/team_metrics/github.rs:351-360; src/team_metrics/github_http.rs:94-97 |

graph_request retains the old gh CLI field shape: query, owner, name and optional after in one map. gh api used to group every non-query field into the GraphQL variables object. The replacement calls builder.json(&request.fields), so the wire body is {"query":"query($owner:String!,...) ...","owner":"agent-ix","name":"repo"} rather than {"query":"...","variables":{"owner":"agent-ix","name":"repo"}}. Native GraphQL therefore sees no values for required owner/name and returns an error. The fake GraphQL handlers inspect the in-process request fields and return canned success; loopback tests currently verify attempt counts/auth/headers, but never require the production request variable envelope.

Fix the actual typed outgoing GraphQL body, including optional pagination cursor; enforce16KiB on the final serialized wire body. Add a loopback fixture which decodes the HTTP JSON and rejects misplaced/missing variables, then covers both the listing and historical batch query paths plus paginated after. This requires no live GitHub traffic. The old gh API primary-source helper explicitly performs groupGraphQLVariables before marshaling: https://github.com/cli/cli/blob/v2.92.0/pkg/cmd/api/http.go .

## Dispositions

Round1 inspected frozen73c0c8d727f490cb80fa976f5b3a473d9359f627. Original findings and comments remain unchanged. Independent offline library tests:52 passed,2 subprocess fixtures ignored; fragmented HTTP bin test:1 passed. The independent matrix enumerates13/13 FR033 criteria tagged, and the spec validates. No live GitHub API, real credential read, production start or aggregate gate was run.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Collector verifies and checkpoints valid empty-floor advancement, retains strict invalid-page checks, and marks failures unhealthy; restart/next-tick regressions pass. |
| FND-002 | fixed | Guard samples after acquiring admission and again at receipt; persisted high-water clock rejects backward time. Delayed rolling-hour, two-worker lease and long Retry-After regressions pass. |
| FND-003 | fixed | Required nullable merge/close fields have explicit deserializers; native state and timestamp consistency are validated for listed and batched rows. Missing/null/inconsistent/future fixtures pass. |
| FND-004 | fixed | Credential resolution stays inside the binary, and the response namespace binds endpoint and effective credential. Loopback principal-switch/restart tests isolate bodies while sharing accounting; no real credentials were accessed by this reviewer. |
| FND-005 | fixed | Owned reqwest transport disables redirects and retries. Real loopback tests count wire attempts, including the40th admitted redirect, and verify one attempt per debit. |
| FND-006 | still-open | The new direct POST serializes Request.fields with query/owner/name/after at the root; required GraphQL variables are absent from the standard variables object. No fix exists at73c0c8d. |

Latest verdict: CHANGES REQUESTED; FND006 must be fixed before merge or restart.

### Round2

Inspected frozen92a9a7ae2e9fcf84d492f8b4d8760ddb1c1ee08d. Original findings and the round1 still-open row remain unchanged; the latest row below supersedes that outcome.

| FND | outcome | reason |
| --- | --- | --- |
| FND-006 | fixed | Request now owns typed query and variables fields. The actual POST body uses its bounded serialized JSON bytes, including owner/name and optional after under variables. Production-transport loopback assertions cover listing, pagination and historical batches, plus exact16KiB and escaping boundaries. |

Independent offline focused suite:54 passed,2 subprocess fixtures ignored. The new loopback server decodes actual HTTP bytes and requires exactly query/variables at the JSON root, checks native owner/name and paginated after, and observes two open pages, one recent page and one historical batch. Oversized serialized envelopes or escaped queries produce zero extra sends. Transport and admission use the same deterministic encoder over immutable Request values; bytes passed to reqwest are the checked representation. No real GitHub API calls, credential reads or aggregate gate were run.

All code findings now have latest fixed outcomes, including FND006. The gap and integrity artifacts retain their round1 fixed outcomes; all seven original findings and the subsequent transport regression are resolved. No new findings in this round. Local merge-tree against origin/main is conflict-free and yielded treebe307c699b8a12a64661e78808a36a4ee9c7ca05. Latest verdict: PASS/MERGEABLE subject to the lead's final aggregate gate, final dependency pin review and exact-head merge. Service restart remains the lead's controlled deployment step.
