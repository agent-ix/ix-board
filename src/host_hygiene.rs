// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Build-host hygiene decisions and locked-build metrics parsing (FR-048).

use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Process paths and command evidence needed to decide whether a target is in use.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProcessEvidence {
    /// Process identifier.
    pub pid: u32,
    /// Current working directory, if readable.
    pub cwd: Option<PathBuf>,
    /// Process command line.
    pub cmdline: Vec<u8>,
    /// Process environment.
    pub environ: Vec<u8>,
    /// Paths held by open file descriptors.
    pub fds: Vec<PathBuf>,
    /// File paths in the process memory map.
    pub maps: Vec<PathBuf>,
}

/// A process snapshot with an explicit completeness bit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProcessTable {
    /// Processes discovered on the host.
    pub processes: Vec<ProcessEvidence>,
    /// False when any relevant process source could not be read completely.
    pub complete: bool,
}

/// Whether process evidence proves a directory is safe to clean.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UseState {
    /// At least one process references the directory.
    InUse,
    /// The complete process table contains no reference.
    Unused,
    /// No reference was found, but process evidence is incomplete.
    Unknown,
}

/// Configured policy for one host's build-output cleanup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostPolicy {
    /// Minimum free disk space in bytes.
    pub free_floor_bytes: u64,
    /// Minimum age of a target directory before cleanup.
    pub idle_age_secs: u64,
}

/// Planned cleanup for one target directory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanDecision {
    /// Target directory.
    pub target: PathBuf,
    /// Whether applying this decision would remove the target.
    pub eligible: bool,
    /// Stable machine-readable reason.
    pub reason: String,
}

/// One host's locked-build lease and resource metrics.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildHostMetrics {
    /// Host label from the exposition.
    pub host: String,
    /// One-minute load average.
    pub load1: Option<f64>,
    /// CPU core count used to normalize load.
    pub cores: Option<f64>,
    /// Total physical memory bytes.
    pub memory_total_bytes: Option<u64>,
    /// Available physical memory bytes.
    pub memory_available_bytes: Option<u64>,
    /// Total swap bytes.
    pub swap_total_bytes: Option<u64>,
    /// Free swap bytes.
    pub swap_free_bytes: Option<u64>,
    /// Number of queued build requests.
    pub queue_depth: u64,
    /// Running locked-build lease count.
    pub running_leases: u64,
}

/// A host's eligibility threshold for running a heavy gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildPolicy {
    /// Maximum raw one-minute load average.
    pub max_load1: f64,
    /// Maximum simultaneous heavy gates, including the proposed one.
    pub heavy_gate_slots: u64,
}

/// Result of evaluating one host for a heavy gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Eligibility {
    /// Current telemetry satisfies policy.
    Eligible,
    /// Telemetry is absent or incomplete.
    Unknown,
    /// Current telemetry exceeds policy.
    Ineligible,
}

/// Host-scoped hygiene observation used by `hygiene check` and `clean`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HygieneReport {
    /// Host label.
    pub host: String,
    /// Free disk bytes on the worktree filesystem, if available.
    pub free_bytes: Option<u64>,
    /// Configured free-space floor.
    pub free_floor_bytes: u64,
    /// Target directories examined.
    pub targets: Vec<CleanDecision>,
    /// Whether process inspection was complete.
    pub process_table_complete: bool,
    /// Unsupported host OS/platform condition.
    pub unsupported: bool,
}

impl HygieneReport {
    /// Stable command status: 0 clean, 1 findings, 2 missing evidence, 3 unsupported platform.
    pub fn exit_code(&self) -> u8 {
        if self.unsupported {
            return 3;
        }
        if self.free_bytes.is_none() || !self.process_table_complete {
            return 2;
        }
        if self
            .free_bytes
            .is_some_and(|free| free < self.free_floor_bytes)
            || self.targets.iter().any(|target| {
                matches!(
                    target.reason.as_str(),
                    "eligible" | "process_evidence_incomplete"
                )
            })
        {
            1
        } else {
            0
        }
    }
}

/// Collects process references using the host platform's native process table.
pub fn system_process_table() -> io::Result<ProcessTable> {
    #[cfg(target_os = "linux")]
    {
        linux_process_table(Path::new("/proc"))
    }
    #[cfg(target_os = "macos")]
    {
        macos_process_table()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Ok(ProcessTable {
            processes: Vec::new(),
            complete: false,
        })
    }
}

#[cfg(target_os = "linux")]
fn linux_process_table(proc_root: &Path) -> io::Result<ProcessTable> {
    use std::fs;
    let mut table = ProcessTable {
        processes: Vec::new(),
        complete: true,
    };
    for entry in fs::read_dir(proc_root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                table.complete = false;
                continue;
            }
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        let process_dir = entry.path();
        let read_optional = |name: &str| match fs::read(process_dir.join(name)) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => None,
        };
        let cwd = fs::read_link(process_dir.join("cwd")).ok();
        let cmdline = read_optional("cmdline").unwrap_or_default();
        let environ = read_optional("environ").unwrap_or_default();
        let mut fds = Vec::new();
        match fs::read_dir(process_dir.join("fd")) {
            Ok(entries) => {
                for fd in entries.flatten() {
                    if let Ok(path) = fs::read_link(fd.path()) {
                        fds.push(path);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => table.complete = false,
        }
        let mut maps = Vec::new();
        match fs::read_to_string(process_dir.join("maps")) {
            Ok(text) => {
                for line in text.lines() {
                    if let Some(path) = line
                        .split_whitespace()
                        .nth(5)
                        .filter(|part| part.starts_with('/'))
                    {
                        maps.push(PathBuf::from(path));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => table.complete = false,
        }
        table.processes.push(ProcessEvidence {
            pid,
            cwd,
            cmdline,
            environ,
            fds,
            maps,
        });
    }
    Ok(table)
}

#[cfg(target_os = "macos")]
fn macos_process_table() -> io::Result<ProcessTable> {
    let output = Command::new("lsof")
        .args(["-nP", "-Fpn", "-d", "cwd,txt,0-"])
        .output()?;
    if !output.status.success() {
        return Ok(ProcessTable {
            processes: Vec::new(),
            complete: false,
        });
    }
    let mut table = ProcessTable {
        processes: Vec::new(),
        complete: true,
    };
    let mut current: Option<ProcessEvidence> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(pid) = line
            .strip_prefix('p')
            .and_then(|pid| pid.parse::<u32>().ok())
        {
            if let Some(process) = current.take() {
                table.processes.push(process);
            }
            current = Some(ProcessEvidence {
                pid,
                ..Default::default()
            });
        } else if let Some(path) = line.strip_prefix('n').map(PathBuf::from)
            && let Some(process) = current.as_mut()
        {
            process.fds.push(path);
        }
    }
    if let Some(process) = current {
        table.processes.push(process);
    }
    Ok(table)
}

/// Reads available disk bytes through the platform `df` utility without a shell.
pub fn free_disk_bytes(path: &Path) -> io::Result<u64> {
    let output = Command::new("df").args(["-Pk"]).arg(path).output()?;
    if !output.status.success() {
        return Err(io::Error::other("df failed"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let available_kib = text
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::other("df output unavailable"))?;
    available_kib
        .checked_mul(1024)
        .ok_or_else(|| io::Error::other("df value overflow"))
}

fn path_references(bytes: &[u8], target: &Path) -> bool {
    let needle = target.as_os_str().as_encoded_bytes();
    bytes
        .split(|byte| *byte == 0 || byte.is_ascii_whitespace())
        .any(|part| {
            part.starts_with(needle)
                || part
                    .split(|byte| *byte == b'=')
                    .any(|value| value.starts_with(needle))
        })
}

/// Finds a process reference using cwd, command, environment, descriptors and maps.
#[implements("FR-048-AC-2")]
pub fn target_use(target: &Path, table: &ProcessTable) -> UseState {
    let used = table.processes.iter().any(|process| {
        process
            .cwd
            .as_deref()
            .is_some_and(|cwd| cwd.starts_with(target))
            || path_references(&process.cmdline, target)
            || path_references(&process.environ, target)
            || process.fds.iter().any(|path| path.starts_with(target))
            || process.maps.iter().any(|path| path.starts_with(target))
    });
    if used {
        UseState::InUse
    } else if table.complete {
        UseState::Unused
    } else {
        UseState::Unknown
    }
}

/// Returns a deterministic safe-clean decision for a single `target` directory.
#[implements("FR-048-AC-5", "FR-048-AC-6")]
pub fn clean_decision(
    target: &Path,
    worktree: &Path,
    now: SystemTime,
    modified: SystemTime,
    policy: &HostPolicy,
    processes: &ProcessTable,
) -> CleanDecision {
    let reason = if target.file_name().is_none_or(|name| name != "target") {
        "not_target_directory"
    } else if target.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some("dev_bak" | "dev_archive" | ".git")
        )
    }) {
        "protected_path"
    } else if !target.starts_with(worktree) {
        "outside_worktree"
    } else if now
        .duration_since(modified)
        .map_or(true, |age| age.as_secs() < policy.idle_age_secs)
    {
        "too_recent"
    } else {
        match target_use(target, processes) {
            UseState::InUse => "process_in_use",
            UseState::Unknown => "process_evidence_incomplete",
            UseState::Unused => "eligible",
        }
    };
    CleanDecision {
        target: target.to_owned(),
        eligible: reason == "eligible",
        reason: reason.to_owned(),
    }
}

/// Parses the Prometheus text exposition emitted by locked-build and node-exporter.
#[implements("FR-048-AC-7")]
pub fn parse_build_metrics(text: &str) -> BTreeMap<String, BuildHostMetrics> {
    let mut result = BTreeMap::<String, BuildHostMetrics>::new();
    let mut cpus = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let Some((sample, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(value) = value.trim().parse::<f64>() else {
            continue;
        };
        let (name, labels) = if let Some((name, rest)) = sample.split_once('{') {
            let Some(labels) = rest.strip_suffix('}') else {
                continue;
            };
            (name, labels)
        } else {
            (sample, "")
        };
        let Some(host) = label(labels, "host") else {
            continue;
        };
        let row = result
            .entry(host.to_owned())
            .or_insert_with(|| BuildHostMetrics {
                host: host.to_owned(),
                ..BuildHostMetrics::default()
            });
        if name == "node_cpu_seconds_total"
            && let Some(cpu) = label(labels, "cpu")
        {
            cpus.entry(host.to_owned())
                .or_default()
                .insert(cpu.to_owned());
        }
        match name {
            "node_load1" => row.load1 = Some(value),
            "node_memory_MemTotal_bytes" => row.memory_total_bytes = value_u64(value),
            "node_memory_MemAvailable_bytes" => row.memory_available_bytes = value_u64(value),
            "node_memory_SwapTotal_bytes" => row.swap_total_bytes = value_u64(value),
            "node_memory_SwapFree_bytes" => row.swap_free_bytes = value_u64(value),
            "ix_build_queue_depth" => {
                row.queue_depth = row
                    .queue_depth
                    .saturating_add(value_u64(value).unwrap_or(0))
            }
            "ix_build_lane_running" => {
                row.running_leases = row
                    .running_leases
                    .saturating_add(value_u64(value).unwrap_or(0))
            }
            _ => {}
        }
    }
    for (host, values) in cpus {
        if let Some(row) = result.get_mut(&host) {
            row.cores = u32::try_from(values.len()).ok().map(f64::from);
        }
    }
    result
}

fn value_u64(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= u64::MAX as f64)
        .then_some(value as u64)
}

fn label<'a>(labels: &'a str, wanted: &str) -> Option<&'a str> {
    labels.split(',').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == wanted).then(|| value.trim_matches('"'))
    })
}

/// Evaluates host telemetry against a configured heavy-gate policy.
#[implements("FR-048-AC-8")]
pub fn eligibility(metrics: &BuildHostMetrics, policy: BuildPolicy) -> Eligibility {
    let (Some(load1), Some(cores)) = (metrics.load1, metrics.cores) else {
        return Eligibility::Unknown;
    };
    if !load1.is_finite() || !cores.is_finite() || cores <= 0.0 {
        return Eligibility::Unknown;
    }
    if load1 >= policy.max_load1 || metrics.running_leases >= policy.heavy_gate_slots {
        Eligibility::Ineligible
    } else {
        Eligibility::Eligible
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_trace_rs::trace;

    #[test]
    #[trace("FR-048-AC-2")]
    fn every_linux_reference_kind_marks_the_target_in_use() {
        let target = Path::new("/work/wt/target");
        let values = [
            ProcessEvidence {
                cwd: Some(target.join("debug")),
                ..Default::default()
            },
            ProcessEvidence {
                cmdline: b"cargo\0/work/wt/target/debug".to_vec(),
                ..Default::default()
            },
            ProcessEvidence {
                environ: b"CARGO_TARGET_DIR=/work/wt/target".to_vec(),
                ..Default::default()
            },
            ProcessEvidence {
                fds: vec![target.join("debug/build")],
                ..Default::default()
            },
            ProcessEvidence {
                maps: vec![target.join("debug/lib.so")],
                ..Default::default()
            },
        ];
        for process in values {
            let table = ProcessTable {
                processes: vec![process],
                complete: true,
            };
            assert_eq!(target_use(target, &table), UseState::InUse);
        }
        assert_eq!(
            target_use(target, &ProcessTable::default()),
            UseState::Unknown
        );
    }

    #[test]
    #[trace("FR-048-AC-5", "FR-048-AC-6")]
    fn cleanup_requires_age_and_complete_absence_of_process_use() {
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let target = Path::new("/work/wt/target");
        let worktree = Path::new("/work/wt");
        let policy = HostPolicy {
            free_floor_bytes: 100,
            idle_age_secs: 3600,
        };
        let old = now - std::time::Duration::from_secs(3601);
        let decision = clean_decision(
            target,
            worktree,
            now,
            old,
            &policy,
            &ProcessTable {
                processes: Vec::new(),
                complete: true,
            },
        );
        assert_eq!(
            decision,
            CleanDecision {
                target: target.into(),
                eligible: true,
                reason: "eligible".into()
            }
        );
        let busy = ProcessTable {
            processes: vec![ProcessEvidence {
                cwd: Some(target.into()),
                ..Default::default()
            }],
            complete: true,
        };
        assert_eq!(
            clean_decision(target, worktree, now, old, &policy, &busy).reason,
            "process_in_use"
        );
        assert_eq!(
            clean_decision(
                target,
                worktree,
                now,
                old,
                &policy,
                &ProcessTable::default()
            )
            .reason,
            "process_evidence_incomplete"
        );
    }

    #[test]
    #[trace("FR-048-AC-7")]
    fn locked_build_metrics_aggregate_priorities_and_owner_leases() {
        let rows = parse_build_metrics(concat!(
            "node_load1{host=\"luna\"} 7.6\n",
            "node_cpu_seconds_total{host=\"luna\",cpu=\"0\",mode=\"idle\"} 1\n",
            "node_cpu_seconds_total{host=\"luna\",cpu=\"1\",mode=\"idle\"} 1\n",
            "node_memory_MemTotal_bytes{host=\"luna\"} 1000\n",
            "node_memory_MemAvailable_bytes{host=\"luna\"} 400\n",
            "node_memory_SwapTotal_bytes{host=\"luna\"} 200\n",
            "node_memory_SwapFree_bytes{host=\"luna\"} 10\n",
            "ix_build_queue_depth{host=\"luna\",priority=\"must-fix\"} 2\n",
            "ix_build_queue_depth{host=\"luna\",priority=\"periodic\"} 3\n",
            "ix_build_lane_running{host=\"luna\",lane=\"luna:team:coder\"} 1\n",
        ));
        let row = &rows["luna"];
        assert_eq!(row.load1, Some(7.6));
        assert_eq!(row.cores, Some(2.0));
        assert_eq!(row.memory_available_bytes, Some(400));
        assert_eq!(row.swap_free_bytes, Some(10));
        assert_eq!(row.queue_depth, 5);
        assert_eq!(row.running_leases, 1);
    }

    #[test]
    #[trace("FR-048-AC-8")]
    fn luna_policy_rejects_load_eight_and_any_existing_heavy_gate() {
        let policy = BuildPolicy {
            max_load1: 8.0,
            heavy_gate_slots: 1,
        };
        let mut metrics = BuildHostMetrics {
            load1: Some(7.9),
            cores: Some(8.0),
            ..Default::default()
        };
        assert_eq!(eligibility(&metrics, policy), Eligibility::Eligible);
        metrics.load1 = Some(8.0);
        assert_eq!(eligibility(&metrics, policy), Eligibility::Ineligible);
        metrics.load1 = Some(7.9);
        metrics.running_leases = 1;
        assert_eq!(eligibility(&metrics, policy), Eligibility::Ineligible);
    }

    #[test]
    #[trace("FR-048-AC-1")]
    fn check_exit_code_compares_free_bytes_with_policy_floor() {
        let report = |free_bytes| HygieneReport {
            host: "draco".into(),
            free_bytes,
            free_floor_bytes: 100,
            targets: Vec::new(),
            process_table_complete: true,
            unsupported: false,
        };
        assert_eq!(report(Some(99)).exit_code(), 1);
        assert_eq!(report(Some(100)).exit_code(), 0);
        assert_eq!(report(None).exit_code(), 2);
    }
}
