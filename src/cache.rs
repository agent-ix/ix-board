// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! SQLite persistence for the five-minute structured workspace snapshot.

use crate::{
    Error, WorkspaceSnapshot,
    hygiene::FocusBaseline,
    read_bound::{Deadline, decode_within},
};
use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

/// Cache lifetime required by FR-004.
pub const CACHE_TTL_SECONDS: i64 = 300;

/// The lock wait SQLite connections use outside a bounded read.
const LEGACY_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Read-only source of Unix timestamps, replaceable for deterministic tests.
pub trait Clock: Send + Sync {
    /// Current Unix timestamp in seconds.
    fn now_epoch_seconds(&self) -> i64;

    /// A monotonic instant that measures the deadline of a bounded read.
    /// Override it to control elapsed time in tests.
    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }
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
        add_read_start_columns(&connection)?;
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
                   payload = excluded.payload, read_started_at = NULL,
                   read_started_for = NULL, read_started_digest = NULL",
                params![self.cache_key, now, payload],
            )
            .map_err(Error::Cache)?;
        Ok(())
    }

    /// Returns the cached snapshot only when the source read that produced it began no
    /// more than `max_age_seconds` ago. An entry without that evidence is not accepted.
    pub(crate) fn fresh_within(
        &mut self,
        now: i64,
        max_age_seconds: i64,
        deadline: &Deadline<'_>,
    ) -> Result<Option<WorkspaceSnapshot>, Error> {
        let cache_key = self.cache_key.clone();
        let row = with_lock_budget(&mut self.connection, deadline, |connection| {
            connection
                .query_row(
                    "SELECT fetched_at, payload, read_started_at, read_started_for,
                            read_started_digest
                     FROM workspace_snapshot WHERE cache_key = ?1",
                    params![cache_key],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .optional()
        })?;
        // The lookup and copy may have used the rest of the budget.
        deadline.remaining()?;
        let Some((fetched_at, payload, Some(started), Some(started_for), Some(digest))) = row
        else {
            return Ok(None);
        };
        // The evidence only describes the payload it was written with. A writer that
        // does not keep it replaces the payload, possibly within the same second, so
        // equal timestamps prove nothing: the evidence must carry the payload's digest.
        if started_for != fetched_at || digest != payload_digest(&payload) {
            return Ok(None);
        }
        let expires_at = fetched_at
            .checked_add(CACHE_TTL_SECONDS)
            .ok_or(Error::CacheTimestamp)?;
        let age = now.checked_sub(started).ok_or(Error::CacheTimestamp)?;
        if expires_at <= now || age < 0 || age > max_age_seconds {
            return Ok(None);
        }
        // Decoding checks the deadline as it goes and again before it returns success.
        decode_within(&payload, deadline).map(Some)
    }

    /// Stores a refreshed snapshot with the time its source read began, unless the
    /// deadline has already expired when the write would commit.
    pub(crate) fn replace_bounded(
        &mut self,
        snapshot: &WorkspaceSnapshot,
        read_started_at: i64,
        deadline: &Deadline<'_>,
    ) -> Result<(), Error> {
        let payload = serde_json::to_vec(snapshot).map_err(Error::CacheEncode)?;
        let digest = payload_digest(&payload);
        let cache_key = self.cache_key.clone();
        let committed = with_lock_budget(&mut self.connection, deadline, |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "INSERT INTO workspace_snapshot(
                     cache_key, fetched_at, payload, read_started_at, read_started_for,
                     read_started_digest)
                 VALUES (?1, ?2, ?3, ?4, ?2, ?5)
                 ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at,
                   payload = excluded.payload, read_started_at = excluded.read_started_at,
                   read_started_for = excluded.read_started_for,
                   read_started_digest = excluded.read_started_digest",
                params![
                    cache_key,
                    snapshot.fetched_at,
                    payload,
                    read_started_at,
                    digest
                ],
            )?;
            if deadline.remaining().is_err() {
                // Dropping the transaction rolls it back: nothing is stored after expiry.
                return Ok(false);
            }
            transaction.commit()?;
            Ok(true)
        })?;
        if committed {
            Ok(())
        } else {
            Err(Error::ReadTimeout {
                deadline: deadline.limit(),
            })
        }
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

/// The SHA-256 of a stored payload, as lowercase hex. Read-start evidence carries it so
/// that it only ever vouches for the exact payload it was written with.
fn payload_digest(payload: &[u8]) -> String {
    format!("{:x}", Sha256::digest(payload))
}

/// Adds the evidence columns of a bounded read. Old readers select and write only the
/// original columns, so the table stays usable by them.
fn add_read_start_columns(connection: &Connection) -> Result<(), Error> {
    let mut statement = connection
        .prepare("PRAGMA table_info(workspace_snapshot)")
        .map_err(Error::Cache)?;
    let present = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(Error::Cache)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::Cache)?;
    for (column, kind) in [
        ("read_started_at", "INTEGER"),
        ("read_started_for", "INTEGER"),
        ("read_started_digest", "TEXT"),
    ] {
        if present.iter().any(|name| name == column) {
            continue;
        }
        match connection.execute(
            &format!("ALTER TABLE workspace_snapshot ADD COLUMN {column} {kind}"),
            [],
        ) {
            Ok(_) => {}
            // Another process added it between the check and the statement.
            Err(error) if error.to_string().contains("duplicate column name") => {}
            Err(error) => return Err(Error::Cache(error)),
        }
    }
    Ok(())
}

/// Runs one cache operation whose lock wait is limited to the time remaining, then
/// restores the lock wait that every other operation uses.
fn with_lock_budget<T>(
    connection: &mut Connection,
    deadline: &Deadline<'_>,
    operation: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
) -> Result<T, Error> {
    connection
        .busy_timeout(deadline.remaining()?)
        .map_err(Error::Cache)?;
    let result = operation(connection);
    let restored = connection.busy_timeout(LEGACY_BUSY_TIMEOUT);
    let value = result.map_err(|error| match error {
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) =>
        {
            // The wait was the time remaining, so a lock still held means expiry.
            Error::ReadTimeout {
                deadline: deadline.limit(),
            }
        }
        other => Error::Cache(other),
    })?;
    restored.map_err(Error::Cache)?;
    Ok(value)
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
