// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Bounded, privacy-preserving transcript source for deterministic role rules.
//!
//! The caller supplies the session binding from hook/relay evidence. Transcript
//! files never carry the Herdr address or role reliably enough to infer them.

use super::{EvalWindow, QueryValue, RoleSpec};
use ix_trace_rs::implements;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_TRANSCRIPT_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const DEFAULT_MAX_DISCOVERY_ENTRIES: usize = 20_000;
pub const DEFAULT_MAX_COLLECTION_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_TRANSCRIPT_SESSIONS: usize = 128;

/// A transcript encoding supported by the bounded parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessFormat {
    Claude,
    Codex,
}

/// Explicit identity supplied by a hook or relay source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionBinding {
    pub transcript_id: String,
    pub session_addr: String,
    pub session_role: String,
    pub started_at_ms: i64,
}

/// A normalized transcript event. `body` is kept only in memory for pattern
/// checks and is deliberately not serializable or printable.
#[derive(Clone)]
pub struct TranscriptEvent {
    pub at_ms: i64,
    pub kind: TranscriptEventKind,
    pub tool: Option<String>,
    pub tool_call_id: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub path: Option<String>,
    pub recipient: Option<String>,
    pub exit_code: Option<i32>,
    pub status_complete: Option<bool>,
    body: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptEventKind {
    IncomingMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
}

/// One session and the bounded transcript evidence accepted for it.
#[derive(Clone)]
pub struct TranscriptSession {
    pub binding: SessionBinding,
    pub events: Vec<TranscriptEvent>,
    pub complete: bool,
    pub unavailable_reason: Option<String>,
    pub bytes_read: usize,
    pub fleet_status_checks: Vec<FleetStatusCheck>,
}

/// Hook transition used to make a virtual `session_turn` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookTurn {
    pub transcript_id: String,
    pub session_addr: String,
    pub session_role: String,
    pub started_at_ms: i64,
}

/// In-memory session-turn projection. It is not persisted by the evaluator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTurnRow {
    /// Transcript ID from an authoritative source, or `None` when unavailable.
    pub transcript_id: Option<String>,
    /// Explicit relay/session address.
    pub session_addr: String,
    /// Configured role attributed to the address.
    pub session_role: String,
    /// Hook event time in Unix milliseconds.
    pub started_at_ms: i64,
}

/// Explicit limits for live transcript discovery and reading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TranscriptReadLimits {
    /// Maximum bytes in any one transcript.
    pub max_transcript_bytes: usize,
    /// Maximum bytes in one newline-delimited record.
    pub max_record_bytes: usize,
    /// Maximum total transcript file bytes read in one discovery call.
    pub max_collection_bytes: usize,
    /// Maximum directory entries inspected across both harness roots.
    pub max_discovery_entries: usize,
    /// Maximum distinct transcript IDs accepted from the input rows.
    pub max_sessions: usize,
    /// Maximum directory nesting below either harness root.
    pub max_directory_depth: usize,
}

impl Default for TranscriptReadLimits {
    fn default() -> Self {
        Self {
            max_transcript_bytes: DEFAULT_MAX_TRANSCRIPT_BYTES,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            max_collection_bytes: DEFAULT_MAX_COLLECTION_BYTES,
            max_discovery_entries: DEFAULT_MAX_DISCOVERY_ENTRIES,
            max_sessions: DEFAULT_MAX_TRANSCRIPT_SESSIONS,
            max_directory_depth: 32,
        }
    }
}

/// Transcript evidence joined to explicit session-turn ledger identities.
/// Turns remain present even when a transcript is missing or unavailable.
#[derive(Clone)]
pub struct TranscriptInventory {
    /// Explicit ledger turns, preserved even when their transcript is unavailable.
    pub turns: Vec<SessionTurnRow>,
    /// Parsed sessions indexed by the explicit ledger transcript ID.
    pub transcripts: BTreeMap<String, TranscriptSession>,
    /// Whether discovery and every accepted transcript were complete.
    pub complete: bool,
    /// Stable source issue codes; no paths, transcript text, or credentials.
    pub unavailable_reasons: Vec<String>,
}

impl TranscriptInventory {
    /// Produces query values separately for each explicit transcript identity.
    #[implements("FR-047-AC-5")]
    pub fn adapters_by_transcript(
        &self,
        spec: &RoleSpec,
        window: &EvalWindow,
        context: &AdapterContext,
    ) -> BTreeMap<String, BTreeMap<String, QueryValue>> {
        self.transcripts
            .iter()
            .filter(|(_, transcript)| transcript.binding.session_role == spec.role)
            .map(|(transcript_id, transcript)| {
                (
                    transcript_id.clone(),
                    adapters(spec, transcript, window, context),
                )
            })
            .collect()
    }

    /// Ledger-shaped virtual rows for the existing session scorer.
    #[implements("FR-047-AC-5")]
    pub fn ledger_rows(&self) -> Vec<serde_json::Map<String, Value>> {
        self.turns
            .iter()
            .map(SessionTurnRow::as_ledger_row)
            .collect()
    }
}

impl SessionTurnRow {
    /// Converts the virtual row to the keys consumed by session scoring.
    pub fn as_ledger_row(&self) -> serde_json::Map<String, Value> {
        let mut row = serde_json::Map::from_iter([
            (
                "session_addr".to_owned(),
                Value::String(self.session_addr.clone()),
            ),
            (
                "session_role".to_owned(),
                Value::String(self.session_role.clone()),
            ),
            (
                "started_at".to_owned(),
                Value::Number(self.started_at_ms.into()),
            ),
        ]);
        if let Some(transcript_id) = &self.transcript_id {
            row.insert(
                "transcript_id".to_owned(),
                Value::String(transcript_id.clone()),
            );
        }
        row
    }
}

/// Converts rows returned by the existing read-only `session_turn` query into
/// typed transcript inputs. Legacy rows without `transcript_id` are retained
/// with `None`; malformed present IDs become invalid IDs and score unavailable.
#[implements("FR-047-AC-5")]
pub fn turn_rows_from_ledger(rows: &[serde_json::Map<String, Value>]) -> Vec<SessionTurnRow> {
    let mut turns = rows
        .iter()
        .filter_map(|row| {
            Some(SessionTurnRow {
                transcript_id: row
                    .get("transcript_id")
                    .map(|value| value.as_str().unwrap_or_default().to_owned()),
                session_addr: row.get("session_addr")?.as_str()?.to_owned(),
                session_role: row.get("session_role")?.as_str()?.to_owned(),
                started_at_ms: row.get("started_at")?.as_i64()?,
            })
        })
        .collect::<Vec<_>>();
    turns.sort_by(|left, right| {
        (
            &left.session_addr,
            &left.session_role,
            left.started_at_ms,
            &left.transcript_id,
        )
            .cmp(&(
                &right.session_addr,
                &right.session_role,
                right.started_at_ms,
                &right.transcript_id,
            ))
    });
    turns.dedup();
    turns
}

/// Cross-source facts required by checks whose answer depends on external
/// evidence. Missing facts keep the affected query unavailable.
#[derive(Clone, Debug, Default)]
pub struct AdapterContext {
    pub gate_commands: Option<BTreeMap<String, Vec<String>>>,
    pub default_branches: Option<BTreeSet<String>>,
    pub team_worktree_roots: Option<BTreeSet<String>>,
    pub reviewed_worktrees: Option<BTreeSet<String>>,
    pub dispatcher_addresses: Option<BTreeSet<String>>,
    pub worktree_actions: Option<Vec<WorktreeAction>>,
    pub pull_request_repositories: Option<BTreeMap<String, BTreeSet<String>>>,
    pub owner_instruction_times_ms: Option<Vec<i64>>,
    pub answered_action_ids: Option<BTreeSet<String>>,
    pub open_escalate_block_lanes: Option<BTreeSet<String>>,
    pub relay_messages: Option<Vec<RelayMessage>>,
    pub fleet_actions: Option<Vec<FleetAction>>,
    pub dialog_answer_log_ids: Option<BTreeSet<String>>,
    pub fleet_status_checks: Option<Vec<FleetStatusCheck>>,
    pub review_age_limit_hours: u64,
}

/// Relay fact projected by the separate relay adapter. The body is never
/// copied into report evidence.
#[derive(Clone, Debug)]
pub struct RelayMessage {
    pub at_ms: i64,
    pub sender: String,
    pub recipient: String,
    pub body: String,
    pub kind: Option<String>,
    pub session_addr: Option<String>,
    pub lane: Option<String>,
    pub action_id: Option<String>,
    pub is_coder_report: bool,
    pub is_dialog_answer: bool,
    pub has_action_reference: bool,
    pub has_status_evidence: bool,
    pub is_etiquette_reply: Option<bool>,
}

/// Worktree lifecycle fact joined from bounded command events.
#[derive(Clone, Debug)]
pub struct WorktreeAction {
    pub at_ms: i64,
    pub session_addr: String,
    pub repository: String,
    pub ticket: String,
    pub source_path: Option<String>,
    pub target_path: String,
    pub removed: bool,
}

/// Fleet action fact needed to correlate transcript answers to their source.
#[derive(Clone, Debug)]
pub struct FleetAction {
    pub action_id: String,
    pub kind: String,
    pub lane: Option<String>,
    pub emitted_at_ms: i64,
    pub due_by_ms: Option<i64>,
    pub resolved_at_ms: Option<i64>,
    pub standing_order_id: Option<String>,
}

/// Result of a complete `fleet status` probe.
#[derive(Clone, Debug)]
pub struct FleetStatusCheck {
    pub at_ms: i64,
    pub exited_zero: bool,
    pub complete: bool,
}

/// Parses bounded newline-delimited Claude Code or Codex transcript records.
/// A malformed, oversized, mismatched, or untimed record makes the whole
/// transcript incomplete so a partial scan cannot become a passing zero.
#[implements("FR-047-AC-5")]
pub fn parse_lines(
    harness: HarnessFormat,
    binding: SessionBinding,
    lines: &[&[u8]],
    max_bytes: usize,
    max_record_bytes: usize,
) -> TranscriptSession {
    let bytes_read = lines.iter().map(|line| line.len()).sum::<usize>();
    let mut session = TranscriptSession {
        binding,
        events: Vec::new(),
        complete: true,
        unavailable_reason: None,
        bytes_read,
        fleet_status_checks: Vec::new(),
    };
    if lines.is_empty() {
        return incomplete(session, "transcript_empty");
    }
    if bytes_read > max_bytes {
        return incomplete(session, "transcript_size_limit");
    }
    for line in lines {
        if line.len() > max_record_bytes {
            return incomplete(session, "transcript_record_size_limit");
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return incomplete(session, "malformed_transcript_record");
        };
        if transcript_id(&record, harness).is_some_and(|id| id != session.binding.transcript_id) {
            return incomplete(session, "transcript_session_identity_mismatch");
        }
        match parse_record(&record, harness) {
            Ok(events) => session.events.extend(events),
            Err(reason) => return incomplete(session, reason),
        }
    }
    session.events.sort_by_key(|event| event.at_ms);
    session.fleet_status_checks = fleet_status_checks(&session.events);
    session
}

/// Merges hook-attributed session starts with completely read transcripts.
/// This produces rows compatible with the `session_turn` ledger view without
/// writing to the ledger, preserving FR-047's read-only contract.
#[implements("FR-047-AC-5")]
pub fn session_turn_rows(
    _transcripts: &[TranscriptSession],
    hooks: &[HookTurn],
    window: &EvalWindow,
) -> Vec<SessionTurnRow> {
    let mut rows = hooks
        .iter()
        .filter(|hook| hook.started_at_ms >= window.start_ms && hook.started_at_ms < window.end_ms)
        .map(|hook| SessionTurnRow {
            transcript_id: Some(hook.transcript_id.clone()),
            session_addr: hook.session_addr.clone(),
            session_role: hook.session_role.clone(),
            started_at_ms: hook.started_at_ms,
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        (&left.session_addr, left.started_at_ms, &left.transcript_id).cmp(&(
            &right.session_addr,
            right.started_at_ms,
            &right.transcript_id,
        ))
    });
    rows.dedup_by(|left, right| {
        left.session_addr == right.session_addr && left.started_at_ms == right.started_at_ms
    });
    rows
}

/// Discovers and reads transcripts for ledger rows that already contain an explicit
/// transcript ID, address, and role. Claude files are matched by exact `<id>.jsonl`
/// names; Codex files are matched by an exact ID suffix and then validated by the
/// session metadata record. No timestamps, modification times, or nearest-session
/// heuristics are used to bind a transcript.
#[implements("FR-047-AC-5")]
pub fn discover_transcripts(
    home: &Path,
    ledger_rows: &[SessionTurnRow],
    limits: TranscriptReadLimits,
) -> TranscriptInventory {
    let mut turns = ledger_rows.to_vec();
    turns.sort_by(|left, right| {
        (
            &left.session_addr,
            &left.session_role,
            left.started_at_ms,
            &left.transcript_id,
        )
            .cmp(&(
                &right.session_addr,
                &right.session_role,
                right.started_at_ms,
                &right.transcript_id,
            ))
    });
    turns.dedup();

    let mut inventory = TranscriptInventory {
        turns,
        transcripts: BTreeMap::new(),
        complete: true,
        unavailable_reasons: Vec::new(),
    };
    let mut identities = BTreeMap::<String, (String, String, i64, bool)>::new();
    for turn in &inventory.turns {
        let Some(transcript_id) = turn.transcript_id.as_deref() else {
            inventory.complete = false;
            inventory
                .unavailable_reasons
                .push("transcript_id_unavailable".to_owned());
            let unavailable_key = format!(
                "transcript-id-unavailable:{}:{}",
                turn.session_addr, turn.started_at_ms
            );
            inventory
                .transcripts
                .entry(unavailable_key)
                .or_insert_with(|| unavailable_transcript(turn, "transcript_id_unavailable"));
            continue;
        };
        if !valid_transcript_id(transcript_id) {
            inventory.complete = false;
            inventory
                .unavailable_reasons
                .push("invalid_transcript_id".to_owned());
            let unavailable_key = format!(
                "invalid-transcript-id:{}:{}",
                turn.session_addr, turn.started_at_ms
            );
            inventory
                .transcripts
                .entry(unavailable_key)
                .or_insert_with(|| unavailable_transcript(turn, "invalid_transcript_id"));
            continue;
        }
        match identities.get_mut(transcript_id) {
            Some((address, role, started_at, consistent)) => {
                if address != &turn.session_addr || role != &turn.session_role {
                    *consistent = false;
                }
                *started_at = (*started_at).min(turn.started_at_ms);
            }
            None => {
                identities.insert(
                    transcript_id.to_owned(),
                    (
                        turn.session_addr.clone(),
                        turn.session_role.clone(),
                        turn.started_at_ms,
                        true,
                    ),
                );
            }
        }
    }

    let requested = identities
        .iter()
        .filter(|(_, (_, _, _, consistent))| *consistent)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    if identities.len() > limits.max_sessions {
        inventory.complete = false;
        inventory
            .unavailable_reasons
            .push("transcript_session_limit".to_owned());
        for id in requested {
            if let Some((address, role, started_at_ms, _)) = identities.get(&id) {
                let turn = SessionTurnRow {
                    transcript_id: Some(id.clone()),
                    session_addr: address.clone(),
                    session_role: role.clone(),
                    started_at_ms: *started_at_ms,
                };
                inventory.transcripts.insert(
                    id,
                    unavailable_transcript(&turn, "transcript_session_limit"),
                );
            }
        }
        return inventory;
    }

    for (id, (address, role, started_at_ms, consistent)) in &identities {
        if !consistent {
            let turn = SessionTurnRow {
                transcript_id: Some(id.clone()),
                session_addr: address.clone(),
                session_role: role.clone(),
                started_at_ms: *started_at_ms,
            };
            inventory.transcripts.insert(
                id.clone(),
                unavailable_transcript(&turn, "transcript_binding_conflict"),
            );
            inventory.complete = false;
            inventory
                .unavailable_reasons
                .push("transcript_binding_conflict".to_owned());
        }
    }

    if requested.is_empty() {
        inventory.unavailable_reasons.sort_unstable();
        inventory.unavailable_reasons.dedup();
        return inventory;
    }

    let roots = [
        (home.join(".claude/projects"), HarnessFormat::Claude),
        (home.join(".codex/sessions"), HarnessFormat::Codex),
    ];
    let mut candidates = BTreeMap::<String, Vec<(HarnessFormat, PathBuf)>>::new();
    let mut entries_seen = 0usize;
    let mut discovery_error = None;
    for (root, format) in roots {
        if let Err(reason) = collect_candidates(
            &root,
            format,
            &requested,
            0,
            &mut entries_seen,
            limits,
            &mut candidates,
        ) {
            discovery_error = Some(reason);
            break;
        }
    }
    if let Some(reason) = discovery_error {
        inventory.complete = false;
        inventory.unavailable_reasons.push(reason.to_owned());
        for (id, (address, role, started_at_ms, consistent)) in identities {
            if inventory.transcripts.contains_key(&id) {
                continue;
            }
            let turn = SessionTurnRow {
                transcript_id: Some(id.clone()),
                session_addr: address,
                session_role: role,
                started_at_ms,
            };
            let reason = if consistent {
                "transcript_discovery_incomplete"
            } else {
                "transcript_binding_conflict"
            };
            inventory
                .transcripts
                .insert(id, unavailable_transcript(&turn, reason));
        }
        return inventory;
    }

    let mut bytes_used = 0usize;
    for (id, (address, role, started_at_ms, consistent)) in identities {
        if inventory.transcripts.contains_key(&id) {
            continue;
        }
        let turn = SessionTurnRow {
            transcript_id: Some(id.clone()),
            session_addr: address,
            session_role: role,
            started_at_ms,
        };
        if !consistent {
            continue;
        }
        let found = candidates.remove(&id).unwrap_or_default();
        if found.is_empty() {
            inventory.complete = false;
            inventory
                .unavailable_reasons
                .push("transcript_missing".to_owned());
            inventory
                .transcripts
                .insert(id, unavailable_transcript(&turn, "transcript_missing"));
            continue;
        }
        if found.len() != 1 {
            inventory.complete = false;
            inventory
                .unavailable_reasons
                .push("transcript_ambiguous".to_owned());
            inventory
                .transcripts
                .insert(id, unavailable_transcript(&turn, "transcript_ambiguous"));
            continue;
        }
        let (format, path) = &found[0];
        let parsed = match read_bounded_lines(path, &mut bytes_used, limits) {
            Ok(lines) => parse_lines(
                *format,
                SessionBinding {
                    transcript_id: id.clone(),
                    session_addr: turn.session_addr.clone(),
                    session_role: turn.session_role.clone(),
                    started_at_ms: turn.started_at_ms,
                },
                &lines.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                limits.max_transcript_bytes,
                limits.max_record_bytes,
            ),
            Err(reason) => unavailable_transcript(&turn, reason),
        };
        if !parsed.complete {
            inventory.complete = false;
            if let Some(reason) = parsed.unavailable_reason.as_ref() {
                inventory.unavailable_reasons.push(reason.clone());
            }
        }
        inventory.transcripts.insert(id, parsed);
    }
    inventory.unavailable_reasons.sort_unstable();
    inventory.unavailable_reasons.dedup();
    inventory
}

#[implements("FR-047-AC-5")]
fn valid_transcript_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 512
        && !id.contains(['/', '\\', '\0'])
        && id.bytes().all(|byte| !byte.is_ascii_control())
}

#[implements("FR-047-AC-5")]
fn unavailable_transcript(turn: &SessionTurnRow, reason: &'static str) -> TranscriptSession {
    incomplete(
        TranscriptSession {
            binding: SessionBinding {
                transcript_id: turn.transcript_id.clone().unwrap_or_default(),
                session_addr: turn.session_addr.clone(),
                session_role: turn.session_role.clone(),
                started_at_ms: turn.started_at_ms,
            },
            events: Vec::new(),
            complete: false,
            unavailable_reason: None,
            bytes_read: 0,
            fleet_status_checks: Vec::new(),
        },
        reason,
    )
}

#[implements("FR-047-AC-5")]
fn collect_candidates(
    directory: &Path,
    format: HarnessFormat,
    requested: &BTreeSet<String>,
    depth: usize,
    entries_seen: &mut usize,
    limits: TranscriptReadLimits,
    candidates: &mut BTreeMap<String, Vec<(HarnessFormat, PathBuf)>>,
) -> Result<(), &'static str> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("transcript_discovery_unreadable"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("transcript_root_invalid");
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("transcript_discovery_unreadable"),
    };
    if depth > limits.max_directory_depth {
        return Err("transcript_discovery_depth_limit");
    }
    for entry in entries {
        *entries_seen = entries_seen.saturating_add(1);
        if *entries_seen > limits.max_discovery_entries {
            return Err("transcript_discovery_entry_limit");
        }
        let entry = entry.map_err(|_| "transcript_discovery_unreadable")?;
        let file_type = entry
            .file_type()
            .map_err(|_| "transcript_discovery_unreadable")?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_candidates(
                &entry.path(),
                format,
                requested,
                depth + 1,
                entries_seen,
                limits,
                candidates,
            )?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        for id in requested {
            let matches = match format {
                HarnessFormat::Claude => file_name == format!("{id}.jsonl"),
                HarnessFormat::Codex => codex_filename_matches(file_name, id),
            };
            if matches {
                candidates
                    .entry(id.clone())
                    .or_default()
                    .push((format, path.clone()));
            }
        }
    }
    Ok(())
}

#[implements("FR-047-AC-5")]
fn codex_filename_matches(file_name: &str, id: &str) -> bool {
    let Some(stem) = file_name.strip_suffix(".jsonl") else {
        return false;
    };
    stem.strip_suffix(id)
        .is_some_and(|prefix| prefix.ends_with('-'))
}

#[implements("FR-047-AC-5")]
fn read_bounded_lines(
    path: &Path,
    bytes_used: &mut usize,
    limits: TranscriptReadLimits,
) -> Result<Vec<Vec<u8>>, &'static str> {
    let before = fs::metadata(path).map_err(|_| "transcript_unreadable")?;
    let file_len = usize::try_from(before.len()).map_err(|_| "transcript_size_limit")?;
    if file_len == 0 {
        return Err("transcript_empty");
    }
    if file_len > limits.max_transcript_bytes {
        return Err("transcript_size_limit");
    }
    let next_used = bytes_used
        .checked_add(file_len)
        .ok_or("transcript_collection_size_limit")?;
    if next_used > limits.max_collection_bytes {
        return Err("transcript_collection_size_limit");
    }
    let file = File::open(path).map_err(|_| "transcript_unreadable")?;
    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut bytes_read = 0usize;
    loop {
        let available = reader.fill_buf().map_err(|_| "transcript_unreadable")?;
        if available.is_empty() {
            break;
        }
        let chunk_len = available.len();
        let mut start = 0usize;
        while start < chunk_len {
            let end = available[start..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(chunk_len, |offset| start + offset + 1);
            let piece = &available[start..end];
            bytes_read = bytes_read
                .checked_add(piece.len())
                .ok_or("transcript_size_limit")?;
            if bytes_read > limits.max_transcript_bytes {
                return Err("transcript_size_limit");
            }
            let content_len = piece
                .last()
                .filter(|byte| **byte == b'\n')
                .map_or(piece.len(), |_| piece.len().saturating_sub(1));
            if record.len().saturating_add(content_len) > limits.max_record_bytes {
                return Err("transcript_record_size_limit");
            }
            record.extend_from_slice(&piece[..content_len]);
            if content_len < piece.len() {
                records.push(std::mem::take(&mut record));
            }
            start = end;
        }
        reader.consume(chunk_len);
    }
    if !record.is_empty() {
        records.push(record);
    }
    if bytes_read != file_len {
        return Err("transcript_changed_during_read");
    }
    let after = fs::metadata(path).map_err(|_| "transcript_unreadable")?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err("transcript_changed_during_read");
    }
    *bytes_used = next_used;
    Ok(records)
}

/// Produces transcript-backed deterministic query values for one role.
/// Incomplete transcripts return unavailable for every query they might feed.
#[implements("FR-047-AC-5")]
pub fn adapters(
    spec: &RoleSpec,
    session: &TranscriptSession,
    window: &EvalWindow,
    context: &AdapterContext,
) -> BTreeMap<String, QueryValue> {
    let queries = spec
        .rules
        .iter()
        .filter(|rule| rule.method == "deterministic")
        .map(|rule| rule.query_or_rubric.as_str())
        .filter(|query| query.starts_with("transcript."))
        .collect::<BTreeSet<_>>();
    queries
        .into_iter()
        .map(|query| {
            let value = if !session.complete {
                unavailable(
                    query,
                    session
                        .unavailable_reason
                        .as_deref()
                        .unwrap_or("transcript unavailable"),
                )
            } else if session.binding.session_role != spec.role {
                unavailable(
                    query,
                    "transcript role does not match explicit hook attribution",
                )
            } else {
                evaluate(query, session, window, context)
            };
            (query.to_owned(), value)
        })
        .collect()
}

fn evaluate(
    query: &str,
    session: &TranscriptSession,
    window: &EvalWindow,
    context: &AdapterContext,
) -> QueryValue {
    let events = session
        .events
        .iter()
        .filter(|event| event.at_ms >= window.start_ms && event.at_ms < window.end_ms)
        .collect::<Vec<_>>();
    if query == "transcript.coder_aggregate_gate_runs"
        && context.gate_commands.is_none()
        && events.iter().any(|event| event.command.is_some())
    {
        return unavailable(
            query,
            "repository gate-command configuration is unavailable",
        );
    }
    if query == "transcript.coder_merge_commands"
        && context.default_branches.is_none()
        && events.iter().any(|event| {
            event
                .command
                .as_deref()
                .is_some_and(|command| command_has(command, "git push"))
        })
    {
        return unavailable(
            query,
            "default-branch configuration is unavailable for a push command",
        );
    }
    let count = match query {
        "transcript.coder_aggregate_gate_runs" => events
            .iter()
            .filter(|event| is_aggregate_gate(event, context))
            .count(),
        "transcript.coder_merge_commands" => events
            .iter()
            .filter(|event| {
                event.command.as_deref().is_some_and(|command| {
                    command_has(command, "gh pr merge")
                        || context.default_branches.as_ref().is_some_and(|branches| {
                            branches
                                .iter()
                                .any(|branch| push_targets_branch(command, branch))
                        })
                })
            })
            .count(),
        "transcript.reviewer_branch_commits" => {
            let Some(worktrees) = context.reviewed_worktrees.as_ref() else {
                return unavailable(query, "reviewed worktree attribution is unavailable");
            };
            events
                .iter()
                .filter(|event| {
                    let Some(command) = event.command.as_deref() else {
                        return false;
                    };
                    (command_has(command, "git commit") || command_has(command, "git push"))
                        && event
                            .cwd
                            .as_deref()
                            .is_some_and(|cwd| worktrees.iter().any(|tree| cwd.starts_with(tree)))
                })
                .count()
        }
        "transcript.reviewer_github_posts_beyond_ticket_id" => events
            .iter()
            .filter(|event| event.command.as_deref().is_some_and(is_non_id_github_post))
            .count(),
        "transcript.planner_code_or_merge_actions" => {
            let Some(roots) = context.team_worktree_roots.as_ref() else {
                return unavailable(query, "team worktree roots are unavailable");
            };
            events
                .iter()
                .filter(|event| {
                    event.path.as_deref().is_some_and(|path| {
                        roots.iter().any(|root| path.starts_with(root)) && is_non_markdown(path)
                    }) || event
                        .command
                        .as_deref()
                        .is_some_and(|command| command_has(command, "gh pr merge"))
                        || event.body.as_deref().is_some_and(|body| {
                            body.contains("skills/coder") || body.contains("/coder")
                        })
                })
                .count()
        }
        "transcript.lead_authored_edits" => {
            let Some(roots) = context.team_worktree_roots.as_ref() else {
                return unavailable(query, "team worktree roots are unavailable");
            };
            events
                .iter()
                .filter(|event| {
                    matches!(event.kind, TranscriptEventKind::ToolCall)
                        && event.path.as_deref().is_some_and(|path| {
                            roots.iter().any(|root| path.starts_with(root)) && is_non_markdown(path)
                        })
                        && event
                            .path
                            .as_deref()
                            .is_some_and(|path| !path.split('/').any(|part| part == "reviews"))
                })
                .count()
        }
        "transcript.lead_extra_trees" | "transcript.coder_extra_trees" => {
            let Some(actions) = context.worktree_actions.as_ref() else {
                return unavailable(
                    query,
                    "worktree lifecycle and pull-request repository joins are unavailable",
                );
            };
            if query == "transcript.coder_extra_trees"
                && context.pull_request_repositories.is_none()
            {
                return unavailable(
                    query,
                    "session pull-request repository population is unavailable",
                );
            }
            worktree_violations(query, actions, &session.binding.session_addr, context)
        }
        "transcript.coder_unrequested_pushes" => {
            let Some(messages) = context.relay_messages.as_ref() else {
                return unavailable(
                    query,
                    "relay dispatch and coder-report population is unavailable",
                );
            };
            let Some(dispatchers) = context.dispatcher_addresses.as_ref() else {
                return unavailable(query, "dispatcher identity population is unavailable");
            };
            let Some(branches) = context.default_branches.as_ref() else {
                return unavailable(query, "default-branch configuration is unavailable");
            };
            if dispatchers.is_empty() || branches.is_empty() {
                return unavailable(
                    query,
                    "dispatcher identity or default-branch population is empty",
                );
            }
            unrequested_pushes(&events, messages, dispatchers, branches)
        }
        "transcript.review_passes_over_age_limit" => {
            let Some(messages) = context.relay_messages.as_ref() else {
                return unavailable(query, "review dispatch messages are unavailable");
            };
            let Some(dispatchers) = context.dispatcher_addresses.as_ref() else {
                return unavailable(query, "reviewer dispatcher identity is unavailable");
            };
            if dispatchers.is_empty() {
                return unavailable(query, "reviewer dispatcher identity population is empty");
            }
            let age_limit_ms = context.review_age_limit_hours.saturating_mul(3_600_000) as i64;
            review_passes_over_limit(
                &events,
                messages,
                dispatchers,
                &session.binding.session_addr,
                age_limit_ms,
            )
        }
        "transcript.in_target_claims_without_complete_status" => {
            let Some(statuses) = context.fleet_status_checks.as_ref() else {
                return unavailable(query, "fleet status population is unavailable");
            };
            claims_without_complete_status(&events, statuses)
        }
        "transcript.ownership_answers_without_lane_address" => {
            let Some(relay) = context.relay_messages.as_ref() else {
                return unavailable(
                    query,
                    "orchestrator outbound answer population is unavailable",
                );
            };
            ownership_answers_missing_address(&events, relay, &session.binding.session_addr, window)
        }
        "transcript.orchestrator_loop_removed_without_owner_instruction"
        | "transcript.minder_loop_removed_without_owner_instruction" => {
            let Some(owner_instructions) = context.owner_instruction_times_ms.as_ref() else {
                return unavailable(query, "owner-instruction history is unavailable");
            };
            loop_removals_without_instruction(&events, owner_instructions)
        }
        "transcript.minder_own_probe_commands" => events
            .iter()
            .filter(|event| event.command.as_deref().is_some_and(is_own_probe))
            .count(),
        "transcript.dialog_answers_missing_from_answer_log" => {
            let Some(relay) = context.relay_messages.as_ref() else {
                return unavailable(query, "dialog-answer send population is unavailable");
            };
            let Some(answer_log) = context.dialog_answer_log_ids.as_ref() else {
                return unavailable(query, "dialog answer log is unavailable");
            };
            relay
                .iter()
                .filter(|message| {
                    message.is_dialog_answer
                        && message.session_addr.as_deref()
                            == Some(session.binding.session_addr.as_str())
                        && message.at_ms >= window.start_ms
                        && message.at_ms < window.end_ms
                        && message
                            .action_id
                            .as_ref()
                            .is_none_or(|action_id| !answer_log.contains(action_id))
                })
                .count()
        }
        "transcript.answers_sent_while_escalate_block_open" => {
            let Some(relay) = context.relay_messages.as_ref() else {
                return unavailable(query, "dialog-answer send population is unavailable");
            };
            let Some(actions) = context.fleet_actions.as_ref() else {
                return unavailable(query, "escalate_block action intervals are unavailable");
            };
            answers_during_open_escalations(relay, actions, &session.binding.session_addr, window)
        }
        "transcript.escalations_without_action_reference" => {
            let Some(relay) = context.relay_messages.as_ref() else {
                return unavailable(query, "escalation send population is unavailable");
            };
            relay
                .iter()
                .filter(|message| {
                    message.session_addr.as_deref() == Some(session.binding.session_addr.as_str())
                        && message.at_ms >= window.start_ms
                        && message.at_ms < window.end_ms
                        && is_escalation(message)
                        && (!message.has_action_reference || !message.has_status_evidence)
                })
                .count()
        }
        "transcript.orchestrator_messages_outside_allowed_kinds" => {
            let Some(relay) = context.relay_messages.as_ref() else {
                return unavailable(query, "orchestrator relay send population is unavailable");
            };
            if relay.iter().any(|message| {
                message.session_addr.as_deref() == Some(session.binding.session_addr.as_str())
                    && message.at_ms >= window.start_ms
                    && message.at_ms < window.end_ms
                    && message.kind.is_none()
            }) {
                return unavailable(query, "relay message kind is absent for an in-window send");
            }
            relay
                .iter()
                .filter(|message| {
                    message.session_addr.as_deref() == Some(session.binding.session_addr.as_str())
                        && message.at_ms >= window.start_ms
                        && message.at_ms < window.end_ms
                        && !matches!(
                            message.kind.as_deref(),
                            Some("done" | "blocked" | "decision" | "handoff")
                        )
                })
                .count()
        }
        "transcript.etiquette_replies_per_sender_hour_max" => {
            let Some(relay) = context.relay_messages.as_ref() else {
                return unavailable(query, "orchestrator relay send population is unavailable");
            };
            match max_etiquette_replies(relay, &session.binding.session_addr, window) {
                Ok(value) => value,
                Err(reason) => return unavailable(query, reason),
            }
        }
        "transcript.fleet_health_claims"
        | "transcript.owner_ownership_questions"
        | "transcript.orchestrator_loop_removal_events"
        | "transcript.orchestrator_outbound_messages" => {
            return unavailable(
                query,
                "query is judgement-only or a population trigger, not a numeric rule",
            );
        }
        _ => return unavailable(query, "no transcript query producer is registered"),
    };
    QueryValue::Value {
        value: count as f64,
        evidence_refs: evidence_refs(query, session, &events, window, context),
    }
}

fn evidence_refs(
    query: &str,
    session: &TranscriptSession,
    events: &[&TranscriptEvent],
    window: &EvalWindow,
    context: &AdapterContext,
) -> Vec<String> {
    let mut refs = events
        .iter()
        .filter(|event| event_hits_query(query, event, context))
        .map(|event| {
            format!(
                "transcript:{}:{}",
                session.binding.transcript_id, event.at_ms
            )
        })
        .collect::<Vec<_>>();
    if let Some(messages) = context.relay_messages.as_ref() {
        refs.extend(
            messages
                .iter()
                .filter(|message| {
                    message.session_addr.as_deref() == Some(session.binding.session_addr.as_str())
                        && message.at_ms >= window.start_ms
                        && message.at_ms < window.end_ms
                        && relay_message_hits_query(query, message)
                })
                .map(|message| format!("relay:{}:{}", message.sender, message.at_ms)),
        );
    }
    if let Some(actions) = context.fleet_actions.as_ref() {
        refs.extend(
            actions
                .iter()
                .filter(|action| {
                    action.emitted_at_ms < window.end_ms
                        && action.resolved_at_ms.unwrap_or(i64::MAX) >= window.start_ms
                        && (query == "transcript.answers_sent_while_escalate_block_open"
                            && action.kind == "escalate_block")
                })
                .map(|action| format!("fleet-action:{}", action.action_id)),
        );
    }
    if let Some(checks) = context.fleet_status_checks.as_ref()
        && query == "transcript.in_target_claims_without_complete_status"
    {
        refs.extend(
            checks
                .iter()
                .filter(|check| check.at_ms <= window.end_ms)
                .map(|check| format!("fleet-status:{}", check.at_ms)),
        );
    }
    if refs.is_empty() {
        refs.push(format!(
            "transcript-session:{}",
            session.binding.transcript_id
        ));
    }
    refs.sort();
    refs.dedup();
    refs
}

fn relay_message_hits_query(query: &str, message: &RelayMessage) -> bool {
    match query {
        "transcript.coder_unrequested_pushes" => message.is_coder_report,
        "transcript.dialog_answers_missing_from_answer_log"
        | "transcript.answers_sent_while_escalate_block_open" => message.is_dialog_answer,
        "transcript.escalations_without_action_reference" => is_escalation(message),
        "transcript.orchestrator_messages_outside_allowed_kinds" => !matches!(
            message.kind.as_deref(),
            Some("done" | "blocked" | "decision" | "handoff")
        ),
        "transcript.etiquette_replies_per_sender_hour_max" => {
            message.is_etiquette_reply == Some(true)
        }
        "transcript.ownership_answers_without_lane_address" => true,
        _ => false,
    }
}

fn event_hits_query(query: &str, event: &TranscriptEvent, context: &AdapterContext) -> bool {
    match query {
        "transcript.coder_aggregate_gate_runs" => is_aggregate_gate(event, context),
        "transcript.coder_merge_commands" => event.command.as_deref().is_some_and(|command| {
            command_has(command, "gh pr merge")
                || context.default_branches.as_ref().is_some_and(|branches| {
                    branches
                        .iter()
                        .any(|branch| push_targets_branch(command, branch))
                })
        }),
        "transcript.reviewer_branch_commits" => event.command.as_deref().is_some_and(|command| {
            command_has(command, "git commit") || command_has(command, "git push")
        }),
        "transcript.reviewer_github_posts_beyond_ticket_id" => {
            event.command.as_deref().is_some_and(is_non_id_github_post)
        }
        "transcript.coder_unrequested_pushes" => event
            .command
            .as_deref()
            .is_some_and(|command| command_has(command, "git push")),
        "transcript.review_passes_over_age_limit" => event
            .command
            .as_deref()
            .is_some_and(|command| command_has(command, "linear issue comment add")),
        "transcript.planner_code_or_merge_actions" => {
            event.path.as_deref().is_some_and(is_non_markdown)
                || event
                    .command
                    .as_deref()
                    .is_some_and(|command| command_has(command, "gh pr merge"))
                || event
                    .body
                    .as_deref()
                    .is_some_and(|body| body.contains("skills/coder") || body.contains("/coder"))
        }
        "transcript.lead_authored_edits" => event.path.as_deref().is_some_and(is_non_markdown),
        "transcript.in_target_claims_without_complete_status" => {
            event.body.as_deref().is_some_and(|body| {
                let lower = body.to_ascii_lowercase();
                ["in target", "within target", "healthy", "saturated"]
                    .iter()
                    .any(|phrase| lower.contains(phrase))
                    && ["host", "lane", "fleet", "worker"]
                        .iter()
                        .any(|noun| lower.contains(noun))
            })
        }
        "transcript.ownership_answers_without_lane_address" => event
            .body
            .as_deref()
            .is_some_and(|body| body.to_ascii_lowercase().contains("who owns")),
        "transcript.orchestrator_loop_removed_without_owner_instruction"
        | "transcript.minder_loop_removed_without_owner_instruction" => {
            event_is_loop_removal(event)
        }
        "transcript.minder_own_probe_commands" => {
            event.command.as_deref().is_some_and(is_own_probe)
        }
        "transcript.lead_extra_trees" | "transcript.coder_extra_trees" => {
            event.command.as_deref().is_some_and(|command| {
                command_has(command, "git worktree add") || command_has(command, "git clone")
            })
        }
        _ => false,
    }
}

fn worktree_violations(
    query: &str,
    actions: &[WorktreeAction],
    session_addr: &str,
    context: &AdapterContext,
) -> usize {
    match query {
        "transcript.coder_extra_trees" => {
            let Some(repositories) = context.pull_request_repositories.as_ref() else {
                return 0;
            };
            let Some(pushed_repositories) = repositories.get(session_addr) else {
                return 0;
            };
            actions
                .iter()
                .filter(|action| {
                    action.session_addr == session_addr
                        && !action.removed
                        && pushed_repositories.contains(&action.repository)
                        && action.source_path.as_deref() != Some(action.target_path.as_str())
                })
                .count()
        }
        "transcript.lead_extra_trees" => {
            let mut actions = actions.to_vec();
            actions.sort_by_key(|action| action.at_ms);
            let mut open = BTreeMap::<(String, String), BTreeSet<String>>::new();
            let mut violations = 0;
            for action in actions {
                let key = (action.repository.clone(), action.ticket.clone());
                let paths = open.entry(key).or_default();
                if action.removed {
                    paths.remove(&action.target_path);
                } else {
                    if !paths.is_empty() && action.session_addr == session_addr {
                        violations += 1;
                    }
                    paths.insert(action.target_path);
                }
            }
            violations
        }
        _ => 0,
    }
}

fn unrequested_pushes(
    events: &[&TranscriptEvent],
    relay: &[RelayMessage],
    dispatcher_addresses: &BTreeSet<String>,
    default_branches: &BTreeSet<String>,
) -> usize {
    events
        .iter()
        .filter_map(|event| {
            let command = event.command.as_deref()?;
            if !command_has(command, "git push")
                || default_branches
                    .iter()
                    .any(|branch| push_targets_branch(command, branch))
            {
                return None;
            }
            let latest_report = relay
                .iter()
                .filter(|message| message.is_coder_report && message.at_ms <= event.at_ms)
                .map(|message| message.at_ms)
                .max()?;
            let dispatcher_after_report = relay.iter().any(|message| {
                dispatcher_addresses.contains(&message.sender)
                    && message.at_ms > latest_report
                    && message.at_ms <= event.at_ms
            });
            (!dispatcher_after_report).then_some(event.at_ms)
        })
        .count()
}

fn review_passes_over_limit(
    events: &[&TranscriptEvent],
    relay: &[RelayMessage],
    dispatcher_addresses: &BTreeSet<String>,
    session_addr: &str,
    age_limit_ms: i64,
) -> usize {
    let mut dispatches = relay
        .iter()
        .filter(|message| {
            message.recipient == session_addr
                && dispatcher_addresses.contains(&message.sender)
                && message.kind.as_deref() == Some("handoff")
        })
        .map(|message| message.at_ms)
        .collect::<Vec<_>>();
    dispatches.sort_unstable();
    dispatches
        .iter()
        .enumerate()
        .filter(|(index, dispatch_at)| {
            let next_dispatch = dispatches.get(index + 1).copied().unwrap_or(i64::MAX);
            events
                .iter()
                .filter(|event| event.at_ms >= **dispatch_at && event.at_ms < next_dispatch)
                .filter_map(|event| event.command.as_deref().map(|command| (event, command)))
                .find(|(_, command)| command_has(command, "linear issue comment add"))
                .is_some_and(|(event, _)| event.at_ms > **dispatch_at + age_limit_ms)
        })
        .count()
}

fn claims_without_complete_status(
    events: &[&TranscriptEvent],
    checks: &[FleetStatusCheck],
) -> usize {
    events
        .iter()
        .filter(|event| {
            let Some(body) = event.body.as_deref() else {
                return false;
            };
            let lower = body.to_ascii_lowercase();
            let makes_claim = ["in target", "within target", "healthy", "saturated"]
                .iter()
                .any(|phrase| lower.contains(phrase))
                && ["host", "lane", "fleet", "worker"]
                    .iter()
                    .any(|noun| lower.contains(noun));
            makes_claim
                && !checks
                    .iter()
                    .any(|check| check.at_ms <= event.at_ms && check.exited_zero && check.complete)
        })
        .count()
}

fn ownership_answers_missing_address(
    events: &[&TranscriptEvent],
    relay: &[RelayMessage],
    session_addr: &str,
    window: &EvalWindow,
) -> usize {
    let questions = events
        .iter()
        .filter(|event| {
            event.kind == TranscriptEventKind::IncomingMessage
                && event
                    .body
                    .as_deref()
                    .is_some_and(|body| body.to_ascii_lowercase().contains("who owns"))
        })
        .collect::<Vec<_>>();
    questions
        .iter()
        .enumerate()
        .filter(|(index, question)| {
            let next_question = questions
                .get(index + 1)
                .map(|event| event.at_ms)
                .unwrap_or(i64::MAX);
            let answer = relay
                .iter()
                .filter(|message| {
                    message.session_addr.as_deref() == Some(session_addr)
                        && message.at_ms > question.at_ms
                        && message.at_ms < next_question
                        && message.at_ms >= window.start_ms
                        && message.at_ms < window.end_ms
                })
                .min_by_key(|message| message.at_ms);
            answer.is_some_and(|message| !contains_lane_address(&message.body))
        })
        .count()
}

fn contains_lane_address(text: &str) -> bool {
    text.split(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'' || ch == '`')
        .any(|candidate| candidate.matches(':').count() >= 2)
}

fn loop_removals_without_instruction(
    events: &[&TranscriptEvent],
    owner_instruction_times_ms: &[i64],
) -> usize {
    events
        .iter()
        .filter(|event| {
            event_is_loop_removal(event)
                && !owner_instruction_times_ms
                    .iter()
                    .any(|instruction_at| *instruction_at <= event.at_ms)
        })
        .count()
}

fn event_is_loop_removal(event: &TranscriptEvent) -> bool {
    let command_removal = event.command.as_deref().is_some_and(|command| {
        [
            "crontab -r",
            "launchctl bootout",
            "launchctl unload",
            "systemctl disable",
            "systemctl stop",
            "herdr schedule remove",
        ]
        .iter()
        .any(|needle| command.contains(needle))
    });
    let path_removal = event.path.as_deref().is_some_and(|path| {
        (path.contains("launchd") || path.contains("cron") || path.contains("schedule"))
            && event
                .tool
                .as_deref()
                .is_some_and(|tool| tool == "Edit" || tool == "Write")
    });
    command_removal || path_removal
}

fn is_own_probe(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let probe = [
        "ssh ",
        "curl ",
        "herdr agent list",
        "agent-msg who",
        "agent-msg find",
        "uptime",
        " ps ",
        " top ",
        "free -",
        "df -",
        "kubectl get",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    probe && !lower.contains("ix-board fleet tick") && !lower.contains("ix-board fleet status")
}

fn answers_during_open_escalations(
    relay: &[RelayMessage],
    actions: &[FleetAction],
    session_addr: &str,
    window: &EvalWindow,
) -> usize {
    relay
        .iter()
        .filter(|message| {
            message.is_dialog_answer
                && message.session_addr.as_deref() == Some(session_addr)
                && message.at_ms >= window.start_ms
                && message.at_ms < window.end_ms
                && actions.iter().any(|action| {
                    action.kind == "escalate_block"
                        && action.lane.as_deref() == message.lane.as_deref()
                        && action.emitted_at_ms <= message.at_ms
                        && action
                            .resolved_at_ms
                            .or(action.due_by_ms)
                            .is_none_or(|end| message.at_ms < end)
                })
        })
        .count()
}

fn is_escalation(message: &RelayMessage) -> bool {
    message.kind.as_deref() == Some("escalation")
        || message.body.to_ascii_lowercase().contains("escalate")
}

fn max_etiquette_replies(
    relay: &[RelayMessage],
    session_addr: &str,
    window: &EvalWindow,
) -> Result<usize, &'static str> {
    let messages = relay
        .iter()
        .filter(|message| {
            message.session_addr.as_deref() == Some(session_addr)
                && message.at_ms >= window.start_ms
                && message.at_ms < window.end_ms
        })
        .collect::<Vec<_>>();
    if messages
        .iter()
        .any(|message| message.is_etiquette_reply.is_none())
    {
        return Err("etiquette reply classification is incomplete");
    }
    let mut counts = BTreeMap::<(String, i64), usize>::new();
    for message in messages
        .into_iter()
        .filter(|message| message.is_etiquette_reply == Some(true))
    {
        let hour = message.at_ms.div_euclid(3_600_000);
        *counts.entry((message.sender.clone(), hour)).or_default() += 1;
    }
    Ok(counts.values().copied().max().unwrap_or(0))
}

fn is_aggregate_gate(event: &TranscriptEvent, context: &AdapterContext) -> bool {
    event.command.as_deref().is_some_and(|command| {
        command_has(command, "team-gate gate")
            || context.gate_commands.as_ref().is_some_and(|commands| {
                commands
                    .values()
                    .any(|parts| command_starts_with_tokens(command, parts))
            })
    })
}

fn is_non_id_github_post(command: &str) -> bool {
    let writes_comment_or_review = command_has(command, "gh pr comment")
        || command_has(command, "gh pr review")
        || command_has(command, "gh issue comment")
        || (command_has(command, "gh api")
            && (command.contains("/comments") || command.contains("/reviews"))
            && (command.contains("--method POST") || command.contains("-X POST")));
    writes_comment_or_review
        && !(command_has(command, "gh pr comment") && tracker_key_only(command))
}

fn tracker_key_only(command: &str) -> bool {
    let Some((_, body)) = command.split_once("--body") else {
        return false;
    };
    let body = body.trim_start_matches(['=', ' ', '\'', '"']);
    let key = body
        .split(|ch: char| ch.is_whitespace() || ch == '\'' || ch == '"')
        .next()
        .unwrap_or_default();
    key.len() >= 4
        && key
            .split_once('-')
            .is_some_and(|(team, id)| !team.is_empty() && id.chars().all(|ch| ch.is_ascii_digit()))
}

fn push_targets_branch(command: &str, branch: &str) -> bool {
    command_has(command, "git push") && command.split_whitespace().any(|part| part == branch)
}

fn command_starts_with_tokens(command: &str, tokens: &[String]) -> bool {
    let command = command.trim();
    let joined = tokens.join(" ");
    !joined.is_empty() && command.contains(&joined)
}

fn command_has(command: &str, pattern: &str) -> bool {
    command.contains(pattern)
}

fn is_non_markdown(path: &str) -> bool {
    let lowered = path.to_ascii_lowercase();
    !lowered.ends_with(".md") && !lowered.ends_with(".mdx")
}

fn unavailable(query: &str, reason: &str) -> QueryValue {
    QueryValue::Unavailable {
        reason: format!("{query}: {reason}"),
        evidence_refs: Vec::new(),
    }
}

fn incomplete(mut session: TranscriptSession, reason: &str) -> TranscriptSession {
    session.events.clear();
    session.complete = false;
    session.unavailable_reason = Some(reason.to_owned());
    session
}

fn parse_record(
    record: &Value,
    harness: HarnessFormat,
) -> Result<Vec<TranscriptEvent>, &'static str> {
    match harness {
        HarnessFormat::Claude => parse_claude_record(record),
        HarnessFormat::Codex => parse_codex_record(record),
    }
}

fn parse_claude_record(record: &Value) -> Result<Vec<TranscriptEvent>, &'static str> {
    let Some(kind) = record.get("type").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let Some(at_ms) = timestamp_ms(record) else {
        return if matches!(kind, "user" | "assistant") {
            Err("transcript_event_missing_timestamp")
        } else {
            Ok(Vec::new())
        };
    };
    match kind {
        "assistant" => {
            let mut events = Vec::new();
            let content = record
                .get("message")
                .and_then(|message| message.get("content"));
            for block in content.and_then(Value::as_array).into_iter().flatten() {
                if block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && let Some(event) = claude_tool_event(block, at_ms)
                {
                    events.push(event);
                }
            }
            if let Some(body) = extract_message(content) {
                events.push(TranscriptEvent {
                    at_ms,
                    kind: TranscriptEventKind::AssistantMessage,
                    tool: None,
                    tool_call_id: None,
                    command: None,
                    cwd: record.get("cwd").and_then(Value::as_str).map(str::to_owned),
                    path: None,
                    recipient: None,
                    exit_code: None,
                    status_complete: None,
                    body: Some(body),
                });
            }
            Ok(events)
        }
        "user" => {
            let content = record
                .get("message")
                .and_then(|message| message.get("content"));
            let mut events = Vec::new();
            if let Some(body) = extract_message(content) {
                events.push(TranscriptEvent {
                    at_ms,
                    kind: TranscriptEventKind::IncomingMessage,
                    tool: None,
                    tool_call_id: None,
                    command: None,
                    cwd: record.get("cwd").and_then(Value::as_str).map(str::to_owned),
                    path: None,
                    recipient: None,
                    exit_code: None,
                    status_complete: None,
                    body: Some(body),
                });
            }
            for block in content.and_then(Value::as_array).into_iter().flatten() {
                if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                    continue;
                }
                let result = extract_message(block.get("content"));
                events.push(TranscriptEvent {
                    at_ms,
                    kind: TranscriptEventKind::ToolResult,
                    tool: None,
                    tool_call_id: block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    command: None,
                    cwd: None,
                    path: None,
                    recipient: None,
                    exit_code: Some(
                        if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                            1
                        } else {
                            0
                        },
                    ),
                    status_complete: result.as_deref().map(status_complete_marker),
                    body: result,
                });
            }
            Ok(events)
        }
        _ => Ok(Vec::new()),
    }
}

fn claude_tool_event(block: &Value, at_ms: i64) -> Option<TranscriptEvent> {
    let tool = block.get("name")?.as_str()?.to_owned();
    let input = block.get("input")?;
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let path = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let body = input
        .get("prompt")
        .or_else(|| input.get("content"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(TranscriptEvent {
        at_ms,
        kind: TranscriptEventKind::ToolCall,
        tool: Some(tool),
        tool_call_id: block.get("id").and_then(Value::as_str).map(str::to_owned),
        command,
        cwd: None,
        path,
        recipient: input
            .get("recipient")
            .or_else(|| input.get("to"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        exit_code: None,
        status_complete: None,
        body,
    })
}

fn parse_codex_record(record: &Value) -> Result<Vec<TranscriptEvent>, &'static str> {
    let record_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if record_type == "event_msg"
        && record.pointer("/payload/type").and_then(Value::as_str) == Some("item_completed")
        && record.pointer("/payload/item/type").and_then(Value::as_str) == Some("CommandExecution")
    {
        let item = record
            .pointer("/payload/item")
            .ok_or("transcript_command_missing_item")?;
        let at_ms = item
            .get("completed_at_ms")
            .and_then(Value::as_i64)
            .or_else(|| timestamp_ms(record))
            .ok_or("transcript_command_missing_timestamp")?;
        return Ok(vec![TranscriptEvent {
            at_ms,
            kind: TranscriptEventKind::ToolCall,
            tool: Some("CommandExecution".to_owned()),
            tool_call_id: item.get("id").and_then(Value::as_str).map(str::to_owned),
            command: item
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned),
            cwd: item.get("cwd").and_then(Value::as_str).map(str::to_owned),
            path: None,
            recipient: None,
            exit_code: item
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|code| i32::try_from(code).ok()),
            status_complete: item
                .get("aggregated_output")
                .and_then(Value::as_str)
                .map(status_complete_marker),
            body: None,
        }]);
    }
    if record_type == "response_item"
        && record.pointer("/payload/type").and_then(Value::as_str) == Some("function_call")
    {
        let name = record
            .pointer("/payload/name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = record
            .pointer("/payload/arguments")
            .and_then(Value::as_str)
            .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok());
        let at_ms = timestamp_ms(record).ok_or("transcript_tool_call_missing_timestamp")?;
        return Ok(arguments
            .map(|args| {
                vec![TranscriptEvent {
                    at_ms,
                    kind: TranscriptEventKind::ToolCall,
                    tool: Some(name.to_owned()),
                    tool_call_id: record
                        .pointer("/payload/call_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    command: args
                        .get("cmd")
                        .or_else(|| args.get("command"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    cwd: args
                        .get("workdir")
                        .or_else(|| args.get("cwd"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    path: args
                        .get("file_path")
                        .or_else(|| args.get("path"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    recipient: None,
                    exit_code: None,
                    status_complete: None,
                    body: args
                        .get("prompt")
                        .or_else(|| args.get("content"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }]
            })
            .unwrap_or_default());
    }
    if record_type == "response_item"
        && record.pointer("/payload/type").and_then(Value::as_str) == Some("message")
    {
        let role = record
            .pointer("/payload/role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = extract_message(record.pointer("/payload/content"));
        if let Some(body) = body {
            let at_ms = timestamp_ms(record).ok_or("transcript_message_missing_timestamp")?;
            return Ok(vec![TranscriptEvent {
                at_ms,
                kind: if role == "user" {
                    TranscriptEventKind::IncomingMessage
                } else {
                    TranscriptEventKind::AssistantMessage
                },
                tool: None,
                tool_call_id: None,
                command: None,
                cwd: None,
                path: None,
                recipient: None,
                exit_code: None,
                status_complete: None,
                body: Some(body),
            }]);
        }
    }
    Ok(Vec::new())
}

fn extract_message(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn timestamp_ms(record: &Value) -> Option<i64> {
    record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.timestamp_millis())
        .or_else(|| record.get("timestamp_ms").and_then(Value::as_i64))
}

#[implements("FR-047-AC-5")]
fn transcript_id(record: &Value, harness: HarnessFormat) -> Option<&str> {
    match harness {
        HarnessFormat::Claude => record.get("sessionId").and_then(Value::as_str),
        HarnessFormat::Codex
            if record.get("type").and_then(Value::as_str) == Some("session_meta") =>
        {
            record.pointer("/payload/id").and_then(Value::as_str)
        }
        HarnessFormat::Codex => record
            .pointer("/payload/session_id")
            .and_then(Value::as_str)
            .or_else(|| record.pointer("/payload/sessionId").and_then(Value::as_str)),
    }
}

fn status_complete_marker(output: &str) -> bool {
    output
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .and_then(|value| value.get("complete").and_then(Value::as_bool))
        == Some(true)
}

fn fleet_status_checks(events: &[TranscriptEvent]) -> Vec<FleetStatusCheck> {
    events
        .iter()
        .filter(|event| {
            event.kind == TranscriptEventKind::ToolCall
                && event
                    .command
                    .as_deref()
                    .is_some_and(|command| command.contains("fleet status"))
        })
        .map(|command| {
            let result = command.tool_call_id.as_deref().and_then(|call_id| {
                events.iter().find(|event| {
                    event.kind == TranscriptEventKind::ToolResult
                        && event.tool_call_id.as_deref() == Some(call_id)
                })
            });
            let exit_code = command
                .exit_code
                .or_else(|| result.and_then(|event| event.exit_code));
            let complete = command
                .status_complete
                .or_else(|| result.and_then(|event| event.status_complete))
                .unwrap_or(false);
            FleetStatusCheck {
                at_ms: command.at_ms,
                exited_zero: exit_code == Some(0),
                complete,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retro::eval::EvaluationRule;
    use ix_trace_rs::trace;

    fn binding(transcript_id: &str, role: &str) -> SessionBinding {
        SessionBinding {
            transcript_id: transcript_id.to_owned(),
            session_addr: "test:workspace:coder".to_owned(),
            session_role: role.to_owned(),
            started_at_ms: 1_700_000_000_000,
        }
    }

    fn rule(query: &str) -> EvaluationRule {
        EvaluationRule {
            id: "CODE-R11-E1".to_owned(),
            rule: "rule".to_owned(),
            evidence_source: "transcript".to_owned(),
            trigger: "hooks.role_turns".to_owned(),
            method: "deterministic".to_owned(),
            query_or_rubric: query.to_owned(),
            comparator: "=".to_owned(),
            target: 0.0,
            unit: "commands".to_owned(),
        }
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn parses_real_claude_tool_use_shape_without_exposing_body_in_results() {
        let line = br#"{"type":"assistant","timestamp":"2023-11-14T22:13:20Z","sessionId":"claude-1","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"gh pr merge 42","description":"merge"}}]}}"#;
        let session = parse_lines(
            HarnessFormat::Claude,
            binding("claude-1", "coder"),
            &[line],
            4096,
            2048,
        );
        assert!(session.complete);
        assert_eq!(session.events.len(), 1);
        assert_eq!(session.events[0].tool.as_deref(), Some("Bash"));
        assert_eq!(session.events[0].command.as_deref(), Some("gh pr merge 42"));
        let spec = RoleSpec {
            role: "coder".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule("transcript.coder_merge_commands")],
        };
        let values = adapters(
            &spec,
            &session,
            &EvalWindow {
                start_ms: 1_700_000_000_000,
                end_ms: 1_900_000_000_000,
            },
            &AdapterContext::default(),
        );
        assert!(matches!(
            values["transcript.coder_merge_commands"],
            QueryValue::Value { value: 1.0, .. }
        ));
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn parses_real_codex_command_execution_shape_and_rejects_truncated_input() {
        let line = br#"{"type":"event_msg","timestamp":"2023-11-14T22:13:20Z","payload":{"type":"item_completed","item":{"type":"CommandExecution","completed_at_ms":1700000000000,"command":"git push origin main","cwd":"/repo","status":"completed"}}}"#;
        let session = parse_lines(
            HarnessFormat::Codex,
            binding("codex-1", "coder"),
            &[line],
            4096,
            2048,
        );
        assert!(session.complete);
        assert_eq!(
            session.events[0].command.as_deref(),
            Some("git push origin main")
        );
        let broken = parse_lines(
            HarnessFormat::Codex,
            binding("codex-1", "coder"),
            &[b"{"],
            4096,
            2048,
        );
        assert!(!broken.complete);
        assert_eq!(
            broken.unavailable_reason.as_deref(),
            Some("malformed_transcript_record")
        );
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn session_turns_require_matching_complete_transcript_and_hook_attribution() {
        let complete = parse_lines(
            HarnessFormat::Claude,
            binding("claude-1", "coder"),
            &[],
            4096,
            2048,
        );
        let hooks = vec![HookTurn {
            transcript_id: "claude-1".to_owned(),
            session_addr: "test:workspace:coder".to_owned(),
            session_role: "coder".to_owned(),
            started_at_ms: 1_700_000_000_000,
        }];
        let window = EvalWindow {
            start_ms: 1_699_999_000_000,
            end_ms: 1_700_001_000_000,
        };
        let rows = session_turn_rows(&[complete], &hooks, &window);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_addr, "test:workspace:coder");
        let broken = parse_lines(
            HarnessFormat::Claude,
            binding("claude-1", "coder"),
            &[b"bad"],
            4096,
            2048,
        );
        assert_eq!(
            session_turn_rows(&[broken], &hooks, &window).len(),
            1,
            "hook-attributed turns stay visible so their rule rows can report unavailable"
        );
    }

    fn ledger_turn(id: &str, role: &str) -> SessionTurnRow {
        SessionTurnRow {
            transcript_id: Some(id.to_owned()),
            session_addr: "test:workspace:coder".to_owned(),
            session_role: role.to_owned(),
            started_at_ms: 1_700_000_000_000,
        }
    }

    fn write_transcript(home: &Path, relative: &str, content: &str) -> PathBuf {
        let path = home.join(relative);
        fs::create_dir_all(path.parent().expect("transcript parent"))
            .expect("create transcript parent");
        fs::write(&path, content).expect("write transcript fixture");
        path
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn discovers_and_parses_claude_transcript_by_explicit_ledger_id() {
        let home = tempfile::tempdir().expect("temporary home");
        write_transcript(
            home.path(),
            ".claude/projects/work/claude-7.jsonl",
            r#"{"type":"assistant","timestamp":"2023-11-14T22:13:20Z","sessionId":"claude-7","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git status --short"}}]}}"#,
        );
        let turns = vec![ledger_turn("claude-7", "coder")];

        let inventory = discover_transcripts(home.path(), &turns, TranscriptReadLimits::default());

        assert!(inventory.complete);
        assert_eq!(inventory.turns.len(), 1);
        let session = &inventory.transcripts["claude-7"];
        assert!(session.complete);
        assert_eq!(session.binding.session_addr, "test:workspace:coder");
        assert_eq!(
            session.events[0].command.as_deref(),
            Some("git status --short")
        );
        let spec = RoleSpec {
            role: "coder".to_owned(),
            metrics: Vec::new(),
            rules: vec![rule("transcript.coder_merge_commands")],
        };
        assert!(
            inventory
                .adapters_by_transcript(
                    &spec,
                    &EvalWindow {
                        start_ms: 1_699_999_000_000,
                        end_ms: 1_700_001_000_000,
                    },
                    &AdapterContext::default(),
                )
                .contains_key("claude-7")
        );
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn discovers_codex_transcript_by_exact_id_and_validates_session_meta() {
        let home = tempfile::tempdir().expect("temporary home");
        write_transcript(
            home.path(),
            ".codex/sessions/2023/11/14/rollout-2023-11-14T22-13-20-codex-8.jsonl",
            concat!(
                r#"{"timestamp":"2023-11-14T22:13:20Z","type":"session_meta","payload":{"id":"codex-8","source":"cli","cwd":"/work"}}"#,
                "\n",
                r#"{"type":"event_msg","timestamp":"2023-11-14T22:14:20Z","payload":{"id":"call-1","type":"item_completed","item":{"type":"CommandExecution","completed_at_ms":1700000060000,"command":"cargo check","cwd":"/work","status":"completed"}}}"#,
                "\n"
            ),
        );
        let turns = vec![ledger_turn("codex-8", "coder")];

        let inventory = discover_transcripts(home.path(), &turns, TranscriptReadLimits::default());

        assert!(inventory.complete);
        let session = &inventory.transcripts["codex-8"];
        assert!(session.complete);
        assert_eq!(session.events[0].command.as_deref(), Some("cargo check"));
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn missing_ambiguous_and_malformed_sources_keep_turns_unavailable() {
        let home = tempfile::tempdir().expect("temporary home");
        write_transcript(
            home.path(),
            ".claude/projects/work/bad.jsonl",
            "{not-json}\n",
        );
        write_transcript(
            home.path(),
            ".claude/projects/work/shared.jsonl",
            r#"{"type":"assistant","timestamp":"2023-11-14T22:13:20Z","sessionId":"shared","message":{"content":"Done"}}"#,
        );
        write_transcript(
            home.path(),
            ".codex/sessions/2023/11/14/rollout-2023-11-14T22-13-20-shared.jsonl",
            r#"{"timestamp":"2023-11-14T22:13:20Z","type":"session_meta","payload":{"id":"shared","source":"cli"}}"#,
        );
        let turns = vec![
            ledger_turn("missing", "coder"),
            ledger_turn("bad", "coder"),
            ledger_turn("shared", "coder"),
            SessionTurnRow {
                transcript_id: None,
                session_addr: "test:workspace:minder".to_owned(),
                session_role: "minder".to_owned(),
                started_at_ms: 1_700_000_000_100,
            },
        ];

        let inventory = discover_transcripts(home.path(), &turns, TranscriptReadLimits::default());

        assert!(!inventory.complete);
        assert_eq!(inventory.turns.len(), 4);
        assert_eq!(
            inventory.transcripts["missing"]
                .unavailable_reason
                .as_deref(),
            Some("transcript_missing")
        );
        assert_eq!(
            inventory.transcripts["bad"].unavailable_reason.as_deref(),
            Some("malformed_transcript_record")
        );
        assert_eq!(
            inventory.transcripts["shared"]
                .unavailable_reason
                .as_deref(),
            Some("transcript_ambiguous")
        );
        assert!(inventory.transcripts.values().any(|session| {
            session.binding.session_role == "minder"
                && session.unavailable_reason.as_deref() == Some("transcript_id_unavailable")
        }));
        assert!(
            inventory
                .turns
                .iter()
                .any(|turn| turn.session_role == "minder" && turn.transcript_id.is_none())
        );
        assert!(inventory.ledger_rows().iter().any(|row| {
            row.get("session_role").and_then(Value::as_str) == Some("minder")
                && !row.contains_key("transcript_id")
        }));
        assert!(
            turn_rows_from_ledger(&inventory.ledger_rows())
                .iter()
                .any(|turn| turn.session_role == "minder" && turn.transcript_id.is_none())
        );
    }

    #[test]
    #[trace("FR-047-AC-5")]
    fn oversized_files_and_records_are_rejected_with_bounded_reads() {
        let home = tempfile::tempdir().expect("temporary home");
        write_transcript(
            home.path(),
            ".claude/projects/work/large.jsonl",
            &format!("{}\n", "x".repeat(80)),
        );
        write_transcript(
            home.path(),
            ".claude/projects/work/record.jsonl",
            &format!("{}\n", " ".repeat(40)),
        );
        let turns = vec![
            ledger_turn("large", "coder"),
            ledger_turn("record", "coder"),
        ];
        let limits = TranscriptReadLimits {
            max_transcript_bytes: 64,
            max_record_bytes: 16,
            max_collection_bytes: 128,
            ..TranscriptReadLimits::default()
        };

        let inventory = discover_transcripts(home.path(), &turns, limits);

        assert!(!inventory.complete);
        assert_eq!(
            inventory.transcripts["large"].unavailable_reason.as_deref(),
            Some("transcript_size_limit")
        );
        assert_eq!(
            inventory.transcripts["record"]
                .unavailable_reason
                .as_deref(),
            Some("transcript_record_size_limit")
        );
        assert!(
            inventory
                .transcripts
                .values()
                .all(|session| session.bytes_read == 0)
        );
    }
}
