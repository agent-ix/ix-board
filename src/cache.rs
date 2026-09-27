// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! SQLite persistence for the five-minute structured workspace snapshot.

use crate::{Error, WorkspaceSnapshot};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Cache lifetime required by FR-004.
pub const CACHE_TTL_SECONDS: i64 = 300;

/// Read-only source of Unix timestamps, replaceable for deterministic tests.
pub trait Clock: Send + Sync {
    /// Current Unix timestamp in seconds.
    fn now_epoch_seconds(&self) -> i64;
}

/// Wall clock used by the CLI cache.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch_seconds(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
            })
    }
}

/// SQLite-backed cache for a single workspace snapshot.
pub(crate) struct SnapshotCache {
    connection: Connection,
}

impl SnapshotCache {
    /// Opens or creates the cache database at `path`.
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let connection = Connection::open(path).map_err(Error::Cache)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS workspace_snapshot (
                    cache_key TEXT PRIMARY KEY,
                    fetched_at INTEGER NOT NULL,
                    payload BLOB NOT NULL
                );",
            )
            .map_err(Error::Cache)?;
        Ok(Self { connection })
    }

    pub(crate) fn fresh(&self, now: i64) -> Result<Option<WorkspaceSnapshot>, Error> {
        let row = self
            .connection
            .query_row(
                "SELECT fetched_at, payload FROM workspace_snapshot WHERE cache_key = 'workspace'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(Error::Cache)?;
        let Some((fetched_at, payload)) = row else {
            return Ok(None);
        };
        let expires_at = fetched_at
            .checked_add(CACHE_TTL_SECONDS)
            .ok_or(Error::CacheTimestamp)?;
        if expires_at <= now {
            return Ok(None);
        }
        serde_json::from_slice(&payload)
            .map(Some)
            .map_err(Error::CacheDecode)
    }

    pub(crate) fn replace(&mut self, now: i64, snapshot: &WorkspaceSnapshot) -> Result<(), Error> {
        let payload = serde_json::to_vec(snapshot).map_err(Error::CacheEncode)?;
        self.connection
            .execute(
                "INSERT INTO workspace_snapshot(cache_key, fetched_at, payload)
                 VALUES ('workspace', ?1, ?2)
                 ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at,
                   payload = excluded.payload",
                params![now, payload],
            )
            .map_err(Error::Cache)?;
        Ok(())
    }
}
