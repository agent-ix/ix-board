// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Credential-independent, process-shared admission for every Linear request.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

pub use crate::quota_decision::{QuotaBlocker, QuotaDecision, QuotaDimension, QuotaState};
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
    /// Fixed local accounting generation, independent of rolling server reset headers.
    pub local_generation: i64,
    /// Fixed local accounting window deadline, Unix milliseconds.
    pub local_window_reset_at: i64,
}

/// One durable, single-use attempted-request charge. Dropping it keeps the estimate.
/// Only the gate can construct this token; completion consumes it by value.
#[derive(Debug)]
pub struct Reservation {
    id: i64,
    generation: i64,
    estimated_cost: u64,
    database_identity: String,
    server_complexity_generation: i64,
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
                ("database_identity", "TEXT"),
                ("local_generation", "INTEGER NOT NULL DEFAULT 0"),
                ("local_reset", "INTEGER"),
                ("server_complexity_generation", "INTEGER NOT NULL DEFAULT 0"),
                ("server_complexity_debt", "INTEGER NOT NULL DEFAULT 0"),
                ("paired_evidence", "INTEGER NOT NULL DEFAULT 0"),
                ("last_clock", "INTEGER NOT NULL DEFAULT 0"),
                ("evidence_error_until", "INTEGER NOT NULL DEFAULT 0"),
            ] {
                if !names.iter().any(|existing| existing == name) {
                    transaction
                        .execute_batch(&format!(
                            "ALTER TABLE linear_shared_quota ADD COLUMN {name} {definition}"
                        ))
                        .map_err(Error::Cache)?;
                }
            }
            let window = i64::try_from(gate.config.window_seconds)
                .unwrap_or(i64::MAX / 1000)
                .saturating_mul(1000);
            let reset = gate.clock.now_epoch_millis().saturating_add(window);
            transaction
                .execute(
                    "UPDATE linear_shared_quota SET
                database_identity=COALESCE(database_identity,lower(hex(randomblob(16)))),
                local_reset=COALESCE(local_reset,CASE WHEN MAX(request_reset,complexity_reset)>0
                    THEN MAX(request_reset,complexity_reset) ELSE ?1 END) WHERE singleton=1",
                    [reset],
                )
                .map_err(Error::Cache)?;
            transaction
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS linear_quota_reservations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                generation INTEGER NOT NULL,
                estimate INTEGER NOT NULL,
                server_complexity_generation INTEGER NOT NULL
            );",
                )
                .map_err(Error::Cache)?;
            transaction.execute_batch("CREATE TABLE IF NOT EXISTS linear_quota_observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT, dimension TEXT NOT NULL,
                remaining INTEGER NOT NULL, charge INTEGER NOT NULL DEFAULT 0,
                observed_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
                provenance TEXT NOT NULL, paired_reset INTEGER NOT NULL, capacity INTEGER
            );").map_err(Error::Cache)?;
            let migrated: bool = transaction.query_row("SELECT paired_evidence FROM linear_shared_quota WHERE singleton=1", [], |row| row.get(0)).map_err(Error::Cache)?;
            if !migrated {
                for (dimension, remaining, reset, limit) in [
                    ("requests", "request_remaining", "request_reset", "request_limit"),
                    ("complexity", "complexity_remaining", "complexity_reset", "complexity_limit"),
                    ("endpoint_requests", "endpoint_remaining", "endpoint_reset", "endpoint_limit"),
                ] {
                    let (left, expiry): (Option<i64>, i64) = transaction.query_row(&format!("SELECT {remaining},{reset} FROM linear_shared_quota WHERE singleton=1"), [], |row| Ok((row.get(0)?,row.get(1)?))).map_err(Error::Cache)?;
                    let capacity: Option<i64> = transaction.query_row(&format!("SELECT {limit} FROM linear_shared_quota WHERE singleton=1"), [], |row|row.get(0)).map_err(Error::Cache)?;
                    if let Some(left) = left {
                        if left < 0 || expiry <= 0 {
                            transaction.execute("UPDATE linear_shared_quota SET evidence_error_until=-1 WHERE singleton=1", []).map_err(Error::Cache)?;
                        } else {
                            transaction.execute("INSERT INTO linear_quota_observations(dimension,remaining,charge,observed_at,expires_at,provenance,paired_reset,capacity) VALUES(?1,?2,CASE WHEN ?1='complexity' THEN (SELECT server_complexity_debt FROM linear_shared_quota WHERE singleton=1)+(SELECT COALESCE(SUM(estimate),0) FROM linear_quota_reservations) ELSE 0 END,0,?3,'legacy',0,?4)",params![dimension,left,expiry,capacity]).map_err(Error::Cache)?;
                        }
                    }
                }
                transaction.execute("UPDATE linear_shared_quota SET paired_evidence=1 WHERE singleton=1", []).map_err(Error::Cache)?;
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

    fn effective_now(&self, connection: &Connection) -> Result<i64, Error> {
        connection
            .execute(
                "UPDATE linear_shared_quota SET last_clock=MAX(last_clock,?1) WHERE singleton=1",
                [self.clock.now_epoch_millis()],
            )
            .map_err(Error::Cache)?;
        connection
            .query_row(
                "SELECT last_clock FROM linear_shared_quota WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(Error::Cache)
    }

    fn advance(&self, connection: &Connection, now: i64) -> Result<(), Error> {
        let window = i64::try_from(self.config.window_seconds)
            .unwrap_or(i64::MAX / 1000)
            .saturating_mul(1000);
        let Some(reset) = now.checked_add(window) else {
            connection
                .execute(
                    "UPDATE linear_shared_quota SET evidence_error_until=-1 WHERE singleton=1",
                    [],
                )
                .map_err(Error::Cache)?;
            return Ok(());
        };
        connection.execute(
            "UPDATE linear_shared_quota SET
                requests = CASE WHEN local_reset <= ?1 THEN 0 ELSE requests END,
                complexity = CASE WHEN local_reset <= ?1 THEN 0 ELSE complexity END,
                request_remaining = CASE WHEN request_reset <= ?1 THEN NULL ELSE request_remaining END,
                complexity_remaining = CASE WHEN complexity_reset <= ?1 THEN NULL ELSE complexity_remaining END,
                request_budget = CASE WHEN local_reset <= ?1 THEN NULL ELSE request_budget END,
                complexity_budget = CASE WHEN local_reset <= ?1 THEN NULL ELSE complexity_budget END,
                local_generation=local_generation+CASE WHEN local_reset<=?1 THEN ((?1-local_reset)/?3)+1 ELSE 0 END,
                local_reset=CASE WHEN local_reset<=?1 THEN local_reset+(((?1-local_reset)/?3)+1)*?3 ELSE local_reset END,
                server_complexity_generation=server_complexity_generation+CASE WHEN complexity_reset<=?1 THEN 1 ELSE 0 END,
                endpoint_used = CASE WHEN local_reset <= ?1 THEN 0 ELSE endpoint_used END,
                endpoint_remaining = CASE WHEN endpoint_reset <= ?1 THEN NULL ELSE endpoint_remaining END,
                endpoint_reset = CASE WHEN endpoint_reset <= ?1 THEN ?2 ELSE endpoint_reset END,
                request_reset = CASE WHEN request_reset <= ?1 THEN ?2 ELSE request_reset END,
                complexity_reset = CASE WHEN complexity_reset <= ?1 THEN ?2 ELSE complexity_reset END
             WHERE singleton = 1",
            params![now, reset, window],
        ).map_err(Error::Cache)?;
        connection
            .execute(
                "DELETE FROM linear_quota_observations WHERE expires_at<=?1",
                [now],
            )
            .map_err(Error::Cache)?;
        self.project_observations(connection)?;
        Ok(())
    }

    fn project_observations(&self, connection: &Connection) -> Result<(), Error> {
        connection.execute("UPDATE linear_shared_quota SET
            request_remaining=(SELECT MIN(remaining) FROM linear_quota_observations WHERE dimension='requests'),
            complexity_remaining=(SELECT MIN(remaining) FROM linear_quota_observations WHERE dimension='complexity'),
            endpoint_remaining=(SELECT MIN(remaining) FROM linear_quota_observations WHERE dimension='endpoint_requests') WHERE singleton=1", []).map_err(Error::Cache)?;
        Ok(())
    }

    fn read(&self, connection: &Connection) -> Result<QuotaStatus, Error> {
        let (fallback_requests, fallback_complexity) = self.config.fallback_limits();
        connection
            .query_row(
                "SELECT cooldown, requests, complexity, local_reset, local_reset,
                    request_limit, complexity_limit, request_budget, complexity_budget,local_generation,last_clock
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
                        rate_limit_until: (cooldown > row.get::<_,i64>(10)?).then_some(cooldown),
                        requests_used: row.get(1)?,
                        complexity_used: row.get(2)?,
                        request_reset_at: row.get(3)?,
                        complexity_reset_at: row.get(4)?,
                        local_generation: row.get(9)?,
                        local_window_reset_at: row.get(3)?,
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
    ) -> Result<Reservation, Error> {
        self.reserve_evaluated(estimated_cost, deadline)
            .map_err(|error| {
                let Error::QuotaAdmissionRefused { decision } = error else {
                    return error;
                };
                let retry_after = decision
                    .next_reevaluation_at
                    .map_or(Duration::ZERO, |until| {
                        ceil_delay(until, decision.evaluated_at)
                    });
                match decision.state {
                    QuotaState::ServerBackoff => Error::LinearRateLimited { retry_after },
                    QuotaState::LocalWindowHeld | QuotaState::ServerReserveHeld => {
                        Error::LinearLocalBudgetExhausted { retry_after }
                    }
                    QuotaState::RequestRejected => Error::LinearQueryComplexityExceeded {
                        cost: decision.estimated_complexity,
                        limit: UNKNOWN_COMPLEXITY,
                    },
                    QuotaState::Available | QuotaState::EvidenceUnavailable => {
                        Error::QuotaAdmissionRefused { decision }
                    }
                }
            })
    }

    /// Atomically evaluates and reserves, retaining the exact refusal for SDK consumers.
    pub fn reserve_evaluated(
        &self,
        estimated_cost: u64,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<Reservation, Error> {
        self.transaction(deadline, |transaction| {
            let now = self.effective_now(transaction)?;
            self.advance(transaction, now)?;
            let decision = self.evaluate(transaction, estimated_cost, now)?;
            if decision.state != QuotaState::Available {
                return Ok(Err(Error::QuotaAdmissionRefused { decision: Box::new(decision) }));
            }
            let state = self.read(transaction)?;
            let cost = decision.estimated_complexity;
            transaction.execute("UPDATE linear_shared_quota SET requests=requests+1, complexity=complexity+?1,
                 endpoint_used=endpoint_used+1, request_budget=?2, complexity_budget=?3 WHERE singleton=1",
                params![cost,state.request_budget,state.complexity_budget]).map_err(Error::Cache)?;
            transaction.execute("UPDATE linear_quota_observations SET charge=charge+CASE WHEN dimension='complexity' THEN ?1 ELSE 1 END", [cost]).map_err(Error::Cache)?;
            transaction.execute("INSERT INTO linear_quota_reservations(generation,estimate,server_complexity_generation)
                SELECT local_generation,?1,server_complexity_generation FROM linear_shared_quota WHERE singleton=1",[cost]).map_err(Error::Cache)?;
            let (database_identity, server_complexity_generation) = transaction.query_row("SELECT database_identity,server_complexity_generation FROM linear_shared_quota WHERE singleton=1",[],|row|Ok((row.get(0)?,row.get(1)?))).map_err(Error::Cache)?;
            Ok(Ok(Reservation { id:transaction.last_insert_rowid(),generation:state.local_generation,estimated_cost:cost,database_identity,server_complexity_generation }))
        })?
    }

    /// Evaluates a labeled GraphQL request estimate without granting permission to send.
    pub fn decision(&self, estimated_cost: u64) -> Result<QuotaDecision, Error> {
        self.decision_within(estimated_cost, None)
    }

    /// Evaluates under the caller's storage deadline without network or credentials.
    pub fn decision_within(
        &self,
        estimated_cost: u64,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<QuotaDecision, Error> {
        self.transaction(deadline, |transaction| {
            let now = self.effective_now(transaction)?;
            self.advance(transaction, now)?;
            self.evaluate(transaction, estimated_cost, now)
        })
    }

    fn evaluate(
        &self,
        connection: &Connection,
        estimated_cost: u64,
        now: i64,
    ) -> Result<QuotaDecision, Error> {
        let state = self.read(connection)?;
        let cost = if estimated_cost == 0 {
            UNKNOWN_COMPLEXITY
        } else {
            estimated_cost
        };
        let mut decision = QuotaDecision::new(
            now,
            cost,
            state.local_generation,
            state.local_window_reset_at,
        );
        let (request_limit, complexity_limit, endpoint_limit, endpoint_used, evidence_error): (Option<u64>,Option<u64>,Option<u64>,u64,i64) = connection.query_row("SELECT request_limit,complexity_limit,endpoint_limit,endpoint_used,evidence_error_until FROM linear_shared_quota WHERE singleton=1", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).map_err(Error::Cache)?;
        if let Some(until) = state.rate_limit_until.filter(|until| *until > now) {
            decision.blockers.push(QuotaBlocker::nondimensional(
                QuotaState::ServerBackoff,
                Some(until),
            ));
        }
        let pending: u64 = connection
            .query_row(
                "SELECT COUNT(*) FROM linear_quota_reservations",
                [],
                |row| row.get(0),
            )
            .map_err(Error::Cache)?;
        let pending_cost: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(estimate),0) FROM linear_quota_reservations",
                [],
                |row| row.get(0),
            )
            .map_err(Error::Cache)?;
        if pending >= 8192 {
            decision.blockers.push(QuotaBlocker::nondimensional(
                QuotaState::EvidenceUnavailable,
                None,
            ));
        }
        if evidence_error == -1 || evidence_error > now {
            decision.blockers.push(QuotaBlocker::nondimensional(
                QuotaState::EvidenceUnavailable,
                (evidence_error > 0).then_some(evidence_error),
            ));
        }
        if cost > UNKNOWN_COMPLEXITY || cost > state.complexity_budget || state.request_budget == 0
        {
            decision.blockers.push(QuotaBlocker::nondimensional(
                QuotaState::RequestRejected,
                None,
            ));
        }
        let fallback = self.config.fallback_limits();
        let dimensions = [
            (
                QuotaDimension::Requests,
                request_limit.unwrap_or(fallback.0),
                state.request_budget,
                state.requests_used,
                1,
            ),
            (
                QuotaDimension::Complexity,
                complexity_limit.unwrap_or(fallback.1),
                state.complexity_budget,
                state.complexity_used,
                cost,
            ),
        ];
        for (dimension, capacity, budget, used, estimate) in dimensions {
            if used.saturating_add(estimate) > budget {
                decision.blockers.push(QuotaBlocker::dimensional(
                    QuotaState::LocalWindowHeld,
                    dimension,
                    Some(state.local_window_reset_at),
                    capacity,
                    self.config.reserved_headroom(capacity),
                    estimate,
                    used,
                    0,
                    None,
                    None,
                ));
            }
        }
        if let Some(capacity) = endpoint_limit
            && endpoint_used.saturating_add(1) > self.config.allocate(capacity)
        {
            decision.blockers.push(QuotaBlocker::dimensional(
                QuotaState::LocalWindowHeld,
                QuotaDimension::EndpointRequests,
                Some(state.local_window_reset_at),
                capacity,
                self.config.reserved_headroom(capacity),
                1,
                endpoint_used,
                0,
                None,
                None,
            ));
        }
        let mut statement = connection.prepare("SELECT id,dimension,remaining,charge,observed_at,expires_at,provenance,paired_reset,capacity FROM linear_quota_observations ORDER BY id").map_err(Error::Cache)?;
        let observations = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                ))
            })
            .map_err(Error::Cache)?;
        for observation in observations {
            let (
                id,
                kind,
                remaining,
                charge,
                observed,
                expiry,
                provenance,
                paired,
                observed_capacity,
            ) = observation.map_err(Error::Cache)?;
            let dimension = match kind.as_str() {
                "requests" => QuotaDimension::Requests,
                "complexity" => QuotaDimension::Complexity,
                "endpoint_requests" => QuotaDimension::EndpointRequests,
                _ => {
                    decision.blockers.push(QuotaBlocker::nondimensional(
                        QuotaState::EvidenceUnavailable,
                        None,
                    ));
                    continue;
                }
            };
            if remaining < 0
                || charge < 0
                || (dimension == QuotaDimension::Complexity && charge < pending_cost)
                || observed_capacity
                    .is_some_and(|capacity| !(0..=9_007_199_254_740_991).contains(&capacity))
                || expiry <= observed
                || !(provenance == "legacy"
                    || provenance == "headers"
                    || provenance
                        .strip_prefix("reservation:")
                        .and_then(|id| id.parse::<i64>().ok())
                        .is_some_and(|id| id > 0))
            {
                decision.blockers.push(QuotaBlocker::nondimensional(
                    QuotaState::EvidenceUnavailable,
                    None,
                ));
                continue;
            }
            let (capacity, used, estimate) = match dimension {
                QuotaDimension::Requests => {
                    (request_limit.unwrap_or(fallback.0), state.requests_used, 1)
                }
                QuotaDimension::Complexity => (
                    complexity_limit.unwrap_or(fallback.1),
                    state.complexity_used,
                    cost,
                ),
                QuotaDimension::EndpointRequests => (endpoint_limit.unwrap_or(0), endpoint_used, 1),
            };
            let capacity = observed_capacity
                .and_then(|capacity| u64::try_from(capacity).ok())
                .unwrap_or(capacity);
            let reserve = self.config.reserved_headroom(capacity);
            let effective =
                u64::try_from(remaining.saturating_sub(charge).max(0)).map_err(|_| {
                    Error::InvalidQuotaReservation {
                        reason: "observation arithmetic invalid",
                    }
                })?;
            let mut evidence = QuotaBlocker::dimensional(
                QuotaState::ServerReserveHeld,
                dimension,
                Some(expiry),
                capacity,
                reserve,
                estimate,
                used,
                u64::try_from(charge).unwrap_or(u64::MAX),
                Some(effective),
                Some(id),
            );
            if dimension == QuotaDimension::EndpointRequests
                && observed_capacity.is_none()
                && endpoint_limit.is_none()
            {
                evidence.capacity = None;
            }
            evidence.observed_at = Some(observed);
            evidence.paired_reset = paired;
            // Available observations remain visible as evidence, independently of blockers.
            decision.evidence.push(evidence.clone());
            if effective < reserve.saturating_add(estimate) {
                decision.blockers.push(evidence);
            }
        }
        decision.finish();
        Ok(decision)
    }

    /// Records actual capacities, observed cost and the latest applicable cooldown.
    /// `graphql` is the complete decoded response, including its open error metadata.
    pub fn observe(
        &self,
        reservation: Reservation,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
    ) -> Result<(), Error> {
        self.observe_within(reservation, status, headers, graphql, None)
    }

    /// Observes a response with SQLite waits under the same operation deadline.
    pub fn observe_within(
        &self,
        reservation: Reservation,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<(), Error> {
        self.observe_response(Some(reservation), status, headers, graphql, deadline)
            .map(|_| ())
    }

    /// Records server metadata without fabricating an attempted-request charge.
    pub fn observe_headers(
        &self,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
    ) -> Result<(), Error> {
        self.observe_response(None, status, headers, graphql, None)
            .map(|_| ())
    }

    /// Records metadata with SQLite waits bounded by the caller's operation.
    pub fn observe_headers_within(
        &self,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<(), Error> {
        self.observe_response(None, status, headers, graphql, deadline)
            .map(|_| ())
    }

    /// Records a response and captures its resulting decision in the same transaction.
    pub fn observe_evaluated_within(
        &self,
        reservation: Reservation,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<QuotaDecision, Error> {
        self.observe_response(Some(reservation), status, headers, graphql, deadline)
    }

    fn observe_response(
        &self,
        reservation: Option<Reservation>,
        status: u16,
        headers: &HeaderMap,
        graphql: Option<&Value>,
        deadline: Option<&Deadline<'_>>,
    ) -> Result<QuotaDecision, Error> {
        self.transaction(deadline, |transaction| {
            let now = self.effective_now(transaction)?;
            self.advance(transaction, now)?;
            let provenance = reservation.as_ref().map_or_else(|| "headers".to_owned(), |reservation| format!("reservation:{}", reservation.id));
            if let Some(reservation)=reservation {
                self.settle(transaction,reservation,header_integer(headers,"x-complexity"),header_integer(headers,"x-ratelimit-complexity-remaining").is_some())?;
            }
            for (kind, dimension, limit_column) in [
                ("requests", "requests", "request_limit"),
                ("complexity", "complexity", "complexity_limit"),
                ("endpoint-requests", "endpoint_requests", "endpoint_limit"),
            ] {
                let limit = header_integer(headers, &format!("x-ratelimit-{kind}-limit"));
                transaction.execute(&format!("UPDATE linear_shared_quota SET {limit_column}=COALESCE(?1,{limit_column}) WHERE singleton=1"), [limit]).map_err(Error::Cache)?;
                let Some(remaining) = header_integer(headers, &format!("x-ratelimit-{kind}-remaining")) else { continue };
                let reset = header_integer(headers, &format!("x-ratelimit-{kind}-reset")).and_then(|value| i64::try_from(value).ok());
                // An explicitly expired pair is retired, never re-dated as fallback evidence.
                if reset.is_some_and(|reset| reset <= now) { continue; }
                let expiry = reset.unwrap_or_else(|| now.saturating_add(if remaining==0 {60_000} else {i64::try_from(self.config.window_seconds).unwrap_or(3600)*1000}));
                let capacity: Option<u64> = transaction.query_row(&format!("SELECT {limit_column} FROM linear_shared_quota WHERE singleton=1"),[],|row|row.get(0)).map_err(Error::Cache)?;
                let capacity = capacity.or_else(|| match dimension {"requests"=>Some(self.config.fallback_limits().0),"complexity"=>Some(self.config.fallback_limits().1),_=>None});
                let charge: i64 = if dimension=="complexity" {
                    transaction.query_row("SELECT COALESCE(SUM(estimate),0) FROM linear_quota_reservations",[],|row|row.get(0)).map_err(Error::Cache)?
                } else { transaction.query_row("SELECT COUNT(*) FROM linear_quota_reservations",[],|row|row.get(0)).map_err(Error::Cache)? };
                // All still-pending reservations are owned by every newly observed
                // complexity constraint; later settlement adjusts only those estimates.
                let dominated: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM linear_quota_observations WHERE dimension=?1 AND remaining-charge<=?2-?3 AND expires_at>=?4 AND COALESCE(capacity,0)>=?5)",params![dimension,remaining,charge,expiry,capacity.unwrap_or(0)], |row| row.get(0)).map_err(Error::Cache)?;
                if dominated { continue; }
                transaction.execute("DELETE FROM linear_quota_observations WHERE dimension=?1 AND remaining-charge>=?2-?3 AND expires_at<=?4 AND COALESCE(capacity,0)<=?5",params![dimension,remaining,charge,expiry,capacity.unwrap_or(0)]).map_err(Error::Cache)?;
                let count: u64 = transaction.query_row("SELECT COUNT(*) FROM linear_quota_observations",[],|row|row.get(0)).map_err(Error::Cache)?;
                if count>=512 {
                    transaction.execute("UPDATE linear_shared_quota SET evidence_error_until=CASE WHEN evidence_error_until=-1 THEN -1 ELSE MAX(evidence_error_until,?1) END WHERE singleton=1",[expiry]).map_err(Error::Cache)?;
                } else {
                    transaction.execute("INSERT INTO linear_quota_observations(dimension,remaining,charge,observed_at,expires_at,provenance,paired_reset,capacity) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![dimension,remaining,charge,now,expiry,provenance,reset.is_some(),capacity]).map_err(Error::Cache)?;
                }
            }
            self.project_observations(transaction)?;
            let mut exhausted_reset = 0;
            for kind in ["requests", "complexity", "endpoint-requests"] {
                if header_integer(headers, &format!("x-ratelimit-{kind}-remaining")) == Some(0) {
                    let reset = header_integer(headers, &format!("x-ratelimit-{kind}-reset"))
                        .and_then(|reset| i64::try_from(reset).ok()).filter(|reset| *reset > now)
                        .unwrap_or_else(|| now.saturating_add(60_000));
                    exhausted_reset = exhausted_reset.max(reset);
                }
            }
            let rate_limited = status == 429 || graphql.is_some_and(is_graphql_rate_limit);
            if rate_limited {
                let retry_after = headers.get("retry-after").and_then(|value| value.to_str().ok())
                    .and_then(|value| retry_after_until(value, now));
                let metadata = graphql.and_then(|value| metadata_retry_until(value, now));
                let until = retry_after.into_iter().chain(metadata).chain((exhausted_reset > now).then_some(exhausted_reset))
                    .filter(|until|*until>now).max().unwrap_or_else(|| now.saturating_add(60_000));
                transaction.execute("UPDATE linear_shared_quota SET cooldown=MAX(cooldown,?1) WHERE singleton=1", [until]).map_err(Error::Cache)?;
            }
            self.evaluate(transaction, UNKNOWN_COMPLEXITY, now)
        })
    }

    fn settle(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        reservation: Reservation,
        actual: Option<u64>,
        has_remaining: bool,
    ) -> Result<(), Error> {
        let (identity,local_generation,server_generation):(String,i64,i64)=transaction.query_row(
            "SELECT database_identity,local_generation,server_complexity_generation FROM linear_shared_quota WHERE singleton=1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))
        ).map_err(Error::Cache)?;
        if identity != reservation.database_identity {
            return Err(Error::InvalidQuotaReservation {
                reason: "reservation belongs to a different quota database",
            });
        }
        let stored:Option<(i64,u64,i64)>=transaction.query_row(
            "SELECT generation,estimate,server_complexity_generation FROM linear_quota_reservations WHERE id=?1",[reservation.id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))
        ).optional().map_err(Error::Cache)?;
        if stored
            != Some((
                reservation.generation,
                reservation.estimated_cost,
                reservation.server_complexity_generation,
            ))
        {
            return Err(Error::InvalidQuotaReservation {
                reason: "reservation is absent, already observed, or inconsistent",
            });
        }
        let actual = actual.unwrap_or(reservation.estimated_cost);
        let delta = i64::try_from(actual).map_err(|_| Error::InvalidQuotaReservation {
            reason: "actual complexity exceeds persistent accounting range",
        })? - i64::try_from(reservation.estimated_cost).map_err(|_| {
            Error::InvalidQuotaReservation {
                reason: "reserved complexity exceeds persistent accounting range",
            }
        })?;
        let (local_charge,debt):(i64,i64)=transaction.query_row("SELECT complexity,server_complexity_debt FROM linear_shared_quota WHERE singleton=1",[],|row|Ok((row.get(0)?,row.get(1)?))).map_err(Error::Cache)?;
        let actual_signed = i64::try_from(actual).map_err(|_| Error::InvalidQuotaReservation {
            reason: "actual cost exceeds accounting range",
        })?;
        let observation_overflow:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM linear_quota_observations WHERE dimension='complexity' AND charge>?1)",[i64::MAX.saturating_sub(delta.max(0))],|row|row.get(0)).map_err(Error::Cache)?;
        if (local_generation == reservation.generation && local_charge.checked_add(delta).is_none())
            || (server_generation == reservation.server_complexity_generation
                && !has_remaining
                && debt.checked_add(actual_signed).is_none())
            || observation_overflow
        {
            transaction
                .execute(
                    "UPDATE linear_shared_quota SET evidence_error_until=-1 WHERE singleton=1",
                    [],
                )
                .map_err(Error::Cache)?;
            return Ok(());
        }
        if local_generation == reservation.generation {
            let changed=transaction.execute("UPDATE linear_shared_quota SET complexity=complexity+?1 WHERE singleton=1 AND complexity>=?2",
                params![delta,reservation.estimated_cost]).map_err(Error::Cache)?;
            if changed != 1 {
                return Err(Error::InvalidQuotaReservation {
                    reason: "reserved complexity is missing from its accounting window",
                });
            }
        }
        let invalid: bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM linear_quota_observations WHERE dimension='complexity' AND charge<?1)",[reservation.estimated_cost],|row|row.get(0)).map_err(Error::Cache)?;
        if invalid {
            return Err(Error::InvalidQuotaReservation {
                reason: "observation is missing its reserved complexity charge",
            });
        }
        transaction.execute("UPDATE linear_quota_observations SET charge=charge+?1 WHERE dimension='complexity' AND charge>=?2",params![delta,reservation.estimated_cost]).map_err(Error::Cache)?;
        if server_generation == reservation.server_complexity_generation && !has_remaining {
            transaction.execute("UPDATE linear_shared_quota SET server_complexity_debt=server_complexity_debt+?1 WHERE singleton=1",[actual]).map_err(Error::Cache)?;
        }
        transaction
            .execute(
                "DELETE FROM linear_quota_reservations WHERE id=?1",
                [reservation.id],
            )
            .map_err(Error::Cache)?;
        Ok(())
    }

    /// Returns accounting state without sending a request or reading credentials.
    pub fn status(&self) -> Result<QuotaStatus, Error> {
        self.status_within(None)
    }

    /// Reads accounting state while honoring a bounded operation's deadline.
    pub fn status_within(&self, deadline: Option<&Deadline<'_>>) -> Result<QuotaStatus, Error> {
        self.transaction(deadline, |transaction| {
            let now = self.effective_now(transaction)?;
            self.advance(transaction, now)?;
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
            gate.observe_headers_within(429, &HeaderMap::new(), None, Some(&deadline)),
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

    #[trace("FR-031-AC-4", "FR-031-AC-5")]
    #[test]
    fn durable_ledger_rejects_replayed_completion_without_another_refund() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let gate = QuotaGate::open(
            &directory.path().join("quota.sqlite"),
            &QuotaConfig::default(),
        )
        .expect("quota");
        let original = gate.reserve(10_000, None).expect("reservation");
        // Simulate a duplicate completion at the storage boundary. The public
        // token deliberately provides neither Clone nor a caller constructor.
        let replay = Reservation {
            id: original.id,
            generation: original.generation,
            estimated_cost: original.estimated_cost,
            database_identity: original.database_identity.clone(),
            server_complexity_generation: original.server_complexity_generation,
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-complexity", "100".parse().expect("cost header"));
        gate.observe(original, 200, &headers, None)
            .expect("first settlement");
        assert!(matches!(
            gate.observe(replay, 200, &headers, None),
            Err(Error::InvalidQuotaReservation { .. })
        ));
        assert_eq!(
            gate.status().expect("one actual charge").complexity_used,
            100
        );
    }
}
