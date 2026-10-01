// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! SQLite persistence for the five-minute structured workspace snapshot.

use crate::{Error, WorkspaceSnapshot, hygiene::FocusBaseline};
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
                );
                CREATE TABLE IF NOT EXISTS focus_hygiene_baseline (
                    workspace_key TEXT NOT NULL,
                    focus_key TEXT NOT NULL,
                    report_time TEXT NOT NULL,
                    payload BLOB NOT NULL,
                    PRIMARY KEY (workspace_key, focus_key)
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

    pub(crate) fn hygiene_baseline(&self, focus_key: &str) -> Result<Option<FocusBaseline>, Error> {
        let payload = self.connection.query_row(
            "SELECT payload FROM focus_hygiene_baseline WHERE workspace_key = ?1 AND focus_key = ?2",
            params![self.cache_key, focus_key],
            |row| row.get::<_, Vec<u8>>(0),
        ).optional().map_err(Error::Cache)?;
        payload
            .map(|bytes| serde_json::from_slice(&bytes).map_err(Error::CacheDecode))
            .transpose()
    }

    pub(crate) fn replace_hygiene_baseline(
        &mut self,
        focus_key: &str,
        baseline: &FocusBaseline,
    ) -> Result<(), Error> {
        let payload = serde_json::to_vec(baseline).map_err(Error::CacheEncode)?;
        let transaction = self.connection.transaction().map_err(Error::Cache)?;
        transaction
            .execute(
                "INSERT INTO focus_hygiene_baseline(workspace_key, focus_key, report_time, payload)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(workspace_key, focus_key) DO UPDATE SET
               report_time = excluded.report_time, payload = excluded.payload",
                params![self.cache_key, focus_key, baseline.report_time, payload],
            )
            .map_err(Error::Cache)?;
        transaction.commit().map_err(Error::Cache)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FocusBaseline, FocusBaselineIssue, IssueId, WorkflowStateType};

    /// Trace: FR-013-AC-7
    #[test]
    fn tc_013_07_baseline_upsert_is_per_workspace_and_canonical_focus() {
        let directory = tempfile::tempdir().expect("cache dir");
        let path = directory.path().join("linear.sqlite3");
        let mut cache = SnapshotCache::open(&path, "workspace-one").expect("open cache");
        let saved = FocusBaseline {
            report_time: "2026-02-01T00:00:00Z".to_owned(),
            issues: vec![FocusBaselineIssue {
                id: IssueId("IR-1".to_owned()),
                in_focus: true,
                state: "Todo".to_owned(),
                state_type: WorkflowStateType::Started,
                project_id: None,
                milestone_id: None,
                blocked_by: vec![],
            }],
        };
        cache
            .replace_hygiene_baseline("[\"project:p1\"]", &saved)
            .expect("atomic baseline upsert");
        assert_eq!(
            cache
                .hygiene_baseline("[\"project:p1\"]")
                .expect("read baseline"),
            Some(saved.clone())
        );
        assert_eq!(
            cache
                .hygiene_baseline("[\"project:p2\"]")
                .expect("other focus"),
            None
        );
        cache
            .replace_hygiene_baseline(
                "[\"project:p1\"]",
                &FocusBaseline {
                    report_time: "2026-02-02T00:00:00Z".to_owned(),
                    issues: vec![],
                },
            )
            .expect("replace baseline");
        assert_eq!(
            cache
                .hygiene_baseline("[\"project:p1\"]")
                .expect("read replaced"),
            Some(FocusBaseline {
                report_time: "2026-02-02T00:00:00Z".to_owned(),
                issues: vec![]
            })
        );
        let other_workspace = SnapshotCache::open(&path, "workspace-two").expect("other workspace");
        assert_eq!(
            other_workspace
                .hygiene_baseline("[\"project:p1\"]")
                .expect("workspace isolation"),
            None
        );
    }
}
