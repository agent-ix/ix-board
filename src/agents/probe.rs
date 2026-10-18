// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Machine access for the detector (FR-018): the `Host` trait, the batched
//! probe request and its output parser, and the system implementation that
//! runs herdr locally and the probe script locally or over `ssh`.

use super::{AgentsError, harness::HarnessKind};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
                let mut fields = value.splitn(4, '\t');
                let (Some(pid), Some(pane), Some(comm)) =
                    (fields.next(), fields.next(), fields.next())
                else {
                    return Err(bad(line));
                };
                pid.parse::<u32>().map_err(|_| bad(line))?;
                probe
                    .processes
                    .entry(pane.to_owned())
                    .or_default()
                    .push(PaneProcess {
                        comm: comm.to_owned(),
                        args: fields.next().unwrap_or("").trim().to_owned(),
                    });
            }
            _ => return Err(bad(line)),
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

/// Longest a single herdr or ssh call may take before it becomes an error.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn run(
    command: &mut Command,
    stdin: Option<&str>,
    timeout: Duration,
) -> Result<String, AgentsError> {
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
    let readers = (child.stdout.take(), child.stderr.take());
    let (out_reader, err_reader) = (drain(readers.0), drain(readers.1));
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A child that exits early closes the pipe; its status reports why.
        let _ = pipe.write_all(text.as_bytes());
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(spawn_error)? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AgentsError::Timeout {
                    command: rendered,
                    secs: timeout.as_secs(),
                });
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
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
        run(command.args(args), None, CALL_TIMEOUT)
    }

    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError> {
        let args = request.script_args();
        let output = match &self.machine {
            None => run(
                Command::new("sh").args(["-s", "--"]).args(&args),
                Some(PROBE_SCRIPT),
                CALL_TIMEOUT,
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
                    CALL_TIMEOUT,
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

    #[test]
    fn a_hung_command_is_killed_and_reported_as_a_timeout() {
        let started = Instant::now();
        let result = run(
            Command::new("sleep").arg("30"),
            None,
            Duration::from_millis(150),
        );
        assert!(
            matches!(result, Err(AgentsError::Timeout { .. })),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
