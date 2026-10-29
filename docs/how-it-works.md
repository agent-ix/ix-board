# How it works

ix-board loads one snapshot of a Linear workspace, caches it, and computes every
report from that snapshot. This page describes the data boundary, the cache,
the readiness rules and the commands that write.

## Data sources

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

The first read of a session fetches projects, initiatives, milestones and issues
from Linear in paged queries and stores them as one snapshot in a SQLite file.
Each snapshot expires five minutes after it was fetched.

- A read of an unexpired snapshot makes no Linear request.
- A read of an expired snapshot fetches again and replaces it.
- If the fetch fails, the command returns the error. It never returns the
  expired snapshot.
- Text reports show the fetch time and its age. JSON reports carry
  `fetched_at` as Unix seconds.
- A Linear rate-limit response is reported as an error with the cooldown derived
  from the exhausted quota.

`IX_BOARD_CACHE_PATH` selects the database path. See [setup](setup.md#cache-location)
for the default locations. `focus hygiene` also stores its most recent baseline
in the same file.

Library callers use `ReadService::workspace_snapshot` for the cached snapshot,
`force_refresh_workspace` to fetch through the cache, and `invalidate_cache`
to discard a snapshot before a write. A failed forced refresh leaves no cached
snapshot.

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

## Output and exit status

Text output is for reading in a terminal. `--json` prints one JSON value on
stdout, and diagnostics go to stderr (`--diagnostics-format` and `--color` control
their form). A command exits non-zero when it fails. `issues` also exits
non-zero when every requested issue is missing, `scope check` when the scope is
invalid, and `agents` when a machine could not be read.

The public wire types derive JSON Schema. [CLI reference](cli-reference.md#json-schema-export)
shows how to export them.
