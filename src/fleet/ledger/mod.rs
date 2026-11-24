// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The fleet ledger (FR-039): a schema-versioned SQLite store outside git, written under a
//! single-writer lock with the atomic-commit discipline of the team-metrics journal.
//!
//! A writer holds an exclusive lock on `<ledger>.lock`, assigns a monotonic run sequence and
//! commits each run in one transaction, so a crash leaves the previous state. Readers open the
//! file read-only and never migrate.

mod schema;
#[cfg(test)]
mod tests;

use super::FleetConfig;
use super::classify::ClassKind;
use super::report::{Defect, FleetReport};
use rusqlite::{Connection, OpenFlags, params};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error as ThisError;

pub use schema::{QUERYABLE, VERSION};

/// A ledger failure.
#[derive(Debug, ThisError)]
pub enum LedgerError {
    /// No state directory could be resolved.
    #[error("cannot resolve the ledger path: set IX_BOARD_LEDGER_PATH, XDG_STATE_HOME or HOME")]
    NoStateDir,
    /// The ledger path lies inside a git work tree.
    #[error("refusing to write the ledger to {path}: it is inside a git work tree ({git})")]
    InsideGit {
        /// Resolved ledger path.
        path: PathBuf,
        /// The `.git` entry found.
        git: PathBuf,
    },
    /// The ledger has a schema version this build does not understand.
    #[error("{path}: unsupported ledger schema version {found}; this build writes {VERSION}")]
    UnsupportedVersion {
        /// Ledger file.
        path: PathBuf,
        /// Version found.
        found: i64,
    },
    /// The file is a database but not a fleet ledger.
    #[error("{0}: not a fleet ledger")]
    NotALedger(PathBuf),
    /// The writer lock was not free within the bound.
    #[error("{path}: another writer holds the ledger lock (waited {waited:?})")]
    LockTimeout {
        /// Ledger file.
        path: PathBuf,
        /// How long this writer waited.
        waited: Duration,
    },
    /// The view is not queryable.
    #[error("unknown ledger view {0}")]
    UnknownView(String),
    /// The view carries no time to filter on.
    #[error("view {0} has no time column and does not take --since")]
    NoSince(String),
    /// The ledger file does not exist.
    #[error("{0}: no ledger yet; `fleet status` creates it")]
    Missing(PathBuf),
    /// Filesystem failure.
    #[error("{path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
    /// SQLite failure.
    #[error("ledger database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A value could not be encoded.
    #[error("ledger value: {0}")]
    Json(#[from] serde_json::Error),
}

/// The default ledger file under the user state directory.
///
/// # Errors
///
/// [`LedgerError::NoStateDir`] when neither `XDG_STATE_HOME` nor a home directory exists.
pub fn default_path() -> Result<PathBuf, LedgerError> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .ok_or(LedgerError::NoStateDir)?;
    Ok(base.join("ix-board").join("fleet-ledger.sqlite3"))
}

/// Resolves the ledger path: `IX_BOARD_LEDGER_PATH`, then the configured path, then the default.
///
/// # Errors
///
/// [`LedgerError::NoStateDir`] when there is no default.
pub fn resolve_path(config: &FleetConfig) -> Result<PathBuf, LedgerError> {
    if let Some(path) = std::env::var_os("IX_BOARD_LEDGER_PATH").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    match &config.ledger_path {
        Some(path) => Ok(path.clone()),
        None => default_path(),
    }
}

/// Refuses a path whose existing ancestors include a git work tree.
fn refuse_inside_git(path: &Path) -> Result<(), LedgerError> {
    let io = |source| LedgerError::Io {
        path: path.to_owned(),
        source,
    };
    let absolute = std::path::absolute(path).map_err(io)?;
    let mut existing = absolute.as_path();
    let mut tail = Vec::new();
    while !existing.exists() {
        let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
            break;
        };
        tail.push(name.to_owned());
        existing = parent;
    }
    let mut resolved = std::fs::canonicalize(existing).map_err(io)?;
    resolved.extend(tail.iter().rev());
    for directory in resolved.ancestors().skip(1) {
        let git = directory.join(".git");
        if std::fs::symlink_metadata(&git).is_ok() {
            return Err(LedgerError::InsideGit {
                path: resolved,
                git,
            });
        }
    }
    Ok(())
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// The exclusive writer lock; released when dropped.
struct WriterLock {
    #[cfg(unix)]
    _file: std::fs::File,
    #[cfg(not(unix))]
    path: PathBuf,
}

impl WriterLock {
    #[cfg(unix)]
    fn acquire(ledger: &Path, wait: Duration) -> Result<Self, LedgerError> {
        use rustix::fs::{FlockOperation, flock};
        let path = lock_path(ledger);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| LedgerError::Io {
                path: path.clone(),
                source,
            })?;
        let started = Instant::now();
        loop {
            match flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(Self { _file: file }),
                Err(rustix::io::Errno::WOULDBLOCK) if started.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(rustix::io::Errno::WOULDBLOCK) => {
                    return Err(LedgerError::LockTimeout {
                        path: ledger.to_owned(),
                        waited: started.elapsed(),
                    });
                }
                Err(errno) => {
                    return Err(LedgerError::Io {
                        path,
                        source: errno.into(),
                    });
                }
            }
        }
    }

    #[cfg(not(unix))]
    fn acquire(ledger: &Path, wait: Duration) -> Result<Self, LedgerError> {
        let path = lock_path(ledger);
        let started = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        && started.elapsed() < wait =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(LedgerError::LockTimeout {
                        path: ledger.to_owned(),
                        waited: started.elapsed(),
                    });
                }
                Err(source) => return Err(LedgerError::Io { path, source }),
            }
        }
    }
}

#[cfg(not(unix))]
impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_path(ledger: &Path) -> PathBuf {
    let mut name = ledger.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

fn user_version(connection: &Connection) -> Result<i64, LedgerError> {
    Ok(connection.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

fn has_tables(connection: &Connection) -> Result<bool, LedgerError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table')",
        [],
        |r| r.get(0),
    )?)
}

/// Applies the version 2 statements and views in the open transaction, failing at statement
/// `fail_at` when given (the crash injection of the migration test).
fn apply_v2(tx: &rusqlite::Transaction<'_>, fail_at: Option<usize>) -> Result<(), LedgerError> {
    let views = schema::views();
    let statements = schema::V2
        .iter()
        .copied()
        .chain(views.iter().map(String::as_str));
    for (index, statement) in statements.enumerate() {
        if fail_at == Some(index) {
            return Err(LedgerError::Sqlite(rusqlite::Error::ExecuteReturnedResults));
        }
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", schema::VERSION)?;
    Ok(())
}

/// An open ledger holding the writer lock.
pub struct Ledger {
    path: PathBuf,
    connection: Connection,
    retention: Duration,
    _lock: WriterLock,
}

impl Ledger {
    /// Opens or creates the ledger as the writer, holding the lock until it is dropped. A
    /// version 1 ledger migrates to version 2 in one transaction; a newer or foreign file is
    /// refused unchanged.
    ///
    /// # Errors
    ///
    /// A path inside git, a lock held past the bound, an unsupported schema or a database
    /// failure.
    pub fn open(path: &Path, config: &FleetConfig) -> Result<Self, LedgerError> {
        Self::open_with(path, config, None)
    }

    fn open_with(
        path: &Path,
        config: &FleetConfig,
        fail_migration_at: Option<usize>,
    ) -> Result<Self, LedgerError> {
        refuse_inside_git(path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LedgerError::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        let lock = WriterLock::acquire(path, config.ledger_lock_wait)?;
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "DELETE")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let version = user_version(&connection)?;
        match version {
            0 if has_tables(&connection)? => {
                return Err(LedgerError::NotALedger(path.to_owned()));
            }
            0 => {
                let tx = connection.transaction()?;
                tx.execute_batch(schema::V1)?;
                apply_v2(&tx, fail_migration_at)?;
                tx.commit()?;
            }
            1 => {
                let tx = connection.transaction()?;
                apply_v2(&tx, fail_migration_at)?;
                tx.commit()?;
            }
            2 => {}
            found => {
                return Err(LedgerError::UnsupportedVersion {
                    path: path.to_owned(),
                    found,
                });
            }
        }
        let ledger = Self {
            path: path.to_owned(),
            connection,
            retention: config.ledger_retention,
            _lock: lock,
        };
        ledger.write_meta(config)?;
        Ok(ledger)
    }

    fn write_meta(&self, config: &FleetConfig) -> Result<(), LedgerError> {
        let grace = config
            .grace
            .unwrap_or(config.cycle_interval + config.tick_interval);
        for (key, value) in [
            ("tick_interval_s", config.tick_interval.as_secs()),
            ("cycle_interval_s", config.cycle_interval.as_secs()),
            ("grace_s", grace.as_secs()),
        ] {
            self.connection.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value.to_string()],
            )?;
        }
        Ok(())
    }

    /// The ledger file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one `fleet status` run: the run record, one `lane_tick` row per lane, the
    /// per-host figures and the defect rows, then prunes by retention. Returns each report
    /// defect's first-seen time, in report order. The whole run commits or nothing does.
    ///
    /// # Errors
    ///
    /// A database failure; the previous state is intact.
    pub fn record_status(&mut self, report: &FleetReport) -> Result<Vec<i64>, LedgerError> {
        self.record_inner(report, false)
    }

    fn record_inner(
        &mut self,
        report: &FleetReport,
        crash_before_commit: bool,
    ) -> Result<Vec<i64>, LedgerError> {
        let run_ms = report.run_ms;
        let retention = millis(self.retention);
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO run(kind, relay_ms) VALUES('status', ?1)",
            [run_ms],
        )?;
        let sequence = tx.last_insert_rowid();
        {
            let mut lane = tx.prepare(
                "INSERT INTO lane_tick(run_seq, lane, host, class, reason, observed_ms,
                   last_event_ms, evidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            )?;
            for row in &report.rows {
                lane.execute(params![
                    sequence,
                    row.lane,
                    row.host,
                    class_name(row.class),
                    row.reason,
                    run_ms,
                    row.last_event_ms,
                    serde_json::to_string(&row.evidence)?,
                ])?;
            }
            let mut host = tx.prepare(
                "INSERT INTO host_tick(run_seq, host, observed_ms, lanes, working, not_working)
                 VALUES(?1,?2,?3,?4,?5,?6)",
            )?;
            for summary in &report.hosts {
                host.execute(params![
                    sequence,
                    summary.host,
                    run_ms,
                    count(summary.lanes),
                    count(summary.working),
                    count(summary.not_working),
                ])?;
            }
        }
        let first_seen = reconcile_defects(&tx, &report.defects, run_ms)?;
        if crash_before_commit {
            return Err(LedgerError::Sqlite(rusqlite::Error::ExecuteReturnedResults));
        }
        prune(&tx, run_ms.saturating_sub(retention))?;
        tx.commit()?;
        Ok(first_seen)
    }
}

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn class_name(class: ClassKind) -> String {
    match serde_json::to_value(class) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

type DefectKey = (String, Option<String>, Option<String>, String);

fn defect_key(defect: &Defect) -> DefectKey {
    let kind = match serde_json::to_value(defect.kind) {
        Ok(serde_json::Value::String(kind)) => kind,
        _ => String::new(),
    };
    (
        defect.source.clone(),
        defect.host.clone(),
        defect.lane.clone(),
        kind,
    )
}

/// Opens a row for each defect not already open, closes the open rows no longer present, and
/// returns the first-seen time of every defect.
fn reconcile_defects(
    tx: &rusqlite::Transaction<'_>,
    defects: &[Defect],
    run_ms: i64,
) -> Result<Vec<i64>, LedgerError> {
    let mut open: BTreeMap<DefectKey, (i64, i64)> = BTreeMap::new();
    {
        let mut statement = tx.prepare(
            "SELECT defect_id, source, host, lane, kind, first_seen_at
             FROM defect WHERE closed_at IS NULL",
        )?;
        let rows = statement.query_map([], |r| {
            Ok((
                (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?),
                (r.get::<_, i64>(0)?, r.get::<_, i64>(5)?),
            ))
        })?;
        for row in rows {
            let (key, value) = row?;
            open.insert(key, value);
        }
    }
    let mut first_seen = Vec::with_capacity(defects.len());
    let mut present: BTreeMap<DefectKey, i64> = BTreeMap::new();
    for defect in defects {
        let key = defect_key(defect);
        if let Some(seen) = present.get(&key) {
            first_seen.push(*seen);
            continue;
        }
        let seen = if let Some((_, seen)) = open.get(&key) {
            *seen
        } else {
            tx.execute(
                "INSERT INTO defect(source, host, lane, kind, first_seen_at)
                 VALUES(?1,?2,?3,?4,?5)",
                params![key.0, key.1, key.2, key.3, run_ms],
            )?;
            run_ms
        };
        present.insert(key, seen);
        first_seen.push(seen);
    }
    for (key, (id, _)) in &open {
        if !present.contains_key(key) {
            tx.execute(
                "UPDATE defect SET closed_at=?1 WHERE defect_id=?2",
                params![run_ms, id],
            )?;
        }
    }
    Ok(first_seen)
}

/// Deletes rows older than `cutoff`; runs go last so no row outlives its run.
fn prune(tx: &rusqlite::Transaction<'_>, cutoff: i64) -> Result<(), LedgerError> {
    for (table, column) in schema::PRUNE {
        tx.execute(
            &format!("DELETE FROM {table} WHERE {column} IS NOT NULL AND {column} < ?1"),
            [cutoff],
        )?;
    }
    tx.execute("DELETE FROM run WHERE relay_ms < ?1", [cutoff])?;
    Ok(())
}

/// Writes one `fleet status` run to the ledger and fills each defect's `first_seen_ms`. When
/// the ledger cannot be written the report gains a `ledger_unwritable` defect instead; the
/// command never stays silent about it.
pub fn record_status(config: &FleetConfig, report: &mut FleetReport) {
    let written = resolve_path(config)
        .and_then(|path| Ledger::open(&path, config))
        .and_then(|mut ledger| ledger.record_status(report));
    match written {
        Ok(first_seen) => {
            for (defect, seen) in report.defects.iter_mut().zip(first_seen) {
                defect.first_seen_ms = Some(seen);
            }
        }
        Err(error) => report.add_ledger_defect(error.to_string()),
    }
}

/// The rows `fleet ledger query` prints.
#[derive(Clone, Debug, serde::Serialize)]
pub struct LedgerRows {
    /// View name.
    pub view: String,
    /// Lower bound applied to `at_ms`, relay server milliseconds.
    pub since_ms: Option<i64>,
    /// One object per row, columns by name.
    pub rows: Vec<serde_json::Map<String, serde_json::Value>>,
}

impl LedgerRows {
    /// One tab-separated line per row, `column=value` pairs in column order.
    pub fn render_text(&self) -> String {
        let mut lines = vec![format!("view: {}", self.view)];
        for row in &self.rows {
            let cells: Vec<String> = row
                .iter()
                .map(|(name, value)| match value {
                    serde_json::Value::String(text) => format!("{name}={text}"),
                    other => format!("{name}={other}"),
                })
                .collect();
            lines.push(cells.join("\t"));
        }
        lines.join("\n")
    }
}

/// Rows of one view as JSON objects.
///
/// # Errors
///
/// An unknown view, `--since` on a view without a time, a missing or newer ledger, or a
/// database failure. Nothing is written, migrated or locked.
pub fn query(path: &Path, view: &str, since: Option<Duration>) -> Result<LedgerRows, LedgerError> {
    if !QUERYABLE.contains(&view) {
        return Err(LedgerError::UnknownView(view.to_owned()));
    }
    if since.is_some() && schema::UNDATED.contains(&view) {
        return Err(LedgerError::NoSince(view.to_owned()));
    }
    if !path.exists() {
        return Err(LedgerError::Missing(path.to_owned()));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let found = user_version(&connection)?;
    if found != schema::VERSION {
        return Err(LedgerError::UnsupportedVersion {
            path: path.to_owned(),
            found,
        });
    }
    // `since` is measured back from the newest run on the relay clock, never this clock.
    let since_ms = match since {
        Some(window) => {
            let newest: Option<i64> =
                connection.query_row("SELECT MAX(relay_ms) FROM run", [], |r| r.get(0))?;
            newest.map(|newest| newest.saturating_sub(millis(window)))
        }
        None => None,
    };
    let sql = if since_ms.is_some() {
        format!("SELECT * FROM {view} WHERE at_ms >= ?1 ORDER BY at_ms")
    } else {
        format!("SELECT * FROM {view}")
    };
    let mut statement = connection.prepare(&sql)?;
    let names: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = if let Some(ms) = since_ms {
        statement.query([ms])?
    } else {
        statement.query([])?
    };
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = serde_json::Map::new();
        for (index, name) in names.iter().enumerate() {
            let value = match row.get_ref(index)? {
                rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                rusqlite::types::ValueRef::Integer(n) => n.into(),
                rusqlite::types::ValueRef::Real(f) => serde_json::Number::from_f64(f)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number),
                rusqlite::types::ValueRef::Text(t) => {
                    serde_json::Value::String(String::from_utf8_lossy(t).into_owned())
                }
                rusqlite::types::ValueRef::Blob(_) => serde_json::Value::Null,
            };
            object.insert(name.clone(), value);
        }
        out.push(object);
    }
    Ok(LedgerRows {
        view: view.to_owned(),
        since_ms,
        rows: out,
    })
}
