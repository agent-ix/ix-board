// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

#![cfg(unix)]

use std::{fs, process::Command};

use ix_board::agents::{
    AgentsError, Host, PROBE_VERSION, Probe, ProbeEnvelope, ProbeRequest, ProbeResponse, SystemHost,
};
use ix_trace_rs::trace;

#[test]
#[trace("FR-018-AC-5", "FR-018-AC-21")]
fn remote_probe_bootstraps_user_bins_preserves_path_and_refuses_skew() {
    let request = ProbeRequest {
        sessions: vec![],
        cwds: vec!["/work with 'quotes' and $(touch injected)".into()],
    };
    if let Ok(expected) = std::env::var("PROBE_EXPECTED_NOW") {
        let result = SystemHost::remote("fixture", "host with 'quotes'").probe(&request);
        if expected == "skew" {
            assert!(
                matches!(result, Err(AgentsError::ProbeFormat(reason)) if reason == "incompatible_protocol_version")
            );
        } else {
            assert_eq!(result.unwrap().now, expected.parse::<u64>().unwrap());
        }
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let fixture = root.path().join("transport");
    let compile = Command::new("rustc")
        .args(["--edition=2024", "tests/support/probe_transport.rs", "-o"])
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let tools = root.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::copy(&fixture, tools.join("ssh")).unwrap();
    // Spaces, quotes and shell expansion syntax must stay literal in HOME.
    let home = root.path().join("home with 'quotes' and $(touch injected)");
    let preferred = root.path().join("preferred");
    let local = home.join(".local/bin");
    let cargo = home.join(".cargo/bin");
    for directory in [&preferred, &local, &cargo] {
        fs::create_dir_all(directory).unwrap();
    }
    let input = serde_json::to_string(&ProbeEnvelope {
        protocol_version: PROBE_VERSION,
        request,
    })
    .unwrap();
    for (installations, expected, version) in [
        (vec![&cargo], "33", PROBE_VERSION),
        (vec![&local, &cargo], "22", PROBE_VERSION),
        (vec![&preferred, &local, &cargo], "11", PROBE_VERSION),
        (vec![&preferred, &local, &cargo], "skew", 1),
    ] {
        for directory in [&preferred, &local, &cargo] {
            let binary = directory.join("ix-board");
            if binary.exists() {
                fs::remove_file(binary).unwrap();
            }
        }
        for directory in installations {
            fs::copy(&fixture, directory.join("ix-board")).unwrap();
        }
        let response = serde_json::to_string(&ProbeResponse {
            protocol_version: version,
            probe: Probe::default(),
        })
        .unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "remote_probe_bootstraps_user_bins_preserves_path_and_refuses_skew",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", &home)
            .env("PATH", &tools)
            .env(
                "PROBE_REMOTE_PATH",
                format!("{}:/usr/bin:/bin", preferred.display()),
            )
            .env("PROBE_EXPECTED_INPUT", &input)
            .env("PROBE_RESPONSE", response)
            .env("PROBE_EXPECTED_NOW", expected)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "case {expected}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!root.path().join("injected").exists());
    }
}
