// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Offline inspection of dataset provenance and the shared Linear allocation.

use crate::quota::{QuotaConfig, QuotaGate, QuotaStatus};
use crate::{Clock, Error, SystemClock};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

/// Inspectable metadata for one independently refreshed dataset partition.
#[derive(Debug, Serialize)]
pub struct DatasetStatus {
    /// Opaque compatibility identity, containing no credential.
    pub partition: String,
    /// Summary or detailed ticket coverage.
    pub coverage: &'static str,
    /// Actual successful source-fetch start, unknown for legacy rows.
    pub last_successful_sync: Option<i64>,
    /// Last accessible-catalog reconciliation start.
    pub last_full_reconciliation: Option<i64>,
    /// Seconds since the successful source-fetch start.
    pub age_seconds: Option<i64>,
    /// Unknown provenance is never certified fresh.
    pub stale: bool,
}

/// Cache and quota facts read without credentials or network requests.
#[derive(Debug, Serialize)]
pub struct CacheStatus {
    /// Location shared by transport and snapshot coordination.
    pub cache_path: String,
    /// Independent compatible datasets stored in this cache.
    pub datasets: Vec<DatasetStatus>,
    /// Machine/cache-shared allocation and cooldown.
    pub quota: QuotaStatus,
    /// Latest refresh counters, null until a refresh has been recorded.
    pub latest_refresh: Option<RefreshCounters>,
}

/// Costs and completion of a source refresh, including failed partial work.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshCounters {
    /// Full reconciliation or incremental merge.
    pub mode: String,
    /// Whether the complete result was published.
    pub success: bool,
    /// Requests admitted before send during this refresh.
    pub requests_attempted: u64,
    /// Actual reported complexity or conservative estimate when unavailable.
    pub complexity: u64,
    /// Elapsed monotonic time including failed requests.
    pub duration_millis: u64,
}

/// Reads status locally; does not resolve a Linear authorization identity.
pub fn read_status(path: &Path) -> Result<CacheStatus, Error> {
    let quota = QuotaGate::open(
        path,
        &QuotaConfig::from_config(crate::settings::load()?.as_ref())?,
    )?
    .status()?;
    let connection = Connection::open(path).map_err(Error::Cache)?;
    let now = SystemClock.now_epoch_seconds();
    let ttl = crate::cache::ordinary_ttl()?;
    let mut datasets = Vec::new();
    for (table, coverage, sql) in [
        (
            "workspace_snapshot",
            "detail",
            "SELECT cache_key,read_started_at,full_reconciled_at FROM workspace_snapshot",
        ),
        (
            "summary_snapshot",
            "summary",
            "SELECT cache_key,synced_at,full_at FROM summary_snapshot",
        ),
    ] {
        if !table_exists(&connection, table)? {
            continue;
        }
        let columns = {
            let mut statement = connection
                .prepare(&format!("PRAGMA table_info({table})"))
                .map_err(Error::Cache)?;
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(Error::Cache)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(Error::Cache)?
        };
        let sql = if table == "workspace_snapshot"
            && !columns.iter().any(|column| column == "full_reconciled_at")
        {
            if columns.iter().any(|column| column == "read_started_at") {
                "SELECT cache_key,read_started_at,NULL FROM workspace_snapshot"
            } else {
                "SELECT cache_key,NULL,NULL FROM workspace_snapshot"
            }
        } else {
            sql
        };
        let mut statement = connection.prepare(sql).map_err(Error::Cache)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })
            .map_err(Error::Cache)?;
        for row in rows {
            let (partition, sync, full) = row.map_err(Error::Cache)?;
            let age_seconds = sync
                .and_then(|sync| now.checked_sub(sync))
                .filter(|age| *age >= 0);
            datasets.push(DatasetStatus {
                partition,
                coverage,
                last_successful_sync: sync,
                last_full_reconciliation: full,
                age_seconds,
                stale: age_seconds.is_none_or(|age| age >= ttl),
            });
        }
    }
    let latest_refresh = if table_exists(&connection, "refresh_counters")? {
        let bytes: Option<Vec<u8>> = connection
            .query_row(
                "SELECT payload FROM refresh_counters WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::Cache)?;
        bytes
            .map(|bytes| serde_json::from_slice(&bytes).map_err(Error::CacheDecode))
            .transpose()?
    } else {
        None
    };
    Ok(CacheStatus {
        cache_path: path.display().to_string(),
        datasets,
        quota,
        latest_refresh,
    })
}

pub(crate) fn record_refresh(path: &Path, counters: &RefreshCounters) -> Result<(), Error> {
    let connection = Connection::open(path).map_err(Error::Cache)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(Error::Cache)?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS refresh_counters(singleton INTEGER PRIMARY KEY CHECK(singleton=1),payload BLOB NOT NULL)").map_err(Error::Cache)?;
    connection.execute("INSERT INTO refresh_counters(singleton,payload) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET payload=excluded.payload",
        [serde_json::to_vec(counters).map_err(Error::CacheEncode)?]).map_err(Error::Cache)?;
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, Error> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .map_err(Error::Cache)
}

/// Human-readable equivalent of the offline JSON status.
pub fn render_text(status: &CacheStatus) -> String {
    let mut lines = vec![
        format!("cache_path: {}", status.cache_path),
        format!(
            "rate_limit_until: {}",
            status
                .quota
                .rate_limit_until
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ),
        format!(
            "local budget: requests {}/{}, complexity {}/{}",
            status.quota.requests_used,
            status.quota.request_budget,
            status.quota.complexity_used,
            status.quota.complexity_budget
        ),
        format!(
            "reset: requests {}, complexity {}",
            status.quota.request_reset_at, status.quota.complexity_reset_at
        ),
    ];
    for dataset in &status.datasets {
        lines.push(format!("coverage: {} | stale: {} | age_seconds: {} | last_successful_sync: {:?} | last_full_reconciliation: {:?}",dataset.coverage,dataset.stale,dataset.age_seconds.map_or_else(|| "unknown".to_owned(),|age| age.to_string()),dataset.last_successful_sync,dataset.last_full_reconciliation));
    }
    lines.push(format!("latest_refresh: {:?}", status.latest_refresh));
    lines.join("\n")
}
