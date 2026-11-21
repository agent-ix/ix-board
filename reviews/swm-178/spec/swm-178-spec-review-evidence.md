---
id: SR-6071
title: "spec-review/evidence"
type: SpecReview
analysis: evidence
scope: "agent-ix/ix-board; SWM-178; spec/functional/FR-004, FR-010, FR-012, FR-016"
review_set: subset
---

## Summary

Ticket: SWM-178. Reviewed head 5e4b4f4af986a0c96b2cdbc4cd35e79e687cf07a; base d6e28f40ddb71b05e0cb3e9607e799c6399aabc6. Accepted consumer contract relay 0d62c93c916ee2eca4b59ed2665a78145f66746c, FR-061..064. Only the existing four FR diffs were reviewed, with read-only source context. No applicable AssuranceProfile exists under spec/, and no enforced human acceptance is claimed. Direct proportional review set was authorized by the dispatch. No runtime build, live ix-board/source/credential call, auth change, source/spec edit or second worktree. Exact runtime model identifier is unavailable from the harness; model=unavailable records this provenance limitation.

quoin advise ran (deterministic output saved as swm178-advice.json): 54 recognized obligations; legacy FR-004-AC-3 inconclusive and FR-012-AC-20 Inspection-vs-Test advisory mismatch. Reviewer judgement: Test is appropriate for stale-on-failure; Inspection is appropriate for architectural reuse/no second calculator. No method change is required merely by the heuristic. Sixteen FR-010 criteria invisible to the advisor were manually reviewed and retain behavioral Test; base FND-001 requires restoring their engine visibility. Future fixture tests should cover overlaps/reversal, qualification, unknown catalog, excluded blockers, budget/cancel/cache waits and schema rejection; no implemented evidence is claimed.

## Verdict

**PASS** — 0 defect(s) in this method; this is a specification review, not an implementation acceptance.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |

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
  excerpt: The CLI and library `ReadService::open_default` SHALL use the same platform
    cache location, and `IX_BOARD_CACHE_PATH` SHALL override that location. On Unix,
    an absolute `XDG_CACHE_HOME` SHALL select `<XDG_CACHE_HOME>/ix-board/linear.sqlite3`
    even when `HOME` is unset; otherwise `HOME` SHALL select `<HOME>/.cache/ix-board/linear.sqlite3`.
- id: FR-004-AC-6
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: Given a non-absolute or absent `XDG_CACHE_HOME`, ix-board SHALL fall back
    to the available home directory; if neither an absolute XDG cache directory nor
    a home directory is available, the library SHALL return `CacheDirectoryUnavailable`
    rather than inventing a path.
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
    D returns the typed timeout with no more than D plus one legacy request timeout
    elapsed; no page or nested read begins after D, and each request is issued with
    a timeout no greater than the time remaining.
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
- id: FR-004-description-1
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    The ix-board read library SHALL use SQLite to cache structured Linear read data.
    Each cache entry expires five minutes after it is fetched. A read of an expired
    entry SHALL refresh it from Linear before returning. A refresh failure SHALL
    return an error. Stale cache data SHALL NOT be returned after a failed refresh.
    A successful refresh SHALL replace the expired value.
- id: FR-004-bounded-reads-opt-in-1
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    The library SHALL additionally offer a **bounded read** that a caller opts into
    with an explicit read policy: a maximum cache age and an operation-wide
    deadline. The policy is a parameter of the new operation only. Every existing
    read method keeps its legacy behavior: the five-minute entry lifetime, the
    30-second per-request transport timeout, the existing typed errors, and no
    deadline.
- id: FR-004-bounded-reads-opt-in-2
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **Age.** A cached entry satisfies the policy only when the age of the data,
      measured from when the source read that produced it began, is no greater than
      the caller's maximum age. The measure is the actual source fetch, never the
      time the entry was written to the cache, the time a record was reported, or
      the time of a later read. An entry that cannot show when its source read
      began (for example an entry written before that evidence was kept) does not
      satisfy a policy. An entry that does not satisfy the policy is refreshed under
      the same deadline. A maximum age above the five-mi
- id: FR-004-bounded-reads-opt-in-3
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **Deadline.** The deadline covers the whole operation: the cache read and any
      cache lock wait, every page of every paginated source query, every nested
      read, and the cache write of a refreshed entry. No request starts after the
      deadline has expired, and each request's timeout is the lesser of its legacy
      per-request timeout and the time remaining. Expiry returns a typed timeout
      error that is distinct from a transport failure and a rate limit.
- id: FR-004-bounded-reads-opt-in-4
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **Start.** The deadline starts when the bounded read begins on a source whose
      credentials are already resolved. Credential resolution happens before, under
      its existing operating-system-managed behavior, and may wait for user
      interaction. The bounded read does not bound that wait, and it neither
      resolves credentials again nor retries, falls back, or passes a credential
      around that behavior.
- id: FR-004-bounded-reads-opt-in-5
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **No continuation.** A timeout or failure returns no snapshot, stores no
      partial or unfinished data as a fresh entry, and returns no stale entry. It
      leaves no thread, request, or retry running after the call returns, and a
      source completion that arrives after expiry cannot change the cache or the
      result. The library adds no retry or sleep loop.
- id: FR-004-bounded-reads-opt-in-6
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **Capability.** A source that cannot honor a requested bound SHALL fail the
      bounded read with an explicit typed error before it reads, and SHALL NOT
      perform an unbounded read in its place. The legacy read method of such a
      source is unchanged.
- id: FR-004-bounded-reads-opt-in-7
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **Publication.** The bounded read returns a snapshot only when it is complete
      and within the policy. A caller that persists or publishes a result does so
      after that return, outside the deadline; a timeout therefore leaves the caller
      nothing to publish. Last-good storage and adoption history remain with the
      caller.
- id: FR-004-inputs-1
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- Structured Linear records obtained through the read client."
- id: FR-004-inputs-2
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- Cached records and their refresh metadata."
- id: FR-004-inputs-3
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- The five-minute cache-entry lifetime."
- id: FR-004-inputs-4
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - For a bounded read only: the caller's maximum cache age and operation-wide
      deadline, and a source that can honor them.
- id: FR-004-outputs-1
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- Cached records for reads that do not require a refresh."
- id: FR-004-outputs-2
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- Refreshed records after a successful poll observes source changes."
- id: FR-004-outputs-3
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- An error when an expired entry cannot be refreshed."
- id: FR-004-outputs-4
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - For a bounded read: the snapshot with its actual source fetch time, or a
      typed timeout, unsupported-bound, invalid-policy, rate-limit, or existing
      failure error, and no partial snapshot.
- id: FR-004-dependencies-1
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: "- **Upstream**: [US-001](../usecase/US-001-team-leader-reads.md)"
- id: FR-004-dependencies-2
  path: spec/functional/FR-004-cache-polling.md
  role: examined
  excerpt: |-
    - **Consumers**: [FR-010](FR-010-explicit-work-scope.md),
      [FR-012](FR-012-focus-status-and-ready-queue.md) and
      [FR-016](FR-016-focus-record.md) use the bounded read for named-focus
      validation; the consumer chooses its own policy values.
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
    both children and never the parent; a selected ticket whose descendants are all
    excluded is not turned into a leaf.
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
  excerpt: Given the same selection and two snapshots in which project P2 joins and
    then leaves initiative I, the selection bytes are unchanged and the effective
    set follows each snapshot; the stored selection contains no issue IDs other than
    ticket members.
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
  excerpt: For include and exclude members that are resolved, absent, a ticket of
    another team, a milestone with the wrong project, and not provable, the outcomes
    are `Resolved`, `Missing`, `WrongTeam`, `ProjectMismatch`, and `Unavailable` respectively,
    in deterministic order, naming kind and identifier; a name, label, or ticket count
    never changes an outcome; a selection with any non-resolved outcome is reported
    incomplete with those outcomes.
- id: FR-010-AC-17
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: An initiative with no project association in the snapshot and no catalog
    evidence is `Unavailable`, not `Missing`; a team that appears in no project or
    issue gives an unavailable result rather than an empty or healthy one; an absent
    project, milestone, or ticket ID in a complete snapshot is `Missing`. Legacy `ScopeSelection`
    still refuses an unknown team.
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
- id: FR-010-description-1
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    Structured scope selection SHALL resolve initiative, project, milestone and ticket selectors
    and measure leaf-task progress from one complete snapshot.
- id: FR-010-behavior-1
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    Selectors form a deduplicated union. Initiative selectors include their projects;
    initiative selectors accept Linear UUIDs, human-readable identifiers such as
    `I-10`, and exact names without case sensitivity. The source snapshot SHALL
    retain the structured initiative identifier for this resolution.
    Milestone selectors accept a Linear UUID, a globally unique name without case
    sensitivity, or `<project identifier|name>/<milestone name>`. A slash may occur
    within either name; the resolver considers the whole input and every project
    separator interpretation, then requires one distinct milestone ID
- id: FR-010-named-selection-1
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    A **named selection** is an additive input that reuses the resolvers, closures,
    team fence, and leaf classification above. It does not change `ScopeSelection`,
    `FocusSelection`, or their results. A named selection has one team, an
    **include** list, and an **exclude** list. Each list holds canonical members of
    the four existing kinds: initiative, project, milestone, and ticket. A milestone
    member carries its owning project ID as well as its milestone ID. The selection
    stores selector identities only, never expanded issue IDs.
- id: FR-010-named-selection-2
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: "- **Effective set.** For one selection, the effective issue set is the
    union of\n  the include closures minus the union of the exclude closures, then
    intersected\n  with the team fence. The closures are the existing ones: an initiative
    selects\n  the projects it holds in the current snapshot and their issues; a project\n
    \ selects its assigned issues; a milestone selects only its assigned issues; a\n
    \ ticket selects itself and its descendants. An exclusion removes by the same\n
    \ closure, and an exclusion wins over every inclusion of the same issue within\n
    \ its own selection. An exclusion is local to "
- id: FR-010-named-selection-3
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - **Combination.** The combined set of several selections of one team is the
      union of each selection's effective set, never the union of includes minus the
      union of excludes. Selections of different teams are separate groups and are
      never combined.
- id: FR-010-named-selection-4
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - **Leaves.** Leaf classification keeps full snapshot ancestry. Excluding a child
      does not make its parent a leaf, and excluding a parent does not hide a
      descendant that another member still selects. Canceled and duplicate leaves
      and parents are not counted, as above.
- id: FR-010-named-selection-5
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - **Intent.** A named selection with an empty include list is an explicit empty
      selection: its effective set is empty, whatever its team and exclusions, and it
      never means the whole team. A request with zero selections is a distinct
      no-active-selection result. Both differ from a `ScopeSelection` with only a
      team, which still selects the whole team, and from an empty inline
      `FocusSelection`, which is still rejected.
- id: FR-010-named-selection-6
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - **Dynamic membership.** Membership is recomputed from the current snapshot and
      the stored selector identities on every evaluation. A project that joins or
      leaves an included or excluded initiative changes membership with no change to
      the selection.
- id: FR-010-named-selection-7
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - **Qualified milestones.** A milestone member is valid only when its recorded
      project owns it in the snapshot. A string input keeps the existing grammar,
      including slashes inside either name, and canonicalizes to the milestone ID and
      its owning project ID. A recorded pair whose owner differs is reported, not
      repaired, and a same-named milestone of another project is never selected.
- id: FR-010-named-selection-8
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - **Member outcomes.** Every include and exclude member has one typed outcome:
      `Resolved`, `Missing`, `WrongTeam`, `ProjectMismatch`, or `Unavailable`,
      listed in a deterministic order. `WrongTeam` applies to a ticket of another
      team. A project, initiative, or milestone that holds other-team work stays
      resolved, and the other-team work is left out by the team fence. `Missing` is
      reported only for a kind that the snapshot's source enumerates completely
      (projects, milestones, and issues). An initiative absent from every project
      association, and a team with no project or issue in the
- id: FR-010-dependencies-1
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: "- Linear project initiatives, ticket parent relationships and workflow
    categories."
- id: FR-010-dependencies-2
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: "- The existing complete workspace snapshot and cache in ReadService."
- id: FR-010-dependencies-3
  path: spec/functional/FR-010-explicit-work-scope.md
  role: examined
  excerpt: |-
    - [FR-004](FR-004-cache-polling.md) for the opt-in bounded read that a
      named-selection caller may use to obtain the snapshot.
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
  excerpt: An explicit empty selection returns state `empty` with no rows and zero
    leaves; a batch with zero selections returns `none`; a selection with an unresolved
    or unavailable member or an unknown team returns `incomplete` or `unavailable`
    with counts marked unavailable or partial; the four results are distinguishable
    and none is reported as complete, healthy, or zero.
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
- id: FR-012-description-1
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    The CLI SHALL provide `focus status` over the inline selectors and expansion
    defined by FR-010 and FR-011. One result SHALL include the status of the focus,
    its blocker details, and its ready queue. This requirement adds those views to
    the existing focus expansion; it SHALL NOT redefine selector membership,
    issue descendants, or blocker traversal.
- id: FR-012-description-2
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    For every selected project and its milestones, the result SHALL report the
    open issue count grouped by Linear workflow-state display name, the target
    date when present, and the issues in progress. An issue is open when FR-001
    classifies it as neither `Completed` nor `Canceled`. An issue is in progress
    when its Linear workflow-state type is `started`. Open-state groups and issue
    lists SHALL be ordered deterministically by state display name and stable
    Linear issue identifier, respectively.
- id: FR-012-description-3
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    Each project SHALL also report an `unmilestoned` group for focus issues
    assigned to that project without a milestone. It SHALL use the same open-state,
    in-progress, and leaf-progress rules as milestone groups, including when the
    group is empty. Terminal output SHALL label open-state counts as including
    parent issues and progress as counting leaf tasks.
- id: FR-012-description-4
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    Open-state counts and in-progress rows SHALL include only resolved focus
    member issues assigned to the corresponding project or milestone. Blocker-only
    issues SHALL NOT contribute to these counts or rows unless they are also
    resolved focus members.
- id: FR-012-description-5
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    Progress SHALL reuse FR-010's leaf-work classification: completed leaves
    count as done; canceled and duplicate leaves do not count as work; parents do
    not count as leaves. Project progress SHALL count classified leaf issues in
    that project that belong to the resolved focus. Milestone progress SHALL count
    the subset assigned to that milestone. Each issue SHALL contribute at most
    once to each applicable project or milestone group. The result SHALL include
    done and total leaf counts and the completed percentage. For a group with no
    countable leaves, the CLI SHALL report zero done and total counts
- id: FR-012-description-6
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    For every focus issue, the result SHALL expose its complete transitive
    `blocked_by` closure from FR-011, including each blocker’s stable identifier,
    title, workflow-state display name, and FR-001 state classification
    (`Completed`, `Canceled`, or open). The result SHALL preserve the blocker-layer
    ordering and include blockers from every project and team, including completed
    and canceled blockers. The result SHALL report the aggregate FR-011 blocker
    layers, with each blocker included once in its layer and a `focus_root_ids`
    list sorted by stable Linear issue identifier, naming every resolved foc
- id: FR-012-description-7
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    The ready queue SHALL contain focus issues that satisfy FR-001's full ready
    eligibility rules, including project-running state, issue and spec state, and
    all transitive cross-team blockers being `Completed`. It SHALL use FR-001's
    ordering and readiness reasons, restricted to the resolved focus issue set.
    The output SHALL preserve the distinction between focus issues and blocker-only
    issues; blocker-only issues SHALL NOT enter the ready queue unless they are
    also members of the resolved focus issue set.
- id: FR-012-description-8
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    The optional repeatable `--work-milestone` selector SHALL accept an exact
    milestone name or ID among the selected projects. If supplied, blocker roots
    and ready issues SHALL be limited to focus issues assigned to matching
    milestones. The complete prerequisite closure of those roots SHALL remain
    visible even when a prerequisite belongs to another milestone or project.
    Project and milestone progress SHALL remain for the full selected focus, so
    the report retains its denominator. A selector matching no selected project
    milestone SHALL fail before emitting a partial report. The JSON result SHALL
    i
- id: FR-012-named-selection-summaries-1
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    The library SHALL additionally evaluate a **batch** of FR-010 named selections
    against one snapshot and return, in one deterministic result, a summary for each
    selection and a combined summary for each team group. This extends, and does not
    replace, `focus status`; it reuses FR-001 readiness, FR-002 prerequisite
    closure, FR-010 membership and leaf rules, and the existing project status
    category. It introduces no second calculator and no store.
- id: FR-012-named-selection-summaries-2
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **One snapshot.** Every selection and every combination in one batch is
      computed from one snapshot, whose actual fetch time the result carries. The
      caller's opaque key for each selection (for example a focus ID and revision) is
      returned unchanged with that selection's result, so a result is never presented
      as another selection or another snapshot. The result order is the order of the
      FR-010 member and selection keys, then by name, then by ID, and the same input
      gives the same output.
- id: FR-012-named-selection-summaries-3
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Per-selection summary.** Each selection reports its state (`empty`, `none`,
      `complete`, `incomplete`, or `unavailable`), its FR-010 member outcomes, its
      deduplicated leaf set, and for that set: done, total, remaining by stage,
      ready, and blocked counts. A ready leaf is one that FR-001 would place in its
      ready queue. A blocked leaf is an open leaf whose complete FR-002 transitive
      `blocked_by` closure contains a prerequisite not classified `Completed`.
- id: FR-012-named-selection-summaries-4
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Placements.** Each selection reports initiative rows with their projects and
      direct-ticket rows. A row's counts are restricted to that selection's leaves.
      A row is marked **partial**, with one or more reasons, when the selection covers
      only part of the Linear object: a milestone-only inclusion, a ticket-only
      inclusion, an exclusion, or the team boundary. A row is marked **shared** when
      it appears in more than one selection of the batch or under more than one
      initiative, and every placement of it shows the same value for the same leaf
      scope.
- id: FR-012-named-selection-summaries-5
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Combination.** For each team, the combined summary counts the union of the
      leaf sets of that team's selections once per leaf, with the same fields.
      Different teams are separate groups with no cross-team total, union, or
      readiness. No result sums per-selection, per-initiative, or per-project
      totals.
- id: FR-012-named-selection-summaries-6
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Prerequisites.** An excluded prerequisite stays in the complete blocker
      closure as evidence and is not counted as a member of the selection. An
      exclusion SHALL NOT make an included leaf ready or hide a blocker.
- id: FR-012-named-selection-summaries-7
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Unknown is not zero.** A selection that is `empty` (explicit empty include
      list), the batch with zero selections (`none`), and a selection with an
      unresolved member, an unavailable member or team, or an incomplete result are
      distinct states. A project whose native status category is `Unknown` keeps that
      classification in its row. Counts that depend on unresolved, unavailable, or
      unknown facts are marked unavailable or partial, never complete, healthy, or
      zero.
- id: FR-012-named-selection-summaries-8
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Failures.** A snapshot that is incomplete, a parent cycle, a missing parent,
      a missing blocker record, or a dependency cycle returns the existing explicit
      error for the whole batch and no partial result. A failed or timed-out read
      returns the FR-004 error and no result. The library keeps no last-good result
      and no revision cache; the caller keeps any last-good result with its original
      fetch time.
- id: FR-012-inputs-1
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - One or more inline initiative, project, or issue selectors accepted by
      `focus status` and resolved under FR-010 and FR-011.
- id: FR-012-inputs-2
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - A complete structured Linear workspace snapshot with projects, project
      milestones, issues, workflow states, project target dates, and blocker
      relations.
- id: FR-012-inputs-3
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    Selector matching, deduplication, issue descendants, and focus membership
    follow FR-010 and FR-011. An empty focus or incomplete source snapshot SHALL
    produce an explicit error and no partial status result.
- id: FR-012-outputs-1
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - A deterministic status result containing project and milestone target dates,
      open issue counts by state, in-progress issue rows, and leaf progress counts
      and percentage, restricted to resolved focus members.
- id: FR-012-outputs-2
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - Aggregate blocker layers with each blocker’s current workflow state and
      classification, plus the sorted `focus_root_ids` attribution list.
- id: FR-012-outputs-3
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - The FR-001 eligible ready queue for focus issues, with deterministic order
      and reasons.
- id: FR-012-outputs-4
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - The actual snapshot fetch timestamp, preserving cache age under FR-010.
      Terminal output SHALL show the fetch time as UTC ISO time and its age; JSON
      SHALL retain the Unix-second `fetched_at` value.
- id: FR-012-dependencies-1
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - **Upstream**: [FR-001](FR-001-ready-queue.md),
      [FR-002](FR-002-project-chunk.md),
      [FR-005](FR-005-structured-data-boundary.md),
      [FR-010](FR-010-explicit-work-scope.md),
      [FR-011](FR-011-inline-planner-focus.md),
      [FR-004](FR-004-cache-polling.md) for the bounded snapshot read
- id: FR-012-dependencies-2
  path: spec/functional/FR-012-focus-status-and-ready-queue.md
  role: examined
  excerpt: |-
    - Structured Linear project, milestone, issue, workflow-state, target-date,
      and blocker records.
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
    and no conversion; versions 0, 1, and 4 are refused the same way; a version 3
    document presented to the version 2 decoder is refused.
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
    and unknown members are not allowed; the version 2 schema remains exported separately.
- id: FR-016-AC-11
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: Canonicalizing named string inputs yields canonical IDs and the milestone's
    owning project ID, and fails with no partial document for a missing, ambiguous,
    other-team, or project-mismatched member; the stored document contains no label,
    expanded issue ID, or status.
- id: FR-016-description-1
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    `ix-board --json focus report` SHALL emit one `FocusRecord` with an explicit
    schema version, typed canonical Linear-ID initiative, project, milestone, and
    issue members, the report time, and the
    complete ix-board focus status. The library SHALL expose the same Rust type
    and a generated JSON Schema. The record SHALL be computed from one workspace
    snapshot, so its selector resolution and status refer to the same source data.
    The report command SHALL be read-only and SHALL NOT assume a transport or
    storage service.
- id: FR-016-schema-versions-1
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    **Version 2 (existing, unchanged).** `FocusRecord` with `schema_version` 2 has
    exactly the members `schema_version`, `selection` (a list of `{kind, id}`
    members with kind `initiative`, `project`, `milestone`, or `issue`),
    `reported_at`, and `status`. It carries no team, no exclusions, no milestone
    project qualification, and no empty-selection intent, and unknown members are
    rejected. `focus report` keeps emitting version 2 for inline selections, and its
    bytes, its empty-selection failure, and its schema do not change.
- id: FR-016-schema-versions-2
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    **Version 3 (additive named selection).** A separate versioned document, the
    **named selection**, carries the FR-010 named selection for storage by another
    service. It has exactly these members, all required:
- id: FR-016-schema-versions-3
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: "- `schema_version`: the number 3."
- id: FR-016-schema-versions-4
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: "- `team`: one Linear team key."
- id: FR-016-schema-versions-5
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    - `include` and `exclude`: lists of `{kind, id}` members; a `milestone` member
      also requires `project_id`, and no other kind carries it. An empty `include`
      list is the explicit empty selection. A missing `include` or `exclude` is a
      decode error and never an empty list.
- id: FR-016-schema-versions-6
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    The document holds canonical Linear IDs only, with no labels, expanded issue IDs,
    or status. It is encoded in one canonical order (kind, then ID, with the
    milestone project as a tie-break) with no duplicate member in a list, so the
    same selection always encodes to the same bytes. A member that is in both lists
    is kept in both; the exclusion wins at evaluation. The library exposes the Rust
    type and a generated JSON Schema for it, and `focus report` and its schema are
    unaffected.
- id: FR-016-schema-versions-7
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    **Version handling.** A decoder accepts exactly the version it implements and
    checks the version before the other members. Any other version, older or newer,
    is a typed unsupported-version error that names the version found and the
    version supported. The library never converts, upgrades, or fills in a missing
    team, exclusion list, or intent. In particular a version 2 record presented to
    the named-selection decoder is refused as unsupported and is never read as a
    named selection. A version 3 document presented to the version 2 `FocusRecord`
    decoder is refused. A malformed document of the suppor
- id: FR-016-dependencies-1
  path: spec/functional/FR-016-focus-record.md
  role: examined
  excerpt: |-
    - **Upstream**: [FR-010](FR-010-explicit-work-scope.md) for named-selection
      membership and outcomes, [FR-011](FR-011-inline-planner-focus.md),
      [FR-012](FR-012-focus-status-and-ready-queue.md), and
      [US-007](../usecase/US-007-share-focus-record.md).
 ```

## Frozen correction verification (round 2)

Verified 0db46b8f7e0d03899e7b0cf8dd4b23326a2c6617 against round 1 head a899b69ae12f13a9ed8bb5dbbbd08f3a9d91ccd8. Only FR-012 placement/combination prose and AC-21/22 changed; all fourteen prior fixes remain present. Complete plus explicit-empty yields complete, all-empty yields empty, and unknown-dependent facts continue to contaminate union availability. An explicitly excluded project is omitted; an unexcluded directly included known-empty project retains its valid zero row. Four-FR validation exit 0, no output; 72 criteria remain matrix-visible (non-strict, no implementation coverage claim). This is a confined follow-up, not a fresh repository-wide review. Latest disposition verdict: **PASS**.
