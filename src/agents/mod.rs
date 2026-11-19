// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board agents`: per-agent activity verdicts for herdr workspaces
//! (FR-018).
//!
//! A team is a herdr workspace label. For every agent pane in a named team on
//! every machine this module gathers herdr state, transcript ages, live
//! subagents, child processes, the pane tail and git ages, and decides one
//! [`Verdict`] with [`decide`]. All machine access goes through [`Host`].

mod collect;
mod harness;
mod herdr;
mod lifecycle;
mod probe;

pub use harness::{Harness, HarnessKind, adapter_for};
pub use herdr::{
    HerdrAgent, HerdrMachine, HerdrState, HerdrWorkspace, parse_agents, parse_machines,
    parse_workspaces,
};
pub use probe::{
    GitTimes, Host, PaneProcess, Probe, ProbeRequest, SessionFiles, SessionQuery, SystemHost,
    parse_probe,
};

pub use collect::collect_probe;
pub use lifecycle::{
    AgentAvailability, AgentId, Availability, EvidenceSource, Lifecycle, WorkerCounts,
    WorkerObservation, WorkerRole, WorkerState, count_workers,
};
pub use probe::{MAX_PROTOCOL_BYTES, PROBE_VERSION, ProbeEnvelope, ProbeResponse, serve_probe};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Lifecycle report schema.
pub const AGENTS_SCHEMA_VERSION: u32 = 2;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error as ThisError;

/// A background wait with no activity for this many seconds is stuck.
const STUCK_AFTER_SECS: u64 = 15 * 60;
/// Lines of pane tail read from herdr.
const TAIL_LINES: &str = "30";
/// Characters kept in an excerpt.
const EXCERPT_CHARS: usize = 600;
/// Commands whose presence in a pane means a build or gate is running.
const WORK_COMMANDS: [&str; 9] = [
    "cargo", "rustc", "make", "gh", "flock", "pytest", "npm", "pnpm", "kani",
];

/// Failures reading machines or decoding what they returned.
#[derive(Debug, ThisError)]
pub enum AgentsError {
    /// A command could not be started or its pipes failed.
    #[error("could not run {command}: {source}")]
    Spawn {
        /// Rendered command.
        command: String,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// A command exited unsuccessfully.
    #[error("{command} exited with {status}: {stderr}")]
    CommandFailed {
        /// Rendered command.
        command: String,
        /// Exit status text.
        status: String,
        /// Trimmed standard error.
        stderr: String,
    },
    /// A command ran past its time limit and was killed.
    #[error("{command} did not finish within {secs}s")]
    Timeout {
        /// Rendered command.
        command: String,
        /// Limit in seconds.
        secs: u64,
    },
    /// herdr returned JSON of an unexpected shape.
    #[error("could not decode {what}: {source}")]
    Decode {
        /// Which output failed to decode.
        what: &'static str,
        /// Decoder failure.
        source: serde_json::Error,
    },
    /// The probe script printed a record this parser does not know.
    #[error("unrecognized probe record: {0}")]
    ProbeFormat(String),
}

/// What an agent is doing right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Verdict {
    /// Positive current lead execution.
    #[serde(rename = "WORKING")]
    Working,
    /// The lead awaits unfinished worker assignments.
    #[serde(rename = "WAITING")]
    Waiting,
    /// Execution ended recently; no assigned workers remain.
    #[serde(rename = "RECENT")]
    Recent,
    /// Confirmed pending wait with complete evidence of no viable work.
    #[serde(rename = "WAITING-STUCK")]
    WaitingStuck,
    /// Stopped at an approval or question dialog.
    #[serde(rename = "PROMPT")]
    Prompt,
    /// Finished a turn by asking a question or for permission.
    #[serde(rename = "ASKING")]
    Asking,
    /// Ready for input and quiet past the idle threshold.
    #[serde(rename = "IDLE")]
    Idle,
    /// herdr could not classify the agent.
    #[serde(rename = "UNKNOWN")]
    Unknown,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Self::Working => "WORKING",
            Self::Waiting => "WAITING",
            Self::Recent => "RECENT",
            Self::WaitingStuck => "WAITING-STUCK",
            Self::Prompt => "PROMPT",
            Self::Asking => "ASKING",
            Self::Idle => "IDLE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Normalized evidence consumed by the harness-independent classifier.
#[derive(Clone, Debug)]
pub struct Evidence<'a> {
    /// Current Herdr observation.
    pub herdr: HerdrState,
    /// Pane text, read even when Herdr reports working.
    pub tail: Option<&'a str>,
    /// Shared parent lifecycle facts.
    pub lifecycle: &'a Lifecycle,
    /// Deduplicated worker graph and independent exact counts.
    pub workers: &'a WorkerCounts,
    /// Attributed executing build/gate commands.
    pub active_processes: usize,
    /// Whether process absence is established.
    pub processes_complete: bool,
    /// Whether the parent transcript was fully understood.
    pub transcript_complete: bool,
    /// Host clock at observation.
    pub now: u64,
    /// Quiet threshold in seconds.
    pub idle_after: u64,
}

/// Applies lifecycle verdict precedence, without using file ages as execution.
pub fn decide(e: &Evidence<'_>) -> Verdict {
    let life = e.lifecycle;
    if e.herdr == HerdrState::Blocked
        || e.tail.is_some_and(|tail| {
            let text = tail.to_lowercase();
            text.contains("esc to cancel")
                && (text.contains("allow once") || text.contains("yes, proceed"))
        })
    {
        return Verdict::Prompt;
    }
    let terminal = life.execution_ended_at.is_some();
    let awaiting = life.state == WorkerState::Waiting
        || e.tail
            .is_some_and(|tail| tail.to_lowercase().contains("waiting for background agent"));
    if life.assistant.as_deref().is_some_and(asks_for_input)
        && (terminal || matches!(e.herdr, HerdrState::Idle | HerdrState::Done))
        && e.active_processes == 0
    {
        return Verdict::Asking;
    }
    let clock_bad = life.last_activity_at.is_some_and(|at| at > e.now)
        || life.state_changed_at.is_some_and(|at| at > e.now);
    if clock_bad
        || !life.issues.is_empty()
        || (terminal && e.herdr == HerdrState::Working && !awaiting)
        || (terminal && e.active_processes > 0)
    {
        return Verdict::Unknown;
    }
    let any_workers = e.workers.known_direct_active > 0 || e.workers.known_nested_active > 0;
    let direct_active = e.workers.known_direct_active > 0;
    if awaiting {
        if any_workers {
            return Verdict::Waiting;
        }
        if e.workers.direct == Some(0)
            && e.workers.nested == Some(0)
            && e.processes_complete
            && e.active_processes == 0
            && e.transcript_complete
            && life.state == WorkerState::Waiting
            && life
                .state_changed_at
                .zip(life.last_activity_at)
                .is_some_and(|(wait, activity)| {
                    e.now.saturating_sub(wait.max(activity)) >= STUCK_AFTER_SECS
                })
        {
            return Verdict::WaitingStuck;
        }
        return Verdict::Unknown;
    }
    let current = e.herdr == HerdrState::Working || e.active_processes > 0;
    if !current && direct_active {
        return Verdict::Waiting;
    }
    // Reading pane evidence is necessary to rule out a pending explicit wait.
    if current && e.tail.is_some() {
        return Verdict::Working;
    }
    if e.herdr == HerdrState::Unknown
        || !e.transcript_complete
        || e.tail.is_none()
        || e.workers.direct != Some(0)
        || e.workers.nested != Some(0)
    {
        return Verdict::Unknown;
    }
    // An old open task is not proof of live execution or established quiet.
    if terminal && let Some(age) = life.last_activity_at.and_then(|at| e.now.checked_sub(at)) {
        return if age < e.idle_after {
            Verdict::Recent
        } else {
            Verdict::Idle
        };
    }
    Verdict::Unknown
}

/// Whether an assistant message ends by asking the human something.
pub fn asks_for_input(message: &str) -> bool {
    const PHRASES: [&str; 8] = [
        "should i",
        "shall i",
        "want me to",
        "do you want",
        "ok to proceed",
        "please confirm",
        "your call",
        "need your approval",
    ];
    let trimmed = message.trim_end();
    let last_line = trimmed.lines().next_back().unwrap_or("").trim_end();
    if last_line
        .trim_end_matches(['*', '_', '`', ')', '"'])
        .ends_with('?')
    {
        return true;
    }
    let tail_start = trimmed
        .char_indices()
        .rev()
        .nth(300)
        .map_or(0, |(index, _)| index);
    let recent = trimmed[tail_start..].to_lowercase();
    PHRASES.iter().any(|phrase| recent.contains(phrase))
}

/// The next ready ticket of a team.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadyTicket {
    /// Linear issue identifier.
    pub identifier: String,
    /// Issue title.
    pub title: String,
}

/// Ages of a pane's git working directory, in seconds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitAges {
    /// Seconds since the last commit.
    pub last_commit_secs: Option<u64>,
    /// Seconds since the newest uncommitted file changed.
    pub uncommitted_secs: Option<u64>,
}

/// One agent pane and its verdict.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentRow {
    /// Team (workspace label).
    pub team: String,
    /// Machine label.
    pub machine: String,
    /// Pane id within the machine's herdr server.
    pub pane_id: String,
    /// Harness name herdr reported.
    pub agent: String,
    /// Terminal title, which usually names the agent.
    pub title: String,
    /// Pane working directory.
    pub cwd: String,
    /// Raw herdr status string.
    pub herdr_status: String,
    /// Computed verdict.
    pub verdict: Verdict,
    /// True when no transcript could be read for this pane.
    pub partial: bool,
    /// Seconds since the last turn.
    pub last_turn_age_secs: Option<u64>,
    /// Seconds since the newest subagent write.
    pub subagent_age_secs: Option<u64>,
    /// Deprecated nullable alias of active_direct_workers.
    pub live_subagents: Option<usize>,
    /// Active unfinished direct assignments; null means incomplete evidence.
    pub active_direct_workers: Option<usize>,
    /// Active unfinished descendants at depth two or greater.
    pub active_nested_workers: Option<usize>,
    /// Normalized observations, including excluded internal sessions.
    pub workers: Vec<WorkerObservation>,
    /// Independent source availability.
    pub availability: AgentAvailability,
    /// Last structured execution activity, independent of file writes.
    pub recent_activity_at: Option<u64>,
    /// Clock of the host holding this row's files.
    pub host_now: u64,
    /// Collection start and end in that host's clock domain.
    pub collection_started_at: u64,
    /// Collection end in that host's clock domain.
    pub collection_finished_at: u64,
    /// Build or gate commands running in the pane.
    pub processes: Vec<String>,
    /// Git ages of the pane directory.
    pub git: GitAges,
    /// Evidence text for `PROMPT`, `ASKING` and `UNKNOWN`.
    pub excerpt: Option<String>,
    /// Last lines of the pane, read for every state.
    pub pane_tail: Option<String>,
    /// The team's next ready ticket, on `IDLE` rows.
    pub next_ready: Option<ReadyTicket>,
    /// Degradations that affected this row.
    pub notes: Vec<String>,
}

/// Result of one `agents` invocation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentsReport {
    /// Lifecycle schema identity.
    pub schema_version: u32,
    /// Teams asked for.
    pub teams: Vec<String>,
    /// Idle threshold in seconds.
    pub idle_after_secs: u64,
    /// One row per agent pane.
    pub agents: Vec<AgentRow>,
    /// Teams that matched no workspace on any machine.
    pub missing_teams: Vec<String>,
    /// Machines or reads that failed.
    pub errors: Vec<String>,
    /// Why `IDLE` rows carry no next ticket, when the queue was unreadable.
    pub ready_queue_error: Option<String>,
}

/// Looks up a team's next ready ticket; an `Err` is a reason, not a verdict.
pub type ReadyLookup<'a> = dyn FnMut(&str) -> Result<Option<ReadyTicket>, String> + 'a;

struct HostResult {
    rows: Vec<AgentRow>,
    found: BTreeSet<String>,
    errors: Vec<String>,
}

/// The local machine plus every enabled saved herdr machine.
pub fn system_hosts() -> Result<Vec<Box<dyn Host>>, AgentsError> {
    let local = SystemHost::local();
    let machines = parse_machines(&local.herdr(&["machine", "list", "--json"])?)?;
    let mut hosts: Vec<Box<dyn Host>> = vec![Box::new(local)];
    hosts.extend(
        machines
            .into_iter()
            .filter(|machine| machine.enabled)
            .map(|machine| {
                Box::new(SystemHost::remote(machine.label, machine.target)) as Box<dyn Host>
            }),
    );
    Ok(hosts)
}

/// Reads every host in parallel and builds the report.
pub fn build_report(
    hosts: &[Box<dyn Host>],
    teams: &[String],
    idle_after_secs: u64,
    ready: &mut ReadyLookup<'_>,
) -> AgentsReport {
    let results: Vec<HostResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = hosts
            .iter()
            .map(|host| scope.spawn(|| gather_host(host.as_ref(), teams, idle_after_secs)))
            .collect();
        handles
            .into_iter()
            .zip(hosts)
            .map(|(handle, host)| {
                handle.join().unwrap_or_else(|_| HostResult {
                    rows: Vec::new(),
                    found: BTreeSet::new(),
                    errors: vec![format!("{}: reader thread panicked", host.label())],
                })
            })
            .collect()
    });
    let mut agents = Vec::new();
    let mut found = BTreeSet::new();
    let mut errors = Vec::new();
    for result in results {
        agents.extend(result.rows);
        found.extend(result.found);
        errors.extend(result.errors);
    }
    let order = |team: &str| {
        teams
            .iter()
            .position(|wanted| wanted.eq_ignore_ascii_case(team))
            .unwrap_or(usize::MAX)
    };
    agents.sort_by(|a, b| {
        (order(&a.team), &a.machine, &a.pane_id).cmp(&(order(&b.team), &b.machine, &b.pane_id))
    });
    let ready_queue_error = attach_next_ready(&mut agents, ready);
    let missing_teams = teams
        .iter()
        .filter(|team| !found.contains(&team.to_lowercase()))
        .cloned()
        .collect();
    AgentsReport {
        schema_version: AGENTS_SCHEMA_VERSION,
        teams: teams.to_vec(),
        idle_after_secs,
        agents,
        missing_teams,
        errors,
        ready_queue_error,
    }
}

fn attach_next_ready(agents: &mut [AgentRow], ready: &mut ReadyLookup<'_>) -> Option<String> {
    let mut cache: BTreeMap<String, Option<ReadyTicket>> = BTreeMap::new();
    let mut failure = None;
    for row in agents.iter_mut().filter(|row| row.verdict == Verdict::Idle) {
        if !cache.contains_key(&row.team) {
            let ticket = ready(&row.team).unwrap_or_else(|reason| {
                failure.get_or_insert(reason);
                None
            });
            cache.insert(row.team.clone(), ticket);
        }
        row.next_ready = cache.get(&row.team).cloned().flatten();
    }
    failure
}

fn gather_host(host: &dyn Host, teams: &[String], idle_after: u64) -> HostResult {
    let mut result = HostResult {
        rows: Vec::new(),
        found: BTreeSet::new(),
        errors: Vec::new(),
    };
    let label = host.label();
    let listed = host
        .herdr(&["workspace", "list"])
        .and_then(|text| parse_workspaces(&text))
        .and_then(|workspaces| {
            let agents = parse_agents(&host.herdr(&["agent", "list"])?)?;
            Ok((workspaces, agents))
        });
    let (workspaces, agents) = match listed {
        Ok(listed) => listed,
        Err(error) => {
            result.errors.push(format!("{label}: {error}"));
            return result;
        }
    };
    let team_of: BTreeMap<&str, &HerdrWorkspace> = workspaces
        .iter()
        .filter(|workspace| {
            teams
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&workspace.label))
        })
        .map(|workspace| (workspace.workspace_id.as_str(), workspace))
        .collect();
    result.found.extend(
        team_of
            .values()
            .map(|workspace| workspace.label.to_lowercase()),
    );
    let agents: Vec<&HerdrAgent> = agents
        .iter()
        .filter(|agent| team_of.contains_key(agent.workspace_id.as_str()))
        .collect();
    let request = ProbeRequest {
        sessions: agents
            .iter()
            .filter_map(|agent| {
                let kind = adapter_for(&agent.agent)?.kind();
                Some(SessionQuery {
                    kind,
                    session_id: agent.session_id()?.to_owned(),
                })
            })
            .collect(),
        cwds: agents
            .iter()
            .map(|agent| agent.cwd.clone())
            .filter(|cwd| !cwd.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    };
    let (probe, probe_note) = match host.probe(&request) {
        Ok(probe) => (probe, None),
        Err(error) => {
            result.errors.push(format!("{label}: {error}"));
            (Probe::default(), Some(format!("probe failed: {error}")))
        }
    };
    for agent in agents {
        let team = team_of[agent.workspace_id.as_str()].label.clone();
        let mut row = build_row(host, agent, &probe, idle_after, team);
        row.notes.extend(probe_note.clone());
        result.rows.push(row);
    }
    result
}

fn build_row(
    host: &dyn Host,
    agent: &HerdrAgent,
    probe: &Probe,
    idle_after: u64,
    team: String,
) -> AgentRow {
    let mut notes = Vec::new();
    let state = agent.state();
    let mut pane_availability = Availability::available();
    let pane_tail = match host.herdr(&["agent", "read", &agent.pane_id, "--lines", TAIL_LINES]) {
        Ok(text) => Some(last_chars(&text, 8192)),
        Err(error) => {
            notes.push(format!("pane tail unreadable: {error}"));
            pane_availability.missing("pane_tail_unreadable");
            None
        }
    };
    let unavailable = SessionFiles::default();
    let files = agent
        .session_id()
        .and_then(|sid| probe.sessions.get(sid))
        .unwrap_or(&unavailable);
    let mut lifecycle_availability = files.lifecycle_availability.clone();
    let mut transcript_availability = files.transcript_availability.clone();
    if adapter_for(&agent.agent).is_none() {
        lifecycle_availability.missing("adapter_unsupported");
        transcript_availability.missing("adapter_unsupported");
    }
    if files == &unavailable {
        lifecycle_availability.missing("session_evidence_missing");
        transcript_availability.missing("session_evidence_missing");
    }
    let mut life = files.parent_lifecycle.clone();
    if life.identity.is_none() {
        life.identity = agent.session_id().map(|id| (AgentId(id.into()), None));
    }
    let timing_valid = probe.started_at <= probe.finished_at && probe.finished_at <= probe.now;
    if !timing_valid {
        life.issue("inconsistent_host_collection_clock");
        transcript_availability.missing("inconsistent_host_collection_clock");
    }
    let counts = count_workers(
        &AgentId(agent.session_id().unwrap_or("").into()),
        &files.workers,
        lifecycle_availability,
    );
    let age = |epoch| {
        if timing_valid {
            probe.now.checked_sub(epoch)
        } else {
            None
        }
    };
    let last_turn_age = files.transcript_mtime.and_then(age);
    let subagent_age = files.child_mtimes.iter().filter_map(|&m| age(m)).min();
    let mut processes = Vec::new();
    for process in probe.processes.get(&agent.pane_id).into_iter().flatten() {
        if is_work_command(process) && !processes.contains(&process.comm) {
            processes.push(process.comm.clone());
        }
    }
    let verdict = decide(&Evidence {
        herdr: state,
        tail: pane_tail.as_deref(),
        lifecycle: &life,
        workers: &counts,
        active_processes: processes.len(),
        processes_complete: probe.process_availability.complete,
        transcript_complete: transcript_availability.complete,
        now: probe.now,
        idle_after,
    });
    let availability = AgentAvailability {
        lifecycle: counts.availability,
        transcript: transcript_availability,
        processes: probe.process_availability.clone(),
        pane: pane_availability,
        git: probe
            .git_availability
            .get(&agent.cwd)
            .cloned()
            .unwrap_or_else(|| {
                let mut source = Availability::default();
                source.missing("git_source_unavailable");
                source
            }),
    };
    for source in [
        &availability.lifecycle,
        &availability.transcript,
        &availability.processes,
        &availability.pane,
        &availability.git,
    ] {
        notes.extend(source.reasons.clone());
    }
    notes.extend(life.issues.clone());
    let excerpt = match verdict {
        Verdict::Prompt => pane_tail.as_deref().map(|t| last_chars(t, EXCERPT_CHARS)),
        Verdict::Asking => life
            .assistant
            .as_deref()
            .map(|t| last_chars(t, EXCERPT_CHARS)),
        Verdict::Unknown => Some(format!(
            "herdr status {:?}; title {:?}; pane {}; {}",
            agent.agent_status,
            agent.terminal_title_stripped,
            agent.pane_id,
            if notes.is_empty() {
                "classification_unresolved".into()
            } else {
                notes.join("; ")
            }
        )),
        _ => None,
    };
    let git = probe
        .git
        .get(&agent.cwd)
        .map(|t| GitAges {
            last_commit_secs: t.last_commit.and_then(age),
            uncommitted_secs: t.uncommitted.and_then(age),
        })
        .unwrap_or_default();
    AgentRow {
        team,
        machine: host.label().into(),
        pane_id: agent.pane_id.clone(),
        agent: agent.agent.clone(),
        title: agent.terminal_title_stripped.clone(),
        cwd: agent.cwd.clone(),
        herdr_status: agent.agent_status.clone(),
        verdict,
        partial: [
            &availability.lifecycle,
            &availability.transcript,
            &availability.processes,
            &availability.pane,
            &availability.git,
        ]
        .iter()
        .any(|s| !s.complete),
        last_turn_age_secs: last_turn_age,
        subagent_age_secs: subagent_age,
        live_subagents: counts.direct,
        active_direct_workers: counts.direct,
        active_nested_workers: counts.nested,
        workers: counts.workers,
        availability,
        recent_activity_at: life.last_activity_at,
        host_now: probe.now,
        collection_started_at: probe.started_at,
        collection_finished_at: probe.finished_at,
        processes,
        git,
        excerpt,
        pane_tail,
        next_ready: None,
        notes,
    }
}

fn is_work_command(process: &PaneProcess) -> bool {
    let runs_gate_wrapper = process
        .args
        .split_whitespace()
        .take(2)
        .any(|word| word.rsplit('/').next() == Some("locked-build.sh"));
    WORK_COMMANDS.contains(&process.comm.as_str()) || runs_gate_wrapper
}

fn last_chars(text: &str, count: usize) -> String {
    let start = text
        .char_indices()
        .rev()
        .nth(count)
        .map_or(0, |(index, _)| index);
    text[start..].trim().to_owned()
}

/// Parses `--idle-after`: a number followed by `s`, `m` or `h`.
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| format!("duration {text:?} needs a unit: s, m or h"))?;
    let (digits, unit) = text.split_at(split);
    let count: u64 = digits
        .parse()
        .map_err(|_| format!("duration {text:?} must start with a number"))?;
    if count == 0 {
        return Err("duration must be positive".into());
    }
    let scale = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return Err(format!("duration {text:?} unit must be s, m or h")),
    };
    count
        .checked_mul(scale)
        .ok_or_else(|| format!("duration {text:?} is too large"))
}

fn minutes(secs: Option<u64>) -> String {
    match secs {
        None => "-".to_owned(),
        Some(secs) if secs < 3600 => format!("{}m", secs / 60),
        Some(secs) => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
    }
}

/// Renders the report as a table with indented evidence lines.
pub fn render_text(report: &AgentsReport) -> String {
    let mut lines = vec![format!(
        "{:<8} {:<8} {:<8} {:<7} {:<14} {:>7} {:>7}  NOTE",
        "TEAM", "MACHINE", "PANE", "AGENT", "VERDICT", "TURN", "SUB"
    )];
    for row in &report.agents {
        let mut flags = Vec::new();
        if row.partial {
            flags.push("partial".to_owned());
        }
        flags.push(format!(
            "direct {} / nested {}",
            row.active_direct_workers
                .map_or("unknown".into(), |n| n.to_string()),
            row.active_nested_workers
                .map_or("unknown".into(), |n| n.to_string())
        ));
        if !row.processes.is_empty() {
            flags.push(row.processes.join("+"));
        }
        if let Some(age) = row.git.uncommitted_secs {
            flags.push(format!("uncommitted {}", minutes(Some(age))));
        }
        lines.push(format!(
            "{:<8} {:<8} {:<8} {:<7} {:<14} {:>7} {:>7}  {}{}",
            row.team,
            row.machine,
            row.pane_id,
            row.agent,
            row.verdict.label(),
            minutes(row.last_turn_age_secs),
            minutes(row.subagent_age_secs),
            row.title,
            if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(", "))
            }
        ));
        if let Some(excerpt) = &row.excerpt {
            lines.push(format!("    {}", excerpt.replace('\n', "\n    ")));
        }
        if let Some(ticket) = &row.next_ready {
            lines.push(format!(
                "    next ready: {} {}",
                ticket.identifier, ticket.title
            ));
        }
    }
    if report.agents.is_empty() {
        lines.push("(no agent panes found)".to_owned());
    }
    for team in &report.missing_teams {
        lines.push(format!("Missing team: {team} matches no workspace"));
    }
    for error in &report.errors {
        lines.push(format!("Error: {error}"));
    }
    if let Some(reason) = &report.ready_queue_error {
        lines.push(format!("Ready queue unavailable: {reason}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_accepts_units_and_refuses_the_rest() {
        assert_eq!(parse_duration("90s"), Ok(90));
        assert_eq!(parse_duration("20m"), Ok(1200));
        assert_eq!(parse_duration("2h"), Ok(7200));
        assert!(parse_duration("20").is_err());
        assert!(parse_duration("m").is_err());
        assert!(parse_duration("5d").is_err());
    }

    #[test]
    fn question_detection_reads_the_end_of_the_message() {
        assert!(asks_for_input("Done.\n\nWhich option do you prefer?"));
        assert!(asks_for_input("Plan ready. Want me to start coding?\n"));
        assert!(asks_for_input("I can do A or B. Should I proceed with A."));
        assert!(!asks_for_input(
            "Is this ok? I checked it myself.\nPR opened."
        ));
        assert!(!asks_for_input("All gates are green and the PR is open."));
    }
}
