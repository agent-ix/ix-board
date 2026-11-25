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

    /// Persists prompt events for configured orchestrator and minder sessions.
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
        let tx = self
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let mut inserted = 0;
        for event in events
            .iter()
            .filter(|event| event.event.kind == HookKind::Prompt)
        {
            let role = settings.role_of(&event.addr);
            if !matches!(role, "orchestrator" | "minder") {
                continue;
            }
            inserted += tx
                .execute(
                    "INSERT OR IGNORE INTO session_turn(session_addr,session_role,started_at)
                 VALUES(?1,?2,?3)",
                    params![event.addr, role, event.event.server_ms],
                )
                .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(inserted)
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
        ledger.cycle_start("test-server:test-workspace:test-orchestrator", 1_000, 600).unwrap();
        ledger.cycle_end("test-server:test-workspace:test-orchestrator", 2_000).unwrap();
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
        ledger.cycle_start("test-server:test-workspace:test-orchestrator", 1_000, 600).unwrap();
        ledger.cycle_start("test-server:test-workspace:test-orchestrator-two", 1_100, 300).unwrap();
        ledger.cycle_start("test-server:test-workspace:test-orchestrator", 1_200, 900).unwrap();
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
        assert_eq!(rows[1], ("test-server:test-workspace:test-orchestrator-two".to_owned(), 1_100, None, None));
        assert_eq!(rows[2], ("test-server:test-workspace:test-orchestrator".to_owned(), 1_200, None, None));
    }

    #[trace("FR-039-AC-14")]
    #[test]
    fn session_turns_keep_only_configured_role_prompts_once() {
        let (_directory, mut ledger) = ledger();
        let settings = TickSettings {
            orchestrator: Some("test-server:test-workspace:test-orchestrator".to_owned()),
            minder: Some("test-server:test-workspace:test-minder".to_owned()),
            ..TickSettings::default()
        };
        let events = vec![
            RawEvent {
                addr: "test-server:test-workspace:test-orchestrator".to_owned(),
                event: HookEvent {
                    server_ms: 1_000,
                    kind: HookKind::Prompt,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-minder".to_owned(),
                event: HookEvent {
                    server_ms: 1_100,
                    kind: HookKind::Prompt,
                },
            },
            RawEvent {
                addr: "other:w:lane".to_owned(),
                event: HookEvent {
                    server_ms: 1_200,
                    kind: HookKind::Prompt,
                },
            },
            RawEvent {
                addr: "test-server:test-workspace:test-minder".to_owned(),
                event: HookEvent {
                    server_ms: 1_300,
                    kind: HookKind::PreTool,
                },
            },
        ];
        assert_eq!(
            ledger
                .record_session_turns(&settings, &events, &ActivityCoverage::Complete)
                .unwrap(),
            2
        );
        assert_eq!(
            ledger
                .record_session_turns(&settings, &events, &ActivityCoverage::Complete)
                .unwrap(),
            0
        );
        let rows: Vec<(String, String, i64)> = ledger
            .connection
            .prepare(
                "SELECT session_addr,session_role,started_at FROM session_turn ORDER BY started_at",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("test-server:test-workspace:test-orchestrator".to_owned(), "orchestrator".to_owned(), 1_000),
                ("test-server:test-workspace:test-minder".to_owned(), "minder".to_owned(), 1_100),
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

    #[trace("FR-039-AC-16")]
    #[test]
    fn a_missing_cycle_is_recorded_once_after_two_intervals() {
        let (_directory, mut ledger) = ledger();
        ledger.cycle_start("test-server:test-workspace:test-orchestrator", 1_000, 600).unwrap();
        let tx = ledger.connection.transaction().unwrap();
        record_missed_cycle(&tx, Some("test-server:test-workspace:test-orchestrator"), 1_201_000).unwrap();
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
        record_missed_cycle(&tx, Some("test-server:test-workspace:test-orchestrator"), 1_201_001).unwrap();
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
        record_missed_cycle(&tx, Some("test-server:test-workspace:test-orchestrator"), 1_300_000).unwrap();
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
