// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Flat clones and grouped checkouts share the configured-root discovery rules.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use ix_trace_rs::trace;
use std::path::PathBuf;
use std::process::Command;

#[test]
#[trace("FR-048-AC-11", "FR-048-AC-13")]
fn flat_clone_root_is_accepted_from_flag_and_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("dev");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    for checkout in ["alpha", "beta", "group/gamma", "too/deep/checkout"] {
        let checkout = root.join(checkout);
        std::fs::create_dir_all(checkout.join("target")).unwrap();
        let output = Command::new("git")
            .args(["init", "--quiet"])
            .arg(&checkout)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(checkout.join(".git").exists());
    }
    std::fs::create_dir_all(root.join("alpha/src/target")).unwrap();
    let expected = vec![
        root.join("alpha/target"),
        root.join("beta/target"),
        root.join("group/gamma/target"),
    ];

    for explicit_root in [true, false] {
        let settings = directory.path().join("settings.toml");
        let config = if explicit_root {
            toml::Value::Table(Default::default())
        } else {
            toml::Value::Table(toml::map::Map::from_iter([(
                "hygiene".to_owned(),
                toml::Value::Table(toml::map::Map::from_iter([(
                    "worktree_root".to_owned(),
                    toml::Value::String(root.to_str().unwrap().to_owned()),
                )])),
            )]))
        };
        std::fs::write(&settings, toml::to_string(&config).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_ix-board"));
        command
            .args([
                "--json",
                "hygiene",
                "check",
                "--host",
                "draco",
                "--free-floor-gb",
                "1",
            ])
            .env("IX_BOARD_CONFIG_PATH", &settings);
        if explicit_root {
            command.arg("--root").arg(&root);
        }
        let output = command.output().unwrap();
        assert!(
            matches!(output.status.code(), Some(0..=2)),
            "explicit_root={explicit_root}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let found = report["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|target| PathBuf::from(target["target"].as_str().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(found, expected, "explicit_root={explicit_root}: {report}");
    }
    assert!(root.join("alpha/src/target").is_dir());
    assert!(root.join("too/deep/checkout/target").is_dir());
}
