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
    cache_key: String,
}

impl SnapshotCache {
    /// Opens or creates the cache database at `path`.
    pub(crate) fn open(path: &Path, cache_key: &str) -> Result<Self, Error> {
        restrict_permissions(path)?;
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
        Ok(Self {
            connection,
            cache_key: cache_key.to_owned(),
        })
    }

    pub(crate) fn fresh(&self, now: i64) -> Result<Option<WorkspaceSnapshot>, Error> {
        let row = self
            .connection
            .query_row(
                "SELECT fetched_at, payload FROM workspace_snapshot WHERE cache_key = ?1",
                params![self.cache_key],
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
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at,
                   payload = excluded.payload",
                params![self.cache_key, now, payload],
            )
            .map_err(Error::Cache)?;
        Ok(())
    }

    pub(crate) fn invalidate(&mut self) -> Result<(), Error> {
        self.connection
            .execute(
                "DELETE FROM workspace_snapshot WHERE cache_key = ?1",
                params![self.cache_key],
            )
            .map_err(Error::Cache)?;
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), Error> {
    use std::{
        fs::OpenOptions,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(Error::CachePermissions)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(Error::CachePermissions)
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), Error> {
    Ok(())
}
