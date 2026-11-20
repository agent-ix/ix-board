// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Standalone Rust SSH/probe double; no network or credential-store access.

use std::{env, io::Read, path::Path, process::Command};

fn main() {
    let args: Vec<String> = env::args().collect();
    let executable = Path::new(&args[0]);
    if executable.file_name().unwrap() == "ssh" {
        assert_eq!(
            &args[1..7],
            [
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                "--",
                "host with 'quotes'"
            ]
        );
        assert_eq!(args.len(), 8, "remote command must remain one SSH argument");
        // Model the server's noninteractive shell, starting with no user bins.
        let status = Command::new("/bin/sh")
            .args(["-c", &args[7]])
            .env("PATH", env::var_os("PROBE_REMOTE_PATH").unwrap())
            .status()
            .unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }
    assert_eq!(&args[1..], ["__agent-probe"]);
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    assert_eq!(input, env::var("PROBE_EXPECTED_INPUT").unwrap());
    let location = executable.parent().unwrap();
    let now = if location.ends_with("preferred") {
        11
    } else if location.ends_with(".local/bin") {
        22
    } else if location.ends_with(".cargo/bin") {
        33
    } else {
        panic!("unexpected installation: {}", executable.display());
    };
    let response = env::var("PROBE_RESPONSE").unwrap();
    println!(
        "{}",
        response.replace("\"now\":0", &format!("\"now\":{now}"))
    );
}
