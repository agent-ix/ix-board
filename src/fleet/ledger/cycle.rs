// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Orchestrator cycle and minder prompt-turn ledger writers (FR-039, FR-044).

use super::{Ledger, LedgerError};
use crate::fleet::{HookKind, RawEvent, tick::TickSettings};
use ix_trace_rs::implements;
use rusqlite::{OptionalExtension, Transaction, params};

/// Completeness of the relay activity event read used for session-turn history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivityCoverage {
    /// The bounded relay event read is complete for the requested retention window.
    Complete,
    /// The source was capped, gapped, or otherwise incomplete; no partial rows are written.
    Unavailable(String),
}

impl Ledger {
    /// Returns the durable relay stream cursor for incremental session-turn ingestion.
    pub fn session_turn_stream_cursor(&self) -> Result<Option<(i64, i64)>, String> {
        self.connection
            .query_row(
                "SELECT cursor_ms,cursor_sequence FROM session_turn_stream_state
                 WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())
    }

    /// Writes one bounded activity page and advances its Redis cursor atomically.
    ///
    /// Entries with unreadable payloads still advance the cursor and increment the durable
    /// coverage defect count, so one malformed event cannot pin ingestion forever.
    #[implements("FR-039-AC-14")]
    pub fn record_session_turn_page(
        &mut self,
        settings: &TickSettings,
        entries: &[(String, Option<RawEvent>)],
        role_observations: &[RawEvent],
        oldest_available_id: Option<&str>,
        observed_at_ms: i64,
    ) -> Result<usize, String> {
        let Some((next_ms, next_sequence)) = entries
            .last()
            .map(|(id, _)| parse_stream_id(id))
            .transpose()?
        else {
            return Ok(0);
        };
        let oldest_cursor = oldest_available_id.map(parse_stream_id).transpose()?;
        let oldest_ms = oldest_cursor.map(|(milliseconds, _)| milliseconds);
        let tx = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let previous_cursor: Option<(i64, i64)> = tx
            .query_row(
                "SELECT cursor_ms,cursor_sequence FROM session_turn_stream_state
                 WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let retention_gap_start = match (previous_cursor, oldest_cursor) {
            (Some(previous), Some(oldest)) if previous < oldest => Some(oldest.0),
            _ => None,
        };
        for event in role_observations
            .iter()
            .chain(entries.iter().filter_map(|(_, event)| event.as_ref()))
        {
            if event.event.kind != HookKind::SessionStart {
                continue;
            }
            let (Some(session), Some(role_name)) = (
                event.transcript_id.as_deref(),
                event.session_role.as_deref(),
            ) else {
                continue;
            };
            let Some(role) = configured_session_role(role_name) else {
                continue;
            };
            tx.execute(
                "INSERT INTO session_turn_session_role(session_addr,transcript_id,session_role)
                 VALUES(?1,?2,?3)
                 ON CONFLICT(session_addr,transcript_id) DO UPDATE SET
                   session_role=CASE WHEN session_turn_session_role.session_role=
                                           excluded.session_role
                                     THEN session_turn_session_role.session_role
                                     ELSE NULL END",
                params![event.addr, session, role],
            )
            .map_err(|error| error.to_string())?;
        }
        let mut inserted = 0;
        for (_, event) in entries {
            let Some(event) = event else { continue };
            if event.event.kind == HookKind::SessionStart {
                continue;
            }
            if event.event.kind != HookKind::Prompt {
                continue;
            }
            let role = match settings.role_of(&event.addr) {
                role @ ("orchestrator" | "minder") => Some(role),
                _ => {
                    let recorded_role: Option<String> = match event.transcript_id.as_deref() {
                        Some(session) => tx
                            .query_row(
                                "SELECT session_role FROM session_turn_session_role
                                 WHERE session_addr=?1 AND transcript_id=?2",
                                params![event.addr, session],
                                |row| row.get(0),
                            )
                            .optional()
                            .map_err(|error| error.to_string())?
                            .flatten(),
                        None => None,
                    };
                    recorded_role.as_deref().and_then(configured_session_role)
                }
            };
            let Some(role) = role else { continue };
            inserted += tx
                .execute(
                    "INSERT INTO session_turn(session_addr,session_role,started_at,transcript_id)
                     VALUES(?1,?2,?3,?4)
                     ON CONFLICT(session_addr,started_at) DO UPDATE SET
                       transcript_id=excluded.transcript_id
                     WHERE session_turn.transcript_id IS NULL
                       AND excluded.transcript_id IS NOT NULL",
                    params![event.addr, role, event.event.server_ms, event.transcript_id],
                )
                .map_err(|error| error.to_string())?;
        }
        let unreadable = i64::try_from(entries.iter().filter(|(_, event)| event.is_none()).count())
            .map_err(|error| error.to_string())?;
        let event_count = i64::try_from(entries.len()).map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO session_turn_stream_state(
               singleton,cursor_ms,cursor_sequence,coverage_start_ms,incomplete_before_ms,
               last_read_ms,events_read,unreadable_events,pages_read,retention_gaps,
               latest_gap_start_ms)
             VALUES(1,?1,?2,?3,?3,?4,?5,?6,1,?7,?8)
             ON CONFLICT(singleton) DO UPDATE SET
               cursor_ms=excluded.cursor_ms,cursor_sequence=excluded.cursor_sequence,
               coverage_start_ms=COALESCE(session_turn_stream_state.coverage_start_ms,
                                           excluded.coverage_start_ms),
               incomplete_before_ms=COALESCE(session_turn_stream_state.incomplete_before_ms,
                                             excluded.incomplete_before_ms),
               last_read_ms=excluded.last_read_ms,
               events_read=session_turn_stream_state.events_read+excluded.events_read,
               unreadable_events=session_turn_stream_state.unreadable_events+
                                  excluded.unreadable_events,
               pages_read=session_turn_stream_state.pages_read+1,
               retention_gaps=session_turn_stream_state.retention_gaps+excluded.retention_gaps,
               latest_gap_start_ms=COALESCE(excluded.latest_gap_start_ms,
                                            session_turn_stream_state.latest_gap_start_ms)
             WHERE (excluded.cursor_ms,excluded.cursor_sequence) >
                   (session_turn_stream_state.cursor_ms,
                    session_turn_stream_state.cursor_sequence)",
            params![
                next_ms,
                next_sequence,
                oldest_ms,
                observed_at_ms,
                event_count,
                unreadable,
                if retention_gap_start.is_some() {
                    1_i64
                } else {
                    0_i64
                },
                retention_gap_start
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(inserted)
    }

    /// Starts a cycle, superseding any still-open cycle for the same session.
    #[implements("FR-044-AC-1")]
    pub fn cycle_start(
        &mut self,
        session_addr: &str,
        started_at_ms: i64,
        interval_s: i64,
    ) -> Result<i64, String> {
        if session_addr.trim().is_empty() || interval_s <= 0 {
            return Err(
                "session address must be non-empty and interval must be positive".to_owned(),
            );
        }
        let tx = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        tx.execute(
            "UPDATE cycle_event SET ended_at=?1, ended_reason='superseded'
             WHERE session_addr=?2 AND ended_at IS NULL",
            params![started_at_ms, session_addr],
        )
        .map_err(|error| error.to_string())?;
        tx.execute(
            "INSERT INTO cycle_event(session_addr,started_at,interval_s)
             VALUES(?1,?2,?3)",
            params![session_addr, started_at_ms, interval_s],
        )
        .map_err(|error| error.to_string())?;
        let cycle_id = tx.last_insert_rowid();
        tx.commit().map_err(|error| error.to_string())?;
        Ok(cycle_id)
    }

    /// Ends the newest open cycle for `session_addr`.
    #[implements("FR-044-AC-1")]
    pub fn cycle_end(&mut self, session_addr: &str, ended_at_ms: i64) -> Result<i64, String> {
        let changed = self
            .connection
            .execute(
                "UPDATE cycle_event SET ended_at=?1, ended_reason='ended'
                 WHERE cycle_id=(SELECT cycle_id FROM cycle_event
                   WHERE session_addr=?2 AND ended_at IS NULL AND started_at <= ?1
                   ORDER BY cycle_id DESC LIMIT 1)",
                params![ended_at_ms, session_addr],
            )
            .map_err(|error| error.to_string())?;
        if changed != 1 {
            return Err(format!("session {session_addr} has no open fleet cycle"));
        }
        Ok(ended_at_ms)
    }

    /// Persists prompt events for configured role sessions with their relay-owned transcript ids.
    #[implements("FR-039-AC-14")]
    pub fn record_session_turns(
        &mut self,
        settings: &TickSettings,
        events: &[RawEvent],
        coverage: &ActivityCoverage,
    ) -> Result<usize, String> {
        if let ActivityCoverage::Unavailable(reason) = coverage {
            return Err(format!("relay session-turn source unavailable: {reason}"));
        }
        let mut roles_by_session = std::collections::BTreeMap::new();
        for event in events {
            if event.event.kind != HookKind::SessionStart {
                continue;
            }
            let (Some(session), Some(role_name)) = (
                event.transcript_id.as_deref(),
                event.session_role.as_deref(),
            ) else {
                continue;
            };
            let Some(role) = configured_session_role(role_name) else {
                continue;
            };
            let key = (event.addr.clone(), session.to_owned());
            roles_by_session
                .entry(key)
                .and_modify(|existing| {
                    if *existing != Some(role) {
                        *existing = None;
                    }
                })
                .or_insert(Some(role));
        }
        let tx = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let mut inserted = 0;
        for event in events
            .iter()
            .filter(|event| event.event.kind == HookKind::Prompt)
        {
            let role = match settings.role_of(&event.addr) {
                role @ ("orchestrator" | "minder") => Some(role),
                _ => event.transcript_id.as_deref().and_then(|session| {
                    roles_by_session
                        .get(&(event.addr.clone(), session.to_owned()))
                        .copied()
                        .flatten()
                }),
            };
            let Some(role) = role else { continue };
            inserted += tx
                .execute(
                    "INSERT INTO session_turn(session_addr,session_role,started_at,transcript_id)
                     VALUES(?1,?2,?3,?4)
                     ON CONFLICT(session_addr,started_at) DO UPDATE SET
                       transcript_id=excluded.transcript_id
                     WHERE session_turn.transcript_id IS NULL
                       AND excluded.transcript_id IS NOT NULL",
                    params![event.addr, role, event.event.server_ms, event.transcript_id],
                )
                .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(inserted)
    }
}

fn parse_stream_id(id: &str) -> Result<(i64, i64), String> {
    let (milliseconds, sequence) = id
        .split_once('-')
        .ok_or_else(|| format!("invalid Redis activity stream ID {id:?}"))?;
    let milliseconds = milliseconds
        .parse::<i64>()
        .map_err(|error| format!("invalid Redis activity stream milliseconds: {error}"))?;
    let sequence = sequence
        .parse::<i64>()
        .map_err(|error| format!("invalid Redis activity stream sequence: {error}"))?;
    if milliseconds < 0 || sequence < 0 {
        return Err("Redis activity stream IDs must be non-negative".to_owned());
    }
    Ok((milliseconds, sequence))
}

fn configured_session_role(role: &str) -> Option<&'static str> {
    match role {
        "code-lead" => Some("code-lead"),
        "coder" => Some("coder"),
        "minder" => Some("minder"),
        "orchestrator" => Some("orchestrator"),
        "planner" => Some("planner"),
        "reviewer" => Some("reviewer"),
        _ => None,
    }
}

#[implements("FR-039-AC-16")]
pub(super) fn record_missed_cycle(
    tx: &Transaction<'_>,
    session_addr: Option<&str>,
    detected_at_ms: i64,
) -> Result<(), LedgerError> {
    let Some(session_addr) = session_addr else {
        return Ok(());
    };
    let latest: Option<(i64, Option<i64>, i64)> = tx
        .query_row(
            "SELECT started_at,ended_at,interval_s FROM cycle_event
             WHERE session_addr=?1 ORDER BY cycle_id DESC LIMIT 1",
            [session_addr],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((started_at, ended_at, interval_s)) = latest else {
        return Ok(());
    };
    let last_ended_at = ended_at.unwrap_or(started_at);
    let expected_by = last_ended_at.saturating_add(interval_s.saturating_mul(2_000));
    if detected_at_ms <= expected_by {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO cycle_missed(session_addr,last_ended_at,expected_by,detected_at,at_ms)
         VALUES(?1,?2,?3,?4,?4) ON CONFLICT(session_addr,last_ended_at) DO NOTHING",
        params![session_addr, last_ended_at, expected_by, detected_at_ms],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{FleetConfig, HookEvent};
    use ix_trace_rs::trace;
    use std::path::PathBuf;

    fn ledger() -> (tempfile::TempDir, Ledger) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fleet.sqlite3");
        let config = FleetConfig {
            ledger_path: Some(PathBuf::from(&path)),
            ..FleetConfig::default()
        };
        (directory, Ledger::open(&path, &config).unwrap())
    }

    #[trace("FR-044-AC-1")]
    #[test]
    fn cycle_start_and_end_record_relay_times_and_interval() {
        let (_directory, mut ledger) = ledger();
        ledger
            .cycle_start("test-server:test-workspace:test-orchestrator", 1_000, 600)
            .unwrap();
        ledger
            .cycle_end("test-server:test-workspace:test-orchestrator", 2_000)
            .unwrap();
        let row: (String, i64, Option<i64>, Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT session_addr,started_at,ended_at,ended_reason,interval_s
                 FROM cycle_event",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "test-server:test-workspace:test-orchestrator".to_owned(),
                1_000,
                Some(2_000),
                Some("ended".to_owned()),
                600
            )
        );
    }

    #[trace("FR-044-AC-2")]
    #[test]
    fn cycle_start_supersedes_only_the_same_sessions_open_cycle() {
        let (_directory, mut ledger) = ledger();
        ledger
            .cycle_start("test-server:test-workspace:test-orchestrator", 1_000, 600)
            .unwrap();
        ledger
            .cycle_start(
                "test-server:test-workspace:test-orchestrator-two",
                1_100,
                300,
            )
            .unwrap();
        ledger
            .cycle_start("test-server:test-workspace:test-orchestrator", 1_200, 900)
            .unwrap();
        let rows: Vec<(String, i64, Option<i64>, Option<String>)> = ledger
            .connection
            .prepare(
                "SELECT session_addr,started_at,ended_at,ended_reason FROM cycle_event
                 ORDER BY cycle_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows[0],
            (
                "test-server:test-workspace:test-orchestrator".to_owned(),
                1_000,
                Some(1_200),
                Some("superseded".to_owned())
            )
        );
        assert_eq!(
            rows[1],
            (
                "test-server:test-workspace:test-orchestrator-two".to_owned(),
                1_100,
                None,
                None
            )
        );
        assert_eq!(
            rows[2],
            (
                "test-server:test-workspace:test-orchestrator".to_owned(),
                1_200,
                None,
                None
            )
        );
    }

    #[trace("FR-039-AC-14")]
    #[test]
    fn session_turns_keep_configured_and_session_role_prompts_once() {
        let (_directory, mut ledger) = ledger();
        let settings = TickSettings {
            orchestrator: Some("test-server:test-workspace:test-orchestrator".to_owned()),
            minder: Some("test-server:test-workspace:test-minder".to_owned()),
            ..TickSettings::default()
        };
        let events = vec![
            RawEvent {
                addr: "test-server:test-workspace:test-orchestrator".to_owned(),
                transcript_id: Some("orch-transcript".to_owned()),
                session_role: None,
                event: HookEvent {
                    server_ms: 1_000,
                    kind: HookKind::Prompt,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-minder".to_owned(),
                transcript_id: None,
                session_role: None,
                event: HookEvent {
                    server_ms: 1_100,
                    kind: HookKind::Prompt,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-lane".to_owned(),
                transcript_id: Some("unconfigured-transcript".to_owned()),
                session_role: None,
                event: HookEvent {
                    server_ms: 1_200,
                    kind: HookKind::Prompt,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-minder".to_owned(),
                transcript_id: Some("minder-non-prompt".to_owned()),
                session_role: None,
                event: HookEvent {
                    server_ms: 1_300,
                    kind: HookKind::PreTool,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-coder".to_owned(),
                transcript_id: Some("coder-transcript".to_owned()),
                session_role: Some("coder".to_owned()),
                event: HookEvent {
                    server_ms: 1_400,
                    kind: HookKind::SessionStart,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-coder".to_owned(),
                transcript_id: Some("coder-transcript".to_owned()),
                session_role: None,
                event: HookEvent {
                    server_ms: 1_500,
                    kind: HookKind::Prompt,
                },
            },
        ];
        assert_eq!(
            ledger
                .record_session_turns(&settings, &events, &ActivityCoverage::Complete)
                .unwrap(),
            3
        );
        assert_eq!(
            ledger
                .record_session_turns(&settings, &events, &ActivityCoverage::Complete)
                .unwrap(),
            0
        );
        let rows: Vec<(String, String, i64, Option<String>)> = ledger
            .connection
            .prepare(
                "SELECT session_addr,session_role,started_at,transcript_id
                 FROM session_turn ORDER BY started_at",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "test-server:test-workspace:test-orchestrator".to_owned(),
                    "orchestrator".to_owned(),
                    1_000,
                    Some("orch-transcript".to_owned())
                ),
                (
                    "test-server:test-workspace:test-minder".to_owned(),
                    "minder".to_owned(),
                    1_100,
                    None
                ),
                (
                    "test-server:test-workspace:test-coder".to_owned(),
                    "coder".to_owned(),
                    1_500,
                    Some("coder-transcript".to_owned())
                ),
            ]
        );
    }

    #[trace("FR-039-AC-14")]
    #[test]
    fn capped_activity_reads_write_no_partial_session_turn_rows() {
        let (_directory, mut ledger) = ledger();
        let settings = TickSettings {
            minder: Some("test-server:test-workspace:test-minder".to_owned()),
            ..TickSettings::default()
        };
        let events = vec![RawEvent {
            addr: "test-server:test-workspace:test-minder".to_owned(),
            transcript_id: None,
            session_role: None,
            event: HookEvent {
                server_ms: 1_100,
                kind: HookKind::Prompt,
            },
        }];
        let result = ledger.record_session_turns(
            &settings,
            &events,
            &ActivityCoverage::Unavailable("activity stream reached its cap".to_owned()),
        );
        assert!(
            result
                .unwrap_err()
                .contains("activity stream reached its cap")
        );
        let rows: i64 = ledger
            .connection
            .query_row("SELECT COUNT(*) FROM session_turn", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[trace("FR-039-AC-14")]
    #[test]
    fn bounded_stream_pages_resume_cursor_and_keep_role_across_page_boundary() {
        let (_directory, mut ledger) = ledger();
        let settings = TickSettings::default();
        let session_start = RawEvent {
            addr: "test-server:test-workspace:test-coder".to_owned(),
            transcript_id: Some("session-coder".to_owned()),
            session_role: Some("coder".to_owned()),
            event: HookEvent {
                server_ms: 1_000,
                kind: HookKind::SessionStart,
            },
        };
        let idle = RawEvent {
            addr: "test-server:test-workspace:test-coder".to_owned(),
            transcript_id: Some("session-coder".to_owned()),
            session_role: None,
            event: HookEvent {
                server_ms: 1_000,
                kind: HookKind::Idle,
            },
        };
        assert_eq!(ledger.session_turn_stream_cursor().unwrap(), None);
        ledger
            .record_session_turn_page(
                &settings,
                &[("1000-0".to_owned(), Some(idle))],
                &[session_start],
                Some("800-0"),
                1_010,
            )
            .unwrap();
        assert_eq!(
            ledger.session_turn_stream_cursor().unwrap(),
            Some((1_000, 0))
        );

        let prompt = RawEvent {
            addr: "test-server:test-workspace:test-coder".to_owned(),
            transcript_id: Some("session-coder".to_owned()),
            session_role: None,
            event: HookEvent {
                server_ms: 1_100,
                kind: HookKind::Prompt,
            },
        };
        assert_eq!(
            ledger
                .record_session_turn_page(
                    &settings,
                    &[("1100-0".to_owned(), Some(prompt))],
                    &[],
                    Some("900-0"),
                    1_110,
                )
                .unwrap(),
            1
        );
        assert_eq!(
            ledger.session_turn_stream_cursor().unwrap(),
            Some((1_100, 0))
        );
        let row: (String, String, i64, Option<String>) = ledger
            .connection
            .query_row(
                "SELECT session_addr,session_role,started_at,transcript_id FROM session_turn",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "test-server:test-workspace:test-coder".to_owned(),
                "coder".to_owned(),
                1_100,
                Some("session-coder".to_owned())
            )
        );
        let coverage: (i64, i64, i64, String) = ledger
            .connection
            .query_row(
                "SELECT cursor_ms,coverage_start_ms,incomplete_before_ms,status
                 FROM session_turn_stream_coverage",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            coverage,
            (
                1_100,
                800,
                800,
                "incomplete_before_first_observed_event".to_owned()
            )
        );

        let later_prompt = RawEvent {
            addr: "test-server:test-workspace:test-coder".to_owned(),
            transcript_id: Some("session-coder".to_owned()),
            session_role: None,
            event: HookEvent {
                server_ms: 1_300,
                kind: HookKind::Prompt,
            },
        };
        ledger
            .record_session_turn_page(
                &settings,
                &[("1300-0".to_owned(), Some(later_prompt))],
                &[],
                Some("1200-0"),
                1_310,
            )
            .unwrap();
        let retention: (i64, i64, String) = ledger
            .connection
            .query_row(
                "SELECT retention_gaps,latest_gap_start_ms,status
                 FROM session_turn_stream_coverage",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            retention,
            (1, 1_200, "incomplete_possible_retention_gap".to_owned())
        );
    }

    #[trace("FR-039-AC-16")]
    #[test]
    fn a_missing_cycle_is_recorded_once_after_two_intervals() {
        let (_directory, mut ledger) = ledger();
        ledger
            .cycle_start("test-server:test-workspace:test-orchestrator", 1_000, 600)
            .unwrap();
        let tx = ledger.connection.transaction().unwrap();
        record_missed_cycle(
            &tx,
            Some("test-server:test-workspace:test-orchestrator"),
            1_201_000,
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT COUNT(*) FROM cycle_missed", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let tx = ledger.connection.transaction().unwrap();
        record_missed_cycle(
            &tx,
            Some("test-server:test-workspace:test-orchestrator"),
            1_201_001,
        )
        .unwrap();
        tx.commit().unwrap();
        let row: (String, i64, i64, i64, i64) = ledger
            .connection
            .query_row(
                "SELECT session_addr,last_ended_at,expected_by,detected_at,at_ms FROM cycle_missed",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "test-server:test-workspace:test-orchestrator".to_owned(),
                1_000,
                1_201_000,
                1_201_001,
                1_201_001
            )
        );
        let tx = ledger.connection.transaction().unwrap();
        record_missed_cycle(
            &tx,
            Some("test-server:test-workspace:test-orchestrator"),
            1_300_000,
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT COUNT(*) FROM cycle_missed", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
