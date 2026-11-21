// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
use super::wire::AssignmentStatus;
use super::{Assignment, Binding, Change, Config, Error, Event, Selectors};
use crate::retro::Record;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Persisted exporter state; all native identities remain outside metric labels.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub(crate) cursor: String,
    pub(crate) history_from_ms: Option<i64>,
    pub(crate) history_at_ms: i64,
    pub(crate) history_ok: bool,
    pub(crate) history_gaps: u64,
    pub(crate) journal_from_ms: i64,
    pub(crate) scopes: BTreeMap<String, ScopeState>,
}
/// Bounded cached state for one operational scope.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeState {
    pub(crate) bindings: BTreeMap<String, Binding>,
    pub(crate) scope_compatible: BTreeMap<String, bool>,
    pub(crate) resolved_selectors: Option<Selectors>,
    pub(crate) selectors: Option<Selectors>,
    pub(crate) query_identity: Option<String>,
    pub(crate) comparable_since_ms: i64,
    pub(crate) members: BTreeMap<String, Member>,
    pub(crate) tracker_at_ms: i64,
    pub(crate) tracker_ok: bool,
    pub(crate) record: Option<Record>,
    pub(crate) pr_stats: Option<super::pr_stats::PrStats>,
    pub(crate) review_stats: Option<super::reviews::ReviewStats>,
    pub(crate) pr_success_at_ms: i64,
    pub(crate) review_success_at_ms: i64,
    pub(crate) assignments: BTreeMap<String, Active>,
    pub(crate) pauses: BTreeMap<String, bool>,
    pub(crate) reported_scope: BTreeMap<String, String>,
    pub(crate) totals: BTreeMap<String, f64>,
    pub(crate) samples: BTreeMap<String, Sample>,
    pub(crate) stages: BTreeMap<String, Vec<(i64, f64)>>,
    pub(crate) lifecycle_at_ms: i64,
    pub(crate) lifecycle_ok: bool,
    pub(crate) lifecycle_success_at_ms: i64,
    pub(crate) population_observed: bool,
    pub(crate) unattributed_executing_agents: usize,
    pub(crate) covered_seconds: f64,
    pub(crate) effort_points: Vec<(i64, f64, f64)>,
    pub(crate) build_execution_seconds_24h: Option<f64>,
    pub(crate) ready_issues: Option<usize>,
    pub(crate) blocked_issues: Option<usize>,
    pub(crate) coverage_started_ms: Option<i64>,
    pub(crate) last_error: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Member {
    pub ticket: String,
    pub state: crate::WorkflowStateType,
    pub leaf: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Active {
    pub binding: Binding,
    pub assignment: Assignment,
    pub started_ms: i64,
    pub start_known: bool,
    pub accounted_ms: i64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sample {
    pub at_ms: i64,
    pub working: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct ExecutionSample {
    pub machine: String,
    pub session: String,
    pub parent_session: Option<String>,
    pub working: bool,
    pub at_ms: i64,
}

pub(crate) fn stage(role: &str) -> &'static str {
    match role {
        "coder" | "code" | "coding" => "coding",
        "reviewer" | "review" => "review",
        "fix" | "fixes" => "fix",
        "gate" => "gate",
        "build_queue" => "build_queue",
        "decision" | "spec-steward" => "decision",
        _ => "other",
    }
}
fn pending(status: AssignmentStatus) -> bool {
    matches!(
        status,
        AssignmentStatus::Running | AssignmentStatus::Waiting
    )
}
fn state_label(status: AssignmentStatus) -> &'static str {
    match status {
        AssignmentStatus::Waiting => "waiting",
        _ => "assigned",
    }
}
fn add(totals: &mut BTreeMap<String, f64>, key: &str, value: f64) {
    *totals.entry(key.into()).or_default() += value;
}
fn scope_matches(selected: &Selectors, reported: &relay_work_wire::ReportedScope) -> bool {
    let same = |a: &[String], b: &[String]| {
        a.iter().collect::<BTreeSet<_>>() == b.iter().collect::<BTreeSet<_>>()
    };
    same(&selected.initiatives, &reported.initiatives)
        && same(&selected.projects, &reported.projects)
        && same(&selected.tickets, &reported.tickets)
}
fn numeric_cursor(id: &str) -> Result<(u64, u64), Error> {
    let (a, b) = id.split_once('-').ok_or(Error::Invalid("stream cursor"))?;
    Ok((
        a.parse().map_err(|_| Error::Invalid("stream cursor"))?,
        b.parse().map_err(|_| Error::Invalid("stream cursor"))?,
    ))
}

impl State {
    pub(crate) fn bootstrap(&mut self, config: &Config, snapshot: &relay_work_wire::Snapshot) {
        for scope in config
            .scopes
            .iter()
            .filter(|s| s.owns(&snapshot.binding.address))
        {
            let state = self.scopes.entry(scope.key()).or_default();
            let incarnation = snapshot.binding.key();
            let selectors = state
                .resolved_selectors
                .as_ref()
                .unwrap_or(&scope.selectors);
            state.scope_compatible.insert(
                snapshot.binding.address.to_lowercase(),
                snapshot
                    .scope
                    .as_ref()
                    .is_some_and(|reported| scope_matches(selectors, reported)),
            );
            if state
                .bindings
                .get(&snapshot.binding.address.to_lowercase())
                .is_some_and(|b| b.key() != incarnation)
            {
                state
                    .pauses
                    .remove(&snapshot.binding.address.to_lowercase());
            }
            state.assignments.retain(|_, a| {
                !a.binding
                    .address
                    .eq_ignore_ascii_case(&snapshot.binding.address)
                    || a.binding.key() == incarnation
            });
            state.bindings.insert(
                snapshot.binding.address.to_lowercase(),
                snapshot.binding.clone(),
            );
            if let Some(paused) = snapshot.paused {
                state
                    .pauses
                    .insert(snapshot.binding.address.to_lowercase(), paused);
            }
            for agent in snapshot.assignments.iter().filter(|a| pending(a.status)) {
                let key = format!("{incarnation}:{}", agent.agent_id);
                if state.assignments.len() < 4096 {
                    state.assignments.entry(key).or_insert_with(|| Active {
                        binding: snapshot.binding.clone(),
                        assignment: agent.clone(),
                        started_ms: snapshot.observed_at_ms,
                        accounted_ms: snapshot.observed_at_ms,
                        start_known: false,
                    });
                }
            }
        }
    }
    /// Establishes only explicitly configured scopes, preserving matching durable totals.
    pub fn configure(&mut self, config: &Config) {
        let keys: BTreeSet<_> = config.scopes.iter().map(|s| s.key()).collect();
        self.scopes.retain(|key, _| keys.contains(key));
        for scope in &config.scopes {
            let state = self.scopes.entry(scope.key()).or_default();
            if state
                .selectors
                .as_ref()
                .is_some_and(|s| s != &scope.selectors)
            {
                state.effort_points.clear();
                state.samples.clear();
                state.stages.clear();
                state.lifecycle_ok = false;
                state.tracker_ok = false;
                state.coverage_started_ms = None;
            }
            state.bindings.retain(|address, _| scope.owns(address));
            state.pauses.retain(|address, _| scope.owns(address));
            state
                .assignments
                .retain(|_, a| scope.owns(&a.binding.address));
        }
        if self.cursor.is_empty() {
            self.cursor = "0-0".into();
        }
    }
    /// Applies an immutable page event once. The caller checkpoints its cursor atomically.
    pub fn apply(&mut self, config: &Config, event: &Event) -> Result<bool, Error> {
        if numeric_cursor(&event.id)? <= numeric_cursor(&self.cursor)? {
            return Ok(false);
        }
        if event.at_ms < 0 || event.binding.process_id == 0 || event.binding.session.is_empty() {
            return Err(Error::Invalid("invalid event identity or server timestamp"));
        }
        for scope in config
            .scopes
            .iter()
            .filter(|s| s.owns(&event.binding.address))
        {
            let state = self.scopes.entry(scope.key()).or_default();
            let incarnation = event.binding.key();
            state
                .bindings
                .insert(event.binding.address.to_lowercase(), event.binding.clone());
            // A new session in an address ends attribution for its predecessor.
            state.assignments.retain(|_, a| {
                !a.binding
                    .address
                    .eq_ignore_ascii_case(&event.binding.address)
                    || a.binding.key() == incarnation
            });
            match &event.change {
                Change::Scope { scope: reported } => {
                    let selectors = state
                        .resolved_selectors
                        .as_ref()
                        .unwrap_or(&scope.selectors);
                    state.scope_compatible.insert(
                        event.binding.address.to_lowercase(),
                        scope_matches(selectors, reported),
                    );
                    let digest = serde_json::to_string(&(
                        reported.initiatives.clone(),
                        reported.projects.clone(),
                        reported.tickets.clone(),
                    ))?;
                    if state
                        .reported_scope
                        .get(&incarnation)
                        .is_some_and(|old| old != &digest)
                    {
                        add(&mut state.totals, "reported_scope_revisions", 1.0);
                    }
                    state.reported_scope.insert(incarnation, digest);
                }
                Change::Agent { agent } => {
                    if agent.agent_id.len() > 128
                        || agent.ticket.len() > 128
                        || agent.role.len() > 128
                    {
                        return Err(Error::Invalid("oversize assignment identity"));
                    }
                    let key = format!("{incarnation}:{}", agent.agent_id);
                    if let Some(previous) = state.assignments.get(&key) {
                        if !previous.start_known && event.at_ms <= previous.accounted_ms {
                            continue;
                        }
                        if event.at_ms < previous.accounted_ms {
                            return Err(Error::Invalid("event time moved backwards"));
                        }
                        let same = previous.assignment.ticket == agent.ticket
                            && stage(&previous.assignment.role) == stage(&agent.role);
                        if same && previous.assignment.status == agent.status {
                            // Worker binding can change without a stage transition.
                            if let Some(active) = state.assignments.get_mut(&key) {
                                active.assignment = agent.clone();
                            }
                            continue;
                        }
                        if same
                            && previous.start_known
                            && agent.status == AssignmentStatus::Done
                            && state.members.values().any(|m| m.ticket == agent.ticket)
                        {
                            let duration =
                                (event.at_ms - previous.started_ms).max(0) as f64 / 1000.0;
                            let values = state.stages.entry(stage(&agent.role).into()).or_default();
                            if values.len() == 1000 {
                                values.remove(0);
                            }
                            values.push((event.at_ms, duration));
                        }
                    }
                    let start = state
                        .assignments
                        .get(&key)
                        .filter(|a| {
                            a.assignment.ticket == agent.ticket
                                && stage(&a.assignment.role) == stage(&agent.role)
                        })
                        .map_or(event.at_ms, |a| a.started_ms);
                    let start_known = state
                        .assignments
                        .get(&key)
                        .filter(|a| {
                            a.assignment.ticket == agent.ticket
                                && stage(&a.assignment.role) == stage(&agent.role)
                        })
                        .is_none_or(|a| a.start_known);
                    account_one(state, &key, event.at_ms);
                    if pending(agent.status) {
                        if state.assignments.len() >= 4096 && !state.assignments.contains_key(&key)
                        {
                            return Err(Error::Invalid("assignment state capacity reached"));
                        }
                        state.assignments.insert(
                            key,
                            Active {
                                binding: event.binding.clone(),
                                assignment: agent.clone(),
                                started_ms: start,
                                start_known,
                                accounted_ms: event.at_ms,
                            },
                        );
                    } else {
                        state.assignments.remove(&key);
                    }
                }
                Change::Remove { agent_id } => {
                    let key = format!("{incarnation}:{agent_id}");
                    account_one(state, &key, event.at_ms);
                    state.assignments.remove(&key);
                }
                Change::Activity { paused } => {
                    state
                        .pauses
                        .insert(event.binding.address.to_lowercase(), *paused);
                }
            }
        }
        self.cursor = event.id.clone();
        Ok(true)
    }
    pub(crate) fn observe(
        &mut self,
        config: &Config,
        samples: &[ExecutionSample],
        now: i64,
        complete: bool,
    ) {
        // Only uniquely attributed sessions receive execution credit. A worker
        // assigned to overlapping scopes is shared, not split heuristically.
        let mut attribution: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut bound_roots: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for scope in &config.scopes {
            if let Some(state) = self.scopes.get(&scope.key()) {
                let selected: BTreeSet<_> =
                    state.members.values().map(|m| m.ticket.as_str()).collect();
                for a in state
                    .assignments
                    .values()
                    .filter(|a| selected.contains(a.assignment.ticket.as_str()))
                {
                    let session = a
                        .assignment
                        .session
                        .as_deref()
                        .unwrap_or(&a.assignment.agent_id);
                    let key = format!("{}:{session}", a.binding.machine);
                    attribution.entry(key).or_default().push(scope.key());
                }
                // Lead orchestration counts as effort only when its native
                // session is established by a retained report for this scope.
                for binding in state.bindings.values() {
                    let key = format!("{}:{}", binding.machine, binding.session);
                    bound_roots
                        .entry(key.clone())
                        .or_default()
                        .push(scope.key());
                    if state.scope_compatible.get(&binding.address.to_lowercase()) == Some(&true) {
                        attribution.entry(key).or_default().push(scope.key());
                    }
                }
            }
        }
        for values in attribution.values_mut() {
            values.sort();
            values.dedup();
        }
        let mut related = attribution.clone();
        for (key, owners) in bound_roots {
            related.entry(key).or_default().extend(owners);
        }
        for owners in related.values_mut() {
            owners.sort();
            owners.dedup();
        }
        for _ in 0..32 {
            let mut changed = false;
            for sample in samples {
                let key = format!("{}:{}", sample.machine, sample.session);
                if !related.contains_key(&key)
                    && let Some(parent) = &sample.parent_session
                    && let Some(owners) = related
                        .get(&format!("{}:{parent}", sample.machine))
                        .cloned()
                {
                    related.insert(key, owners);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for scope in &config.scopes {
            let Some(state) = self.scopes.get_mut(&scope.key()) else {
                continue;
            };
            let keys: Vec<_> = state.assignments.keys().cloned().collect();
            for key in keys {
                account_one(state, &key, now.min(self.history_at_ms));
            }
            let previous_lifecycle_at = state.lifecycle_at_ms;
            let previous_complete = state.lifecycle_ok;
            state.unattributed_executing_agents = samples
                .iter()
                .filter(|sample| {
                    let key = format!("{}:{}", sample.machine, sample.session);
                    sample.working
                        && related
                            .get(&key)
                            .is_some_and(|owners| owners.contains(&scope.key()))
                        && !attribution
                            .get(&key)
                            .is_some_and(|owners| owners == &[scope.key()])
                })
                .count();
            state.population_observed = complete
                && scope.leads.iter().all(|lead| {
                    state.bindings.get(&lead.to_lowercase()).is_some_and(|b| {
                        samples
                            .iter()
                            .any(|s| s.machine == b.machine && s.session == b.session)
                    })
                });
            let complete = complete
                && state.unattributed_executing_agents == 0
                && scope.leads.iter().all(|lead| {
                    state.bindings.get(&lead.to_lowercase()).is_some_and(|b| {
                        samples
                            .iter()
                            .any(|s| s.machine == b.machine && s.session == b.session)
                    })
                })
                && state
                    .assignments
                    .values()
                    .filter(|a| {
                        state
                            .members
                            .values()
                            .any(|m| m.ticket == a.assignment.ticket)
                    })
                    .all(|a| {
                        samples.iter().any(|s| {
                            s.machine == a.binding.machine
                                && s.session
                                    == a.assignment
                                        .session
                                        .as_deref()
                                        .unwrap_or(&a.assignment.agent_id)
                        })
                    });
            state.lifecycle_at_ms = now;
            state.lifecycle_ok = complete;
            if complete {
                state.lifecycle_success_at_ms = now;
            }
            let mut current = BTreeMap::new();
            if complete {
                for sample in samples {
                    let key = format!("{}:{}", sample.machine, sample.session);
                    if attribution
                        .get(&key)
                        .is_some_and(|owners| owners == &[scope.key()])
                    {
                        if let Some(old) = state.samples.get(&key) {
                            let delta = sample.at_ms - old.at_ms;
                            if delta > 0
                                && delta
                                    <= i64::try_from(
                                        config.activity_interval_seconds.saturating_mul(3) * 1000,
                                    )
                                    .unwrap_or(i64::MAX)
                            {
                                let seconds = delta as f64 / 1000.0;
                                if old.working && sample.working {
                                    add(&mut state.totals, "execution_seconds", seconds);
                                }
                                add(&mut state.totals, "observed_agent_seconds", seconds);
                            }
                        }
                        current.insert(
                            key,
                            Sample {
                                at_ms: sample.at_ms,
                                working: sample.working,
                            },
                        );
                    }
                }
                if state.coverage_started_ms.is_none() {
                    state.coverage_started_ms = Some(now);
                }
                // Elapsed coverage is separate from summed agent-seconds.
                if previous_complete {
                    let delta = now - previous_lifecycle_at;
                    if delta > 0
                        && delta
                            <= i64::try_from(config.activity_interval_seconds * 3000)
                                .unwrap_or(i64::MAX)
                    {
                        state.covered_seconds += delta as f64 / 1000.0;
                    }
                }
            }
            state.samples = current;
            let cutoff = now - i64::try_from(config.retention_seconds * 1000).unwrap_or(i64::MAX);
            for values in state.stages.values_mut() {
                values.retain(|v| v.0 >= cutoff);
            }
            if state
                .effort_points
                .last()
                .is_none_or(|p| now - p.0 >= 300000)
            {
                state.effort_points.push((
                    now,
                    *state.totals.get("execution_seconds").unwrap_or(&0.0),
                    state.covered_seconds,
                ));
                state.effort_points.retain(|p| p.0 >= now - 604800000);
            }
        }
    }
}
fn account_one(state: &mut ScopeState, key: &str, now: i64) {
    let Some(active) = state.assignments.get_mut(key) else {
        return;
    };
    let elapsed = (now - active.accounted_ms).max(0) as f64 / 1000.0;
    // This is reported residence, never execution. Unknown/missing membership
    // cannot credit another team's ticket.
    if state
        .members
        .values()
        .any(|m| m.ticket == active.assignment.ticket)
    {
        let metric = format!(
            "assignment_seconds:{}:{}",
            stage(&active.assignment.role),
            state_label(active.assignment.status)
        );
        add(&mut state.totals, &metric, elapsed);
    }
    active.accounted_ms = now.max(active.accounted_ms);
}
