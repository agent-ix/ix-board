// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The ledger schema (FR-039): versioned tables, additive migrations, and read-only views.
//!
//! Times are relay server milliseconds. Tables whose writers are other tickets are created
//! empty so the schema is one unit under one migration.

use ix_trace_rs::implements;

/// The schema version this build writes.
pub const VERSION: i64 = 9;

pub const V1: &str = "
CREATE TABLE run(
  sequence INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, relay_ms INTEGER);
CREATE TABLE tick(
  id INTEGER PRIMARY KEY AUTOINCREMENT, started_at INTEGER NOT NULL, committed_at INTEGER);
CREATE TABLE lane_interval(
  id INTEGER PRIMARY KEY AUTOINCREMENT, lane TEXT NOT NULL, host TEXT NOT NULL,
  class TEXT NOT NULL, reason TEXT, started_at INTEGER NOT NULL, ended_at INTEGER, focus TEXT);
CREATE TABLE action(
  id INTEGER PRIMARY KEY AUTOINCREMENT, tick_id INTEGER, kind TEXT NOT NULL, lane TEXT,
  emitted_at INTEGER NOT NULL, executed_at INTEGER, resolved_at INTEGER);
CREATE TABLE defect(
  defect_id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT NOT NULL, host TEXT, lane TEXT,
  kind TEXT NOT NULL, first_seen_at INTEGER);
CREATE TABLE assignment_history(
  id INTEGER PRIMARY KEY AUTOINCREMENT, lane TEXT NOT NULL, ticket TEXT,
  assigned_at INTEGER NOT NULL, replaced_at INTEGER);
CREATE TABLE ground_truth_sample(
  lane TEXT NOT NULL, at INTEGER NOT NULL, active INTEGER NOT NULL, PRIMARY KEY(lane, at));
CREATE TABLE calibration_run(
  id INTEGER PRIMARY KEY AUTOINCREMENT, started_at INTEGER NOT NULL, ended_at INTEGER,
  detail TEXT);
CREATE TABLE audit_sample(
  id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, lane TEXT, detail TEXT);
CREATE TABLE saturation_sample(
  id INTEGER PRIMARY KEY AUTOINCREMENT, tick_id INTEGER, host TEXT NOT NULL,
  at INTEGER NOT NULL, deficit INTEGER);
";

/// Version 2 statements, applied one by one inside one transaction. The migration is additive:
/// it adds columns and tables and never rewrites a row.
pub const V2: &[&str] = &[
    "ALTER TABLE run ADD COLUMN host_ms INTEGER",
    "ALTER TABLE run ADD COLUMN relay_unavailable INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE run ADD COLUMN tick_interval_s INTEGER",
    "ALTER TABLE run ADD COLUMN cycle_interval_s INTEGER",
    "ALTER TABLE run ADD COLUMN grace_s INTEGER",
    "ALTER TABLE defect ADD COLUMN first_seen_host_ms INTEGER",
    "ALTER TABLE defect ADD COLUMN closed_at INTEGER",
    "ALTER TABLE defect ADD COLUMN routed_ticket TEXT",
    "ALTER TABLE defect ADD COLUMN routed_at INTEGER",
    "ALTER TABLE action ADD COLUMN severity TEXT",
    "ALTER TABLE action ADD COLUMN due_by INTEGER",
    "ALTER TABLE action ADD COLUMN target_role TEXT",
    "ALTER TABLE action ADD COLUMN target_addr TEXT",
    "ALTER TABLE action ADD COLUMN condition_started_at INTEGER",
    "ALTER TABLE action ADD COLUMN parent_action_id INTEGER",
    "ALTER TABLE action ADD COLUMN rung INTEGER",
    "ALTER TABLE assignment_history ADD COLUMN order_id TEXT",
    "ALTER TABLE assignment_history ADD COLUMN expect_ticket TEXT",
    "ALTER TABLE assignment_history ADD COLUMN sent_at INTEGER",
    "ALTER TABLE assignment_history ADD COLUMN deadline_at INTEGER",
    "ALTER TABLE assignment_history ADD COLUMN result TEXT CHECK(result IS NULL OR result IN \
     ('pending','confirmed','acted_not_focused','ack_expired','timed_out'))",
    "ALTER TABLE assignment_history ADD COLUMN result_at INTEGER",
    "ALTER TABLE assignment_history ADD COLUMN confirm_latency_s INTEGER",
    "ALTER TABLE assignment_history ADD COLUMN focus_at_deadline TEXT",
    "ALTER TABLE tick ADD COLUMN session_addr TEXT",
    "ALTER TABLE tick ADD COLUMN session_role TEXT",
    "CREATE TABLE lane_tick(
       id INTEGER PRIMARY KEY AUTOINCREMENT,
       run_seq INTEGER NOT NULL REFERENCES run(sequence),
       tick_id INTEGER,
       source TEXT NOT NULL DEFAULT 'status' CHECK(source IN ('status','tick')),
       lane TEXT NOT NULL, host TEXT NOT NULL, class TEXT NOT NULL, reason TEXT,
       remaining_assigned INTEGER, ready_work_available INTEGER,
       observed_ms INTEGER NOT NULL, last_event_ms INTEGER, evidence TEXT NOT NULL)",
    "CREATE INDEX lane_tick_lane_time ON lane_tick(lane, observed_ms)",
    "CREATE INDEX lane_tick_time ON lane_tick(observed_ms)",
    "CREATE INDEX run_relay_ms ON run(relay_ms)",
    "CREATE TABLE host_tick(
       run_seq INTEGER NOT NULL REFERENCES run(sequence), host TEXT NOT NULL,
       observed_ms INTEGER NOT NULL, lanes INTEGER NOT NULL, working INTEGER NOT NULL,
       not_working INTEGER NOT NULL, PRIMARY KEY(run_seq, host))",
    "CREATE INDEX host_tick_time ON host_tick(observed_ms)",
    "CREATE TABLE plan_coverage_tick(
       tick_id INTEGER PRIMARY KEY, unowned_ready_count INTEGER, short_queue_count INTEGER,
       stale_assignment_count INTEGER, unavailable_lists TEXT)",
    "CREATE TABLE plan_coverage_gap(
       gap_id INTEGER PRIMARY KEY AUTOINCREMENT,
       kind TEXT NOT NULL CHECK(kind IN ('unowned_ready','short_queue','stale_assignment')),
       subject TEXT NOT NULL, detail TEXT, opened_tick INTEGER NOT NULL, opened_at INTEGER NOT NULL,
       closed_tick INTEGER, closed_at INTEGER)",
    "CREATE TABLE drift_finding(
       finding_id INTEGER PRIMARY KEY AUTOINCREMENT, lane TEXT NOT NULL, rule TEXT NOT NULL,
       focus TEXT, opened_tick INTEGER, opened_at INTEGER NOT NULL, closed_at INTEGER,
       evidence_times TEXT)",
    "CREATE TABLE order_focus_at_deadline(
       order_id TEXT PRIMARY KEY,
       sender_role TEXT NOT NULL CHECK(sender_role IN ('orchestrator','minder')),
       sender_addr TEXT NOT NULL, lane TEXT NOT NULL, expect_ticket TEXT NOT NULL,
       deadline_at INTEGER NOT NULL, focus_ticket TEXT, focus_state TEXT,
       outcome TEXT NOT NULL CHECK(outcome IN
         ('confirmed','acted_not_focused','ack_expired','timed_out')),
       followup_at INTEGER)",
    "CREATE TABLE action_send(
       action_id INTEGER NOT NULL, sent_at INTEGER NOT NULL, sender_addr TEXT NOT NULL,
       recipient_addr TEXT NOT NULL, receipt_id TEXT NOT NULL, PRIMARY KEY(action_id, receipt_id))",
    "CREATE TABLE session_turn(
       session_addr TEXT NOT NULL, session_role TEXT NOT NULL, started_at INTEGER NOT NULL,
       PRIMARY KEY(session_addr, started_at))",
    "CREATE TABLE cycle_event(
       cycle_id INTEGER PRIMARY KEY AUTOINCREMENT, session_addr TEXT NOT NULL,
       started_at INTEGER NOT NULL, ended_at INTEGER,
       ended_reason TEXT CHECK(ended_reason IS NULL OR ended_reason IN ('ended','superseded')),
       interval_s INTEGER NOT NULL)",
    "CREATE TABLE cycle_missed(
       session_addr TEXT NOT NULL, last_ended_at INTEGER NOT NULL, expected_by INTEGER NOT NULL,
       detected_at INTEGER NOT NULL, PRIMARY KEY(session_addr, last_ended_at))",
    "CREATE TABLE dialog_answer(
       answer_id INTEGER PRIMARY KEY AUTOINCREMENT, lane TEXT NOT NULL,
       dialog_class TEXT NOT NULL CHECK(dialog_class IN
         ('dialog','permission','question','login','usage_limit','other')),
       action_id INTEGER, standing_order_id TEXT, answered_at INTEGER NOT NULL,
       lane_class_at_answer TEXT, answer_text TEXT, answer_len INTEGER NOT NULL, violation TEXT)",
    "CREATE TABLE capacity_run(
       run_id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT NOT NULL, trigger TEXT,
       started_at INTEGER NOT NULL, ended_at INTEGER,
       end_reason TEXT CHECK(end_reason IS NULL OR end_reason IN
         ('knee','build_slots_saturated','step_bound','inconclusive_no_demand','throttled')),
       found_level INTEGER)",
    "CREATE TABLE capacity_step(
       run_id INTEGER NOT NULL, step_no INTEGER NOT NULL, repeat_of INTEGER, host TEXT NOT NULL,
       lanes INTEGER, window_start INTEGER, window_end INTEGER, rampup_end INTEGER, valid INTEGER,
       inconclusive_reason TEXT CHECK(inconclusive_reason IS NULL OR inconclusive_reason IN
         ('demand_short','deficit_coverage')),
       deficit_coverage REAL, deficit_zero_throughout INTEGER, host_output_per_hour REAL,
       tickets_closed INTEGER, prs_merged INTEGER, commits_pushed INTEGER,
       output_basis TEXT CHECK(output_basis IS NULL OR output_basis IN ('tickets_prs','commits')),
       queue_wait_p90_s REAL, throttled_share REAL, memory_headroom_min REAL,
       thermal_headroom_min REAL, slots_busy_equals_sustainable INTEGER, knee_signal INTEGER,
       knee_reason TEXT CHECK(knee_reason IS NULL OR knee_reason IN ('output_gain','throttle')))",
    "CREATE TABLE capacity_step_lane_output(
       run_id INTEGER NOT NULL, step_no INTEGER NOT NULL, lane TEXT NOT NULL,
       tickets_closed INTEGER, prs_merged INTEGER, commits_pushed INTEGER)",
    "CREATE TABLE capacity_level(
       host TEXT NOT NULL, level INTEGER NOT NULL,
       output_basis TEXT CHECK(output_basis IS NULL OR output_basis IN ('tickets_prs','commits')),
       recorded_at INTEGER NOT NULL, run_id INTEGER, reason TEXT, evidence_steps TEXT,
       superseded_at INTEGER)",
    "CREATE TABLE capacity_event(
       event_id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT NOT NULL, lane TEXT,
       kind TEXT NOT NULL CHECK(kind IN ('lane_added','draining_mark','drain_done',
         'lane_removed','lane_lost','step_down','overload_event')),
       reason TEXT, evidence_kind TEXT, cycle_id INTEGER, at INTEGER NOT NULL,
       ceiling_until INTEGER)",
    "CREATE TABLE host_output_sample(
       host TEXT NOT NULL, at INTEGER NOT NULL, lanes INTEGER, tickets_closed INTEGER,
       prs_merged INTEGER, commits_pushed INTEGER, PRIMARY KEY(host, at))",
];

/// Version 7 adds order/session attribution and role-spec metric producer tables.
pub const V7: &[&str] = &[
    "ALTER TABLE order_focus_at_deadline RENAME TO order_focus_at_deadline_v2",
    "CREATE TABLE order_focus_at_deadline(
       order_id TEXT PRIMARY KEY,
       sender_role TEXT NOT NULL CHECK(sender_role IN ('orchestrator','minder','other')),
       sender_addr TEXT NOT NULL, lane TEXT NOT NULL, expect_ticket TEXT NOT NULL,
       sent_at INTEGER,
       deadline_at INTEGER NOT NULL, focus_ticket TEXT, focus_state TEXT,
       outcome TEXT NOT NULL CHECK(outcome IN
         ('confirmed','acted_not_focused','ack_expired','timed_out')),
       followup_at INTEGER)",
    "INSERT INTO order_focus_at_deadline(
       order_id,sender_role,sender_addr,lane,expect_ticket,deadline_at,focus_ticket,focus_state,
       outcome,followup_at)
     SELECT order_id,sender_role,sender_addr,lane,expect_ticket,deadline_at,focus_ticket,focus_state,
       outcome,followup_at FROM order_focus_at_deadline_v2",
    "DROP TABLE order_focus_at_deadline_v2",
    "ALTER TABLE action_send ADD COLUMN sender_role TEXT",
    "CREATE TABLE lane_lead(
       lane TEXT NOT NULL, lead_addr TEXT NOT NULL, observed_at INTEGER NOT NULL,
       PRIMARY KEY(lane, observed_at))",
    "CREATE TABLE failed_probe_tick(
       defect_id INTEGER NOT NULL, source TEXT NOT NULL, host TEXT, tick_id INTEGER,
       at_ms INTEGER NOT NULL, PRIMARY KEY(defect_id, at_ms))",
    "CREATE TABLE relay_order_snapshot(
       snapshot_id INTEGER PRIMARY KEY AUTOINCREMENT, observed_at INTEGER NOT NULL,
       window_since INTEGER NOT NULL, window_until INTEGER NOT NULL,
       oldest_available_at INTEGER, available INTEGER NOT NULL CHECK(available IN (0,1)),
       unavailable_reason TEXT, skipped INTEGER NOT NULL DEFAULT 0)",
    "CREATE TABLE relay_order(
       snapshot_id INTEGER NOT NULL REFERENCES relay_order_snapshot(snapshot_id),
       order_id TEXT NOT NULL, sender TEXT NOT NULL, recipient TEXT NOT NULL,
       receipt_target TEXT NOT NULL, expect_ticket TEXT, ack_ms INTEGER NOT NULL,
       deadline_at INTEGER NOT NULL, enqueued_at INTEGER NOT NULL,
       delivered_at INTEGER, acted_at INTEGER, state TEXT NOT NULL, expired INTEGER NOT NULL,
       PRIMARY KEY(snapshot_id, order_id))",
    "CREATE INDEX relay_order_snapshot_deadline ON relay_order(snapshot_id, deadline_at)",
    "ALTER TABLE assignment_history ADD COLUMN sender_role TEXT",
    "ALTER TABLE assignment_history ADD COLUMN sender_addr TEXT",
    "CREATE UNIQUE INDEX action_send_receipt_unique ON action_send(receipt_id)",
    "CREATE TABLE host_build_pause_interval(
       id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT NOT NULL,
       start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL CHECK(end_ms >= start_ms))",
    "CREATE INDEX host_build_pause_interval_host_time
       ON host_build_pause_interval(host, start_ms, end_ms)",
];

/// Version 8 adds the relay-authored transcript identity to prompt-turn rows.
pub const V8: &[&str] = &[
    "ALTER TABLE session_turn ADD COLUMN transcript_id TEXT",
    "DROP VIEW IF EXISTS session_turn_rows",
    "CREATE VIEW session_turn_rows AS
       SELECT session_addr,session_role,started_at,transcript_id,started_at AS at_ms
       FROM session_turn",
];

/// Version 9 records the durable position and historical coverage of prompt-turn ingestion.
pub const V9: &[&str] = &[
    "CREATE TABLE session_turn_session_role(
       session_addr TEXT NOT NULL, transcript_id TEXT NOT NULL,
       session_role TEXT, PRIMARY KEY(session_addr,transcript_id))",
    "CREATE TABLE session_turn_stream_state(
       singleton INTEGER PRIMARY KEY CHECK(singleton=1),
       cursor_ms INTEGER NOT NULL, cursor_sequence INTEGER NOT NULL,
       coverage_start_ms INTEGER, incomplete_before_ms INTEGER,
       last_read_ms INTEGER NOT NULL, events_read INTEGER NOT NULL DEFAULT 0,
       unreadable_events INTEGER NOT NULL DEFAULT 0, pages_read INTEGER NOT NULL DEFAULT 0,
       retention_gaps INTEGER NOT NULL DEFAULT 0, latest_gap_start_ms INTEGER)",
    "CREATE VIEW session_turn_stream_coverage AS
       SELECT cursor_ms,cursor_sequence,coverage_start_ms,incomplete_before_ms,
              last_read_ms,events_read,unreadable_events,pages_read,retention_gaps,
              latest_gap_start_ms,
              (SELECT COUNT(*) FROM session_turn_session_role WHERE session_role IS NOT NULL)
                AS role_mappings,
              CASE WHEN coverage_start_ms IS NULL THEN 'no_events_seen'
                   WHEN retention_gaps > 0 THEN 'incomplete_possible_retention_gap'
                   WHEN unreadable_events > 0 THEN 'incomplete_unreadable_events'
                   WHEN incomplete_before_ms IS NOT NULL THEN 'incomplete_before_first_observed_event'
                   ELSE 'observed_since_stream_start' END AS status,
              last_read_ms AS at_ms
       FROM session_turn_stream_state WHERE singleton=1",
];

/// Views replaced when installing the version 7 definitions.
pub const V7_DROP_VIEWS: &[&str] = &[
    "lane_tick_rows",
    "lanes_with_ready_work",
    "idle_lane_with_ready_work_ticks",
    "idle_lane_episodes_with_ready_work",
    "idle_lane_episode_with_ready_work_max",
    "lane_span",
    "idle_minutes",
    "defect_rows",
    "idle_capacity_minutes_with_ready_work",
    "time_to_action_p90",
    "missed_idle_minutes",
    "detection_to_action_p90",
    "assignment_confirm_latency_p90",
    "drift_lane_minutes_share",
    "blocked_lane_minutes",
    "idle_lane_minutes_during_build_pause",
    "need_ready_tickets_actions",
    "need_ready_tickets_due",
    "need_ready_tickets_without_planner_order",
    "lane_orders_due",
    "lane_orders_not_taken_up",
    "failed_probe_ticks",
    "probe_failure_without_routed_defect",
    "plan_coverage_gap_rows",
    "plan_coverage_gap_open_cycles_max",
    "short_queue_open_cycles_max",
    "drift_findings",
    "drift_finding_open_cycles_max",
    "assignments_sent",
    "unconfirmed_assign_results",
    "unconfirmed_assignments_without_followup",
    "assignment_confirm_latency_max",
    "ladder_rungs_emitted",
    "escalation_rung",
    "orchestrator_rungs_emitted",
    "orchestrator_rung_open_cycles_max",
    "escalation_rung_send_latency_max",
    "escalation_rungs_suppressed",
    "add_lane_action_open_cycles_max",
    "act_on_undetermined_actions",
    "push_next_ticket_actions",
    "answer_dialog_actions",
    "escalate_block_actions",
    "add_lane_actions",
    "act_on_undetermined_unexecuted_ticks",
    "minder_addressed_actions",
    "tick_actions_unexecuted_by_minder",
    "minder_turns_without_committed_tick",
    "minder_orders_unconfirmed_at_deadline",
    "unconfirmed_orders_not_resent_or_escalated",
    "cycle_events",
    "dialog_answer_violations",
    "answer_dialog_latency_max",
];

/// Version 3 adds source provenance for every recorded capacity-step measurement.
pub const V3: &[&str] = &[
    "ALTER TABLE capacity_step ADD COLUMN source_basis TEXT NOT NULL DEFAULT '{}'",
    "ALTER TABLE capacity_step_lane_output ADD COLUMN source_basis TEXT NOT NULL DEFAULT '{}'",
];

/// Version 4 persists the currently open discovery hold so a later invocation can measure it.
pub const V4: &[&str] = &[
    "CREATE TABLE capacity_window(
       run_id INTEGER PRIMARY KEY, step_no INTEGER NOT NULL, repeat_of INTEGER,
       host TEXT NOT NULL, lanes INTEGER NOT NULL, lane_added_at_ms INTEGER NOT NULL,
       rampup_end_ms INTEGER NOT NULL)",
    "CREATE UNIQUE INDEX capacity_step_run_step ON capacity_step(run_id,step_no)",
    "CREATE UNIQUE INDEX capacity_step_lane_output_run_step_lane
       ON capacity_step_lane_output(run_id,step_no,lane)",
];

/// Version 5 persists scheduler slot values separately from their provenance.
pub const V5: &[&str] = &[
    "ALTER TABLE capacity_step ADD COLUMN build_slots_busy INTEGER",
    "ALTER TABLE capacity_step ADD COLUMN build_slots_sustainable INTEGER",
    "ALTER TABLE capacity_step ADD COLUMN measurement_values TEXT NOT NULL DEFAULT '{}'",
    "DROP INDEX capacity_step_run_step",
    "DROP INDEX capacity_step_lane_output_run_step_lane",
];

/// Version 6 stores the requested ticket count on planner buffer actions.
pub const V6: &[&str] = &[
    "ALTER TABLE action ADD COLUMN project TEXT",
    "ALTER TABLE action ADD COLUMN requested_count INTEGER",
    "CREATE TABLE IF NOT EXISTS capacity_throttle_sample(
       host TEXT NOT NULL, at INTEGER NOT NULL, interval_s INTEGER NOT NULL,
       throttled_share REAL, coverage REAL, unavailable_reason TEXT,
       source_basis TEXT NOT NULL, PRIMARY KEY(host, at))",
    "CREATE TABLE IF NOT EXISTS capacity_overload_state(
       host TEXT NOT NULL, cause TEXT NOT NULL, active INTEGER NOT NULL,
       updated_at INTEGER NOT NULL, source_basis TEXT NOT NULL,
       PRIMARY KEY(host,cause))",
    "ALTER TABLE capacity_event ADD COLUMN source_basis TEXT",
    "ALTER TABLE capacity_event ADD COLUMN drain_cap_ms INTEGER NOT NULL DEFAULT 14400000",
    "ALTER TABLE capacity_event ADD COLUMN ceiling_lanes INTEGER",
    "ALTER TABLE host_output_sample ADD COLUMN source_basis TEXT NOT NULL DEFAULT '{}'",
    "ALTER TABLE host_output_sample ADD COLUMN sample_interval_ms INTEGER NOT NULL DEFAULT 3600000",
    "ALTER TABLE capacity_step_lane_output ADD COLUMN window_end INTEGER",
    "ALTER TABLE capacity_step ADD COLUMN window_min_ms INTEGER NOT NULL DEFAULT 3600000",
    "UPDATE capacity_step_lane_output SET window_end=(
       SELECT s.window_end FROM capacity_step s WHERE s.run_id=capacity_step_lane_output.run_id
         AND s.step_no=capacity_step_lane_output.step_no LIMIT 1)
       WHERE window_end IS NULL AND (SELECT COUNT(*) FROM capacity_step s
         WHERE s.run_id=capacity_step_lane_output.run_id
           AND s.step_no=capacity_step_lane_output.step_no)=1",
    "ALTER TABLE saturation_sample ADD COLUMN source_basis TEXT NOT NULL DEFAULT 'unknown'",
    "ALTER TABLE saturation_sample ADD COLUMN unavailable_reason TEXT",
    "ALTER TABLE saturation_sample ADD COLUMN coverage REAL",
    "ALTER TABLE saturation_sample ADD COLUMN ready_buffer INTEGER",
    "ALTER TABLE saturation_sample ADD COLUMN active_lanes INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE saturation_sample ADD COLUMN supplied_lanes INTEGER",
    "ALTER TABLE saturation_sample ADD COLUMN unsupplied_lanes INTEGER",
    "ALTER TABLE saturation_sample ADD COLUMN ready_share INTEGER",
    "ALTER TABLE saturation_sample ADD COLUMN registry_current INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE saturation_sample ADD COLUMN queued_behind_first INTEGER",
    "ALTER TABLE saturation_sample ADD COLUMN bootstrap_ticket_pool INTEGER",
];

/// Capacity-history views consumed by the orchestrator and minder role specs.
pub const CAPACITY_VIEWS: &[&str] = &[
    "CREATE VIEW discovery_steps AS
       SELECT s.*, s.window_end AS at_ms FROM capacity_step s",
    "CREATE VIEW discovery_steps_violating_ramp AS
       WITH ordered AS (
         SELECT s.*, LAG(lanes) OVER (PARTITION BY run_id ORDER BY step_no) AS previous_lanes
         FROM capacity_step s)
       SELECT *, window_end AS at_ms FROM ordered
       WHERE repeat_of IS NULL AND
         ((previous_lanes IS NOT NULL AND lanes <> previous_lanes + 1)
          OR window_end - window_start < window_min_ms)",
    "CREATE VIEW discovery_steps_judged_without_demand AS
       SELECT s.run_id,s.step_no,s.host,s.lanes,s.valid,s.knee_signal,s.window_end AS at_ms,
              'new_step_after_invalid' AS violation
       FROM capacity_step s WHERE s.repeat_of IS NULL AND EXISTS (
         SELECT 1 FROM capacity_step p WHERE p.run_id=s.run_id
           AND p.step_no=(SELECT MAX(x.step_no) FROM capacity_step x
                          WHERE x.run_id=s.run_id AND x.step_no<s.step_no)
           AND p.valid=0 AND p.window_end=(SELECT MAX(x.window_end) FROM capacity_step x
             WHERE x.run_id=p.run_id AND x.step_no=p.step_no))
       UNION ALL
       SELECT run_id,step_no,host,lanes,valid,knee_signal,window_end,'knee_on_invalid'
       FROM capacity_step WHERE knee_signal=1 AND valid<>1
       UNION ALL
       SELECT s.run_id,s.step_no,s.host,s.lanes,s.valid,s.knee_signal,s.window_end,
              'level_uses_invalid_step'
       FROM capacity_level l,json_each(l.evidence_steps) e
       JOIN capacity_step s ON s.run_id=l.run_id AND s.step_no=CAST(e.value AS INTEGER)
       WHERE s.valid<>1",
    "CREATE VIEW discovery_knee_signals AS
       SELECT *, window_end AS at_ms FROM capacity_step WHERE knee_signal=1",
    "CREATE VIEW discovery_raised_past_knee AS
       SELECT s.*,s.window_end AS at_ms FROM capacity_step s
       JOIN capacity_step k ON k.run_id=s.run_id AND k.knee_signal=1
         AND s.step_no>k.step_no AND s.lanes>k.lanes",
    "CREATE VIEW discovery_runs_ended AS
       SELECT *,ended_at AS at_ms FROM capacity_run WHERE ended_at IS NOT NULL",
    "CREATE VIEW discovery_runs_without_recorded_level AS
       SELECT r.*,r.ended_at AS at_ms FROM capacity_run r
       WHERE r.ended_at IS NOT NULL AND r.found_level IS NOT NULL AND NOT EXISTS (
         SELECT 1 FROM capacity_level l WHERE l.run_id=r.run_id)",
    "CREATE VIEW unrecorded_hosts_with_demand AS
       SELECT s.*,s.at AS at_ms FROM saturation_sample s
       WHERE s.ready_buffer IS NOT NULL AND s.supplied_lanes IS NOT NULL
         AND s.unsupplied_lanes IS NOT NULL AND s.ready_share IS NOT NULL
         AND ((s.active_lanes>0 AND s.registry_current=1
                 AND s.supplied_lanes+s.ready_share>=s.active_lanes)
          OR (s.active_lanes=0 AND s.registry_current=1
              AND s.bootstrap_ticket_pool>=1))
         AND NOT EXISTS (
         SELECT 1 FROM capacity_level l WHERE lower(l.host)=lower(s.host)
           AND l.recorded_at<=s.at
           AND (l.superseded_at IS NULL OR l.superseded_at>s.at))",
    "CREATE VIEW unrecorded_hosts_without_discovery_run AS
       SELECT s.*,s.at AS at_ms FROM unrecorded_hosts_with_demand s
       WHERE s.at+2*COALESCE((SELECT tick_interval_s FROM run WHERE kind='status'
                               ORDER BY sequence DESC LIMIT 1),300)*1000<=unixepoch('now')*1000
         AND NOT EXISTS (SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(s.host)
              AND r.started_at>=s.at AND r.started_at<=s.at+2*COALESCE((SELECT tick_interval_s FROM run WHERE kind='status'
                               ORDER BY sequence DESC LIMIT 1),300)*1000)
         AND NOT EXISTS (SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(s.host)
              AND r.started_at<=s.at AND (r.ended_at IS NULL OR r.ended_at>s.at))
         AND (SELECT COUNT(*) FROM capacity_run r WHERE lower(r.host)=lower(s.host)
              AND date(r.started_at/1000,'unixepoch')=date(s.at/1000,'unixepoch')
              AND r.started_at<=s.at)<2
         AND NOT EXISTS (SELECT 1 FROM capacity_event e WHERE lower(e.host)=lower(s.host)
              AND e.kind='step_down' AND e.at<=s.at AND e.ceiling_until>s.at)",
    "CREATE VIEW unrecorded_hosts_ready_for_discovery AS
       SELECT s.*,s.at AS at_ms FROM unrecorded_hosts_with_demand s
       WHERE s.at<=unixepoch('now')*1000
         AND s.at>=unixepoch('now')*1000-2*COALESCE((SELECT tick_interval_s FROM run WHERE kind='status'
                               ORDER BY sequence DESC LIMIT 1),300)*1000
         AND s.at=(SELECT MAX(latest.at) FROM saturation_sample latest
                   WHERE lower(latest.host)=lower(s.host)
                     AND latest.at<=unixepoch('now')*1000)
         AND NOT EXISTS (SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(s.host)
              AND r.started_at<=unixepoch('now')*1000+999
              AND (r.ended_at IS NULL OR r.ended_at>unixepoch('now')*1000+999))
         AND NOT EXISTS (SELECT 1 FROM capacity_level l WHERE lower(l.host)=lower(s.host)
              AND l.recorded_at<=s.at
              AND (l.superseded_at IS NULL OR l.superseded_at>s.at))
         AND NOT EXISTS (SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(s.host)
              AND r.started_at<=s.at AND (r.ended_at IS NULL OR r.ended_at>s.at))
         AND (SELECT COUNT(*) FROM capacity_run r WHERE lower(r.host)=lower(s.host)
              AND date(r.started_at/1000,'unixepoch')=date(unixepoch('now'),'unixepoch')
              AND r.started_at<=unixepoch('now')*1000+999)<2
         AND NOT EXISTS (SELECT 1 FROM capacity_event e WHERE lower(e.host)=lower(s.host)
              AND e.kind='step_down' AND e.at<=unixepoch('now')*1000+999
              AND e.ceiling_until>unixepoch('now')*1000+999)",
    "CREATE VIEW hosts_with_recorded_level AS
       SELECT *,recorded_at AS at_ms FROM capacity_level WHERE superseded_at IS NULL",
    "CREATE VIEW host_step_downs AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind='step_down'",
    "CREATE VIEW raises_during_cool_off AS
       SELECT DISTINCT a.*,a.at AS at_ms FROM capacity_event a JOIN capacity_event d
         ON lower(d.host)=lower(a.host) AND d.kind='step_down' AND a.kind='lane_added'
          AND a.at>=d.at AND d.ceiling_until IS NOT NULL AND a.at<d.ceiling_until
       WHERE EXISTS (SELECT 1 FROM lane_tick_rows sample
         WHERE lower(sample.host)=lower(a.host) AND sample.observed_ms=(
           SELECT MIN(t.observed_ms) FROM lane_tick_rows t
           WHERE lower(t.host)=lower(a.host) AND t.observed_ms>=a.at
             AND t.observed_ms<=a.at+t.tick_ms)
         AND (d.ceiling_lanes IS NULL OR (SELECT COUNT(DISTINCT t.lane) FROM lane_tick_rows t
           WHERE lower(t.host)=lower(a.host) AND t.observed_ms=sample.observed_ms
             AND NOT EXISTS (SELECT 1 FROM capacity_event draining
               WHERE lower(draining.lane)=lower(t.lane) AND draining.kind='draining_mark'
                 AND draining.at<=sample.observed_ms AND NOT EXISTS (
                   SELECT 1 FROM capacity_event finished WHERE lower(finished.lane)=lower(t.lane)
                     AND finished.kind IN ('drain_done','lane_removed','lane_lost')
                     AND finished.at>=draining.at AND finished.at<=sample.observed_ms)))
             > d.ceiling_lanes))",
    "CREATE VIEW discovery_runs AS SELECT *,started_at AS at_ms FROM capacity_run",
    "CREATE VIEW discovery_steps_ended AS
       SELECT s.*,s.window_end AS at_ms FROM capacity_step s",
    "CREATE VIEW discovery_runs_per_host_day AS
       SELECT lower(host) AS host,date(started_at/1000,'unixepoch') AS day,
              COUNT(*) AS runs,MAX(started_at) AS at_ms FROM capacity_run
       GROUP BY lower(host),day",
    "CREATE VIEW discovery_runs_per_host_day_max AS
       SELECT host,day,runs,at_ms FROM discovery_runs_per_host_day
       WHERE runs=(SELECT MAX(runs) FROM discovery_runs_per_host_day)",
    "CREATE VIEW host_minutes_above_recorded_level_outside_bounded_discovery AS
       WITH lane_counts AS (
         SELECT host,observed_ms,COUNT(DISTINCT lane) AS lanes
         FROM lane_tick_rows t WHERE NOT EXISTS (
           SELECT 1 FROM capacity_event d WHERE lower(d.lane)=lower(t.lane)
             AND d.kind='draining_mark' AND NOT EXISTS (
               SELECT 1 FROM capacity_event f WHERE lower(f.lane)=lower(d.lane)
                 AND f.kind IN ('drain_done','lane_removed','lane_lost') AND f.at>=d.at))
         GROUP BY lower(host),observed_ms),
       spans AS (
         SELECT host,observed_ms,lanes,
           LEAD(observed_ms) OVER(PARTITION BY lower(host) ORDER BY observed_ms) AS next_ms
         FROM lane_counts)
       SELECT l.host,l.level,
         SUM(CASE WHEN s.lanes>l.level AND NOT EXISTS (
           SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(l.host)
             AND r.started_at<=s.observed_ms
             AND (r.ended_at IS NULL OR r.ended_at>s.observed_ms)
             AND (SELECT COUNT(*) FROM capacity_step step WHERE step.run_id=r.run_id)<=8)
           THEN s.next_ms-s.observed_ms ELSE 0 END)/60000.0 AS minutes,
         MAX(s.observed_ms) AS at_ms
       FROM spans s JOIN capacity_level l ON lower(l.host)=lower(s.host)
       WHERE l.superseded_at IS NULL AND s.next_ms IS NOT NULL
       GROUP BY l.host,l.level",
    "CREATE VIEW overload_events_without_draining_mark_in_cycle AS
       SELECT o.*,o.at AS at_ms FROM capacity_event o WHERE o.kind='overload_event'
         AND NOT EXISTS (SELECT 1 FROM capacity_event d WHERE lower(d.host)=lower(o.host)
           AND d.kind='draining_mark' AND d.at>=o.at AND d.at<=o.at+2*COALESCE(
             (SELECT tick_interval_s FROM run WHERE kind='status' ORDER BY sequence DESC LIMIT 1),300)*1000)",
    "CREATE VIEW lane_removals AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind='lane_removed'",
    "CREATE VIEW lane_removals_before_drain AS
       SELECT r.*,r.at AS at_ms FROM capacity_event r WHERE r.kind='lane_removed'
         AND NOT EXISTS (SELECT 1 FROM capacity_event d WHERE lower(d.lane)=lower(r.lane)
           AND d.kind='draining_mark' AND d.at<=r.at
           AND EXISTS (SELECT 1 FROM capacity_event f WHERE lower(f.lane)=lower(d.lane)
             AND f.kind IN ('drain_done','lane_lost') AND f.at>=d.at AND f.at<=r.at)
           AND NOT EXISTS (SELECT 1 FROM capacity_event x WHERE lower(x.lane)=lower(d.lane)
             AND x.kind='lane_removed' AND x.at>d.at AND x.at<r.at))",
    "CREATE VIEW lane_change_unrecorded AS
       SELECT *,at AS at_ms FROM capacity_event
       WHERE kind IN ('lane_added','lane_removed') AND reason='lane_change_unrecorded'",
    "CREATE VIEW lanes_lost AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind='lane_lost'",
    "CREATE VIEW draining_lanes AS
       SELECT d.*,d.at AS at_ms FROM capacity_event d WHERE d.kind='draining_mark'
         AND NOT EXISTS (SELECT 1 FROM capacity_event f WHERE lower(f.lane)=lower(d.lane)
           AND f.kind IN ('drain_done','lane_removed','lane_lost') AND f.at>=d.at)",
    "CREATE VIEW drains_over_cap_without_escalation AS
       SELECT d.*,d.at AS at_ms FROM draining_lanes d
       WHERE d.at+d.drain_cap_ms<=(SELECT MAX(relay_ms) FROM run) AND NOT EXISTS (
           SELECT 1 FROM action_send x JOIN action a ON a.id=x.action_id
           WHERE a.kind='escalate_drain' AND lower(a.lane)=lower(d.lane))",
    "CREATE VIEW discovery_inconclusive_windows AS
       SELECT *,window_end AS at_ms FROM capacity_step WHERE valid=0",
    "CREATE VIEW no_demand_runs_not_ended AS
       SELECT r.*,r.started_at AS at_ms FROM capacity_run r
         WHERE r.end_reason IS NOT 'inconclusive_no_demand'
         AND (SELECT COUNT(*) FROM (SELECT valid FROM capacity_step s
           WHERE s.run_id=r.run_id ORDER BY step_no DESC LIMIT 2) WHERE valid=0)>=2",
    "CREATE VIEW discovery_slot_saturated_windows AS
       SELECT *,window_end AS at_ms FROM capacity_step
       WHERE valid=1 AND slots_busy_equals_sustainable=1",
    "CREATE VIEW discovery_raised_past_slot_saturation AS
       SELECT s.*,s.window_end AS at_ms FROM capacity_step s
       JOIN capacity_step c ON c.run_id=s.run_id AND c.slots_busy_equals_sustainable=1
         AND c.valid=1 AND s.step_no>c.step_no AND s.lanes>c.lanes",
    "CREATE VIEW lane_count_reductions AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind IN
         ('lane_removed','draining_mark','step_down')",
    "CREATE VIEW lane_reductions_without_throttle_evidence AS
       SELECT *,at AS at_ms FROM lane_count_reductions
       WHERE evidence_kind IS NULL OR reason='lane_change_unrecorded'",
    "CREATE VIEW host_output_share_of_recorded_level AS
       WITH current_level_ranked AS (
         SELECT l.*,ROW_NUMBER() OVER(PARTITION BY lower(host) ORDER BY recorded_at DESC,run_id DESC) AS level_rank
         FROM capacity_level l WHERE superseded_at IS NULL),
       current_level AS (SELECT * FROM current_level_ranked WHERE level_rank=1),
       level_step_ranked AS (
         SELECT l.host,l.output_basis,s.host_output_per_hour,s.step_no,
           ROW_NUMBER() OVER(PARTITION BY lower(l.host) ORDER BY CAST(e.value AS INTEGER) DESC) AS evidence_rank
         FROM current_level l,json_each(l.evidence_steps) e
         JOIN capacity_step s ON s.run_id=l.run_id AND s.step_no=CAST(e.value AS INTEGER)),
       level_step AS (SELECT * FROM level_step_ranked WHERE evidence_rank=1),
       latest AS (SELECT h.*,ROW_NUMBER() OVER(PARTITION BY lower(host) ORDER BY at DESC) AS rn
         FROM host_output_sample h)
       SELECT l.host,l.output_basis,l.step_no,
         CASE l.output_basis
           WHEN 'commits' THEN h.commits_pushed*3600000.0/h.sample_interval_ms
           WHEN 'tickets_prs' THEN CASE WHEN h.tickets_closed IS NOT NULL AND h.prs_merged IS NOT NULL
             THEN (h.tickets_closed+h.prs_merged)*3600000.0/h.sample_interval_ms END
         END AS output_per_hour,
         h.sample_interval_ms AS sample_interval_ms,
         l.host_output_per_hour AS recorded_output_per_hour,
         CASE WHEN l.host_output_per_hour>0 AND (CASE l.output_basis
           WHEN 'commits' THEN h.commits_pushed*3600000.0/h.sample_interval_ms
           WHEN 'tickets_prs' THEN CASE WHEN h.tickets_closed IS NOT NULL AND h.prs_merged IS NOT NULL
             THEN (h.tickets_closed+h.prs_merged)*3600000.0/h.sample_interval_ms END END) IS NOT NULL
           THEN 100.0*(CASE l.output_basis
           WHEN 'commits' THEN h.commits_pushed*3600000.0/h.sample_interval_ms
           WHEN 'tickets_prs' THEN (h.tickets_closed+h.prs_merged)*3600000.0/h.sample_interval_ms END)/l.host_output_per_hour
           ELSE NULL END AS share_percent,h.at AS at_ms,
         CASE WHEN h.source_basis IS NULL OR h.source_basis='{}' THEN
           CASE l.output_basis
             WHEN 'commits' THEN json_object('commits_pushed',json_object(
               'availability','unavailable','reason','no_current_host_output_sample'))
             WHEN 'tickets_prs' THEN json_object(
               'tickets_closed',json_object('availability','unavailable','reason','no_current_host_output_sample'),
               'prs_merged',json_object('availability','unavailable','reason','no_current_host_output_sample'))
             ELSE json_object('host_output_per_hour',json_object(
               'availability','unavailable','reason','no_current_host_output_sample'))
           END
           ELSE h.source_basis END AS source_basis,
         COALESCE(
           json_extract(h.source_basis,'$.host_output_per_hour.reason'),
           CASE l.output_basis WHEN 'commits'
             THEN json_extract(h.source_basis,'$.commits_pushed.reason')
             WHEN 'tickets_prs' THEN COALESCE(
               json_extract(h.source_basis,'$.tickets_closed.reason'),
               json_extract(h.source_basis,'$.prs_merged.reason'))
           END,
           CASE WHEN h.source_basis IS NULL THEN 'no_current_host_output_sample'
                WHEN h.source_basis='{}' THEN 'source_basis_unavailable' END
         ) AS unavailable_reason
       FROM level_step l LEFT JOIN latest h ON lower(h.host)=lower(l.host) AND h.rn=1",
];

/// The `open_cycles` formula of FR-039, `max(1, ceil((end - start - grace) / cycle))`, in
/// integer milliseconds. The arguments are SQL expressions.
pub fn open_cycles_sql(start: &str, end: &str, grace_ms: &str, cycle_ms: &str) -> String {
    format!("max(1, (({end}) - ({start}) - ({grace_ms}) + ({cycle_ms}) - 1) / ({cycle_ms}))")
}

/// The base columns of `lane_tick` that views expose.
const LANE_TICK_COLUMNS: &str = "id, run_seq, tick_id, source, lane, host, class, reason, \
     remaining_assigned, ready_work_available, observed_ms, last_event_ms, evidence, at_ms";

/// Views over `lane_tick` and `defect`. Each view with a time carries it as `at_ms`, which
/// `--since` filters on. `lane_tick_rows` keeps one row per lane per run time (a `tick` row wins
/// over a `status` row) and carries the interval and grace of the run that wrote it, so a
/// changed configuration never re-scores history.
#[implements("FR-039-AC-15")]
pub fn views() -> Vec<String> {
    let episode_cycles = open_cycles_sql("start_ms", "end_ms", "grace_ms", "cycle_ms");
    vec![
        "CREATE VIEW lane_tick_rows AS
           SELECT t.*, t.observed_ms AS at_ms,
                  COALESCE(r.tick_interval_s, 300) * 1000 AS tick_ms,
                  COALESCE(r.cycle_interval_s, 600) * 1000 AS cycle_ms,
                  COALESCE(r.grace_s,
                           COALESCE(r.cycle_interval_s, 600) + COALESCE(r.tick_interval_s, 300))
                    * 1000 AS grace_ms
           FROM lane_tick t JOIN run r ON r.sequence = t.run_seq
           WHERE t.id = (SELECT x.id FROM lane_tick x
                         WHERE x.lane = t.lane AND x.observed_ms = t.observed_ms
                         ORDER BY (x.source = 'tick') DESC, x.id LIMIT 1)"
            .to_owned(),
        format!(
            "CREATE VIEW lanes_with_ready_work AS
               SELECT {LANE_TICK_COLUMNS} FROM lane_tick_rows WHERE ready_work_available = 1"
        ),
        format!(
            "CREATE VIEW idle_lane_with_ready_work_ticks AS
               SELECT {LANE_TICK_COLUMNS} FROM lane_tick_rows
               WHERE class = 'IDLE' AND ready_work_available = 1"
        ),
        format!(
            "CREATE VIEW idle_lane_episodes_with_ready_work AS
               WITH o AS (
                 SELECT lane, observed_ms, class, ready_work_available AS ready,
                        tick_ms, cycle_ms, grace_ms,
                        ROW_NUMBER() OVER (PARTITION BY lane ORDER BY observed_ms, run_seq) AS rn
                 FROM lane_tick_rows),
               q AS (
                 SELECT lane, observed_ms, tick_ms, cycle_ms, grace_ms,
                        rn - ROW_NUMBER() OVER (PARTITION BY lane ORDER BY rn) AS grp
                 FROM o WHERE class = 'IDLE' AND ready = 1),
               e AS (
                 SELECT lane, MIN(observed_ms) AS start_ms,
                        MAX(observed_ms) + tick_ms AS end_ms, COUNT(*) AS ticks,
                        cycle_ms, grace_ms
                 FROM q GROUP BY lane, grp)
               SELECT lane, start_ms, end_ms, ticks, {episode_cycles} AS open_cycles,
                      start_ms AS at_ms
               FROM e"
        ),
        "CREATE VIEW idle_lane_episode_with_ready_work_max AS
           SELECT MAX(open_cycles) AS open_cycles FROM idle_lane_episodes_with_ready_work"
            .to_owned(),
        "CREATE VIEW lane_span AS
           SELECT t.lane, t.host, t.class,
                  COALESCE(t.ready_work_available,(
                    SELECT p.ready_work_available FROM lane_tick_rows p
                    WHERE p.lane=t.lane AND p.observed_ms<t.observed_ms
                      AND p.ready_work_available IS NOT NULL
                      AND t.observed_ms-p.observed_ms<=2*p.tick_ms
                    ORDER BY p.observed_ms DESC,p.run_seq DESC LIMIT 1)) AS ready,
                  t.observed_ms AS start_ms,
                  COALESCE(t.remaining_assigned,(
                    SELECT p.remaining_assigned FROM lane_tick_rows p
                    WHERE p.lane=t.lane AND p.observed_ms<t.observed_ms
                      AND p.remaining_assigned IS NOT NULL
                      AND t.observed_ms-p.observed_ms<=2*p.tick_ms
                    ORDER BY p.observed_ms DESC,p.run_seq DESC LIMIT 1)) AS remaining_assigned,
                  t.tick_ms,
                  observed_ms AS at_ms,
                  LEAD(t.observed_ms) OVER (PARTITION BY t.lane ORDER BY t.observed_ms, t.run_seq)
                    AS end_ms
           FROM lane_tick_rows t"
            .to_owned(),
        "CREATE VIEW idle_minutes AS
           SELECT lane, host, date(start_ms / 1000, 'unixepoch') AS day, class,
                  SUM(end_ms - start_ms) / 60000.0 AS minutes,
                  SUM(CASE WHEN ready = 1 THEN end_ms - start_ms ELSE 0 END) / 60000.0
                    AS minutes_with_ready_work,
                  CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
                    AS at_ms
           FROM lane_span
           WHERE class NOT IN ('WORKING','UNREACHABLE') AND end_ms IS NOT NULL
             AND end_ms - start_ms <= 2 * tick_ms
           GROUP BY lane, host, day, class"
            .to_owned(),
        "CREATE VIEW defect_rows AS
           SELECT defect_id, source, host, lane, kind, first_seen_at, first_seen_host_ms,
                  closed_at, routed_ticket, routed_at, first_seen_at AS at_ms
           FROM defect"
            .to_owned(),
        "CREATE VIEW idle_capacity_minutes_with_ready_work AS
           WITH spans AS (
             SELECT date(start_ms / 1000, 'unixepoch') AS day, ready,
                    end_ms-start_ms AS duration_ms,
                    CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
                      AS at_ms
             FROM lane_span WHERE class='IDLE' AND end_ms IS NOT NULL
               AND end_ms-start_ms<=2*tick_ms),
           totals AS (
             SELECT day,MAX(at_ms) AS at_ms,
                    SUM(CASE WHEN ready IS NULL THEN 1 ELSE 0 END) AS unknown_count,
                    SUM(CASE WHEN ready=1 THEN duration_ms ELSE 0 END)/60000.0 AS known_minutes
             FROM spans GROUP BY day)
           SELECT day,at_ms,
                  CASE WHEN unknown_count=0 THEN known_minutes END AS slot_minutes,
                  CASE WHEN unknown_count=0 THEN 1 ELSE 0 END AS available,
                  CASE WHEN unknown_count=0 THEN NULL ELSE 'ready work source unavailable' END
                    AS unavailable_reason
           FROM totals"
            .to_owned(),
        "CREATE VIEW missed_idle_minutes AS
           WITH spans AS (
             SELECT lane,host,date(start_ms/1000,'unixepoch') AS day,remaining_assigned,
                    end_ms-start_ms AS duration_ms,
                    CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
                      AS at_ms
             FROM lane_span WHERE class='IDLE' AND end_ms IS NOT NULL
               AND end_ms-start_ms<=2*tick_ms),
           totals AS (
             SELECT lane,host,day,MAX(at_ms) AS at_ms,
                    SUM(CASE WHEN remaining_assigned IS NULL THEN 1 ELSE 0 END) AS unknown_count,
                    SUM(CASE WHEN remaining_assigned>0 THEN duration_ms ELSE 0 END)/60000.0
                      AS known_minutes
             FROM spans GROUP BY lane,host,day)
           SELECT lane,host,day,at_ms,
                  CASE WHEN unknown_count=0 THEN known_minutes END AS minutes,
                  CASE WHEN unknown_count=0 THEN 1 ELSE 0 END AS available,
                  CASE WHEN unknown_count=0 THEN NULL ELSE 'assignment source unavailable' END
                    AS unavailable_reason
           FROM totals"
            .to_owned(),
        "CREATE VIEW blocked_lane_minutes AS
           SELECT lane, host, date(start_ms / 1000, 'unixepoch') AS day,
                  CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
                    AS at_ms,
                  SUM(end_ms - start_ms) / 60000.0 AS minutes
           FROM lane_span
           WHERE class = 'BLOCKED' AND end_ms IS NOT NULL
             AND end_ms - start_ms <= 2 * tick_ms
           GROUP BY lane, host, day"
            .to_owned(),
        "CREATE VIEW time_to_action_p90 AS
           WITH marked AS (
             SELECT lane, observed_ms, tick_ms, class,
                    CASE WHEN class = 'WORKING' THEN 1 ELSE 0 END AS working,
                    LAG(CASE WHEN class = 'WORKING' THEN 1 ELSE 0 END)
                      OVER (PARTITION BY lane ORDER BY observed_ms, run_seq) AS prior_working
             FROM lane_tick_rows),
           grouped AS (
             SELECT lane, observed_ms, tick_ms, class, working,
                    SUM(CASE WHEN prior_working IS NULL OR prior_working <> working
                             THEN 1 ELSE 0 END)
                      OVER (PARTITION BY lane ORDER BY observed_ms) AS episode
             FROM marked),
           episodes AS (
             SELECT lane, episode, MIN(working) AS working,
                    MIN(observed_ms) AS start_ms, MAX(observed_ms) AS last_ms,
                    MAX(tick_ms) AS tick_ms
             FROM grouped GROUP BY lane, episode),
           transitions AS (
             SELECT lane, episode, working, start_ms, last_ms, tick_ms,
                    LEAD(working) OVER (PARTITION BY lane ORDER BY episode) AS next_working,
                    LEAD(start_ms) OVER (PARTITION BY lane ORDER BY episode) AS next_start
             FROM episodes),
           samples AS (
             SELECT date(start_ms / 1000, 'unixepoch') AS day,
                    CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
                      AS at_ms,
                    (next_start - start_ms) / 60000.0 AS minutes,
                    CASE WHEN next_start IS NULL THEN 1 ELSE 0 END AS unbounded
             FROM transitions
             WHERE working = 0 AND (next_working = 1 OR next_start IS NULL)
               AND (next_start IS NULL OR next_start - start_ms <= 2 * tick_ms)),
           ranked AS (
             SELECT day, at_ms, minutes, unbounded,
                    ROW_NUMBER() OVER (PARTITION BY day ORDER BY minutes) AS rank
             FROM samples),
           totals AS (
             SELECT day, MAX(at_ms) AS at_ms, COUNT(*) AS sample_count,
                    SUM(unbounded) AS unbounded_count,
                    SUM(CASE WHEN unbounded=0 THEN 1 ELSE 0 END) AS bounded_count
             FROM samples GROUP BY day),
           percentile AS (
             SELECT r.day, r.minutes FROM ranked r JOIN totals t USING(day)
             WHERE r.unbounded=0 AND r.rank=(9 * t.sample_count + 9) / 10)
           SELECT totals.day, totals.at_ms,
                  CASE WHEN totals.unbounded_count = 0 THEN percentile.minutes END AS p90_minutes,
                  totals.sample_count, totals.unbounded_count
           FROM totals LEFT JOIN percentile USING(day)"
            .to_owned(),
        "CREATE VIEW detection_to_action_p90 AS
           WITH samples AS (
             SELECT date(emitted_at / 1000, 'unixepoch') AS day,
                    CAST(strftime('%s', date(emitted_at / 1000, 'unixepoch')) AS INTEGER) * 1000
                      AS at_ms,
                    CASE WHEN executed_at >= emitted_at
                         THEN (executed_at - emitted_at) / 60000.0 END AS minutes,
                    CASE WHEN executed_at >= emitted_at THEN 0 ELSE 1 END AS unbounded
             FROM action WHERE lane IS NOT NULL),
           ranked AS (
             SELECT day, at_ms, minutes, unbounded,
                    ROW_NUMBER() OVER (PARTITION BY day ORDER BY minutes) AS rank
             FROM samples WHERE unbounded = 0),
           totals AS (
             SELECT day, MAX(at_ms) AS at_ms, COUNT(*) AS sample_count,
                    SUM(unbounded) AS unbounded_count,
                    SUM(CASE WHEN unbounded=0 THEN 1 ELSE 0 END) AS bounded_count
             FROM samples GROUP BY day),
           percentile AS (
             SELECT r.day, r.minutes FROM ranked r JOIN totals t USING(day)
             WHERE r.rank = (9 * t.sample_count + 9) / 10)
           SELECT totals.day, totals.at_ms,
                  CASE WHEN totals.unbounded_count = 0 THEN percentile.minutes END AS p90_minutes,
                  totals.sample_count, totals.unbounded_count
           FROM totals LEFT JOIN percentile USING(day)"
            .to_owned(),
        "CREATE VIEW assignment_confirm_latency_p90 AS
           WITH samples AS (
             SELECT date(sent_at / 1000, 'unixepoch') AS day,
                    CAST(strftime('%s', date(sent_at / 1000, 'unixepoch')) AS INTEGER) * 1000
                      AS at_ms,
                    CASE WHEN result = 'confirmed' AND result_at IS NOT NULL
                                  AND confirm_latency_s IS NOT NULL
                         THEN confirm_latency_s / 60.0 END AS minutes,
                    CASE WHEN result = 'confirmed' AND result_at IS NOT NULL
                                  AND confirm_latency_s IS NOT NULL THEN 0 ELSE 1 END AS unbounded
             FROM assignment_history
             WHERE sent_at IS NOT NULL
               AND deadline_at <= COALESCE((SELECT MAX(relay_ms) FROM run), 0)),
           ranked AS (
             SELECT day, minutes,
                    ROW_NUMBER() OVER (PARTITION BY day ORDER BY minutes) AS rank
             FROM samples WHERE unbounded = 0),
           totals AS (
             SELECT day, MAX(at_ms) AS at_ms, COUNT(*) AS sample_count,
                    SUM(unbounded) AS unbounded_count,
                    SUM(CASE WHEN unbounded=0 THEN 1 ELSE 0 END) AS bounded_count
             FROM samples GROUP BY day),
           percentile AS (
             SELECT r.day, r.minutes FROM ranked r JOIN totals t USING(day)
             WHERE r.rank = (9 * t.sample_count + 9) / 10)
           SELECT totals.day, totals.at_ms,
                  CASE WHEN totals.unbounded_count = 0 THEN percentile.minutes END AS p90_minutes,
                  totals.sample_count, totals.unbounded_count
           FROM totals LEFT JOIN percentile USING(day)"
            .to_owned(),
        "CREATE VIEW drift_lane_minutes_share AS
           WITH RECURSIVE spans AS (
             SELECT ROW_NUMBER() OVER (ORDER BY lane, host, start_ms) AS span_id,
                    lane, host, start_ms, end_ms, tick_ms
             FROM lane_span
             WHERE class = 'WORKING' AND end_ms IS NOT NULL AND end_ms > start_ms
               AND end_ms - start_ms <= 2 * tick_ms),
           days(span_id,lane,start_ms,end_ms,tick_ms,day_start_ms) AS (
             SELECT span_id,lane,start_ms,end_ms,tick_ms,
                    CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
             FROM spans
             UNION ALL
             SELECT span_id,lane,start_ms,end_ms,tick_ms,day_start_ms + 86400000
             FROM days WHERE day_start_ms + 86400000 < end_ms),
           segments AS (
             SELECT span_id,lane,start_ms,end_ms,tick_ms,day_start_ms,
                    max(start_ms,day_start_ms) AS segment_start,
                    min(end_ms,day_start_ms + 86400000) AS segment_end
             FROM days),
           daily_working AS (
             SELECT day_start_ms, SUM(segment_end - segment_start) AS working_ms
             FROM segments GROUP BY day_start_ms),
           drift_intervals AS (
             SELECT s.span_id,s.day_start_ms,
                    max(s.segment_start,d.opened_at) AS drift_start,
                    min(s.segment_end,COALESCE(d.closed_at,s.segment_end)) AS drift_end
             FROM segments s JOIN drift_finding d ON d.lane = s.lane
             WHERE d.rule IN ('not_assigned','not_in_plan','priority_inversion')
               AND d.opened_at < s.segment_end
               AND COALESCE(d.closed_at,s.segment_end) > s.segment_start),
           ordered_drift AS (
             SELECT span_id,day_start_ms,drift_start,drift_end,
                    MAX(drift_end) OVER (
                      PARTITION BY span_id,day_start_ms ORDER BY drift_start,drift_end
                      ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING) AS previous_end
             FROM drift_intervals),
           daily_drift AS (
             SELECT day_start_ms,
                    SUM(max(0,drift_end - max(drift_start,COALESCE(previous_end,drift_start))))
                      AS drift_ms
             FROM ordered_drift GROUP BY day_start_ms)
           SELECT date(w.day_start_ms / 1000, 'unixepoch') AS day,
                  w.day_start_ms AS at_ms,
                  w.working_ms / 60000.0 AS working_minutes,
                  COALESCE(d.drift_ms,0) / 60000.0 AS drift_minutes,
                  100.0 * COALESCE(d.drift_ms,0) / w.working_ms AS share_percent
           FROM daily_working w LEFT JOIN daily_drift d USING(day_start_ms)"
            .to_owned(),
        "CREATE VIEW idle_lane_minutes_during_build_pause AS
           SELECT NULL AS lane, NULL AS host, NULL AS day, NULL AS at_ms, NULL AS minutes,
                  0 AS available, 'no pause producer' AS unavailable_reason"
            .to_owned(),
        "CREATE VIEW need_ready_tickets_actions AS
           SELECT id AS action_id, tick_id, emitted_at, resolved_at, project, requested_count,
                  target_role, target_addr
           FROM action WHERE kind = 'need_ready_tickets'"
            .to_owned(),
        "CREATE VIEW need_ready_tickets_due AS
           SELECT action_id, tick_id, project, requested_count, emitted_at, resolved_at,
                  emitted_at + 86400000 AS at_ms, target_role, target_addr
           FROM need_ready_tickets_actions"
            .to_owned(),
        "CREATE VIEW need_ready_tickets_without_planner_order AS
           SELECT a.id AS action_id, a.tick_id, a.emitted_at AS at_ms, a.target_addr
           FROM action a WHERE a.kind = 'need_ready_tickets' AND NOT EXISTS (
             SELECT 1 FROM action_send s WHERE s.action_id = a.id
               AND s.recipient_addr = a.target_addr)"
            .to_owned(),
        "CREATE VIEW plan_coverage_gap_rows AS
           SELECT gap_id,kind,subject,detail,opened_tick,opened_at,closed_tick,closed_at,
                  COALESCE(closed_at,opened_at) AS at_ms,
                  max(1, (COALESCE(closed_at,(SELECT MAX(relay_ms) FROM run)) - opened_at -
                    COALESCE((SELECT grace_s FROM run ORDER BY sequence DESC LIMIT 1),
                      COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600) +
                      COALESCE((SELECT tick_interval_s FROM run ORDER BY sequence DESC LIMIT 1),300))*1000 +
                    COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000 - 1) /
                    (COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000))
                    AS open_cycles
           FROM plan_coverage_gap"
            .to_owned(),
        "CREATE VIEW plan_coverage_gap_open_cycles_max AS
           SELECT MAX(open_cycles) AS open_cycles, MAX(at_ms) AS at_ms
           FROM plan_coverage_gap_rows"
            .to_owned(),
        "CREATE VIEW short_queue_open_cycles_max AS
           SELECT MAX(open_cycles) AS open_cycles, MAX(at_ms) AS at_ms
           FROM plan_coverage_gap_rows WHERE kind='short_queue'"
            .to_owned(),
        "CREATE VIEW drift_findings AS
           SELECT finding_id,lane,rule,focus,opened_tick,opened_at,closed_at,evidence_times,
                  COALESCE(closed_at,opened_at) AS at_ms,
                  max(1, (COALESCE(closed_at,(SELECT MAX(relay_ms) FROM run)) - opened_at -
                    COALESCE((SELECT grace_s FROM run ORDER BY sequence DESC LIMIT 1),
                      COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600) +
                      COALESCE((SELECT tick_interval_s FROM run ORDER BY sequence DESC LIMIT 1),300))*1000 +
                    COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000 - 1) /
                    (COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000))
                    AS open_cycles
           FROM drift_finding"
            .to_owned(),
        "CREATE VIEW drift_finding_open_cycles_max AS
           SELECT MAX(open_cycles) AS open_cycles, MAX(at_ms) AS at_ms FROM drift_findings"
            .to_owned(),
        "CREATE VIEW assignments_sent AS
           SELECT id AS assignment_id,lane,ticket,order_id,expect_ticket,sent_at,deadline_at,result,
                  result_at,confirm_latency_s,focus_at_deadline,sender_role,sender_addr,
                  sent_at AS at_ms
           FROM assignment_history WHERE sent_at IS NOT NULL"
            .to_owned(),
        "CREATE VIEW unconfirmed_assign_results AS
           SELECT * FROM assignments_sent
           WHERE result IN ('acted_not_focused','ack_expired','timed_out')"
            .to_owned(),
        "CREATE VIEW unconfirmed_assignments_without_followup AS
           SELECT u.assignment_id,u.lane,u.order_id,u.result_at AS at_ms,u.result,
                  CASE WHEN EXISTS (SELECT 1 FROM lane_lead ll WHERE ll.lane=u.lane
                    AND ll.observed_at <= u.result_at) THEN 1 ELSE 0 END AS available,
                  CASE WHEN EXISTS (SELECT 1 FROM lane_lead ll WHERE ll.lane=u.lane
                    AND ll.observed_at <= u.result_at) THEN NULL
                    ELSE 'lane lead mapping unavailable' END AS unavailable_reason
           FROM unconfirmed_assign_results u
           WHERE NOT EXISTS (SELECT 1 FROM lane_lead ll WHERE ll.lane=u.lane
               AND ll.observed_at <= u.result_at) OR (NOT EXISTS (
             SELECT 1 FROM assignment_history later
             WHERE later.lane=u.lane AND later.sent_at > u.result_at
               AND later.sent_at <= u.result_at +
                 COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000)
             AND NOT EXISTS (
             SELECT 1 FROM action_send s JOIN action a ON a.id=s.action_id
             WHERE s.sender_role='orchestrator' AND s.sent_at > u.result_at
               AND s.sent_at <= u.result_at +
                 COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000
               AND (s.recipient_addr=u.lane OR EXISTS (
                 SELECT 1 FROM lane_lead ll WHERE ll.lane=u.lane
                   AND ll.lead_addr=s.recipient_addr AND ll.observed_at <= s.sent_at
                   AND ll.observed_at=(SELECT MAX(ll2.observed_at) FROM lane_lead ll2
                     WHERE ll2.lane=u.lane AND ll2.observed_at <= s.sent_at)))))"
            .to_owned(),
        "CREATE VIEW assignment_confirm_latency_max AS
           WITH population AS (
             SELECT confirm_latency_s / 60.0 AS minutes,
                    CASE WHEN result='confirmed' AND confirm_latency_s IS NOT NULL
                         THEN 0 ELSE 1 END AS unbounded,
                    sent_at AS at_ms
             FROM assignments_sent
             WHERE deadline_at <= COALESCE((SELECT MAX(relay_ms) FROM run),0))
           SELECT CASE WHEN SUM(unbounded)=0 THEN MAX(minutes) END AS max_minutes,
                  COUNT(*) AS sample_count,COALESCE(SUM(unbounded),0) AS unbounded_count,
                  MAX(at_ms) AS at_ms
           FROM population"
            .to_owned(),
        "CREATE VIEW ladder_rungs_emitted AS
           SELECT a.id AS action_id,a.tick_id,a.kind,a.lane,a.emitted_at,a.resolved_at,
                  a.due_by,a.rung,a.target_role,a.target_addr,a.condition_started_at,
                  a.emitted_at AS at_ms
           FROM action a
           WHERE a.rung BETWEEN 1 AND 3 OR (a.rung=0 AND a.kind IN
             ('act_on_undetermined','push_next_ticket','answer_dialog','escalate_block','resend_order'))"
            .to_owned(),
        "CREATE VIEW escalation_rung AS
           SELECT a.*,s.sent_at,s.sender_addr AS send_sender_addr,
                  COALESCE(s.sent_at,a.emitted_at) AS at_ms
           FROM ladder_rungs_emitted a LEFT JOIN action_send s ON s.action_id=a.action_id"
            .to_owned(),
        "CREATE VIEW orchestrator_rungs_emitted AS
           SELECT * FROM ladder_rungs_emitted WHERE target_role='orchestrator'"
            .to_owned(),
        "CREATE VIEW orchestrator_rung_open_cycles_max AS
           SELECT MAX(max(1,(COALESCE(resolved_at,(SELECT MAX(relay_ms) FROM run))-emitted_at+
             COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000-1)/
             (COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000)))
             AS open_cycles, MAX(COALESCE(resolved_at,emitted_at)) AS at_ms
           FROM orchestrator_rungs_emitted"
            .to_owned(),
        "CREATE VIEW escalation_rung_send_latency_max AS
           SELECT CASE WHEN COUNT(*)=0 THEN NULL ELSE MAX(
                    CASE WHEN sent_at IS NULL THEN
                      (COALESCE((SELECT MAX(relay_ms) FROM run),emitted_at)-emitted_at) /
                      (COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000.0)
                    ELSE (sent_at-emitted_at) /
                      (COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000.0)
                    END) END AS max_cycles,
                  COUNT(*) AS sample_count,MAX(COALESCE(sent_at,emitted_at)) AS at_ms
           FROM escalation_rung"
            .to_owned(),
        "CREATE VIEW escalation_rungs_suppressed AS
           SELECT * FROM escalation_rung
           WHERE sent_at IS NULL AND due_by IS NOT NULL
             AND due_by <= COALESCE((SELECT MAX(relay_ms) FROM run),0)"
            .to_owned(),
        "CREATE VIEW add_lane_action_open_cycles_max AS
           SELECT MAX(max(1,(COALESCE(resolved_at,(SELECT MAX(relay_ms) FROM run))-emitted_at+
             COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000-1)/
             (COALESCE((SELECT cycle_interval_s FROM run ORDER BY sequence DESC LIMIT 1),600)*1000)))
             AS open_cycles,MAX(COALESCE(resolved_at,emitted_at)) AS at_ms
           FROM action WHERE kind='add_lane'"
            .to_owned(),
        "CREATE VIEW act_on_undetermined_actions AS
           SELECT *,emitted_at AS at_ms FROM action WHERE kind='act_on_undetermined'"
            .to_owned(),
        "CREATE VIEW push_next_ticket_actions AS
           SELECT *,emitted_at AS at_ms FROM action WHERE kind='push_next_ticket'"
            .to_owned(),
        "CREATE VIEW answer_dialog_actions AS
           SELECT *,emitted_at AS at_ms FROM action WHERE kind='answer_dialog'"
            .to_owned(),
        "CREATE VIEW escalate_block_actions AS
           SELECT *,emitted_at AS at_ms FROM action WHERE kind='escalate_block'"
            .to_owned(),
        "CREATE VIEW add_lane_actions AS
           SELECT *,emitted_at AS at_ms FROM action WHERE kind='add_lane'"
            .to_owned(),
        "CREATE VIEW act_on_undetermined_unexecuted_ticks AS
           SELECT a.id AS action_id,a.lane,a.emitted_at AS at_ms,COUNT(later.id) AS unexecuted_ticks
           FROM action a LEFT JOIN action later ON later.kind=a.kind AND later.lane=a.lane
             AND later.condition_started_at=a.condition_started_at AND later.id>a.id
             AND later.executed_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM action_send s WHERE s.action_id=later.id)
           WHERE a.kind='act_on_undetermined' GROUP BY a.id"
            .to_owned(),
        "CREATE VIEW minder_addressed_actions AS
           SELECT *,emitted_at AS at_ms FROM action WHERE target_role IN ('lane','lead')"
            .to_owned(),
        "CREATE VIEW tick_actions_unexecuted_by_minder AS
           SELECT a.id AS action_id,a.kind,a.lane,a.due_by AS at_ms,a.target_role,a.target_addr
           FROM minder_addressed_actions a
           WHERE a.due_by <= COALESCE((SELECT MAX(relay_ms) FROM run),0)
             AND NOT EXISTS (SELECT 1 FROM action_send s WHERE s.action_id=a.id)
             AND (a.kind<>'answer_dialog' OR NOT EXISTS (
               SELECT 1 FROM dialog_answer d WHERE d.action_id=a.id))"
            .to_owned(),
        "CREATE VIEW minder_turns_without_committed_tick AS
           SELECT s.session_addr,s.session_role,s.started_at AS at_ms
           FROM session_turn s
           WHERE s.session_role='minder' AND NOT EXISTS (
             SELECT 1 FROM tick t WHERE t.session_addr=s.session_addr
               AND t.committed_at >= s.started_at
               AND t.committed_at <= s.started_at +
                 COALESCE((SELECT tick_interval_s FROM run ORDER BY sequence DESC LIMIT 1),300)*1000)
             AND s.started_at <= COALESCE((SELECT MAX(relay_ms) FROM run),0) -
                 COALESCE((SELECT tick_interval_s FROM run ORDER BY sequence DESC LIMIT 1),300)*1000"
            .to_owned(),
        "CREATE VIEW session_turn_rows AS
           SELECT session_addr,session_role,started_at,started_at AS at_ms FROM session_turn"
            .to_owned(),
        "CREATE VIEW minder_orders_unconfirmed_at_deadline AS
           SELECT *,deadline_at AS at_ms FROM order_focus_at_deadline
           WHERE sender_role='minder' AND outcome<>'confirmed'"
            .to_owned(),
        "CREATE VIEW unconfirmed_orders_not_resent_or_escalated AS
           SELECT * FROM minder_orders_unconfirmed_at_deadline
           WHERE followup_at IS NULL OR followup_at > deadline_at +
             COALESCE((SELECT tick_interval_s FROM run ORDER BY sequence DESC LIMIT 1),300)*1000"
            .to_owned(),
        "CREATE VIEW cycle_events AS
           SELECT cycle_id,session_addr,started_at,ended_at,ended_reason,interval_s,
                  COALESCE(ended_at,started_at) AS at_ms FROM cycle_event"
            .to_owned(),
        "CREATE VIEW dialog_answer_violations AS
           SELECT *,answered_at AS at_ms FROM dialog_answer WHERE COALESCE(violation,'')<>''"
            .to_owned(),
        "CREATE VIEW answer_dialog_latency_max AS
           SELECT CASE WHEN COUNT(*)=0 THEN NULL ELSE MAX(
                    (COALESCE(d.answered_at,(SELECT MAX(relay_ms) FROM run))-a.emitted_at) /
                    (COALESCE((SELECT tick_interval_s FROM run ORDER BY sequence DESC LIMIT 1),300)*1000.0))
                    END AS max_cycles,
                  COUNT(*) AS sample_count,MAX(COALESCE(d.answered_at,a.emitted_at)) AS at_ms
           FROM action a LEFT JOIN dialog_answer d ON d.action_id=a.id
           WHERE a.kind='answer_dialog'"
            .to_owned(),
        "CREATE VIEW lane_orders_due AS
           WITH latest AS (SELECT * FROM relay_order_snapshot
                           ORDER BY observed_at DESC,snapshot_id DESC LIMIT 1)
           SELECT o.order_id, o.recipient AS lane, o.expect_ticket, o.deadline_at AS at_ms,
                  o.deadline_at, o.enqueued_at AS sent_at, o.acted_at, o.state, o.expired,
                  NULL AS focus_ticket, NULL AS focus_state, NULL AS outcome,
                  1 AS available, NULL AS unavailable_reason
           FROM relay_order o JOIN latest s USING(snapshot_id)
           WHERE s.available=1 AND o.deadline_at<=s.observed_at
             AND NOT EXISTS (
               SELECT 1 FROM relay_order later
               WHERE later.snapshot_id=o.snapshot_id AND later.recipient=o.recipient
                 AND (later.enqueued_at,later.order_id)>(o.enqueued_at,o.order_id)
                 AND later.enqueued_at<=o.deadline_at)
           UNION ALL
           SELECT NULL,NULL,NULL,s.observed_at,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,
                  0,s.unavailable_reason FROM latest s WHERE s.available=0
           UNION ALL
           SELECT NULL,NULL,NULL,(SELECT MAX(relay_ms) FROM run),NULL,NULL,NULL,NULL,NULL,
                  NULL,NULL,NULL,0,'relay all-orders source has not been read'
           WHERE NOT EXISTS (SELECT 1 FROM relay_order_snapshot)"
            .to_owned(),
        "CREATE VIEW lane_orders_not_taken_up AS
           SELECT * FROM lane_orders_due WHERE available=0
           UNION ALL
           SELECT order_id,lane,expect_ticket,at_ms,deadline_at,sent_at,acted_at,state,expired,
                  focus_ticket,focus_state,outcome,available,unavailable_reason
           FROM lane_orders_due WHERE available=1 AND expired=1 AND acted_at IS NULL"
            .to_owned(),
        "CREATE VIEW failed_probe_ticks AS
           SELECT defect_id,host,source,tick_id,at_ms FROM failed_probe_tick"
            .to_owned(),
        "CREATE VIEW probe_failure_without_routed_defect AS
           SELECT defect_id, host, source, first_seen_at AS at_ms, routed_ticket, routed_at
           FROM defect WHERE source LIKE 'probe:%' AND routed_ticket IS NULL AND closed_at IS NULL"
            .to_owned(),
    ]
}

/// Views readable through `fleet ledger query`.
pub const QUERYABLE: &[&str] = &[
    "lanes_with_ready_work",
    "idle_lane_with_ready_work_ticks",
    "idle_lane_episodes_with_ready_work",
    "idle_lane_episode_with_ready_work_max",
    "idle_minutes",
    "defect_rows",
    "idle_capacity_minutes_with_ready_work",
    "time_to_action_p90",
    "missed_idle_minutes",
    "detection_to_action_p90",
    "assignment_confirm_latency_p90",
    "drift_lane_minutes_share",
    "blocked_lane_minutes",
    "idle_lane_minutes_during_build_pause",
    "need_ready_tickets_actions",
    "need_ready_tickets_due",
    "need_ready_tickets_without_planner_order",
    "lane_orders_due",
    "lane_orders_not_taken_up",
    "failed_probe_ticks",
    "probe_failure_without_routed_defect",
    "plan_coverage_gap_rows",
    "plan_coverage_gap_open_cycles_max",
    "short_queue_open_cycles_max",
    "drift_findings",
    "drift_finding_open_cycles_max",
    "assignments_sent",
    "unconfirmed_assign_results",
    "unconfirmed_assignments_without_followup",
    "assignment_confirm_latency_max",
    "ladder_rungs_emitted",
    "escalation_rung",
    "orchestrator_rungs_emitted",
    "orchestrator_rung_open_cycles_max",
    "escalation_rung_send_latency_max",
    "escalation_rungs_suppressed",
    "add_lane_action_open_cycles_max",
    "act_on_undetermined_actions",
    "push_next_ticket_actions",
    "answer_dialog_actions",
    "escalate_block_actions",
    "add_lane_actions",
    "act_on_undetermined_unexecuted_ticks",
    "minder_addressed_actions",
    "tick_actions_unexecuted_by_minder",
    "minder_turns_without_committed_tick",
    "session_turn_rows",
    "session_turn_stream_coverage",
    "minder_orders_unconfirmed_at_deadline",
    "unconfirmed_orders_not_resent_or_escalated",
    "cycle_events",
    "dialog_answer_violations",
    "answer_dialog_latency_max",
    "lane_tick_rows",
    "lane_span",
    "cycle_missed",
    "discovery_steps",
    "discovery_steps_violating_ramp",
    "discovery_steps_judged_without_demand",
    "discovery_knee_signals",
    "discovery_raised_past_knee",
    "discovery_runs_ended",
    "discovery_runs_without_recorded_level",
    "unrecorded_hosts_with_demand",
    "unrecorded_hosts_without_discovery_run",
    "unrecorded_hosts_ready_for_discovery",
    "hosts_with_recorded_level",
    "host_step_downs",
    "raises_during_cool_off",
    "discovery_runs",
    "discovery_runs_per_host_day",
    "discovery_runs_per_host_day_max",
    "host_minutes_above_recorded_level_outside_bounded_discovery",
    "overload_events_without_draining_mark_in_cycle",
    "lane_removals",
    "lane_removals_before_drain",
    "lane_change_unrecorded",
    "lanes_lost",
    "draining_lanes",
    "drains_over_cap_without_escalation",
    "discovery_inconclusive_windows",
    "no_demand_runs_not_ended",
    "discovery_slot_saturated_windows",
    "discovery_raised_past_slot_saturation",
    "lane_count_reductions",
    "lane_reductions_without_throttle_evidence",
    "host_output_share_of_recorded_level",
];

/// Views without an `at_ms` column, which refuse `--since`.
pub const UNDATED: &[&str] = &["idle_lane_episode_with_ready_work_max"];

/// `(table, column)` pairs that retention prunes by age: a row goes once `column` is set and
/// older than the cutoff. Interval-like rows are keyed on the time they close, so a row that is
/// still open (a gap, a finding, an unresolved action, an open cycle, the current capacity
/// level) is never pruned however old it is.
pub const PRUNE: &[(&str, &str)] = &[
    ("lane_tick", "observed_ms"),
    ("host_tick", "observed_ms"),
    ("defect", "closed_at"),
    ("action", "resolved_at"),
    ("lane_interval", "ended_at"),
    ("assignment_history", "replaced_at"),
    ("ground_truth_sample", "at"),
    ("audit_sample", "at"),
    ("saturation_sample", "at"),
    ("plan_coverage_gap", "closed_at"),
    ("drift_finding", "closed_at"),
    ("order_focus_at_deadline", "deadline_at"),
    ("action_send", "sent_at"),
    ("session_turn", "started_at"),
    ("cycle_event", "ended_at"),
    ("cycle_missed", "detected_at"),
    ("lane_lead", "observed_at"),
    ("dialog_answer", "answered_at"),
    ("capacity_run", "ended_at"),
    ("capacity_step", "window_end"),
    ("capacity_level", "superseded_at"),
    ("capacity_event", "at"),
    ("host_output_sample", "at"),
    ("host_build_pause_interval", "end_ms"),
    ("capacity_throttle_sample", "at"),
    ("tick", "committed_at"),
];
