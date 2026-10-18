// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Machine access for the detector (FR-018): the `Host` trait, the batched
//! probe request and its output parser, and the system implementation that
//! runs herdr locally and the probe script locally or over `ssh`.

use super::{AgentsError, harness::HarnessKind};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Output, Stdio};

const PROBE_SCRIPT: &str = include_str!("probe.sh");

/// Minutes of recent rollout files scanned for Codex child threads.
const CHILD_WINDOW_MINUTES: u64 = 180;

/// A machine the detector can read: herdr state plus one batched file probe.
pub trait Host: Sync {
    /// Machine label shown in output.
    fn label(&self) -> &str;

    /// Runs `herdr [--machine <label>] <args>` and returns its stdout.
    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError>;

    /// Reads transcripts, processes and git state in one batched round trip.
    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError>;
}

/// One agent session to locate on a machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionQuery {
    /// Harness that owns the transcript layout.
    pub kind: HarnessKind,
    /// Session id from herdr.
    pub session_id: String,
}

/// Everything one probe reads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProbeRequest {
    /// Sessions whose transcripts to locate.
    pub sessions: Vec<SessionQuery>,
    /// Working directories whose git ages to read.
    pub cwds: Vec<String>,
}

impl ProbeRequest {
    fn script_args(&self) -> Vec<String> {
        let mut args = vec![CHILD_WINDOW_MINUTES.to_string()];
        args.extend(
            self.sessions
                .iter()
                .map(|query| format!("session|{}|{}", query.kind.as_str(), query.session_id)),
        );
        args.extend(self.cwds.iter().map(|cwd| format!("git|{cwd}")));
        args
    }
}

/// Files found for one session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionFiles {
    /// Modification time (epoch seconds) of the main transcript.
    pub transcript_mtime: Option<u64>,
    /// Last bytes of the main transcript.
    pub tail: String,
    /// Modification times of subagent or child-thread transcripts.
    pub child_mtimes: Vec<u64>,
}

/// One process running inside a herdr pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneProcess {
    /// Executable name.
    pub comm: String,
    /// Full command line.
    pub args: String,
}

/// Git timestamps (epoch seconds) for a working directory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitTimes {
    /// Committer time of HEAD.
    pub last_commit: Option<u64>,
    /// Newest modification time among uncommitted files.
    pub uncommitted: Option<u64>,
}

/// Parsed probe output.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Probe {
    /// The machine's clock, epoch seconds, when the probe ran.
    pub now: u64,
    /// Files per session id.
    pub sessions: BTreeMap<String, SessionFiles>,
    /// Processes per herdr pane id.
    pub processes: BTreeMap<String, Vec<PaneProcess>>,
    /// Git times per working directory.
    pub git: BTreeMap<String, GitTimes>,
}

enum Section {
    None,
    Session(String),
    Tail(String),
    Git(String),
}

fn bad(line: &str) -> AgentsError {
    AgentsError::ProbeFormat(line.chars().take(120).collect())
}

fn number(line: &str, text: &str) -> Result<u64, AgentsError> {
    text.trim().parse().map_err(|_| bad(line))
}

/// Parses the line records emitted by the probe script.
pub fn parse_probe(text: &str) -> Result<Probe, AgentsError> {
    let mut probe = Probe::default();
    let mut section = Section::None;
    let mut pane_of_pid: BTreeMap<u32, String> = BTreeMap::new();
    let mut ps: Vec<(u32, String, String)> = Vec::new();
    for line in text.lines() {
        if let Section::Tail(sid) = &section {
            if line == "@@END" {
                section = Section::None;
            } else if let Some(files) = probe.sessions.get_mut(sid) {
                files.tail.push_str(line);
                files.tail.push('\n');
            }
            continue;
        }
        let Some(record) = line.strip_prefix("@@") else {
            continue;
        };
        let (tag, value) = record.split_once(' ').unwrap_or((record, ""));
        match (tag, &section) {
            ("NOW", _) => probe.now = number(line, value)?,
            ("SESSION", _) => {
                probe
                    .sessions
                    .insert(value.to_owned(), SessionFiles::default());
                section = Section::Session(value.to_owned());
            }
            ("MTIME", Section::Session(sid)) => {
                if let Some(files) = probe.sessions.get_mut(sid) {
                    files.transcript_mtime = Some(number(line, value)?);
                }
            }
            ("CHILD", Section::Session(sid)) => {
                if let Some(files) = probe.sessions.get_mut(sid) {
                    files.child_mtimes.push(number(line, value)?);
                }
            }
            ("TAIL", Section::Session(sid)) => section = Section::Tail(sid.clone()),
            ("GIT", _) => {
                probe.git.insert(value.to_owned(), GitTimes::default());
                section = Section::Git(value.to_owned());
            }
            ("COMMIT", Section::Git(cwd)) => {
                if let Some(times) = probe.git.get_mut(cwd) {
                    times.last_commit = Some(number(line, value)?);
                }
            }
            ("DIRTY", Section::Git(cwd)) => {
                if let Some(times) = probe.git.get_mut(cwd) {
                    times.uncommitted = Some(number(line, value)?);
                }
            }
            ("END", _) => section = Section::None,
            ("P", _) => {
                let (pid, pane) = value.split_once(' ').ok_or_else(|| bad(line))?;
                let pid = pid.parse().map_err(|_| bad(line))?;
                pane_of_pid.insert(pid, pane.trim().to_owned());
            }
            ("PS", _) => {
                let mut fields = value.splitn(4, char::is_whitespace);
                let pid = fields.next().and_then(|field| field.parse().ok());
                let _ppid = fields.next();
                let comm = fields.next();
                let args = fields.next().unwrap_or("");
                let (Some(pid), Some(comm)) = (pid, comm) else {
                    return Err(bad(line));
                };
                ps.push((pid, comm.to_owned(), args.trim().to_owned()));
            }
            _ => return Err(bad(line)),
        }
    }
    for (pid, comm, args) in ps {
        if let Some(pane) = pane_of_pid.get(&pid) {
            probe
                .processes
                .entry(pane.clone())
                .or_default()
                .push(PaneProcess { comm, args });
        }
    }
    Ok(probe)
}

/// A machine reached through the system `herdr`, `sh` and `ssh` binaries.
pub struct SystemHost {
    label: String,
    machine: Option<Machine>,
}

struct Machine {
    herdr_label: String,
    ssh_target: String,
}

impl SystemHost {
    /// The machine the command runs on.
    pub fn local() -> Self {
        Self {
            label: "local".to_owned(),
            machine: None,
        }
    }

    /// A saved herdr machine reached with `herdr --machine` and `ssh`.
    pub fn remote(label: impl Into<String>, ssh_target: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            machine: Some(Machine {
                herdr_label: label.clone(),
                ssh_target: ssh_target.into(),
            }),
            label,
        }
    }
}

fn run(command: &mut Command, stdin: Option<&str>) -> Result<String, AgentsError> {
    let rendered = format!("{command:?}");
    let spawn_error = |source| AgentsError::Spawn {
        command: rendered.clone(),
        source,
    };
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(spawn_error)?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes()).map_err(spawn_error)?;
    }
    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output().map_err(spawn_error)?;
    if !status.success() {
        return Err(AgentsError::CommandFailed {
            command: rendered,
            status: status.to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Quotes one word for a POSIX shell command line.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

impl Host for SystemHost {
    fn label(&self) -> &str {
        &self.label
    }

    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError> {
        let mut command = Command::new("herdr");
        if let Some(machine) = &self.machine {
            command.args(["--machine", &machine.herdr_label]);
        }
        run(command.args(args), None)
    }

    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError> {
        let args = request.script_args();
        let output = match &self.machine {
            None => run(
                Command::new("sh").args(["-s", "--"]).args(&args),
                Some(PROBE_SCRIPT),
            )?,
            Some(machine) => {
                let quoted: Vec<String> = args.iter().map(|arg| shell_quote(arg)).collect();
                let remote = format!("sh -s -- {}", quoted.join(" "));
                run(
                    Command::new("ssh")
                        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8"])
                        .arg(&machine.ssh_target)
                        .arg(remote),
                    Some(PROBE_SCRIPT),
                )?
            }
        };
        parse_probe(&output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_survives_single_quotes() {
        assert_eq!(shell_quote("a'b c"), r"'a'\''b c'");
    }
}
