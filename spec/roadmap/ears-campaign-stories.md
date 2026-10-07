# Dataset campaign lead stories (proposed)

These stories came from the EARS dataset campaign field test. They record user
needs, not implemented behavior. Move a story into `spec/usecase/` and trace it
to functional requirements when work is accepted for implementation. Do not
describe a proposed story as a current CLI capability.

## Initiative briefing and handoff

As a campaign lead taking over an initiative, I want one initiative view with
its overview, projects, milestones, progress, and a read order so I can
understand the work without reconstructing it from individual Linear calls.

- Given a named initiative with two projects and an overview, the view returns
  the overview and both projects with progress from structured Linear records.
- A handoff identifies what is done, in review, in progress, blocked, and next
  per milestone, and makes source age visible.
- The overview may contain human guidance, but cannot override structured
  readiness or dependency facts.
- For initiative `I-10`, the lead can read the initiative `content` brief
  (goal, role, read order, owner decisions, checkpoints, rules, and deferred
  work) and titles and URLs of attached plan documents before acting on a
  plan chunk. `ix-board initiative brief I-10` returns the content verbatim,
  documents, and fetch age in text and JSON. An empty brief is clearly shown
  without failing the command. Project-level handoffs expose project `content`
  the same way.
- `focus plan-chunk` points to the brief and attached plans before its ordered
  rows. The brief may constrain the selected work milestones and checkpoints;
  it does not change a ticket's computed readiness or dependency order.

Current coverage: `focus status` provides progress, blockers, and ready work;
`focus plan-chunk` provides ordered work rows. Neither shows the initiative
overview or a complete read order.

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

Current coverage: a focus-record schema and JSON report are under review.
Relay publication, retrieval, and shared UI use are not implemented.
