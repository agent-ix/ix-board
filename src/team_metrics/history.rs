// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::{Error, EventPage};
use std::time::Duration;
#[cfg(unix)]
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    thread,
    time::Instant,
};

/// History source, injectable for deterministic replay tests.
pub trait History {
    /// Reads after an exclusive cursor; reading must not consume inbox entries.
    fn page(&self, after: &str) -> Result<EventPage, Error>;
}
/// Installed agent-msg history reader; credentials follow the existing relay config.
pub struct CommandHistory;
impl History for CommandHistory {
    fn page(&self, after: &str) -> Result<EventPage, Error> {
        let output = read_history(
            "agent-msg",
            &["work", "events", "--after", after, "--limit", "500"],
            Duration::from_secs(10),
        )?;
        let page: EventPage = serde_json::from_slice(&output)?;
        if page.version != 1 || page.events.len() > 500 {
            return Err(Error::Invalid("unsupported/oversize history page"));
        }
        Ok(page)
    }
}

#[cfg(unix)]
struct SourceChild(Child);
#[cfg(unix)]
impl Drop for SourceChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs a read-only source command with bounded time and output, without a shell.
/// Nonblocking pipe reads avoid waiting on a descendant that retains stdout.
#[cfg(unix)]
pub fn read_history(program: &str, args: &[&str], timeout: Duration) -> Result<Vec<u8>, Error> {
    let mut command = Command::new(program);
    command.args(args);
    read_command(command, timeout)
}
#[cfg(unix)]
fn read_command(mut command: Command, timeout: Duration) -> Result<Vec<u8>, Error> {
    let name = command.get_program().to_string_lossy().into_owned();
    let mut child = SourceChild(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut stdout = child
        .0
        .stdout
        .take()
        .ok_or(Error::Invalid("source stdout missing"))?;
    let flags = rustix::fs::fcntl_getfl(&stdout).map_err(std::io::Error::from)?;
    rustix::fs::fcntl_setfl(&stdout, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(std::io::Error::from)?;
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut closed = false;
    loop {
        if start.elapsed() >= timeout {
            return Err(Error::Source(format!("{name} timed out")));
        }
        let mut progress = false;
        if !closed {
            let mut chunk = [0; 8192];
            match stdout.read(&mut chunk) {
                Ok(0) => closed = true,
                Ok(n) => {
                    if bytes.len() + n > 8 * 1024 * 1024 {
                        return Err(Error::Source(format!("{name} output exceeded 8MiB")));
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    progress = true;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(status) = child.0.try_wait()?
            && closed
        {
            return if status.success() {
                Ok(bytes)
            } else {
                Err(Error::Source(format!("{name} exited {status}")))
            };
        }
        if !progress {
            thread::sleep(Duration::from_millis(5));
        }
    }
}
/// Native Windows collection requires a Unix/WSL host; other ix-board commands remain portable.
#[cfg(not(unix))]
pub fn read_history(_program: &str, _args: &[&str], _timeout: Duration) -> Result<Vec<u8>, Error> {
    Err(Error::Invalid(
        "team collection requires a Unix host or WSL",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_descendant_retaining_stdout_cannot_extend_the_source_deadline() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "team_metrics::history::tests::holding_stdout",
            "--nocapture",
        ]);
        command.env("IX_TEAM_TEST_PIPE_ROLE", "parent");
        let start = Instant::now();
        let result = read_command(command, Duration::from_millis(100));
        assert!(result.unwrap_err().to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    #[ignore = "subprocess fixture for pipe inheritance test"]
    #[allow(clippy::zombie_processes)] // The inherited pipe is this fixture's behavior.
    fn holding_stdout() {
        match std::env::var("IX_TEAM_TEST_PIPE_ROLE").as_deref() {
            Ok("parent") => {
                let mut child = Command::new(std::env::current_exe().unwrap());
                child.args([
                    "--ignored",
                    "--exact",
                    "team_metrics::history::tests::holding_stdout",
                    "--nocapture",
                ]);
                child.env("IX_TEAM_TEST_PIPE_ROLE", "descendant");
                // Intentional: this fixture exits before the child closes stdout.
                let _child = child.spawn().unwrap();
            }
            Ok("descendant") => thread::sleep(Duration::from_secs(2)),
            _ => {}
        }
    }
}
