// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Lifecycle collection, reduction, protocol and report acceptance tests.
use ix_board::agents::*;
use ix_trace_rs::trace;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const NOW: u64 = 1_800_000_000;
const IDLE: u64 = 1200;
fn life(kind: &str, records: &str) -> Lifecycle {
    let mut life = Lifecycle::default();
    let adapter = adapter_for(kind).unwrap();
    for line in records.lines() {
        adapter.observe_record(&serde_json::from_str(line).unwrap(), &mut life);
    }
    life
}
fn worker(id: &str, parent: &str, state: WorkerState) -> WorkerObservation {
    WorkerObservation {
        id: AgentId(id.into()),
        parent_id: Some(AgentId(parent.into())),
        agent_path: None,
        declared_depth: None,
        role: WorkerRole::Worker,
        state,
        last_activity_at: Some(NOW - 3600),
        state_changed_at: Some(NOW - 3600),
        timestamp_unavailable_reason: None,
        observed_at: NOW,
        source: EvidenceSource::Adapter,
        provenance: vec!["fixture".into()],
        descendants_complete: true,
        issues: vec![],
    }
}
fn counts(workers: &[WorkerObservation]) -> WorkerCounts {
    count_workers(&AgentId("root".into()), workers, Availability::available())
}
fn ended(at: u64) -> Lifecycle {
    Lifecycle {
        state: WorkerState::Completed,
        state_changed_at: Some(at),
        last_activity_at: Some(at),
        execution_ended_at: Some(at),
        ..Lifecycle::default()
    }
}
fn verdict(life: &Lifecycle, workers: &WorkerCounts, herdr: HerdrState, now: u64) -> Verdict {
    decide(&Evidence {
        herdr,
        tail: Some("❯"),
        lifecycle: life,
        workers,
        active_processes: 0,
        processes_complete: true,
        transcript_complete: true,
        now,
        idle_after: IDLE,
    })
}

#[trace("FR-018-AC-8", "FR-018-AC-9", "FR-018-AC-13", "FR-018-AC-14")]
#[test]
fn eight_file_topology_counts_assignments_and_terminal_graphs_have_exact_zero() {
    let workers = vec![
        worker("a", "root", WorkerState::Running),
        worker("b", "root", WorkerState::Waiting),
        worker("c", "root", WorkerState::Completed),
        worker("d", "root", WorkerState::Completed),
        worker("e", "c", WorkerState::Completed),
        worker("f", "d", WorkerState::Completed),
        worker("g", "root", WorkerState::Stopped),
        worker("h", "root", WorkerState::Stopped),
    ];
    let result = counts(&workers);
    assert_eq!((result.direct, result.nested), (Some(2), Some(0)));
    assert_eq!((counts(&[]).direct, counts(&[]).nested), (Some(0), Some(0)));
    assert_eq!(counts(&workers[2..]).direct, Some(0));
    let mut internal = worker("internal", "root", WorkerState::Unknown);
    internal.role = WorkerRole::Internal;
    assert_eq!(counts(&[internal]).direct, Some(0));
    let mut nested = workers.clone();
    nested.push(worker("n", "a", WorkerState::Running));
    nested.push(workers[0].clone());
    assert_eq!(
        (counts(&nested).direct, counts(&nested).nested),
        (Some(2), Some(1))
    );
}
#[trace("FR-018-AC-13", "FR-018-AC-14")]
#[test]
fn uncertainty_is_scoped_to_proven_depth_and_conflicting_duplicates_are_not_chosen() {
    let mut unknown = worker("nested", "direct", WorkerState::Unknown);
    let direct = worker("direct", "root", WorkerState::Running);
    assert_eq!(
        (
            counts(&[direct.clone(), unknown.clone()]).direct,
            counts(&[direct.clone(), unknown.clone()]).nested
        ),
        (Some(1), None)
    );
    unknown.parent_id = None;
    assert_eq!(counts(&[unknown]).direct, None);
    let mut conflicting = direct.clone();
    conflicting.state = WorkerState::Stopped;
    assert_eq!(counts(&[direct, conflicting]).direct, None);
    let mut unknown_role = worker("x", "root", WorkerState::Running);
    unknown_role.role = WorkerRole::Unknown;
    assert_eq!(
        (
            counts(&[unknown_role.clone()]).direct,
            counts(&[unknown_role]).nested
        ),
        (None, Some(0))
    );
}
#[trace("FR-018-AC-17", "FR-018-AC-18", "FR-018-AC-19")]
#[test]
fn shared_classifier_thresholds_and_pending_wait_precedence() {
    for harness in ["claude", "codex"] {
        let records = match harness {
            "claude" => include_str!("fixtures/agents/claude_done.jsonl"),
            _ => include_str!("fixtures/agents/codex_done.jsonl"),
        };
        let ended = life(harness, records);
        let at = ended.last_activity_at.unwrap();
        let empty = counts(&[]);
        assert_eq!(
            verdict(&ended, &empty, HerdrState::Done, at + 1199),
            Verdict::Recent,
            "{harness}"
        );
        assert_eq!(
            verdict(&ended, &empty, HerdrState::Done, at + 1200),
            Verdict::Idle,
            "{harness}"
        );
        assert_eq!(
            verdict(&ended, &empty, HerdrState::Working, at + 1),
            Verdict::Unknown,
            "{harness}"
        );
        assert_eq!(
            verdict(&ended, &empty, HerdrState::Blocked, at + 1),
            Verdict::Prompt,
            "{harness}"
        );
        let asking = life(
            harness,
            match harness {
                "claude" => include_str!("fixtures/agents/claude_asking.jsonl"),
                _ => include_str!("fixtures/agents/codex_asking.jsonl"),
            },
        );
        assert_eq!(
            verdict(&asking, &empty, HerdrState::Done, at + 1),
            Verdict::Asking,
            "{harness}"
        );
        let mut running = Lifecycle {
            state: WorkerState::Running,
            last_activity_at: Some(at),
            ..Lifecycle::default()
        };
        assert_eq!(
            verdict(&running, &empty, HerdrState::Working, at + 1),
            Verdict::Working,
            "{harness}"
        );
        assert_eq!(
            verdict(&running, &empty, HerdrState::Done, at + 3600),
            Verdict::Idle,
            "{harness}"
        );
        let active = counts(&[worker("child", "root", WorkerState::Running)]);
        assert_eq!(
            verdict(&running, &active, HerdrState::Done, at + 3600),
            Verdict::Waiting,
            "{harness}"
        );
        running.state = WorkerState::Waiting;
        running.state_changed_at = Some(at);
        assert_eq!(
            verdict(&running, &active, HerdrState::Working, at + 3600),
            Verdict::Waiting,
            "{harness}"
        );
        assert_eq!(
            verdict(&running, &empty, HerdrState::Working, at + 899),
            Verdict::Unknown,
            "{harness}"
        );
        assert_eq!(
            verdict(&running, &empty, HerdrState::Working, at + 900),
            Verdict::WaitingStuck,
            "{harness}"
        );
        let incomplete = count_workers(&AgentId("root".into()), &[], Availability::default());
        assert_eq!(
            verdict(&running, &incomplete, HerdrState::Working, at + 900),
            Verdict::Unknown,
            "{harness}"
        );
    }
}
#[trace("FR-018-AC-10", "FR-018-AC-12", "FR-018-AC-19")]
#[test]
fn codex_generations_and_correlated_snapshots_reject_incidental_outputs() {
    let adapter = adapter_for("codex").unwrap();
    let mut state = life("codex", include_str!("fixtures/agents/codex_done.jsonl"));
    adapter.observe_record(
        &json!({"type":"session_meta","payload":{"id":"fixture-session","source":"cli"}}),
        &mut state,
    );
    assert_eq!(state.state, WorkerState::Completed);
    let at = "2026-10-18T10:00:11Z";
    let output = json!({"type":"response_item","timestamp":at,"payload":{"type":"function_call_output","call_id":"list","output":json!({"agents":[{"agent_name":"/root/child","agent_status":{"completed":"SECRET RESULT"}}]}).to_string()}});
    adapter.observe_record(&output, &mut state);
    assert!(state.child_states.is_empty());
    adapter.observe_record(&json!({"type":"response_item","timestamp":at,"payload":{"type":"function_call","name":"list_agents","call_id":"list","arguments":"{}"}}),&mut state);
    adapter.observe_record(&output, &mut state);
    assert_eq!(state.child_states["/root/child"].1, WorkerState::Completed);
    assert!(!serde_json::to_string(&state).unwrap().contains("SECRET"));
    adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-10-18T10:00:12Z","payload":{"type":"task_started","turn_id":"turn-2"}}),&mut state);
    assert_eq!(state.state, WorkerState::Running);
    assert!(state.assistant.is_none());
    adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-10-18T10:00:13Z","payload":{"type":"task_complete","turn_id":"turn-1"}}),&mut state);
    assert!(state.issues.contains(&"generation_conflict".into()));
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn request(kind: HarnessKind) -> ProbeRequest {
    ProbeRequest {
        sessions: vec![SessionQuery {
            kind,
            session_id: "root".into(),
        }],
        cwds: vec![],
    }
}
fn claude_root(home: &Path) {
    write(
        home,
        ".claude/projects/test/root.jsonl",
        include_str!("fixtures/agents/claude_done.jsonl"),
    );
}
fn claude_child(home: &Path, id: &str, metadata: Value) {
    let transcript = include_str!("fixtures/agents/claude_done.jsonl")
        .lines()
        .map(|line| {
            let mut record: Value = serde_json::from_str(line).unwrap();
            record["agentId"] = json!(id);
            record["sessionId"] = json!("root");
            record["isSidechain"] = json!(true);
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    write(
        home,
        &format!(".claude/projects/test/root/subagents/agent-{id}.jsonl"),
        &transcript,
    );
    write(
        home,
        &format!(".claude/projects/test/root/subagents/agent-{id}.meta.json"),
        &metadata.to_string(),
    );
}
#[trace(
    "FR-018-AC-8",
    "FR-018-AC-9",
    "FR-018-AC-10",
    "FR-018-AC-11",
    "FR-018-AC-15",
    "FR-018-AC-20"
)]
#[test]
fn claude_files_reconcile_metadata_without_using_fresh_write_ages() {
    let home = tempfile::tempdir().unwrap();
    claude_root(home.path());
    for (id, parent, depth, finished, stopped) in [
        ("a", None, 1, false, false),
        ("b", None, 1, false, false),
        ("c", None, 1, true, false),
        ("d", None, 1, true, false),
        ("e", Some("c"), 2, true, false),
        ("f", Some("d"), 2, true, false),
        ("g", None, 1, true, true),
        ("h", None, 1, false, true),
    ] {
        claude_child(
            home.path(),
            id,
            json!({"agentType":"general-purpose","parentAgentId":parent,"spawnDepth":depth,"finished":finished,"stoppedByUser":stopped}),
        );
    }
    let probe = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
    let files = &probe.sessions["root"];
    let counts = count_workers(
        &AgentId("root".into()),
        &files.workers,
        files.lifecycle_availability.clone(),
    );
    assert_eq!(
        (counts.direct, counts.nested),
        (Some(2), Some(0)),
        "{counts:?}"
    );
    assert_eq!(
        files.workers.iter().find(|w| w.id.0 == "g").unwrap().state,
        WorkerState::Stopped
    );
    for w in files
        .workers
        .iter()
        .filter(|w| matches!(w.state, WorkerState::Completed | WorkerState::Stopped))
    {
        assert_eq!(w.state_changed_at, None);
        assert!(w.timestamp_unavailable_reason.is_some());
        assert!(w.issues.is_empty());
    }
    // Reopening/inspection and a fresh mtime do not restart finished assignments.
    write(
        home.path(),
        ".claude/projects/test/root/subagents/agent-g.meta.json",
        r#"{"agentType":"general-purpose","spawnDepth":1,"finished":true,"stoppedByUser":true,"success":true,"resumedAgentId":"g"}"#,
    );
    assert_eq!(
        collect_probe(home.path(), &request(HarnessKind::Claude))
            .unwrap()
            .sessions["root"]
            .workers
            .iter()
            .find(|w| w.id.0 == "g")
            .unwrap()
            .state,
        WorkerState::Stopped
    );
}
#[trace("FR-018-AC-11", "FR-018-AC-14", "FR-018-AC-20")]
#[test]
fn missing_metadata_parent_uuid_and_oversized_metadata_preserve_unknown() {
    let home = tempfile::tempdir().unwrap();
    claude_root(home.path());
    claude_child(
        home.path(),
        "a",
        json!({"agentType":"general-purpose","parentUuid":"root","finished":false}),
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
    assert_eq!(probe.sessions["root"].workers[0].parent_id, None);
    write(
        home.path(),
        ".claude/projects/test/root/subagents/agent-a.meta.json",
        &" ".repeat(65537),
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
    assert!(
        probe.sessions["root"].workers[0]
            .issues
            .contains(&"file_size_limit".into())
    );
}
fn codex_header(id: &str, parent: Option<&str>, path: &str) -> Value {
    json!({"type":"session_meta","timestamp":"2026-10-18T10:00:00Z","payload":{"id":id,"parent_thread_id":parent,"source": if parent.is_some() { json!({"subagent":{"thread_spawn":{"parent_thread_id":parent,"agent_path":path}}}) } else {json!("cli")}}})
}
fn codex_file(home: &Path, id: &str, header: Value, events: &str) {
    write(
        home,
        &format!(".codex/sessions/rollout-test-{id}.jsonl"),
        &format!("{header}\n{events}"),
    );
}
#[trace("FR-018-AC-3", "FR-018-AC-12", "FR-018-AC-13", "FR-018-AC-14")]
#[test]
fn codex_discovers_quiet_nested_and_internal_threads_and_missing_referenced_children() {
    let home = tempfile::tempdir().unwrap();
    codex_file(
        home.path(),
        "root",
        codex_header("root", None, "/root"),
        include_str!("fixtures/agents/codex_done.jsonl")
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .as_str(),
    );
    let running = r#"{"type":"event_msg","timestamp":"2026-10-18T10:00:01Z","payload":{"type":"task_started","turn_id":"t"}}"#;
    codex_file(
        home.path(),
        "a",
        codex_header("a", Some("root"), "/root/a"),
        running,
    );
    codex_file(
        home.path(),
        "b",
        codex_header("b", Some("a"), "/root/a/b"),
        running,
    );
    let mut internal = codex_header("g", Some("root"), "/root/g");
    internal["payload"]["thread_source"] = json!("guardian_review");
    codex_file(home.path(), "g", internal, running);
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let files = &probe.sessions["root"];
    let c = count_workers(
        &AgentId("root".into()),
        &files.workers,
        files.lifecycle_availability.clone(),
    );
    assert_eq!((c.direct, c.nested), (Some(1), Some(1)), "{c:?}");
    assert_eq!(c.workers.len(), 3);
    assert!(c.workers.iter().any(|w| w.role == WorkerRole::Internal));
    let launch = r#"{"type":"response_item","timestamp":"2026-10-18T10:00:02Z","payload":{"type":"function_call","name":"spawn_agent","call_id":"spawn","arguments":"{}"}}
{"type":"response_item","timestamp":"2026-10-18T10:00:03Z","payload":{"type":"function_call_output","call_id":"spawn","output":"{\"task_name\":\"/root/missing\"}"}}"#;
    codex_file(
        home.path(),
        "root",
        codex_header("root", None, "/root"),
        launch,
    );
    assert!(
        !collect_probe(home.path(), &request(HarnessKind::Codex))
            .unwrap()
            .sessions["root"]
            .lifecycle_availability
            .complete
    );
}
#[trace("FR-018-AC-12", "FR-018-AC-14", "FR-018-AC-19")]
#[test]
fn header_parent_disagreement_unknown_events_and_unconfirmed_followups_are_explicit() {
    let adapter = adapter_for("codex").unwrap();
    let mut state = Lifecycle::default();
    let mut header = codex_header("a", Some("root"), "/root/a");
    header["payload"]["source"]["subagent"]["thread_spawn"]["parent_thread_id"] = json!("other");
    adapter.observe_record(&header, &mut state);
    assert!(state.issues.contains(&"parent_identity_conflict".into()));
    adapter.observe_record(
        &json!({"type":"event_msg","payload":{"type":"task_unrecognized"}}),
        &mut state,
    );
    assert!(
        state
            .issues
            .contains(&"unsupported_lifecycle_record".into())
    );
    state = ended(NOW - 20);
    adapter.observe_record(&json!({"type":"response_item","payload":{"type":"function_call","name":"followup_task","call_id":"f","arguments":"{\"target\":\"/root/a\"}"}}),&mut state);
    assert_eq!(state.state, WorkerState::Completed);
    assert!(state.child_states.is_empty());
}

#[derive(Clone)]
struct Machine {
    label: &'static str,
    status: &'static str,
    harness: &'static str,
    probe: Probe,
    tail: Result<String, &'static str>,
    fail: bool,
}
impl Host for Machine {
    fn label(&self) -> &str {
        self.label
    }
    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError> {
        if self.fail {
            return Err(AgentsError::Timeout {
                command: "fixture".into(),
                secs: 60,
            });
        }
        match args {
            ["workspace","list"]=>Ok(json!({"result":{"workspaces":[{"workspace_id":"w","label":"Team"}]}}).to_string()),
            ["agent","list"]=>Ok(json!({"result":{"agents":[{"pane_id":"w:p","workspace_id":"w","agent":self.harness,"agent_status":self.status,"agent_session":{"value":"root"},"cwd":"/work"}]}}).to_string()),
            ["agent","read",_,"--lines","30"]=>self.tail.clone().map_err(|e| AgentsError::ProbeFormat(e.into())),
            _=>panic!("unexpected {args:?}")
        }
    }
    fn probe(&self, _: &ProbeRequest) -> Result<Probe, AgentsError> {
        parse_probe(
            &serde_json::to_string(&ProbeResponse {
                protocol_version: PROBE_VERSION,
                probe: self.probe.clone(),
            })
            .unwrap(),
        )
    }
}
fn machine() -> Machine {
    Machine {
        label: "local",
        status: "done",
        harness: "codex",
        probe: Probe {
            now: NOW,
            started_at: NOW,
            finished_at: NOW,
            sessions: BTreeMap::from([(
                "root".into(),
                SessionFiles {
                    parent_lifecycle: ended(NOW - IDLE),
                    transcript_mtime: Some(NOW - 10),
                    lifecycle_availability: Availability::available(),
                    transcript_availability: Availability::available(),
                    ..SessionFiles::default()
                },
            )]),
            process_availability: Availability::available(),
            ..Probe::default()
        },
        tail: Ok("❯".into()),
        fail: false,
    }
}
fn report(machines: Vec<Machine>) -> AgentsReport {
    build_report(
        &machines
            .into_iter()
            .map(|h| Arc::new(h) as Arc<dyn Host>)
            .collect::<Vec<_>>(),
        &["team".into(), "missing".into()],
        IDLE,
        &mut |_| {
            Ok(Some(ReadyTicket {
                identifier: "T-1".into(),
                title: "next".into(),
            }))
        },
    )
}
#[trace(
    "FR-018-AC-1",
    "FR-018-AC-5",
    "FR-018-AC-6",
    "FR-018-AC-7",
    "FR-018-AC-21",
    "FR-018-AC-22",
    "FR-018-AC-23"
)]
#[test]
fn healthy_rows_survive_machine_failure_and_report_host_clock_counts_schema_and_queue() {
    let mut remote = machine();
    remote.label = "remote";
    remote.probe.now += 3600;
    remote.probe.started_at += 3600;
    remote.probe.finished_at += 3600;
    remote
        .probe
        .sessions
        .get_mut("root")
        .unwrap()
        .transcript_mtime = Some(NOW + 3590);
    let mut down = machine();
    down.label = "down";
    down.fail = true;
    let r = report(vec![machine(), remote, down]);
    assert_eq!(r.agents.len(), 2);
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.missing_teams, ["missing"]);
    for row in &r.agents {
        assert_eq!(row.last_turn_age_secs, Some(10));
        assert_eq!(row.verdict, Verdict::Idle);
        assert_eq!(row.live_subagents, row.active_direct_workers);
        assert_eq!(row.next_ready.as_ref().unwrap().identifier, "T-1");
    }
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["schema_version"], 2);
    assert_eq!(json["agents"][0]["active_nested_workers"], 0);
    let text = render_text(&r);
    assert!(text.contains("direct 0 / nested 0"));
    assert!(text.contains("next ready: T-1"));
    assert!(text.contains("Error:"));
    let mut skew = machine();
    skew.probe.finished_at = NOW - 1;
    let row = report(vec![skew]).agents.remove(0);
    assert_eq!(row.verdict, Verdict::Unknown);
    assert_eq!(row.last_turn_age_secs, None);
}
#[trace("FR-018-AC-4", "FR-018-AC-7", "FR-018-AC-19", "FR-018-AC-23")]
#[test]
fn unavailable_sources_are_partial_nullable_and_do_not_fabricate_zero() {
    let mut m = machine();
    m.harness = "unsupported";
    m.probe.sessions.clear();
    m.status = "working";
    let row = report(vec![m.clone()]).agents.remove(0);
    assert!(row.partial);
    assert_eq!(row.live_subagents, None);
    assert_eq!(row.verdict, Verdict::Working);
    assert!(render_text(&report(vec![m.clone()])).contains("direct unknown"));
    assert!(serde_json::to_value(&row).unwrap()["live_subagents"].is_null());
    m.tail = Err("pane_permission_denied");
    assert_eq!(report(vec![m]).agents[0].verdict, Verdict::Unknown);
}
#[trace("FR-018-AC-6")]
#[test]
fn unreadable_queue_is_reported_once_and_does_not_change_idle() {
    let hosts: Vec<Arc<dyn Host>> = vec![Arc::new(machine()), Arc::new(machine())];
    let mut calls = 0;
    let r = build_report(&hosts, &["Team".into()], IDLE, &mut |_| {
        calls += 1;
        Err("denied".into())
    });
    assert_eq!(calls, 1);
    assert_eq!(r.ready_queue_error.as_deref(), Some("denied"));
    assert!(r.agents.iter().all(|r| r.verdict == Verdict::Idle));
}
#[trace("FR-018-AC-5", "FR-018-AC-20", "FR-018-AC-21", "FR-018-AC-23")]
#[test]
fn hidden_probe_round_trip_refuses_version_truncation_unknown_fields_and_input_limits() {
    let home = tempfile::tempdir().unwrap();
    claude_root(home.path());
    let envelope = ProbeEnvelope {
        protocol_version: PROBE_VERSION,
        request: request(HarnessKind::Claude),
    };
    let text = serve_probe(
        serde_json::to_vec(&envelope).unwrap().as_slice(),
        home.path(),
    )
    .unwrap();
    let p = parse_probe(&text).unwrap();
    assert_eq!(p.sessions.len(), 1);
    if cfg!(target_os = "macos") {
        assert!(
            p.process_availability
                .reasons
                .contains(&"process_collection_unsupported_platform".into())
        );
    }
    assert!(parse_probe(&text[..text.len() - 1]).is_err());
    let mut bad: Value = serde_json::from_str(&text).unwrap();
    bad["protocol_version"] = json!(1);
    assert!(parse_probe(&bad.to_string()).is_err());
    assert!(
        serve_probe(
            br#"{"protocol_version":2,"request":{"sessions":[],"cwds":[]},"typo":true}"#.as_slice(),
            home.path()
        )
        .is_err()
    );
    let invalid = ProbeRequest {
        sessions: vec![SessionQuery {
            kind: HarnessKind::Claude,
            session_id: "../escape".into(),
        }],
        cwds: vec![],
    };
    assert!(invalid.validate().is_err());
    assert!(serve_probe(vec![b' '; MAX_PROTOCOL_BYTES + 1].as_slice(), home.path()).is_err());
    for value in ["0s", "-1m", "1d", "20", ""] {
        assert!(parse_duration(value).is_err(), "{value}");
    }
    assert_eq!(parse_duration("90s"), Ok(90));
    assert_eq!(parse_duration("2h"), Ok(7200));
}
struct Third;
impl Harness for Third {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Other
    }
    fn evidence_source(&self) -> EvidenceSource {
        EvidenceSource::Adapter
    }
    fn observe_record(&self, _: &Value, s: &mut Lifecycle) {
        s.state = WorkerState::Waiting;
        s.last_activity_at = Some(NOW);
    }
}
#[trace("FR-018-AC-16", "FR-018-AC-17")]
#[test]
fn third_adapter_uses_unchanged_classifier_and_counts() {
    let mut s = Lifecycle::default();
    Third.observe_record(&json!({}), &mut s);
    let c = counts(&[worker("child", "root", WorkerState::Waiting)]);
    assert_eq!(verdict(&s, &c, HerdrState::Working, NOW), Verdict::Waiting);
}

#[trace("FR-018-AC-10", "FR-018-AC-12")]
#[test]
fn real_followup_interaction_receipt_needs_new_child_generation_to_restart() {
    // Sanitized projection of the 2026-11-19 rollout: result text is absent.
    let adapter = adapter_for("codex").unwrap();
    let mut parent = life("codex", include_str!("fixtures/agents/codex_running.jsonl"));
    for record in [
        json!({"type":"response_item","timestamp":"2026-11-19T21:34:01.295Z","payload":{"type":"function_call","name":"followup_task","call_id":"call-followup","arguments":"{\"target\":\"research\"}"}}),
        json!({"type":"event_msg","timestamp":"2026-11-19T21:34:01.466Z","payload":{"type":"item_completed","item":{"id":"call-followup","type":"SubAgentActivity","kind":"interacted","agent_path":"/root/research","agent_thread_id":"child"}}}),
        json!({"type":"response_item","timestamp":"2026-11-19T21:34:01.470Z","payload":{"type":"function_call_output","call_id":"call-followup","output":""}}),
    ] {
        adapter.observe_record(&record, &mut parent);
    }
    assert!(parent.expected_children.contains("child"));
    assert!(parent.child_states.is_empty());
    assert!(parent.issues.is_empty());
    let mut child = life("codex", include_str!("fixtures/agents/codex_done.jsonl"));
    adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-11-19T21:34:01.511Z","payload":{"type":"task_started","turn_id":"new-turn","turn_attribution":{"initiating_agent_path":"/root","parent_turn_id":"parent-turn"}}}),&mut child);
    assert_eq!(child.state, WorkerState::Running);
    assert_eq!(child.generation.as_deref(), Some("new-turn"));
    assert!(child.execution_ended_at.is_none());
    adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-11-19T21:34:02Z","payload":{"type":"turn_aborted","turn_id":"new-turn"}}),&mut child);
    assert_eq!(child.state, WorkerState::Stopped);
    adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-11-19T21:34:03Z","payload":{"type":"task_started","turn_id":"new-turn"}}),&mut child);
    assert_eq!(child.state, WorkerState::Stopped);
    assert!(child.issues.contains(&"reused_terminal_generation".into()));
}
#[trace("FR-018-AC-20", "FR-018-AC-21")]
#[test]
fn oversized_records_and_permission_sources_never_become_complete_empty_graphs() {
    let home = tempfile::tempdir().unwrap();
    claude_root(home.path());
    write(
        home.path(),
        ".claude/projects/test/root.jsonl",
        &format!(
            "{{\"type\":\"assistant\",\"padding\":\"{}\"}}",
            "x".repeat(1024 * 1024)
        ),
    );
    let p = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
    assert!(!p.sessions["root"].transcript_availability.complete);
    assert!(
        p.sessions["root"]
            .transcript_availability
            .reasons
            .contains(&"record_size_limit".into())
    );
    let missing = tempfile::tempdir().unwrap();
    let p = collect_probe(missing.path(), &request(HarnessKind::Codex)).unwrap();
    assert!(!p.sessions["root"].lifecycle_availability.complete);
    assert!(
        p.sessions["root"]
            .lifecycle_availability
            .reasons
            .contains(&"source_directory_missing".into())
    );
}
#[trace("FR-018-AC-19", "FR-018-AC-22")]
#[test]
fn writes_do_not_prove_execution_and_active_worker_waits_do_not_become_stuck() {
    let mut m = machine();
    m.probe.sessions.get_mut("root").unwrap().parent_lifecycle = Lifecycle::default();
    // A done pane with no lifecycle record is idle, not unknown.
    assert_eq!(report(vec![m.clone()]).agents[0].verdict, Verdict::Idle);
    m.probe
        .sessions
        .get_mut("root")
        .unwrap()
        .workers
        .push(worker("child", "root", WorkerState::Waiting));
    m.tail = Ok("waiting for background agents".into());
    m.status = "working";
    assert_eq!(report(vec![m]).agents[0].verdict, Verdict::Waiting);
}

#[trace("FR-018-AC-10", "FR-018-AC-12", "FR-018-AC-15")]
#[test]
fn owned_parent_snapshots_confirm_completion_but_do_not_resurrect_terminal_children() {
    let home = tempfile::tempdir().unwrap();
    let snapshot = |status: Value| {
        format!(
            "{}\n{}",
            json!({"type":"response_item","timestamp":"2026-10-18T10:00:02Z","payload":{"type":"function_call","name":"list_agents","call_id":"list","arguments":"{}"}}),
            json!({"type":"response_item","timestamp":"2026-10-18T10:00:03Z","payload":{"type":"function_call_output","call_id":"list","output":json!({"agents":[{"agent_name":"/root/a","agent_status":status}]}).to_string()}})
        )
    };
    let start = r#"{"type":"event_msg","timestamp":"2026-10-18T10:00:01Z","payload":{"type":"task_started","turn_id":"t1"}}"#;
    codex_file(
        home.path(),
        "root",
        codex_header("root", None, "/root"),
        &snapshot(json!({"completed":"discard this result"})),
    );
    codex_file(
        home.path(),
        "a",
        codex_header("a", Some("root"), "/root/a"),
        start,
    );
    let collected = || {
        let p = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
        let f = &p.sessions["root"];
        count_workers(
            &AgentId("root".into()),
            &f.workers,
            f.lifecycle_availability.clone(),
        )
    };
    let c = collected();
    assert_eq!(c.direct, Some(0));
    assert_eq!(c.workers[0].state, WorkerState::Completed);
    assert!(
        c.workers[0]
            .provenance
            .iter()
            .any(|p| p.starts_with("correlated_parent_snapshot"))
    );
    let done = format!(
        "{start}\n{}",
        json!({"type":"event_msg","timestamp":"2026-10-18T10:00:02Z","payload":{"type":"task_complete","turn_id":"t1"}})
    );
    codex_file(
        home.path(),
        "a",
        codex_header("a", Some("root"), "/root/a"),
        &done,
    );
    codex_file(
        home.path(),
        "root",
        codex_header("root", None, "/root"),
        &snapshot(json!("running")),
    );
    assert_eq!(collected().direct, Some(0));
    let restarted = format!(
        "{done}\n{}",
        json!({"type":"event_msg","timestamp":"2026-10-18T10:00:04Z","payload":{"type":"task_started","turn_id":"t2"}})
    );
    codex_file(
        home.path(),
        "a",
        codex_header("a", Some("root"), "/root/a"),
        &restarted,
    );
    assert_eq!(collected().direct, Some(1));
}
#[trace("FR-018-AC-11", "FR-018-AC-14")]
#[test]
fn conflicting_declared_depth_role_duplicates_and_unorderable_claude_metadata_are_uncertain() {
    let mut a = worker("a", "root", WorkerState::Running);
    a.declared_depth = Some(2);
    assert_eq!(
        (counts(&[a.clone()]).direct, counts(&[a]).nested),
        (None, None)
    );
    let mut internal = worker("a", "root", WorkerState::Running);
    internal.role = WorkerRole::Internal;
    assert_eq!(
        counts(&[internal, worker("a", "root", WorkerState::Running)]).direct,
        None
    );
    let home = tempfile::tempdir().unwrap();
    claude_root(home.path());
    claude_child(
        home.path(),
        "a",
        json!({"agentType":"general-purpose","spawnDepth":1,"finished":true}),
    );
    let path = home
        .path()
        .join(".claude/projects/test/root/subagents/agent-a.jsonl");
    let existing = fs::read_to_string(&path).unwrap();
    fs::write(&path,format!("{existing}\n{}",json!({"type":"user","agentId":"a","sessionId":"root","timestamp":"2026-10-18T10:00:15Z","message":{"content":"new assignment"}}))).unwrap();
    let p = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
    assert!(
        p.sessions["root"].workers[0]
            .issues
            .contains(&"metadata_assignment_order_unresolved".into())
    );
}

#[trace(
    "FR-018-AC-4",
    "FR-018-AC-7",
    "FR-018-AC-8",
    "FR-018-AC-19",
    "FR-018-AC-20"
)]
#[test]
fn verified_gate_survives_missing_transcripts_and_report_counts_alias_active_assignments() {
    let mut m = machine();
    m.harness = "unsupported";
    m.probe.sessions.clear();
    m.probe.processes.insert(
        "w:p".into(),
        vec![PaneProcess {
            comm: "cargo".into(),
            args: String::new(),
        }],
    );
    let row = report(vec![m]).agents.remove(0);
    assert_eq!(row.verdict, Verdict::Working);
    assert!(row.partial);
    assert_eq!(row.live_subagents, None);
    let mut m = machine();
    m.probe.sessions.get_mut("root").unwrap().workers = vec![
        worker("a", "root", WorkerState::Running),
        worker("b", "root", WorkerState::Waiting),
    ];
    let row = report(vec![m]).agents.remove(0);
    assert_eq!(row.verdict, Verdict::Waiting);
    assert_eq!(
        (
            row.active_direct_workers,
            row.active_nested_workers,
            row.live_subagents
        ),
        (Some(2), Some(0), Some(2))
    );
    let too_many: Vec<_> = (0..1025)
        .map(|i| worker(&format!("child-{i}"), "root", WorkerState::Running))
        .collect();
    let c = counts(&too_many);
    assert_eq!(c.direct, None);
    assert_eq!(c.workers.len(), 1024);
    assert!(
        c.availability
            .reasons
            .contains(&"worker_state_limit".into())
    );
}

#[derive(Clone, Copy)]
enum FixtureChildren {
    Empty,
    Running,
    Completed,
    MissingMetadata,
}
fn collected_scenario(harness: &str, mode: &str, children: FixtureChildren) -> Machine {
    let home = tempfile::tempdir().unwrap();
    let (kind, records) = match (harness, mode) {
        ("claude", "running") => (
            HarnessKind::Claude,
            include_str!("fixtures/agents/claude_running.jsonl"),
        ),
        ("claude", "waiting") => (
            HarnessKind::Claude,
            include_str!("fixtures/agents/claude_waiting.jsonl"),
        ),
        ("claude", "asking") => (
            HarnessKind::Claude,
            include_str!("fixtures/agents/claude_asking.jsonl"),
        ),
        ("claude", "done") => (
            HarnessKind::Claude,
            include_str!("fixtures/agents/claude_done.jsonl"),
        ),
        ("codex", "running") => (
            HarnessKind::Codex,
            include_str!("fixtures/agents/codex_running.jsonl"),
        ),
        ("codex", "waiting") => (
            HarnessKind::Codex,
            include_str!("fixtures/agents/codex_waiting.jsonl"),
        ),
        ("codex", "returned_wait") => (
            HarnessKind::Codex,
            include_str!("fixtures/agents/codex_wait_returned.jsonl"),
        ),
        ("codex", "asking") => (
            HarnessKind::Codex,
            include_str!("fixtures/agents/codex_asking.jsonl"),
        ),
        ("codex", "done") => (
            HarnessKind::Codex,
            include_str!("fixtures/agents/codex_done.jsonl"),
        ),
        _ => panic!("unknown fixture {harness}/{mode}"),
    };
    if kind == HarnessKind::Claude {
        write(home.path(), ".claude/projects/test/root.jsonl", records);
        if !matches!(children, FixtureChildren::Empty) {
            claude_child(
                home.path(),
                "child",
                json!({"agentType":"general-purpose","spawnDepth":1,"finished":matches!(children,FixtureChildren::Completed)}),
            );
            if matches!(children, FixtureChildren::MissingMetadata) {
                fs::remove_file(
                    home.path()
                        .join(".claude/projects/test/root/subagents/agent-child.meta.json"),
                )
                .unwrap();
            }
        }
    } else {
        codex_file(
            home.path(),
            "root",
            codex_header("root", None, "/root"),
            &records.lines().skip(1).collect::<Vec<_>>().join("\n"),
        );
        if !matches!(children, FixtureChildren::Empty) {
            let start = json!({"type":"event_msg","timestamp":"2026-10-18T10:00:00Z","payload":{"type":"task_started","turn_id":"child-turn"}});
            let terminal = if matches!(children, FixtureChildren::Completed) {
                format!(
                    "\n{}",
                    json!({"type":"event_msg","timestamp":"2026-10-18T10:00:01Z","payload":{"type":"task_complete","turn_id":"child-turn"}})
                )
            } else {
                String::new()
            };
            codex_file(
                home.path(),
                "child",
                codex_header("child", Some("root"), "/root/child"),
                &format!("{start}{terminal}"),
            );
            if matches!(children, FixtureChildren::MissingMetadata) {
                fs::remove_file(home.path().join(".codex/sessions/rollout-test-child.jsonl"))
                    .unwrap();
                let mut parent = records.lines().skip(1).collect::<Vec<_>>().join("\n");
                parent.push_str(&format!("\n{}\n{}",json!({"type":"response_item","timestamp":"2026-10-18T10:00:01Z","payload":{"type":"function_call","name":"spawn_agent","call_id":"spawn","arguments":"{}"}}),json!({"type":"response_item","timestamp":"2026-10-18T10:00:01Z","payload":{"type":"function_call_output","call_id":"spawn","output":"{\"task_name\":\"/root/child\"}"}})));
                codex_file(
                    home.path(),
                    "root",
                    codex_header("root", None, "/root"),
                    &parent,
                );
            }
        }
    }
    let mut host = machine();
    host.harness = if kind == HarnessKind::Claude {
        "claude"
    } else {
        "codex"
    };
    host.probe = collect_probe(home.path(), &request(kind)).unwrap();
    // Replay the real filesystem collection with an injected complete process
    // sample; the adapter-derived assignment/state facts remain unchanged.
    host.probe.processes.clear();
    host.probe.process_availability = Availability::available();
    host
}
fn host_age(host: &mut Machine, age: u64) {
    let at = host.probe.sessions["root"]
        .parent_lifecycle
        .last_activity_at
        .unwrap();
    host.probe.now = at + age;
    host.probe.started_at = at + age;
    host.probe.finished_at = at + age;
    let files = host.probe.sessions.get_mut("root").unwrap();
    // File age is deliberately fresh even for ended and quiet fixtures.
    files.transcript_mtime = Some(at + age);
    for worker in &mut files.workers {
        worker.observed_at = at + age;
    }
}

#[trace(
    "FR-018-AC-2",
    "FR-018-AC-3",
    "FR-018-AC-17",
    "FR-018-AC-18",
    "FR-018-AC-19"
)]
#[test]
fn both_harnesses_decode_all_eight_verdicts_thresholds_and_wait_precedence_from_records() {
    use FixtureChildren::{Completed, Empty, MissingMetadata, Running};
    let cases = [
        ("running", Empty, "working", 1, Verdict::Working),
        ("waiting", Running, "blocked", 3600, Verdict::Prompt),
        ("asking", Empty, "done", 1, Verdict::Asking),
        ("done", Empty, "working", 1, Verdict::Unknown),
        ("done", Running, "done", 3600, Verdict::Waiting),
        ("waiting", Running, "working", 3600, Verdict::Waiting),
        ("waiting", Completed, "working", 899, Verdict::Unknown),
        ("waiting", Completed, "working", 900, Verdict::WaitingStuck),
        ("waiting", MissingMetadata, "working", 900, Verdict::Unknown),
        ("done", Empty, "done", 1199, Verdict::Recent),
        ("done", Empty, "done", 1200, Verdict::Idle),
        ("running", Empty, "done", 3600, Verdict::Idle),
    ];
    for harness in ["claude", "codex"] {
        let mut seen = Vec::new();
        for (mode, children, status, age, expected) in cases {
            let mut host = collected_scenario(harness, mode, children);
            if mode == "waiting" {
                assert_eq!(
                    host.probe.sessions["root"].parent_lifecycle.state,
                    WorkerState::Waiting,
                    "{harness}: decoder must establish pending wait"
                );
            }
            if mode == "running" {
                assert_eq!(
                    host.probe.sessions["root"].parent_lifecycle.state,
                    WorkerState::Running,
                    "{harness}: decoder must establish assignment"
                );
            }
            host_age(&mut host, age);
            host.status = status;
            let row = report(vec![host]).agents.remove(0);
            assert_eq!(
                row.verdict, expected,
                "{harness}/{mode}/{status}/{age}: {row:?}"
            );
            if !seen.contains(&row.verdict) {
                seen.push(row.verdict);
            }
        }
        assert_eq!(
            seen.len(),
            8,
            "{harness} fixtures must exercise every verdict"
        );
    }
}

#[trace("FR-018-AC-10", "FR-018-AC-11", "FR-018-AC-14")]
#[test]
fn claude_injected_user_context_preserves_terminal_metadata_but_real_assignments_remain_uncertain()
{
    for stopped in [false, true] {
        let home = tempfile::tempdir().unwrap();
        claude_root(home.path());
        claude_child(
            home.path(),
            "child",
            json!({"agentType":"general-purpose","spawnDepth":1,"finished":true,"stoppedByUser":stopped}),
        );
        let path = home
            .path()
            .join(".claude/projects/test/root/subagents/agent-child.jsonl");
        let original = fs::read_to_string(&path).unwrap();
        let injected = json!({"type":"user","agentId":"child","sessionId":"root","isMeta":true,"timestamp":"2026-10-18T10:00:11Z","message":{"content":"<system-reminder>Base directory for this skill: /sanitized/skill</system-reminder>"}});
        fs::write(&path, format!("{original}\n{injected}\n")).unwrap();
        let p = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
        let f = &p.sessions["root"];
        let c = count_workers(
            &AgentId("root".into()),
            &f.workers,
            f.lifecycle_availability.clone(),
        );
        assert_eq!(
            (c.direct, c.nested),
            (Some(0), Some(0)),
            "meta context is not an assignment: {c:?}"
        );
        assert_eq!(
            c.workers[0].state,
            if stopped {
                WorkerState::Stopped
            } else {
                WorkerState::Completed
            }
        );
        assert!(c.workers[0].issues.is_empty());
        assert_eq!(
            life("claude", &fs::read_to_string(&path).unwrap()).assignment_starts,
            1
        );
        let assignment = json!({"type":"user","agentId":"child","sessionId":"root","isMeta":false,"timestamp":"2026-10-18T10:00:12Z","message":{"content":"A real new assignment."}});
        fs::write(&path, format!("{original}\n{injected}\n{assignment}\n")).unwrap();
        let p = collect_probe(home.path(), &request(HarnessKind::Claude)).unwrap();
        let f = &p.sessions["root"];
        let c = count_workers(
            &AgentId("root".into()),
            &f.workers,
            f.lifecycle_availability.clone(),
        );
        assert_eq!(c.direct, None);
        assert!(
            c.workers[0]
                .issues
                .contains(&"metadata_assignment_order_unresolved".into())
        );
    }
}

#[trace("FR-018-AC-3", "FR-018-AC-12", "FR-018-AC-17", "FR-018-AC-19")]
#[test]
fn codex_correlated_wait_return_resumes_execution_while_unresolved_waits_remain_pending() {
    let mut host = collected_scenario("codex", "returned_wait", FixtureChildren::Running);
    host_age(&mut host, 1);
    host.status = "working";
    let state = &host.probe.sessions["root"].parent_lifecycle;
    assert_eq!(state.state, WorkerState::Running);
    assert!(state.issues.is_empty(), "{state:?}");
    assert_eq!(report(vec![host]).agents[0].verdict, Verdict::Working);
    let adapter = adapter_for("codex").unwrap();
    let mut state = life("codex", include_str!("fixtures/agents/codex_waiting.jsonl"));
    adapter.observe_record(&json!({"type":"response_item","timestamp":"2026-10-18T10:00:03Z","payload":{"type":"function_call","name":"functions.exec","call_id":"parallel-exec","arguments":"{}"}}),&mut state);
    assert_eq!(
        state.state,
        WorkerState::Waiting,
        "ordinary calls do not discard an unresolved correlated wait"
    );
    assert!(state.calls.contains_key("wait1"));
    adapter.observe_record(&json!({"type":"response_item","timestamp":"2026-10-18T10:00:04Z","payload":{"type":"function_call_output","call_id":"unrelated","output":"{}"}}),&mut state);
    assert_eq!(state.state, WorkerState::Waiting);
    adapter.observe_record(&json!({"type":"response_item","timestamp":"2026-10-18T10:00:05Z","payload":{"type":"function_call_output","call_id":"wait1","output":"{\"message\":\"No updates\",\"timed_out\":true}"}}),&mut state);
    assert_eq!(state.state, WorkerState::Running);
    assert!(!state.calls.contains_key("wait1"));
}

#[trace("FR-018-AC-10", "FR-018-AC-12", "FR-018-AC-19")]
#[test]
fn codex_late_wait_events_cannot_reopen_completed_or_stopped_generations() {
    let adapter = adapter_for("codex").unwrap();
    for terminal in ["task_complete", "turn_aborted"] {
        for event in ["collab_waiting_begin", "collab_waiting_end"] {
            let mut state = life("codex", include_str!("fixtures/agents/codex_running.jsonl"));
            adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-10-18T10:00:03Z","payload":{"type":terminal,"turn_id":"g1"}}),&mut state);
            let expected = state.state;
            let ended = state.execution_ended_at;
            adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-10-18T10:00:04Z","payload":{"type":event,"turn_id":"g1"}}),&mut state);
            assert_eq!(state.state, expected, "{terminal}/{event}");
            assert_eq!(state.execution_ended_at, ended);
            assert!(
                state
                    .issues
                    .contains(&"execution_after_terminal_without_assignment".into())
            );
        }
    }
    let mut state = life("codex", include_str!("fixtures/agents/codex_waiting.jsonl"));
    adapter.observe_record(&json!({"type":"event_msg","timestamp":"2026-10-18T10:00:03Z","payload":{"type":"task_complete","turn_id":"g1"}}),&mut state);
    adapter.observe_record(&json!({"type":"response_item","timestamp":"2026-10-18T10:00:04Z","payload":{"type":"function_call_output","call_id":"wait1","output":"{}"}}),&mut state);
    assert_eq!(state.state, WorkerState::Completed);
    assert!(state.execution_ended_at.is_some());
    assert!(
        state
            .issues
            .contains(&"execution_after_terminal_without_assignment".into())
    );
}

#[trace("FR-018-AC-27")]
#[test]
fn herdr_done_without_prompt_question_or_work_is_idle_or_recent() {
    let none = counts(&[]);
    let blank = Lifecycle::default();
    assert_eq!(verdict(&blank, &none, HerdrState::Done, NOW), Verdict::Idle);
    let recent = Lifecycle {
        last_activity_at: Some(NOW - 10),
        ..Lifecycle::default()
    };
    assert_eq!(
        verdict(&recent, &none, HerdrState::Done, NOW),
        Verdict::Recent
    );
    let old = Lifecycle {
        last_activity_at: Some(NOW - IDLE - 1),
        ..Lifecycle::default()
    };
    assert_eq!(verdict(&old, &none, HerdrState::Done, NOW), Verdict::Idle);
    let asking = Lifecycle {
        assistant: Some("Should I merge this?".into()),
        ..Lifecycle::default()
    };
    assert_eq!(
        verdict(&asking, &none, HerdrState::Done, NOW),
        Verdict::Asking
    );
    let prompt = decide(&Evidence {
        herdr: HerdrState::Done,
        tail: Some("Allow once\nesc to cancel"),
        lifecycle: &blank,
        workers: &none,
        active_processes: 0,
        processes_complete: true,
        transcript_complete: true,
        now: NOW,
        idle_after: IDLE,
    });
    assert_eq!(prompt, Verdict::Prompt);
    let pane: HerdrAgent = serde_json::from_value(json!({
        "pane_id":"w:p","agent":"codex","agent_status":"done"
    }))
    .unwrap();
    assert_eq!(pane.state(), HerdrState::Done);
}

fn codex_rollout(filler_bytes: usize, events: &str) -> String {
    let filler = json!({
        "type":"response_item",
        "timestamp":"2026-10-18T10:00:00Z",
        "payload":{"type":"message","role":"user","content":"x".repeat(900)}
    })
    .to_string();
    let lines = filler_bytes / (filler.len() + 1);
    let mut text = format!("{}\n", codex_header("root", None, "/root"));
    for _ in 0..lines {
        text.push_str(&filler);
        text.push('\n');
    }
    text.push_str(events);
    text
}

#[trace("FR-018-AC-28")]
#[test]
fn codex_rollout_larger_than_the_first_window_yields_its_tail_state() {
    let home = tempfile::tempdir().unwrap();
    let events = include_str!("fixtures/agents/codex_done.jsonl")
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    write(
        home.path(),
        ".codex/sessions/rollout-test-root.jsonl",
        &codex_rollout(5 * 1024 * 1024, &events),
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let files = &probe.sessions["root"];
    assert!(files.lifecycle_availability.complete, "{files:?}");
    assert_eq!(files.parent_lifecycle.state, WorkerState::Completed);
    assert!(files.parent_lifecycle.execution_ended_at.is_some());
}

#[trace("FR-018-AC-28")]
#[test]
fn codex_rollout_without_a_lifecycle_record_within_the_limit_is_unavailable_with_reason() {
    let home = tempfile::tempdir().unwrap();
    write(
        home.path(),
        ".codex/sessions/rollout-test-root.jsonl",
        &codex_rollout(66 * 1024 * 1024, ""),
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let files = &probe.sessions["root"];
    assert!(!files.lifecycle_availability.complete);
    assert!(
        files
            .lifecycle_availability
            .reasons
            .contains(&"lifecycle_not_found_within_limit".into()),
        "{:?}",
        files.lifecycle_availability
    );
}

struct SlowHost(Machine);
impl Host for SlowHost {
    fn label(&self) -> &str {
        self.0.label
    }
    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError> {
        std::thread::sleep(Duration::from_secs(3));
        self.0.herdr(args)
    }
    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError> {
        self.0.probe(request)
    }
}

#[trace("FR-018-AC-29")]
#[test]
fn a_slow_machine_is_listed_missing_while_healthy_machines_still_report() {
    let mut slow = machine();
    slow.label = "luna";
    let hosts: Vec<Arc<dyn Host>> = vec![Arc::new(machine()), Arc::new(SlowHost(slow))];
    let started = std::time::Instant::now();
    let r = build_report_with_timeout(
        &hosts,
        &["team".into()],
        IDLE,
        Duration::from_millis(300),
        &mut |_| Ok(None),
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(r.agents.len(), 1);
    assert_eq!(r.agents[0].machine, "local");
    assert_eq!(r.agents[0].verdict, Verdict::Idle);
    assert_eq!(r.missing_hosts.len(), 1);
    let missing = &r.missing_hosts[0];
    assert_eq!(missing.machine, "luna");
    assert!(missing.reason.contains("timed out"));
    assert!(missing.elapsed_ms >= 250 && missing.elapsed_ms < 2000);
    assert!(render_text(&r).contains("Missing host: luna"));
    assert!(r.errors.iter().any(|e| e.starts_with("luna:")));
    assert_eq!(r.coverage(), Coverage::Partial);
    let mut down = machine();
    down.label = "down";
    down.fail = true;
    let r = report(vec![machine(), down]);
    assert_eq!(r.missing_hosts[0].machine, "down");
}

#[trace("FR-018-AC-27")]
#[test]
fn herdr_done_is_not_idle_with_an_active_worker_or_attributed_process() {
    let blank = Lifecycle::default();
    let active = counts(&[worker("child", "root", WorkerState::Running)]);
    assert_eq!(
        verdict(&blank, &active, HerdrState::Done, NOW),
        Verdict::Waiting
    );
    let running_build = decide(&Evidence {
        herdr: HerdrState::Done,
        tail: Some("❯"),
        lifecycle: &blank,
        workers: &counts(&[]),
        active_processes: 1,
        processes_complete: true,
        transcript_complete: true,
        now: NOW,
        idle_after: IDLE,
    });
    assert_eq!(running_build, Verdict::Working);
}

/// Larger than the 64 MiB limit under which a rollout is read whole.
const LARGE: usize = 65 * 1024 * 1024;
fn json_line(value: &Value) -> String {
    format!("{value}\n")
}
fn filler_line(total_len: usize) -> String {
    let line = |pad: usize| {
        json_line(
            &json!({"type":"response_item","timestamp":"2026-10-18T10:00:00Z","payload":{"type":"message","role":"user","content":"x".repeat(pad)}}),
        )
    };
    let line = line(total_len - line(0).len());
    assert_eq!(line.len(), total_len);
    line
}
fn fillers(total: usize) -> String {
    let count = total / 1000;
    let mut text = String::new();
    for _ in 1..count {
        text.push_str(&filler_line(1000));
    }
    text.push_str(&filler_line(total - (count - 1) * 1000));
    text
}
fn event(kind: &str, at: &str, turn: &str) -> String {
    json_line(&json!({"type":"event_msg","timestamp":at,"payload":{"type":kind,"turn_id":turn}}))
}

#[trace("FR-018-AC-28")]
#[test]
fn a_child_rollout_over_the_first_window_keeps_its_identity_and_header() {
    let home = tempfile::tempdir().unwrap();
    codex_file(
        home.path(),
        "root",
        codex_header("root", None, "/root"),
        include_str!("fixtures/agents/codex_done.jsonl")
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .as_str(),
    );
    let mut child = format!("{}\n", codex_header("a", Some("root"), "/root/a"));
    child.push_str(&fillers(LARGE));
    child.push_str(&event("task_started", "2026-10-18T10:00:01Z", "t"));
    child.push_str(&fillers(1536 * 1024));
    child.push_str(&event("task_complete", "2026-10-18T10:00:09Z", "t"));
    write(home.path(), ".codex/sessions/rollout-test-a.jsonl", &child);
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let files = &probe.sessions["root"];
    let child = files
        .workers
        .iter()
        .find(|w| w.id.0 == "a")
        .expect("child worker");
    assert_eq!(child.parent_id, Some(AgentId("root".into())));
    assert_eq!(child.agent_path.as_deref(), Some("/root/a"));
    assert_eq!(child.state, WorkerState::Completed);
    assert!(child.issues.is_empty(), "{:?}", child.issues);
}

#[trace("FR-018-AC-28")]
#[test]
fn the_tail_replay_starts_at_the_latest_turn_start_even_when_the_window_would_start_mid_turn() {
    let home = tempfile::tempdir().unwrap();
    let mut text = format!("{}\n", codex_header("root", None, "/root"));
    text.push_str(&fillers(LARGE));
    text.push_str(&event("task_started", "2026-10-18T10:00:01Z", "t"));
    text.push_str(&fillers(1536 * 1024));
    text.push_str(&event("collab_waiting_begin", "2026-10-18T10:00:02Z", "t"));
    text.push_str(&event("collab_waiting_end", "2026-10-18T10:00:03Z", "t"));
    text.push_str(&event("task_complete", "2026-10-18T10:00:09Z", "t"));
    write(
        home.path(),
        ".codex/sessions/rollout-test-root.jsonl",
        &text,
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let life = &probe.sessions["root"].parent_lifecycle;
    assert!(life.issues.is_empty(), "{:?}", life.issues);
    assert_eq!(life.state, WorkerState::Completed);
    assert!(probe.sessions["root"].lifecycle_availability.complete);
}

#[trace("FR-018-AC-28")]
#[test]
fn a_turn_start_exactly_on_the_scan_floor_boundary_is_not_dropped() {
    // The deepest backward scan: the transcript limit less the header reserve.
    const LIMIT: usize = 64 * 1024 * 1024 - 1024 * 1024 - 2;
    let home = tempfile::tempdir().unwrap();
    let mut text = format!("{}\n", codex_header("root", None, "/root"));
    text.push_str(&fillers(1536 * 1024));
    let started = event("task_started", "2026-10-18T10:00:01Z", "t");
    let complete = event("task_complete", "2026-10-18T10:00:09Z", "t");
    // The scan covers exactly the last 64 MiB, which begins at the start record.
    let middle = LIMIT - started.len() - complete.len();
    let chunk = 1000 * 1000;
    text.push_str(&started);
    let full = middle / chunk;
    for _ in 0..full {
        text.push_str(&filler_line(chunk));
    }
    text.push_str(&filler_line(middle - full * chunk));
    text.push_str(&complete);
    write(
        home.path(),
        ".codex/sessions/rollout-test-root.jsonl",
        &text,
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let files = &probe.sessions["root"];
    assert!(files.lifecycle_availability.complete, "{files:?}");
    assert_eq!(files.parent_lifecycle.state, WorkerState::Completed);
}

#[trace("FR-018-AC-29")]
#[test]
fn missing_hosts_alone_make_coverage_partial_and_a_total_loss_failed() {
    let mut healthy = report(vec![machine()]);
    assert_eq!(healthy.coverage(), Coverage::Complete);
    healthy.missing_hosts.push(MissingHost {
        machine: "luna".into(),
        reason: "timed out after 45s".into(),
        elapsed_ms: 45_000,
    });
    assert!(healthy.errors.is_empty());
    assert_eq!(healthy.coverage(), Coverage::Partial);
    healthy.agents.clear();
    assert_eq!(healthy.coverage(), Coverage::Failed);
}

#[trace("FR-018-AC-28")]
#[test]
fn a_rollout_within_the_replay_limit_keeps_spawns_from_earlier_turns() {
    let home = tempfile::tempdir().unwrap();
    let mut text = format!("{}\n", codex_header("root", None, "/root"));
    text.push_str(&event("task_started", "2026-10-18T10:00:01Z", "t1"));
    text.push_str(&json_line(
        &json!({"type":"response_item","timestamp":"2026-10-18T10:00:02Z","payload":{"type":"function_call","name":"spawn_agent","call_id":"spawn","arguments":"{}"}}),
    ));
    text.push_str(&json_line(
        &json!({"type":"response_item","timestamp":"2026-10-18T10:00:03Z","payload":{"type":"function_call_output","call_id":"spawn","output":"{\"task_name\":\"/root/child\"}"}}),
    ));
    text.push_str(&event("task_complete", "2026-10-18T10:00:04Z", "t1"));
    text.push_str(&event("task_started", "2026-10-18T10:00:05Z", "t2"));
    write(
        home.path(),
        ".codex/sessions/rollout-test-root.jsonl",
        &text,
    );
    let probe = collect_probe(home.path(), &request(HarnessKind::Codex)).unwrap();
    let files = &probe.sessions["root"];
    assert!(
        files
            .lifecycle_availability
            .reasons
            .contains(&"referenced_child_missing_or_ambiguous".into()),
        "{:?}",
        files.lifecycle_availability
    );
}
