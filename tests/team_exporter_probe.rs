// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn exporter_handles_its_local_lifecycle_probe_without_config_or_credentials() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ix-team-exporter"))
        .arg("__agent-probe")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&ix_board::agents::ProbeEnvelope {
                protocol_version: ix_board::agents::PROBE_VERSION,
                request: ix_board::agents::ProbeRequest::default(),
            })
            .unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    ix_board::agents::parse_probe(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
}
