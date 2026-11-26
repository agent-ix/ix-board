# SR-6150 — PR #102 capacity history code review

Redacted review record. Raw host labels, ticket identifiers, local paths, endpoints,
timestamps and metric values are omitted to keep live fleet data out of the public repo.

Reviewed head: `f25d380`.

## Accepted findings tracked in this fix

- FND-012: unavailable knee evidence must remain unavailable.
- FND-018: schema-2 read compatibility and writer/version attribution.
- FND-020: drain marks must be idempotent and status must deduplicate lanes.
- FND-031/032/049: partial output reads must not become zeros; normalize and deduplicate
  assignment keys, including aggregate rows.
- FND-034/035: compare sustained slot occupancy and calculate coverage without series
  multiplication.
- FND-036/037/038: validate drain evidence, record overload transitions, and omit
  unsupported zero counters.
- FND-039/040/041/043/044/045/046: make history views respect repeated windows, run
  bounds, event order, configured timing, exact sample windows, and fresh per-lane
  drain evidence; isolate unavailable lanes and record lane loss.
- FND-050: `cargo clippy --all-targets -- -D warnings` must pass.
- FND-051: remove machine-specific paths and raw live acceptance data from public files.

Earlier accepted findings are retained in the originating review record; these
dispositions are summarized here without copying live examples.
