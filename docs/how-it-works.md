# How it works

ix-board is a cached reader of structured Linear data. It computes planning
reports from shared summaries and detailed workspace snapshots.

## Structured data only

Filtering, readiness, ordering and every other decision use Linear's structured
fields: workflow states and their types, priorities, manual order, project
status, milestones, initiative membership, parent links and blocker relations.
Ticket descriptions and comments are returned as data. ix-board does not change
its behavior because of their content and does not call a language model. Description
text is used in two places: `search` matches it, and `focus plan-chunk` shows
an acceptance-criteria excerpt from it. Neither affects readiness, ordering or
focus membership.

GitHub is read only for pull requests linked by structured Linear attachments
and for the repositories in a scope file. ix-board does not read or write GitHub
Issues.

## Cache

<a id="cache"></a>

Roadmap and scope checks load native initiative and project summaries. Reports
that need tickets load a separate detailed snapshot. Each dataset defaults to
a five-minute lifetime. A warm read makes no Linear request.

Expired datasets merge records changed since their last successful source read,
with a 60-second overlap. Archived and trashed records observed in a delta leave
active results immediately. A full accessible-catalog scan runs at least daily
when that dataset is requested; it removes unobserved deletions and visibility changes.

One process refreshes at a time using an adjacent operating-system file lock.
Other processes wait up to five seconds and reuse its completed refresh.
Dataset identity includes the exact endpoint, an opaque credential digest, model
version, and coverage. Native Linear credentials bind workspace and authorization
visibility, so changing a credential requires a separate full baseline. Credentials
are never stored in the cache. Quota accounting remains shared across those partitions.
Failed ordinary reads can reuse the last complete compatible snapshot. Such results
carry `stale`, `age_seconds`, `rate_limit_until`, and `failure_reason` in JSON and text.
Bounded library reads preserve their explicit age limit, deadline, and error contract.

Every request reserves shared request and estimated complexity capacity before
send, including histories, update probes, and mutations. Unknown query cost reserves
10,000 complexity units. Response headers supply actual limits and reset times.
Server rate limits persist across processes; absent retry metadata means a 60-second cooldown.
No request starts during cooldown, and commands do not sleep or retry.

Run `ix-board cache status --json` to inspect coverage, successful sync and full
reconciliation times, quota state, and refresh counters without credentials or network access.
`latest_refresh` describes the latest recorded attempt. It is null while a refresh
is running or when a bounded timeout left no time to record a final descriptor.
Transport reservations remain counted even when a refresh fails or times out.
Summary and fallback JSON wrap the command payload in `result`, beside freshness metadata.
Fresh detailed commands retain their array or object payload shape. `cache status`
provides every dataset's provenance, and library callers can inspect `cache_freshness()`.
Summary and stale envelopes include `last_successful_sync` and `last_full_reconciliation`.

`IX_BOARD_CACHE_PATH` selects the database path. See [setup](setup.md#cache-location)
for the default locations. `focus hygiene` also stores its most recent baseline
in the same file.

Library callers use `ReadService::workspace_snapshot` for the cached snapshot,
`force_refresh_workspace` to fetch through the cache, and `invalidate_cache`
to discard a snapshot before a write. A forced refresh bypasses the ordinary TTL
and retains the shared quota and refresh coordination.

## Readiness

`next` and the ready queue in `focus status` apply the same rules. A ticket is
ready when all of these hold:

1. Its project status is `Started`. A `Paused` project and its tickets are
   excluded.
2. Its issue state is open. Linear states of type `completed` and `canceled` are
   closed whatever their display names.
3. Its spec state maps to a next step: `Backlog` and `Specify` map to Specify,
   and `Specced` maps to Coding. Tickets in Coding or Review, and tickets in any
   other state, are not offered.
4. Every ticket in its recursive `blocked-by` graph is `Completed`. This
   includes blockers in other teams and other projects. A `Canceled` blocker
   stays unresolved.

A blocker record missing from the snapshot is an error. A dependency cycle is an
error.

### Ordering

Ready tickets are sorted by these keys, in order:

1. Project priority. `No priority` sorts after Urgent, High, Medium and Low.
2. Project manual order.
3. Ticket priority, with the same treatment of `No priority`.
4. Ticket manual order.
5. Project identifier, ticket identifier and ID, as tie-breakers.

Each row's reason states the project, the mapped step and the key values.
Identical snapshots produce identical rows, order and reasons.

## Project chunk

`project-chunk <PROJECT>` returns a project's goal, its milestones, its open
tickets and its blocker layers. A project is found by identifier, UUID or exact
case-insensitive name; an identifier or UUID wins over a colliding name.
Blocker layers are the full transitive `blocked-by` closure of the project's
open tickets, including blockers in other projects and teams and blockers that
are completed or canceled. Prerequisites come before their dependents, and each
layer is sorted by issue identifier.

## Commands that write

Two commands change Linear. Each changes one field of one project.

```bash
ix-board project set-status <PROJECT> <Started|Paused>
ix-board project set-priority <PROJECT> <0|1|2|3|4>
```

Priority `0` is no priority, `1` urgent, `2` high, `3` medium and `4` low. After
a write ix-board updates the cached snapshot with the new value. No
other command writes to Linear or GitHub, and ix-board never writes manual
order.

## Retro store

The `retro` commands append measurement records to a local store outside every git work tree
(`$TEAM_RETRO_HOME`, default `~/.local/state/team-retro`). They write nothing to Linear, GitHub or
any repository. See [retro](retro.md).

## Output and exit status

Text output is for reading in a terminal. `--json` prints one JSON value on
stdout, and diagnostics go to stderr (`--diagnostics-format` and `--color` control
their form). A command exits non-zero when it fails. `issues` also exits
non-zero when every requested issue is missing, `scope check` when the scope is
invalid, and `agents` when a machine could not be read.

The public wire types derive JSON Schema. [CLI reference](cli-reference.md#json-schema-export)
shows how to export them.
