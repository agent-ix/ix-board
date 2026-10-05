# ix-board

Deterministic, cached Linear reads for team leaders: an ordered ready queue,
project context, and structured project/issue search. Decisions use Linear's
structured states and dependency relationships; ticket descriptions and comments
remain data.

## Setup

If this plugin is uninitialized or a command fails, follow the [plugin setup guide](setup.md) for its required CLIs, configuration, and local diagnosis.

## Community help

If the setup checks leave a reproducible Agent IX ix-board bug that blocks progress, [join the Agent IX Discord](https://discord.gg/k8DVhuYBR2). Community help is a last resort for Agent IX product bugs, not a help desk for local credentials, machine setup, third party tools, or unrelated projects. See [setup.md](setup.md#community-help) for what to include.

## Project queue convention

Linear project status controls whether a project runs: `Started` projects
contribute tickets to `next`, while `Paused` projects are on hold and their
tickets are excluded. Project priority and manual order determine the global
queue position before ticket priority and manual order. ix-board may write only
Linear project status and priority; it does not write manual order or any other
Linear fields.

The project mutation commands are `project set-status <project> <Started|Paused>`
and `project set-priority <project> <0|1|2|3|4>`. The priority values map to
Linear as `0` = No priority, `1` = Urgent, `2` = High, `3` = Medium, and `4` =
Low. These make an explicit update to the requested project's corresponding
Linear field.

## Build

```bash
make test
```

### Signed test builds on macOS

Repeated unsigned builds can cause a new Keychain prompt for the saved Linear
credential. For live development testing, the owner can create a local code
signing certificate in Keychain Access and keep its private key on this Mac.
After each build, sign the executable with the same identity and the fixed
`org.agent-ix.ix-board.test` identifier:

```bash
cargo build
IX_BOARD_SIGNING_IDENTITY="<local code signing identity>" \
  bash scripts/sign_test_build.sh target/debug/ix-board
```

The script verifies the signature and prints its designated requirement. Run
the signed binary for `auth status` and grant its first Keychain request only
after checking the signing identity. Rebuilds overwrite the signature, so sign
each new build before using the saved credential. If the same identity still
causes repeated prompts, stop live dev-binary tests and report the signature
and prompt behavior without disclosing the credential. No signing identity is
created or imported by the script.

## Read commands

Save a Linear API key in the operating system credential store, then run:

```bash
ix-board auth login
ix-board auth status
```

The prompt hides the key. For scripts, pipe the key to `ix-board auth login --stdin`.
`LINEAR_API_KEY` overrides the saved key without changing it. `ix-board auth
logout` removes the saved key. `auth status` only reports whether a key is
available and which source is active; it never displays the value. If the OS
credential store is locked or unavailable, the CLI reports that failure and
does not create a plaintext fallback file.

Run reads with:

```bash
ix-board next --team SWM
ix-board project-chunk SWM-P1
ix-board search "dependency graph"
ix-board issues SWM-12 SWM-13 SWM-14
```

For a versioned focus record that another client can validate and store as
JSON, run `ix-board --json focus report --initiative "Initiative name"`.
The record includes canonical Linear IDs, the report time, and ix-board's
status report. This command does not publish to ix-relay.

Add `--json` to any command for machine-readable output. The CLI stores one
workspace snapshot in SQLite for five minutes. The default cache is the
platform's application cache directory; `IX_BOARD_CACHE_PATH` can select a
specific database path.

Library consumers can call `ReadService::workspace_snapshot` for the cached
workspace, `force_refresh_workspace` to fetch through the five-minute cache,
and `invalidate_cache` to discard a snapshot before a write. A failed forced
refresh leaves no cached snapshot for subsequent reads.

## Agent plugin

The standalone ix-board plugin provides planning and early-alpha feedback
skills for Claude Code and Codex. The `focus-status`, `focus-hygiene`, and
`plan-chunk` skills live here with the commands and report schema they use;
team lead and planner role skills consume their results. Install the CLI
separately:

```bash
cargo +1.98.1 install --git https://github.com/agent-ix/ix-board --tag v0.2.1 --locked
```

For Claude Code:

```text
/plugin marketplace add agent-ix/agent-plugins
/plugin install ix-board@agent-ix
```

For Codex:

```bash
codex plugin marketplace add agent-ix/agent-plugins
codex plugin add ix-board@agent-ix
```

The planning skill uses the CLI's structured `next`, `project-chunk`, `search`,
and `focus` reports. Live reads require a Linear key saved with `ix-board auth
login` or supplied through `LINEAR_API_KEY`.

## Drift report

`drift` evaluates a versioned evidence file against an explicit lead and
threshold configuration. It does not guess attribution from the shared GitHub
account and does not call an LLM:

```bash
ix-board drift --config drift-config.json --evidence drift-evidence-v1.json
ix-board drift --config drift-config.json --evidence drift-evidence-v1.json --json
```

The config JSON is a strict `DriftConfig` object with `report_time` (RFC 3339),
`window_hours`, `stall_hours`, `spend_threshold`,
`max_net_ticket_growth`, `min_merged_prs_for_churn`, and an ordered `leads`
array of `{ "lead_id": "...", "team_key": "..." }` values. Time-window and
stall hours must be positive; spend and churn thresholds must be nonnegative;
each team can belong to only one configured lead.

The evidence JSON envelope is strict and versioned:

```json
{
  "schema_version": 1,
  "evidence": {
    "projects": [],
    "tickets": [],
    "transitions": [],
    "pull_requests": [],
    "commits": [],
    "herdr_busy": { "lead-id": false },
    "otel_spend": { "lead-id": 0.0 },
    "missing": []
  }
}
```

Each record uses the corresponding strict, typed struct in the public Rust API.
Project and issue snapshots must be complete for the configured leads.
`transitions` carry a `queue_before` snapshot for each Coding transition,
including the historical project/ticket ordering, spec and workflow states,
and blocker edges. `queue_before.missing_inputs` names any historical field
that could not be reconstructed. Pull request and commit records must cover
all configured repositories and the whole reporting interval (plus PRs merged
in the interval even if opened earlier); no pagination or partition may be
silently omitted. Add `missing` entries such as
`{"source":"github.prs","reason":"page 3 cursor unresolved"}` for every
source page, partition, or history query that is incomplete. Applicable signals
remain visible but become `unavailable`; their counts are `null` and the report
cannot claim `on_track` from partial evidence. Omit a lead key from
`herdr_busy` or `otel_spend` when that observation is unavailable. An explicit
observed spend of zero is not activity, including when `spend_threshold` is
zero.

The evidence contract is the source boundary for drift v1. ix-board does not
fetch GitHub or historical Linear data for this command because this
repository has no configured repository list or complete historical queue
source. A collector must supply structured API records and pagination/history
completeness; the evaluator fails closed when it reports missing evidence.

## JSON Schema for downstream clients

The public project, ready-queue, focus-report, and drift wire types derive `schemars::JsonSchema`
from their authoritative Rust definitions. Export a schema for a downstream
contract build with:

```bash
cargo run --example export_schema -- Project > project.schema.json
cargo run --example export_schema -- ProjectChunk > project-chunk.schema.json
cargo run --example export_schema -- ReadyItem > ready-item.schema.json
cargo run --example export_schema -- DriftReport > drift-report.schema.json
cargo run --example export_schema -- FocusStatus > focus-status.schema.json
cargo run --example export_schema -- FocusRecord > focus-record.schema.json
cargo run --example export_schema -- FocusHygieneReport > focus-hygiene.schema.json
cargo run --example export_schema -- FocusPlanChunk > focus-plan-chunk.schema.json
```

The schemas describe the serialized wire shape, including flattened signal
availability fields and enum values. Downstream projects can generate
TypeScript declarations from these schemas without copying the Rust models.

## License

AGPL-3.0-or-later
