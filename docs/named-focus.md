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
without producing a partial document. It has no deadline of its own. To create or
update a definition from names under one deadline, use
[`canonicalize_named_selection_bounded`](#canonicalizing-names-under-one-deadline).

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
- **Failures.** A parent cycle or a dependency cycle fails the whole batch with its
  usual error.
- **Unavailable references.** A missing parent, prerequisite, project or milestone
  record does not fail the batch. Its `warnings` member lists them, as in
  [focus reads](focus.md#unavailable-references). A selection that depends on a
  missing parent, prerequisite or project is `incomplete` with
  `unavailable_references` under `unknown_facts`. So is a selection with a ticket
  member while any issue of its team has a missing parent.

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
| `incomplete` | A member did not resolve, or a fact the counts depend on is unknown. Counts are `partial`. |
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

### Canonicalizing names under one deadline

A caller that accepts names or identifiers and must store only canonical IDs uses
`ReadService::canonicalize_named_selection_bounded`. It starts one deadline, reads
the snapshot as `workspace_snapshot_bounded` does, and resolves the input against that
snapshot under the same deadline:

```rust
use ix_board::{NamedMemberInput, NamedSelectionInput, ReadPolicy};

let input = NamedSelectionInput {
    team: "SWM".into(),
    include: vec![
        NamedMemberInput::Initiative("I-10".into()),
        NamedMemberInput::Milestone {
            selector: "Project / Alpha/Beta".into(),
            project: None,
        },
    ],
    exclude: vec![NamedMemberInput::Ticket("SWM-2".into())],
};
let selection = service.canonicalize_named_selection_bounded(&policy, &input)?;
// Only now, outside the deadline, store or publish `selection.encode()?`.
```

- The deadline covers the cache lookup and decode, the source read, every name,
  identifier, team, project, initiative, milestone and ticket scan, and the ordering of
  each member list. The read's time counts: resolution gets only what is left. The
  deadline is checked for every record of a scan, including the initiatives, milestones
  and team keys inside one project, so a single large project cannot outrun it, and
  again before the call returns.
- The result is what `canonicalize_named_selection` returns for the same snapshot and
  input: IDs only, a milestone with its owning project, an explicit empty `include`
  kept empty.
- A timeout is `Error::ReadTimeout`. A member that is missing, ambiguous, of another
  team or owned by another project is the same error the standalone form returns.
  Either way there is no document. The call stores and publishes nothing.
- Resolve credentials first, as for every bounded read. Policy and unsupported-source
  errors are the same as for `workspace_snapshot_bounded`.

### Keeping your labels: which input became which member

The canonical selection is sorted and de-duplicated, so it cannot say which input
became which member. When several names, identifiers or UUIDs resolve to one member,
or you want to keep the text a person typed, use
`ReadService::resolve_named_selection_bounded`. It is the same call with the same
deadline, snapshot and resolution, and it also returns what every input occurrence
resolved to. Do not resolve a second time to find out.

```rust
use ix_board::{ListKind, NamedMemberInput, NamedSelectionInput};
use std::collections::BTreeMap;

let input = NamedSelectionInput {
    team: "SWM".into(),
    include: vec![
        NamedMemberInput::Ticket("SWM-2".into()),
        NamedMemberInput::Project("Project / Alpha".into()),
        NamedMemberInput::Project("PRJ-1".into()), // the same project by identifier
    ],
    exclude: vec![NamedMemberInput::Ticket("SWM-2".into())],
};
// Your own labels, keyed by where the input stood: (list, index in that list).
let labels = BTreeMap::from([
    ((ListKind::Include, 0), "second ticket"),
    ((ListKind::Include, 1), "alpha"),
    ((ListKind::Include, 2), "alpha (by key)"),
    ((ListKind::Exclude, 0), "skip it"),
]);

let resolved = service.resolve_named_selection_bounded(&policy, &input)?;
store(resolved.selection.encode()?); // outside the deadline; carries no label
for entry in &resolved.associations {
    let label = labels[&(entry.list, entry.index)];
    keep(label, &entry.member); // entry.input is the text you supplied
}
```

- `resolved.selection` is exactly what `canonicalize_named_selection_bounded` returns,
  including the bytes of `encode()`. It carries no label.
- `resolved.associations` has one entry for every input occurrence, exactly once:
  every `include` occurrence in the order you supplied, then every `exclude`
  occurrence in the order you supplied. This is the **input order**, not the canonical
  order. Join by `entry.list` and `entry.index`, which are the list and zero-based
  index you supplied, or call `resolved.association(list, index)`. Never join by
  position in `selection.include` or `selection.exclude`: those are sorted and have
  fewer entries when occurrences collapse.
- Occurrences stay separate. In the example, `Project / Alpha` and `PRJ-1` are two
  associations holding the same `member`, and the selection holds it once. The same
  ticket in both lists is two associations (one per list) and one member in each list
  of the selection. Repeating an identical input gives one association per repetition,
  so each can keep a different label. Each entry also carries the `input` you supplied.
- You own the labels. The SDK returns each association with its input and resolved
  member. Keep your labels alongside the inputs. When aliases have different labels,
  choose whether to keep the first, last, or all labels.
- The timeout and error rules are those of `canonicalize_named_selection_bounded`.
  Capturing the associations is inside the same deadline. A timeout, or any member that
  is missing, ambiguous, of another team or owned by another project, returns the error
  and neither a selection nor associations.
