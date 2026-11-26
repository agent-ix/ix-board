// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Build-host hygiene decisions and locked-build metrics parsing (FR-048).

use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

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

/// One filesystem mount observed from the operating system mount table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountEntry {
    /// Mount identifier or device source reported by the host.
    pub source: String,
    /// Mount location.
    pub mount_point: PathBuf,
    /// Filesystem type.
    pub fs_type: String,
}

/// Whether a path can be scanned without crossing an unexpected mount.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountSafety {
    /// Path and anchor share one filesystem and the path contains no nested mount.
    Safe,
    /// The path is on another filesystem or contains a nested mount.
    Boundary,
    /// The mount table is absent, malformed or insufficient to identify the path.
    Unknown,
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
    /// Latest explicit sample timestamp, in Unix seconds, when supplied.
    pub observed_at_epoch_secs: Option<u64>,
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
    /// Whether the exposition contained a queue depth sample for this host.
    pub queue_depth_available: bool,
    /// Running locked-build lease count.
    pub running_leases: u64,
    /// Whether the exposition contained a running lease sample for this host.
    pub running_leases_available: bool,
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
    /// Maximum filesystem entries examined while deriving target idle age.
    pub mtime_scan_entries: Option<usize>,
    /// Maximum duration in milliseconds spent deriving target idle age.
    pub mtime_scan_duration_ms: Option<u64>,
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
            mtime_scan_entries: Some(100_000),
            mtime_scan_duration_ms: Some(2_000),
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
            mtime_scan_entries: overrides.mtime_scan_entries.or(Some(100_000)),
            mtime_scan_duration_ms: overrides.mtime_scan_duration_ms.or(Some(2_000)),
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
        if config.mtime_scan_entries == Some(0) || config.mtime_scan_duration_ms == Some(0) {
            return Err("hygiene mtime scan limits must be positive".to_owned());
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
    /// Whether the mount table could be read and interpreted.
    pub mount_table_complete: bool,
    /// Unsupported host OS/platform condition.
    pub unsupported: bool,
}

impl HygieneReport {
    /// Stable command status: 0 clean, 1 findings, 2 missing evidence, 3 unsupported platform.
    #[implements("FR-048-AC-1", "FR-048-AC-3", "FR-048-AC-10")]
    pub fn exit_code(&self) -> u8 {
        if self.unsupported {
            return 3;
        }
        if self.free_bytes.is_none()
            || !self.process_table_complete
            || !self.mount_table_complete
            || self
                .targets
                .iter()
                .any(|target| target.reason == "mount_evidence_unknown")
        {
            return 2;
        }
        if self
            .free_bytes
            .is_some_and(|free| free < self.free_floor_bytes)
            || self.targets.iter().any(|target| {
                matches!(
                    target.reason.as_str(),
                    "eligible" | "process_evidence_incomplete" | "mount_boundary"
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

/// Reads the platform mount table into a portable source/mountpoint/type form.
#[implements("FR-048-AC-10")]
pub fn system_mount_table() -> io::Result<Vec<MountEntry>> {
    #[cfg(target_os = "linux")]
    let text = std::fs::read_to_string("/proc/self/mountinfo")?;
    #[cfg(target_os = "macos")]
    let text = {
        let output = Command::new("mount").arg("-p").output()?;
        if !output.status.success() {
            return Err(io::Error::other("mount table command failed"));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| io::Error::other("mount table is not UTF-8"))?
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err(io::Error::other("mount table unsupported"));
    parse_mount_table(&text)
}

/// Parses Linux mountinfo or BSD `mount -p` rows.
#[implements("FR-048-AC-10")]
pub fn parse_mount_table(text: &str) -> io::Result<Vec<MountEntry>> {
    let mut entries = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields = if let Some((before, after)) = line.split_once(" - ") {
            let left = before.split_whitespace().collect::<Vec<_>>();
            let right = after.split_whitespace().collect::<Vec<_>>();
            if left.len() < 5 || right.len() < 2 {
                continue;
            }
            (
                left[0].to_owned(),
                unescape_mount_field(left[4]),
                right[0].to_owned(),
            )
        } else {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 3 {
                continue;
            }
            (
                unescape_mount_field(fields[0]),
                unescape_mount_field(fields[1]),
                fields[2].to_owned(),
            )
        };
        entries.push(MountEntry {
            source: fields.0,
            mount_point: PathBuf::from(fields.1),
            fs_type: fields.2,
        });
    }
    if entries.is_empty() {
        return Err(io::Error::other("mount table contains no usable entries"));
    }
    entries.sort_by(|left, right| left.mount_point.cmp(&right.mount_point));
    Ok(entries)
}

fn unescape_mount_field(field: &str) -> String {
    field
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

/// Checks a candidate path against an injected or system mount table.
#[implements("FR-048-AC-10")]
pub fn mount_safety(path: &Path, anchor: &Path, mounts: &[MountEntry]) -> MountSafety {
    let path = canonical_or_original(path);
    let anchor = canonical_or_original(anchor);
    let Some(path_mount) = mount_for(&path, mounts) else {
        return MountSafety::Unknown;
    };
    let Some(anchor_mount) = mount_for(&anchor, mounts) else {
        return MountSafety::Unknown;
    };
    if path_mount.source != anchor_mount.source || path_mount.fs_type != anchor_mount.fs_type {
        return MountSafety::Boundary;
    }
    if mounts
        .iter()
        .any(|entry| entry.mount_point != path && entry.mount_point.starts_with(&path))
    {
        return MountSafety::Boundary;
    }
    MountSafety::Safe
}

fn mount_for<'a>(path: &Path, mounts: &'a [MountEntry]) -> Option<&'a MountEntry> {
    mounts
        .iter()
        .filter(|entry| path.starts_with(&entry.mount_point))
        .max_by_key(|entry| entry.mount_point.components().count())
}

/// Reports whether a mount table has an entry covering a path.
#[implements("FR-048-AC-10")]
pub fn mount_table_covers(path: &Path, mounts: &[MountEntry]) -> bool {
    mount_for(&canonical_or_original(path), mounts).is_some()
}

/// Reports whether two paths resolve to the same source and filesystem type.
#[implements("FR-048-AC-10")]
pub fn mount_filesystem_matches(left: &Path, right: &Path, mounts: &[MountEntry]) -> MountSafety {
    let Some(left_mount) = mount_for(&canonical_or_original(left), mounts) else {
        return MountSafety::Unknown;
    };
    let Some(right_mount) = mount_for(&canonical_or_original(right), mounts) else {
        return MountSafety::Unknown;
    };
    if left_mount.source == right_mount.source && left_mount.fs_type == right_mount.fs_type {
        MountSafety::Safe
    } else {
        MountSafety::Boundary
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
        .args(["-nP", "-Ffpn", "-d", "cwd,txt,mem,0-"])
        .output()?;
    if !output.status.success() {
        return Ok(ProcessTable {
            processes: Vec::new(),
            complete: false,
        });
    }
    let process_names = Command::new("ps").args(["-axo", "pid=,args="]).output()?;
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
        macos_process_evidence_complete(
            current_user_is_root(),
            output.status.success(),
            process_names.status.success(),
        ),
    ))
}

#[cfg(target_os = "macos")]
#[implements("FR-048-AC-3")]
fn current_user_is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|output| output.status.success() && output.stdout.trim_ascii() == b"0")
}

#[cfg(target_os = "macos")]
#[implements("FR-048-AC-3")]
fn macos_process_evidence_complete(is_root: bool, lsof_ok: bool, ps_ok: bool) -> bool {
    is_root && lsof_ok && ps_ok
}

#[cfg(target_os = "macos")]
#[implements("FR-048-AC-3")]
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
    let mut current_descriptor: Option<String> = None;
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
            current_descriptor = None;
        } else if let Some(descriptor) = line.strip_prefix('f') {
            current_descriptor = Some(descriptor.to_owned());
        } else if let Some(path) = line.strip_prefix('n').map(PathBuf::from)
            && let Some(process) = current.as_mut()
        {
            if current_descriptor.as_deref() == Some("cwd") {
                process.cwd = Some(path);
            } else {
                process.fds.push(path);
            }
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

/// Finds the newest modification time anywhere below a target directory.
#[implements("FR-048-AC-5")]
pub fn newest_mtime_under(
    path: &Path,
    max_entries: usize,
    max_duration: Duration,
) -> io::Result<SystemTime> {
    let started = Instant::now();
    let limit_error = || io::Error::new(io::ErrorKind::TimedOut, "mtime scan limit exceeded");
    if max_entries == 0 || max_duration.is_zero() {
        return Err(limit_error());
    }
    let mut newest = std::fs::symlink_metadata(path)?.modified()?;
    let mut pending = vec![path.to_owned()];
    let mut entries_seen = 0_usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            if started.elapsed() >= max_duration {
                return Err(limit_error());
            }
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > max_entries {
                return Err(limit_error());
            }
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            newest = newest.max(metadata.modified()?);
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                pending.push(entry.path());
            }
        }
    }
    if started.elapsed() > max_duration {
        return Err(limit_error());
    }
    Ok(newest)
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
    target_use_in_worktree(target, target.parent().unwrap_or(target), table)
}

/// Canonicalizes a configured worktree root before scanning it.
#[implements("FR-048-AC-1", "FR-048-AC-5")]
pub fn canonicalize_worktree_root(root: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(root)
}

fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

fn path_starts_with_any(path: &Path, roots: &[PathBuf]) -> bool {
    path.starts_with(&roots[0]) || roots.get(1).is_some_and(|root| path.starts_with(root))
}

fn bytes_reference_any(bytes: &[u8], paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| path_references(bytes, path))
}

/// Finds process references to a target or any cwd within its owning worktree.
#[implements("FR-048-AC-2")]
pub fn target_use_in_worktree(target: &Path, worktree: &Path, table: &ProcessTable) -> UseState {
    let target_paths = [target.to_owned(), canonical_or_original(target)];
    let worktree_paths = [worktree.to_owned(), canonical_or_original(worktree)];
    let used = table.processes.iter().any(|process| {
        process.cwd.as_deref().is_some_and(|cwd| {
            path_starts_with_any(cwd, &worktree_paths)
                || path_starts_with_any(&canonical_or_original(cwd), &worktree_paths)
        }) || bytes_reference_any(&process.cmdline, &target_paths)
            || bytes_reference_any(&process.cmdline, &worktree_paths)
            || bytes_reference_any(&process.environ, &target_paths)
            || bytes_reference_any(&process.environ, &worktree_paths)
            || process.fds.iter().any(|path| {
                path_starts_with_any(path, &target_paths)
                    || path_starts_with_any(&canonical_or_original(path), &target_paths)
            })
            || process.maps.iter().any(|path| {
                path_starts_with_any(path, &target_paths)
                    || path_starts_with_any(&canonical_or_original(path), &target_paths)
            })
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
    let canonical_target = canonical_or_original(target);
    let canonical_worktree = canonical_or_original(worktree);
    let reason = if canonical_target
        .file_name()
        .is_none_or(|name| name != "target")
    {
        "not_target_directory"
    } else if target
        .components()
        .chain(canonical_target.components())
        .any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("dev_bak" | "dev_archive" | ".git")
            )
        })
    {
        "protected_path"
    } else if !canonical_target.starts_with(&canonical_worktree) {
        "outside_worktree"
    } else if now
        .duration_since(modified)
        .map_or(true, |age| age.as_secs() < policy.idle_age_secs)
    {
        "too_recent"
    } else {
        match target_use_in_worktree(&canonical_target, &canonical_worktree, processes) {
            UseState::InUse => "process_in_use",
            UseState::Unknown => "process_evidence_incomplete",
            UseState::Unused => "eligible",
        }
    };
    CleanDecision {
        target: canonical_target,
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

/// Rechecks a planned eligible target with fresh age and process evidence before deletion.
#[implements("FR-048-AC-5", "FR-048-AC-6")]
pub fn recheck_clean_decision(
    planned: &CleanDecision,
    worktree: &Path,
    now: SystemTime,
    newest_modified: SystemTime,
    policy: &HostPolicy,
    processes: &ProcessTable,
) -> CleanDecision {
    if !planned.eligible {
        return planned.clone();
    }
    clean_decision(
        &planned.target,
        worktree,
        now,
        newest_modified,
        policy,
        processes,
    )
}

/// Parses the Prometheus text exposition emitted by locked-build and node-exporter.
#[implements("FR-048-AC-7", "FR-050-AC-3")]
pub fn parse_build_metrics(text: &str) -> BTreeMap<String, BuildHostMetrics> {
    let mut result = BTreeMap::<String, BuildHostMetrics>::new();
    let mut cpus = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut busy_leases = BTreeMap::<String, u64>::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let mut fields = line.split_whitespace();
        let Some(sample) = fields.next() else {
            continue;
        };
        let Some(raw_value) = fields.next() else {
            continue;
        };
        let raw_timestamp = fields.next();
        let Ok(value) = raw_value.parse::<f64>() else {
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
        let host = normalize_host_label(host);
        let row = result
            .entry(host.clone())
            .or_insert_with(|| BuildHostMetrics {
                host: host.clone(),
                ..BuildHostMetrics::default()
            });
        if let Some(timestamp) = raw_timestamp
            .and_then(|value| value.parse::<f64>().ok())
            .and_then(value_u64)
        {
            let timestamp_secs = timestamp / 1000;
            row.observed_at_epoch_secs = Some(
                row.observed_at_epoch_secs
                    .map_or(timestamp_secs, |current| current.max(timestamp_secs)),
            );
        }
        if name == "node_cpu_seconds_total"
            && let Some(cpu) = label(labels, "cpu")
        {
            cpus.entry(host.clone()).or_default().insert(cpu.to_owned());
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
                row.queue_depth_available = true;
                row.queue_depth = row
                    .queue_depth
                    .saturating_add(value_u64(value).unwrap_or(0))
            }
            "ix_build_slots_busy" => {
                row.running_leases_available = true;
                busy_leases.insert(host, value_u64(value).unwrap_or(0));
            }
            "ix_build_lane_running" => {
                row.running_leases_available = true;
                let count = value_u64(value).unwrap_or(0);
                row.running_leases = row.running_leases.saturating_add(count);
                if count > 0
                    && let Some(lane) = label(labels, "lane")
                {
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
    for (host, count) in busy_leases {
        if let Some(row) = result.get_mut(&host) {
            row.running_leases = count;
        }
    }
    result
}

/// Normalizes host labels for case-insensitive policy and metrics joins.
#[implements("FR-048-AC-8")]
pub fn normalize_host_label(host: &str) -> String {
    host.to_lowercase()
}

/// Parses a CLI host policy and normalizes its host key for metrics joins.
#[implements("FR-048-AC-8")]
pub fn parse_build_policy(value: &str) -> Result<(String, BuildPolicy), String> {
    let fields = value.split(':').collect::<Vec<_>>();
    if fields.len() != 3 || fields[0].trim().is_empty() {
        return Err(format!(
            "invalid host policy {value:?}; expected host:max_load1:heavy_gate_slots"
        ));
    }
    let max_load1 = fields[1]
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("invalid max_load1 in host policy {value:?}"))?;
    let heavy_gate_slots = fields[2]
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("invalid lease count in host policy {value:?}"))?;
    Ok((
        normalize_host_label(fields[0].trim()),
        BuildPolicy {
            max_load1,
            heavy_gate_slots,
        },
    ))
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
#[implements("FR-048-AC-8", "FR-050-AC-3")]
pub fn eligibility(metrics: &BuildHostMetrics, policy: BuildPolicy) -> Eligibility {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    eligibility_at(metrics, policy, now)
}

/// Evaluates host telemetry with a deterministic current timestamp.
#[implements("FR-048-AC-8", "FR-050-AC-3")]
pub fn eligibility_at(
    metrics: &BuildHostMetrics,
    policy: BuildPolicy,
    now_epoch_secs: u64,
) -> Eligibility {
    let (Some(load1), Some(cores)) = (metrics.load1, metrics.cores) else {
        return Eligibility::Unknown;
    };
    if !load1.is_finite() || !cores.is_finite() || cores <= 0.0 {
        return Eligibility::Unknown;
    }
    if metrics.memory_total_bytes.is_none()
        || metrics.memory_available_bytes.is_none()
        || metrics.swap_total_bytes.is_none()
        || metrics.swap_free_bytes.is_none()
        || !metrics.queue_depth_available
        || !metrics.running_leases_available
        || metrics
            .observed_at_epoch_secs
            .is_some_and(|observed| observed > now_epoch_secs || now_epoch_secs - observed > 120)
    {
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
    #[trace("FR-048-AC-3", "FR-050-AC-4")]
    fn macos_process_output_retains_build_commands_and_pane_environment() {
        let environments = BTreeMap::from([
            (123, b"HERDR_PANE_ID=tab-123\0".to_vec()),
            (124, b"OTHER=value\0".to_vec()),
        ]);
        let table = parse_macos_process_output(
            b"p123\nfcwd\nn/Users/peter/dev/worktrees/active\nftxt\nn/usr/bin/cargo\np124\nfcwd\nn/Users/peter/other\nf0\nn/dev/null\n",
            b"123 cargo\n124 cargo\n",
            &environments,
            true,
        );
        assert!(table.complete);
        assert_eq!(table.processes.len(), 2);
        assert_eq!(table.processes[0].cmdline, b"cargo");
        assert_eq!(
            table.processes[0].cwd.as_deref(),
            Some(Path::new("/Users/peter/dev/worktrees/active"))
        );
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
        assert_eq!(defaults.mtime_scan_entries, Some(100_000));
        assert_eq!(defaults.mtime_scan_duration_ms, Some(2_000));
        assert_eq!(defaults.hosts["terra"].free_floor_gb, Some(150));

        let settings = toml::from_str::<toml::Value>(
            r#"
            [hygiene]
            worktree_root = "/srv/dev/worktrees"
            idle_minutes = 90
            mtime_scan_entries = 500
            mtime_scan_duration_ms = 150

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
        assert_eq!(config.mtime_scan_entries, Some(500));
        assert_eq!(config.mtime_scan_duration_ms, Some(150));
        assert_eq!(config.hosts["terra"].free_floor_gb, Some(150));
        assert_eq!(config.hosts["luna"].free_floor_gb, Some(100));
        assert_eq!(
            config.build_policy("LUNA"),
            Some(BuildPolicy {
                max_load1: 8.0,
                heavy_gate_slots: 1,
            })
        );
        assert_eq!(normalize_host_label("LuNa"), "luna");
        assert_eq!(
            parse_build_policy("LuNa:8:1").expect("case-insensitive CLI policy"),
            (
                "luna".to_owned(),
                BuildPolicy {
                    max_load1: 8.0,
                    heavy_gate_slots: 1,
                }
            )
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
            assert_eq!(
                target_use_in_worktree(target, target.parent().unwrap(), &table),
                UseState::InUse
            );
        }
        assert_eq!(
            target_use(target, &ProcessTable::default()),
            UseState::Unknown
        );
    }

    #[test]
    #[trace("FR-048-AC-2")]
    fn command_or_environment_reference_to_worktree_protects_target_with_other_cwd() {
        let target = Path::new("/work/wt/target");
        let worktree = target.parent().unwrap();
        for process in [
            ProcessEvidence {
                cwd: Some(PathBuf::from("/tmp")),
                cmdline: b"cargo\0--manifest-path=/work/wt/Cargo.toml".to_vec(),
                ..Default::default()
            },
            ProcessEvidence {
                cwd: Some(PathBuf::from("/tmp")),
                environ: b"PROJECT_ROOT=/work/wt".to_vec(),
                ..Default::default()
            },
        ] {
            assert_eq!(
                target_use_in_worktree(
                    target,
                    worktree,
                    &ProcessTable {
                        processes: vec![process],
                        complete: true,
                    }
                ),
                UseState::InUse
            );
        }
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
        let worktree_cwd = ProcessTable {
            processes: vec![ProcessEvidence {
                cwd: Some(worktree.into()),
                ..Default::default()
            }],
            complete: true,
        };
        assert_eq!(
            clean_decision(target, worktree, now, old, &policy, &worktree_cwd).reason,
            "process_in_use"
        );
    }

    #[test]
    #[cfg(unix)]
    #[trace("FR-048-AC-5", "FR-048-AC-6")]
    fn canonicalized_symlink_root_and_target_still_match_live_process_paths() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let real_root = sandbox.path().join("real/worktrees");
        let worktree = real_root.join("swm-1");
        let target = worktree.join("target");
        std::fs::create_dir_all(target.join("debug")).expect("target");
        std::fs::write(
            worktree.join(".git"),
            "gitdir: ../repo/.git/worktrees/swm-1\n",
        )
        .expect("git metadata");
        let alias_root = sandbox.path().join("alias/worktrees");
        std::fs::create_dir_all(alias_root.parent().unwrap()).expect("alias parent");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_root, &alias_root).expect("symlink root");

        let canonical_root = canonicalize_worktree_root(&alias_root).expect("canonical root");
        assert_eq!(canonical_root, real_root.canonicalize().unwrap());
        let alias_worktree = alias_root.join("swm-1");
        let alias_target = alias_worktree.join("target");
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let table = ProcessTable {
            processes: vec![ProcessEvidence {
                cwd: Some(target.join("debug")),
                ..Default::default()
            }],
            complete: true,
        };
        let decision = clean_decision(
            &alias_target,
            &alias_worktree,
            now,
            now - std::time::Duration::from_secs(4000),
            &HostPolicy {
                free_floor_bytes: 1,
                idle_age_secs: 60,
            },
            &table,
        );
        assert_eq!(decision.reason, "process_in_use");
        assert_eq!(decision.target, target.canonicalize().unwrap());
    }

    #[test]
    #[trace("FR-048-AC-5")]
    fn newest_target_descendant_mtime_controls_idle_age() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let target = sandbox.path().join("target");
        std::fs::create_dir_all(target.join("debug/deep")).expect("nested target");
        let newest_file = target.join("debug/deep/recent-build-artifact");
        std::fs::write(&newest_file, "built").expect("recent artifact");
        let old_target_mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let recent_artifact_mtime = old_target_mtime + std::time::Duration::from_secs(7_200);
        std::fs::File::open(&target)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old_target_mtime))
            .unwrap();
        for directory in [target.join("debug"), target.join("debug/deep")] {
            std::fs::File::open(directory)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(old_target_mtime))
                .unwrap();
        }
        std::fs::File::open(&newest_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(recent_artifact_mtime))
            .unwrap();
        let newest = newest_mtime_under(&target, 100, Duration::from_secs(1))
            .expect("recursive newest mtime");
        assert_eq!(newest, recent_artifact_mtime);
        let decision = clean_decision(
            &target,
            sandbox.path(),
            recent_artifact_mtime + std::time::Duration::from_secs(300),
            newest,
            &HostPolicy {
                free_floor_bytes: 1,
                idle_age_secs: 3_600,
            },
            &ProcessTable {
                processes: Vec::new(),
                complete: true,
            },
        );
        assert_eq!(decision.reason, "too_recent");
    }

    #[test]
    #[trace("FR-048-AC-5", "FR-048-AC-6")]
    fn bounded_mtime_scan_fails_closed_and_fresh_apply_age_skips_removal() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let target = sandbox.path().join("target");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(target.join("one"), "1").expect("first entry");
        std::fs::write(target.join("two"), "2").expect("second entry");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(3_000_000_000);
        let policy = HostPolicy {
            free_floor_bytes: 1,
            idle_age_secs: 3_600,
        };
        let no_processes = ProcessTable {
            processes: Vec::new(),
            complete: true,
        };

        let entry_limit = newest_mtime_under(&target, 1, Duration::from_secs(1))
            .expect_err("entry limit makes age unknown");
        assert_eq!(entry_limit.kind(), io::ErrorKind::TimedOut);
        let time_limit = newest_mtime_under(&target, 100, Duration::from_nanos(1))
            .expect_err("expired time budget makes age unknown");
        assert_eq!(time_limit.kind(), io::ErrorKind::TimedOut);
        let fail_closed = clean_decision(&target, sandbox.path(), now, now, &policy, &no_processes);
        assert!(
            !fail_closed.eligible,
            "unknown age must never permit deletion"
        );
        assert_eq!(fail_closed.reason, "too_recent");

        let nested = target.join("debug");
        std::fs::create_dir(&nested).expect("nested build output");
        let artifact = nested.join("artifact");
        std::fs::write(&artifact, "build").expect("artifact");
        let old = now - Duration::from_secs(7_200);
        for path in [&target, &nested, &artifact] {
            std::fs::File::open(path)
                .expect("open path")
                .set_times(std::fs::FileTimes::new().set_modified(old))
                .expect("set old mtime");
        }
        let planned_mtime =
            newest_mtime_under(&target, 100, Duration::from_secs(1)).expect("plan age");
        let planned = clean_decision(
            &target,
            sandbox.path(),
            now,
            planned_mtime,
            &policy,
            &no_processes,
        );
        assert!(planned.eligible, "old build output is initially eligible");

        let completed_build_mtime = now - Duration::from_secs(30);
        std::fs::File::open(&artifact)
            .expect("open artifact")
            .set_times(std::fs::FileTimes::new().set_modified(completed_build_mtime))
            .expect("record build completion");
        let current_mtime =
            newest_mtime_under(&target, 100, Duration::from_secs(1)).expect("apply age");
        let rechecked = recheck_clean_decision(
            &planned,
            sandbox.path(),
            now,
            current_mtime,
            &policy,
            &no_processes,
        );
        assert!(!rechecked.eligible, "a build finished after planning");
        assert_eq!(rechecked.reason, "too_recent");
        assert!(target.exists(), "fresh target remains available");
    }

    #[test]
    #[trace("FR-048-AC-5", "FR-048-AC-6")]
    fn cleanup_reasons_are_stable_for_protected_outside_and_recent_paths() {
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let policy = HostPolicy {
            free_floor_bytes: 1,
            idle_age_secs: 60,
        };
        let unused = ProcessTable {
            processes: Vec::new(),
            complete: true,
        };
        assert_eq!(
            clean_decision(
                Path::new("/work/wt/output"),
                Path::new("/work/wt"),
                now,
                now - std::time::Duration::from_secs(600),
                &policy,
                &unused
            )
            .reason,
            "not_target_directory"
        );
        assert_eq!(
            clean_decision(
                Path::new("/work/dev_bak/wt/target"),
                Path::new("/work/dev_bak/wt"),
                now,
                now - std::time::Duration::from_secs(600),
                &policy,
                &unused
            )
            .reason,
            "protected_path"
        );
        assert_eq!(
            clean_decision(
                Path::new("/other/wt/target"),
                Path::new("/work/wt"),
                now,
                now - std::time::Duration::from_secs(600),
                &policy,
                &unused
            )
            .reason,
            "outside_worktree"
        );
        assert_eq!(
            clean_decision(
                Path::new("/work/wt/target"),
                Path::new("/work/wt"),
                now,
                now,
                &policy,
                &unused
            )
            .reason,
            "too_recent"
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
            "node_load1{host=\"luna\"} 6.5\n",
            "node_cpu_seconds_total{host=\"luna\",cpu=\"0\",mode=\"idle\"} 1\n",
            "node_cpu_seconds_total{host=\"luna\",cpu=\"1\",mode=\"idle\"} 1\n",
            "node_memory_MemTotal_bytes{host=\"luna\"} 1000\n",
            "node_memory_MemAvailable_bytes{host=\"luna\"} 400\n",
            "node_memory_SwapTotal_bytes{host=\"luna\"} 200\n",
            "node_memory_SwapFree_bytes{host=\"luna\"} 10\n",
            "node_filesystem_avail_bytes{host=\"luna\",mountpoint=\"/\",fstype=\"ext4\"} 700\n",
            "ix_build_queue_depth{host=\"luna\",priority=\"must-fix\"} 2\n",
            "ix_build_queue_depth{host=\"luna\",priority=\"periodic\"} 3\n",
            "ix_build_slots_busy{host=\"luna\"} 1\n",
            "ix_build_lane_running{host=\"luna\",lane=\"luna:team:coder\"} 1\n",
            "ix_build_lane_running{host=\"luna\",lane=\"luna:team:idle\"} 0\n",
            "node_load1{host=\"LUNA\"} 7.6 1700000000\n",
        ));
        let row = &rows["luna"];
        assert_eq!(rows.len(), 1);
        assert_eq!(row.load1, Some(7.6));
        assert_eq!(row.cores, Some(2.0));
        assert_eq!(row.memory_available_bytes, Some(400));
        assert_eq!(row.swap_free_bytes, Some(10));
        assert_eq!(row.disk_available_bytes, Some(700));
        assert_eq!(row.queue_depth, 5);
        assert!(row.queue_depth_available);
        assert_eq!(row.running_leases, 1);
        assert!(row.running_leases_available);
        assert_eq!(row.leases_by_lane["luna:team:coder"], 1);
        assert!(!row.leases_by_lane.contains_key("luna:team:idle"));
        assert_eq!(row.load1, Some(7.6));
        assert_eq!(row.host, "luna");
    }

    #[test]
    #[trace("FR-048-AC-8", "FR-050-AC-3")]
    fn luna_policy_rejects_load_eight_and_any_existing_heavy_gate() {
        let policy = BuildPolicy {
            max_load1: 8.0,
            heavy_gate_slots: 1,
        };
        let mut metrics = BuildHostMetrics {
            load1: Some(7.9),
            cores: Some(8.0),
            memory_total_bytes: Some(1000),
            memory_available_bytes: Some(500),
            swap_total_bytes: Some(100),
            swap_free_bytes: Some(100),
            queue_depth_available: true,
            running_leases_available: true,
            ..Default::default()
        };
        assert_eq!(eligibility(&metrics, policy), Eligibility::Eligible);
        metrics.load1 = Some(8.0);
        assert_eq!(eligibility(&metrics, policy), Eligibility::Ineligible);
        metrics.memory_available_bytes = None;
        assert_eq!(eligibility(&metrics, policy), Eligibility::Unknown);
        metrics.memory_available_bytes = Some(500);
        metrics.load1 = Some(7.9);
        metrics.running_leases = 1;
        assert_eq!(eligibility(&metrics, policy), Eligibility::Ineligible);
        metrics.running_leases = 0;
        metrics.observed_at_epoch_secs = Some(1000);
        assert_eq!(
            eligibility_at(&metrics, policy, 1120),
            Eligibility::Eligible
        );
        assert_eq!(eligibility_at(&metrics, policy, 1121), Eligibility::Unknown);
    }

    #[test]
    #[trace("FR-048-AC-7", "FR-048-AC-8", "FR-050-AC-3")]
    fn empty_locked_build_lease_gauge_is_known_zero() {
        let rows = parse_build_metrics(concat!(
            "node_load1{host=\"luna\"} 1\n",
            "node_cpu_seconds_total{host=\"luna\",cpu=\"0\",mode=\"idle\"} 1\n",
            "node_memory_MemTotal_bytes{host=\"luna\"} 1000\n",
            "node_memory_MemAvailable_bytes{host=\"luna\"} 800\n",
            "node_memory_SwapTotal_bytes{host=\"luna\"} 0\n",
            "node_memory_SwapFree_bytes{host=\"luna\"} 0\n",
            "ix_build_queue_depth{host=\"luna\",class=\"must-fix\"} 0\n",
            "ix_build_queue_depth{host=\"luna\",class=\"point\"} 0\n",
            "ix_build_queue_depth{host=\"luna\",class=\"block\"} 0\n",
            "ix_build_queue_depth{host=\"luna\",class=\"periodic\"} 0\n",
            "ix_build_slots_busy{host=\"luna\"} 0\n",
        ));
        let metrics = &rows["luna"];
        assert!(metrics.queue_depth_available);
        assert_eq!(metrics.queue_depth, 0);
        assert!(metrics.running_leases_available);
        assert_eq!(metrics.running_leases, 0);
        assert_eq!(
            eligibility(
                metrics,
                BuildPolicy {
                    max_load1: 8.0,
                    heavy_gate_slots: 1,
                },
            ),
            Eligibility::Eligible
        );
    }

    #[test]
    #[trace("FR-048-AC-10")]
    fn injected_mount_table_blocks_other_filesystems_and_nested_mounts() {
        let mounts = parse_mount_table(
            "36 25 8:1 / /home rw - ext4 /dev/root rw\n37 36 8:2 / /home/p/dev/worktrees/wt/target/cache rw - ext4 /dev/cache rw\n38 36 8:3 / /mnt/other rw - ext4 /dev/other rw\n",
        )
        .expect("mount table parses");
        assert_eq!(
            mount_safety(
                Path::new("/home/p/dev/worktrees/other"),
                Path::new("/home/p/dev/worktrees"),
                &mounts
            ),
            MountSafety::Safe
        );
        assert_eq!(
            mount_safety(
                Path::new("/mnt/other"),
                Path::new("/home/p/dev/worktrees"),
                &mounts
            ),
            MountSafety::Boundary
        );
        assert_eq!(
            mount_safety(
                Path::new("/home/p/dev/worktrees/wt/target"),
                Path::new("/home/p/dev/worktrees"),
                &mounts
            ),
            MountSafety::Boundary
        );
        assert_eq!(
            mount_safety(
                Path::new("/unknown"),
                Path::new("/home/p/dev/worktrees"),
                &mounts
            ),
            MountSafety::Unknown
        );
        assert!(parse_mount_table("garbage").is_err());
    }

    #[test]
    #[trace("FR-048-AC-10")]
    fn mount_table_parser_unescapes_spaces_in_mountpoints() {
        let mounts =
            parse_mount_table("36 25 8:1 / /home/p/dev\\040space rw - ext4 /dev/root rw\n")
                .expect("mount table parses");
        assert_eq!(mounts[0].mount_point, Path::new("/home/p/dev space"));
    }

    #[test]
    #[trace("FR-048-AC-1", "FR-048-AC-10")]
    fn check_exit_code_compares_free_bytes_with_policy_floor() {
        let report = |free_bytes| HygieneReport {
            host: "draco".into(),
            free_bytes,
            free_floor_bytes: 100,
            targets: Vec::new(),
            prunable_worktrees: Vec::new(),
            orphan_build_processes: Vec::new(),
            process_table_complete: true,
            mount_table_complete: true,
            unsupported: false,
        };
        assert_eq!(report(Some(99)).exit_code(), 1);
        assert_eq!(report(Some(100)).exit_code(), 0);
        assert_eq!(
            HygieneReport {
                targets: vec![CleanDecision {
                    target: "/wt/target".into(),
                    age_secs: 0,
                    eligible: false,
                    reason: "mount_boundary".into(),
                }],
                ..report(Some(100))
            }
            .exit_code(),
            1
        );
        assert_eq!(
            HygieneReport {
                targets: vec![CleanDecision {
                    target: "/wt/target".into(),
                    age_secs: 0,
                    eligible: false,
                    reason: "mount_evidence_unknown".into(),
                }],
                ..report(Some(100))
            }
            .exit_code(),
            2
        );
        assert_eq!(report(None).exit_code(), 2);
        assert_eq!(
            HygieneReport {
                process_table_complete: false,
                ..report(Some(100))
            }
            .exit_code(),
            2
        );
        assert_eq!(
            HygieneReport {
                unsupported: true,
                ..report(Some(100))
            }
            .exit_code(),
            3
        );
        assert_eq!(
            HygieneReport {
                targets: vec![CleanDecision {
                    target: "/wt/target".into(),
                    age_secs: 0,
                    eligible: true,
                    reason: "eligible".into()
                }],
                ..report(Some(100))
            }
            .exit_code(),
            1
        );
        assert_eq!(
            HygieneReport {
                prunable_worktrees: vec!["/wt/dead".into()],
                ..report(Some(100))
            }
            .exit_code(),
            1
        );
        assert_eq!(
            HygieneReport {
                orphan_build_processes: vec![42],
                ..report(Some(100))
            }
            .exit_code(),
            1
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[trace("FR-048-AC-3", "FR-048-AC-5")]
    fn incomplete_macos_process_visibility_never_authorizes_cleanup() {
        assert!(!macos_process_evidence_complete(false, true, true));
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let decision = clean_decision(
            Path::new("/work/wt/target"),
            Path::new("/work/wt"),
            now,
            now - std::time::Duration::from_secs(600),
            &HostPolicy {
                free_floor_bytes: 1,
                idle_age_secs: 60,
            },
            &ProcessTable {
                processes: Vec::new(),
                complete: macos_process_evidence_complete(false, true, true),
            },
        );
        assert_eq!(decision.reason, "process_evidence_incomplete");
        assert!(!decision.eligible);
    }
}
