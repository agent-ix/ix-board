# Dataset campaign lead stories (proposed)

These stories came from the EARS dataset campaign field test. They record user
needs, not implemented behavior. Move a story into `spec/usecase/` and trace it
to functional requirements when work is accepted for implementation. Do not
describe a proposed story as a current CLI capability.

## Typed focus across roles

As a manager, lead, coder, or reviewer, I want one focus selection made from
initiatives, projects, milestones, and tickets in any combination so each role can use the
same structured scope at its appropriate level.

- An initiative-only, project-only, milestone-only, ticket-only, or mixed selection resolves to
  canonical Linear identities and the same issue membership on repeated reads.
- The saved focus record represents that selection independently from any
  optional report filter, transport, or live agent session.
- Role skills use the selected focus without interpreting ticket prose as
  instructions to add or remove scope.

Current coverage: `focus report` emits version 2 typed canonical members and
status. Shared role-skill consumption is future work. The earlier
initiative-brief command proposal was withdrawn:
standing rules belong in agent instructions and memory; attached plans are
optional context. `--milestone` selects focus membership; the older
`--work-milestone` view filter still restricts visible work within that focus.

## Cached reads without a credential

As a lead using a new build or a locked Keychain, I want to read the last
cached focus snapshot without resolving a credential so I can see its known
status and source age promptly.

- `ix-board focus status --initiative I-10 --offline` reads only the matching
  cached snapshot, exits successfully, and prints its actual fetch age.
- With no cached snapshot, it exits non-zero with a clear cache-miss message.
- Offline output is labeled as cached evidence and never implies a live Linear
  refresh or a current agent session.

Current coverage: warm reads are fast but CLI credential resolution happens
before the snapshot cache is consulted. There is no offline mode.

## Ticket and PR work in one place

As a campaign lead, I want linked PRs, branches, and review activity beside
their tickets so I can see work that has moved ahead of Linear and correct
stale ticket state.

- Given a structured PR attachment, show its repository, number, state, and
  review evidence beside the ticket, with an explicit mismatch when ticket
  and PR states disagree.
- Distinguish an unlinked PR or unpushed branch from a proven absence of work.
- Avoid inferring ticket identity from prose alone.

Current coverage: `focus hygiene` checks linked PR state and reports state
mismatches. It does not provide a general PR activity column, branch state, or
unpublished local work.

## Campaign setup in batches

As a planner establishing a campaign, I want to submit a reviewed file of
projects, milestones, tickets, updates, and dependency relations so I can
avoid one CLI call per ticket and inspect the intended write set first.

- A preview lists exact creates, updates, relations, and unchanged records.
- Applying the file reports each success and failure without silently
  changing unrelated records.
- Exact custom workflow status names are shown and ambiguous type names are
  rejected before mutation.

Current coverage: `ix-board issues` reads many existing issue states in one
call. ix-board has no batch issue write command.

## Shared focus for reporting clients

As a lead using a CLI, dashboard, and agent session, I want one versioned focus
record so each client reports the same selected initiative, projects, tickets,
and source age without reselecting them.

- ix-board defines the focus schema and emits a JSON record with canonical
  selectors and status.
- A generic relay store can publish and retrieve that record when configured;
  persistence policy belongs to the store.
- A client reports when the record is absent or stale rather than treating it
  as a live agent session.

Current coverage: a focus-record schema and JSON report are implemented.
Relay publication, retrieval, and shared UI use are not implemented.
