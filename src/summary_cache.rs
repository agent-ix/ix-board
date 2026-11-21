// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Independently coordinated summary coverage and successful source provenance.

use crate::cache::{RefreshLock, ordinary_ttl};
use crate::linear_summary::{SummarySnapshot, merge_summary};
use crate::{CacheFreshness, Clock, Error, LinearGraphQlSource, SystemClock, WorkspaceSource};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// A native summary read with source provenance and any failed-refresh reason.
#[derive(Debug, Serialize)]
pub struct SummaryRead {
    /// Complete active summary dataset.
    pub data: SummarySnapshot,
    /// Freshness and fallback metadata.
    pub freshness: CacheFreshness,
    /// Actual start of the latest successful source read.
    pub last_successful_sync: i64,
    /// Last authoritative full accessible-catalog scan.
    pub last_full_reconciliation: i64,
}

impl LinearGraphQlSource {
    /// Returns native summaries from their own cache partition without loading tickets.
    pub fn summary_snapshot_cached(&self) -> Result<SummaryRead, Error> {
        let path = self.cache_path();
        crate::cache::restrict_permissions(path)?;
        let ttl = ordinary_ttl()?;
        let mut connection = Connection::open(path).map_err(Error::Cache)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(Error::Cache)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS summary_snapshot (
            cache_key TEXT PRIMARY KEY, payload BLOB NOT NULL, synced_at INTEGER NOT NULL,
            full_at INTEGER NOT NULL)",
            )
            .map_err(Error::Cache)?;
        let key = format!("{}:summary:v1", self.cache_namespace());
        let load = |connection: &Connection| -> Result<Option<(SummarySnapshot, i64, i64)>, Error> {
            let row = connection
                .query_row(
                    "SELECT payload,synced_at,full_at FROM summary_snapshot WHERE cache_key=?1",
                    params![key],
                    |row| {
                        Ok((
                            row.get::<_, Vec<u8>>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(Error::Cache)?;
            row.map(|(bytes, sync, full)| {
                serde_json::from_slice(&bytes)
                    .map(|data| (data, sync, full))
                    .map_err(Error::CacheDecode)
            })
            .transpose()
        };
        let now = SystemClock.now_epoch_seconds();
        let baseline = load(&connection)?;
        if let Some((data, sync, full)) = &baseline
            && now >= *sync
            && now.saturating_sub(*sync) < ttl
        {
            return Ok(summary_read(data.clone(), *sync, *full, now, None));
        }
        let refresh = (|| {
            let _lock = RefreshLock::acquire(path, None)?;
            let current = load(&connection)?;
            let started = SystemClock.now_epoch_seconds();
            if let Some((data, sync, full)) = &current
                && started >= *sync
                && started.saturating_sub(*sync) < ttl
            {
                return Ok(summary_read(data.clone(), *sync, *full, started, None));
            }
            let full = current
                .as_ref()
                .is_none_or(|(_, _, full)| started.saturating_sub(*full) >= 86_400);
            let metrics = self.metrics_start();
            let fetched = match &current {
                Some((data, sync, _)) if !full => self
                    .fetch_summary_changes(*sync)
                    .map(|changes| merge_summary(data, changes)),
                _ => self
                    .fetch_summary()
                    .map(|data| merge_summary(&SummarySnapshot::default(), data)),
            };
            let mode = if full {
                "summary_full"
            } else {
                "summary_incremental"
            };
            if fetched.is_err() {
                self.record_metrics(metrics, mode, false)?;
            }
            let data = fetched?;
            let full_at = if full {
                started
            } else {
                current.as_ref().map_or(started, |(_, _, full)| *full)
            };
            let bytes = serde_json::to_vec(&data).map_err(Error::CacheEncode)?;
            let transaction = connection.transaction().map_err(Error::Cache)?;
            transaction.execute("INSERT INTO summary_snapshot(cache_key,payload,synced_at,full_at) VALUES(?1,?2,?3,?4)
                ON CONFLICT(cache_key) DO UPDATE SET payload=excluded.payload,synced_at=excluded.synced_at,full_at=excluded.full_at",
                params![key,bytes,started,full_at]).map_err(Error::Cache)?;
            transaction.commit().map_err(Error::Cache)?;
            self.record_metrics(metrics, mode, true)?;
            Ok(summary_read(data, started, full_at, started, None))
        })();
        match refresh {
            Ok(read) => Ok(read),
            Err(error) => match baseline {
                Some((data, sync, full)) => Ok(summary_read(data, sync, full, now, Some(error))),
                None => Err(error),
            },
        }
    }
}

fn summary_read(
    data: SummarySnapshot,
    sync: i64,
    full: i64,
    now: i64,
    error: Option<Error>,
) -> SummaryRead {
    let rate_limit_until = match &error {
        Some(Error::LinearRateLimited { retry_after }) => {
            now.checked_add(i64::try_from(retry_after.as_secs()).unwrap_or(i64::MAX))
        }
        _ => None,
    };
    SummaryRead {
        data,
        last_successful_sync: sync,
        last_full_reconciliation: full,
        freshness: CacheFreshness {
            stale: error.is_some(),
            age_seconds: now.checked_sub(sync).filter(|age| *age >= 0),
            rate_limit_until,
            failure_reason: error.map(|error| error.to_string()),
        },
    }
}
