// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Canonical deterministic department calculations, isolated by source capability (FR-029).
use super::{
    CATALOG, CATALOG_VERSION, Config, Distribution, Evidence, Fact, IssueFact, Measurement,
    Observation, PinEventKind, Report, Scope, Severity, Summary, Value,
};
use crate::retro::{Direction, Frame, Metric, Status};
use std::collections::{BTreeMap, BTreeSet};

wire! {
    /// One repeat detector finding, with stable ordered source event IDs.
    LoopFinding { /// Ordered source events.
        event_ids: Vec<String>, /// First event UTC time.
        start: String, /// Last event UTC time.
        end: String, /// Source-native participants.
        participants: BTreeSet<String>, /// Stable detector/entity/event identity.
        deduplication_key: String }
}
wire! {
    /// A detector result; incomplete ordering is unavailable, never empty success.
    LoopReport { /// Stable detector ID.
        detector_id: String, /// Available or unavailable.
        status: Status, /// Missing capability/configuration reason.
        reason: Option<String>, /// Deterministically ordered, deduplicated findings.
        findings: Vec<LoopFinding> }
}
fn epoch(at: &str) -> Result<f64, String> {
    crate::drift::timestamp_epoch(at).map_err(|e| e.to_string())
}
fn event_time(o: &Observation) -> Result<f64, String> {
    epoch(
        o.event_at
            .as_deref()
            .ok_or("missing source event timestamp")?,
    )
}
fn family(fact: &Fact) -> &'static str {
    match fact {
        Fact::Issue(_) => "issue",
        Fact::State(_) => "state",
        Fact::Lane(_) => "lane",
        Fact::PullRequest(_) => "pull_request",
        Fact::Pin(_) => "pin",
        Fact::PinEvent(_) => "pin_event",
        Fact::Check(_) => "check",
        Fact::Lifecycle(_) => "lifecycle",
    }
}

struct Calculator<'a> {
    config: &'a Config,
    frame: Frame,
    batches: &'a [Evidence],
    rows: Vec<&'a Observation>,
}
impl<'a> Calculator<'a> {
    fn new(config: &'a Config, frame: Frame, batches: &'a [Evidence]) -> Self {
        let issues: BTreeSet<&str> = batches
            .iter()
            .flat_map(|b| &b.observations)
            .filter_map(|o| match &o.fact {
                Fact::Issue(i) if selected_issue(&config.scope, i) => Some(o.entity_id.as_str()),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        let rows = batches
            .iter()
            .flat_map(|b| &b.observations)
            .filter(|o| {
                let selected = match &o.fact {
                    Fact::Issue(i) => selected_issue(&config.scope, i),
                    Fact::State(_) => issues.contains(o.entity_id.as_str()),
                    Fact::Lane(_) | Fact::Lifecycle(_) => {
                        config.scope.lanes.iter().any(|s| s.id == o.entity_id)
                    }
                    Fact::PullRequest(p) => {
                        config.scope.repositories.iter().any(|s| s.id == p.repo)
                    }
                    Fact::Pin(p) => config.scope.repositories.iter().any(|s| s.id == p.repo),
                    Fact::PinEvent(_) | Fact::Check(_) => o
                        .scope
                        .repositories
                        .iter()
                        .any(|s| config.scope.repositories.iter().any(|c| c.id == s.id)),
                };
                selected && seen.insert(o.event_id.as_str())
            })
            .collect();
        Self {
            config,
            frame,
            batches,
            rows,
        }
    }
    fn contains(&self, at: f64) -> bool {
        at > self.frame.now - self.frame.window_secs as f64 && at <= self.frame.now
    }
    fn capability(&self, family: &str) -> Result<f64, String> {
        let batches: Vec<_> = self
            .batches
            .iter()
            .filter(|b| b.complete.contains(family))
            .collect();
        if batches.is_empty() {
            return Err(self
                .batches
                .iter()
                .find_map(|b| b.reasons.get(family))
                .cloned()
                .unwrap_or_else(|| {
                    format!("{family} collector/history not implemented or configured")
                }));
        }
        let mut age: f64 = 0.0;
        for b in batches {
            let limit = self
                .config
                .freshness_seconds
                .get(&b.source)
                .ok_or_else(|| format!("missing freshness_seconds.{}", b.source))?;
            let at = epoch(
                b.observed_at
                    .as_deref()
                    .ok_or_else(|| format!("{} missing source timestamp", b.source))?,
            )?;
            let source_age = self.frame.now - at;
            if source_age < -(self.config.clock_tolerance_seconds as f64) {
                return Err(format!("{} excessive future clock skew", b.source));
            }
            if source_age > *limit as f64 {
                return Err(format!("{} stale source", b.source));
            }
            age = age.max(source_age.max(0.0));
        }
        for row in self.rows.iter().filter(|o| family == self::family(&o.fact)) {
            if !row.complete && family != "lifecycle" {
                return Err(format!("{family} partial source row {}", row.event_id));
            }
            let observed = epoch(
                row.observed_at
                    .as_deref()
                    .ok_or("missing row observation timestamp")?,
            )?;
            if observed > self.frame.now + self.config.clock_tolerance_seconds as f64 {
                return Err("excessive future row observation skew".into());
            }
            if let Some(at) = &row.event_at
                && epoch(at)? > self.frame.now + self.config.clock_tolerance_seconds as f64
            {
                return Err("excessive future event skew".into());
            }
        }
        Ok(age)
    }
    fn issues(&self) -> Vec<(&Observation, &IssueFact)> {
        let mut entities = BTreeMap::new();
        for o in &self.rows {
            if let Fact::Issue(i) = &o.fact {
                entities.insert(o.entity_id.as_str(), (*o, i));
            }
        }
        entities.into_values().collect()
    }
    fn attributed(&self, o: &Observation, classifier: Option<&str>) -> Result<(), String> {
        let a = o
            .attribution
            .as_ref()
            .ok_or_else(|| format!("missing attribution for {}", o.event_id))?;
        if a.actor.is_empty() || a.classifier_version.is_empty() || a.offset_end < a.offset_start {
            return Err("incomplete classification attribution".into());
        }
        if classifier.is_some_and(|c| c != a.classifier_version) {
            return Err("diff classifier_version mismatch".into());
        }
        Ok(())
    }
    fn calculate(&self, id: &str) -> Result<(Value, f64, Option<u64>), String> {
        let issue_count = |n: usize| u64::try_from(n).map_err(|_| "population overflow".to_owned());
        let scalar = |v, age, n| Ok((Value::Scalar(v), age, n));
        let parameters = &self.config.parameters;
        match id {
            "lane_execution_state" => {
                let age = self.capability("lifecycle")?;
                let mut values = BTreeMap::new();
                for observation in &self.rows {
                    if let Fact::Lifecycle(lifecycle) = &observation.fact {
                        let category =
                            serde_json::to_value(lifecycle.verdict).map_err(|e| e.to_string())?;
                        let category = category
                            .as_str()
                            .ok_or("lifecycle verdict is not categorical")?;
                        if lifecycle.verdict == crate::agents::Verdict::Unknown {
                            return Err(format!(
                                "unavailable lifecycle category {}: {:?}",
                                observation.entity_id, lifecycle.notes
                            ));
                        }
                        values.insert(
                            observation.entity_id.clone(),
                            super::SourceObservation {
                                category: category.into(),
                                source_schema_version: lifecycle.source_schema_version.clone(),
                                observation_ref: observation.event_id.clone(),
                            },
                        );
                    }
                }
                if self.config.scope.lanes.is_empty()
                    || self
                        .config
                        .scope
                        .lanes
                        .iter()
                        .any(|s| !values.contains_key(&s.id))
                {
                    return Err("missing configured lane lifecycle source row".into());
                }
                Ok((Value::SourceObservation(values), age, None))
            }
            "wip_active"
            | "wip_capacity_ratio"
            | "claims_without_movement"
            | "merged_open"
            | "oldest_gated_hours" => {
                let age = self.capability("issue")?;
                let issues = self.issues();
                let started: Vec<_> = issues
                    .iter()
                    .filter(|(_, i)| i.started && !i.terminal)
                    .collect();
                let count = issue_count(issues.len())?;
                match id {
                    "wip_active" => scalar(started.len() as f64, age, Some(count)),
                    "wip_capacity_ratio" => {
                        if self.config.scope.lanes.is_empty() {
                            return Err("missing lane selectors/capacity".into());
                        }
                        let mut rows = BTreeMap::new();
                        for lane in &self.config.scope.lanes {
                            let capacity = parameters
                                .lane_capacity
                                .get(&lane.id)
                                .filter(|v| **v > 0)
                                .ok_or_else(|| {
                                    format!("missing or zero lane_capacity.{}", lane.id)
                                })?;
                            let mut n = 0u64;
                            for (o, _) in &started {
                                let owner = parameters
                                    .issue_lanes
                                    .get(&o.entity_id)
                                    .ok_or("missing native issue lane assignment")?;
                                if owner == &lane.id {
                                    n += 1;
                                }
                            }
                            rows.insert(lane.id.clone(), n as f64 / *capacity as f64);
                        }
                        Ok((Value::Keyed(rows), age, Some(count)))
                    }
                    "claims_without_movement" => {
                        let stale = parameters.stale_seconds.ok_or("missing stale_seconds")? as f64;
                        let mut n = 0.0;
                        for (_, i) in started {
                            let claim = epoch(
                                i.claim_at
                                    .as_deref()
                                    .ok_or("missing latest claim/start timestamp history")?,
                            )?;
                            if !i.movement_complete {
                                return Err(
                                    "missing complete PR-head/commit/worktree movement history"
                                        .into(),
                                );
                            }
                            let moved = i.movement_at.as_deref().map(epoch).transpose()?;
                            let cutoff = self.frame.now - stale;
                            if claim < cutoff && moved.is_none_or(|t| t <= cutoff) {
                                n += 1.0;
                            }
                        }
                        scalar(n, age, Some(count))
                    }
                    "merged_open" => {
                        let required = parameters
                            .completion_obligations
                            .as_ref()
                            .filter(|r| !r.is_empty())
                            .ok_or("missing completion_obligations")?;
                        let mut n = 0.0;
                        for (_, i) in issues.iter().filter(|(_, i)| !i.terminal) {
                            let merged =
                                i.merged_pr.ok_or("incomplete merged attachment evidence")?;
                            if merged {
                                let met = i
                                    .obligations
                                    .as_ref()
                                    .ok_or("missing completion obligation evidence")?;
                                if required.is_subset(met) {
                                    n += 1.0;
                                }
                            }
                        }
                        scalar(n, age, Some(count))
                    }
                    "oldest_gated_hours" => {
                        let graph = self.graph()?;
                        let mut max: Option<f64> = None;
                        for (_, i) in issues.iter().filter(|(o, i)| {
                            !i.terminal
                                && graph
                                    .get(&o.entity_id)
                                    .is_some_and(|edges| !edges.is_empty())
                        }) {
                            let at = epoch(
                                i.blocked_at
                                    .as_deref()
                                    .ok_or("missing latest blocked-condition entry history")?,
                            )?;
                            max = Some(max.unwrap_or(0.0).max((self.frame.now - at) / 3600.0));
                        }
                        scalar(
                            max.ok_or("empty gated duration population")?,
                            age,
                            Some(count),
                        )
                    }
                    _ => Err("unknown issue metric".into()),
                }
            }
            "open_chain_length" | "lane_fanin" => {
                let age = self.capability("issue")?;
                let graph = self.graph()?;
                if id == "open_chain_length" {
                    if parameters.gates.is_empty() {
                        return Err("missing dependency gate selection".into());
                    }
                    let mut max = 0;
                    for gate in &parameters.gates {
                        max = max.max(depth(gate, &graph, &mut BTreeSet::new())?);
                    }
                    scalar(max as f64, age, Some(issue_count(graph.len())?))
                } else {
                    let mut rows: BTreeMap<String, BTreeSet<String>> = self
                        .config
                        .scope
                        .lanes
                        .iter()
                        .map(|s| (s.id.clone(), BTreeSet::new()))
                        .collect();
                    for (dependent, blockers) in &graph {
                        for blocker in blockers {
                            let lane = parameters
                                .issue_lanes
                                .get(blocker)
                                .ok_or("missing blocker owning lane assignment")?;
                            let set = rows
                                .get_mut(lane)
                                .ok_or("blocker owning lane outside configured scope")?;
                            set.insert(dependent.clone());
                        }
                    }
                    Ok((
                        Value::Keyed(rows.into_iter().map(|(k, v)| (k, v.len() as f64)).collect()),
                        age,
                        Some(issue_count(graph.len())?),
                    ))
                }
            }
            "idle_with_ready" => {
                let age = self.capability("lane")?;
                let lanes: BTreeMap<_, _> = self
                    .rows
                    .iter()
                    .filter_map(|o| {
                        if let Fact::Lane(l) = &o.fact {
                            Some((&o.entity_id, l))
                        } else {
                            None
                        }
                    })
                    .collect();
                if self
                    .config
                    .scope
                    .lanes
                    .iter()
                    .any(|l| !lanes.contains_key(&l.id))
                {
                    return Err("missing configured lane observation".into());
                }
                scalar(
                    lanes
                        .values()
                        .filter(|l| l.idle && l.ready_count > 0)
                        .count() as f64,
                    age,
                    Some(issue_count(lanes.len())?),
                )
            }
            "state_cycle_hours" => {
                let age = self.capability("state")?;
                let ordered = self.ordered("state", None)?;
                if parameters.states.is_empty() {
                    return Err("missing configured state mapping".into());
                }
                let mut groups: BTreeMap<String, Vec<&Observation>> = BTreeMap::new();
                for o in ordered {
                    groups.entry(o.entity_id.clone()).or_default().push(o);
                }
                let mut samples: BTreeMap<String, Vec<f64>> = parameters
                    .states
                    .iter()
                    .map(|s| (s.clone(), Vec::new()))
                    .collect();
                for rows in groups.values() {
                    for pair in rows.windows(2) {
                        if let Fact::State(state) = &pair[0].fact
                            && let Some(values) = samples.get_mut(&state.state)
                        {
                            let exit = event_time(pair[1])?;
                            if self.contains(exit) {
                                values.push((exit - event_time(pair[0])?) / 3600.0);
                            }
                        }
                    }
                }
                let rows = samples
                    .into_iter()
                    .filter_map(|(k, v)| Distribution::from_samples(&v).map(|d| (k, d)))
                    .collect::<BTreeMap<_, _>>();
                if rows.is_empty() {
                    return Err("empty duration population".into());
                }
                Ok((Value::Durations(rows), age, None))
            }
            "pr_without_reviewer_age_hours"
            | "repin_only_prs"
            | "review_round_policy_excess"
            | "gates_per_pr" => {
                let age = self.capability("pull_request")?;
                let prs: BTreeMap<_, _> = self
                    .rows
                    .iter()
                    .filter_map(|o| {
                        if let Fact::PullRequest(p) = &o.fact {
                            Some((&o.entity_id, (*o, p)))
                        } else {
                            None
                        }
                    })
                    .collect();
                let mut values = BTreeMap::new();
                let mut n = 0.0;
                for (key, (o, p)) in &prs {
                    let relevant = p.open
                        || p.merged_at
                            .as_deref()
                            .map(epoch)
                            .transpose()?
                            .is_some_and(|t| self.contains(t));
                    if !relevant {
                        continue;
                    }
                    match id {
                        "pr_without_reviewer_age_hours" if p.open && !p.draft => {
                            let reviewers = p
                                .assigned_reviewers
                                .ok_or("missing assigned reviewer history")?;
                            let reviews = p
                                .qualifying_reviews
                                .ok_or("missing qualifying review history")?;
                            if reviewers == 0 && reviews == 0 {
                                let at = epoch(
                                    p.review_ready_at
                                        .as_deref()
                                        .ok_or("missing review-ready timestamp")?,
                                )?;
                                values.insert((*key).clone(), (self.frame.now - at) / 3600.0);
                            }
                        }
                        "repin_only_prs" => {
                            let classifier = parameters
                                .diff_classifier
                                .as_deref()
                                .ok_or("missing diff_classifier version")?;
                            self.attributed(o, Some(classifier))?;
                            if p.repin_only
                                .ok_or("missing substantive diff classification")?
                            {
                                n += 1.0;
                            }
                        }
                        "review_round_policy_excess" => {
                            let limit = parameters
                                .review_round_limit
                                .ok_or("missing review_round_limit policy")?;
                            if p.fix_rounds
                                .ok_or("missing complete attributed fix-round markers")?
                                > limit
                            {
                                n += 1.0;
                            }
                        }
                        "gates_per_pr"
                            if p.merged_at
                                .as_deref()
                                .map(epoch)
                                .transpose()?
                                .is_some_and(|t| self.contains(t)) =>
                        {
                            self.attributed(o, None)?;
                            values.insert(
                                (*key).clone(),
                                p.full_gates
                                    .ok_or("missing complete attributed gate runs")?
                                    as f64,
                            );
                        }
                        _ => {}
                    }
                }
                if id == "pr_without_reviewer_age_hours" {
                    let samples: Vec<_> = values.into_values().collect();
                    let d =
                        Distribution::from_samples(&samples).ok_or("empty duration population")?;
                    Ok((
                        Value::Durations([("all".into(), d)].into()),
                        age,
                        Some(issue_count(prs.len())?),
                    ))
                } else if id == "gates_per_pr" {
                    Ok((Value::Keyed(values), age, Some(issue_count(prs.len())?)))
                } else {
                    scalar(n, age, Some(issue_count(prs.len())?))
                }
            }
            "crate_revision_count" | "pins_off_main" => {
                let age = self.capability("pin")?;
                let mut crates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
                let mut n = 0.0;
                for o in &self.rows {
                    if let Fact::Pin(pin) = &o.fact {
                        crates
                            .entry(pin.dependency.clone())
                            .or_default()
                            .insert(pin.revision.clone());
                        if id == "pins_off_main" && !pin.on_main.ok_or("missing dependency/default-head reachability or force-pushed-away revision")? { n += 1.0; }
                    }
                }
                if id == "pins_off_main" {
                    scalar(n, age, None)
                } else {
                    Ok((
                        Value::Keyed(
                            crates
                                .into_iter()
                                .map(|(k, v)| (k, v.len() as f64))
                                .collect(),
                        ),
                        age,
                        None,
                    ))
                }
            }
            "tag_repin_events" => {
                let age = self.capability("pin_event")?;
                let pairs = self.tag_pairs(false)?;
                scalar(pairs.len() as f64, age, Some(issue_count(pairs.len())?))
            }
            "focused_check_runs" | "workspace_check_runs" => {
                let age = self.capability("check")?;
                let mut n = 0.0;
                for o in &self.rows {
                    if let Fact::Check(check) = &o.fact
                        && self.contains(event_time(o)?)
                    {
                        self.attributed(o, None)?;
                        let selected = match check.scope {
                            super::CheckScope::Focused => id == "focused_check_runs",
                            super::CheckScope::Workspace => id == "workspace_check_runs",
                        };
                        if selected {
                            n += 1.0;
                        }
                    }
                }
                scalar(n, age, None)
            }
            _ => Err("collector not implemented or configured for this metric".into()),
        }
    }
    fn graph(&self) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
        self.capability("issue")?;
        // Retain external blockers for reachability, but never count terminal/canceled nodes.
        let all: BTreeMap<_, _> = self
            .batches
            .iter()
            .flat_map(|b| &b.observations)
            .filter_map(|o| {
                if let Fact::Issue(i) = &o.fact {
                    Some((o.entity_id.clone(), i))
                } else {
                    None
                }
            })
            .collect();
        let mut graph = BTreeMap::new();
        let mut stack: Vec<_> = self
            .issues()
            .iter()
            .filter(|(_, i)| !i.terminal)
            .map(|(o, _)| o.entity_id.clone())
            .chain(self.config.parameters.gates.iter().cloned())
            .collect();
        while let Some(id) = stack.pop() {
            if graph.contains_key(&id) {
                continue;
            }
            let issue = all
                .get(&id)
                .ok_or_else(|| format!("partial dependency graph: unresolved {id}"))?;
            if issue.terminal {
                graph.insert(id, BTreeSet::new());
                continue;
            }
            let mut blockers = BTreeSet::new();
            for blocker in &issue.blockers {
                let b = all
                    .get(blocker)
                    .ok_or_else(|| format!("partial dependency graph: unresolved {blocker}"))?;
                if !b.terminal {
                    blockers.insert(blocker.clone());
                    stack.push(blocker.clone());
                }
            }
            graph.insert(id, blockers);
        }
        for id in graph.keys() {
            depth(id, &graph, &mut BTreeSet::new())?;
        }
        Ok(graph)
    }
    fn ordered(&self, selected: &str, cutoff: Option<f64>) -> Result<Vec<&'a Observation>, String> {
        self.capability(selected)?;
        let mut rows = self
            .rows
            .iter()
            .copied()
            .filter(|o| family(&o.fact) == selected)
            .map(|o| Ok((event_time(o)?, o)))
            .collect::<Result<Vec<_>, String>>()?;
        rows.retain(|(at, _)| *at <= self.frame.now && cutoff.is_none_or(|c| *at > c));
        rows.sort_by(|(a, x), (b, y)| {
            a.total_cmp(b)
                .then(x.sequence.cmp(&y.sequence))
                .then(x.event_id.cmp(&y.event_id))
        });
        for pair in rows.windows(2) {
            if pair[0].0 == pair[1].0
                && (pair[0].1.sequence.is_none()
                    || pair[1].1.sequence.is_none()
                    || pair[0].1.sequence == pair[1].1.sequence)
            {
                return Err("missing stable source sequence for equal timestamps".into());
            }
        }
        Ok(rows.into_iter().map(|(_, o)| o).collect())
    }
    fn tag_pairs(
        &self,
        loop_window: bool,
    ) -> Result<Vec<(&'a Observation, &'a Observation)>, String> {
        let interval = self
            .config
            .parameters
            .repin_seconds
            .ok_or("missing repin_seconds")? as f64;
        let lookback = if loop_window {
            self.config
                .parameters
                .loop_lookback_seconds
                .ok_or("missing loop_lookback_seconds")? as f64
        } else {
            self.frame.window_secs as f64 + interval
        };
        let rows = self.ordered("pin_event", Some(self.frame.now - lookback))?;
        let mut used = BTreeSet::new();
        let mut pairs = Vec::new();
        for tag in &rows {
            let Fact::PinEvent(t) = &tag.fact else {
                continue;
            };
            if t.event != PinEventKind::Tag {
                continue;
            }
            for repin in &rows {
                let Fact::PinEvent(r) = &repin.fact else {
                    continue;
                };
                if r.event != PinEventKind::Repin
                    || r.dependency != t.dependency
                    || used.contains(&repin.event_id)
                {
                    continue;
                }
                let delta = event_time(repin)? - event_time(tag)?;
                let after = delta > 0.0
                    || (delta == 0.0
                        && tag.sequence.zip(repin.sequence).is_some_and(|(a, b)| b > a));
                if after && delta <= interval && (loop_window || self.contains(event_time(repin)?))
                {
                    used.insert(repin.event_id.clone());
                    pairs.push((*tag, *repin));
                    break;
                }
            }
        }
        Ok(pairs)
    }
    fn detector(&self, id: &str) -> Result<Vec<LoopFinding>, String> {
        let repeat = self
            .config
            .parameters
            .loop_repeat_count
            .ok_or("missing loop_repeat_count")?;
        let lookback = self
            .config
            .parameters
            .loop_lookback_seconds
            .ok_or("missing loop_lookback_seconds")?;
        let mut matches: BTreeMap<String, Vec<&Observation>> = BTreeMap::new();
        match id {
            "tag_repin_loop" => {
                for (tag, repin) in self.tag_pairs(true)? {
                    if let Fact::PinEvent(p) = &tag.fact {
                        matches
                            .entry(p.dependency.clone())
                            .or_default()
                            .extend([tag, repin]);
                    }
                }
            }
            "state_bounce_loop" => {
                let rows = self.ordered("state", Some(self.frame.now - lookback as f64))?;
                let mut entities: BTreeMap<&str, Vec<&Observation>> = BTreeMap::new();
                for row in rows {
                    entities.entry(&row.entity_id).or_default().push(row);
                }
                for (entity, rows) in entities {
                    let mut previous: Option<&str> = None;
                    let mut visits: BTreeMap<&str, Vec<&Observation>> = BTreeMap::new();
                    for o in rows {
                        if let Fact::State(s) = &o.fact {
                            if previous == Some(&s.state) {
                                continue;
                            }
                            previous = Some(&s.state);
                            visits.entry(&s.state).or_default().push(o);
                        }
                    }
                    for (state, visits) in visits {
                        if visits.len() as u64 > repeat {
                            matches.insert(format!("{entity}/{state}"), visits);
                        }
                    }
                }
            }
            _ => {
                return Err(
                    "ordered attributed order collector not implemented or configured".into(),
                );
            }
        }
        let mut findings = Vec::new();
        for (entity, rows) in matches {
            let count = if id == "tag_repin_loop" {
                rows.len() / 2
            } else {
                rows.len() - 1
            };
            if count as u64 >= repeat
                && let (Some(first), Some(last)) = (rows.first(), rows.last())
            {
                let event_ids: Vec<_> = rows.iter().map(|o| o.event_id.clone()).collect();
                findings.push(LoopFinding {
                    deduplication_key: super::digest(&(id, &entity, &event_ids)),
                    event_ids,
                    start: first
                        .event_at
                        .clone()
                        .ok_or("missing first event timestamp")?,
                    end: last
                        .event_at
                        .clone()
                        .ok_or("missing last event timestamp")?,
                    participants: rows
                        .iter()
                        .map(|o| o.entity_id.clone())
                        .chain([entity])
                        .collect(),
                });
            }
        }
        Ok(findings)
    }
}
fn selected_issue(scope: &Scope, i: &IssueFact) -> bool {
    (scope.teams.is_empty() || scope.teams.iter().any(|s| s.id == i.team))
        && (scope.projects.is_empty()
            || i.project
                .as_ref()
                .is_some_and(|id| scope.projects.iter().any(|s| &s.id == id)))
}
fn depth(
    id: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    visiting: &mut BTreeSet<String>,
) -> Result<u64, String> {
    if !visiting.insert(id.into()) {
        return Err(format!("dependency graph cycle at {id}"));
    }
    let edges = graph
        .get(id)
        .ok_or_else(|| format!("partial dependency graph: unresolved {id}"))?;
    let mut length = 0;
    for blocker in edges {
        length = length.max(
            depth(blocker, graph, visiting)?
                .checked_add(1)
                .ok_or("dependency chain overflow")?,
        );
    }
    visiting.remove(id);
    Ok(length)
}

/// Computes the complete catalog from admitted normalized source passes and unchanged old metrics.
/// Missing future collectors remain visible; no source is read by this function.
pub fn collect(
    config: &Config,
    frame: Frame,
    batches: &[Evidence],
    legacy: &BTreeMap<String, Metric>,
) -> Report {
    let calc = Calculator::new(config, frame, batches);
    let mut measurements = BTreeMap::new();
    for d in CATALOG {
        let outcome = calc.calculate(d.id);
        let (value, reason, age, population) = match outcome {
            Ok((v, age, pop)) => (Some(v), None, Some(age), pop),
            Err(reason) => {
                if let Some(old) = legacy.get(d.id) {
                    (old.value.map(Value::Scalar), old.reason.clone(), None, None)
                } else {
                    (None, Some(reason), None, None)
                }
            }
        };
        let evidence_refs = calc.rows.iter().map(|o| o.event_id.clone()).collect();
        let denominator = if d.id == "wip_capacity_ratio" {
            None
        } else {
            population
        };
        measurements.insert(
            d.id.into(),
            Measurement {
                id: d.id.into(),
                catalog_version: CATALOG_VERSION.into(),
                status: if value.is_some() {
                    Status::Ok
                } else {
                    Status::Unavailable
                },
                value,
                reason,
                unit: d.unit.into(),
                direction: legacy.get(d.id).map_or(Direction::Down, |m| m.direction),
                scope: config.scope.clone(),
                query_identity: config.measurement_identity(d.id, frame.window_secs),
                window_start: crate::drift::format_timestamp(frame.now - frame.window_secs as f64),
                window_end: crate::drift::format_timestamp(frame.now),
                window_seconds: frame.window_secs,
                source_age_seconds: age,
                population,
                denominator,
                evidence_refs,
            },
        );
    }
    let loops = ["tag_repin_loop", "state_bounce_loop", "order_flip_loop"]
        .into_iter()
        .map(|id| {
            let (status, reason, findings) = match calc.detector(id) {
                Ok(findings) => (Status::Ok, None, findings),
                Err(reason) => (Status::Unavailable, Some(reason), Vec::new()),
            };
            (
                id.into(),
                LoopReport {
                    detector_id: id.into(),
                    status,
                    reason,
                    findings,
                },
            )
        })
        .collect();
    let mut observations: Vec<_> = calc.rows.into_iter().cloned().collect();
    observations.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    let mut report = Report {
        version: 1,
        catalog_version: CATALOG_VERSION.into(),
        policy_id: config.policy_id.clone(),
        config_identity: config.identity(),
        scope: config.scope.clone(),
        observations,
        measurements,
        loops,
        classes: BTreeMap::new(),
        rollup: Summary {
            severity: Severity::Unknown,
            complete: false,
            missing: BTreeMap::new(),
        },
    };
    super::assess(&mut report, config, None, None);
    report
}
