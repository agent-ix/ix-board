---
id: SR-6076
title: "code-review of SWM-178 named-focus SDK"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; SWM-178; src/, tests/, examples/export_schema.rs, README.md, docs/named-focus.md, docs/focus.md, spec/functional/FR-004/010/012/016 and FR-001/002/011 context"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
---

## Summary

Reviewed exact frozen HEAD 56e0e696d2c8acbbbf84ab9bdbce2627231e0e01 against accepted base 0cfeefcf57fc906dc7c6f1698eeee66faa15af2f, PR https://github.com/agent-ix/ix-board/pull/75. Ticket: SWM-178. No source/spec edits, builds, Cargo, live source/auth calls, credentials, child agents or worktrees were used. Exact model runtime ID is unavailable.

Code-review and the Rust lane ran in one pass over the changed public APIs, membership and display paths, bounded source/cache pipeline, schemas, docs and tests.

## Verdict

**FAIL** — late successful results and unknown-dependent membership violate the bounded and availability contracts; all findings require same-PR disposition.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | Cache-hit reads can return a successful snapshot after the operation deadline. fresh_within checks remaining time only before its SQL query; payload copying and JSON decoding have no deadline check, and the caller returns immediately. A short policy with a large cached snapshot can therefore expire during lookup/decode and still return Ok. Carry the deadline through this path, check expiry before success, and bound or cooperatively interrupt substantial decoding work; add a controlled-clock cache-hit expiry regression. | src/lib.rs:763-767 |
| FND-002 | high | named_batch_bounded drops the read deadline before selection validation and evaluation. After workspace_snapshot_bounded returns, catalog_evidence and evaluate_named_batch run without any remaining budget; ancestry, closure, readiness and per-selection traversal can run arbitrarily past D and return a publishable Ok batch. Retain one deadline from after credentials-ready and before cache lookup through validation/evaluation, with cooperative checks in expensive traversals and before success; do not restart the budget or add a detached timeout worker. Test cache-hit and near-deadline refresh cases. | src/lib.rs:781-787 |
| FND-003 | medium | Old-writer coexistence mistakes equal second-resolution fetched_at values for payload identity. After a bounded entry at timestamp T, an old writer can replace its payload in the same second T without touching the new evidence columns. started_for still equals fetched_at, so the bounded reader accepts a payload whose source-read start is unknown instead of refreshing it. Bind evidence to the actual payload (or invalidate it on every old-style update), and extend the old-writer regression to replace a different payload without changing fetched_at. | src/cache.rs:142-148 |
| FND-004 | high | Unavailable initiative links do not contaminate initiative-dependent membership or union availability. With initiative_links=false, an observed I-to-P1 association resolves I and produces a complete selection and complete team union, even though unestablished links on P2 can hide additional I leaves. The flag is consulted only for standalone display rows. Propagate unknown link membership for initiative includes and excludes into selection and union availability/provenance while retaining established leaves; add I/P1 plus unestablished P2-link cases and a same-team complete companion selection. | src/named_focus.rs:490-503 |

## Evidence and adverse cases

Root's actual focused results supersede the original coder's uncompiled statement: library 170 PASS; bounded_read 14 PASS after tempdir-lifetime fix; named_selection, named_focus, schema, scope, integration, credential_provider and cache_path PASS; clippy all-targets exit 0; fmt check exit 0; both schema exports exit 0. Evidence: /Users/peter/dev/.ix-board-lifecycle-delivery/swm178-focused-check-results.md and referenced logs. No final aggregate make ci, cargo deny, or unsafe gate has run in this review; root owns the one final gate after dispositions. These are inspected root results, not reviewer execution claims. No CI gate/config/dependency changes occur in the diff.

The consumer wording was read directly from accepted ix-relay commit 9a804538a0db533b8972d9b9aef908d9dd49619a, FR-061 lines 147–162 and FR-063 lines 161–170: one budget starts after credentials-ready and before cache lookup, read and selection validation; persistence follows successful validation. The composing named_batch_bounded API must supply this remaining-budget path. Findings above are traced source failures, not executed reproductions; no extra runtime check was requested because the missing checks and scope are explicit in the code.

## Coverage

Computed both quire matrix (0.52.0 engine 0.62.2) and quoin matrix (0.36.7): 273 criteria; tagged 84, untagged 180, method-without-symbol 9; all 273 have no bound run evidence. Root test logs are separate runtime evidence, not engine-bound evidence. All newly added test-kind ACs in FR-004/010/012/016 are tagged; FR-012-AC-20 is Inspection/method-without-symbol. Existing comment-only legacy trace gaps remain, including old ACs in these four FRs; this pass does not claim a repository-wide traceability PASS or require a legacy repair campaign. Engine diagnostics also report existing cfg symbol collisions and NFR heading/declaration warnings; no silent fallback was used.

Plan completion: not assessed

Semantic-review fanout: not invoked; no optional semantic campaign was authorized. Ordinary code/test intent checks needed for code review were performed. Spec review: not applicable to this implementation diff (zero spec/plan changes; accepted specification reviewed separately). React/Python/Tokio lanes: not applicable. No applicable AssuranceProfile was found in spec; no assurance exception was used.

Reverse-gap inventory: named document encode/decode/canonicalization maps to FR-016; closures, outcomes, team fences and dynamics map to FR-010; counts/placements/unions/blockers map to FR-012 and reused FR-001/002/011; cache/deadline/source capability maps to FR-004; schema exports map to FR-016. No new unowned behavior or live source/test stub was identified in those changed surfaces. Repository source/test stub scan found only existing documented benchmark/subprocess-helper ignores and fixture text; no source todo!/unimplemented! stub. No coverage-percentage claim was used. Full legacy source reverse-gap assurance remains outside detailed review coverage, so this is not a full-repository clean claim.

## Examined scope

```yaml
scope:
- id: FR-004-AC-1
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given an unexpired cache entry, repeated fixture reads use the cached value
    and return identical results without another source read.
- id: FR-004-AC-2
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given an expired entry and a changed fixture source, the next read refreshes
    and returns the changed value.
- id: FR-004-AC-3
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given an expired entry and a failed fixture refresh, the read returns an
    error and does not return the stale value.
- id: FR-004-AC-4
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given identical cache state and fixture source, repeated reads return identical
    results.
- id: FR-004-AC-5
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given an absolute `XDG_CACHE_HOME` on Unix with `HOME` unset or set, the
    CLI and `ReadService::open_default` both select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3`;
    given no absolute `XDG_CACHE_HOME` and a set `HOME`, both select `<HOME>/.cache/ix-board/linear.sqlite3`;
    given `IX_BOARD_CACHE_PATH`, both select that path instead.
- id: FR-004-AC-6
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a non-absolute or absent `XDG_CACHE_HOME`, the library falls back
    to the available home directory; given neither an absolute XDG cache directory
    nor a home directory, the library returns `CacheDirectoryUnavailable` and invents
    no path.
- id: FR-004-AC-7
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Project queries use a page size of 20 records.
- id: FR-004-AC-8
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Issue queries use a page size of 50 records.
- id: FR-004-AC-9
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: A structured RATELIMITED response produces a typed LinearRateLimited error.
- id: FR-004-AC-10
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: A rate-limit error carries the cooldown derived from exhausted quota resets.
- id: FR-004-AC-11
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Without a read policy, every existing read method keeps the 300-second
    entry lifetime, the 30-second per-request timeout, no deadline, and its existing
    typed errors; AC-1 through AC-10 pass unchanged and no existing method signature
    requires a new argument.
- id: FR-004-AC-12
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a bounded read with maximum age 240 seconds, an entry whose source
    read began 200 seconds earlier is returned with no source read, and an entry whose
    source read began 250 seconds earlier is refreshed; a maximum age of 301 seconds
    or more is refused as an invalid policy with no source read.
- id: FR-004-AC-13
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a snapshot whose source read began 250 seconds ago but whose cache
    write or report occurred 10 seconds ago, a bounded read with maximum age 240 seconds
    refreshes it; an entry with no evidence of when its source read began is refreshed
    rather than accepted; the returned fetch time is the actual source fetch time.
- id: FR-004-AC-14
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a fake source whose pages never complete, a bounded read with deadline
    D returns the typed timeout with an elapsed time of at most D plus a stated scheduler
    and clock-precision tolerance of 250 milliseconds, and never D plus a further
    legacy request timeout; with a controlled clock the elapsed time is exactly D;
    no page or nested read begins after D, and each request is issued with a timeout
    no greater than the time remaining, so a request begun with 5 seconds remaining
    carries a timeout of at most 5 seconds.
- id: FR-004-AC-15
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a source whose credential resolution takes longer than the deadline
    before it becomes ready, a bounded read still performs its full budgeted read
    after readiness; the bounded read never invokes credential resolution, never retries
    it, and returns an existing credential error unchanged when the source reports
    one.
- id: FR-004-AC-16
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: After a timeout or failed bounded read, the caller receives no snapshot
    and the cache holds no partial entry as fresh; a fake source that completes after
    expiry changes neither the cache nor any later result; the fake observes zero
    requests after the call returns and exactly one attempt per request issued, and
    the implementation spawns no detached worker.
- id: FR-004-AC-17
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: A source that cannot honor a bound returns the explicit unsupported-bound
    error before any source read; its legacy read method still works unchanged.
- id: FR-004-AC-18
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: A structured RATELIMITED response during a bounded read returns the existing
    typed rate-limit error with its cooldown, not a timeout, and the library does
    not retry or sleep; a deadline that expires first returns the timeout.
- id: FR-004-AC-19
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a cache that is locked for longer than the deadline, a bounded read
    returns the typed timeout within the deadline and does not read the source; a
    refreshed entry is written to the cache only before expiry.
- id: FR-010-AC-1
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Initiative, project and ticket selectors resolve from structured relationships;
    overlapping selectors count a ticket only once.
- id: FR-010-AC-2
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Progress excludes parents, canceled and duplicate leaves and groups remaining
    leaves by workflow stage.
- id: FR-010-AC-3
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Missing or ambiguous selectors, cyclic ancestry and missing ancestors return
    explicit errors.
- id: FR-010-AC-4
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Transport retains initiative identities, parent IDs and workflow categories;
    cached reads preserve actual fetch age and forced refresh replaces it.
- id: FR-010-AC-5
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: A team boundary selects only tickets owned by that team, including unprojected
    tickets when no narrower selectors exist; project, initiative and ticket selectors
    intersect that boundary, while leaf classification retains full snapshot ancestry.
    Unknown teams are refused.
- id: FR-010-AC-6
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Project and initiative membership follows assigned project IDs without
    expanding into descendants in other projects. Explicit ticket selection includes
    descendants across projects within the team boundary; global ancestry still prevents
    counting parents as leaves.
- id: FR-010-AC-7
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: A Linear initiative identifier resolves to the same project and issue set
    as its UUID or exact name; missing and ambiguous selectors still fail explicitly.
- id: FR-010-AC-8
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: A milestone UUID, unique name, or project-qualified name selects only its
    assigned issues, contributes its containing project for reporting, and unions
    without duplication with other selectors; slash characters inside either name
    resolve, missing names fail, and ambiguous names list project candidates.
- id: FR-010-AC-9
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Given a named selection including initiative I (projects P1 and P2) and
    excluding P2, the effective leaves are P1's; a P1 leaf excluded by ticket is absent;
    a P2 leaf also included by ticket is absent because the exclusion wins; removing
    the exclusions from the selection restores exactly those leaves and no others.
- id: FR-010-AC-10
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Excluding an initiative removes the leaves of its current projects; excluding
    a project removes its assigned leaves; excluding a milestone removes only its
    assigned leaves and keeps the rest of its project; excluding a ticket removes
    it and its descendants, including those in other projects; every exclusion is
    limited to the selection's team.
- id: FR-010-AC-11
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Given selections A (includes P) and B (includes P, excludes ticket T in
    P) of one team, A keeps T, B does not, and the combined set contains T once; given
    A (includes P1) and B (excludes P1), B's exclusion removes nothing from A.
- id: FR-010-AC-12
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Given a parent with two children, excluding one child leaves the parent
    a non-leaf and uncounted; excluding the parent while including both children counts
    neither child and never the parent, because the parent exclusion removes its descendants
    even when the same selection includes them; excluding only a child never promotes
    its parent to a leaf; a selected ticket whose descendants are all excluded is
    not turned into a leaf.
- id: FR-010-AC-13
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: A named selection with an empty include list returns the explicit-empty
    state with an empty issue set, project set, and leaf set, with and without exclusions
    and for a known team, and never the whole team; a request with zero selections
    returns the distinct no-active-selection state; a `ScopeSelection` with only a
    known team still selects the whole team and an empty inline `FocusSelection` is
    still rejected.
- id: FR-010-AC-14
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: Across two snapshots in which project P2 joins and then leaves initiative
    I, the same selection keeps unchanged bytes and its effective set follows each
    snapshot; the stored selection contains no issue IDs other than ticket members.
- id: FR-010-AC-15
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: A milestone member whose recorded project owns it selects its assigned
    issues; the same milestone ID recorded with another project reports `ProjectMismatch`
    and selects nothing; a same-named milestone in another project is not selected;
    string inputs `<project>/<milestone>`, a unique bare name, and names containing
    slashes canonicalize to the milestone ID with its owning project ID under the
    AC-8 grammar, and an ambiguous name still fails listing candidates.
- id: FR-010-AC-16
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: For include and exclude members that are resolved, proven absent by enumeration
    evidence, a ticket of another team, a milestone with the wrong project, and not
    provable, the outcomes are `Resolved`, `Missing`, `WrongTeam`, `ProjectMismatch`,
    and `Unavailable` respectively, in deterministic order, naming kind and identifier;
    a name, label, or ticket count never changes an outcome; a selection with any
    non-resolved outcome is reported incomplete with those outcomes.
- id: FR-010-AC-17
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: An initiative with no project association in the snapshot and no catalog
    evidence is `Unavailable`, not `Missing`; a team that appears in no project or
    issue gives an unavailable result rather than an empty or healthy one; an absent
    project, milestone, or ticket ID is `Missing` only when authoritative enumeration
    evidence covers that kind and visibility domain, and otherwise `Unavailable`,
    so an existing but archived or unseen project that the snapshot did not enumerate
    is `Unavailable` and never `Missing`, even when the snapshot is complete. Legacy
    `ScopeSelection` still refuses an unknown te
- id: FR-010-AC-18
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: An unresolved include member leaves its selection incomplete and contributes
    no leaves; an unresolved exclude member removes nothing, is reported, and leaves
    its selection incomplete; neither is silently ignored or turned into a zero result.
- id: FR-010-AC-19
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: A selection of team X that includes an initiative holding X and Y tasks
    counts only X's leaves and reports the team-boundary reason; a ticket member of
    team Y is `WrongTeam`.
- id: FR-010-AC-20
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: All existing FR-010 fixtures for `ScopeSelection` and `FocusSelection`
    (AC-1 through AC-8) return identical results and errors after the named selection
    exists.
- id: FR-012-AC-1
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given selected projects and milestones, `focus status` reports each group's
    target date, open issue counts grouped by workflow-state display name, and in-progress
    issues (`type=started`) in stable order, considering only resolved focus members
    assigned to the group. `Completed` and `Canceled` issues are not counted as open;
    blocker-only issues do not contribute unless they are also focus members.
- id: FR-012-AC-2
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given project and milestone issue trees with completed, open, canceled,
    duplicate, and parent issues, progress uses FR-010 leaf classification, reports
    done/total and percentage, and returns zero counts with a null percentage for
    an empty countable set.
- id: FR-012-AC-3
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given two focus issues with a shared transitive blocker and other blockers
    across projects and teams in open, completed, and canceled states, aggregate layers
    include the full FR-002 closure, each blocker appears once in its layer, and each
    blocker row reports its identifier, title, state, FR-001 classification, and sorted
    `focus_root_ids`. The shared blocker row lists both focus issues in identifier
    order; other blockers list only the focus issue whose closure contains them.
- id: FR-012-AC-4
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given focus issues with different project, spec, and blocker eligibility,
    the ready queue includes exactly the focus issues eligible under FR-001, applies
    FR-001 ordering, and includes its readiness reasons; blocker-only issues are excluded.
- id: FR-012-AC-5
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given identical complete Linear records, repeated `focus status` calls
    return identical project and milestone groups, issue lists, blocker layers, ready
    queue order, and reasons; the result reports the source snapshot's actual fetch
    timestamp.
- id: FR-012-AC-6
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: An empty focus, incomplete source snapshot, missing blocker record, or
    dependency cycle returns an explicit error and no partial status result.
- id: FR-012-AC-7
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Project focus issues without a milestone appear in `unmilestoned` with
    open-state, in-progress, and leaf-progress facts; parent issues contribute to
    open-state counts but not leaf progress, and terminal output explains the difference.
    The terminal fetch time is readable UTC with age while JSON retains `fetched_at`.
- id: FR-012-AC-8
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: A work-milestone selector keeps full project and milestone progress but
    restricts blocker roots and ready issues to matching milestones, preserving cross-milestone
    prerequisites; an unknown selector fails explicitly.
- id: FR-012-AC-9
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: An empty filtered ready queue reports that it is empty and counts otherwise-ready
    issues excluded by the selected milestones.
- id: FR-012-AC-10
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: A project reached only through milestone or issue focus is marked `partial_focus`,
    shows only selected or member-bearing milestones, and labels its project totals
    as selected-focus totals. The work-milestone view filter uses the same name, UUID,
    qualification, and ambiguity rules as saved milestone focus, but searches only
    focus projects so an out-of-focus name collision does not block a view.
- id: FR-012-AC-11
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given three named selections of two teams and one snapshot, one batch call
    returns each selection's result with its caller key, one combined result per team,
    and one snapshot fetch time; repeating the call on identical input returns identical
    order and values; a call on a changed snapshot returns results that all come from
    that one snapshot.
- id: FR-012-AC-12
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given a selection with open, completed, canceled, duplicate and parent
    issues, the per-selection done, total, remaining-by-stage, ready, and blocked
    counts equal the FR-010 leaf classification and the FR-001 and FR-002 results
    for the same leaves; two selections over the same project with different effective
    sets report their own counts.
- id: FR-012-AC-13
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given two selections of one team that share project P and one leaf placed
    under two initiatives, the combined result counts each leaf once, each per-selection
    row keeps its own count, P carries the shared mark in every placement with the
    same value for the same leaf scope, and no output field equals a sum of overlapping
    row totals.
- id: FR-012-AC-14
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given selections of teams X and Y, the results form separate groups and
    no total, union, or readiness spans teams.
- id: FR-012-AC-15
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Partial reasons are reported on initiative rows and project rows for a
    milestone-only inclusion, a ticket-only inclusion, an exclusion, and a team boundary
    that left other-team leaves out, each alone and together; a ticket-only selection
    whose project is not included lists its tickets as direct-ticket rows with the
    project partial and never becomes a project or team selection.
- id: FR-012-AC-16
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given an included leaf whose only open prerequisite is excluded by the
    selection, the leaf is not ready and is counted blocked, the prerequisite appears
    in the complete blocker closure and is not counted in the selection's totals;
    the same holds for an excluded prerequisite in another project or team; excluding
    a completed prerequisite changes no readiness.
- id: FR-012-AC-17
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: An explicit empty selection returns state `empty` with no rows and a valid
    zero leaf count; a batch with zero selections returns `none` with no totals at
    all; a selection with an unresolved or unavailable member or an unknown team returns
    `incomplete` or `unavailable` with counts marked unavailable or partial; the four
    results are distinguishable, and no result that depends on an unresolved, unavailable,
    or unknown fact is reported as complete, healthy, or zero.
- id: FR-012-AC-18
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: A project whose native status category is `Unknown` is reported with that
    classification and its row is not reported as complete or healthy; the existing
    status category derivation is reused and not changed.
- id: FR-012-AC-19
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: A parent cycle, a missing parent or blocker record, a dependency cycle,
    or an incomplete snapshot fails the whole batch with the existing explicit error
    and no partial result; a bounded-read timeout or rate limit returns its FR-004
    error and no result; the library stores no last-good result, revision cache, or
    named definition.
- id: FR-012-AC-20
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Inspection confirms the batch evaluation reuses the FR-010 resolvers and
    leaf classification, the FR-001 readiness evaluation, the FR-002 closure, and
    the existing status category, and adds no second calculator, store, service, or
    raw tracker read.
- id: FR-012-AC-21
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: Given a batch of team X with a `complete` selection A and a selection B
    whose exclude member is unresolved, team X's union is `incomplete`, is not reported
    complete or healthy, names B's key and cause, and marks its counts partial; A's
    own result stays `complete`; a team Y group with only complete selections stays
    `complete`; a team whose selections are all `empty` reports union `empty`; a `complete`
    selection A plus an `empty` selection B yields a `complete` union; an `incomplete`
    A plus an `empty` B yields an `incomplete` union (`unavailable` when no selection
    has a usable result).
- id: FR-012-AC-22
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: 'Given a selection that includes only project P directly and no initiative,
    the result contains a standalone project row for P with initiative-link status
    `none selected`; given a snapshot whose initiative links for P cannot be established,
    the row reports `unavailable` instead; the two are distinguishable, and in neither
    case is P''s row omitted or P''s leaves counted as a direct ticket row; given
    a selection that includes P and also excludes P, no standalone row for P appears;
    given a selection that directly includes an authoritatively known-empty P without
    excluding it, the standalone row for '
- id: FR-016-AC-1
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: A name-based selection produces canonical ID selectors, a version, a report
    timestamp, and the same status facts as `focus status` over that snapshot.
- id: FR-016-AC-2
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: "`FocusRecord` has an exportable structural JSON Schema."
- id: FR-016-AC-3
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: An empty or unresolved selection fails without a partial record.
- id: FR-016-AC-4
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: Version 2 encodes each focus member as `{kind,id}`; repeated reads of the
    same snapshot and selection produce the same ordered member list and issue set.
    Qualified milestone members select assigned issues even when names collide across
    projects.
- id: FR-016-AC-5
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: After the named selection exists, `focus report` for an inline selection
    still emits version 2 with the same members and bytes, an empty or unresolved
    inline selection still fails with no partial record, and the exported `FocusRecord`
    schema is unchanged.
- id: FR-016-AC-6
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: A version 3 named selection with team, include, exclude, and a milestone
    carrying `project_id` round-trips through encode and decode unchanged; the same
    selection built in any member order encodes to identical bytes in canonical order;
    a member present in both lists stays in both.
- id: FR-016-AC-7
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: 'A version 3 document with `include: []` decodes as the explicit empty
    selection; a document with `include` or `exclude` or `team` missing, a milestone
    without `project_id`, a non-milestone with `project_id`, a duplicate member in
    one list, or an unknown member is refused as an invalid document and is never
    read as empty or as a whole-team selection.'
- id: FR-016-AC-8
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: A version 2 `FocusRecord` presented to the named-selection decoder is refused
    as unsupported, naming versions 2 and 3, with no team, exclusion, or intent supplied
    and no conversion; versions 0, 1, and 4 are refused the same way; the existing
    version 2 `FocusRecord` deserialization behaves exactly as before the named selection
    existed, including for a version 3 document.
- id: FR-016-AC-9
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: An unsupported version is reported as a different typed error from an invalid
    document of version 3, and the version is checked before the other members, so
    a version 4 document with unknown members reports the version.
- id: FR-016-AC-10
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: The named selection has an exportable structural JSON Schema in which `team`,
    `include`, and `exclude` are required, a milestone member requires `project_id`,
    member `kind` is limited to exactly `initiative`, `project`, `milestone`, and
    `issue`, and unknown members are not allowed; a document with kind `ticket` or
    any other value is refused as an invalid document; the version 2 schema remains
    exported separately.
- id: FR-016-AC-11
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: Canonicalizing named string inputs yields canonical IDs and the milestone's
    owning project ID, and fails with no partial document for a missing, ambiguous,
    other-team, or project-mismatched member; the stored document contains no label,
    expanded issue ID, or status.
- id: FR-001
  path: spec/functional/FR-001-ready-queue.md
  role: context_only
  excerpt: |-
    The ix-board `next --team <T>` command SHALL return eligible, not-yet-active
    tickets for the requested team. A project is running only when its Linear state
    is `Started`; `Paused` projects and their tickets are excluded. Issue states
    `Completed` and `Canceled` are closed; all other issue states are open. For
    Linear workflow states, `type=completed` maps to `Completed` and
    `type=canceled` maps to `Canceled`, regardless of display name; other workflow
    types retain their display name. Thus a `Done` state with `type=completed` is
    closed, while a canceled prerequisite remains unresolved. The
    `Specc
- id: FR-002
  path: spec/functional/FR-002-project-chunk.md
  role: context_only
  excerpt: |-
    The ix-board `project-chunk <project>` command SHALL return the requested
    project's goal, milestones, open tickets, and blocker layers together in one
    command result. Issues in `Completed` or `Canceled` states are closed; issues in
    all other states are open. The result distinguishes each category and preserves
    relationships from structured Linear records.

    The ix-board `project-chunk <project>` command SHALL include in
    `blocker_layers` the full transitive `blocked_by` closure rooted at the requested
    project's open issues. The closure includes those open issues and every
    referenced prerequisite
- id: FR-011
  path: spec/functional/FR-011-inline-planner-focus.md
  role: context_only
  excerpt: |-
    When a planner supplies one or more initiative, project, or issue selectors,
    ix-board SHALL resolve them against the current structured Linear workspace
    snapshot. Focus selection SHALL use the selector membership,
    deduplication, and issue-descendant rules in FR-010. An initiative selects its
    assigned projects; a project selects all issues assigned to that project,
    including issues assigned to project milestones and issues with no milestone;
    an issue selector selects that issue and its descendants. The result SHALL
    preserve milestone associations and distinguish milestone-assigned issues from
    p
```

## Examined trace bindings

```yaml
bindings:
- test_id: tests/bounded_read.rs::legacy_reads_keep_the_five_minute_lifetime_and_need_no_policy
  ac_id: FR-004-AC-11
  trace: correct
- test_id: tests/bounded_read.rs::old_and_new_cache_entries_coexist_with_old_readers_and_writers
  ac_id: FR-004-AC-11
  trace: correct
- test_id: tests/bounded_read.rs::a_cache_created_before_the_evidence_columns_still_opens_and_serves_legacy_reads
  ac_id: FR-004-AC-11
  trace: correct
- test_id: tests/bounded_read.rs::cached_data_is_accepted_by_the_age_of_its_source_read_and_over_age_is_refreshed
  ac_id: FR-004-AC-12
  trace: correct
- test_id: tests/bounded_read.rs::age_is_measured_from_the_source_read_not_the_cache_write
  ac_id: FR-004-AC-13
  trace: correct
- test_id: tests/bounded_read.rs::old_and_new_cache_entries_coexist_with_old_readers_and_writers
  ac_id: FR-004-AC-13
  trace: correct
- test_id: tests/bounded_read.rs::requests_never_start_after_the_deadline_and_each_is_clamped_to_the_time_left
  ac_id: FR-004-AC-14
  trace: correct
- test_id: tests/bounded_read.rs::a_page_that_never_completes_times_out_within_the_deadline_with_no_work_left_behind
  ac_id: FR-004-AC-14
  trace: correct
- test_id: tests/bounded_read.rs::a_nested_relation_read_is_inside_the_same_deadline
  ac_id: FR-004-AC-14
  trace: correct
- test_id: tests/bounded_read.rs::the_budget_starts_after_credentials_are_ready_and_credentials_are_never_resolved_again
  ac_id: FR-004-AC-15
  trace: correct
- test_id: tests/bounded_read.rs::a_credential_failure_is_the_existing_error_and_nothing_is_read_or_retried
  ac_id: FR-004-AC-15
  trace: correct
- test_id: tests/bounded_read.rs::a_late_completion_changes_neither_the_cache_nor_a_later_result
  ac_id: FR-004-AC-16
  trace: correct
- test_id: tests/bounded_read.rs::a_page_that_never_completes_times_out_within_the_deadline_with_no_work_left_behind
  ac_id: FR-004-AC-16
  trace: correct
- test_id: tests/bounded_read.rs::a_source_that_cannot_honor_a_bound_fails_before_any_read_and_legacy_reads_still_work
  ac_id: FR-004-AC-17
  trace: correct
- test_id: tests/bounded_read.rs::a_rate_limit_is_the_typed_error_with_its_cooldown_and_is_never_retried
  ac_id: FR-004-AC-18
  trace: correct
- test_id: tests/bounded_read.rs::a_locked_cache_times_out_within_the_deadline_without_reading_the_source
  ac_id: FR-004-AC-19
  trace: correct
- test_id: tests/named_focus.rs::exclusions_remove_by_closure_and_win_over_inclusion_and_reversal_restores
  ac_id: FR-010-AC-9
  trace: correct
- test_id: tests/named_focus.rs::each_exclusion_kind_removes_its_own_closure_within_the_team
  ac_id: FR-010-AC-10
  trace: correct
- test_id: tests/named_focus.rs::exclusion_is_local_to_its_selection_and_same_team_selections_union_once
  ac_id: FR-010-AC-11
  trace: correct
- test_id: tests/named_focus.rs::parents_stay_non_leaves_whatever_children_are_excluded
  ac_id: FR-010-AC-12
  trace: correct
- test_id: tests/named_focus.rs::explicit_empty_selection_and_no_active_selection_are_distinct_states
  ac_id: FR-010-AC-13
  trace: correct
- test_id: tests/named_focus.rs::dynamic_membership_follows_each_snapshot_while_the_stored_bytes_stay_fixed
  ac_id: FR-010-AC-14
  trace: correct
- test_id: tests/named_focus.rs::member_outcomes_need_authoritative_evidence_to_be_missing
  ac_id: FR-010-AC-15
  trace: correct
- test_id: tests/named_selection.rs::names_canonicalize_to_ids_with_the_owning_project_and_failures_leave_no_document
  ac_id: FR-010-AC-15
  trace: correct
- test_id: tests/named_focus.rs::member_outcomes_need_authoritative_evidence_to_be_missing
  ac_id: FR-010-AC-16
  trace: correct
- test_id: tests/named_focus.rs::member_outcomes_need_authoritative_evidence_to_be_missing
  ac_id: FR-010-AC-17
  trace: correct
- test_id: tests/named_focus.rs::a_team_that_appears_nowhere_is_unavailable_not_empty_or_healthy
  ac_id: FR-010-AC-17
  trace: correct
- test_id: tests/named_focus.rs::member_outcomes_need_authoritative_evidence_to_be_missing
  ac_id: FR-010-AC-18
  trace: correct
- test_id: tests/named_focus.rs::the_team_boundary_counts_only_the_selected_team_and_says_so
  ac_id: FR-010-AC-19
  trace: correct
- test_id: tests/named_focus.rs::legacy_scope_selection_is_unchanged_beside_the_named_selection
  ac_id: FR-010-AC-20
  trace: correct
- test_id: tests/named_focus.rs::one_batch_is_one_snapshot_ordered_by_key_with_teams_kept_separate
  ac_id: FR-012-AC-11
  trace: correct
- test_id: tests/schema.rs::named_focus_wire_types_have_structural_json_schemas
  ac_id: FR-012-AC-11
  trace: correct
- test_id: tests/named_focus.rs::counts_equal_the_existing_leaf_readiness_and_blocker_results
  ac_id: FR-012-AC-12
  trace: correct
- test_id: tests/named_focus.rs::shared_rows_keep_their_own_counts_and_no_total_is_a_sum_of_overlaps
  ac_id: FR-012-AC-13
  trace: correct
- test_id: tests/named_focus.rs::one_batch_is_one_snapshot_ordered_by_key_with_teams_kept_separate
  ac_id: FR-012-AC-14
  trace: correct
- test_id: tests/named_focus.rs::the_team_boundary_counts_only_the_selected_team_and_says_so
  ac_id: FR-012-AC-15
  trace: correct
- test_id: tests/named_focus.rs::partial_reasons_are_reported_alone_and_together_with_direct_ticket_rows
  ac_id: FR-012-AC-15
  trace: correct
- test_id: tests/named_focus.rs::an_excluded_prerequisite_blocks_and_stays_in_the_closure_without_being_counted
  ac_id: FR-012-AC-16
  trace: correct
- test_id: tests/named_focus.rs::explicit_empty_selection_and_no_active_selection_are_distinct_states
  ac_id: FR-012-AC-17
  trace: correct
- test_id: tests/named_focus.rs::an_unknown_status_category_is_kept_and_never_reported_healthy
  ac_id: FR-012-AC-18
  trace: correct
- test_id: tests/named_focus.rs::snapshot_errors_fail_the_whole_batch_with_the_existing_error
  ac_id: FR-012-AC-19
  trace: correct
- test_id: tests/named_focus.rs::a_team_union_is_only_as_known_as_its_selections
  ac_id: FR-012-AC-21
  trace: correct
- test_id: tests/named_focus.rs::standalone_projects_report_a_link_status_and_honor_exclusions
  ac_id: FR-012-AC-22
  trace: correct
- test_id: tests/named_selection.rs::the_named_selection_schema_is_strict_and_the_version_two_schema_is_unchanged
  ac_id: FR-016-AC-2
  trace: correct
- test_id: tests/schema.rs::named_focus_wire_types_have_structural_json_schemas
  ac_id: FR-016-AC-2
  trace: correct
- test_id: tests/named_selection.rs::the_named_selection_schema_is_strict_and_the_version_two_schema_is_unchanged
  ac_id: FR-016-AC-5
  trace: correct
- test_id: tests/named_selection.rs::a_named_selection_round_trips_and_encodes_canonically_in_any_member_order
  ac_id: FR-016-AC-6
  trace: correct
- test_id: tests/named_selection.rs::explicit_empty_decodes_but_malformed_documents_are_invalid_never_empty
  ac_id: FR-016-AC-7
  trace: correct
- test_id: tests/named_selection.rs::other_versions_are_unsupported_and_checked_before_any_other_member
  ac_id: FR-016-AC-8
  trace: correct
- test_id: tests/named_selection.rs::other_versions_are_unsupported_and_checked_before_any_other_member
  ac_id: FR-016-AC-9
  trace: correct
- test_id: tests/named_selection.rs::the_named_selection_schema_is_strict_and_the_version_two_schema_is_unchanged
  ac_id: FR-016-AC-10
  trace: correct
- test_id: tests/named_selection.rs::names_canonicalize_to_ids_with_the_owning_project_and_failures_leave_no_document
  ac_id: FR-016-AC-11
  trace: correct
```

## Dispositions

Round 1 reviewed exact HEAD 61e7750db3aa7b0fce15870af052c5f45abcbfd0 against the original findings. Original rows above remain unchanged. Current disposition verdict: **FAIL**, because FND-002 remains open. No new finding was added.

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | Cache-hit lookup now checks expiry after copying, decode_within checks every 16 KiB and before/after decoding, and read_within checks before success. The new cache-hit regression fails on RED and passes GREEN. |
| FND-002 | still-open | One shared Deadline and final refusal are now present, and ancestry/readiness/blocker loops cooperate. However initiative_rows (src/named_focus.rs:959-997) and project_row/project_leaves/counts still have no budget checks. For an initiative with P projects and L leaves, initiative_rows invokes project_leaves over all L leaves for each project twice, yielding O(P*L) map lookups between the check at summarize entry and the later blocker-layer check. This substantial traversal can exceed D plus the stated 250 ms tolerance before returning the timeout; final refusal alone does not bound it. Thread the existing Budget through these row/count/shared-placement loops (or index them to a bounded traversal) and check during the work. The current 60-issue single-project clock-step test can expire in lookup/decode/context construction, so it does not demonstrate interruption during row construction. |
| FND-003 | fixed | read_started_digest binds evidence to SHA-256 of exact payload bytes; missing or mismatched evidence refreshes. The same-timestamp/different-payload old-writer regression fails RED and passes GREEN, with old readers retaining original undecorated payloads. |
| FND-004 | fixed | Resolved initiative include/exclude members now detect unestablished links on other team-bearing projects, downgrade complete to incomplete, retain known leaves, report InitiativeLinks and contaminate union availability/causes. Include/exclude/companion/established-link adverse assertions fail RED and pass GREEN. |

### Round 1 evidence

Root actual evidence in swm178-review-fix-check-results.md supersedes worker no-compile claims: RED 5b6516a has exactly four expected new assertion failures and no compile/fixture failures; GREEN 0732160 has library170/bounded17/named20/selection5/schema3 plus selected legacy suites passing. Final 61e7750 changes only a private ListEvaluation return type and three mechanical formatter changes; root clippy all-targets and fmt exit 0. Reviewer ran no Cargo/runtime checks. One final aggregate remains root-owned after all dispositions clear.

The no-after-expiry-Ok property is repaired by the shared final check. The remaining FND-002 issue is cooperative interruption and elapsed bound during expensive placement work. This is the original deadline finding, not a new review campaign. Accepted consumer budget still starts after credentials-ready and before cache/read/validation; no auth or credential behavior changed.

### Round 2 disposition

Reviewed exact frozen HEAD 7b6841d09b12f069d52fdb43d64ba5f9e1a03643, PR75, confined to FND-002's placement fix. Current code/Rust disposition verdict: **PASS**. No open code-review findings remain; the other three code findings retain their round 1 fixed outcomes. Original findings and all prior dispositions remain unchanged. No new findings.

| FND | outcome | reason |
| --- | --- | --- |
| FND-002 | fixed | The existing Budget is now checked inside initiative/project placement, each project_leaves scan, leaf counts, partial-reason scans, standalone/ticket rows, initiative-link uncertainty, team unions, and both shared-placement passes. Result propagation reaches the same named_batch_bounded deadline and existing final refusal; no second budget, worker or engine. The public-service same-300-leaf one-versus-thirty-project regression isolates placement cost for cold reads and cache hits, chooses an intermediate deadline, requires the one-project case to succeed, the many-project case to time out, and limits controlled overshoot to 100 steps. Root RED e8a8543 fails the placement-dominance assertion at 361.7 ms versus 361.7 ms, and exact GREEN 7b6841d passes the new test and focused suites. |

### Round 2 evidence and limits

Source inspection verified checks inside the previously unchecked O(projects*leaves) project_leaves traversal and callers, counts/reasons, and mark_shared. Ordinary sort/library operations and linear helper calls remain synchronous; this disposition establishes the requested cooperative placement interruption rather than a hard-real-time guarantee over arbitrary OS or allocator pauses. No new changed-path issue was found.

Root actual placement-fix-check-results.md: exact-head fmt0/clippy-all-targets0 and focused tests0, including library170/bounded18/named20/selection5/schema3/scope6/integration30 plus one ignored/credential5/cache_path platform0. Root verified the RED assertion failure without compiler or fixture errors. Reviewer ran no Cargo/runtime/auth/source command. Final aggregate remains root-owned after artifact custody; review PASS is not a final-gate or merge execution claim.
