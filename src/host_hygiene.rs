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
    /// Age of the directory in whole seconds at the observation time.
    pub age_secs: u64,
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
    /// Available bytes on the host root filesystem.
    pub disk_available_bytes: Option<u64>,
    /// Number of queued build requests.
    pub queue_depth: u64,
    /// Running locked-build lease count.
    pub running_leases: u64,
    /// Running lease counts by owner lane address.
    pub leases_by_lane: BTreeMap<String, u64>,
}

/// A host's eligibility threshold for running a heavy gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildPolicy {
    /// Maximum raw one-minute load average.
    pub max_load1: f64,
    /// Maximum simultaneous heavy gates, including the proposed one.
    pub heavy_gate_slots: u64,
}

/// Per-host policy values used by hygiene and build admission commands.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPolicyConfig {
    /// Free-space floor in GiB.
    pub free_floor_gb: Option<u64>,
    /// Maximum raw one-minute load average for a heavy gate.
    pub max_load1: Option<f64>,
    /// Maximum simultaneous heavy gates including the proposed gate.
    pub heavy_gate_slots: Option<u64>,
}

/// The `[hygiene]` policy table from ix-board settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HygieneConfig {
    /// Worktree directory on this host.
    pub worktree_root: Option<PathBuf>,
    /// Minimum target-directory idle age in minutes.
    pub idle_minutes: Option<u64>,
    /// Default free-space floor in GiB for hosts without an override.
    pub default_free_floor_gb: Option<u64>,
    /// Policy values keyed by Herdr host label.
    #[serde(default)]
    pub hosts: BTreeMap<String, HostPolicyConfig>,
}

impl Default for HygieneConfig {
    fn default() -> Self {
        Self {
            worktree_root: None,
            idle_minutes: Some(60),
            default_free_floor_gb: Some(100),
            hosts: BTreeMap::from([(
                "terra".to_owned(),
                HostPolicyConfig {
                    free_floor_gb: Some(150),
                    ..Default::default()
                },
            )]),
        }
    }
}

impl HygieneConfig {
    /// Parses and validates the `[hygiene]` settings table.
    #[implements("FR-048-AC-1", "FR-048-AC-8")]
    pub fn from_settings(settings: Option<&toml::Value>) -> Result<Self, String> {
        let Some(value) = settings.and_then(|settings| settings.get("hygiene")) else {
            return Ok(Self::default());
        };
        let overrides: Self = value
            .clone()
            .try_into()
            .map_err(|error| format!("invalid [hygiene] settings: {error}"))?;
        let mut config = Self {
            worktree_root: overrides.worktree_root,
            idle_minutes: overrides.idle_minutes.or(Some(60)),
            default_free_floor_gb: overrides.default_free_floor_gb.or(Some(100)),
            hosts: Self::default().hosts,
        };
        for (host, policy) in overrides.hosts {
            let key = host.to_lowercase();
            let merged = config
                .hosts
                .remove(&key)
                .map_or(policy.clone(), |defaults| HostPolicyConfig {
                    free_floor_gb: policy.free_floor_gb.or(defaults.free_floor_gb),
                    max_load1: policy.max_load1.or(defaults.max_load1),
                    heavy_gate_slots: policy.heavy_gate_slots.or(defaults.heavy_gate_slots),
                });
            config.hosts.insert(host, merged);
        }
        if config.idle_minutes.is_some_and(|minutes| minutes == 0) {
            return Err("hygiene.idle_minutes must be positive".to_owned());
        }
        let mut normalized = BTreeMap::new();
        for (host, policy) in config.hosts {
            if host.trim().is_empty() {
                return Err("hygiene host labels must not be empty".to_owned());
            }
            if policy.free_floor_gb.is_some_and(|value| value == 0)
                || policy.heavy_gate_slots.is_some_and(|value| value == 0)
                || policy
                    .max_load1
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
            {
                return Err(format!("invalid hygiene policy for host {host:?}"));
            }
            if normalized.insert(host.to_lowercase(), policy).is_some() {
                return Err("hygiene host labels must be unique ignoring case".to_owned());
            }
        }
        config.hosts = normalized;
        if config.default_free_floor_gb == Some(0) {
            return Err("hygiene.default_free_floor_gb must be positive".to_owned());
        }
        Ok(config)
    }

    /// Resolves a complete heavy-gate policy for one configured host.
    #[implements("FR-048-AC-8")]
    pub fn build_policy(&self, host: &str) -> Option<BuildPolicy> {
        let policy = self.hosts.get(&host.to_lowercase())?;
        Some(BuildPolicy {
            max_load1: policy.max_load1?,
            heavy_gate_slots: policy.heavy_gate_slots?,
        })
    }
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
    /// Worktree paths that `git worktree list` marks prunable.
    pub prunable_worktrees: Vec<PathBuf>,
    /// Build processes under the worktree root without a Herdr pane association.
    pub orphan_build_processes: Vec<u32>,
    /// Whether process inspection was complete.
    pub process_table_complete: bool,
    /// Unsupported host OS/platform condition.
    pub unsupported: bool,
}

impl HygieneReport {
    /// Stable command status: 0 clean, 1 findings, 2 missing evidence, 3 unsupported platform.
    #[implements("FR-048-AC-1", "FR-048-AC-3")]
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
            || !self.prunable_worktrees.is_empty()
            || !self.orphan_build_processes.is_empty()
        {
            1
        } else {
            0
        }
    }
}

/// Collects process references using the host platform's native process table.
#[implements("FR-048-AC-2", "FR-048-AC-3")]
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
#[implements("FR-048-AC-2")]
fn linux_process_table(proc_root: &Path) -> io::Result<ProcessTable> {
    const MAX_PROCESSES: usize = 50_000;
    const MAX_PROCESS_PATHS: usize = 16_384;
    const MAX_REFERENCES: usize = 1_000_000;
    use std::fs;
    let mut table = ProcessTable {
        processes: Vec::new(),
        complete: true,
    };
    let mut references = 0_usize;
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
        if table.processes.len() >= MAX_PROCESSES {
            table.complete = false;
            break;
        }
        let process_dir = entry.path();
        let mut read_optional = |name: &str| match fs::read(process_dir.join(name)) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => {
                table.complete = false;
                None
            }
        };
        let cwd = match fs::read_link(process_dir.join("cwd")) {
            Ok(path) => Some(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => {
                table.complete = false;
                None
            }
        };
        let cmdline = read_optional("cmdline").unwrap_or_default();
        let environ = read_optional("environ").unwrap_or_default();
        let mut fds = Vec::new();
        match fs::read_dir(process_dir.join("fd")) {
            Ok(entries) => {
                for fd in entries {
                    match fd {
                        Ok(fd) => match fs::read_link(fd.path()) {
                            Ok(path) => fds.push(path),
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                            Err(_) => table.complete = false,
                        },
                        Err(_) => table.complete = false,
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
                    let path = line
                        .split_whitespace()
                        .skip(5)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if path.starts_with('/') {
                        maps.push(PathBuf::from(path));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => table.complete = false,
        }
        let process_references = fds.len().saturating_add(maps.len());
        if process_references > MAX_PROCESS_PATHS
            || references.saturating_add(process_references) > MAX_REFERENCES
        {
            table.complete = false;
            break;
        }
        references += process_references;
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
#[implements("FR-048-AC-3")]
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
    let process_names = Command::new("ps").args(["-axo", "pid=,comm="]).output()?;
    let mut environments = BTreeMap::new();
    for line in String::from_utf8_lossy(&process_names.stdout).lines() {
        let Some((pid, command)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        if is_build_command(command.trim().as_bytes()) {
            match Command::new("ps")
                .args(["eww", "-p", &pid.to_string(), "-o", "command="])
                .output()
            {
                Ok(output) if output.status.success() && output.stdout.len() <= 1_048_576 => {
                    environments.insert(pid, output.stdout);
                }
                _ => {}
            }
        }
    }
    Ok(parse_macos_process_output(
        &output.stdout,
        &process_names.stdout,
        &environments,
        output.status.success() && process_names.status.success(),
    ))
}

#[cfg(target_os = "macos")]
fn is_build_command(command: &[u8]) -> bool {
    command
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .and_then(|command| std::str::from_utf8(command).ok())
        .and_then(|command| Path::new(command).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "cargo" | "rustc"))
}

#[cfg(target_os = "macos")]
#[implements("FR-048-AC-3")]
fn parse_macos_process_output(
    lsof_output: &[u8],
    process_names: &[u8],
    environments: &BTreeMap<u32, Vec<u8>>,
    sources_complete: bool,
) -> ProcessTable {
    let mut table = ProcessTable {
        processes: Vec::new(),
        complete: sources_complete,
    };
    let commands = String::from_utf8_lossy(process_names)
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(char::is_whitespace)?;
            Some((
                pid.trim().parse::<u32>().ok()?,
                command.trim().as_bytes().to_vec(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut current: Option<ProcessEvidence> = None;
    for line in String::from_utf8_lossy(lsof_output).lines() {
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
    for process in &mut table.processes {
        process.cmdline = commands.get(&process.pid).cloned().unwrap_or_default();
        if process.cmdline.is_empty() {
            table.complete = false;
        }
        if is_build_command(&process.cmdline) {
            if let Some(environment) = environments.get(&process.pid) {
                process.environ = environment.clone();
            } else {
                table.complete = false;
            }
        }
    }
    table
}

/// Reads available disk bytes through the platform `df` utility without a shell.
#[implements("FR-048-AC-1")]
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
        age_secs: now.duration_since(modified).map_or(0, |age| age.as_secs()),
        eligible: reason == "eligible",
        reason: reason.to_owned(),
    }
}

/// Applies a previously checked cleanup decision, preserving dry-run behavior by default.
#[implements("FR-048-AC-5")]
pub fn apply_clean_decision(decision: &CleanDecision, apply: bool) -> io::Result<CleanDecision> {
    let mut result = decision.clone();
    if apply && decision.eligible {
        std::fs::remove_dir_all(&decision.target)?;
        result.reason = "removed".to_owned();
    }
    Ok(result)
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
            "node_filesystem_avail_bytes"
                if label(labels, "mountpoint") == Some("/")
                    && !matches!(label(labels, "fstype"), Some("tmpfs" | "overlay")) =>
            {
                row.disk_available_bytes = value_u64(value)
            }
            "ix_build_queue_depth" => {
                row.queue_depth = row
                    .queue_depth
                    .saturating_add(value_u64(value).unwrap_or(0))
            }
            "ix_build_lane_running" => {
                let count = value_u64(value).unwrap_or(0);
                row.running_leases = row.running_leases.saturating_add(count);
                if let Some(lane) = label(labels, "lane") {
                    let entry = row.leases_by_lane.entry(lane.to_owned()).or_default();
                    *entry = entry.saturating_add(count);
                }
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

    #[cfg(target_os = "macos")]
    #[test]
    #[trace("FR-048-AC-3")]
    fn macos_process_output_retains_build_commands_and_pane_environment() {
        let environments = BTreeMap::from([
            (123, b"HERDR_PANE_ID=tab-123\0".to_vec()),
            (124, b"OTHER=value\0".to_vec()),
        ]);
        let table = parse_macos_process_output(
            b"p123\nn/worktrees/active/target/debug\np124\nn/worktrees/old/target/debug\n",
            b"123 cargo\n124 cargo\n",
            &environments,
            true,
        );
        assert!(table.complete);
        assert_eq!(table.processes.len(), 2);
        assert_eq!(table.processes[0].cmdline, b"cargo");
        assert_eq!(table.processes[0].environ, b"HERDR_PANE_ID=tab-123\0");
        assert_eq!(table.processes[1].cmdline, b"cargo");
        assert!(
            !table.processes[1]
                .environ
                .windows(b"HERDR_PANE_ID=".len())
                .any(|window| window == b"HERDR_PANE_ID=")
        );
    }

    #[test]
    #[trace("FR-048-AC-1", "FR-048-AC-8")]
    fn host_hygiene_policy_reads_expected_values_from_settings() {
        let defaults = HygieneConfig::from_settings(None).expect("default policy");
        assert_eq!(defaults.idle_minutes, Some(60));
        assert_eq!(defaults.default_free_floor_gb, Some(100));
        assert_eq!(defaults.hosts["terra"].free_floor_gb, Some(150));

        let settings = toml::from_str::<toml::Value>(
            r#"
            [hygiene]
            worktree_root = "/srv/dev/worktrees"
            idle_minutes = 90

            [hygiene.hosts.terra]
            max_load1 = 32.0
            heavy_gate_slots = 2

            [hygiene.hosts.luna]
            free_floor_gb = 100
            max_load1 = 8.0
            heavy_gate_slots = 1
            "#,
        )
        .expect("valid settings");
        let config = HygieneConfig::from_settings(Some(&settings)).expect("valid hygiene config");
        assert_eq!(
            config.worktree_root.as_deref(),
            Some(Path::new("/srv/dev/worktrees"))
        );
        assert_eq!(config.idle_minutes, Some(90));
        assert_eq!(config.hosts["terra"].free_floor_gb, Some(150));
        assert_eq!(config.hosts["luna"].free_floor_gb, Some(100));
        assert_eq!(
            config.build_policy("LUNA"),
            Some(BuildPolicy {
                max_load1: 8.0,
                heavy_gate_slots: 1,
            })
        );
    }

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
                age_secs: 3601,
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
    #[trace("FR-048-AC-5", "FR-048-AC-6")]
    fn cleanup_plan_accepts_a_real_worktree_target_without_touching_source() {
        let root = tempfile::tempdir().expect("temp worktree root");
        let worktree = root.path().join("swm-1");
        let target = worktree.join("target");
        std::fs::create_dir_all(&target).expect("target dir");
        std::fs::write(worktree.join(".git"), "gitdir: ../.git/worktrees/swm-1\n")
            .expect("worktree metadata");
        std::fs::write(worktree.join("src.rs"), "source").expect("source file");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let old = now - std::time::Duration::from_secs(3601);
        let decision = clean_decision(
            &target,
            &worktree,
            now,
            old,
            &HostPolicy {
                free_floor_bytes: 100,
                idle_age_secs: 3600,
            },
            &ProcessTable {
                processes: Vec::new(),
                complete: true,
            },
        );
        assert!(decision.eligible);
        assert_eq!(decision.age_secs, 3601);
        assert!(worktree.join("src.rs").exists());
        assert!(worktree.join(".git").exists());
        let dry_run = apply_clean_decision(&decision, false).expect("dry run");
        assert_eq!(dry_run.reason, "eligible");
        assert!(target.exists());
        let applied = apply_clean_decision(&decision, true).expect("apply cleanup");
        assert_eq!(applied.reason, "removed");
        assert!(!target.exists());
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
            "node_filesystem_avail_bytes{host=\"luna\",mountpoint=\"/\",fstype=\"ext4\"} 700\n",
            "ix_build_queue_depth{host=\"luna\",priority=\"must-fix\"} 2\n",
            "ix_build_queue_depth{host=\"luna\",priority=\"periodic\"} 3\n",
            "ix_build_lane_running{host=\"luna\",lane=\"luna:team:coder\"} 1\n",
        ));
        let row = &rows["luna"];
        assert_eq!(row.load1, Some(7.6));
        assert_eq!(row.cores, Some(2.0));
        assert_eq!(row.memory_available_bytes, Some(400));
        assert_eq!(row.swap_free_bytes, Some(10));
        assert_eq!(row.disk_available_bytes, Some(700));
        assert_eq!(row.queue_depth, 5);
        assert_eq!(row.running_leases, 1);
        assert_eq!(row.leases_by_lane["luna:team:coder"], 1);
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
            prunable_worktrees: Vec::new(),
            orphan_build_processes: Vec::new(),
            process_table_complete: true,
            unsupported: false,
        };
        assert_eq!(report(Some(99)).exit_code(), 1);
        assert_eq!(report(Some(100)).exit_code(), 0);
        assert_eq!(report(None).exit_code(), 2);
    }
}
