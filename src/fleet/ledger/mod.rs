// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The fleet ledger (FR-039): a schema-versioned SQLite store outside git, written under a
//! single-writer lock with the atomic-commit discipline of the team-metrics journal.
//!
//! A writer holds an exclusive lock on `<ledger>.lock`, assigns a monotonic run sequence and
//! commits each run in one transaction, so a crash leaves the previous state. Readers open the
//! file read-only and never migrate.

mod assign_rows;
mod lock;
mod schema;
#[cfg(test)]
mod tests;

use self::lock::WriterLock;
use super::FleetConfig;
use super::classify::ClassKind;
use super::report::{Defect, FleetReport};
use rusqlite::{Connection, OpenFlags, params};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error as ThisError;

pub use assign_rows::{AssignSink, complete_pending};
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
    /// The ledger directory is on a filesystem that cannot lock a database safely.
    #[error(
        "{path}: the ledger is on a network filesystem ({kind}); SQLite locking is unsafe there, \
         set IX_BOARD_LEDGER_PATH to a local disk"
    )]
    NetworkFilesystem {
        /// Ledger file.
        path: PathBuf,
        /// Filesystem type or kind.
        kind: String,
    },
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
    default_path_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from),
    )
}

fn default_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf, LedgerError> {
    let base = xdg
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home.filter(|home| !home.as_os_str().is_empty())
                .map(|home| home.join(".local").join("state"))
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
    tick_interval_s: i64,
    cycle_interval_s: i64,
    grace_s: i64,
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
        lock::refuse_network(path)?;
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
        let secs = |d: Duration| i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
        let grace = config
            .grace
            .unwrap_or(config.cycle_interval + config.tick_interval);
        Ok(Self {
            path: path.to_owned(),
            connection,
            retention: config.ledger_retention,
            tick_interval_s: secs(config.tick_interval),
            cycle_interval_s: secs(config.cycle_interval),
            grace_s: secs(grace),
            _lock: lock,
        })
    }

    /// The ledger file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one `fleet status` run: the run record, one `lane_tick` row per lane, the
    /// per-host figures and the defect rows, then prunes by retention. Returns each report
    /// defect's first-seen time in relay milliseconds, in report order; a defect first seen in a
    /// run whose relay read failed has none. The whole run commits or nothing does.
    ///
    /// A run whose relay read failed records the host clock in `run.host_ms` and
    /// `relay_unavailable`, writes no lane rows, closes no defect and prunes nothing: host time
    /// never enters a relay-time column.
    ///
    /// # Errors
    ///
    /// A database failure; the previous state is intact.
    pub fn record_status(&mut self, report: &FleetReport) -> Result<Vec<Option<i64>>, LedgerError> {
        self.record_inner(report, false)
    }

    fn record_inner(
        &mut self,
        report: &FleetReport,
        crash_before_commit: bool,
    ) -> Result<Vec<Option<i64>>, LedgerError> {
        let unavailable = report.relay_unavailable;
        let run_ms = report.run_ms;
        let retention = millis(self.retention);
        let (relay_ms, host_ms) = if unavailable {
            (None, Some(run_ms))
        } else {
            (Some(run_ms), None)
        };
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO run(kind, relay_ms, host_ms, relay_unavailable, tick_interval_s,
               cycle_interval_s, grace_s) VALUES('status', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                relay_ms,
                host_ms,
                unavailable,
                self.tick_interval_s,
                self.cycle_interval_s,
                self.grace_s
            ],
        )?;
        let sequence = tx.last_insert_rowid();
        if !unavailable {
            let mut lane = tx.prepare(
                "INSERT INTO lane_tick(run_seq, source, lane, host, class, reason, observed_ms,
                   last_event_ms, evidence) VALUES(?1,'status',?2,?3,?4,?5,?6,?7,?8)",
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
        let first_seen = reconcile_defects(&tx, &report.defects, run_ms, unavailable)?;
        if crash_before_commit {
            return Err(LedgerError::Sqlite(rusqlite::Error::ExecuteReturnedResults));
        }
        if !unavailable {
            prune(&tx, run_ms.saturating_sub(retention))?;
        }
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

/// Opens a row for each defect not already open and returns the first-seen time of every
/// defect. When the relay was readable it also closes the open rows no longer present; a run
/// without the relay observed nothing about the other sources, so it closes nothing.
fn reconcile_defects(
    tx: &rusqlite::Transaction<'_>,
    defects: &[Defect],
    run_ms: i64,
    unavailable: bool,
) -> Result<Vec<Option<i64>>, LedgerError> {
    let mut open: BTreeMap<DefectKey, (i64, Option<i64>)> = BTreeMap::new();
    {
        let mut statement = tx.prepare(
            "SELECT defect_id, source, host, lane, kind, first_seen_at
             FROM defect WHERE closed_at IS NULL",
        )?;
        let rows = statement.query_map([], |r| {
            Ok((
                (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?),
                (r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(5)?),
            ))
        })?;
        for row in rows {
            let (key, value) = row?;
            open.insert(key, value);
        }
    }
    let mut first_seen = Vec::with_capacity(defects.len());
    let mut present: BTreeMap<DefectKey, Option<i64>> = BTreeMap::new();
    for defect in defects {
        let key = defect_key(defect);
        if let Some(seen) = present.get(&key) {
            first_seen.push(*seen);
            continue;
        }
        let seen = if let Some((_, seen)) = open.get(&key) {
            *seen
        } else {
            let (relay, host) = if unavailable {
                (None, Some(run_ms))
            } else {
                (Some(run_ms), None)
            };
            tx.execute(
                "INSERT INTO defect(source, host, lane, kind, first_seen_at, first_seen_host_ms)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![key.0, key.1, key.2, key.3, relay, host],
            )?;
            relay
        };
        present.insert(key, seen);
        first_seen.push(seen);
    }
    if !unavailable {
        for (key, (id, _)) in &open {
            if !present.contains_key(key) {
                tx.execute(
                    "UPDATE defect SET closed_at=?1 WHERE defect_id=?2",
                    params![run_ms, id],
                )?;
            }
        }
    }
    Ok(first_seen)
}

/// Deletes rows closed or recorded before `cutoff`; open rows stay. Runs go last so no row
/// outlives its run, and a run with no relay time goes with the runs before the oldest
/// relay-timed one (sequence order, never a clock comparison).
fn prune(tx: &rusqlite::Transaction<'_>, cutoff: i64) -> Result<(), LedgerError> {
    for (table, column) in schema::PRUNE {
        tx.execute(
            &format!("DELETE FROM {table} WHERE {column} IS NOT NULL AND {column} < ?1"),
            [cutoff],
        )?;
    }
    tx.execute(
        "DELETE FROM capacity_step_lane_output
         WHERE run_id NOT IN (SELECT run_id FROM capacity_run)",
        [],
    )?;
    tx.execute("DELETE FROM run WHERE relay_ms < ?1", [cutoff])?;
    tx.execute(
        "DELETE FROM run WHERE relay_ms IS NULL
           AND sequence < COALESCE((SELECT MIN(sequence) FROM run WHERE relay_ms IS NOT NULL), 0)",
        [],
    )?;
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
                defect.first_seen_ms = seen;
            }
        }
        Err(error) => report.add_ledger_defect(error.to_string()),
    }
}

/// Records one `fleet status` run after completing the assignment rows its commit depends on: a
/// failure of the completion is a defect of this run and is committed with it. `fleet tick` calls
/// this too, so a fleet that runs only ticks does not leave killed `fleet assign` rows `pending`.
pub fn record_status_completing(
    config: &FleetConfig,
    report: &mut FleetReport,
    bus: &mut impl crate::fleet::assign::AssignBus,
) {
    if let Err(error) = complete_pending(config, bus) {
        report.add_ledger_defect(format!("assignment completion: {error}"));
    }
    record_status(config, report);
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
