// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Acceptance tests for the agent activity detector (FR-018). Machines are
//! fixtures behind the `Host` trait; no test touches herdr, ssh or the network.

use ix_board::agents::{
    AgentRow, AgentsError, AgentsReport, Host, Probe, ProbeRequest, ReadyTicket, Verdict,
    adapter_for, build_report, parse_probe, render_text,
};
use std::collections::BTreeMap;

const NOW: u64 = 1_800_000_000;
const MIN: u64 = 60;
const IDLE_AFTER: u64 = 20 * MIN;
const PANE: &str = "w1:p1";

const CLAUDE_ASKING: &str = include_str!("fixtures/agents/claude_asking.jsonl");
const CLAUDE_DONE: &str = include_str!("fixtures/agents/claude_done.jsonl");
const CODEX_ASKING: &str = include_str!("fixtures/agents/codex_asking.jsonl");
const CODEX_DONE: &str = include_str!("fixtures/agents/codex_done.jsonl");

#[derive(Clone, Copy, Debug)]
enum Harness {
    Claude,
    Codex,
}

impl Harness {
    fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn asking(self) -> &'static str {
        match self {
            Self::Claude => CLAUDE_ASKING,
            Self::Codex => CODEX_ASKING,
        }
    }

    fn done(self) -> &'static str {
        match self {
            Self::Claude => CLAUDE_DONE,
            Self::Codex => CODEX_DONE,
        }
    }
}

/// One agent pane on a fixture machine.
struct Scenario {
    harness: &'static str,
    status: &'static str,
    transcript: &'static str,
    /// Seconds before `NOW` the transcript was last written.
    turn_age: u64,
    /// Seconds before `NOW` for each subagent transcript.
    child_ages: Vec<u64>,
    processes: Vec<(&'static str, &'static str)>,
    tail: &'static str,
}

impl Scenario {
    fn new(harness: Harness, status: &'static str) -> Self {
        Self {
            harness: harness.name(),
            status,
            transcript: harness.done(),
            turn_age: 5 * MIN,
            child_ages: Vec::new(),
            processes: Vec::new(),
            tail: "❯",
        }
    }

    fn transcript(mut self, text: &'static str) -> Self {
        self.transcript = text;
        self
    }

    fn turn_age(mut self, secs: u64) -> Self {
        self.turn_age = secs;
        self
    }

    fn children(mut self, ages: &[u64]) -> Self {
        self.child_ages = ages.to_vec();
        self
    }

    fn process(mut self, comm: &'static str, args: &'static str) -> Self {
        self.processes.push((comm, args));
        self
    }

    fn tail(mut self, text: &'static str) -> Self {
        self.tail = text;
        self
    }
}

struct FakeHost {
    label: &'static str,
    workspace: &'static str,
    scenario: Option<Scenario>,
    herdr_down: bool,
    herdr_hangs: bool,
}

impl FakeHost {
    fn new(label: &'static str, workspace: &'static str, scenario: Scenario) -> Self {
        Self {
            label,
            workspace,
            scenario: Some(scenario),
            herdr_down: false,
            herdr_hangs: false,
        }
    }

    fn probe_text(&self) -> String {
        let Some(scenario) = &self.scenario else {
            return format!("@@NOW {NOW}\n");
        };
        let mut text = format!("@@NOW {NOW}\n@@SESSION sid-1\n");
        text += &format!("@@MTIME {}\n", NOW - scenario.turn_age);
        for age in &scenario.child_ages {
            text += &format!("@@CHILD {}\n", NOW - age);
        }
        text += "@@TAIL\n";
        text += scenario.transcript;
        text += "@@END\n";
        text += &format!("@@GIT /work\n@@COMMIT {}\n@@END\n", NOW - 3 * 3600);
        for (index, (comm, args)) in scenario.processes.iter().enumerate() {
            let pid = 100 + index;
            text += &format!("@@P {pid}\t{PANE}\t{comm}\t{args}\n");
        }
        text
    }
}

impl Host for FakeHost {
    fn label(&self) -> &str {
        self.label
    }

    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError> {
        if self.herdr_hangs {
            return Err(AgentsError::Timeout {
                command: "herdr --machine titan".into(),
                secs: 60,
            });
        }
        if self.herdr_down {
            return Err(AgentsError::CommandFailed {
                command: "herdr".into(),
                status: "exit status: 1".into(),
                stderr: "server not running".into(),
            });
        }
        match args {
            ["workspace", "list"] => Ok(format!(
                r#"{{"result":{{"workspaces":[{{"workspace_id":"w1","label":"{}"}},{{"workspace_id":"w9","label":"other"}}]}}}}"#,
                self.workspace
            )),
            ["agent", "list"] => Ok(match &self.scenario {
                Some(s) => format!(
                    r#"{{"result":{{"agents":[{{"agent":"{}","agent_session":{{"value":"sid-1"}},"agent_status":"{}","cwd":"/work","pane_id":"{PANE}","workspace_id":"w1","terminal_title_stripped":"worker"}}]}}}}"#,
                    s.harness, s.status
                ),
                None => r#"{"result":{"agents":[]}}"#.to_owned(),
            }),
            ["agent", "read", pane, ..] => {
                assert_eq!(*pane, PANE);
                Ok(self.scenario.as_ref().map_or("", |s| s.tail).to_owned())
            }
            other => panic!("unexpected herdr call {other:?}"),
        }
    }

    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError> {
        if let Some(scenario) = &self.scenario {
            let wants_session = adapter_for(scenario.harness).is_some();
            assert_eq!(request.sessions.len(), usize::from(wants_session));
            assert_eq!(request.cwds, ["/work"]);
        }
        parse_probe(&self.probe_text())
    }
}

fn no_queue(_: &str) -> Result<Option<ReadyTicket>, String> {
    Ok(None)
}

fn report_for(hosts: Vec<FakeHost>) -> AgentsReport {
    let hosts: Vec<Box<dyn Host>> = hosts.into_iter().map(|h| Box::new(h) as _).collect();
    build_report(&hosts, &["QSL".to_owned()], IDLE_AFTER, &mut no_queue)
}

fn verdict_of(scenario: Scenario) -> AgentRow {
    let report = report_for(vec![FakeHost::new("local", "QSL", scenario)]);
    assert_eq!(report.agents.len(), 1, "{report:?}");
    report.agents.into_iter().next().unwrap()
}

const BOTH: [Harness; 2] = [Harness::Claude, Harness::Codex];

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn herdr_working_is_working() {
    for h in BOTH {
        let row = verdict_of(Scenario::new(h, "working").turn_age(3 * 3600));
        assert_eq!(row.verdict, Verdict::Working, "{h:?}");
        assert!(row.pane_tail.is_none(), "working panes are not tailed");
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn blocked_is_prompt_with_dialog_text() {
    for h in BOTH {
        let row =
            verdict_of(Scenario::new(h, "blocked").tail("Allow this command?\n  1. Yes\n  2. No"));
        assert_eq!(row.verdict, Verdict::Prompt, "{h:?}");
        assert!(row.excerpt.unwrap().contains("Allow this command?"));
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn live_subagent_makes_an_idle_lead_working() {
    for h in BOTH {
        let row = verdict_of(
            Scenario::new(h, "idle")
                .turn_age(3 * 3600)
                .children(&[4 * MIN, 50 * MIN]),
        );
        assert_eq!(row.verdict, Verdict::Working, "{h:?}");
        assert_eq!(row.live_subagents, 1);
        assert_eq!(row.subagent_age_secs, Some(4 * MIN));
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn running_gate_process_makes_an_idle_agent_working() {
    for h in BOTH {
        let row = verdict_of(
            Scenario::new(h, "idle")
                .turn_age(3 * 3600)
                .process("bash", "-bash")
                .process("sh", "/x/locked-build.sh /t cargo test")
                .process("bash", "bash -c echo locked-build.sh is mentioned here")
                .process("cargo", "cargo test")
                .process("cargo", "cargo test --lib"),
        );
        assert_eq!(row.verdict, Verdict::Working, "{h:?}");
        assert_eq!(row.processes, ["sh", "cargo"]);
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn background_wait_without_subagent_writes_is_waiting_stuck() {
    for h in BOTH {
        let stuck = Scenario::new(h, "idle")
            .turn_age(40 * MIN)
            .children(&[20 * MIN])
            .tail("Waiting for background agents");
        assert_eq!(verdict_of(stuck).verdict, Verdict::WaitingStuck, "{h:?}");
        let moving = Scenario::new(h, "idle")
            .turn_age(40 * MIN)
            .children(&[12 * MIN])
            .tail("Waiting for background agents");
        assert_eq!(verdict_of(moving).verdict, Verdict::Working, "{h:?}");
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn unclassified_agent_is_unknown_with_raw_data() {
    for h in BOTH {
        let row = verdict_of(Scenario::new(h, "weird-status"));
        assert_eq!(row.verdict, Verdict::Unknown, "{h:?}");
        assert!(row.excerpt.unwrap().contains("weird-status"));
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn idle_agent_whose_last_message_asks_is_asking_with_excerpt() {
    for h in BOTH {
        let row = verdict_of(
            Scenario::new(h, "done")
                .transcript(h.asking())
                .turn_age(2 * MIN),
        );
        assert_eq!(row.verdict, Verdict::Asking, "{h:?}");
        assert!(row.excerpt.unwrap().ends_with("Should I open the PR now?"));
    }
}

/// Trace: FR-018-AC-2, FR-018-AC-3
#[test]
fn idle_threshold_splits_working_from_idle_at_exactly_idle_after() {
    for h in BOTH {
        let recent = Scenario::new(h, "idle").turn_age(IDLE_AFTER - 1);
        assert_eq!(verdict_of(recent).verdict, Verdict::Working, "{h:?}");
        let at_threshold = Scenario::new(h, "idle").turn_age(IDLE_AFTER);
        let row = verdict_of(at_threshold);
        assert_eq!(row.verdict, Verdict::Idle, "{h:?}");
        assert_eq!(row.last_turn_age_secs, Some(IDLE_AFTER));
        assert_eq!(row.git.last_commit_secs, Some(3 * 3600));
        assert!(!row.partial);
    }
}

/// Trace: FR-018-AC-4
#[test]
fn agent_without_an_adapter_keeps_its_tail_and_processes_and_is_partial() {
    let scenario = Scenario {
        harness: "opencode",
        ..Scenario::new(Harness::Claude, "idle")
            .process("cargo", "cargo build")
            .tail("opencode ready")
    };
    let row = verdict_of(scenario);
    assert!(row.partial);
    assert_eq!(row.pane_tail.as_deref(), Some("opencode ready"));
    assert_eq!(row.processes, ["cargo"]);
    assert_eq!(row.last_turn_age_secs, None);
    assert_eq!(row.verdict, Verdict::Working);
}

/// Trace: FR-018-AC-4
#[test]
fn missing_transcript_is_partial_not_dropped() {
    let mut host = FakeHost::new("local", "QSL", Scenario::new(Harness::Codex, "idle"));
    let scenario = host.scenario.as_mut().unwrap();
    scenario.turn_age = 0;
    // Probe reports the session with no transcript.
    struct NoTranscript(FakeHost);
    impl Host for NoTranscript {
        fn label(&self) -> &str {
            self.0.label()
        }
        fn herdr(&self, args: &[&str]) -> Result<String, AgentsError> {
            self.0.herdr(args)
        }
        fn probe(&self, _: &ProbeRequest) -> Result<Probe, AgentsError> {
            parse_probe(&format!("@@NOW {NOW}\n@@SESSION sid-1\n@@END\n"))
        }
    }
    let hosts: Vec<Box<dyn Host>> = vec![Box::new(NoTranscript(host))];
    let report = build_report(&hosts, &["QSL".to_owned()], IDLE_AFTER, &mut no_queue);
    assert_eq!(report.agents.len(), 1);
    let row = &report.agents[0];
    assert!(row.partial);
    assert_eq!(row.verdict, Verdict::Idle);
    assert!(row.notes.iter().any(|n| n.contains("transcript not found")));
}

/// Trace: FR-018-AC-1
#[test]
fn rows_cover_every_machine_and_failures_do_not_hide_other_rows() {
    let mut down = FakeHost::new("titan", "QSL", Scenario::new(Harness::Claude, "idle"));
    down.herdr_down = true;
    let report = report_for(vec![
        FakeHost::new("local", "qsl", Scenario::new(Harness::Claude, "working")),
        FakeHost::new("luna", "QSL", Scenario::new(Harness::Codex, "working")),
        down,
    ]);
    let machines: Vec<_> = report.agents.iter().map(|r| r.machine.as_str()).collect();
    assert_eq!(machines, ["local", "luna"]);
    assert_eq!(report.agents[0].team, "qsl", "label as herdr spells it");
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].starts_with("titan: "));
    assert!(report.missing_teams.is_empty());
}

/// Trace: FR-018-AC-1, FR-018-AC-5
#[test]
fn a_machine_that_times_out_becomes_an_error_and_other_rows_survive() {
    let mut hung = FakeHost::new("titan", "QSL", Scenario::new(Harness::Claude, "idle"));
    hung.herdr_hangs = true;
    let report = report_for(vec![
        FakeHost::new("local", "QSL", Scenario::new(Harness::Claude, "working")),
        hung,
    ]);
    assert_eq!(report.agents.len(), 1);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].starts_with("titan: "));
    assert!(report.errors[0].contains("did not finish within 60s"));
}

/// Trace: FR-018-AC-1
#[test]
fn unmatched_team_is_listed_as_missing() {
    let hosts: Vec<Box<dyn Host>> = vec![Box::new(FakeHost::new(
        "local",
        "QSL",
        Scenario::new(Harness::Claude, "working"),
    ))];
    let teams = ["QSL".to_owned(), "IR".to_owned()];
    let report = build_report(&hosts, &teams, IDLE_AFTER, &mut no_queue);
    assert_eq!(report.missing_teams, ["IR"]);
    assert_eq!(report.agents.len(), 1);
}

/// Trace: FR-018-AC-5
#[test]
fn probe_parser_joins_processes_to_panes_and_keeps_the_remote_clock() {
    let text = format!(
        "@@NOW {NOW}\n@@SESSION s\n@@MTIME 5\n@@CHILD 6\n@@TAIL\n{{\"a\":1}}\n@@END\n\
         @@GIT /w\n@@COMMIT 7\n@@DIRTY 8\n@@END\n\
         @@P 10\tw1:p2\tcargo\tcargo test --all\n\
         @@P 11\tw1:p2\tnpm exec prism-\tnode /x/prism-mcp\n"
    );
    let probe = parse_probe(&text).unwrap();
    assert_eq!(probe.now, NOW);
    let session = &probe.sessions["s"];
    assert_eq!(session.transcript_mtime, Some(5));
    assert_eq!(session.child_mtimes, [6]);
    assert_eq!(session.tail, "{\"a\":1}\n");
    assert_eq!(probe.git["/w"].last_commit, Some(7));
    assert_eq!(probe.git["/w"].uncommitted, Some(8));
    let procs = &probe.processes["w1:p2"];
    assert_eq!(procs.len(), 2);
    assert_eq!(procs[0].comm, "cargo");
    assert_eq!(procs[0].args, "cargo test --all");
    assert_eq!(procs[1].comm, "npm exec prism-", "comm keeps its spaces");
    assert!(parse_probe("@@NOPE 1\n").is_err());
}

/// Trace: FR-018-AC-6
#[test]
fn idle_rows_carry_the_next_ready_ticket_and_an_unreadable_queue_is_reported_once() {
    let hosts = |status| -> Vec<Box<dyn Host>> {
        vec![
            Box::new(FakeHost::new(
                "local",
                "QSL",
                Scenario::new(Harness::Claude, status).turn_age(3 * 3600),
            )),
            Box::new(FakeHost::new(
                "luna",
                "QSL",
                Scenario::new(Harness::Codex, status).turn_age(3 * 3600),
            )),
        ]
    };
    let mut calls = BTreeMap::<String, u32>::new();
    let mut readable = |team: &str| {
        *calls.entry(team.to_owned()).or_default() += 1;
        Ok(Some(ReadyTicket {
            identifier: "QSL-523".into(),
            title: "Next thing".into(),
        }))
    };
    let teams = ["QSL".to_owned()];
    let report = build_report(&hosts("idle"), &teams, IDLE_AFTER, &mut readable);
    assert!(report.agents.iter().all(|r| r.verdict == Verdict::Idle));
    assert!(
        report
            .agents
            .iter()
            .all(|r| r.next_ready.as_ref().unwrap().identifier == "QSL-523")
    );
    assert_eq!(calls["QSL"], 1, "one lookup per team");

    let mut broken = |_: &str| Err("no Linear credential".to_owned());
    let report = build_report(&hosts("idle"), &teams, IDLE_AFTER, &mut broken);
    assert!(report.agents.iter().all(|r| r.verdict == Verdict::Idle));
    assert!(report.agents.iter().all(|r| r.next_ready.is_none()));
    assert_eq!(
        report.ready_queue_error.as_deref(),
        Some("no Linear credential")
    );

    let report = build_report(&hosts("working"), &teams, IDLE_AFTER, &mut broken);
    assert_eq!(
        report.ready_queue_error, None,
        "queue is read for IDLE rows only"
    );
}

/// Trace: FR-018-AC-7
#[test]
fn text_and_json_carry_the_same_rows() {
    let report = report_for(vec![
        FakeHost::new(
            "local",
            "QSL",
            Scenario::new(Harness::Claude, "idle")
                .transcript(CLAUDE_ASKING)
                .turn_age(30 * MIN),
        ),
        FakeHost::new(
            "luna",
            "QSL",
            Scenario::new(Harness::Codex, "idle").turn_age(2 * 3600),
        ),
    ]);
    let json: serde_json::Value = serde_json::to_value(&report).unwrap();
    let verdicts: Vec<_> = json["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["verdict"].as_str().unwrap())
        .collect();
    assert_eq!(verdicts, ["ASKING", "IDLE"]);
    let text = render_text(&report);
    for (row, verdict) in report.agents.iter().zip(verdicts) {
        let line = text.lines().find(|l| l.contains(&row.machine)).unwrap();
        assert!(line.contains(verdict), "{line}");
    }
    assert!(text.contains("Should I open the PR now?"));
    assert!(text.contains("30m"));
    assert!(text.contains("2h00m"));
}

/// Trace: FR-018-AC-7
#[test]
fn idle_after_without_a_unit_is_refused() {
    assert_cmd::Command::cargo_bin("ix-board")
        .unwrap()
        .args(["agents", "QSL", "--idle-after", "20"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unit"));
}
