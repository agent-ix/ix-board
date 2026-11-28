// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Machine access for the detector (FR-018): the `Host` trait, the batched
//! probe request and its output parser, and the system implementation that
//! runs herdr locally and the shared Rust probe locally or over `ssh`.

use super::{
    AgentsError,
    harness::HarnessKind,
    lifecycle::{Availability, Lifecycle, WorkerObservation},
};
use ix_trace_rs::implements;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Current request/response protocol; incompatible peers are refused.
pub const PROBE_VERSION: u32 = 2;
/// Maximum encoded protocol input or output (16 MiB).
pub const MAX_PROTOCOL_BYTES: usize = 16 * 1024 * 1024;

/// A machine the detector can read: herdr state plus one batched file probe.
pub trait Host: Send + Sync {
    /// Reads redacted ix-build status; older adapters explicitly lack this source.
    fn build_status(&self) -> Result<String, AgentsError> {
        Err(AgentsError::ProbeFormat("build_status_unavailable".into()))
    }
    /// Machine label shown in output.
    fn label(&self) -> &str;

    /// Runs `herdr [--machine <label>] <args>` and returns its stdout.
    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError>;

    /// Reads transcripts, processes and git state in one batched round trip.
    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError>;
}

/// One agent session to locate on a machine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionQuery {
    /// Harness that owns the transcript layout.
    pub kind: HarnessKind,
    /// Session id from herdr.
    pub session_id: String,
}

/// Everything one probe reads.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequest {
    /// Sessions whose transcripts to locate.
    pub sessions: Vec<SessionQuery>,
    /// Working directories whose git ages to read.
    pub cwds: Vec<String>,
}

impl ProbeRequest {
    /// Refuses oversized or malformed requests before filesystem discovery.
    pub fn validate(&self) -> Result<(), AgentsError> {
        if self.sessions.len() > 128
            || self.cwds.len() > 128
            || self.sessions.iter().any(|s| {
                s.session_id.is_empty()
                    || s.session_id.len() > 512
                    || s.session_id.contains(['/', '\\', '\0'])
            })
            || self.cwds.iter().any(|p| p.len() > 4096 || p.contains('\0'))
        {
            return Err(AgentsError::ProbeFormat(
                "request_limit_or_invalid_path".into(),
            ));
        }
        Ok(())
    }
}

/// Versioned request for the hidden read-only probe.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeEnvelope {
    /// Protocol identity.
    pub protocol_version: u32,
    /// Machine-local sources to collect.
    pub request: ProbeRequest,
}

/// Files found for one session.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionFiles {
    /// Modification time (epoch seconds) of the main transcript.
    pub transcript_mtime: Option<u64>,
    /// Modification times of subagent or child-thread transcripts.
    pub child_mtimes: Vec<u64>,
    /// Shared streaming parent facts.
    pub parent_lifecycle: Lifecycle,
    /// Normalized descendants.
    pub workers: Vec<WorkerObservation>,
    /// Child discovery coverage.
    pub lifecycle_availability: Availability,
    /// Parent transcript coverage.
    pub transcript_availability: Availability,
}

/// One process running inside a herdr pane.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaneProcess {
    /// Executable name.
    pub comm: String,
    /// Recognized gate-wrapper signal; collection strips other arguments.
    pub args: String,
}

/// Git timestamps (epoch seconds) for a working directory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitTimes {
    /// Committer time of HEAD.
    pub last_commit: Option<u64>,
    /// Newest modification time among uncommitted files.
    pub uncommitted: Option<u64>,
}

/// Parsed probe output.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// The machine's clock, epoch seconds, when the probe ran.
    pub now: u64,
    /// Collection start in this host's clock domain.
    pub started_at: u64,
    /// Collection end in this host's clock domain.
    pub finished_at: u64,
    /// Process-source coverage.
    pub process_availability: Availability,
    /// Files per session id.
    pub sessions: BTreeMap<String, SessionFiles>,
    /// Processes per herdr pane id.
    pub processes: BTreeMap<String, Vec<PaneProcess>>,
    /// Git times per working directory.
    pub git: BTreeMap<String, GitTimes>,
    /// Coverage for each requested git directory.
    pub git_availability: BTreeMap<String, Availability>,
}

/// Versioned response from the shared Rust probe.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProbeResponse {
    /// Protocol identity.
    pub protocol_version: u32,
    /// Observations and independent coverage results.
    pub probe: Probe,
}

/// Decodes a bounded compatible JSON response; there is no heuristic fallback.
pub fn parse_probe(text: &str) -> Result<Probe, AgentsError> {
    if text.len() > MAX_PROTOCOL_BYTES {
        return Err(AgentsError::ProbeFormat("response_size_limit".into()));
    }
    let response: ProbeResponse =
        serde_json::from_str(text).map_err(|source| AgentsError::Decode {
            what: "probe response",
            source,
        })?;
    if response.protocol_version != PROBE_VERSION {
        return Err(AgentsError::ProbeFormat(
            "incompatible_protocol_version".into(),
        ));
    }
    response.probe.validate()?;
    Ok(response.probe)
}
impl Probe {
    /// Validates retained-state bounds even when the peer is independently installed.
    pub fn validate(&self) -> Result<(), AgentsError> {
        if self.sessions.len() > 128
            || self.git.len() > 128
            || self.processes.len() > 4096
            || self.sessions.values().any(|s| {
                s.workers.len() > 1024
                    || s.child_mtimes.len() > 1024
                    || s.parent_lifecycle.calls.len() > 1024
                    || s.parent_lifecycle.child_states.len() > 1024
            })
        {
            return Err(AgentsError::ProbeFormat("response_state_limit".into()));
        }
        Ok(())
    }
}

/// Runs the hidden command's bounded stdin/stdout protocol without Linear access.
pub fn serve_probe(input: impl Read, home: &std::path::Path) -> Result<String, AgentsError> {
    let mut bytes = Vec::new();
    input
        .take(u64::try_from(MAX_PROTOCOL_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AgentsError::Spawn {
            command: "read probe request".into(),
            source,
        })?;
    if bytes.len() > MAX_PROTOCOL_BYTES {
        return Err(AgentsError::ProbeFormat("request_size_limit".into()));
    }
    let envelope: ProbeEnvelope =
        serde_json::from_slice(&bytes).map_err(|source| AgentsError::Decode {
            what: "probe request",
            source,
        })?;
    if envelope.protocol_version != PROBE_VERSION {
        return Err(AgentsError::ProbeFormat(
            "incompatible_protocol_version".into(),
        ));
    }
    envelope.request.validate()?;
    let probe = super::collect::collect_probe(home, &envelope.request)?;
    let mut writer = BoundedOutput(Vec::new());
    serde_json::to_writer(
        &mut writer,
        &ProbeResponse {
            protocol_version: PROBE_VERSION,
            probe,
        },
    )
    .map_err(|source| AgentsError::Decode {
        what: "encode probe",
        source,
    })?;
    String::from_utf8(writer.0).map_err(|_| AgentsError::ProbeFormat("response_encoding".into()))
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_PROTOCOL_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("response_size_limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A machine reached through the system `herdr`, `ix-board` and `ssh` binaries.
pub struct SystemHost {
    label: String,
    machine: Option<Machine>,
    deadline: Instant,
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
            deadline: Instant::now() + CALL_TIMEOUT,
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
            deadline: Instant::now() + CALL_TIMEOUT,
        }
    }
}

impl SystemHost {
    #[implements("FR-051-AC-5")]
    fn build_status_with(
        &self,
        mut execute: impl FnMut(&mut Command, Duration) -> Result<String, AgentsError>,
    ) -> Result<String, AgentsError> {
        for binary in ["ix-build", "locked-build"] {
            let mut command = match &self.machine {
                None => {
                    let mut command = Command::new(binary);
                    command.arg("status");
                    command
                }
                Some(machine) => {
                    let mut command = Command::new("ssh");
                    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--"]).arg(&machine.ssh_target)
                        .arg(format!("exec env PATH=\"$PATH:$HOME/.local/bin:$HOME/.cargo/bin\" {binary} status"));
                    command
                }
            };
            let result = execute(
                &mut command,
                self.deadline.saturating_duration_since(Instant::now()),
            );
            let absent = match &result {
                Err(AgentsError::Spawn { source, .. }) => {
                    self.machine.is_none() && source.kind() == std::io::ErrorKind::NotFound
                }
                Err(AgentsError::CommandFailed {
                    exit_code: Some(127),
                    ..
                }) => self.machine.is_some(),
                _ => false,
            };
            if !absent || binary == "locked-build" {
                return result;
            }
        }
        unreachable!("the final fallback returns its result")
    }
}

/// Longest a single herdr or ssh call may take before it becomes an error.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe
                .by_ref()
                .take(u64::try_from(MAX_PROTOCOL_BYTES).unwrap_or(u64::MAX) + 1)
                .read_to_end(&mut bytes);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        }
        bytes
    })
}

#[implements("FR-018-AC-20", "FR-018-AC-21")]
pub(super) fn run(
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
    if stdout.len() > MAX_PROTOCOL_BYTES || stderr.len() > MAX_PROTOCOL_BYTES {
        return Err(AgentsError::ProbeFormat("command_output_limit".into()));
    }
    if !status.success() {
        return Err(AgentsError::CommandFailed {
            command: rendered,
            status: status.to_string(),
            exit_code: status.code(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

impl Host for SystemHost {
    #[implements("FR-051-AC-5")]
    fn build_status(&self) -> Result<String, AgentsError> {
        self.build_status_with(|command, timeout| run(command, None, timeout))
            .map_err(|_| AgentsError::ProbeFormat("build_status_unavailable".into()))
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn herdr(&self, args: &[&str]) -> Result<String, AgentsError> {
        let mut command = Command::new("herdr");
        if let Some(machine) = &self.machine {
            command.args(["--machine", &machine.herdr_label]);
        }
        run(
            command.args(args),
            None,
            self.deadline.saturating_duration_since(Instant::now()),
        )
    }

    fn probe(&self, request: &ProbeRequest) -> Result<Probe, AgentsError> {
        request.validate()?;
        let input = serde_json::to_string(&ProbeEnvelope {
            protocol_version: PROBE_VERSION,
            request: request.clone(),
        })
        .map_err(|source| AgentsError::Decode {
            what: "encode probe request",
            source,
        })?;
        let mut command;
        match &self.machine {
            None => {
                let executable = std::env::current_exe().map_err(|source| AgentsError::Spawn {
                    command: "locate current ix-board".into(),
                    source,
                })?;
                command = Command::new(executable);
                command.arg("__agent-probe");
            }
            Some(machine) => {
                command = Command::new("ssh");
                command
                    .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "--"])
                    .arg(&machine.ssh_target)
                    .arg(r#"exec env PATH="$PATH:$HOME/.local/bin:$HOME/.cargo/bin" ix-board __agent-probe"#);
            }
        }
        let output = run(
            &mut command,
            Some(&input),
            self.deadline.saturating_duration_since(Instant::now()),
        )?;
        parse_probe(&output)
    }
}

#[cfg(test)]
mod tests {
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-051-AC-5")]
    fn build_status_falls_back_only_when_the_new_binary_is_absent_on_local_and_ssh_hosts() {
        for host in [SystemHost::local(), SystemHost::remote("luna", "luna")] {
            let remote = host.machine.is_some();
            let mut calls = Vec::new();
            let result = host.build_status_with(|command, _| {
                calls.push(format!("{command:?}"));
                if calls.len() == 1 {
                    if remote {
                        Err(AgentsError::CommandFailed {
                            command: "ssh".into(),
                            status: "127".into(),
                            exit_code: Some(127),
                            stderr: "absent".into(),
                        })
                    } else {
                        Err(AgentsError::Spawn {
                            command: "ix-build".into(),
                            source: std::io::Error::from(std::io::ErrorKind::NotFound),
                        })
                    }
                } else {
                    Ok("legacy status".into())
                }
            });
            assert_eq!(result.unwrap(), "legacy status");
            assert_eq!(calls.len(), 2);
            assert!(
                calls[0].contains("ix-build status")
                    || (!remote && calls[0].contains("\"ix-build\""))
            );
            assert!(calls[1].contains("locked-build"));
            let mut calls = 0;
            let result = host.build_status_with(|_, _| {
                calls += 1;
                Err(AgentsError::CommandFailed {
                    command: "status".into(),
                    status: "2".into(),
                    exit_code: Some(2),
                    stderr: "invalid state".into(),
                })
            });
            assert!(result.is_err());
            assert_eq!(
                calls, 1,
                "a real status failure must not use another reader"
            );
            let mut calls = 0;
            assert_eq!(
                host.build_status_with(|_, _| {
                    calls += 1;
                    Ok("new status".into())
                })
                .unwrap(),
                "new status"
            );
            assert_eq!(calls, 1);
        }
    }

    #[test]
    #[trace("FR-018-AC-20")]
    fn response_limit_is_enforced_before_extending_the_output_buffer() {
        let mut output = BoundedOutput(vec![b'x'; MAX_PROTOCOL_BYTES - 1]);
        assert!(output.write_all(b"xx").is_err());
        assert_eq!(output.0.len(), MAX_PROTOCOL_BYTES - 1);
        output.write_all(b"x").unwrap();
        assert_eq!(output.0.len(), MAX_PROTOCOL_BYTES);
    }
    use super::*;

    #[test]
    #[trace("FR-018-AC-21")]
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
