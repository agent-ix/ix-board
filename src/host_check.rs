// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic host authentication and build-capacity checks (FR-050).

use crate::host_hygiene::{BuildHostMetrics, Eligibility, ProcessTable};
use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Stable result of one local authentication probe.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    /// Credential and authenticated probe succeeded.
    Ready,
    /// No credential was configured.
    Missing,
    /// The operating-system credential store could not be read.
    KeyringLocked,
    /// The service rejected the credential with HTTP 401.
    Unauthorized,
    /// The executable is absent from the process PATH.
    PathMissing,
    /// The probe failed without a more specific safe classification.
    Unavailable,
}

/// Authentication status for one service, without credential material.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthCheck {
    /// Service name (`gh` or `linear`).
    pub service: String,
    /// Stable authentication outcome.
    pub state: AuthState,
    /// Safe credential source label, if known.
    pub source: Option<String>,
}

/// One suspicious process running from the user's development directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DevProcessFinding {
    /// Process identifier.
    pub pid: u32,
    /// Executable basename.
    pub executable: String,
    /// Working directory.
    pub cwd: PathBuf,
}

/// Complete local host check report.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostCheckReport {
    /// Canonical host label.
    pub host: String,
    /// GitHub and Linear authentication probes.
    pub auth: Vec<AuthCheck>,
    /// Host's current locked-build and resource metrics.
    pub metrics: Option<BuildHostMetrics>,
    /// Admission verdict from the configured host policy.
    pub admission: Eligibility,
    /// Quire, quoin or ripgrep processes whose cwd is under `~/dev`.
    pub dev_processes: Vec<DevProcessFinding>,
    /// False when the system process table could not be read completely.
    pub process_table_complete: bool,
    /// Stable process status: 0 complete and healthy, 1 any failed/unknown item.
    pub exit_code: u8,
}

/// Classifies a service probe without retaining its output.
#[implements("FR-050-AC-1", "FR-050-AC-2")]
pub fn auth_state(
    credential_present: bool,
    keyring_locked: bool,
    command_found: bool,
    http_status: Option<u16>,
    probe_succeeded: bool,
) -> AuthState {
    if !command_found {
        AuthState::PathMissing
    } else if keyring_locked {
        AuthState::KeyringLocked
    } else if !credential_present {
        AuthState::Missing
    } else if http_status == Some(401) {
        AuthState::Unauthorized
    } else if probe_succeeded {
        AuthState::Ready
    } else {
        AuthState::Unavailable
    }
}

/// Finds credential-free executable paths using only the current non-login PATH.
#[implements("FR-050-AC-2")]
pub fn find_executable(path: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// Lists quire, quoin or ripgrep processes running from a directory under `dev_root`.
#[implements("FR-050-AC-4")]
pub fn dev_process_findings(
    dev_root: &Path,
    process_table: &ProcessTable,
) -> Vec<DevProcessFinding> {
    let root = canonical_or_original(dev_root);
    let mut findings = process_table
        .processes
        .iter()
        .filter_map(|process| {
            let cwd = process.cwd.as_deref()?;
            let canonical_cwd = canonical_or_original(cwd);
            if !canonical_cwd.starts_with(&root) {
                return None;
            }
            let executable = process
                .cmdline
                .split(|byte| byte.is_ascii_whitespace() || *byte == 0)
                .find(|word| !word.is_empty())
                .and_then(|word| std::str::from_utf8(word).ok())
                .and_then(|word| Path::new(word).file_name())
                .and_then(|name| name.to_str())?;
            matches!(executable, "quire" | "quoin" | "rg").then(|| DevProcessFinding {
                pid: process.pid,
                executable: executable.to_owned(),
                cwd: cwd.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    findings.sort_by(|left, right| {
        (left.pid, &left.executable, &left.cwd).cmp(&(right.pid, &right.executable, &right.cwd))
    });
    findings
}

fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Evaluates whether every required host check is healthy.
#[implements("FR-050-AC-1", "FR-050-AC-3", "FR-050-AC-4")]
pub fn host_exit_code(
    auth: &[AuthCheck],
    metrics: Option<&BuildHostMetrics>,
    admission: Eligibility,
    dev_processes: &[DevProcessFinding],
    process_table_complete: bool,
) -> u8 {
    if auth.len() != 2
        || auth.iter().any(|check| check.state != AuthState::Ready)
        || metrics.is_none()
        || admission != Eligibility::Eligible
        || !dev_processes.is_empty()
        || !process_table_complete
    {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthState, auth_state, dev_process_findings, find_executable, host_exit_code};
    use crate::host_hygiene::{BuildHostMetrics, Eligibility, ProcessEvidence, ProcessTable};
    use ix_trace_rs::trace;
    use std::path::PathBuf;

    #[test]
    #[trace("FR-050-AC-1", "FR-050-AC-2")]
    fn auth_probe_classifies_locked_missing_unauthorized_and_path_failures() {
        assert_eq!(
            auth_state(true, true, true, None, false),
            AuthState::KeyringLocked
        );
        assert_eq!(
            auth_state(false, false, true, None, false),
            AuthState::Missing
        );
        assert_eq!(
            auth_state(true, false, true, Some(401), false),
            AuthState::Unauthorized
        );
        assert_eq!(
            auth_state(true, false, false, None, false),
            AuthState::PathMissing
        );
        assert_eq!(
            auth_state(true, false, true, Some(200), true),
            AuthState::Ready
        );
        assert!(find_executable(std::ffi::OsStr::new("/nonexistent"), "gh").is_none());
    }

    #[test]
    #[trace("FR-050-AC-4")]
    fn injected_process_table_flags_only_selected_tools_inside_dev() {
        let table = ProcessTable {
            complete: true,
            processes: vec![
                ProcessEvidence {
                    pid: 10,
                    cwd: Some(PathBuf::from("/home/p/dev/project")),
                    cmdline: b"/usr/bin/quire validate".to_vec(),
                    ..Default::default()
                },
                ProcessEvidence {
                    pid: 11,
                    cwd: Some(PathBuf::from("/home/p/dev/worktree")),
                    cmdline: b"quoin matrix".to_vec(),
                    ..Default::default()
                },
                ProcessEvidence {
                    pid: 12,
                    cwd: Some(PathBuf::from("/tmp")),
                    cmdline: b"rg quire".to_vec(),
                    ..Default::default()
                },
                ProcessEvidence {
                    pid: 13,
                    cwd: Some(PathBuf::from("/home/p/dev")),
                    cmdline: b"cargo check".to_vec(),
                    ..Default::default()
                },
            ],
        };
        let findings = dev_process_findings(PathBuf::from("/home/p/dev").as_path(), &table);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].pid, 10);
        assert_eq!(findings[1].pid, 11);
    }

    #[test]
    #[trace("FR-050-AC-1", "FR-050-AC-3", "FR-050-AC-4")]
    fn host_exit_requires_two_healthy_auth_checks_metrics_and_no_dev_processes() {
        let auth = [
            super::AuthCheck {
                service: "gh".to_owned(),
                state: AuthState::Ready,
                source: None,
            },
            super::AuthCheck {
                service: "linear".to_owned(),
                state: AuthState::Ready,
                source: Some("credential_store".to_owned()),
            },
        ];
        let metrics = BuildHostMetrics {
            host: "terra".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            host_exit_code(&auth, Some(&metrics), Eligibility::Eligible, &[], true),
            0
        );
        assert_eq!(
            host_exit_code(&auth, None, Eligibility::Unknown, &[], false),
            1
        );
        assert_eq!(
            host_exit_code(
                &auth,
                Some(&metrics),
                Eligibility::Eligible,
                &[super::DevProcessFinding {
                    pid: 1,
                    executable: "rg".to_owned(),
                    cwd: PathBuf::from("/home/p/dev"),
                }],
                true
            ),
            1
        );
    }
}
