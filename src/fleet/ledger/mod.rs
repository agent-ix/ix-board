// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The fleet ledger (FR-039): a schema-versioned SQLite store outside git, written under a
//! single-writer lock with the atomic-commit discipline of the team-metrics journal.
//!
//! A writer holds an exclusive lock on `<ledger>.lock`, assigns a monotonic run sequence and
//! commits each run in one transaction, so a crash leaves the previous state. Readers open the
//! file read-only and never migrate.

mod assign_rows;
pub mod capacity;
mod cycle;
mod lock;
mod role_metrics;
mod schema;
#[cfg(test)]
mod tests;
mod tick;

use self::lock::WriterLock;
use super::FleetConfig;
use super::classify::ClassKind;
use super::report::{Defect, FleetReport};
use ix_trace_rs::implements;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error as ThisError;

pub use assign_rows::{AssignSink, complete_pending};
pub use capacity::{
    CapacityEvent, CapacityHost, CapacityLevel, CapacityRun, CapacityStatus, CapacityStep,
};
pub use cycle::ActivityCoverage;
pub use schema::{QUERYABLE, VERSION};
pub use tick::{TickCommit, tick_metrics};

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
    /// A capacity-ledger operation was refused by its state or input contract.
    #[error("fleet capacity: {0}")]
    Capacity(String),
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

/// Resolves the ledger path: the configured path (which `IX_BOARD_LEDGER_PATH` fills at
/// configuration load, see [`FleetConfig::from_toml`]), then the default.
///
/// # Errors
///
/// [`LedgerError::NoStateDir`] when there is no default.
pub fn resolve_path(config: &FleetConfig) -> Result<PathBuf, LedgerError> {
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
    for (index, statement) in schema::V2.iter().enumerate() {
        if fail_at == Some(index) {
            return Err(LedgerError::Sqlite(rusqlite::Error::ExecuteReturnedResults));
        }
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 2)?;
    Ok(())
}

#[implements("FR-039-AC-18")]
fn apply_v3(tx: &rusqlite::Transaction<'_>, fail_at: Option<usize>) -> Result<(), LedgerError> {
    for (index, statement) in schema::V3.iter().enumerate() {
        if fail_at == Some(index) {
            return Err(LedgerError::Sqlite(rusqlite::Error::ExecuteReturnedResults));
        }
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 3)?;
    Ok(())
}

fn apply_v4(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V4 {
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 4)?;
    Ok(())
}

#[implements("FR-039-AC-19")]
fn apply_v5(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V5 {
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 5)?;
    Ok(())
}

#[implements("FR-039-AC-19")]
fn apply_v6(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V6 {
        let parts = statement.split_whitespace().collect::<Vec<_>>();
        if parts.len() >= 6
            && parts[0] == "ALTER"
            && parts[1] == "TABLE"
            && parts[3] == "ADD"
            && parts[4] == "COLUMN"
        {
            let exists: i64 = tx.query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
                rusqlite::params![parts[2], parts[5]],
                |row| row.get(0),
            )?;
            if exists != 0 {
                continue;
            }
        }
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 6)?;
    Ok(())
}

#[implements("FR-039-AC-15")]
fn apply_v7(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V7 {
        tx.execute_batch(statement)?;
    }
    let cycle_missed_has_at_ms = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('cycle_missed') WHERE name='at_ms')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !cycle_missed_has_at_ms {
        tx.execute_batch(
            "ALTER TABLE cycle_missed ADD COLUMN at_ms INTEGER NOT NULL DEFAULT 0;
             UPDATE cycle_missed SET at_ms=detected_at;",
        )?;
    }
    for name in schema::V7_DROP_VIEWS {
        tx.execute_batch(&format!("DROP VIEW IF EXISTS {name}"))?;
    }
    for name in schema::QUERYABLE {
        if *name == "cycle_missed" {
            continue;
        }
        tx.execute_batch(&format!("DROP VIEW IF EXISTS {name}"))?;
    }
    for statement in schema::CAPACITY_VIEWS {
        let view = statement
            .split_whitespace()
            .nth(2)
            .ok_or_else(|| LedgerError::Capacity("invalid capacity view definition".to_owned()))?;
        tx.execute_batch(&format!("DROP VIEW IF EXISTS {view}"))?;
    }
    for statement in schema::views() {
        tx.execute_batch(&statement)?;
    }
    for statement in schema::CAPACITY_VIEWS {
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 7)?;
    Ok(())
}

#[implements("FR-039-AC-14")]
fn apply_v8(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V8 {
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 8)?;
    Ok(())
}

#[implements("FR-039-AC-14")]
fn apply_v9(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V9 {
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", 9)?;
    Ok(())
}

fn apply_v10(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    for statement in schema::V10 {
        tx.execute_batch(statement)?;
    }
    tx.pragma_update(None, "user_version", schema::VERSION)?;
    Ok(())
}

#[implements("FR-041-AC-11")]
fn refresh_v6_host_output_view(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS capacity_overload_state(
           host TEXT NOT NULL, cause TEXT NOT NULL, active INTEGER NOT NULL,
           updated_at INTEGER NOT NULL, source_basis TEXT NOT NULL,
           PRIMARY KEY(host,cause))",
    )?;
    let drain_cap_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('capacity_event') WHERE name='drain_cap_ms'",
        [],
        |row| row.get(0),
    )?;
    if drain_cap_column == 0 {
        tx.execute_batch(
            "ALTER TABLE capacity_event ADD COLUMN drain_cap_ms INTEGER NOT NULL DEFAULT 14400000",
        )?;
    }
    let ceiling_lanes_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('capacity_event') WHERE name='ceiling_lanes'",
        [],
        |row| row.get(0),
    )?;
    if ceiling_lanes_column == 0 {
        tx.execute_batch("ALTER TABLE capacity_event ADD COLUMN ceiling_lanes INTEGER")?;
    }
    let sample_interval_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('host_output_sample')
         WHERE name='sample_interval_ms'",
        [],
        |row| row.get(0),
    )?;
    if sample_interval_column == 0 {
        tx.execute_batch(
            "ALTER TABLE host_output_sample ADD COLUMN sample_interval_ms INTEGER NOT NULL DEFAULT 3600000",
        )?;
    }
    for (column, statement) in [
        (
            "ready_buffer",
            "ALTER TABLE saturation_sample ADD COLUMN ready_buffer INTEGER",
        ),
        (
            "active_lanes",
            "ALTER TABLE saturation_sample ADD COLUMN active_lanes INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "supplied_lanes",
            "ALTER TABLE saturation_sample ADD COLUMN supplied_lanes INTEGER",
        ),
        (
            "unsupplied_lanes",
            "ALTER TABLE saturation_sample ADD COLUMN unsupplied_lanes INTEGER",
        ),
        (
            "ready_share",
            "ALTER TABLE saturation_sample ADD COLUMN ready_share INTEGER",
        ),
        (
            "registry_current",
            "ALTER TABLE saturation_sample ADD COLUMN registry_current INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "queued_behind_first",
            "ALTER TABLE saturation_sample ADD COLUMN queued_behind_first INTEGER",
        ),
        (
            "bootstrap_ticket_pool",
            "ALTER TABLE saturation_sample ADD COLUMN bootstrap_ticket_pool INTEGER",
        ),
    ] {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('saturation_sample') WHERE name=?1",
            [column],
            |row| row.get(0),
        )?;
        if exists == 0 {
            tx.execute_batch(statement)?;
        }
    }
    for (column, statement) in [
        (
            "source_basis",
            "ALTER TABLE saturation_sample ADD COLUMN source_basis TEXT NOT NULL DEFAULT 'unknown'",
        ),
        (
            "unavailable_reason",
            "ALTER TABLE saturation_sample ADD COLUMN unavailable_reason TEXT",
        ),
        (
            "coverage",
            "ALTER TABLE saturation_sample ADD COLUMN coverage REAL",
        ),
    ] {
        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('saturation_sample') WHERE name=?1",
            [column],
            |row| row.get(0),
        )?;
        if exists == 0 {
            tx.execute_batch(statement)?;
        }
    }
    let output_window_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('capacity_step_lane_output') WHERE name='window_end'",
        [],
        |row| row.get(0),
    )?;
    if output_window_column == 0 {
        tx.execute_batch("ALTER TABLE capacity_step_lane_output ADD COLUMN window_end INTEGER")?;
    }
    let window_min_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('capacity_step') WHERE name='window_min_ms'",
        [],
        |row| row.get(0),
    )?;
    if window_min_column == 0 {
        tx.execute_batch(
            "ALTER TABLE capacity_step ADD COLUMN window_min_ms INTEGER NOT NULL DEFAULT 3600000",
        )?;
    }
    tx.execute_batch(
        "UPDATE capacity_step_lane_output SET window_end=(
           SELECT s.window_end FROM capacity_step s WHERE s.run_id=capacity_step_lane_output.run_id
             AND s.step_no=capacity_step_lane_output.step_no LIMIT 1)
         WHERE window_end IS NULL AND (SELECT COUNT(*) FROM capacity_step s
           WHERE s.run_id=capacity_step_lane_output.run_id
             AND s.step_no=capacity_step_lane_output.step_no)=1",
    )?;
    refresh_v6_demand_views(tx)?;
    Ok(())
}

fn refresh_v6_demand_views(tx: &rusqlite::Transaction<'_>) -> Result<(), LedgerError> {
    let definition: Option<String> = tx
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='capacity_step'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if definition
        .as_deref()
        .is_some_and(|sql| sql.contains("'deficit_nonzero'"))
    {
        for statement in schema::CAPACITY_VIEWS {
            if let Some(name) = statement.split_whitespace().nth(2) {
                tx.execute_batch(&format!("DROP VIEW IF EXISTS {name}"))?;
            }
        }
        tx.execute_batch(
            "DROP VIEW IF EXISTS unrecorded_hosts_at_zero_deficit;
             ALTER TABLE capacity_step RENAME TO capacity_step_before_demand_coverage;
             CREATE TABLE capacity_step(
               run_id INTEGER NOT NULL, step_no INTEGER NOT NULL, repeat_of INTEGER,
               host TEXT NOT NULL, lanes INTEGER, window_start INTEGER, window_end INTEGER,
               rampup_end INTEGER, valid INTEGER,
               inconclusive_reason TEXT CHECK(inconclusive_reason IS NULL OR
                 inconclusive_reason IN ('demand_short','deficit_coverage')),
               deficit_coverage REAL, deficit_zero_throughout INTEGER, host_output_per_hour REAL,
               tickets_closed INTEGER, prs_merged INTEGER, commits_pushed INTEGER,
               output_basis TEXT CHECK(output_basis IS NULL OR output_basis IN ('tickets_prs','commits')),
               queue_wait_p90_s REAL, throttled_share REAL, memory_headroom_min REAL,
               thermal_headroom_min REAL, slots_busy_equals_sustainable INTEGER,
               knee_signal INTEGER, knee_reason TEXT CHECK(knee_reason IS NULL OR
                 knee_reason IN ('output_gain','throttle')),
               source_basis TEXT NOT NULL DEFAULT '{}', build_slots_busy INTEGER,
               build_slots_sustainable INTEGER, measurement_values TEXT NOT NULL DEFAULT '{}',
               window_min_ms INTEGER NOT NULL DEFAULT 3600000);
             INSERT INTO capacity_step(
               run_id,step_no,repeat_of,host,lanes,window_start,window_end,rampup_end,valid,
               inconclusive_reason,deficit_coverage,deficit_zero_throughout,host_output_per_hour,
               tickets_closed,prs_merged,commits_pushed,output_basis,queue_wait_p90_s,
               throttled_share,memory_headroom_min,thermal_headroom_min,
               slots_busy_equals_sustainable,knee_signal,knee_reason,source_basis,
               build_slots_busy,build_slots_sustainable,measurement_values,window_min_ms)
             SELECT run_id,step_no,repeat_of,host,lanes,window_start,window_end,rampup_end,valid,
               CASE inconclusive_reason WHEN 'deficit_nonzero' THEN 'demand_short'
                 ELSE inconclusive_reason END,
               deficit_coverage,deficit_zero_throughout,host_output_per_hour,tickets_closed,
               prs_merged,commits_pushed,output_basis,queue_wait_p90_s,throttled_share,
               memory_headroom_min,thermal_headroom_min,slots_busy_equals_sustainable,
               knee_signal,knee_reason,source_basis,build_slots_busy,build_slots_sustainable,
               measurement_values,window_min_ms FROM capacity_step_before_demand_coverage;
             DROP TABLE capacity_step_before_demand_coverage;",
        )?;
    } else {
        tx.execute_batch("DROP VIEW IF EXISTS unrecorded_hosts_at_zero_deficit")?;
        for statement in schema::CAPACITY_VIEWS {
            if let Some(name) = statement.split_whitespace().nth(2) {
                tx.execute_batch(&format!("DROP VIEW IF EXISTS {name}"))?;
            }
        }
    }
    for statement in schema::CAPACITY_VIEWS {
        tx.execute_batch(statement)?;
    }
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

/// The most recent cadence invocation recorded for a manifest job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CadenceRun {
    /// Unix time when the invocation started, in milliseconds.
    pub started_at_ms: i64,
    /// Unix time when the invocation completed, in milliseconds.
    pub finished_at_ms: Option<i64>,
    /// `running`, `succeeded`, or `failed`.
    pub outcome: String,
    /// Process exit status or failure detail, when available.
    pub detail: Option<String>,
}

impl Ledger {
    /// Opens or creates the ledger as the writer, holding the lock until it is dropped. Older
    /// supported versions migrate to the current schema in one transaction; a newer or foreign
    /// file is refused unchanged.
    ///
    /// # Errors
    ///
    /// A path inside git, a lock held past the bound, an unsupported schema or a database
    /// failure.
    #[implements("FR-039-AC-19")]
    pub fn open(path: &Path, config: &FleetConfig) -> Result<Self, LedgerError> {
        Self::open_with(path, config, None)
    }

    /// Returns the latest cadence invocation for a program/team/job tuple.
    #[implements("FR-048-AC-2")]
    pub fn cadence_last_run(
        &self,
        program: &str,
        team: &str,
        job_id: &str,
    ) -> Result<Option<CadenceRun>, LedgerError> {
        Ok(self
            .connection
            .query_row(
                "SELECT started_at_ms, finished_at_ms, outcome, detail
                 FROM cadence_job_run WHERE program=?1 AND team=?2 AND job_id=?3",
                params![program, team, job_id],
                |row| {
                    Ok(CadenceRun {
                        started_at_ms: row.get(0)?,
                        finished_at_ms: row.get(1)?,
                        outcome: row.get(2)?,
                        detail: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Starts a cadence invocation, replacing the job's latest run row.
    #[implements("FR-048-AC-2")]
    pub fn cadence_start(
        &mut self,
        program: &str,
        team: &str,
        job_id: &str,
        started_at_ms: i64,
    ) -> Result<(), LedgerError> {
        self.connection.execute(
            "INSERT INTO cadence_job_run(
               program,team,job_id,started_at_ms,finished_at_ms,outcome,detail
             ) VALUES (?1,?2,?3,?4,NULL,'running',NULL)
             ON CONFLICT(program,team,job_id) DO UPDATE SET
               started_at_ms=excluded.started_at_ms,finished_at_ms=NULL,
               outcome='running',detail=NULL",
            params![program, team, job_id, started_at_ms],
        )?;
        Ok(())
    }

    /// Finishes the active cadence invocation with its process outcome.
    #[implements("FR-048-AC-2")]
    pub fn cadence_finish(
        &mut self,
        program: &str,
        team: &str,
        job_id: &str,
        finished_at_ms: i64,
        succeeded: bool,
        detail: Option<&str>,
    ) -> Result<(), LedgerError> {
        let outcome = if succeeded { "succeeded" } else { "failed" };
        let changed = self.connection.execute(
            "UPDATE cadence_job_run SET finished_at_ms=?4,outcome=?5,detail=?6
             WHERE program=?1 AND team=?2 AND job_id=?3",
            params![program, team, job_id, finished_at_ms, outcome, detail],
        )?;
        if changed == 0 {
            return Err(LedgerError::Capacity("cadence run was not started".to_owned()));
        }
        Ok(())
    }

    fn open_with(
        path: &Path,
        config: &FleetConfig,
        fail_migration_at: Option<(usize, usize)>,
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
                apply_v2(&tx, fail_migration_at.map(|(v2, _)| v2))?;
                apply_v3(&tx, fail_migration_at.map(|(_, v3)| v3))?;
                apply_v4(&tx)?;
                apply_v5(&tx)?;
                apply_v6(&tx)?;
                apply_v7(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            1 => {
                let tx = connection.transaction()?;
                apply_v2(&tx, fail_migration_at.map(|(v2, _)| v2))?;
                apply_v3(&tx, fail_migration_at.map(|(_, v3)| v3))?;
                apply_v4(&tx)?;
                apply_v5(&tx)?;
                apply_v6(&tx)?;
                apply_v7(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            2 => {
                let tx = connection.transaction()?;
                apply_v3(&tx, fail_migration_at.map(|(_, v3)| v3))?;
                apply_v4(&tx)?;
                apply_v5(&tx)?;
                apply_v6(&tx)?;
                apply_v7(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            3 => {
                let tx = connection.transaction()?;
                apply_v4(&tx)?;
                apply_v5(&tx)?;
                apply_v6(&tx)?;
                apply_v7(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            4 => {
                let tx = connection.transaction()?;
                apply_v5(&tx)?;
                apply_v6(&tx)?;
                apply_v7(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            5 => {
                let tx = connection.transaction()?;
                apply_v6(&tx)?;
                apply_v7(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            6 => {
                let tx = connection.transaction()?;
                apply_v7(&tx)?;
                refresh_v6_host_output_view(&tx)?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            7 => {
                let tx = connection.transaction()?;
                apply_v8(&tx)?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            8 => {
                let tx = connection.transaction()?;
                apply_v9(&tx)?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            9 => {
                let tx = connection.transaction()?;
                apply_v10(&tx)?;
                tx.commit()?;
            }
            10 => {}
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

    /// Records an open probe defect's ticket route at relay time, once.
    #[implements("FR-039-AC-20")]
    pub fn record_defect_route(
        &mut self,
        defect_id: i64,
        ticket: &str,
        routed_at_ms: i64,
    ) -> Result<(), LedgerError> {
        let changed = self.connection.execute(
            "UPDATE defect SET routed_ticket=?1, routed_at=?2
             WHERE defect_id=?3 AND source LIKE 'probe:%' AND routed_ticket IS NULL
               AND closed_at IS NULL AND first_seen_at IS NOT NULL AND ?2 >= first_seen_at",
            params![ticket, routed_at_ms, defect_id],
        )?;
        if changed != 1 {
            return Err(LedgerError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
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
        let first_seen =
            reconcile_defects(&tx, &report.defects, run_ms, unavailable, Reader::Status)?;
        record_failed_probe_ticks(&tx, &report.defects, None, run_ms, unavailable)?;
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

/// Which run reconciles defects. A defect belongs to the run that reads its source: `fleet status`
/// does not read the sources `fleet tick` reads itself (source prefix `tick.`), so it never closes
/// them; a tick reads everything a status run reads and its own sources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Reader {
    Status,
    Tick,
}

impl Reader {
    fn read(self, source: &str) -> bool {
        self == Self::Tick || !source.starts_with("tick.")
    }
}

/// Opens a row for each defect not already open and returns the first-seen time of every
/// defect. When the relay was readable it also closes the open rows no longer present whose source
/// this run reads; a run without the relay observed nothing about the other sources, so it closes
/// nothing.
fn reconcile_defects(
    tx: &rusqlite::Transaction<'_>,
    defects: &[Defect],
    run_ms: i64,
    unavailable: bool,
    reader: Reader,
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
            if !present.contains_key(key) && reader.read(&key.0) {
                tx.execute(
                    "UPDATE defect SET closed_at=?1 WHERE defect_id=?2",
                    params![run_ms, id],
                )?;
            }
        }
    }
    Ok(first_seen)
}

#[implements("FR-039-AC-20")]
fn record_failed_probe_ticks(
    tx: &rusqlite::Transaction<'_>,
    defects: &[Defect],
    tick_id: Option<i64>,
    at_ms: i64,
    unavailable: bool,
) -> Result<(), LedgerError> {
    if unavailable {
        return Ok(());
    }
    for defect in defects
        .iter()
        .filter(|defect| defect.source.starts_with("probe:"))
    {
        let key = defect_key(defect);
        let defect_id: Option<i64> = tx
            .query_row(
                "SELECT defect_id FROM defect WHERE source=?1 AND host IS ?2 AND lane IS ?3
                   AND kind=?4 AND closed_at IS NULL ORDER BY defect_id DESC LIMIT 1",
                params![key.0, key.1, key.2, key.3],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(defect_id) = defect_id {
            tx.execute(
                "INSERT OR IGNORE INTO failed_probe_tick(defect_id,source,host,tick_id,at_ms)
                 VALUES(?1,?2,?3,?4,?5)",
                params![defect_id, defect.source, defect.host, tick_id, at_ms],
            )?;
        }
    }
    Ok(())
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
    settings: &crate::fleet::tick::TickSettings,
    report: &mut FleetReport,
    bus: &mut impl crate::fleet::assign::AssignBus,
) {
    complete_assignments(config, settings, report, bus);
    record_status(config, report);
}

/// Completes the pending assignment rows ahead of a ledger commit. A failure is a defect of the
/// run about to commit, never silent. `fleet status` and `fleet tick` share this step.
pub fn complete_assignments(
    config: &FleetConfig,
    settings: &crate::fleet::tick::TickSettings,
    report: &mut FleetReport,
    bus: &mut impl crate::fleet::assign::AssignBus,
) {
    if let Err(error) = complete_pending(config, settings, bus) {
        report.add_unreadable_source(
            "relay:order_history",
            format!("assignment completion: {error}"),
        );
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
#[implements("FR-041-AC-11")]
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
    if found > schema::VERSION {
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
                    let text = String::from_utf8_lossy(t).into_owned();
                    if name == "source_basis" {
                        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
                    } else {
                        serde_json::Value::String(text)
                    }
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
