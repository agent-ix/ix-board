# Named selections and bounded reads

Library consumers (not the CLI) can store a **named selection**, evaluate many of
them against one snapshot, and read that snapshot under a deadline. Nothing here
changes `focus status`, `focus report`, or the existing read methods.

## Named selection

A named selection is a team plus an `include` list and an `exclude` list. Each
member is one of four kinds, named by canonical Linear ID:

| `kind` | Fields |
| --- | --- |
| `initiative` | `id` |
| `project` | `id` |
| `milestone` | `id`, `project_id` (the project that owns it) |
| `issue` | `id` (the ticket and its descendants) |

```rust
use ix_board::{NamedMember, NamedSelection, TeamKey, decode_named_selection};

let selection = NamedSelection::new(
    TeamKey("SWM".into()),
    vec![NamedMember::Initiative { id: initiative_id }],
    vec![NamedMember::Project { id: paused_project_id }],
);
let document: String = selection.encode()?; // `schema_version` 3, canonical order
let stored = decode_named_selection(document.as_bytes())?;
```

The document holds IDs only: no names, expanded issue IDs, or status. The same
selection always encodes to the same bytes, in kind order (initiative, project,
milestone, issue) and then ID. `NamedSelection::new` sorts and removes duplicates.

`decode_named_selection` reads version 3 only and checks the version first.
Another version, including a version 2 `FocusRecord`, is
`Error::UnsupportedSchemaVersion { found, supported }`. A version 3 document that
is malformed (missing or unknown member, milestone without `project_id`, a kind
other than the four above, duplicate member) is `Error::InvalidNamedSelection`.
Nothing is converted or filled in: a missing `include` is an error, never an empty
list or a whole-team selection. `include: []` is the explicit empty selection.

To build a selection from names, use `canonicalize_named_selection`. It accepts
the same names, identifiers, UUIDs and `<project>/<milestone>` forms as `focus
status`, and refuses a missing, ambiguous, other-team, or project-mismatched member
without producing a partial document.

Export the JSON Schema with
`cargo run --example export_schema -- NamedSelection`.

## Evaluating a batch

```rust
use ix_board::{CatalogEvidence, NamedFocusRequest, evaluate_named_batch};

let batch = evaluate_named_batch(
    &snapshot,
    &CatalogEvidence::default(),
    &[NamedFocusRequest { key: "focus-7@rev-2".into(), selection }],
)?;
```

Every selection and team result in a `NamedBatch` comes from the one snapshot, whose
fetch time is `batch.fetched_at`. Your `key` comes back unchanged. Results are
ordered by key, and teams by team key.

- **Membership.** Per selection: include closures minus exclude closures, limited to
  the selection's team. An exclusion applies only inside its own selection. A ticket
  exclusion removes its descendants, even ones the same selection includes.
- **Leaves.** A parent is never a leaf, whatever is excluded.
- **Teams.** Selections of one team are combined by union, counting each leaf once.
  Different teams are never combined.
- **Counts.** `total`, `done`, `remaining_by_stage`, `ready` and `blocked`.
  Readiness and blocking use the full prerequisite closure, so a prerequisite that a
  selection excludes still blocks. It is listed in `blocker_layers` and never counted.
  No total is a sum of overlapping rows.
- **Rows.** Initiative rows with their projects, standalone project rows, and
  direct-ticket rows. `partial` says why a row covers only part of its object
  (`milestone_only`, `ticket_only`, `exclusion`, `team_boundary`). `shared` marks a
  project that appears in more than one placement.
- **Failures.** A parent cycle, missing parent or prerequisite record, or dependency
  cycle fails the whole batch with its usual error.

### What is known

Every member has one outcome: `resolved`, `missing`, `wrong_team`,
`project_mismatch`, or `unavailable`. A snapshot holds only the records it fetched,
so an absent project, milestone, ticket or initiative is `unavailable` unless you
pass `CatalogEvidence` saying the source enumerated every record of that kind,
including archived and hidden ones. `LinearGraphQlSource` does not claim that.

A selection state tells you how much to trust the counts:

| `state` | Meaning |
| --- | --- |
| `empty` | `include` is empty. The result is exactly zero. |
| `complete` | Every member resolved. |
| `incomplete` | A member did not resolve. Counts cover resolved members only (`partial`). |
| `unavailable` | Nothing could be established, such as a team that appears nowhere. Zeros mean unknown. |

A batch with no selections has `state: none` and no totals. A team union is
`complete` only when every non-empty selection is complete; `causes` names each
selection that keeps it from being complete. A project whose Linear status category
is `unknown` keeps that category in its row and lists it under `unknown_facts`.

Initiative members depend on each project's initiative links. When the source does not
establish them (`CatalogEvidence::initiative_links` is `false`) and a project that holds
the team's work is not known to belong to the initiative, that project may still belong
to it. An initiative include or exclude then keeps the leaves it knows, but its selection
is `incomplete` with `initiative_links` under `unknown_facts`, and the team union is
`incomplete` and names the selection in `causes`. Set `initiative_links` only when the
source really enumerates every project's links.

## Bounded read

`ReadService::workspace_snapshot_bounded` reads the snapshot under a policy:

```rust
use std::time::Duration;
use ix_board::ReadPolicy;

let policy = ReadPolicy {
    max_age: Duration::from_secs(120),
    deadline: Duration::from_secs(20),
};
let snapshot = service.workspace_snapshot_bounded(&policy)?;
let batch = service.named_batch_bounded(&policy, &requests)?; // read, then evaluate
```

- `max_age` is how old cached data may be, counted from when its source read began.
  It can be at most 300 seconds; a longer value is `Error::InvalidReadPolicy`.
  Cached data that cannot show when its read began (for example from an older
  version) is refreshed, not accepted. `snapshot.fetched_at` is when the source read
  began.
- `deadline` bounds the whole call: the cache lookup, lock wait and decoding, every
  page and nested request, and the cache write. `named_batch_bounded` keeps that one
  deadline through the source's catalog evidence, validation and evaluation too, and
  returns `Error::ReadTimeout` rather than a batch that finished late. A cached
  snapshot is also returned only while the deadline has not expired. Each request gets the lesser of 30 seconds and
  the time left, and none starts after the deadline. Expiry is `Error::ReadTimeout`,
  distinct from a transport failure and from `Error::LinearRateLimited`, which keeps
  its cooldown. There is no retry, and nothing keeps running after the call returns.
- The deadline starts when the call begins. Resolve credentials first, with your
  credential provider: the read never resolves them, and the wait for them is not
  counted.
- A source that cannot honor a deadline (`WorkspaceSource::supports_bounded_read`)
  fails with `Error::BoundUnsupported` before any read.
- A timeout or failure returns no snapshot and stores nothing as fresh. Keep any
  last-good result yourself, with its original fetch time.

The existing methods, such as `workspace_snapshot`, are unchanged: five-minute entry
lifetime, 30-second request timeout, no deadline.
