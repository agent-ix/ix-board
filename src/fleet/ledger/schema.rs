// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The ledger schema (FR-039): version 1 tables, the additive version 2 migration, the views.
//!
//! Times are relay server milliseconds. Tables whose writers are other tickets are created
//! empty so the schema is one unit under one migration.

/// The schema version this build writes.
pub const VERSION: i64 = 6;

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
         ('deficit_nonzero','deficit_coverage')),
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
       reason TEXT, evidence_kind TEXT, cycle_id INTEGER, at INTEGER NOT NULL, ceiling_until INTEGER)",
    "CREATE TABLE host_output_sample(
       host TEXT NOT NULL, at INTEGER NOT NULL, lanes INTEGER, tickets_closed INTEGER,
       prs_merged INTEGER, commits_pushed INTEGER, PRIMARY KEY(host, at))",
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
    "CREATE TABLE capacity_throttle_sample(
       host TEXT NOT NULL, at INTEGER NOT NULL, interval_s INTEGER NOT NULL,
       throttled_share REAL, coverage REAL, unavailable_reason TEXT,
       source_basis TEXT NOT NULL, PRIMARY KEY(host, at))",
    "ALTER TABLE capacity_event ADD COLUMN source_basis TEXT",
    "ALTER TABLE host_output_sample ADD COLUMN source_basis TEXT NOT NULL DEFAULT '{}'",
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
          OR window_end - rampup_end < 3600000)",
    "CREATE VIEW discovery_steps_judged_without_demand AS
       SELECT s.run_id,s.step_no,s.host,s.lanes,s.valid,s.knee_signal,s.window_end AS at_ms,
              'new_step_after_invalid' AS violation
       FROM capacity_step s WHERE s.repeat_of IS NULL AND EXISTS (
         SELECT 1 FROM capacity_step p WHERE p.run_id=s.run_id AND p.step_no<s.step_no
           AND p.valid=0 AND p.step_no=(SELECT MAX(x.step_no) FROM capacity_step x
                                      WHERE x.run_id=s.run_id AND x.step_no<s.step_no))
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
    "CREATE VIEW unrecorded_hosts_at_zero_deficit AS
       SELECT s.*,s.at AS at_ms FROM saturation_sample s
       WHERE s.deficit=0 AND NOT EXISTS (
         SELECT 1 FROM capacity_level l WHERE lower(l.host)=lower(s.host)
           AND l.superseded_at IS NULL)",
    "CREATE VIEW unrecorded_hosts_without_discovery_run AS
       SELECT s.*,s.at AS at_ms FROM unrecorded_hosts_at_zero_deficit s
       WHERE NOT EXISTS (SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(s.host)
         AND r.started_at BETWEEN s.at AND s.at+600000)",
    "CREATE VIEW hosts_with_recorded_level AS
       SELECT *,recorded_at AS at_ms FROM capacity_level WHERE superseded_at IS NULL",
    "CREATE VIEW host_step_downs AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind='step_down'",
    "CREATE VIEW raises_during_cool_off AS
       SELECT a.*,a.at AS at_ms FROM capacity_event a JOIN capacity_event d
         ON lower(d.host)=lower(a.host) AND d.kind='step_down' AND a.kind='lane_added'
          AND a.at>=d.at AND d.ceiling_until IS NOT NULL AND a.at<d.ceiling_until",
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
                 AND f.kind IN ('drain_done','lane_removed') AND f.at>=d.at))
         GROUP BY lower(host),observed_ms),
       spans AS (
         SELECT host,observed_ms,lanes,
           LEAD(observed_ms) OVER(PARTITION BY lower(host) ORDER BY observed_ms) AS next_ms
         FROM lane_counts)
       SELECT l.host,l.level,
         SUM(CASE WHEN s.lanes>l.level AND NOT EXISTS (
           SELECT 1 FROM capacity_run r WHERE lower(r.host)=lower(l.host)
             AND r.ended_at IS NULL AND r.started_at<=s.observed_ms)
           THEN s.next_ms-s.observed_ms ELSE 0 END)/60000.0 AS minutes,
         MAX(s.observed_ms) AS at_ms
       FROM spans s JOIN capacity_level l ON lower(l.host)=lower(s.host)
       WHERE l.superseded_at IS NULL AND s.next_ms IS NOT NULL
       GROUP BY l.host,l.level",
    "CREATE VIEW overload_events_without_draining_mark_in_cycle AS
       SELECT o.*,o.at AS at_ms FROM capacity_event o WHERE o.kind='overload_event'
         AND NOT EXISTS (SELECT 1 FROM capacity_event d WHERE lower(d.host)=lower(o.host)
           AND d.kind='draining_mark' AND d.at>=o.at AND d.at<=o.at+600000)",
    "CREATE VIEW lane_removals AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind='lane_removed'",
    "CREATE VIEW lane_removals_before_drain AS
       SELECT r.*,r.at AS at_ms FROM capacity_event r WHERE r.kind='lane_removed'
         AND NOT EXISTS (SELECT 1 FROM capacity_event d WHERE lower(d.lane)=lower(r.lane)
           AND d.kind='drain_done' AND d.at<=r.at)",
    "CREATE VIEW lane_change_unrecorded AS
       SELECT *,at AS at_ms FROM capacity_event
       WHERE kind IN ('lane_added','lane_removed') AND reason='lane_change_unrecorded'",
    "CREATE VIEW lanes_lost AS
       SELECT *,at AS at_ms FROM capacity_event WHERE kind='lane_lost'",
    "CREATE VIEW draining_lanes AS
       SELECT d.*,d.at AS at_ms FROM capacity_event d WHERE d.kind='draining_mark'
         AND NOT EXISTS (SELECT 1 FROM capacity_event f WHERE lower(f.lane)=lower(d.lane)
           AND f.kind IN ('drain_done','lane_removed') AND f.at>=d.at)",
    "CREATE VIEW drains_over_cap_without_escalation AS
       SELECT d.*,d.at AS at_ms FROM draining_lanes d WHERE d.at<=
         (SELECT MAX(relay_ms) FROM run)-14400000 AND NOT EXISTS (
           SELECT 1 FROM action_send x JOIN action a ON a.id=x.action_id
           WHERE a.kind='escalate_drain' AND lower(a.lane)=lower(d.lane))",
    "CREATE VIEW discovery_inconclusive_windows AS
       SELECT *,window_end AS at_ms FROM capacity_step WHERE valid=0",
    "CREATE VIEW no_demand_runs_not_ended AS
       SELECT r.*,r.started_at AS at_ms FROM capacity_run r WHERE r.ended_at IS NULL
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
       WITH current_level AS (SELECT * FROM capacity_level WHERE superseded_at IS NULL),
       level_step AS (SELECT l.host,l.output_basis,s.host_output_per_hour
         FROM current_level l,json_each(l.evidence_steps) e
         JOIN capacity_step s ON s.run_id=l.run_id AND s.step_no=CAST(e.value AS INTEGER)),
       latest AS (SELECT h.*,ROW_NUMBER() OVER(PARTITION BY lower(host) ORDER BY at DESC) AS rn
         FROM host_output_sample h)
       SELECT l.host,l.output_basis,
         CASE l.output_basis WHEN 'commits' THEN h.commits_pushed
           ELSE CASE WHEN h.tickets_closed IS NOT NULL AND h.prs_merged IS NOT NULL
             THEN h.tickets_closed+h.prs_merged END END AS output_per_hour,
         l.host_output_per_hour AS recorded_output_per_hour,
         CASE WHEN l.host_output_per_hour>0 AND (CASE l.output_basis
           WHEN 'commits' THEN h.commits_pushed
           ELSE CASE WHEN h.tickets_closed IS NOT NULL AND h.prs_merged IS NOT NULL
             THEN h.tickets_closed+h.prs_merged END END) IS NOT NULL
           THEN 100.0*(CASE l.output_basis WHEN 'commits' THEN h.commits_pushed
           ELSE h.tickets_closed+h.prs_merged END)/l.host_output_per_hour
           ELSE NULL END AS share_percent,h.at AS at_ms
       FROM level_step l JOIN latest h ON lower(h.host)=lower(l.host) AND h.rn=1",
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
           SELECT lane, host, class, ready_work_available AS ready, observed_ms AS start_ms,
                  tick_ms,
                  LEAD(observed_ms) OVER (PARTITION BY lane ORDER BY observed_ms, run_seq)
                    AS end_ms
           FROM lane_tick_rows"
            .to_owned(),
        "CREATE VIEW idle_minutes AS
           SELECT lane, host, date(start_ms / 1000, 'unixepoch') AS day, class,
                  SUM(end_ms - start_ms) / 60000.0 AS minutes,
                  SUM(CASE WHEN ready = 1 THEN end_ms - start_ms ELSE 0 END) / 60000.0
                    AS minutes_with_ready_work,
                  CAST(strftime('%s', date(start_ms / 1000, 'unixepoch')) AS INTEGER) * 1000
                    AS at_ms
           FROM lane_span
           WHERE class <> 'WORKING' AND end_ms IS NOT NULL AND end_ms - start_ms <= 2 * tick_ms
           GROUP BY lane, host, day, class"
            .to_owned(),
        "CREATE VIEW defect_rows AS
           SELECT defect_id, source, host, lane, kind, first_seen_at, first_seen_host_ms,
                  closed_at, routed_ticket, routed_at, first_seen_at AS at_ms
           FROM defect"
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
    "discovery_steps",
    "discovery_steps_violating_ramp",
    "discovery_steps_judged_without_demand",
    "discovery_knee_signals",
    "discovery_raised_past_knee",
    "discovery_runs_ended",
    "discovery_runs_without_recorded_level",
    "unrecorded_hosts_at_zero_deficit",
    "unrecorded_hosts_without_discovery_run",
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
    ("dialog_answer", "answered_at"),
    ("capacity_throttle_sample", "at"),
    ("tick", "committed_at"),
];
