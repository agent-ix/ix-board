// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Typed admission evidence captured before any refusal is rendered.

use serde::{Deserialize, Serialize};

/// Primary admission state; all independent reasons remain in the blockers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaState {
    /// Every applicable guard admits this estimate; remote headroom may be unknown.
    Available,
    /// Fixed local allocation is exhausted.
    LocalWindowHeld,
    /// Locally enforced reserve against a live server observation.
    ServerReserveHeld,
    /// Actual HTTP 429 or GraphQL RATELIMITED cooldown.
    ServerBackoff,
    /// Persisted provenance cannot be safely interpreted.
    EvidenceUnavailable,
    /// Waiting cannot fix this estimate or configuration.
    RequestRejected,
}

impl QuotaState {
    fn precedence(self) -> u8 {
        match self {
            Self::Available => 0,
            Self::LocalWindowHeld => 1,
            Self::ServerReserveHeld => 2,
            Self::RequestRejected => 3,
            Self::EvidenceUnavailable => 4,
            Self::ServerBackoff => 5,
        }
    }
}

/// Capacity whose guard prevents a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaDimension {
    /// Workspace requests.
    Requests,
    /// Query complexity units.
    Complexity,
    /// Endpoint request allowance, conservatively shared by this GraphQL transport.
    EndpointRequests,
}

/// One independent guard and its own evidence deadline.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaBlocker {
    /// Kind of hold.
    pub state: QuotaState,
    /// Absent for backoff, invalid input or unavailable provenance.
    pub dimension: Option<QuotaDimension>,
    /// Absolute owning deadline; no deadline means transition is unknown.
    pub deadline: Option<i64>,
    /// Capacity used for this evaluation, absent for nondimensional holds.
    pub capacity: Option<u64>,
    /// Locally reserved headroom.
    pub reserve: Option<u64>,
    /// Charge for this request estimate.
    pub estimated_charge: Option<u64>,
    /// Usage in the fixed local generation.
    pub local_usage: Option<u64>,
    /// Pending and subsequent charges owned by the observation.
    pub outstanding_charge: Option<u64>,
    /// Observed remaining after owned charges; absence means unknown.
    pub effective_remaining: Option<u64>,
    /// Durable observation identity, distinct from the local generation.
    pub observation_id: Option<i64>,
    /// Time this response was observed, zero for migrated legacy evidence.
    pub observed_at: Option<i64>,
    /// Whether the deadline was supplied with the remaining value in one response.
    pub paired_reset: bool,
}

impl QuotaBlocker {
    pub(crate) fn nondimensional(state: QuotaState, deadline: Option<i64>) -> Self {
        Self {
            state,
            dimension: None,
            deadline,
            capacity: None,
            reserve: None,
            estimated_charge: None,
            local_usage: None,
            outstanding_charge: None,
            effective_remaining: None,
            observation_id: None,
            observed_at: None,
            paired_reset: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dimensional(
        state: QuotaState,
        dimension: QuotaDimension,
        deadline: Option<i64>,
        capacity: u64,
        reserve: u64,
        estimate: u64,
        used: u64,
        outstanding: u64,
        remaining: Option<u64>,
        observation_id: Option<i64>,
    ) -> Self {
        Self {
            state,
            dimension: Some(dimension),
            deadline,
            capacity: Some(capacity),
            reserve: Some(reserve),
            estimated_charge: Some(estimate),
            local_usage: Some(used),
            outstanding_charge: Some(outstanding),
            effective_remaining: remaining,
            observation_id,
            observed_at: None,
            paired_reset: false,
        }
    }
}

/// Atomic explanation of one labeled estimate; it never grants a future send.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaDecision {
    /// Highest priority active hold, or Available.
    pub state: QuotaState,
    /// Every applicable independent hold.
    pub blockers: Vec<QuotaBlocker>,
    /// Most restrictive live server evidence per dimension; all active blockers remain below.
    pub evidence: Vec<QuotaBlocker>,
    /// Absolute evaluation time in Unix milliseconds.
    pub evaluated_at: i64,
    /// Earliest timed transition; eligibility must be reevaluated then.
    pub next_reevaluation_at: Option<i64>,
    /// Normalized complexity estimate, with zero charged conservatively.
    pub estimated_complexity: u64,
    /// Endpoint to which the decision applies.
    pub endpoint: String,
    /// Unresolved pre-fence reservations; their late response/cooldown is unknown.
    #[serde(default)]
    pub legacy_pending_reservations: u64,
    /// Fixed local accounting generation, independent of observation identities.
    pub local_generation: i64,
    /// Fixed local deadline in Unix milliseconds.
    pub local_window_reset_at: i64,
}

impl QuotaDecision {
    pub(crate) fn new(now: i64, cost: u64, generation: i64, reset: i64) -> Self {
        Self {
            state: QuotaState::Available,
            blockers: Vec::new(),
            evidence: Vec::new(),
            evaluated_at: now,
            next_reevaluation_at: None,
            estimated_complexity: cost,
            endpoint: "graphql".to_owned(),
            local_generation: generation,
            legacy_pending_reservations: 0,
            local_window_reset_at: reset,
        }
    }

    pub(crate) fn finish(&mut self) {
        self.state = self
            .blockers
            .iter()
            .map(|blocker| blocker.state)
            .max_by_key(|state| state.precedence())
            .unwrap_or(QuotaState::Available);
        self.next_reevaluation_at = self
            .blockers
            .iter()
            .filter_map(|blocker| blocker.deadline)
            .min();
    }
}

impl std::fmt::Display for QuotaDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.state == QuotaState::ServerBackoff {
            let until = self
                .blockers
                .iter()
                .find(|blocker| blocker.state == QuotaState::ServerBackoff)
                .and_then(|blocker| blocker.deadline)
                .unwrap_or(self.evaluated_at);
            let seconds = until
                .saturating_sub(self.evaluated_at)
                .max(0)
                .unsigned_abs()
                .div_ceil(1000);
            return write!(f, "Linear rate limited; retry in {seconds}s");
        }
        let label = match self.state {
            QuotaState::Available => "quota available for evaluated estimate",
            QuotaState::LocalWindowHeld => "local allocation hold",
            QuotaState::ServerReserveHeld => "locally enforced server reserve hold",
            QuotaState::EvidenceUnavailable => "quota evidence unavailable",
            QuotaState::RequestRejected => "quota request rejected",
            QuotaState::ServerBackoff => unreachable!("handled above"),
        };
        write!(f, "{label}")?;
        if self.legacy_pending_reservations > 0 {
            write!(
                f,
                "; {} pre-paired reservations require cutover reconciliation; rebuild ix-board clients",
                self.legacy_pending_reservations
            )?;
        }
        for blocker in &self.blockers {
            write!(
                f,
                "; {:?} {:?}, owning deadline {:?}",
                blocker.state, blocker.dimension, blocker.deadline
            )?;
        }
        if let Some(until) = self.next_reevaluation_at {
            write!(
                f,
                "; next reevaluation at {until} (admission not guaranteed)"
            )?;
        }
        Ok(())
    }
}
