// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only bounded Rust collection, shared by local and remote probes.

use super::AgentsError;
use super::harness::{Harness, HarnessKind, adapter_for};
use super::lifecycle::{
    AgentId, Availability, Lifecycle, WorkerObservation, WorkerRole, WorkerState,
};
use super::probe::{GitTimes, PaneProcess, Probe, ProbeRequest, SessionFiles, run};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum bytes in a record/header; oversized records are unsupported.
const MAX_RECORD: u64 = 1024 * 1024;
/// Maximum bytes replayed per transcript, enforced before parsing.
const MAX_TRANSCRIPT: u64 = 64 * 1024 * 1024;
/// Aggregate file-byte budget for a collection, including retries.
const MAX_READ_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum discovered entries, pending directory paths and retained headers.
const MAX_FILES: usize = 20_000;
/// Maximum workers per requested graph.
const MAX_WORKERS: usize = 1024;

struct Budget {
    remaining: u64,
    deadline: Instant,
}
impl Budget {
    fn check(&self) -> Result<(), &'static str> {
        if Instant::now() >= self.deadline {
            Err("collection_timeout")
        } else {
            Ok(())
        }
    }
    fn take(&mut self, bytes: u64) -> Result<(), &'static str> {
        self.check()?;
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or("collection_byte_limit")?;
        Ok(())
    }
}
fn epoch_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |t| t.as_secs())
}
fn mtime(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|t| t.as_secs())
}
fn stamp(path: &Path) -> Result<(u64, SystemTime), &'static str> {
    let meta = fs::metadata(path).map_err(|_| "file_unreadable")?;
    Ok((
        meta.len(),
        meta.modified().map_err(|_| "file_time_unavailable")?,
    ))
}
// A deterministic retry seam: a sample returns None only when it changed while read.
fn retry_sample<T>(
    mut sample: impl FnMut() -> Result<Option<T>, &'static str>,
) -> Result<T, &'static str> {
    for _ in 0..2 {
        if let Some(value) = sample()? {
            return Ok(value);
        }
    }
    Err("file_changed_during_collection")
}
fn read_bytes(path: &Path, limit: u64, budget: &mut Budget) -> Result<Vec<u8>, &'static str> {
    retry_sample(|| {
        let before = stamp(path)?;
        if before.0 > limit {
            return Err("file_size_limit");
        }
        budget.check()?;
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| "file_unreadable")?
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "file_unreadable")?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
            return Err("file_size_limit");
        }
        budget.take(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
        Ok((before == stamp(path)?).then_some(bytes))
    })
}
fn walk(root: &Path, availability: &mut Availability, budget: &Budget) -> Vec<PathBuf> {
    let mut stack = vec![(root.to_owned(), 0)];
    let mut files = Vec::new();
    let mut count = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if budget.check().is_err() || depth > 16 {
            availability.missing("discovery_limit");
            break;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                availability.missing(if error.kind() == std::io::ErrorKind::NotFound {
                    "source_directory_missing"
                } else {
                    "source_directory_unreadable"
                });
                continue;
            }
        };
        for entry in entries {
            count += 1;
            if count > MAX_FILES || stack.len() >= MAX_FILES {
                availability.missing("discovery_limit");
                return files;
            }
            let Ok(entry) = entry else {
                availability.missing("directory_entry_unreadable");
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                availability.missing("file_type_unreadable");
                continue;
            };
            if kind.is_symlink() {
                availability.missing("symlink_source_unsupported");
            } else if kind.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
}
fn read_header(
    path: &Path,
    adapter: &dyn Harness,
    budget: &mut Budget,
) -> Result<Lifecycle, &'static str> {
    retry_sample(|| {
        let before = stamp(path)?;
        budget.check()?;
        let file = File::open(path).map_err(|_| "header_unreadable")?;
        let mut bytes = Vec::new();
        BufReader::new(file.take(MAX_RECORD + 1))
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "header_unreadable")?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RECORD {
            return Err("header_size_limit");
        }
        budget.take(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "header_malformed")?;
        if value["type"] != "session_meta" {
            return Err("unsupported_header");
        }
        let mut life = Lifecycle::default();
        adapter.observe_record(&value, &mut life);
        // Full headers, instructions and prompts are dropped here.
        Ok((before == stamp(path)?).then_some(life))
    })
}
fn read_transcript(
    path: &Path,
    adapter: &dyn Harness,
    budget: &mut Budget,
) -> Result<Lifecycle, &'static str> {
    retry_sample(|| {
        let before = stamp(path)?;
        if before.0 > MAX_TRANSCRIPT {
            return Err("transcript_size_limit");
        }
        budget.take(before.0.saturating_add(1))?;
        let file = File::open(path).map_err(|_| "transcript_unreadable")?;
        let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT + 1));
        let mut life = Lifecycle::default();
        loop {
            budget.check()?;
            let mut bytes = Vec::new();
            let n = reader
                .by_ref()
                .take(MAX_RECORD + 1)
                .read_until(b'\n', &mut bytes)
                .map_err(|_| "transcript_unreadable")?;
            if n == 0 {
                break;
            }
            if u64::try_from(n).unwrap_or(u64::MAX) > MAX_RECORD {
                return Err("record_size_limit");
            }
            if bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value =
                serde_json::from_slice(&bytes).map_err(|_| "malformed_transcript_record")?;
            adapter.observe_record(&value, &mut life);
        }
        Ok((before == stamp(path)?).then_some(life))
    })
}

/// Collects one bounded request against an explicit home, without changing files.
pub fn collect_probe(home: &Path, request: &ProbeRequest) -> Result<Probe, AgentsError> {
    request.validate()?;
    let start = epoch_now();
    let mut probe = Probe {
        now: start,
        started_at: start,
        ..Probe::default()
    };
    let mut budget = Budget {
        remaining: MAX_READ_BYTES,
        deadline: Instant::now() + Duration::from_secs(55),
    };
    let mut claude_scan = Availability::available();
    let mut codex_scan = Availability::available();
    let claude = if request
        .sessions
        .iter()
        .any(|q| q.kind == HarnessKind::Claude)
    {
        walk(&home.join(".claude/projects"), &mut claude_scan, &budget)
    } else {
        Vec::new()
    };
    let codex = if request
        .sessions
        .iter()
        .any(|q| q.kind == HarnessKind::Codex)
    {
        walk(&home.join(".codex/sessions"), &mut codex_scan, &budget)
    } else {
        Vec::new()
    };
    let mut headers = BTreeMap::new();
    let mut retained_workers = 0usize;
    if let Some(adapter) = adapter_for("codex") {
        for path in codex {
            match read_header(&path, adapter, &mut budget) {
                Ok(header) => {
                    headers.insert(path, header);
                }
                Err(reason) => codex_scan.missing(reason),
            }
        }
    }
    for query in &request.sessions {
        let mut files = SessionFiles::default();
        let Some(adapter) = adapter_for(query.kind.as_str()) else {
            files.lifecycle_availability.missing("adapter_unsupported");
            files.transcript_availability.missing("adapter_unsupported");
            probe.sessions.insert(query.session_id.clone(), files);
            continue;
        };
        files.lifecycle_availability = match query.kind {
            HarnessKind::Claude => claude_scan.clone(),
            _ => codex_scan.clone(),
        };
        let candidates: Vec<_> = match query.kind {
            HarnessKind::Claude => claude
                .iter()
                .filter(|p| {
                    p.file_name().and_then(|s| s.to_str())
                        == Some(&format!("{}.jsonl", query.session_id))
                })
                .cloned()
                .collect(),
            _ => headers
                .iter()
                .filter(|(_, h)| {
                    h.identity
                        .as_ref()
                        .is_some_and(|(id, _)| id.0 == query.session_id)
                })
                .map(|(p, _)| p.clone())
                .collect(),
        };
        if candidates.len() != 1 {
            files
                .lifecycle_availability
                .missing("parent_transcript_missing_or_ambiguous");
            files
                .transcript_availability
                .missing("parent_transcript_missing_or_ambiguous");
        } else if let Some(parent) = candidates.first() {
            match read_transcript(parent, adapter, &mut budget) {
                Ok(life) => {
                    files.transcript_availability = Availability::available();
                    for issue in &life.issues {
                        files.transcript_availability.missing(issue.clone());
                    }
                    files.transcript_mtime = mtime(parent);
                    files.parent_lifecycle = life;
                }
                Err(reason) => {
                    files.transcript_availability.missing(reason);
                    files.lifecycle_availability.missing(reason);
                }
            }
            match query.kind {
                HarnessKind::Claude => {
                    collect_claude(parent, &query.session_id, adapter, &mut files, &mut budget)
                }
                _ => collect_codex(
                    &headers,
                    &query.session_id,
                    adapter,
                    &mut files,
                    &mut budget,
                ),
            }
            reconcile(&mut files, start);
        }
        let remaining = MAX_WORKERS.saturating_sub(retained_workers);
        if files.workers.len() > remaining {
            files.workers.truncate(remaining);
            files.child_mtimes.truncate(remaining);
            files
                .lifecycle_availability
                .missing("collection_worker_state_limit");
        }
        retained_workers += files.workers.len();
        // Correlation state has served its purpose. Wire only normalized results.
        files.parent_lifecycle.calls.clear();
        files.parent_lifecycle.expected_children.clear();
        files.parent_lifecycle.child_states.clear();
        files.parent_lifecycle.child_owners.clear();
        probe.sessions.insert(query.session_id.clone(), files);
    }
    collect_processes(&mut probe, &mut budget);
    for cwd in &request.cwds {
        let mut times = GitTimes::default();
        let mut coverage = Availability::available();
        if let Ok(text) = run(
            Command::new("git").args([
                "--no-optional-locks",
                "-C",
                cwd,
                "log",
                "-1",
                "--format=%ct",
            ]),
            None,
            budget.deadline.saturating_duration_since(Instant::now()),
        ) {
            times.last_commit = text.trim().parse().ok();
            if times.last_commit.is_none() {
                coverage.missing("git_commit_time_unavailable");
            }
        } else {
            coverage.missing("git_log_unavailable");
        }
        if let Ok(text) = run(
            Command::new("git").args([
                "--no-optional-locks",
                "-C",
                cwd,
                "status",
                "--porcelain=v1",
                "-z",
            ]),
            None,
            budget.deadline.saturating_duration_since(Instant::now()),
        ) {
            let mut paths = text.split('\0');
            while let Some(record) = paths.next() {
                if budget.check().is_err() {
                    coverage.missing("git_collection_timeout");
                    break;
                }
                if let Some(path) = record.get(3..) {
                    times.uncommitted = times
                        .uncommitted
                        .into_iter()
                        .chain(mtime(&Path::new(cwd).join(path)))
                        .max();
                    if record.starts_with('R') || record.starts_with('C') {
                        let _ = paths.next();
                    }
                }
            }
        } else {
            coverage.missing("git_status_unavailable");
        }
        probe.git_availability.insert(cwd.clone(), coverage);
        probe.git.insert(cwd.clone(), times);
    }
    probe.finished_at = epoch_now();
    probe.now = probe.finished_at;
    Ok(probe)
}
#[derive(Deserialize)]
struct ClaudeMetadata {
    #[serde(rename = "agentType")]
    agent_type: Option<String>,
    #[serde(rename = "parentAgentId")]
    parent_agent_id: Option<String>,
    #[serde(rename = "spawnDepth")]
    spawn_depth: Option<u64>,
    #[serde(default)]
    finished: bool,
    #[serde(default, rename = "stoppedByUser")]
    stopped: bool,
}
fn observation(id: AgentId, life: &Lifecycle, adapter: &dyn Harness) -> WorkerObservation {
    WorkerObservation {
        id,
        parent_id: life.identity.as_ref().and_then(|(_, p)| p.clone()),
        agent_path: life.agent_path.clone(),
        declared_depth: life.declared_depth,
        role: life.role,
        state: life.state,
        last_activity_at: life.last_activity_at,
        state_changed_at: life.state_changed_at,
        timestamp_unavailable_reason: life
            .state_changed_at
            .is_none()
            .then(|| "lifecycle_timestamp_unavailable".into()),
        observed_at: epoch_now(),
        source: adapter.evidence_source(),
        provenance: vec!["structured_transcript".into()],
        issues: life.issues.clone(),
    }
}
fn collect_claude(
    parent: &Path,
    root: &str,
    adapter: &dyn Harness,
    files: &mut SessionFiles,
    budget: &mut Budget,
) {
    let dir = parent.with_extension("").join("subagents");
    match fs::metadata(&dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            files
                .lifecycle_availability
                .missing("child_directory_unreadable");
            return;
        }
        Ok(_) => {}
    }
    let paths = walk(&dir, &mut files.lifecycle_availability, budget);
    for path in paths {
        if files.workers.len() >= MAX_WORKERS {
            files.lifecycle_availability.missing("worker_limit");
            break;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("agent-"))
        else {
            continue;
        };
        if id.len() > 512 {
            files.lifecycle_availability.missing("identity_size_limit");
            continue;
        }
        let mut life = match read_transcript(&path, adapter, budget) {
            Ok(life) => life,
            Err(reason) => {
                let mut life = Lifecycle::default();
                life.issue(reason);
                life
            }
        };
        merge_parent_facts(files, &life);
        let mut worker = observation(AgentId(id.into()), &life, adapter);
        if life
            .record_agent_id
            .as_ref()
            .is_none_or(|actual| actual.0 != id)
            || life
                .root_session_id
                .as_ref()
                .is_none_or(|actual| actual.0 != root)
        {
            worker
                .issues
                .push("claude_child_identity_missing_or_conflicting".into());
        }
        match read_bytes(&path.with_extension("meta.json"), 65536, budget).and_then(|bytes| {
            serde_json::from_slice::<ClaudeMetadata>(&bytes).map_err(|_| "metadata_unsupported")
        }) {
            Ok(meta) => {
                worker.declared_depth = meta.spawn_depth;
                worker.role = match meta.agent_type.as_deref() {
                    Some("guardian_review" | "approval" | "internal") => WorkerRole::Internal,
                    Some(_) => WorkerRole::Worker,
                    None => WorkerRole::Unknown,
                };
                worker.parent_id = match (meta.spawn_depth, meta.parent_agent_id) {
                    (Some(1), None) => Some(AgentId(root.into())),
                    (Some(1), Some(id)) if id == root => Some(AgentId(id)),
                    (Some(depth), Some(id)) if depth > 1 && id.len() <= 512 => Some(AgentId(id)),
                    _ => {
                        worker
                            .issues
                            .push("claude_ancestry_missing_or_conflicting".into());
                        None
                    }
                };
                if meta.finished || meta.stopped {
                    if life.assignment_starts > 1 {
                        worker
                            .issues
                            .push("metadata_assignment_order_unresolved".into());
                    }
                    worker.provenance.push("claude_companion_metadata".into());
                    worker.state = if meta.stopped {
                        WorkerState::Stopped
                    } else {
                        WorkerState::Completed
                    };
                    worker.state_changed_at = None;
                    worker.timestamp_unavailable_reason =
                        Some("claude_metadata_has_no_lifecycle_timestamp".into());
                }
            }
            Err(reason) => worker.issues.push(reason.into()),
        }
        // Retain only normalized child facts and write ages.
        life.calls.clear();
        if let Some(time) = mtime(&path) {
            files.child_mtimes.push(time);
        }
        files.workers.push(worker);
    }
}
fn merge_parent_facts(files: &mut SessionFiles, life: &Lifecycle) {
    for expected in &life.expected_children {
        if files.parent_lifecycle.expected_children.len() < MAX_WORKERS {
            files
                .parent_lifecycle
                .expected_children
                .insert(expected.clone());
        } else {
            files.lifecycle_availability.missing("child_state_limit");
        }
    }
    for (id, snapshot) in &life.child_states {
        if files.parent_lifecycle.child_states.len() < MAX_WORKERS {
            let old = files.parent_lifecycle.child_states.get(id);
            if old.is_some_and(|old| old.0 == snapshot.0 && old.1 != snapshot.1) {
                files
                    .parent_lifecycle
                    .child_states
                    .insert(id.clone(), (snapshot.0, WorkerState::Unknown));
                continue;
            }
            if old.is_none_or(|old| old.0 < snapshot.0) {
                files
                    .parent_lifecycle
                    .child_states
                    .insert(id.clone(), *snapshot);
                if let Some(owner) = life.child_owners.get(id) {
                    files
                        .parent_lifecycle
                        .child_owners
                        .insert(id.clone(), owner.clone());
                }
            }
        } else {
            files.lifecycle_availability.missing("child_state_limit");
        }
    }
}
fn collect_codex(
    headers: &BTreeMap<PathBuf, Lifecycle>,
    root: &str,
    adapter: &dyn Harness,
    files: &mut SessionFiles,
    budget: &mut Budget,
) {
    let mut descendants = BTreeSet::from([AgentId(root.into())]);
    loop {
        let before = descendants.len();
        for header in headers.values() {
            if let Some((id, Some(parent))) = &header.identity
                && descendants.contains(parent)
            {
                descendants.insert(id.clone());
            }
        }
        if descendants.len() == before {
            break;
        }
        if descendants.len() > MAX_WORKERS + 1 {
            files.lifecycle_availability.missing("worker_limit");
            break;
        }
    }
    for (path, header) in headers {
        let Some((id, _)) = &header.identity else {
            continue;
        };
        if id.0 == root || !descendants.contains(id) {
            continue;
        }
        if files.workers.len() >= MAX_WORKERS {
            files.lifecycle_availability.missing("worker_limit");
            break;
        }
        let mut life = match read_transcript(path, adapter, budget) {
            Ok(life) => life,
            Err(reason) => {
                let mut life = header.clone();
                life.issue(reason);
                life
            }
        };
        if life.identity != header.identity {
            life.issue("header_changed_during_collection");
        }
        merge_parent_facts(files, &life);
        if let Some(time) = mtime(path) {
            files.child_mtimes.push(time);
        }
        files.workers.push(observation(id.clone(), &life, adapter));
    }
}
fn reconcile(files: &mut SessionFiles, start: u64) {
    let parents: BTreeMap<_, _> = files
        .workers
        .iter()
        .map(|w| (w.id.clone(), w.parent_id.clone()))
        .collect();
    for expected in &files.parent_lifecycle.expected_children {
        let matches = files
            .workers
            .iter()
            .filter(|w| w.id.0 == *expected || w.agent_path.as_deref() == Some(expected))
            .count();
        if matches != 1 {
            files
                .lifecycle_availability
                .missing("referenced_child_missing_or_ambiguous");
        }
    }
    for worker in &mut files.workers {
        let key = if files
            .parent_lifecycle
            .child_states
            .contains_key(&worker.id.0)
        {
            Some(worker.id.0.as_str())
        } else {
            worker.agent_path.as_deref()
        };
        if let Some((key, &(at, state))) = key.and_then(|key| {
            files
                .parent_lifecycle
                .child_states
                .get(key)
                .map(|s| (key, s))
        }) {
            let owner = files.parent_lifecycle.child_owners.get(key);
            let mut parent = worker.parent_id.clone();
            let mut authorized = false;
            for _ in 0..MAX_WORKERS {
                if parent.as_ref() == owner && owner.is_some() {
                    authorized = true;
                    break;
                }
                parent = parent.and_then(|p| parents.get(&p).cloned().flatten());
                if parent.is_none() {
                    break;
                }
            }
            if !authorized {
                worker
                    .issues
                    .push("snapshot_source_ownership_unresolved".into());
                continue;
            }
            if at > start {
                worker
                    .issues
                    .push("snapshot_timestamp_after_collection_start".into());
                continue;
            }
            if worker.state_changed_at.is_none()
                && worker.state == WorkerState::Unknown
                && worker.issues.is_empty()
            {
                worker.state = state;
                worker.state_changed_at = Some(at);
                worker.timestamp_unavailable_reason = None;
                worker.provenance.push(format!(
                    "correlated_parent_snapshot:{}",
                    owner.map_or("unknown", |id| id.0.as_str())
                ));
            }
            // A snapshot can confirm completion. Running snapshots do not reopen terminal work.
            if worker.state_changed_at.is_some_and(|old| old < at)
                && (!matches!(worker.state, WorkerState::Completed | WorkerState::Stopped)
                    || !state.is_active())
            {
                worker.state = state;
                worker.state_changed_at = Some(at);
                worker.timestamp_unavailable_reason = None;
            } else if worker.state_changed_at == Some(at) && worker.state != state {
                worker.issues.push("child_snapshot_conflict".into());
            }
        }
        if worker.last_activity_at.is_some_and(|at| at > start) {
            worker.issues.push("activity_after_collection_start".into());
        }
    }
}
fn collect_processes(probe: &mut Probe, budget: &mut Budget) {
    if !cfg!(target_os = "linux") {
        probe
            .process_availability
            .missing("process_collection_unsupported_platform");
        return;
    }
    let entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(_) => {
            probe
                .process_availability
                .missing("process_directory_unreadable");
            return;
        }
    };
    probe.process_availability = Availability::available();
    for (n, entry) in entries.enumerate() {
        if n >= MAX_FILES {
            probe
                .process_availability
                .missing("process_enumeration_limit");
            break;
        }
        let Ok(entry) = entry else {
            probe
                .process_availability
                .missing("process_entry_unreadable");
            continue;
        };
        if entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
            .is_none()
        {
            continue;
        }
        let bytes = match read_bytes(&entry.path().join("environ"), 65536, budget) {
            Ok(bytes) => bytes,
            Err(reason) => {
                probe.process_availability.missing(reason);
                continue;
            }
        };
        let pane = bytes
            .split(|&b| b == 0)
            .find_map(|f| f.strip_prefix(b"HERDR_PANE_ID="))
            .and_then(|s| std::str::from_utf8(s).ok());
        let Some(pane) = pane.filter(|p| p.len() <= 512) else {
            continue;
        };
        if probe.processes.len() >= 4096 {
            probe.process_availability.missing("process_state_limit");
            break;
        }
        let command = read_bytes(&entry.path().join("comm"), 1024, budget);
        let args = read_bytes(&entry.path().join("cmdline"), 4096, budget);
        match (command, args) {
            (Ok(comm), Ok(args)) => {
                let list = probe.processes.entry(pane.into()).or_default();
                if list.len() < 128 {
                    list.push(PaneProcess {
                        comm: String::from_utf8_lossy(&comm).trim().into(),
                        args: if String::from_utf8_lossy(&args)
                            .split('\0')
                            .take(2)
                            .any(|word| word.rsplit('/').next() == Some("locked-build.sh"))
                        {
                            "locked-build.sh".into()
                        } else {
                            String::new()
                        },
                    });
                } else {
                    probe.process_availability.missing("pane_process_limit");
                }
            }
            _ => probe
                .process_availability
                .missing("pane_process_unreadable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Trace: FR-018-AC-20
    #[test]
    fn changing_sample_retries_once_then_reports_uncertainty() {
        let mut n = 0;
        assert_eq!(
            retry_sample(|| {
                n += 1;
                Ok((n == 2).then_some(7))
            }),
            Ok(7)
        );
        assert_eq!(n, 2);
        n = 0;
        assert_eq!(
            retry_sample::<()>(|| {
                n += 1;
                Ok(None)
            }),
            Err("file_changed_during_collection")
        );
        assert_eq!(n, 2);
    }
}
