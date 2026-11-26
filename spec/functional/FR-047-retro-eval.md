---
id: FR-047
title: Evaluate role rules from source metrics and session evidence
type: FR
relationships:
  - target: "ix://agent-ix/dev-team/FR-017"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-024"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-025"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-026"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-029"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-033"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-041"
    type: depends_on
---
# FR-047: Evaluate role rules from source metrics and session evidence

## Description

`ix-board retro eval --json` SHALL evaluate deterministic role rules over an
explicit team and time window, return team, role, rule and session results, and
show the change from the immediately preceding window of equal length. It SHALL
read role specifications, evaluator settings, retro records, Prometheus series,
the fleet ledger, tracker history, GitHub pull requests, relay receipts, hook
events and bounded session transcripts. It SHALL NOT infer a pass from missing
or partial evidence and SHALL NOT write to source systems.

## Behavior

- The team comes from the configured retro team unless `--team` selects it
  explicitly. The retro `config.json` remains independent of evaluator
  settings.
- Role specification markdown is read from `role_spec_dir` in
  `$TEAM_RETRO_HOME/<team>/eval.json`. When omitted, `role_spec_dir` defaults
  to `$HOME/.local/share/ix-board/role-specs`. Every supported role spec and every rule in its
  evaluation table is represented. Unknown query references are `unavailable`
  with the reference named; no reference is silently skipped.
- Evaluator settings are read only from `eval.json`: `review_age_limit_hours`
  defaults to 1, `gate_commands` defaults to `make ci`, and absent
  `ready_state` makes queries that require a ready state unavailable.
  `github_guard_state_path` names the shared team-metrics GitHub request journal;
  GitHub queries are unavailable when it is absent. Its byte budget defaults to
  64 MiB.
- `--since` accepts a bounded duration and defaults to 24 hours. The comparison
  window is the immediately preceding interval of the same length. Evidence is
  selected by source event time; observation time is retained for freshness and
  coverage decisions.
- Each deterministic evaluation carries the query, observed value, comparator,
  target, result (`pass`, `fail`, `not_applicable` or `unavailable`), reason
  when not evaluable, and source evidence references. Only `pass` and `fail`
  enter the score denominator. A query with partial source coverage is
  `unavailable`, not a partial value.
- A ledger count or aggregate is evaluated only when the ledger has a collector
  run in the event-time window and its requested stable view is present. An
  empty view without that coverage is `unavailable`, not a zero value. The
  capacity views also require a capacity data row in the window; a generic
  fleet collector run alone does not certify an empty capacity view. The
  evaluator reads ledger schema versions 3 through 10 without migrating them;
  an unknown schema version is `unavailable`.
- Capacity rules use the SWM-252 view name `unrecorded_hosts_with_demand`.
  Demand is covered when `ready_buffer` is at least the host's lane count; a
  positive target-buffer deficit does not make that demand unavailable. The
  inconclusive-window reason for uncovered demand is `demand_short`.
- Judgement-only evaluations are returned with their rubric id and
  `judgement_required=true`; they are excluded from deterministic scores and
  marked for the director.
- Session results contain a row for every applicable rule. Team-level retro
  and Prometheus values are `unavailable` for a session unless that scope and
  window contain exactly one session of the evaluated role. A transcript that
  is truncated, missing or cannot be attributed makes transcript queries
  unavailable for that session.
- Every role and session result contains an `objective_metrics` array. Each item
  has `id`, `query`, numeric-or-null `value`, `status` using the rule status
  enum, `unit`, and `evidence_refs`; a null value always has a `reason`.
  Missing or partial source data is never reported as numeric zero. Session
  objective metrics use the same attribution rule as team-level rule values.
- Role results report deterministic pass rate by rule, the current and prior
  window scores, the direction of change, and failing rule ids. Trend compares
  only the same deterministic rule ids with pass/fail outcomes in both windows;
  unavailable and not-applicable rules do not prevent comparing the remaining
  matched rules. `trend_matched_rules` reports that intersection size. Trend is
  unavailable only when no rule is scored in both windows.
- The JSON document has `schema_version`, `team`, `window`, `previous_window`,
  `roles`, `sessions`, and `source_requests`. The request object reports the
  Linear and Prometheus HTTP attempts plus GitHub CLI calls and observed
  response pages and shared GitHub guard queries for the invocation; GitHub CLI
  retries can make page counts a lower bound. A role row has `role`, `score`, `previous_score`,
  `trend`, `trend_matched_rules`, `rules`,
  `failing_rules`, and its session ids. Each role and session row also has
  `objective_metrics`; each metric item has `id`, `query`, `value`, `status`,
  `unit`, `evidence_refs`, and a required `reason` when `value` is null. A rule
  row has `id`, `query`, `status`,
  `value`, `comparator`, `target`, `evidence_refs`, and optional `reason` or
  `rubric_id`. A session row has `id`, `role`, `score`, `rules`, and
  `failing_rules`.
- Text and JSON render the same evaluations. `--role` and `--session` filter
  presentation only; they do not alter source populations or rule semantics.
- The evaluator reads stored retro records once for both event-time windows and
  makes at most one additional live retro snapshot for the current window. It
  emits stage start and elapsed-time messages to stderr. Source failures are
  represented by unavailable rule rows with reasons, preserving partial JSON.

## Acceptance Criteria

| ID | Criteria | Verification |
|---|---|---|
| FR-047-AC-1 | Given role specs containing deterministic, judgement-only and unknown query references, JSON has one result per rule; deterministic status and evidence follow the query registry, judgement-only rows are marked for the director and excluded from scores, and unknown references are unavailable with their names. | Test |
| FR-047-AC-2 | A hand-computed fixture with passing, failing, not-applicable and unavailable rules yields the expected per-rule rows, role score, failing rule ids and session rows; unavailable and not-applicable rows do not enter the pass-rate denominator. | Test |
| FR-047-AC-3 | The previous score uses the immediately preceding equal-duration window. Trend is the signed change over deterministic rule ids scored in both windows, with the matched count reported; an unavailable rule does not erase a comparison of other matched rules, and no matched rules makes trend unavailable. | Test |
| FR-047-AC-4 | `review_age_limit_hours`, `gate_commands` and `ready_state` are loaded from `eval.json`; defaults and missing `ready_state` follow the behavior above, and `config.json` is unchanged. | Test |
| FR-047-AC-5 | Session query rows use source event time, explicit session attribution and complete transcript boundaries; a missing session, ambiguous role session, truncated transcript or unreadable source is unavailable rather than pass or zero. | Test |
| FR-047-AC-6 | Every role evaluation query in the configured role specs resolves to a registered source adapter; every unknown source or query returns an unavailable row with a diagnostic and no panic. Ledger values require event-window collector coverage and a present view; capacity views also require an event-window capacity row; uncovered empty views are unavailable, and unknown schema versions are refused. | Test |
| FR-047-AC-7 | `--team`, `--role`, `--session`, `--since` and `--json` produce the documented scope and stable JSON schema; text and JSON carry the same values and statuses. | Test |
| FR-047-AC-8 | Evaluation is read-only: a run does not modify retro snapshots, the fleet ledger, tracker, GitHub, relay, hook events or transcripts. | Test |
| FR-047-AC-9 | Evaluation reuses stored retro records across windows, reports attempted Linear/GitHub/Prometheus request counts in JSON, and writes stage timings to stderr while preserving partial results when a source is unavailable. | Test |
| FR-047-AC-10 | The evaluator reads ledger schema versions 3 through 10 without migrating them and refuses versions outside that range. | Test |

## Dependencies

- Role and marker definitions: dev-team FR-017 and the configured role
  specification directory.
- Retro records: FR-024, FR-025, FR-026 and FR-029.
- Session transcript and hook evidence: role-attributed hook turns and bounded
  transcripts. Relay receipt and hook protocols remain owned by ix-relay.
- Exported team metrics: FR-033. Fleet ledger queries and retained events:
  FR-039. Relay receipt and hook event protocols remain owned by ix-relay.
- SWM-261 and the fleet lane own their ledger producers and schema versions:
  capacity schema v6 in #102, role views v7, transcript ids v8, stream
  cursors v9 in #103 and cadence v10. SWM-221 reads their stable views without migration and
  accepts schema versions 3 through 10; a future retro-owned persistent
  producer would require the next schema version.
- Fleet capacity decisions and views: FR-041, including demand coverage for
  the orchestrator's discovery rules.
