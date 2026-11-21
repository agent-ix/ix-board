// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Credential-independent, process-shared admission for every Linear request.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use rusqlite::{Connection, TransactionBehavior, params};
use serde::Serialize;
use serde_json::Value;

use crate::{Deadline, Error};

/// Conservative charge when the transport cannot estimate a query's complexity.
pub const UNKNOWN_COMPLEXITY: u64 = 10_000;

/// Linear subscription used only when the server has supplied no actual limit.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanTier {
    /// Free subscription (the conservative default).
    #[default]
    Free,
    /// Basic subscription.
    Basic,
    /// Business subscription.
    Business,
    /// Enterprise subscription.
    Enterprise,
}

/// Authentication class used to select documented fallback capacities.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// Personal API key.
    #[default]
    ApiKey,
    /// OAuth application.
    OAuth,
}

/// Local allocation of Linear's shared allowance. No credential is stored here.
#[derive(Clone, Debug, Serialize)]
pub struct QuotaConfig {
    /// Subscription fallback multiplier.
    pub tier: PlanTier,
    /// Authentication fallback limits.
    pub auth_mode: AuthMode,
    /// Share of both capacities, strictly positive and at most one half.
    pub share: f64,
    /// Additional fraction held back from this tool's allocation.
    pub reserve_fraction: f64,
    /// Fallback accounting window in seconds.
    pub window_seconds: u64,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            tier: PlanTier::Free,
            auth_mode: AuthMode::ApiKey,
            share: 0.5,
            reserve_fraction: 0.1,
            window_seconds: 3600,
        }
    }
}

impl QuotaConfig {
    /// Reads explicit environment overrides and rejects malformed values.
    pub fn from_env() -> Result<Self, Error> {
        Self::from_config(None)
    }

    /// Resolves optional `[linear]` settings, then explicit environment overrides.
    pub fn from_config(document: Option<&toml::Value>) -> Result<Self, Error> {
        let mut config = Self::default();
        if let Some(linear) = document.and_then(|document| document.get("linear")) {
            let table = linear.as_table().ok_or(Error::InvalidQuotaConfig {
                reason: "linear configuration must be a table",
            })?;
            if let Some(value) = table
                .get("plan_tier")
                .filter(|_| std::env::var_os("IX_BOARD_LINEAR_PLAN_TIER").is_none())
            {
                config.tier = match value.as_str() {
                    Some("free") => PlanTier::Free,
                    Some("basic") => PlanTier::Basic,
                    Some("business") => PlanTier::Business,
                    Some("enterprise") => PlanTier::Enterprise,
                    _ => return invalid("unknown Linear plan tier"),
                };
            }
            if let Some(value) = table
                .get("auth_mode")
                .filter(|_| std::env::var_os("IX_BOARD_LINEAR_AUTH_MODE").is_none())
            {
                config.auth_mode = match value.as_str() {
                    Some("api_key") => AuthMode::ApiKey,
                    Some("oauth") => AuthMode::OAuth,
                    _ => return invalid("unknown Linear authentication mode"),
                };
            }
            for (name, env_name, target) in [
                (
                    "quota_share",
                    "IX_BOARD_LINEAR_QUOTA_SHARE",
                    &mut config.share,
                ),
                (
                    "quota_reserve",
                    "IX_BOARD_LINEAR_QUOTA_RESERVE",
                    &mut config.reserve_fraction,
                ),
            ] {
                if let Some(value) = table
                    .get(name)
                    .filter(|_| std::env::var_os(env_name).is_none())
                {
                    *target = value
                        .as_float()
                        .or_else(|| value.as_integer().map(|number| number as f64))
                        .ok_or(Error::InvalidQuotaConfig {
                            reason: "quota shares must be finite numbers",
                        })?;
                }
            }
            if let Some(value) = table
                .get("quota_window_seconds")
                .filter(|_| std::env::var_os("IX_BOARD_LINEAR_QUOTA_WINDOW_SECONDS").is_none())
            {
                config.window_seconds = value
                    .as_integer()
                    .and_then(|number| u64::try_from(number).ok())
                    .ok_or(Error::InvalidQuotaConfig {
                        reason: "quota window must be positive integer seconds",
                    })?;
            }
        }
        if let Some(value) = environment("IX_BOARD_LINEAR_PLAN_TIER")? {
            config.tier = match value.as_str() {
                "free" => PlanTier::Free,
                "basic" => PlanTier::Basic,
                "business" => PlanTier::Business,
                "enterprise" => PlanTier::Enterprise,
                _ => return invalid("unknown Linear plan tier"),
            };
        }
        if let Some(value) = environment("IX_BOARD_LINEAR_AUTH_MODE")? {
            config.auth_mode = match value.as_str() {
                "api_key" => AuthMode::ApiKey,
                "oauth" => AuthMode::OAuth,
                _ => return invalid("unknown Linear authentication mode"),
            };
        }
        for (name, target) in [
            ("IX_BOARD_LINEAR_QUOTA_SHARE", &mut config.share),
            (
                "IX_BOARD_LINEAR_QUOTA_RESERVE",
                &mut config.reserve_fraction,
            ),
        ] {
            if let Some(value) = environment(name)? {
                *target = value.parse().map_err(|_| Error::InvalidQuotaConfig {
                    reason: "quota shares must be finite numbers",
                })?;
            }
        }
        if let Some(value) = environment("IX_BOARD_LINEAR_QUOTA_WINDOW_SECONDS")? {
            config.window_seconds = value.parse().map_err(|_| Error::InvalidQuotaConfig {
                reason: "quota window must be positive integer seconds",
            })?;
        }
        config.validate()?;
        Ok(config)
    }

    /// Checks all caller-controlled settings before storage or network access.
    pub fn validate(&self) -> Result<(), Error> {
        if !self.share.is_finite() || self.share <= 0.0 || self.share > 0.5 {
            return invalid("quota share must be finite and satisfy 0 < share <= 0.5");
        }
        if !self.reserve_fraction.is_finite()
            || self.reserve_fraction < 0.0
            || self.reserve_fraction >= 1.0
        {
            return invalid("quota reserve must be finite and satisfy 0 <= reserve < 1");
        }
        if self.window_seconds == 0 || self.window_seconds > 31_536_000 {
            return invalid("quota window must be between 1 second and 1 year");
        }
        Ok(())
    }

    /// Documented hourly fallback request and complexity limits before allocation.
    pub fn fallback_limits(&self) -> (u64, u64) {
        let multiplier = match self.tier {
            PlanTier::Free | PlanTier::Basic => 1,
            PlanTier::Business => 2,
            PlanTier::Enterprise => 5,
        };
        let (requests, complexity) = match self.auth_mode {
            AuthMode::ApiKey => (2500, 3_000_000),
            AuthMode::OAuth => (5000, 2_000_000),
        };
        (requests * multiplier, complexity * multiplier)
    }

    fn allocate(&self, capacity: u64) -> u64 {
        // Capacities are bounded to exact f64 integers at the untrusted header boundary.
        (capacity as f64 * self.share * (1.0 - self.reserve_fraction)).floor() as u64
    }

    fn reserved_headroom(&self, capacity: u64) -> u64 {
        (capacity as f64 * self.share * self.reserve_fraction).ceil() as u64
    }
}

fn environment(name: &str) -> Result<Option<String>, Error> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => invalid("quota setting is not Unicode"),
    }
}

fn invalid<T>(reason: &'static str) -> Result<T, Error> {
    Err(Error::InvalidQuotaConfig { reason })
}

/// Millisecond Unix clock, injectable without changing production admission behavior.
pub trait QuotaClock: Send + Sync {
    /// Current Unix time in milliseconds.
    fn now_epoch_millis(&self) -> i64;
}

/// Production clock for persisted absolute retry and reset deadlines.
pub struct SystemQuotaClock;

impl QuotaClock for SystemQuotaClock {
    fn now_epoch_millis(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |now| i64::try_from(now.as_millis()).unwrap_or(i64::MAX))
    }
}

/// Persisted state visible without credential resolution or a Linear request.
#[derive(Clone, Debug, Serialize)]
pub struct QuotaStatus {
    /// Latest server cooldown, Unix milliseconds; absent before observation.
    pub rate_limit_until: Option<i64>,
    /// Current locally allocated request capacity.
    pub request_budget: u64,
    /// Current locally allocated complexity capacity.
    pub complexity_budget: u64,
    /// Attempts admitted during the current request window.
    pub requests_used: u64,
    /// Conservative complexity charged during the current complexity window.
    pub complexity_used: u64,
    /// Request accounting reset, Unix milliseconds.
    pub request_reset_at: i64,
    /// Complexity accounting reset, Unix milliseconds.
    pub complexity_reset_at: i64,
}

/// Shared quota singleton in the snapshot database, independent of snapshot versions.
pub struct QuotaGate {
    path: PathBuf,
    config: QuotaConfig,
    clock: Arc<dyn QuotaClock>,
}

impl QuotaGate {
    /// Opens shared quota storage. Credentials never participate in its identity.
    pub fn open(path: &Path, config: &QuotaConfig) -> Result<Self, Error> {
        Self::open_within(path, config, None)
    }

    /// Opens storage with SQLite waits bounded by the current read's deadline.
    pub fn open_within(
        path: &Path,
        config: &QuotaConfig,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<Self, Error> {
        Self::initialize(path, config, Arc::new(SystemQuotaClock), deadline)
    }

    /// Opens storage with an explicitly supplied clock for deterministic fixtures.
    pub fn open_with_clock(
        path: &Path,
        config: &QuotaConfig,
        clock: Arc<dyn QuotaClock>,
    ) -> Result<Self, Error> {
        Self::initialize(path, config, clock, None)
    }

    fn initialize(
        path: &Path,
        config: &QuotaConfig,
        clock: Arc<dyn QuotaClock>,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<Self, Error> {
        config.validate()?;
        let gate = Self {
            path: path.to_owned(),
            config: config.clone(),
            clock,
        };
        let connection = gate.connection(deadline)?;
        let initialized = connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS linear_shared_quota (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                cooldown INTEGER NOT NULL DEFAULT 0,
                requests INTEGER NOT NULL DEFAULT 0,
                complexity INTEGER NOT NULL DEFAULT 0,
                request_reset INTEGER NOT NULL DEFAULT 0,
                complexity_reset INTEGER NOT NULL DEFAULT 0,
                request_limit INTEGER,
                complexity_limit INTEGER,
                request_remaining INTEGER,
                complexity_remaining INTEGER,
                endpoint_until INTEGER NOT NULL DEFAULT 0,
                request_budget INTEGER,
                complexity_budget INTEGER,
                endpoint_limit INTEGER,
                endpoint_remaining INTEGER,
                endpoint_reset INTEGER NOT NULL DEFAULT 0,
                endpoint_used INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO linear_shared_quota(singleton) VALUES(1);",
            )
            .map_err(Error::Cache);
        if let Some(deadline) = deadline {
            deadline.remaining()?;
        }
        initialized?;
        // Quota storage evolves independently of snapshot cache versions. Older
        // quota tables retain their active cooldown and conservative counters.
        gate.transaction(deadline, |transaction| {
            let mut statement = transaction
                .prepare("PRAGMA table_info(linear_shared_quota)")
                .map_err(Error::Cache)?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(Error::Cache)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(Error::Cache)?;
            drop(statement);
            for (name, definition) in [
                ("endpoint_limit", "INTEGER"),
                ("endpoint_remaining", "INTEGER"),
                ("endpoint_reset", "INTEGER NOT NULL DEFAULT 0"),
                ("endpoint_used", "INTEGER NOT NULL DEFAULT 0"),
            ] {
                if !names.iter().any(|existing| existing == name) {
                    transaction
                        .execute_batch(&format!(
                            "ALTER TABLE linear_shared_quota ADD COLUMN {name} {definition}"
                        ))
                        .map_err(Error::Cache)?;
                }
            }
            Ok(())
        })?;
        Ok(gate)
    }

    fn connection(&self, deadline: Option<&Deadline<'_>>) -> Result<Connection, Error> {
        let timeout = match deadline {
            Some(deadline) => deadline.remaining()?.min(Duration::from_secs(5)),
            None => Duration::from_secs(5),
        };
        let connection = Connection::open(&self.path).map_err(Error::Cache)?;
        connection.busy_timeout(timeout).map_err(Error::Cache)?;
        Ok(connection)
    }

    fn transaction<T>(
        &self,
        deadline: Option<&Deadline<'_>>,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut connection = self.connection(deadline)?;
        let result = (|| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(Error::Cache)?;
            if let Some(deadline) = deadline {
                deadline.remaining()?;
            }
            let value = operation(&transaction)?;
            if let Some(deadline) = deadline {
                deadline.remaining()?;
            }
            transaction.commit().map_err(Error::Cache)?;
            Ok(value)
        })();
        if let Some(deadline) = deadline {
            deadline.remaining()?;
        }
        result
    }

    fn advance(&self, connection: &Connection, now: i64) -> Result<(), Error> {
        let window = i64::try_from(self.config.window_seconds)
            .unwrap_or(i64::MAX / 1000)
            .saturating_mul(1000);
        let reset = now.saturating_add(window);
        connection.execute(
            "UPDATE linear_shared_quota SET
                requests = CASE WHEN request_reset <= ?1 THEN 0 ELSE requests END,
                complexity = CASE WHEN complexity_reset <= ?1 THEN 0 ELSE complexity END,
                request_remaining = CASE WHEN request_reset <= ?1 THEN NULL ELSE request_remaining END,
                complexity_remaining = CASE WHEN complexity_reset <= ?1 THEN NULL ELSE complexity_remaining END,
                request_budget = CASE WHEN request_reset <= ?1 THEN NULL ELSE request_budget END,
                complexity_budget = CASE WHEN complexity_reset <= ?1 THEN NULL ELSE complexity_budget END,
                endpoint_used = CASE WHEN endpoint_reset <= ?1 THEN 0 ELSE endpoint_used END,
                endpoint_remaining = CASE WHEN endpoint_reset <= ?1 THEN NULL ELSE endpoint_remaining END,
                endpoint_reset = CASE WHEN endpoint_reset <= ?1 THEN ?2 ELSE endpoint_reset END,
                request_reset = CASE WHEN request_reset <= ?1 THEN ?2 ELSE request_reset END,
                complexity_reset = CASE WHEN complexity_reset <= ?1 THEN ?2 ELSE complexity_reset END
             WHERE singleton = 1",
            params![now, reset],
        ).map_err(Error::Cache)?;
        Ok(())
    }

    fn read(&self, connection: &Connection) -> Result<QuotaStatus, Error> {
        let (fallback_requests, fallback_complexity) = self.config.fallback_limits();
        connection
            .query_row(
                "SELECT cooldown, requests, complexity, request_reset, complexity_reset,
                    request_limit, complexity_limit, request_budget, complexity_budget
             FROM linear_shared_quota WHERE singleton = 1",
                [],
                |row| {
                    let cooldown: i64 = row.get(0)?;
                    let requests: Option<u64> = row.get(5)?;
                    let complexity: Option<u64> = row.get(6)?;
                    let request_budget =
                        self.config.allocate(requests.unwrap_or(fallback_requests));
                    let complexity_budget = self
                        .config
                        .allocate(complexity.unwrap_or(fallback_complexity));
                    Ok(QuotaStatus {
                        rate_limit_until: (cooldown > 0).then_some(cooldown),
                        requests_used: row.get(1)?,
                        complexity_used: row.get(2)?,
                        request_reset_at: row.get(3)?,
                        complexity_reset_at: row.get(4)?,
                        request_budget: row
                            .get::<_, Option<u64>>(7)?
                            .map_or(request_budget, |old| old.min(request_budget)),
                        complexity_budget: row
                            .get::<_, Option<u64>>(8)?
                            .map_or(complexity_budget, |old| old.min(complexity_budget)),
                    })
                },
            )
            .map_err(Error::Cache)
    }

    /// Atomically reserves one attempted request and a nonzero complexity charge.
    /// No SQLite transaction remains open while the caller performs network I/O.
    pub fn reserve(
        &self,
        estimated_cost: u64,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<(), Error> {
        let cost = if estimated_cost == 0 {
            UNKNOWN_COMPLEXITY
        } else {
            estimated_cost
        };
        if cost > UNKNOWN_COMPLEXITY {
            return Err(Error::LinearQueryComplexityExceeded {
                cost,
                limit: UNKNOWN_COMPLEXITY,
            });
        }
        self.transaction(deadline, |transaction| {
            let now = self.clock.now_epoch_millis();
            self.advance(transaction, now)?;
            let state = self.read(transaction)?;
            if let Some(until) = state.rate_limit_until.filter(|until| *until > now) {
                return Err(Error::LinearRateLimited { retry_after: ceil_delay(until, now) });
            }
            let (request_remaining, complexity_remaining, endpoint_until, request_limit, complexity_limit): (Option<u64>, Option<u64>, i64, Option<u64>, Option<u64>) = transaction.query_row(
                "SELECT request_remaining, complexity_remaining, endpoint_until, request_limit, complexity_limit FROM linear_shared_quota WHERE singleton = 1",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            ).map_err(Error::Cache)?;
            let (fallback_requests, fallback_complexity) = self.config.fallback_limits();
            let request_headroom = self.config.reserved_headroom(request_limit.unwrap_or(fallback_requests));
            let complexity_headroom = self.config.reserved_headroom(complexity_limit.unwrap_or(fallback_complexity));
            let mut exhausted_until = 0;
            if state.requests_used >= state.request_budget
                || request_remaining.is_some_and(|left| left < 1 + request_headroom)
            {
                exhausted_until = state.request_reset_at;
            }
            if state.complexity_used.saturating_add(cost) > state.complexity_budget
                || complexity_remaining.is_some_and(|left| left < cost.saturating_add(complexity_headroom))
            {
                exhausted_until = exhausted_until.max(state.complexity_reset_at);
            }
            exhausted_until = exhausted_until.max(endpoint_until);
            let (endpoint_limit, endpoint_remaining, endpoint_reset, endpoint_used): (Option<u64>, Option<u64>, i64, u64) = transaction.query_row(
                "SELECT endpoint_limit, endpoint_remaining, endpoint_reset, endpoint_used FROM linear_shared_quota WHERE singleton=1",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).map_err(Error::Cache)?;
            let endpoint_headroom = endpoint_limit.map_or(0, |limit| self.config.reserved_headroom(limit));
            if endpoint_limit.is_some_and(|limit| endpoint_used >= self.config.allocate(limit))
                || endpoint_remaining.is_some_and(|left| left < 1 + endpoint_headroom)
            {
                exhausted_until = exhausted_until.max(endpoint_reset);
            }
            if exhausted_until > now {
                return Err(Error::LinearLocalBudgetExhausted { retry_after: ceil_delay(exhausted_until, now) });
            }
            transaction.execute(
                "UPDATE linear_shared_quota SET requests=requests+1, complexity=complexity+?1,
                 request_remaining=MAX(0,request_remaining-1),
                 complexity_remaining=MAX(0,complexity_remaining-?1),
                 endpoint_remaining=MAX(0,endpoint_remaining-1),
                 endpoint_used=endpoint_used+1,
                 request_budget=?2, complexity_budget=?3 WHERE singleton=1",
                params![cost, state.request_budget, state.complexity_budget],
            ).map_err(Error::Cache)?;
            Ok(())
        })
    }

    /// Records actual capacities, observed cost and the latest applicable cooldown.
    /// `graphql` is the complete decoded response, including its open error metadata.
    pub fn observe(
        &self,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
        estimated_cost: u64,
    ) -> Result<(), Error> {
        self.observe_within(status, headers, graphql, estimated_cost, None)
    }

    /// Observes a response with SQLite waits under the same operation deadline.
    pub fn observe_within(
        &self,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
        estimated_cost: u64,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<(), Error> {
        self.transaction(deadline, |transaction| {
            let now = self.clock.now_epoch_millis();
            self.advance(transaction, now)?;
            for (kind, limit_column, remaining_column, reset_column) in [
                ("requests", "request_limit", "request_remaining", "request_reset"),
                ("complexity", "complexity_limit", "complexity_remaining", "complexity_reset"),
                ("endpoint-requests", "endpoint_limit", "endpoint_remaining", "endpoint_reset"),
            ] {
                let limit = header_integer(headers, &format!("x-ratelimit-{kind}-limit"));
                let remaining = header_integer(headers, &format!("x-ratelimit-{kind}-remaining"));
                let reset = header_integer(headers, &format!("x-ratelimit-{kind}-reset"))
                    .and_then(|value| i64::try_from(value).ok()).filter(|reset| *reset > now)
                    // Exhausted remaining state must expire with the same fallback
                    // cooldown, rather than silently extending it to a fallback hour.
                    .or_else(|| (remaining == Some(0)).then(|| now.saturating_add(60_000)));
                // Within a window, late concurrent responses must never restore consumed headroom.
                let sql = format!("UPDATE linear_shared_quota SET {limit_column}=COALESCE(?1,{limit_column}),
                    {remaining_column}=CASE WHEN ?2 IS NULL THEN {remaining_column}
                    WHEN {remaining_column} IS NULL THEN ?2 ELSE MIN({remaining_column},?2) END,
                    {reset_column}=COALESCE(?3,{reset_column}) WHERE singleton=1");
                transaction.execute(&sql, params![limit, remaining, reset]).map_err(Error::Cache)?;
            }
            let estimate = if estimated_cost == 0 { UNKNOWN_COMPLEXITY } else { estimated_cost };
            if let Some(actual) = header_integer(headers, "x-complexity") {
                let additional = actual.saturating_sub(estimate);
                transaction.execute("UPDATE linear_shared_quota SET complexity=complexity+?1 WHERE singleton=1", params![additional]).map_err(Error::Cache)?;
            }
            let mut exhausted_reset = 0;
            for kind in ["requests", "complexity", "endpoint-requests"] {
                if header_integer(headers, &format!("x-ratelimit-{kind}-remaining")) == Some(0) {
                    let reset = header_integer(headers, &format!("x-ratelimit-{kind}-reset"))
                        .and_then(|reset| i64::try_from(reset).ok()).filter(|reset| *reset > now)
                        .unwrap_or_else(|| now.saturating_add(60_000));
                    exhausted_reset = exhausted_reset.max(reset);
                }
            }
            if exhausted_reset > now {
                transaction.execute("UPDATE linear_shared_quota SET endpoint_until=MAX(endpoint_until,?1) WHERE singleton=1", [exhausted_reset]).map_err(Error::Cache)?;
            }
            let rate_limited = status == 429 || graphql.is_some_and(is_graphql_rate_limit);
            if rate_limited {
                let retry_after = headers.get("retry-after").and_then(|value| value.to_str().ok())
                    .and_then(|value| retry_after_until(value, now));
                let metadata = graphql.and_then(|value| metadata_retry_until(value, now));
                let until = retry_after.into_iter().chain(metadata).chain((exhausted_reset > now).then_some(exhausted_reset))
                    .max().unwrap_or_else(|| now.saturating_add(60_000));
                transaction.execute("UPDATE linear_shared_quota SET cooldown=MAX(cooldown,?1) WHERE singleton=1", [until]).map_err(Error::Cache)?;
            }
            Ok(())
        })
    }

    /// Returns accounting state without sending a request or reading credentials.
    pub fn status(&self) -> Result<QuotaStatus, Error> {
        self.status_within(None)
    }

    /// Reads accounting state while honoring a bounded operation's deadline.
    pub fn status_within(&self, deadline: Option<&Deadline<'_>>) -> Result<QuotaStatus, Error> {
        self.transaction(deadline, |transaction| {
            self.advance(transaction, self.clock.now_epoch_millis())?;
            self.read(transaction)
        })
    }
}

fn header_integer(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= 9_007_199_254_740_991)
}

fn ceil_delay(until: i64, now: i64) -> Duration {
    let milliseconds = until.saturating_sub(now).unsigned_abs();
    Duration::from_secs(milliseconds.div_ceil(1000))
}

fn retry_after_until(value: &str, now: i64) -> Option<i64> {
    if let Ok(seconds) = value.parse::<f64>() {
        if seconds.is_finite() && (0.0..=31_536_000.0).contains(&seconds) {
            return Some(now.saturating_add((seconds * 1000.0).ceil() as i64));
        }
        return None;
    }
    chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| date.timestamp_millis())
        .filter(|until| *until > now)
}

fn is_graphql_rate_limit(value: &Value) -> bool {
    value
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| {
            errors.iter().any(|error| {
                error.pointer("/extensions/code").and_then(Value::as_str) == Some("RATELIMITED")
            })
        })
}

fn metadata_retry_until(value: &Value, now: i64) -> Option<i64> {
    // Only inspect structured error extensions, never user-controlled issue data.
    let errors = value.get("errors")?.as_array()?;
    errors
        .iter()
        .filter_map(|error| error.get("extensions"))
        .filter_map(|extensions| metadata_object_until(extensions, now, 0))
        .max()
}

fn metadata_object_until(value: &Value, now: i64, depth: usize) -> Option<i64> {
    if depth > 8 {
        return None;
    }
    let object = value.as_object()?;
    object
        .iter()
        .filter_map(|(key, value)| match key.as_str() {
            "retryAfter" | "retry_after" => value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_f64().map(|number| number.to_string()))
                .and_then(|text| retry_after_until(&text, now)),
            "reset" | "resetAt" | "retryUntil" | "retry_until" => value
                .as_i64()
                .or_else(|| value.as_str()?.parse().ok())
                .map(|timestamp| {
                    if timestamp < 100_000_000_000 {
                        timestamp.saturating_mul(1000)
                    } else {
                        timestamp
                    }
                })
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
                        .ok()
                        .map(|date| date.timestamp_millis())
                })
                .filter(|until| *until > now),
            _ => metadata_object_until(value, now, depth + 1),
        })
        .max()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    use super::*;
    use crate::Clock;
    use ix_trace_rs::trace;

    struct ControlledClock {
        start: Instant,
        elapsed_millis: AtomicU64,
    }

    impl Clock for ControlledClock {
        fn now_epoch_seconds(&self) -> i64 {
            1_800_000_000
        }

        fn monotonic_now(&self) -> Instant {
            self.start + Duration::from_millis(self.elapsed_millis.load(Ordering::SeqCst))
        }
    }

    #[trace("FR-004-AC-14", "FR-031-AC-2")]
    #[test]
    fn expired_deadline_refuses_open_reserve_observe_and_status_before_sqlite() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("quota.sqlite");
        let gate = QuotaGate::open(&path, &QuotaConfig::default()).expect("open quota");
        let clock = ControlledClock {
            start: Instant::now(),
            elapsed_millis: AtomicU64::new(0),
        };
        let deadline = Deadline::start(&clock, Duration::from_millis(10));
        clock.elapsed_millis.store(10, Ordering::SeqCst);
        assert!(matches!(
            gate.reserve(10, Some(&deadline)),
            Err(Error::ReadTimeout { .. })
        ));
        assert!(matches!(
            gate.observe_within(429, &HeaderMap::new(), None, 10, Some(&deadline)),
            Err(Error::ReadTimeout { .. })
        ));
        assert!(matches!(
            gate.status_within(Some(&deadline)),
            Err(Error::ReadTimeout { .. })
        ));
        assert!(matches!(
            QuotaGate::open_within(&path, &QuotaConfig::default(), Some(&deadline)),
            Err(Error::ReadTimeout { .. })
        ));
        let state = gate.status().expect("unbounded status");
        assert_eq!(state.requests_used, 0);
        assert_eq!(state.rate_limit_until, None);
    }
}
