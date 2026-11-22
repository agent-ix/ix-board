// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Local frontier monitoring never opens the quota gate or sends API requests.
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub count: u64,
    pub at_ms: i64,
}

pub(crate) fn read(path: &Path, now: i64) -> Result<Observation, rusqlite::Error> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(Duration::from_millis(100))?;
    let count = connection.query_row(
        "SELECT COUNT(*) FROM linear_quota_observations WHERE expires_at > ?1",
        [now],
        |r| r.get(0),
    )?;
    Ok(Observation { count, at_ms: now })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trace: FR-033-AC-11
    #[test]
    fn observing_a_frontier_above_512_does_not_change_the_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("quota.sqlite3");
        let database = Connection::open(&path).unwrap();
        database.execute_batch("CREATE TABLE linear_quota_observations(id INTEGER PRIMARY KEY, expires_at INTEGER NOT NULL); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<600) INSERT INTO linear_quota_observations SELECT i,600000 FROM n; INSERT INTO linear_quota_observations VALUES(999,500);").unwrap();
        drop(database);
        let before = std::fs::read(&path).unwrap();
        let observation = read(&path, 1000).unwrap();
        assert_eq!(observation.count, 600);
        assert_eq!(observation.at_ms, 1000);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let absent = directory.path().join("absent.sqlite3");
        assert!(read(&absent, 1000).is_err());
        assert!(!absent.exists());
    }
}
