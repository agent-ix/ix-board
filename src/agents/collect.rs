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
/// Identity prepass is part of, and bounded separately within, the total budget.
const MAX_IDENTITY_READ_BYTES: u64 = 16 * 1024 * 1024;
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
        let cap = (limit + 1).min(budget.remaining);
        if cap == 0 || before.0 > cap {
            return Err("collection_byte_limit");
        }
        let mut bytes = Vec::new();
        let result = File::open(path)
            .map_err(|_| "file_unreadable")?
            .take(cap)
            .read_to_end(&mut bytes);
        let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        budget.take(read)?;
        if before != stamp(path)? {
            return Ok(None);
        }
        result.map_err(|_| "file_unreadable")?;
        if read > limit {
            return Err("file_size_limit");
        }
        if cap < limit + 1 && read == cap && before.0 == 0 {
            return Err("collection_byte_limit");
        }
        Ok(Some(bytes))
    })
}
fn walk(root: &Path, availability: &mut Availability, budget: &Budget) -> Vec<PathBuf> {
    walk_limited(root, availability, budget, MAX_FILES)
}
fn walk_limited(
    root: &Path,
    availability: &mut Availability,
    budget: &Budget,
    max_entries: usize,
) -> Vec<PathBuf> {
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
            if count > max_entries || stack.len() >= max_entries {
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
fn first_record(path: &Path, budget: &mut Budget) -> Result<Value, &'static str> {
    retry_sample(|| {
        let before = stamp(path)?;
        budget.check()?;
        let cap = (MAX_RECORD + 1).min(budget.remaining);
        if cap == 0 {
            return Err("collection_byte_limit");
        }
        let file = File::open(path).map_err(|_| "header_unreadable")?;
        let mut reader = BufReader::new(file.take(cap));
        let mut bytes = Vec::new();
        let result = reader.read_until(b'\n', &mut bytes);
        // Charge physical buffered reads, including bytes read past the newline.
        let read = cap - reader.into_inner().limit();
        budget.take(read)?;
        if before != stamp(path)? {
            return Ok(None);
        }
        result.map_err(|_| "header_unreadable")?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RECORD {
            return Err("header_size_limit");
        }
        if cap < MAX_RECORD + 1 && bytes.last() != Some(&b'\n') && before.0 > cap {
            return Err("collection_byte_limit");
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| "header_malformed")?;
        Ok(Some(value))
    })
}
fn read_header(
    path: &Path,
    adapter: &dyn Harness,
    budget: &mut Budget,
) -> Result<Lifecycle, &'static str> {
    let value = first_record(path, budget)?;
    if value["type"] != "session_meta" {
        return Err("unsupported_header");
    }
    let mut life = Lifecycle::default();
    adapter.observe_record(&value, &mut life);
    // Full headers, instructions and prompts are dropped here.
    Ok(life)
}
fn claude_identity(path: &Path, budget: &mut Budget) -> Result<Lifecycle, &'static str> {
    let value = first_record(path, budget)?;
    let id = |key: &str| {
        value[key]
            .as_str()
            .filter(|id| id.len() <= 512)
            .map(|id| AgentId(id.into()))
    };
    Ok(Lifecycle {
        record_agent_id: id("agentId"),
        root_session_id: id("sessionId"),
        ..Lifecycle::default()
    })
}
/// Bytes read per backward step when looking for the latest turn start.
const TAIL_WINDOW: u64 = 1024 * 1024;
/// Deepest backward scan. With one look-behind byte and a header read of at most
/// `MAX_RECORD` bytes, the whole tail read stays within `MAX_TRANSCRIPT`.
const TAIL_SCAN_LIMIT: u64 = MAX_TRANSCRIPT - MAX_RECORD - 2;

fn starts_turn(line: &[u8]) -> bool {
    let mentions = |needle: &[u8]| line.windows(needle.len()).any(|w| w == needle);
    if !(mentions(b"task_started") || mentions(b"turn_started")) {
        return false;
    }
    serde_json::from_slice::<Value>(line).is_ok_and(|value| {
        value["type"] == "event_msg"
            && matches!(
                value["payload"]["type"].as_str(),
                Some("task_started" | "turn_started")
            )
    })
}

/// The records from the latest turn start to the end of the file.
struct TailRegion {
    /// File offset of the first byte of `bytes`, a record boundary.
    offset: u64,
    bytes: Vec<u8>,
}

/// Scans backward once over each byte for the latest turn start.
///
/// The bytes read are kept, so the forward replay needs no second read and each
/// byte is charged once. When the whole file is covered without a turn start the
/// region is the whole file. Fails when `TAIL_SCAN_LIMIT` bytes are scanned
/// without finding one.
fn read_tail_region(
    path: &Path,
    extent: u64,
    budget: &mut Budget,
) -> Result<TailRegion, &'static str> {
    use std::io::Seek;
    let mut file = File::open(path).map_err(|_| "transcript_unreadable")?;
    let floor = extent.saturating_sub(TAIL_SCAN_LIMIT);
    let mut pos = extent;
    // Chunks hold [start, pos) for each step, latest first.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    // Head of the line that begins at `pos`, kept until its start is read.
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let start = pos.saturating_sub(TAIL_WINDOW).max(floor);
        // Only the last step needs a look-behind byte, to tell whether `floor` is a boundary.
        let last = start == floor;
        let from = if last { start.saturating_sub(1) } else { start };
        budget.take(pos - from)?;
        file.seek(std::io::SeekFrom::Start(from))
            .map_err(|_| "transcript_unreadable")?;
        let len = usize::try_from(pos - from).map_err(|_| "transcript_size_limit")?;
        let mut buf = vec![0; len];
        file.read_exact(&mut buf)
            .map_err(|_| "transcript_unreadable")?;
        let skip = usize::try_from(start - from).unwrap_or(0);
        chunks.push(buf[skip..].to_vec());
        buf.extend_from_slice(&carry);
        let first_newline = |from_index: usize| {
            buf[from_index..]
                .iter()
                .position(|b| *b == b'\n')
                .map(|at| at + from_index)
        };
        // Index in `buf` where the first complete line begins.
        let line_begin = if start == 0 {
            Some(0)
        } else if last && skip == 1 && buf[0] == b'\n' {
            Some(1)
        } else {
            first_newline(skip).map(|at| at + 1)
        };
        let Some(line_begin) = line_begin else {
            if u64::try_from(buf.len()).unwrap_or(u64::MAX) > MAX_RECORD {
                return Err("record_size_limit");
            }
            if last {
                return Err("lifecycle_not_found_within_limit");
            }
            carry = buf.split_off(skip);
            pos = start;
            continue;
        };
        let mut offset = line_begin;
        let mut latest = None;
        for line in buf[line_begin..].split(|b| *b == b'\n') {
            if u64::try_from(line.len()).unwrap_or(u64::MAX) > MAX_RECORD {
                return Err("record_size_limit");
            }
            if starts_turn(line) {
                latest = Some(offset);
            }
            offset += line.len() + 1;
        }
        let found = match latest {
            Some(at) => Some(from + at as u64),
            None if start == 0 => Some(0),
            None => None,
        };
        if let Some(found) = found {
            let skip_in_concat = usize::try_from(found - start).unwrap_or(0);
            let mut bytes = Vec::new();
            for chunk in chunks.iter().rev() {
                bytes.extend_from_slice(chunk);
            }
            bytes.drain(..skip_in_concat);
            return Ok(TailRegion {
                offset: found,
                bytes,
            });
        }
        if last {
            return Err("lifecycle_not_found_within_limit");
        }
        // The unprocessed head of this step belongs to the next one.
        carry = buf[skip..line_begin].to_vec();
        pos = start;
    }
}

/// Replays complete records from memory into `life`.
fn replay_bytes(
    bytes: &[u8],
    adapter: &dyn Harness,
    mut life: Lifecycle,
    budget: &Budget,
) -> Result<Lifecycle, &'static str> {
    for line in bytes.split(|b| *b == b'\n') {
        budget.check()?;
        if u64::try_from(line.len()).unwrap_or(u64::MAX) > MAX_RECORD {
            return Err("record_size_limit");
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value = serde_json::from_slice(line).map_err(|_| "malformed_transcript_record")?;
        adapter.observe_record(&value, &mut life);
    }
    Ok(life)
}

/// Reads the latest lifecycle state from the end of a transcript larger than
/// `MAX_TRANSCRIPT`.
///
/// The replay starts at the latest turn start and is seeded from the
/// `session_meta` header so identity and ancestry survive.
fn read_transcript_tail(
    path: &Path,
    adapter: &dyn Harness,
    budget: &mut Budget,
) -> Result<Lifecycle, &'static str> {
    retry_sample(|| {
        let before = stamp(path)?;
        let result = (|| {
            let region = read_tail_region(path, before.0, budget)?;
            let mut seed = Lifecycle::default();
            if region.offset > 0 {
                let header = first_record(path, budget)?;
                if header["type"] == "session_meta" {
                    adapter.observe_record(&header, &mut seed);
                }
            }
            replay_bytes(&region.bytes, adapter, seed, budget)
        })();
        if before != stamp(path)? {
            return Ok(None);
        }
        result.map(Some)
    })
}
fn read_transcript(
    path: &Path,
    adapter: &dyn Harness,
    budget: &mut Budget,
) -> Result<Lifecycle, &'static str> {
    // A rollout within the replay limit is read whole, so earlier turns' spawns are kept.
    if adapter.kind() == HarnessKind::Codex && stamp(path)?.0 > MAX_TRANSCRIPT {
        return read_transcript_tail(path, adapter, budget);
    }
    retry_sample(|| {
        let before = stamp(path)?;
        if before.0 > MAX_TRANSCRIPT {
            return Err("transcript_size_limit");
        }
        budget.take(before.0.saturating_add(1))?;
        let file = File::open(path).map_err(|_| "transcript_unreadable")?;
        // Read the sampled extent. Appended bytes belong to the retry, rather
        // than consuming uncharged bytes or exposing a partial new record.
        let mut reader = BufReader::new(file.take(before.0));
        let result = (|| {
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
            Ok(life)
        })();
        if before != stamp(path)? {
            return Ok(None);
        }
        result.map(Some)
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
        descendants_complete: true,
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
    let mut paths = walk(&dir, &mut files.lifecycle_availability, budget);
    if paths.len() > MAX_WORKERS {
        files.lifecycle_availability.missing("worker_limit");
        paths.truncate(MAX_WORKERS);
    }
    let mut candidates: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let metadata =
                read_bytes(&path.with_extension("meta.json"), 65536, budget).and_then(|bytes| {
                    serde_json::from_slice::<ClaudeMetadata>(&bytes)
                        .map_err(|_| "metadata_unsupported")
                });
            (path, metadata)
        })
        .collect();
    let reserved = budget.remaining.min(MAX_IDENTITY_READ_BYTES);
    let mut identity_budget = Budget {
        remaining: reserved,
        deadline: budget.deadline,
    };
    let mut candidates: Vec<_> = candidates
        .drain(..)
        .map(|(path, metadata)| {
            let identity = claude_identity(&path, &mut identity_budget);
            (path, metadata, identity)
        })
        .collect();
    budget.remaining = budget
        .remaining
        .saturating_sub(reserved - identity_budget.remaining);
    candidates.sort_by(|(a_path, a, _), (b_path, b, _)| {
        let terminal = |meta: &Result<ClaudeMetadata, &'static str>| {
            meta.as_ref()
                .is_ok_and(|meta| meta.finished || meta.stopped)
        };
        (terminal(a), a_path).cmp(&(terminal(b), b_path))
    });
    for (path, metadata, identity) in candidates {
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
        let mut transcript_complete = true;
        let mut life = match read_transcript(&path, adapter, budget) {
            Ok(life) => life,
            Err(reason) => {
                transcript_complete = false;
                // Small identity evidence remains useful even when historical replay
                // cannot fit. It cannot prove the absence of later assignments.
                let mut life = identity.unwrap_or_default();
                life.issue(reason);
                life
            }
        };
        // The prepass only orders work. Re-read authoritative metadata after
        // the transcript so a completion during collection is not missed.
        let metadata =
            match read_bytes(&path.with_extension("meta.json"), 65536, budget).and_then(|bytes| {
                serde_json::from_slice::<ClaudeMetadata>(&bytes).map_err(|_| "metadata_unsupported")
            }) {
                Ok(fresh) => Ok(fresh),
                Err(reason) => {
                    life.issue(reason);
                    metadata
                }
            };
        merge_parent_facts(files, &life);
        let mut worker = observation(AgentId(id.into()), &life, adapter);
        worker.descendants_complete = transcript_complete;
        if !transcript_complete {
            worker.provenance = vec!["identity_header".into()];
        }
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
        match metadata {
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
        let mut transcript_complete = true;
        let mut life = match read_transcript(path, adapter, budget) {
            Ok(life) => life,
            Err(reason) => {
                transcript_complete = false;
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
        let mut worker = observation(id.clone(), &life, adapter);
        worker.descendants_complete = transcript_complete;
        files.workers.push(worker);
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
                worker.provenance.push(format!(
                    "correlated_parent_snapshot:{}",
                    owner.map_or("unknown", |id| id.0.as_str())
                ));
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
    use ix_trace_rs::trace;
    #[trace("FR-018-AC-28")]
    #[test]
    fn the_tail_read_charges_each_scanned_byte_once_and_stays_within_the_transcript_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let line = |kind: &str, pad: usize| {
            format!(
                "{}\n",
                serde_json::json!({"type":"response_item","timestamp":"2026-10-18T10:00:00Z","payload":{"type":kind,"pad":"x".repeat(pad)}})
            )
        };
        let event = |kind: &str, at: &str| {
            format!(
                "{}\n",
                serde_json::json!({"type":"event_msg","timestamp":at,"payload":{"type":kind,"turn_id":"t"}})
            )
        };
        let mut text = format!(
            "{}\n",
            serde_json::json!({"type":"session_meta","timestamp":"2026-10-18T10:00:00Z","payload":{"id":"root","source":"cli"}})
        );
        let filler = line("other", 900);
        for _ in 0..(MAX_TRANSCRIPT as usize / filler.len() + 1) {
            text.push_str(&filler);
        }
        text.push_str(&event("task_started", "2026-10-18T10:00:01Z"));
        let in_turn = 10 * 1024 * 1024;
        for _ in 0..(in_turn / filler.len()) {
            text.push_str(&filler);
        }
        text.push_str(&event("task_complete", "2026-10-18T10:00:09Z"));
        fs::write(&path, &text).unwrap();
        let mut budget = Budget {
            remaining: MAX_READ_BYTES,
            deadline: Instant::now() + Duration::from_secs(120),
        };
        let life = read_transcript_tail(&path, adapter_for("codex").unwrap(), &mut budget).unwrap();
        assert_eq!(life.state, WorkerState::Completed);
        let charged = MAX_READ_BYTES - budget.remaining;
        assert!(charged <= MAX_TRANSCRIPT, "{charged}");
        // A scan of the 10 MiB turn plus one window and the header read; a second pass over
        // the region would charge about twice that.
        assert!(
            charged <= in_turn as u64 + TAIL_WINDOW + 16 * 1024,
            "{charged}"
        );
    }
    #[trace("FR-018-AC-20")]
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
    #[test]
    #[trace("FR-018-AC-20")]
    fn file_enumeration_has_a_configured_finite_entry_budget() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a.jsonl", "b.jsonl", "c.jsonl"] {
            fs::write(root.path().join(name), "{}").unwrap();
        }
        let budget = Budget {
            remaining: MAX_READ_BYTES,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        let mut coverage = Availability::available();
        let paths = walk_limited(root.path(), &mut coverage, &budget, 2);
        assert_eq!(paths.len(), 2);
        assert!(!coverage.complete);
        assert!(coverage.reasons.contains(&"discovery_limit".into()));
    }
    struct ChangingAdapter {
        path: PathBuf,
        changes: std::sync::atomic::AtomicUsize,
        limit: usize,
    }
    impl Harness for ChangingAdapter {
        fn kind(&self) -> HarnessKind {
            HarnessKind::Claude
        }
        fn evidence_source(&self) -> super::super::EvidenceSource {
            super::super::EvidenceSource::Claude
        }
        fn observe_record(&self, value: &Value, life: &mut Lifecycle) {
            adapter_for("claude").unwrap().observe_record(value, life);
            if value["type"] == "user"
                && self
                    .changes
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    < self.limit
            {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&self.path)
                    .unwrap();
                writeln!(file, "{{\"type\":\"system\"}}").unwrap();
            }
        }
    }
    #[test]
    #[trace("FR-018-AC-20")]
    fn actual_transcript_changes_retry_once_then_become_unavailable() {
        for limit in [1, 2] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("session.jsonl");
            fs::write(
                &path,
                include_str!("../../tests/fixtures/agents/claude_done.jsonl"),
            )
            .unwrap();
            let adapter = ChangingAdapter {
                path: path.clone(),
                changes: std::sync::atomic::AtomicUsize::new(0),
                limit,
            };
            let mut budget = Budget {
                remaining: MAX_READ_BYTES,
                deadline: Instant::now() + Duration::from_secs(5),
            };
            let result = read_transcript(&path, &adapter, &mut budget);
            assert_eq!(adapter.changes.load(std::sync::atomic::Ordering::SeqCst), 2);
            if limit == 1 {
                assert!(result.unwrap().execution_ended_at.is_some());
            } else {
                assert_eq!(result.unwrap_err(), "file_changed_during_collection");
            }
        }
    }
    #[test]
    #[trace("FR-018-AC-9", "FR-018-AC-14", "FR-018-AC-20")]
    fn metadata_priority_preserves_current_workers_before_old_logs_exhaust_the_byte_budget() {
        let home = tempfile::tempdir().unwrap();
        let parent = home.path().join(".claude/projects/test/root.jsonl");
        let dir = parent.with_extension("").join("subagents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &parent,
            include_str!("../../tests/fixtures/agents/claude_done.jsonl"),
        )
        .unwrap();
        for (id, terminal, padding) in [
            ("a-old", true, 70 * 1024),
            ("b-old", true, 70 * 1024),
            ("z-current", false, 40 * 1024),
        ] {
            let assignment = serde_json::json!({"type":"user","agentId":id,"sessionId":"root","isMeta":false,"timestamp":"2026-10-18T10:00:00Z","message":{"content":"Assigned checks."}});
            let injected = serde_json::json!({"type":"user","agentId":id,"sessionId":"root","isMeta":true,"timestamp":"2026-10-18T10:00:01Z","message":{"content":"x".repeat(padding)}});
            fs::write(
                dir.join(format!("agent-{id}.jsonl")),
                format!("{assignment}\n{injected}\n"),
            )
            .unwrap();
            fs::write(dir.join(format!("agent-{id}.meta.json")),serde_json::json!({"agentType":"general-purpose","spawnDepth":1,"finished":terminal}).to_string()).unwrap();
        }
        let limit = 100 * 1024;
        let mut budget = Budget {
            remaining: limit,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        let mut files = SessionFiles {
            lifecycle_availability: Availability::available(),
            ..SessionFiles::default()
        };
        collect_claude(
            &parent,
            "root",
            adapter_for("claude").unwrap(),
            &mut files,
            &mut budget,
        );
        let current = files
            .workers
            .iter()
            .find(|w| w.id.0 == "z-current")
            .unwrap();
        assert_eq!(current.state, WorkerState::Running);
        assert!(current.issues.is_empty(), "{current:?}");
        assert!(current.descendants_complete);
        let terminal: Vec<_> = files
            .workers
            .iter()
            .filter(|w| w.id.0 != "z-current")
            .collect();
        assert_eq!(terminal.len(), 2);
        for w in terminal {
            assert_eq!(w.state, WorkerState::Completed);
            assert!(!w.descendants_complete);
            assert!(w.issues.contains(&"collection_byte_limit".into()));
            assert!(
                !w.issues
                    .contains(&"claude_child_identity_missing_or_conflicting".into())
            );
        }
        let count = super::super::count_workers(
            &AgentId("root".into()),
            &files.workers,
            files.lifecycle_availability,
        );
        assert_eq!(count.known_direct_active, 1);
        assert_eq!((count.direct, count.nested), (None, None));
        assert!(
            count
                .availability
                .reasons
                .contains(&"descendant_references_incomplete".into())
        );
        assert!(budget.remaining > 0 && budget.remaining < limit);
    }
    #[test]
    #[trace("FR-018-AC-20")]
    fn header_reads_charge_buffered_bytes_and_stop_at_the_remaining_budget() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("header.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"header-only\"}}\n",
        )
        .unwrap();
        let mut budget = Budget {
            remaining: 8,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        assert_eq!(
            first_record(&path, &mut budget).unwrap_err(),
            "collection_byte_limit"
        );
        assert_eq!(budget.remaining, 0);
        assert_eq!(
            first_record(&path, &mut budget).unwrap_err(),
            "collection_byte_limit"
        );
        assert_eq!(budget.remaining, 0);
    }
    struct ExpandingAdapter {
        path: PathBuf,
        expanded: std::sync::atomic::AtomicBool,
        observed_unpaid_record: std::sync::atomic::AtomicBool,
    }
    impl Harness for ExpandingAdapter {
        fn kind(&self) -> HarnessKind {
            HarnessKind::Claude
        }
        fn evidence_source(&self) -> super::super::EvidenceSource {
            super::super::EvidenceSource::Claude
        }
        fn observe_record(&self, value: &Value, life: &mut Lifecycle) {
            use std::sync::atomic::Ordering::SeqCst;
            adapter_for("claude").unwrap().observe_record(value, life);
            if value.get("padding").is_some() {
                self.observed_unpaid_record.store(true, SeqCst);
            }
            if value["type"] == "user" && !self.expanded.swap(true, SeqCst) {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&self.path)
                    .unwrap();
                writeln!(
                    file,
                    "{}",
                    serde_json::json!({"type":"system","padding":"x".repeat(20*1024)})
                )
                .unwrap();
            }
        }
    }
    #[test]
    #[trace("FR-018-AC-20")]
    fn an_append_cannot_feed_records_beyond_the_prepaid_transcript_extent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("transcript.jsonl");
        fs::write(
            &path,
            include_str!("../../tests/fixtures/agents/claude_done.jsonl"),
        )
        .unwrap();
        let adapter = ExpandingAdapter {
            path: path.clone(),
            expanded: std::sync::atomic::AtomicBool::new(false),
            observed_unpaid_record: std::sync::atomic::AtomicBool::new(false),
        };
        let mut budget = Budget {
            remaining: 1024,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        assert_eq!(
            read_transcript(&path, &adapter, &mut budget).unwrap_err(),
            "collection_byte_limit"
        );
        assert!(
            !adapter
                .observed_unpaid_record
                .load(std::sync::atomic::Ordering::SeqCst)
        );
        assert!(budget.remaining < 1024);
    }
}
