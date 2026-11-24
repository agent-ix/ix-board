---
id: FR-044
title: Record orchestrator cycles and minder dialog answers in the fleet ledger
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-038"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-039"
    type: depends_on
---
# FR-044: Record orchestrator cycles and minder dialog answers in the fleet ledger

## Description

ix-board SHALL give the orchestrator a `fleet cycle` command that records the start and end of each orchestrator cycle, and the minder a `fleet answer-log record` command that records every dialog answer it sends, both in the fleet ledger ([FR-039](./FR-039-fleet-ledger.md)). The dev-team role specs count cycles from the cycle events and find unauthorized answers from the answer log. No other requirement owns either command.

## Behavior

### Cycle events

`fleet cycle start --session <addr> [--interval <duration>]` writes a `cycle_event` row with a new `cycle_id`, the session address, `started_at` and `interval_s` (the configured orchestrator loop interval, default 10 minutes, when `--interval` is absent). `fleet cycle end` closes the session's open cycle with `ended_at`. A `start` while a cycle of the same session is open closes it first with `ended_at` set to the new `started_at` and `ended_reason` `superseded`. Cycle counts in evaluations are counted from these rows ([FR-039](./FR-039-fleet-ledger.md) `open_cycles`).

### Dialog answer log

The answer log is a ledger table, not a relay store. The relay keeps transport records of bounded age (seven days, bounded pages) and does not know standing orders, which are ix-board configuration (the SWM-226 file); the ledger already holds the actions, lane classes and retention rule that the evaluator joins an answer to, so a violation is found from one store.

`fleet answer-log record --lane <addr> --action <action id> --class <dialog class> [--order <standing-order id>]` is called by the minder for each answer or keystroke it sends to a lane pane, and writes a `dialog_answer` row: `answer_id`, lane, `dialog_class` (`dialog`, `permission`, `question`, `login`, `usage_limit`, `other`), `action_id`, `standing_order_id` (empty when none), `answered_at`, `lane_class_at_answer` (the lane's class in the last committed run at that time), `answer_text` and `answer_len`, and `violation`. The command validates the claim against the ledger and the SWM-226 file:

- `standing_order_id` is valid only when the order exists, matches the dialog, and the action `action_id` is an `answer_dialog` action carrying that order.
- `violation` is `no_standing_order` when no valid order is given, and `login_input` when `lane_class_at_answer` is `BLOCKED(login)` or `BLOCKED(usage_limit)`, whatever order is given. A row can carry both, recorded as `no_standing_order,login_input`; otherwise it is empty.
- `answer_text` is stored only when it equals the answer configured by the matching standing order. For any answer with a violation the text is not stored and only `answer_len` is, so a login credential is never written to the ledger.

The command records the row and exits zero even for a violation, so the minder's action is logged first; a violation is detectable afterwards through the views below. Rows are retained under the ledger retention.

### Role-spec query views

| View (`ledger.<name>`) | Definition |
|------|------------|
| `answer_dialog_latency_max` | maximum of `answered_at - emitted_at` over `answer_dialog` actions that have a `dialog_answer` row, in tick intervals; an action with none is measured to the evaluation time |
| `dialog_answer_violations` | `dialog_answer` rows with a non-empty `violation` |
| `cycle_events` | `cycle_event` rows |

The transcript queries `transcript.dialog_answers_missing_from_answer_log` and `transcript.answers_sent_while_escalate_block_open` read the minder transcript and join the `dialog_answer` and `action` tables.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-044-AC-1 | `fleet cycle start` then `end` writes one `cycle_event` with the session, both times and `interval_s` 600 by default and the given value with `--interval`. | Test |
| FR-044-AC-2 | A second `start` of the same session while a cycle is open closes the first with `ended_reason` `superseded` at the new start, and another session's open cycle is untouched. | Test |
| FR-044-AC-3 | An answer for an `answer_dialog` action whose standing order matches is recorded with the order id, the answer text, the lane class `BLOCKED(dialog)` and an empty `violation`. | Test |
| FR-044-AC-4 | An answer with no order, or an order that does not match the dialog or the action, is recorded with `violation` `no_standing_order`, stores no text, stores `answer_len`, and appears in `dialog_answer_violations`; the command exits zero. | Test |
| FR-044-AC-5 | Any answer recorded while the lane's class is `BLOCKED(login)` or `BLOCKED(usage_limit)` is `login_input`, including one that cites a valid standing order, and stores no text. | Test |
| FR-044-AC-6 | `answer_dialog_latency_max` equals the hand-computed maximum on a fixture and measures an unanswered action to the evaluation time; `answer_dialog_actions` and `dialog_answer` rows join on `action_id`. | Test |
| FR-044-AC-7 | `dialog_answer` rows survive until the ledger retention age and are pruned with other rows, and a ledger inside a git work tree is still refused. | Test |

## Dependencies

- [FR-038](./FR-038-fleet-tick.md) `answer_dialog` and `escalate_block` actions; [FR-039](./FR-039-fleet-ledger.md) ledger and retention. Standing orders come from the SWM-226 configuration file. The minder rewrite (SWM-225) calls `fleet answer-log record`.
