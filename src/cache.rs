// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! SQLite persistence for the five-minute structured workspace snapshot.

use crate::{
    Error, WorkspaceSnapshot,
    hygiene::FocusBaseline,
    read_bound::{Deadline, decode_within},
};
use ix_trace_rs::implements;
use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Cache lifetime required by FR-004.
pub const CACHE_TTL_SECONDS: i64 = 300;

/// The lock wait SQLite connections use outside a bounded read.
const LEGACY_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Validates the database and its lock before transport and snapshot readers share it.

#[implements("FR-004-AC-22")]
pub(crate) fn select_cache_path(
    explicit: Option<&Path>,
    preferred: &Path,
    fallback: &Path,
) -> Result<PathBuf, Error> {
    let path = explicit.unwrap_or(preferred);
    match probe_cache_path(path, false) {
        Ok(()) => Ok(path.to_owned()),
        Err(error) if explicit.is_none() && cache_access_failure(&error) => {
            probe_cache_path(fallback, true).map_err(|fallback_error| {
                Error::CachePathUnavailable {
                    path: preferred.to_owned(),
                    source: Box::new(Error::CachePathUnavailable {
                        path: fallback.to_owned(),
                        source: Box::new(fallback_error),
                    }),
                }
            })?;
            Ok(fallback.to_owned())
        }
        Err(error) => Err(Error::CachePathUnavailable {
            path: path.to_owned(),
            source: Box::new(error),
        }),
    }
}

#[implements("FR-004-AC-22")]
fn cache_access_failure(error: &Error) -> bool {
    match error {
        Error::CachePermissions(_) | Error::CacheDirectory(_) => true,
        Error::Cache(rusqlite::Error::SqliteFailure(code, _)) => matches!(
            code.code,
            ErrorCode::ReadOnly
                | ErrorCode::CannotOpen
                | ErrorCode::PermissionDenied
                | ErrorCode::SystemIoFailure
        ),
        _ => false,
    }
}

#[implements("FR-004-AC-22")]
fn probe_cache_path(path: &Path, private: bool) -> Result<(), Error> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        if private {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
                match std::fs::DirBuilder::new().mode(0o700).create(parent) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(Error::CacheDirectory(error)),
                }
                let metadata =
                    std::fs::symlink_metadata(parent).map_err(Error::CachePermissions)?;
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || metadata.uid() != rustix::process::geteuid().as_raw()
                    || metadata.permissions().mode() & 0o777 != 0o700
                {
                    return Err(Error::CachePermissions(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "temporary cache directory must be owned by this user, mode 0700, and not a symlink",
                    )));
                }
            }
            #[cfg(not(unix))]
            std::fs::create_dir_all(parent).map_err(Error::CacheDirectory)?;
        } else {
            std::fs::create_dir_all(parent).map_err(Error::CacheDirectory)?;
        }
    }
    restrict_permissions(path)?;
    restrict_permissions(&refresh_lock_path(path))?;
    let connection = Connection::open(path).map_err(Error::Cache)?;
    connection
        .busy_timeout(LEGACY_BUSY_TIMEOUT)
        .map_err(Error::Cache)?;
    connection.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS ix_board_write_probe (value INTEGER); DROP TABLE ix_board_write_probe; ROLLBACK;").map_err(Error::Cache)?;
    Ok(())
}

#[implements("FR-004-AC-22", "FR-004-AC-23")]
fn refresh_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_owned();
    lock_path.push(".refresh.lock");
    PathBuf::from(lock_path)
}

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
    path: PathBuf,
    ttl: i64,
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
            path: path.to_owned(),
            ttl: ordinary_ttl()?,
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
            .checked_add(self.ttl)
            .ok_or(Error::CacheTimestamp)?;
        if expires_at <= now {
            return Ok(None);
        }
        serde_json::from_slice(&payload)
            .map(Some)
            .map_err(Error::CacheDecode)
    }

    pub(crate) fn last_complete(&self) -> Result<Option<WorkspaceSnapshot>, Error> {
        let payload = self
            .connection
            .query_row(
                "SELECT payload FROM workspace_snapshot WHERE cache_key = ?1",
                params![self.cache_key],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(Error::Cache)?;
        payload
            .map(|bytes| serde_json::from_slice(&bytes).map_err(Error::CacheDecode))
            .transpose()
    }

    pub(crate) fn baseline_within(
        &mut self,
        now: i64,
        deadline: &Deadline<'_>,
    ) -> Result<(Option<WorkspaceSnapshot>, bool), Error> {
        let key = self.cache_key.clone();
        let row =
            with_lock_budget(&mut self.connection, deadline, |connection| {
                connection.query_row(
            "SELECT payload,full_reconciled_at FROM workspace_snapshot WHERE cache_key=?1",[key],
            |row| Ok((row.get::<_,Vec<u8>>(0)?,row.get::<_,Option<i64>>(1)?))).optional()
            })?;
        deadline.remaining()?;
        match row {
            Some((payload, Some(full))) if now.saturating_sub(full) < 86_400 => {
                Ok((Some(decode_within(&payload, deadline)?), false))
            }
            _ => Ok((None, true)),
        }
    }

    pub(crate) fn last_full_reconciliation(&self) -> Result<Option<i64>, Error> {
        self.connection
            .query_row(
                "SELECT full_reconciled_at FROM workspace_snapshot WHERE cache_key = ?1",
                params![self.cache_key],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(Error::Cache)
    }

    pub(crate) fn revision(&self) -> Result<Option<i64>, Error> {
        self.connection
            .query_row(
                "SELECT refresh_revision FROM workspace_snapshot WHERE cache_key=?1",
                params![self.cache_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::Cache)
    }

    pub(crate) fn source_started_at(&self) -> Result<Option<i64>, Error> {
        let row = self.connection.query_row("SELECT fetched_at,payload,read_started_at,read_started_for,read_started_digest FROM workspace_snapshot WHERE cache_key=?1",
            params![self.cache_key],|row| Ok((row.get::<_,i64>(0)?,row.get::<_,Vec<u8>>(1)?,row.get::<_,Option<i64>>(2)?,row.get::<_,Option<i64>>(3)?,row.get::<_,Option<String>>(4)?))).optional().map_err(Error::Cache)?;
        match row {
            Some((fetched, payload, Some(started), Some(for_time), Some(digest)))
                if fetched == for_time && digest == payload_digest(&payload) =>
            {
                Ok(Some(started))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn replace_refresh(
        &mut self,
        now: i64,
        snapshot: &WorkspaceSnapshot,
        full: bool,
    ) -> Result<(), Error> {
        let payload = serde_json::to_vec(snapshot).map_err(Error::CacheEncode)?;
        let transaction = self.connection.transaction().map_err(Error::Cache)?;
        transaction.execute("INSERT INTO workspace_snapshot(cache_key,fetched_at,payload,read_started_at,read_started_for,read_started_digest,full_reconciled_at)
            VALUES(?1,?2,?3,?2,?2,?4,?5) ON CONFLICT(cache_key) DO UPDATE SET fetched_at=excluded.fetched_at,payload=excluded.payload,
            read_started_at=excluded.read_started_at,read_started_for=excluded.read_started_for,read_started_digest=excluded.read_started_digest,
            full_reconciled_at=COALESCE(excluded.full_reconciled_at,workspace_snapshot.full_reconciled_at),refresh_revision=workspace_snapshot.refresh_revision+1",
            params![self.cache_key,now,payload,payload_digest(&payload),full.then_some(now)]).map_err(Error::Cache)?;
        transaction.commit().map_err(Error::Cache)
    }

    pub(crate) fn refresh_lock(
        &self,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<RefreshLock, Error> {
        RefreshLock::acquire(&self.path, deadline)
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
        full: bool,
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
                     read_started_digest,full_reconciled_at)
                 VALUES (?1, ?2, ?3, ?4, ?2, ?5,?6)
                 ON CONFLICT(cache_key) DO UPDATE SET fetched_at = excluded.fetched_at,
                   payload = excluded.payload, read_started_at = excluded.read_started_at,
                   read_started_for = excluded.read_started_for,
                   read_started_digest = excluded.read_started_digest,
                   full_reconciled_at=COALESCE(excluded.full_reconciled_at,workspace_snapshot.full_reconciled_at),
                   refresh_revision=workspace_snapshot.refresh_revision+1",
                params![
                    cache_key,
                    snapshot.fetched_at,
                    payload,
                    read_started_at,
                    digest,
                    full.then_some(read_started_at)
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

/// An adjacent advisory lock, released by the operating system on process exit.
pub(crate) struct RefreshLock {
    _file: std::fs::File,
}

#[derive(Clone, Copy, Default)]
struct LockOwner {
    pid: Option<u32>,
    acquired_at_ms: Option<u64>,
}

impl LockOwner {
    fn diagnostic(self) -> String {
        let pid = self
            .pid
            .map_or_else(|| "unknown".to_owned(), |pid| pid.to_string());
        let age = unix_millis()
            .zip(self.acquired_at_ms)
            .and_then(|(now, acquired)| now.checked_sub(acquired))
            .map_or_else(|| "unknown".to_owned(), |age| format!("{}s", age / 1000));
        format!("pid={pid} age={age}")
    }

    fn is_dead(self) -> bool {
        #[cfg(unix)]
        {
            self.pid
                .and_then(|pid| i32::try_from(pid).ok())
                .and_then(rustix::process::Pid::from_raw)
                .is_some_and(|pid| {
                    rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
                })
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

fn unix_millis() -> Option<u64> {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}

fn read_lock_owner(file: &mut std::fs::File) -> LockOwner {
    use std::io::{Read, Seek};
    let mut bytes = Vec::new();
    if file.rewind().is_err() || (&mut *file).take(512).read_to_end(&mut bytes).is_err() {
        return LockOwner::default();
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return LockOwner::default();
    };
    let mut owner = LockOwner::default();
    for line in text.lines().filter(|line| !line.is_empty()) {
        match line.split_once('=') {
            Some(("pid", value)) if owner.pid.is_none() => {
                owner.pid = value.parse().ok().filter(|pid| *pid > 0)
            }
            Some(("acquired_at_ms", value)) if owner.acquired_at_ms.is_none() => {
                owner.acquired_at_ms = value.parse().ok()
            }
            _ => return LockOwner::default(),
        }
    }
    // Legacy writers recorded only a PID. Their file's modification time is
    // the best available age evidence; the OS lock remains authoritative.
    if owner.acquired_at_ms.is_none() {
        owner.acquired_at_ms = file
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|age| u64::try_from(age.as_millis()).ok());
    }
    owner
}

impl RefreshLock {
    #[implements("FR-004-AC-20", "FR-004-AC-23")]
    pub(crate) fn acquire(path: &Path, deadline: Option<&Deadline<'_>>) -> Result<Self, Error> {
        use std::io::{Seek, Write};
        let lock_path = refresh_lock_path(path);
        restrict_permissions(&lock_path).map_err(|error| Error::CachePathUnavailable {
            path: lock_path.clone(),
            source: Box::new(error),
        })?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(Error::CachePermissions)?;
        let started = Instant::now();
        let mut announced = false;
        loop {
            if let Some(deadline) = deadline {
                deadline.remaining()?;
            }
            match file.try_lock() {
                Ok(()) => {
                    let previous = read_lock_owner(&mut file);
                    if previous.is_dead() {
                        let _ = writeln!(
                            std::io::stderr().lock(),
                            "ix-board: recovered Linear refresh lock at {}; dead holder {}; caller may proceed",
                            lock_path.display(),
                            previous.diagnostic()
                        );
                    }
                    file.rewind().map_err(Error::CachePermissions)?;
                    file.set_len(0).map_err(Error::CachePermissions)?;
                    writeln!(file, "pid={}", std::process::id())
                        .map_err(Error::CachePermissions)?;
                    if let Some(now) = unix_millis() {
                        writeln!(file, "acquired_at_ms={now}").map_err(Error::CachePermissions)?;
                    }
                    file.flush().map_err(Error::CachePermissions)?;
                    return Ok(Self { _file: file });
                }
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(Error::CachePermissions(error));
                }
            }
            let left = LEGACY_BUSY_TIMEOUT.saturating_sub(started.elapsed());
            if left.is_zero() {
                let holder = read_lock_owner(&mut file).diagnostic();
                return Err(Error::RefreshLockHeld {
                    path: lock_path,
                    holder,
                });
            }
            let wait = match deadline {
                Some(deadline) => left.min(deadline.remaining()?),
                None => left,
            };
            if !announced {
                let holder = read_lock_owner(&mut file).diagnostic();
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "ix-board: waiting for Linear refresh lock at {}; recorded holder {}; wait limited to {}ms; caller has not completed",
                    lock_path.display(),
                    holder,
                    wait.as_millis()
                );
                announced = true;
            }
            std::thread::sleep(wait.min(Duration::from_millis(10)));
        }
    }
}

impl Drop for RefreshLock {
    #[implements("FR-004-AC-23")]
    fn drop(&mut self) {
        // The OS lock remains held until the file field is dropped. Clearing
        // diagnostic metadata cannot unlock or replace another owner's file.
        let _ = self._file.set_len(0);
    }
}

pub(crate) fn ordinary_ttl() -> Result<i64, Error> {
    match std::env::var("IX_BOARD_CACHE_TTL_SECONDS") {
        Ok(value) => value
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or(Error::InvalidCacheTtl),
        Err(std::env::VarError::NotPresent) => match crate::settings::load()?
            .as_ref()
            .and_then(|config| config.get("cache"))
            .and_then(|cache| cache.get("ttl_seconds"))
        {
            Some(value) => value
                .as_integer()
                .filter(|value| *value > 0)
                .ok_or(Error::InvalidCacheTtl),
            None => Ok(CACHE_TTL_SECONDS),
        },
        Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidCacheTtl),
    }
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
        ("full_reconciled_at", "INTEGER"),
        ("refresh_revision", "INTEGER NOT NULL DEFAULT 0"),
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
fn restrict_permissions_with(
    file: &std::fs::File,
    chmod: impl FnOnce(&std::fs::File, std::fs::Permissions) -> std::io::Result<()>,
) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    if file
        .metadata()
        .map_err(Error::CachePermissions)?
        .permissions()
        .mode()
        & 0o777
        == 0o600
    {
        return Ok(());
    }
    let desired = std::fs::Permissions::from_mode(0o600);
    match chmod(file, desired) {
        Ok(()) => Ok(()),
        Err(chmod_error) => {
            let mode_is_restricted = file
                .metadata()
                .map(|metadata| metadata.permissions().mode() & 0o777 == 0o600)
                .unwrap_or(false);
            if mode_is_restricted {
                Ok(())
            } else {
                Err(Error::CachePermissions(chmod_error))
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn restrict_permissions(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(Error::CachePermissions)?;
    restrict_permissions_with(&file, |file, permissions| file.set_permissions(permissions))
}

#[cfg(not(unix))]
pub(crate) fn restrict_permissions(_path: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FocusBaseline, FocusBaselineIssue, IssueId, WorkflowStateType};
    use ix_trace_rs::trace;

    #[cfg(unix)]
    #[test]
    #[trace("FR-004-AC-19", "FR-004-AC-24")]
    fn dead_pid_metadata_never_bypasses_a_still_held_os_lock() {
        use std::os::unix::fs::MetadataExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        let lock_path = refresh_lock_path(&path);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&lock_path)
            .unwrap();
        file.lock().unwrap();
        let mut dead = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = dead.id();
        assert!(dead.wait().unwrap().success());
        std::fs::write(&lock_path, format!("pid={dead_pid}\nacquired_at_ms=1\n")).unwrap();
        let inode = file.metadata().unwrap().ino();
        let clock = SystemClock;
        let deadline = Deadline::start(&clock, Duration::from_millis(25));
        assert!(matches!(
            RefreshLock::acquire(&path, Some(&deadline)),
            Err(Error::ReadTimeout { .. })
        ));
        assert_eq!(std::fs::metadata(&lock_path).unwrap().ino(), inode);
        let contender = std::fs::File::open(lock_path).unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
    }

    #[test]
    #[trace("FR-004-AC-22")]
    fn cache_selection_falls_back_and_preserves_explicit_override_errors() {
        let directory = tempfile::tempdir().unwrap();
        let blocked = directory.path().join("not-a-directory");
        std::fs::write(&blocked, b"occupied").unwrap();
        let preferred = blocked.join("linear.sqlite3");
        let fallback = directory.path().join("fallback/linear.sqlite3");
        assert_eq!(
            select_cache_path(None, &preferred, &fallback).unwrap(),
            fallback
        );
        SnapshotCache::open(&fallback, "test").unwrap();
        let error = select_cache_path(Some(&preferred), &preferred, &fallback).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(&preferred.display().to_string()),
            "{message}"
        );
        assert!(message.contains("IX_BOARD_CACHE_PATH"), "{message}");
        assert!(message.contains("writable"), "{message}");
    }

    #[test]
    #[trace("FR-004-AC-22")]
    fn cache_selection_checks_sqlite_writes_and_lock_path_without_replacing_data() {
        let directory = tempfile::tempdir().unwrap();
        let preferred = directory.path().join("preferred.sqlite3");
        let fallback = directory.path().join("fallback/linear.sqlite3");
        std::fs::write(&preferred, b"not sqlite").unwrap();
        assert!(
            select_cache_path(None, &preferred, &fallback).is_err(),
            "corruption is actionable, never hidden by fallback"
        );
        assert_eq!(std::fs::read(&preferred).unwrap(), b"not sqlite");
        std::fs::remove_file(&preferred).unwrap();
        std::fs::remove_file(refresh_lock_path(&preferred)).unwrap();
        std::fs::create_dir(refresh_lock_path(&preferred)).unwrap();
        assert_eq!(
            select_cache_path(None, &preferred, &fallback).unwrap(),
            fallback
        );
    }

    #[test]
    #[trace("FR-004-AC-23")]
    fn refresh_lock_records_current_pid_and_names_holder_on_timeout() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.sqlite3");
        let lock = RefreshLock::acquire(&path, None).unwrap();
        let metadata = std::fs::read_to_string(refresh_lock_path(&path)).unwrap();
        assert!(
            metadata.contains(&format!("pid={}", std::process::id())),
            "{metadata}"
        );
        let started = Instant::now();
        let error = RefreshLock::acquire(&path, None)
            .err()
            .expect("held lock times out");
        assert!(started.elapsed() < Duration::from_secs(6));
        let message = error.to_string();
        assert!(
            message.contains(&format!("pid={}", std::process::id())),
            "{message}"
        );
        assert!(
            message.contains(&refresh_lock_path(&path).display().to_string()),
            "{message}"
        );
        drop(lock);
        let mut stale = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(refresh_lock_path(&path))
            .unwrap();
        stale.write_all(b"pid=999999\n").unwrap();
        let _recovered = RefreshLock::acquire(&path, None).unwrap();
        assert!(
            !std::fs::read_to_string(refresh_lock_path(&path))
                .unwrap()
                .contains("999999")
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "native unprivileged Unix SQLite directory-permission acceptance"]
    #[trace("FR-004-AC-22")]
    fn cache_selection_recovers_from_readonly_sqlite_journal_directory() {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            rustix::process::geteuid().as_raw(),
            0,
            "run as an unprivileged user"
        );
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("readonly");
        std::fs::create_dir(&parent).unwrap();
        let preferred = parent.join("linear.sqlite3");
        let connection = Connection::open(&preferred).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES('saved');",
            )
            .unwrap();
        drop(connection);
        restrict_permissions(&preferred).unwrap();
        restrict_permissions(&refresh_lock_path(&preferred)).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o500)).unwrap();
        let fallback = directory.path().join("private/linear.sqlite3");
        let selected = select_cache_path(None, &preferred, &fallback);
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(selected.unwrap(), fallback);
        let connection = Connection::open(preferred).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT value FROM retained", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "saved"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_permissions_skip_denied_chmod_when_already_restricted() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("cache dir");
        let path = directory.path().join("permissions.sqlite3");
        std::fs::write(&path, b"").expect("cache file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("restrict mode");
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open cache");
        let chmod_called = std::cell::Cell::new(false);

        restrict_permissions_with(&file, |_, _| {
            chmod_called.set(true);
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .expect("already restricted cache is accepted");

        assert!(
            !chmod_called.get(),
            "chmod must not be attempted for mode 0600"
        );
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn cache_permissions_ignore_denial_only_if_mode_became_restricted() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("cache dir");
        let path = directory.path().join("permissions.sqlite3");
        std::fs::write(&path, b"").expect("cache file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("set permissive mode");
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open cache");

        let error = restrict_permissions_with(&file, |file, _| {
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert!(error.is_ok(), "a concurrent mode fix makes denial harmless");
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("restore permissive mode");
        let error = restrict_permissions_with(&file, |_, _| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert!(
            matches!(error, Err(Error::CachePermissions(error)) if error.kind() == std::io::ErrorKind::PermissionDenied)
        );
    }

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
