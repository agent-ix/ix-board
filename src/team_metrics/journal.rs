// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::{Config, Error, Event, State};
use ix_trace_rs::implements;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// SQLite journal with an enforced page ceiling and atomic state/event checkpoints.
pub struct Journal {
    connection: Connection,
    max_bytes: u64,
    retention_ms: i64,
}
impl Journal {
    /// Reserves a complete GitHub collection before any source reads. The singleton
    /// deadline survives restart and coalesces concurrent workers without a quota reset.
    #[implements("FR-033-AC-12")]
    pub(crate) fn claim_github_collection(
        &mut self,
        now: i64,
        interval: u64,
    ) -> Result<bool, Error> {
        if now <= 0 {
            return Err(Error::Invalid("invalid GitHub collection clock"));
        }
        self.connection
            .busy_timeout(std::time::Duration::from_millis(250))?;
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS github_collection(id INTEGER PRIMARY KEY CHECK(id=1), next_at INTEGER NOT NULL); INSERT OR IGNORE INTO github_collection VALUES(1,0);")?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let next: i64 = tx.query_row(
            "SELECT next_at FROM github_collection WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        if now < next {
            return Ok(false);
        }
        // The request guard may not have been initialized on a new journal yet.
        let has_gate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='github_gate' AND type='table')",
            [],
            |r| r.get(0),
        )?;
        if has_gate {
            let cooldown: i64 =
                tx.query_row("SELECT cooldown FROM github_gate WHERE id=1", [], |r| {
                    r.get(0)
                })?;
            if cooldown > now {
                tx.execute(
                    "UPDATE github_collection SET next_at=?1 WHERE id=1",
                    [cooldown],
                )?;
                tx.commit()?;
                return Ok(false);
            }
        }
        let interval_ms = i64::try_from(interval)
            .map_err(|_| Error::Invalid("GitHub interval overflow"))?
            .checked_mul(1000)
            .ok_or(Error::Invalid("GitHub interval overflow"))?;
        tx.execute(
            "UPDATE github_collection SET next_at=?1 WHERE id=1",
            [now.saturating_add(interval_ms)],
        )?;
        tx.commit()?;
        Ok(true)
    }
    /// Opens a versioned store, refusing newer schemas without changing them.
    pub fn open(path: &Path, config: &Config) -> Result<Self, Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            return Err(Error::Invalid("unsupported journal schema"));
        }
        connection.pragma_update(None, "journal_mode", "DELETE")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let page_size: u64 = connection.pragma_query_value(None, "page_size", |r| r.get(0))?;
        let pages: u64 = connection.pragma_query_value(None, "page_count", |r| r.get(0))?;
        if pages.saturating_mul(page_size) > config.max_bytes {
            return Err(Error::Invalid(
                "existing journal exceeds the requested lower storage limit",
            ));
        }
        connection.pragma_update(None, "max_page_count", config.max_bytes / page_size)?;
        let actual: u64 = connection.pragma_query_value(None, "max_page_count", |r| r.get(0))?;
        if actual.saturating_mul(page_size) > config.max_bytes {
            return Err(Error::Invalid("SQLite refused the requested page limit"));
        }
        connection.execute_batch("CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, at_ms INTEGER NOT NULL, body TEXT NOT NULL); CREATE INDEX IF NOT EXISTS event_time ON events(at_ms); CREATE TABLE IF NOT EXISTS state(id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL); PRAGMA user_version=1;")?;
        Ok(Self {
            connection,
            max_bytes: config.max_bytes,
            retention_ms: i64::try_from(config.retention_seconds)
                .map_err(|_| Error::Invalid("retention overflow"))?
                * 1000,
        })
    }
    /// Restores cursors, totals and current assignments after a process restart.
    pub fn load(&self) -> Result<State, Error> {
        let state: Option<String> = self
            .connection
            .query_row("SELECT body FROM state WHERE id=1", [], |r| r.get(0))
            .optional()?;
        state.map_or_else(
            || Ok(State::default()),
            |text| Ok(serde_json::from_str(&text)?),
        )
    }
    /// Stores new immutable events and their resulting cursor/state in one transaction.
    /// Returns the retained lower bound; size eviction is visible in subsequent coverage.
    pub fn checkpoint(&mut self, state: &State, events: &[Event], now: i64) -> Result<i64, Error> {
        let cutoff = now.saturating_sub(self.retention_ms);
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM events WHERE at_ms < ?1", [cutoff])?;
        // Keep event bytes below a quarter of the page budget, leaving space for SQLite
        // indices, current state and its rollback journal. Never allocate unbounded WAL.
        let event_budget =
            i64::try_from(self.max_bytes / 4).map_err(|_| Error::Invalid("budget overflow"))?;
        let mut bytes: i64 = tx.query_row(
            "SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM events",
            [],
            |r| r.get(0),
        )?;
        for event in events {
            let body = serde_json::to_string(event)?;
            if body.len() > 16384 {
                return Err(Error::Invalid("oversize source event"));
            }
            let incoming =
                i64::try_from(body.len()).map_err(|_| Error::Invalid("event size overflow"))?;
            while bytes + incoming > event_budget {
                tx.execute("DELETE FROM events WHERE id IN (SELECT id FROM events ORDER BY at_ms,id LIMIT 128)",[])?;
                bytes = tx.query_row(
                    "SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM events",
                    [],
                    |r| r.get(0),
                )?;
            }
            tx.execute(
                "INSERT OR IGNORE INTO events(id,at_ms,body) VALUES(?1,?2,?3)",
                params![event.id, event.at_ms, body],
            )?;
            bytes += incoming;
        }
        let mut bytes: i64 = tx.query_row(
            "SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM events",
            [],
            |r| r.get(0),
        )?;
        while bytes > event_budget {
            tx.execute("DELETE FROM events WHERE id IN (SELECT id FROM events ORDER BY at_ms,id LIMIT 128)", [])?;
            bytes = tx.query_row(
                "SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM events",
                [],
                |r| r.get(0),
            )?;
        }
        let lower: Option<i64> = tx.query_row("SELECT min(at_ms) FROM events", [], |r| r.get(0))?;
        let body = serde_json::to_string(state)?;
        if body.len()
            > usize::try_from(self.max_bytes / 4)
                .map_err(|_| Error::Invalid("state budget overflow"))?
        {
            return Err(Error::Invalid("current state exceeds journal budget"));
        }
        tx.execute("INSERT INTO state(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [body])?;
        tx.commit()?;
        Ok(lower.unwrap_or(now).max(cutoff))
    }
    /// Ordered retained immutable events; bounded by the store's page limit.
    pub fn events(&self) -> Result<Vec<Event>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT body FROM events ORDER BY at_ms, length(id), id")?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    /// Allocated SQLite bytes; includes reusable free pages.
    pub fn bytes(&self) -> Result<u64, Error> {
        let count: u64 = self
            .connection
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let size: u64 = self
            .connection
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        Ok(count.saturating_mul(size))
    }
}
