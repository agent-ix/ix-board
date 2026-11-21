// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::*;
use super::{
    timeline::{ExecutionSample, Member},
    wire::AssignmentStatus,
};
use crate::WorkflowStateType;
use std::collections::BTreeMap;
fn config(path: &std::path::Path) -> Config {
    Config {
        version: 1,
        listen: "127.0.0.1:0".parse().unwrap(),
        state_path: path.join("metrics.sqlite"),
        max_bytes: 1024 * 1024,
        retention_seconds: 604800,
        activity_interval_seconds: 30,
        tracker_interval_seconds: 300,
        scopes: vec![ScopeConfig {
            team_id: "factory".into(),
            scope_id: "completion".into(),
            selectors: Selectors {
                team: Some("QSL".into()),
                ..Default::default()
            },
            leads: vec!["draco:QSL:code".into()],
            herdr_teams: vec!["QSL".into()],
            retro_config: path.join("retro.json"),
        }],
    }
}
fn binding() -> Binding {
    Binding {
        machine: "draco".into(),
        pane: "w1:p1".into(),
        session: "parent".into(),
        process_id: 42,
        kind: "codex".into(),
        address: "draco:QSL:code".into(),
    }
}
fn event(at: i64, change: Change) -> Event {
    Event {
        id: format!("{at}-0"),
        at_ms: at,
        binding: binding(),
        change,
    }
}
fn assignment(role: &str, status: AssignmentStatus) -> Assignment {
    Assignment {
        agent_id: "child".into(),
        name: "private nickname".into(),
        ticket: "QSL-12".into(),
        role: role.into(),
        status,
        pane: None,
        session: None,
        reported_at_ms: 1000,
    }
}
fn state(config: &Config) -> State {
    let mut state = State::default();
    state.configure(config);
    let scope = state.scopes.get_mut("factory:completion").unwrap();
    scope.members.insert(
        "issue-native-id".into(),
        Member {
            ticket: "QSL-12".into(),
            state: WorkflowStateType::Started,
            leaf: true,
        },
    );
    state
}
fn sample(at: i64, working: bool) -> Vec<ExecutionSample> {
    vec![
        ExecutionSample {
            machine: "draco".into(),
            session: "parent".into(),
            parent_session: None,
            working: false,
            at_ms: at,
        },
        ExecutionSample {
            machine: "draco".into(),
            session: "child".into(),
            parent_session: Some("parent".into()),
            working,
            at_ms: at,
        },
    ]
}

/// Provenance: SWM-180, SWM-181; short stages and read replay.
/// Trace: FR-030-AC-1
#[test]
fn short_stages_survive_removal_and_replayed_events_do_not_count_twice() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    let start = event(
        1000,
        Change::Agent {
            agent: assignment("coder", AssignmentStatus::Running),
        },
    );
    assert!(state.apply(&config, &start).unwrap());
    assert!(!state.apply(&config, &start).unwrap());
    state
        .apply(
            &config,
            &event(
                31000,
                Change::Agent {
                    agent: assignment("reviewer", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    state
        .apply(
            &config,
            &event(
                61000,
                Change::Agent {
                    agent: assignment("reviewer", AssignmentStatus::Done),
                },
            ),
        )
        .unwrap();
    state
        .apply(
            &config,
            &event(
                62000,
                Change::Remove {
                    agent_id: "child".into(),
                },
            ),
        )
        .unwrap();
    let scope = &state.scopes["factory:completion"];
    assert!(scope.assignments.is_empty());
    assert_eq!(scope.totals["assignment_seconds:coding:assigned"], 30.0);
    assert_eq!(scope.totals["assignment_seconds:review:assigned"], 30.0);
    assert_eq!(scope.stages["review"], vec![(61000, 30.0)]);
}

/// Provenance: SWM-181; reports are not execution, pauses/outages never improve throughput.
/// Trace: FR-030-AC-2
#[test]
fn execution_needs_consecutive_verified_observations_and_preserves_stalls() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    state.observe(&config, &sample(1000, true), 1000, true);
    state.observe(&config, &sample(31000, true), 31000, true);
    assert_eq!(
        state.scopes["factory:completion"].totals["execution_seconds"],
        30.0
    );
    state.observe(&config, &sample(61000, false), 61000, true);
    assert_eq!(
        state.scopes["factory:completion"].totals["execution_seconds"],
        30.0
    );
    assert_eq!(state.scopes["factory:completion"].covered_seconds, 60.0);
    state
        .apply(&config, &event(62000, Change::Activity { paused: true }))
        .unwrap();
    state.observe(&config, &sample(91000, true), 91000, true);
    state.observe(&config, &sample(121000, true), 121000, true);
    assert_eq!(
        state.scopes["factory:completion"].totals["execution_seconds"],
        60.0
    );
    state.observe(&config, &[], 151000, false);
    assert!(!state.scopes["factory:completion"].lifecycle_ok);
    state.observe(&config, &sample(181000, true), 181000, true);
    assert_eq!(
        state.scopes["factory:completion"].totals["execution_seconds"],
        60.0
    );
}

/// Provenance: SWM-181; reused pane labels must not inherit the prior session's assignments.
/// Trace: FR-030-AC-2
#[test]
fn a_replacement_session_clears_previous_incarnation_assignments() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    let mut replacement = event(2000, Change::Activity { paused: false });
    replacement.binding.session = "replacement".into();
    replacement.binding.process_id = 43;
    state.apply(&config, &replacement).unwrap();
    assert!(state.scopes["factory:completion"].assignments.is_empty());
    state.observe(&config, &sample(3000, true), 3000, true);
    assert!(!state.scopes["factory:completion"].lifecycle_ok);
}

/// Provenance: SWM-181; shared assignments cannot be charged twice.
/// Trace: FR-030-AC-2
#[test]
fn overlapping_scopes_do_not_double_credit_execution() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path());
    let mut second = config.scopes[0].clone();
    second.scope_id = "overlap".into();
    config.scopes.push(second);
    let mut state = state(&config);
    state.scopes.get_mut("factory:overlap").unwrap().members =
        state.scopes["factory:completion"].members.clone();
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    state.observe(&config, &sample(1000, true), 1000, true);
    state.observe(&config, &sample(31000, true), 31000, true);
    assert!(
        state
            .scopes
            .values()
            .all(|s| !s.totals.contains_key("execution_seconds"))
    );
}

/// Provenance: SWM-181; cursor/events/current totals form one durable checkpoint.
/// Trace: FR-030-AC-1
#[test]
fn journal_restores_cursor_and_totals_and_prunes_older_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path());
    config.retention_seconds = 1;
    let mut state = state(&config);
    let e = event(
        1000,
        Change::Agent {
            agent: assignment("coder", AssignmentStatus::Running),
        },
    );
    state.apply(&config, &e).unwrap();
    {
        let mut journal = Journal::open(&config.state_path, &config).unwrap();
        journal
            .checkpoint(&state, std::slice::from_ref(&e), 1000)
            .unwrap();
        journal
            .checkpoint(&state, std::slice::from_ref(&e), 1000)
            .unwrap();
        assert_eq!(journal.events().unwrap().len(), 1);
        assert!(journal.bytes().unwrap() <= config.max_bytes);
    }
    let mut journal = Journal::open(&config.state_path, &config).unwrap();
    assert_eq!(journal.load().unwrap().cursor, "1000-0");
    journal.checkpoint(&state, &[], 3000).unwrap();
    assert!(journal.events().unwrap().is_empty());
    assert!(
        journal.load().unwrap().scopes["factory:completion"]
            .assignments
            .contains_key("draco:parent:42:child")
    );
}

/// Provenance: SWM-181; no unknown-as-zero, private IDs as labels, or rolling counters.
/// Trace: FR-030-AC-5
#[test]
fn export_omits_unknown_values_and_uses_only_bounded_labels() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let state = state(&config);
    let text = render(&config, &state, 1234, 1000).unwrap();
    assert!(text.contains(
        "ix_team_source_available{team_id=\"factory\",scope_id=\"completion\",source=\"tracker\"} 0"
    ));
    assert!(!text.contains("ix_team_path_closed_24h{"));
    assert!(!text.contains("ix_team_scope_members{"));
    for private in [
        "QSL-12",
        "issue-native-id",
        "private nickname",
        "private-parent-session-identity",
        "private-child-session-identity",
    ] {
        assert!(!text.contains(private), "private identity {private} leaked");
    }
    assert!(text.contains("# TYPE ix_team_execution_seconds_total counter"));
}

/// Provenance: SWM-181; unsupported schemas cannot overwrite evidence.
/// Trace: FR-030-AC-4
#[test]
fn unsupported_journal_and_unknown_config_fields_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let conn = rusqlite::Connection::open(&config.state_path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(Journal::open(&config.state_path, &config).is_err());
    let conn = rusqlite::Connection::open(&config.state_path).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        99
    );
    assert!(serde_json::from_str::<Selectors>(r#"{"team":"QSL","typo":true}"#).is_err());
    assert!(config.validate().is_ok());
}

/// Provenance: SWM-181; scalar throughput snapshots retain gauge semantics.
/// Trace: FR-030-AC-5
#[test]
fn windowed_metrics_are_gauges_and_expired_sources_lose_values() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    let s = state.scopes.get_mut("factory:completion").unwrap();
    s.tracker_ok = true;
    s.tracker_at_ms = 1000;
    s.record = Some(crate::retro::Record {
        schema_version: 1,
        kind: crate::retro::RecordKind::Snapshot,
        taken_at: "1970-01-01T00:00:01Z".into(),
        team: "factory".into(),
        window: "24h".into(),
        metrics: BTreeMap::from([(
            "path_closed".into(),
            crate::retro::Metric {
                id: "path_closed".into(),
                status: crate::retro::Status::Ok,
                value: Some(5.0),
                unit: "issues".into(),
                direction: crate::retro::Direction::Up,
                source: "linear".into(),
                query: "fixture".into(),
                reason: None,
            },
        )]),
        department: None,
    });
    let text = render(&config, &state, 1234, 1000).unwrap();
    assert!(text.contains("# TYPE ix_team_path_closed_24h gauge"));
    assert!(text.contains("scope_id=\"completion\"} 5"));
    assert!(
        !render(&config, &state, 1234, 900000)
            .unwrap()
            .contains("ix_team_path_closed_24h{")
    );
}

/// Provenance: SWM-181; an outage cannot advance accounting beyond source coverage.
/// Trace: FR-030-AC-3
#[test]
fn delayed_completion_after_history_outage_remains_replayable() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    state.history_at_ms = 31000;
    state.observe(&config, &[], 61000, false);
    assert_eq!(
        state.scopes["factory:completion"].assignments["draco:parent:42:child"].accounted_ms,
        31000
    );
    state
        .apply(
            &config,
            &event(
                45000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Done),
                },
            ),
        )
        .unwrap();
    assert!(state.scopes["factory:completion"].assignments.is_empty());
    assert_eq!(
        state.scopes["factory:completion"].stages["coding"],
        vec![(45000, 44.0)]
    );
}

/// Provenance: SWM-181; metadata updates must replace stale worker identity.
/// Trace: FR-030-AC-3
#[test]
fn same_stage_upsert_updates_worker_binding_without_resetting_start() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    let mut updated = assignment("coder", AssignmentStatus::Running);
    updated.pane = Some("w2:p4".into());
    updated.session = Some("new-worker-session".into());
    state
        .apply(&config, &event(2000, Change::Agent { agent: updated }))
        .unwrap();
    let a = &state.scopes["factory:completion"].assignments["draco:parent:42:child"];
    assert_eq!(a.assignment.session.as_deref(), Some("new-worker-session"));
    assert_eq!(a.started_ms, 1000);
}

/// Provenance: SWM-181; failed attempts do not advance last-success or invent population zeros.
/// Trace: FR-030-AC-5
#[test]
fn failed_lifecycle_keeps_last_success_and_omits_current_ages() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    state.observe(&config, &sample(1000, true), 1000, true);
    state.observe(&config, &[], 31000, false);
    let text = render(&config, &state, 1000, 31000).unwrap();
    assert!(text.contains("source=\"lifecycle\"} 1\n")); // last successful timestamp is one Unix second.
    assert!(!text.contains("ix_team_stage_oldest_age_seconds{"));
    assert!(!text.contains("ix_team_unattributed_executing_agents{"));
}

/// Provenance: SWM-181; snapshots establish only current assignment facts.
/// Trace: FR-030-AC-7
#[test]
fn bootstrap_never_invents_historical_stage_starts() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    state.bootstrap(
        &config,
        &relay_work_wire::Snapshot {
            version: 1,
            observed_at_ms: 5000,
            binding: binding(),
            scope: None,
            assignments: vec![assignment("coder", AssignmentStatus::Running)],
            paused: None,
        },
    );
    let a = &state.scopes["factory:completion"].assignments["draco:parent:42:child"];
    assert!(!a.start_known);
    assert_eq!(a.started_ms, 5000);
    state
        .apply(
            &config,
            &event(
                4000,
                Change::Agent {
                    agent: assignment("coder", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    assert!(!state.scopes["factory:completion"].assignments["draco:parent:42:child"].start_known);
}

/// Provenance: SWM-181; Unicode-heavy events stay within the actual byte ceiling.
/// Trace: FR-030-AC-4
#[test]
fn unicode_event_retention_evicts_before_the_page_limit_is_hit() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    let mut journal = Journal::open(&config.state_path, &config).unwrap();
    for at in 1..=300 {
        let mut a = assignment("coder", AssignmentStatus::Running);
        a.name = "🦀".repeat(1800);
        let e = event(at, Change::Agent { agent: a });
        state.cursor = e.id.clone();
        journal.checkpoint(&state, &[e], at).unwrap();
    }
    assert!(journal.bytes().unwrap() <= config.max_bytes);
    assert!(journal.events().unwrap().len() < 300);
    assert_eq!(journal.load().unwrap().cursor, "300-0");
}

/// Trace: FR-030-AC-2, FR-030-AC-6
#[test]
fn completed_stages_do_not_leak_into_a_disjoint_scope_with_the_same_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path());
    let mut other = config.scopes[0].clone();
    other.scope_id = "disjoint".into();
    config.scopes.push(other);
    let mut state = state(&config);
    state
        .scopes
        .get_mut("factory:disjoint")
        .unwrap()
        .members
        .insert(
            "other".into(),
            Member {
                ticket: "QSL-13".into(),
                state: WorkflowStateType::Started,
                leaf: true,
            },
        );
    state
        .apply(
            &config,
            &event(
                1000,
                Change::Agent {
                    agent: assignment("reviewer", AssignmentStatus::Running),
                },
            ),
        )
        .unwrap();
    state
        .apply(
            &config,
            &event(
                31000,
                Change::Agent {
                    agent: assignment("reviewer", AssignmentStatus::Done),
                },
            ),
        )
        .unwrap();
    assert_eq!(
        state.scopes["factory:completion"].stages["review"],
        vec![(31000, 30.0)]
    );
    assert!(state.scopes["factory:disjoint"].stages.is_empty());
}

/// Trace: FR-030-AC-2, FR-030-AC-7
#[test]
fn a_lead_working_a_different_reported_scope_has_unknown_parent_attribution() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let mut state = state(&config);
    let different = relay_work_wire::ReportedScope {
        initiatives: vec![],
        projects: vec!["another-project".into()],
        tickets: vec![],
        reported_at_ms: 1000,
    };
    state.bootstrap(
        &config,
        &relay_work_wire::Snapshot {
            version: 1,
            observed_at_ms: 1000,
            binding: binding(),
            scope: Some(different),
            assignments: vec![],
            paused: None,
        },
    );
    let samples = vec![ExecutionSample {
        machine: "draco".into(),
        session: "parent".into(),
        parent_session: None,
        working: true,
        at_ms: 1000,
    }];
    state.observe(&config, &samples, 1000, true);
    assert_eq!(
        state.scopes["factory:completion"].unattributed_executing_agents,
        1
    );
    assert!(!state.scopes["factory:completion"].lifecycle_ok);
    assert!(
        !state.scopes["factory:completion"]
            .totals
            .contains_key("execution_seconds")
    );
}
