// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only, revisioned source-event exports for authoritative histories and classifications.
//! Coverage is an explicit producer assertion; an empty event array alone proves nothing.
use super::{
    Attribution, CheckFact, CheckScope, Config, Evidence, Fact, LaneFact, Observation, PinEvent,
    PinEventKind, SourceStamp,
};
use crate::retro::Frame;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Source capabilities supplied by a configured export producer.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    /// Complete explicit claim/start history.
    Claims,
    /// Issue-linked PR-head, commit and worktree movement.
    Movement,
    /// Structured transitions into/out of blocked condition.
    Blocked,
    /// Named completion obligation observations.
    Completion,
    /// Verified lane idle status and structured ready issue identities.
    Lanes,
    /// Dependency tag publication and committed resolution events.
    PinEvents,
    /// Attributed focused/workspace check invocations.
    Checks,
    /// Attributed full gate runs for exact repository/PR identities.
    Gates,
    /// Attributed substantive diff classifications.
    Diffs,
}
wire! {
    /// Explicit configured source file and the capabilities its producer supplies.
    LogInput {
        /// Read-only local source export path.
        path: PathBuf,
        /// Producer identity, matching freshness configuration and the export header.
        source: String,
        /// Capabilities this input is authorized to enrich.
        kinds: BTreeSet<LogKind>
    }
}
wire! {
    /// Complete source coverage for explicit native subjects and a closed observation interval.
    Coverage {
        /// Capability whose population is covered.
        kind: LogKind,
        /// Issue UUIDs, full PR IDs, lane IDs, repository IDs or dependency IDs for this kind.
        entities: BTreeSet<String>,
        /// First covered UTC instant.
        from: String,
        /// Source watermark through this UTC instant.
        until: String,
        /// Producer confirms a complete read without missing pages/events.
        complete: bool,
        /// Producer confirms history from subject origin, required for claim/blocker/obligation/gate history.
        #[serde(default)] full_history: bool,
        /// Explicit covered movement channels; all three are required for no-movement claims.
        #[serde(default)] movement_sources: BTreeSet<MovementKind>
    }
}
/// Native movement channels; no channel is inferred from another.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum MovementKind {
    /// Verified PR-head movement.
    PrHead,
    /// Verified commit movement.
    Commit,
    /// Verified worktree movement.
    Worktree,
}
wire! {
    /// One source event with stable identity and explicit classification provenance.
    LogEvent {
        /// Stable producer/native event ID.
        event_id: String,
        /// Native issue/lane/dependency/PR identity.
        entity_id: String,
        /// Actual event UTC time.
        event_at: String,
        /// Native source sequence, when available.
        sequence: Option<u64>,
        /// Resolving native record reference, excluding transcript/command bodies.
        source_ref: String,
        /// Required for classified diff/check/gate records.
        attribution: Option<Attribution>,
        /// Raw typed source record, normalized by the production reader.
        record: NativeRecord
    }
}
/// Export records preserve source facts; text bodies and command-name classifiers are excluded.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeRecord {
    /// Genuine claim/start event, not an issue update timestamp.
    Claim,
    /// Verified issue-linked movement.
    Movement {
        /// Source channel.
        channel: MovementKind,
    },
    /// Transition of the actual structured blocked condition.
    Blocked {
        /// Condition after this event.
        blocked: bool,
    },
    /// Named obligation satisfaction or revocation.
    Obligation {
        /// Configured obligation identity.
        name: String,
        /// Satisfaction after this event.
        met: bool,
    },
    /// Verified current lane and explicitly assigned structured ready population.
    Lane {
        /// Verified idle observation.
        idle: bool,
        /// Native ready issue UUIDs within this lane's configured scope.
        ready_issue_ids: BTreeSet<String>,
    },
    /// Source-native tag publication time, independent of target commit time.
    Tag {
        /// Configured dependency identity.
        dependency: String,
    },
    /// Committed lock/manifests revision change, independent of a gate-log repin invocation.
    Repin {
        /// Configured dependency identity.
        dependency: String,
        /// Full native repository identity.
        repository: String,
    },
    /// Logged check invocation with explicit scope.
    Check {
        /// Full native repository identity.
        repository: String,
        /// Full repository#number identity, if attributed.
        pr: Option<String>,
        /// Source-classified invocation scope.
        scope: CheckScope,
    },
    /// A genuine completed aggregate gate invocation.
    Gate {
        /// Full native repository#number identity.
        pr: String,
    },
    /// Attributed substantive dependency-only classification.
    Diff {
        /// Full native repository#number identity.
        pr: String,
        /// Classifier result.
        repin_only: bool,
    },
}
wire! {
    /// Public source-event export schema, separate from the report schema.
    SourceLog {
        /// Export schema version; currently one.
        version:u32,
        /// Actual producer identity.
        source:String,
        /// Actual source observation time; never filled from the snapshot clock.
        observed_at:String,
        /// Explicit independent population/history coverage.
        coverage:Vec<Coverage>,
        /// Stable native events, with duplicate identities checked before use.
        events:Vec<LogEvent>
    }
}
fn epoch(at: &str) -> Result<f64, String> {
    crate::drift::timestamp_epoch(at).map_err(|e| e.to_string())
}
fn kind(record: &NativeRecord) -> LogKind {
    match record {
        NativeRecord::Claim => LogKind::Claims,
        NativeRecord::Movement { .. } => LogKind::Movement,
        NativeRecord::Blocked { .. } => LogKind::Blocked,
        NativeRecord::Obligation { .. } => LogKind::Completion,
        NativeRecord::Lane { .. } => LogKind::Lanes,
        NativeRecord::Tag { .. } | NativeRecord::Repin { .. } => LogKind::PinEvents,
        NativeRecord::Check { .. } => LogKind::Checks,
        NativeRecord::Gate { .. } => LogKind::Gates,
        NativeRecord::Diff { .. } => LogKind::Diffs,
    }
}
fn valid_attribution(event: &LogEvent) -> bool {
    event.attribution.as_ref().is_some_and(|a| {
        !a.actor.is_empty() && !a.classifier_version.is_empty() && a.offset_end >= a.offset_start
    })
}
fn field(kind: LogKind) -> Option<&'static str> {
    match kind {
        LogKind::Claims => Some("claim_at"),
        LogKind::Movement => Some("movement"),
        LogKind::Blocked => Some("blocked_at"),
        LogKind::Completion => Some("obligations"),
        LogKind::Gates => Some("full_gates"),
        LogKind::Diffs => Some("repin_only"),
        _ => None,
    }
}

/// Reads configured exports through the same injectable file boundary used by snapshots.
/// Enrichment edits existing native rows once; it never appends competing current snapshots.
pub fn read_source_logs(
    config: &Config,
    frame: Frame,
    batches: &mut Vec<Evidence>,
    mut read: impl FnMut(&Path) -> Result<String, String>,
) {
    for input in &config.parameters.event_logs {
        let result = read(&input.path).and_then(|body| decode(config, frame, input, &body));
        match result {
            Ok(log) => apply(config, frame, input, &log, batches),
            Err(reason) => {
                for batch in &mut *batches {
                    for row in &mut batch.observations {
                        for selected in &input.kinds {
                            if let Some(field) = field(*selected) {
                                stamp(
                                    row,
                                    field,
                                    SourceStamp {
                                        source: input.source.clone(),
                                        observed_at: None,
                                        complete: false,
                                        reason: Some(reason.clone()),
                                        evidence_refs: vec![format!(
                                            "file:{}",
                                            input.path.display()
                                        )],
                                    },
                                );
                            }
                        }
                    }
                }
                let mut unavailable = Evidence {
                    source: input.source.clone(),
                    ..Default::default()
                };
                for selected in &input.kinds {
                    let family = match selected {
                        LogKind::Lanes => Some("lane"),
                        LogKind::PinEvents => Some("pin_event"),
                        LogKind::Checks => Some("check"),
                        _ => None,
                    };
                    if let Some(family) = family {
                        unavailable.reasons.insert(family.into(), reason.clone());
                    }
                }
                batches.push(unavailable);
            }
        }
    }
}
fn decode(
    config: &Config,
    frame: Frame,
    input: &LogInput,
    body: &str,
) -> Result<SourceLog, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("{}: {e}", input.path.display()))?;
    if value.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("source export.version: unsupported or missing".into());
    }
    let mut log: SourceLog = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if log.source != input.source {
        return Err("source export.source differs from configuration".into());
    }
    let age = frame.now - epoch(&log.observed_at)?;
    let limit = *config
        .freshness_seconds
        .get(&log.source)
        .ok_or("missing source freshness configuration")? as f64;
    if age > limit {
        return Err("stale source export".into());
    }
    if age < -(config.clock_tolerance_seconds as f64) {
        return Err("source export future clock skew".into());
    }
    let mut ids: BTreeMap<String, LogEvent> = BTreeMap::new();
    for event in log.events.drain(..) {
        if event.event_id.is_empty() || event.entity_id.is_empty() || event.source_ref.is_empty() {
            return Err("source export event lacks native identity/reference".into());
        }
        if !input.kinds.contains(&kind(&event.record)) {
            return Err("source export contains an unconfigured capability".into());
        }
        if epoch(&event.event_at)? > frame.now + config.clock_tolerance_seconds as f64 {
            return Err("source export event future clock skew".into());
        }
        if let Some(prior) = ids.insert(event.event_id.clone(), event.clone())
            && prior != event
        {
            return Err("source export conflicting duplicate event identity".into());
        }
    }
    log.events = ids.into_values().collect();
    log.events.sort_by(|a, b| {
        epoch(&a.event_at)
            .unwrap_or(f64::NEG_INFINITY)
            .total_cmp(&epoch(&b.event_at).unwrap_or(f64::NEG_INFINITY))
            .then(a.sequence.cmp(&b.sequence))
            .then(a.event_id.cmp(&b.event_id))
    });
    Ok(log)
}
fn coverage<'a>(
    log: &'a SourceLog,
    kind: LogKind,
    entity: &str,
    from: f64,
    until: f64,
    origin: bool,
) -> Result<&'a Coverage, String> {
    log.coverage
        .iter()
        .find(|c| {
            c.kind == kind
                && c.entities.contains(entity)
                && c.complete
                && (!origin || c.full_history)
                && epoch(&c.from).is_ok_and(|at| at <= from)
                && epoch(&c.until).is_ok_and(|at| at >= until)
        })
        .ok_or_else(|| format!("missing complete {kind:?} coverage for {entity}"))
}
fn stamp(row: &mut Observation, field: &str, source: SourceStamp) {
    match &mut row.fact {
        Fact::Issue(i) => {
            i.provenance.insert(field.into(), source);
        }
        Fact::PullRequest(p) => {
            p.provenance.insert(field.into(), source);
        }
        _ => {}
    }
}
fn make_stamp(log: &SourceLog, events: &[&LogEvent], result: Result<(), String>) -> SourceStamp {
    SourceStamp {
        source: log.source.clone(),
        observed_at: Some(log.observed_at.clone()),
        complete: result.is_ok(),
        reason: result.err(),
        evidence_refs: events.iter().map(|e| e.source_ref.clone()).collect(),
    }
}
fn ordered_events<'a>(
    log: &'a SourceLog,
    selected: LogKind,
    entity: &str,
    now: f64,
) -> Result<Vec<&'a LogEvent>, String> {
    let events: Vec<_> = log
        .events
        .iter()
        .filter(|e| {
            kind(&e.record) == selected
                && e.entity_id == entity
                && epoch(&e.event_at).is_ok_and(|at| at <= now)
        })
        .collect();
    for pair in events.windows(2) {
        if epoch(&pair[0].event_at)? == epoch(&pair[1].event_at)?
            && (pair[0].sequence.is_none() || pair[0].sequence == pair[1].sequence)
        {
            return Err("missing native source order for equal timestamps".into());
        }
    }
    Ok(events)
}
fn apply(
    config: &Config,
    frame: Frame,
    input: &LogInput,
    log: &SourceLog,
    batches: &mut Vec<Evidence>,
) {
    let start = frame.now - frame.window_secs as f64;
    for batch in &mut *batches {
        for row in &mut batch.observations {
            let entity = row.entity_id.clone();
            for selected in &input.kinds {
                let Some(field) = field(*selected) else {
                    continue;
                };
                let mut events = ordered_events(log, *selected, &entity, frame.now);
                let required_from = if *selected == LogKind::Movement {
                    match &row.fact {
                        Fact::Issue(i) => i
                            .claim_at
                            .as_deref()
                            .and_then(|at| epoch(at).ok())
                            .unwrap_or(start),
                        _ => start,
                    }
                } else {
                    start
                };
                let check = coverage(
                    log,
                    *selected,
                    &entity,
                    required_from,
                    frame.now,
                    matches!(
                        selected,
                        LogKind::Claims | LogKind::Blocked | LogKind::Completion | LogKind::Gates
                    ),
                )
                .and_then(|c| {
                    if *selected == LogKind::Movement
                        && ![
                            MovementKind::PrHead,
                            MovementKind::Commit,
                            MovementKind::Worktree,
                        ]
                        .into_iter()
                        .all(|k| c.movement_sources.contains(&k))
                    {
                        return Err(
                            "incomplete PR-head/commit/worktree movement channel coverage".into(),
                        );
                    }
                    if let Err(reason) = &events {
                        return Err(reason.clone());
                    }
                    Ok(())
                });
                let success = check.is_ok();
                let mut conflict = None;
                let events = events.as_mut().map(|e| e.as_slice()).unwrap_or(&[]);
                if success {
                    match (&mut row.fact, *selected) {
                        (Fact::Issue(i), LogKind::Claims) => {
                            if let Some(event) = events.last()
                                && i.claim_at
                                    .as_deref()
                                    .and_then(|at| epoch(at).ok())
                                    .is_none_or(|old| {
                                        epoch(&event.event_at).is_ok_and(|new| new > old)
                                    })
                            {
                                i.claim_at = Some(event.event_at.clone());
                            }
                        }
                        (Fact::Issue(i), LogKind::Movement) => {
                            i.movement_complete = true;
                            i.movement_at = events.last().map(|e| e.event_at.clone());
                        }
                        (Fact::Issue(i), LogKind::Blocked) => {
                            i.blocked_at = events.last().and_then(|e| match e.record {
                                NativeRecord::Blocked { blocked: true } => Some(e.event_at.clone()),
                                _ => None,
                            });
                        }
                        (Fact::Issue(i), LogKind::Completion) => {
                            let mut obligations: BTreeMap<String, bool> = BTreeMap::new();
                            for event in events {
                                if let NativeRecord::Obligation { name, met } = &event.record {
                                    obligations.insert(name.clone(), *met);
                                }
                            }
                            let required = config.parameters.completion_obligations.as_ref();
                            i.obligations = required
                                .filter(|required| {
                                    required.iter().all(|id| obligations.contains_key(id))
                                })
                                .map(|_| {
                                    obligations
                                        .into_iter()
                                        .filter(|(_, met)| *met)
                                        .map(|(id, _)| id)
                                        .collect()
                                });
                        }
                        (Fact::PullRequest(p), LogKind::Gates) => {
                            if events.iter().all(|e| {
                                valid_attribution(e)
                                    && matches!(&e.record,NativeRecord::Gate{pr} if pr==&entity)
                            }) {
                                p.full_gates = u64::try_from(events.len()).ok();
                                if let Some(event) = events.first() {
                                    row.attribution = event.attribution.clone();
                                }
                            } else {
                                p.full_gates = None;
                            }
                        }
                        (Fact::PullRequest(p), LogKind::Diffs) => {
                            if let Some(event) = events.last()
                                && valid_attribution(event)
                                && let NativeRecord::Diff { pr, repin_only } = &event.record
                                && pr == &entity
                            {
                                if p.repin_only.is_some_and(|native| native != *repin_only) {
                                    conflict=Some("conflicting native/exported substantive diff classifications".into());
                                } else {
                                    p.repin_only = Some(*repin_only);
                                    row.attribution = event.attribution.clone();
                                }
                            }
                        }
                        _ => {}
                    }
                }
                let mut source = make_stamp(log, events, check);
                source.evidence_refs.push(format!(
                    "file:{}#coverage:{selected:?}:{entity}",
                    input.path.display()
                ));
                if let Some(reason) = conflict {
                    source.complete = false;
                    source.reason = Some(reason);
                }
                if *selected == LogKind::Gates
                    && success
                    && events.iter().any(|e| !valid_attribution(e))
                {
                    source.complete = false;
                    source.reason = Some("missing attributed full gate scope/run evidence".into());
                }
                if *selected == LogKind::Diffs
                    && success
                    && events.iter().any(|e| !valid_attribution(e))
                {
                    source.complete = false;
                    source.reason =
                        Some("missing attributed substantive diff classification".into());
                }
                stamp(row, field, source);
            }
        }
    }
    let mut normalized = Evidence {
        source: log.source.clone(),
        observed_at: Some(log.observed_at.clone()),
        ..Default::default()
    };
    for event in &log.events {
        if epoch(&event.event_at).is_err() || epoch(&event.event_at).is_ok_and(|at| at > frame.now)
        {
            continue;
        }
        let fact = match &event.record {
            NativeRecord::Lane {
                idle,
                ready_issue_ids,
            } if config.scope.lanes.iter().any(|s| s.id == event.entity_id) => {
                Some(Fact::Lane(LaneFact {
                    idle: *idle,
                    ready_count: u64::try_from(ready_issue_ids.len()).unwrap_or(u64::MAX),
                }))
            }
            NativeRecord::Tag { dependency }
                if config.parameters.dependencies.contains(dependency) =>
            {
                Some(Fact::PinEvent(PinEvent {
                    dependency: dependency.clone(),
                    event: PinEventKind::Tag,
                }))
            }
            NativeRecord::Repin {
                dependency,
                repository,
            } if config.parameters.dependencies.contains(dependency)
                && config
                    .scope
                    .repositories
                    .iter()
                    .any(|s| &s.id == repository) =>
            {
                Some(Fact::PinEvent(PinEvent {
                    dependency: dependency.clone(),
                    event: PinEventKind::Repin,
                }))
            }
            NativeRecord::Check {
                repository,
                pr,
                scope,
            } if config
                .scope
                .repositories
                .iter()
                .any(|s| &s.id == repository) =>
            {
                Some(Fact::Check(CheckFact {
                    scope: *scope,
                    pr: pr.clone(),
                }))
            }
            _ => None,
        };
        if let Some(fact) = fact {
            normalized.observations.push(Observation {
                event_id: event.event_id.clone(),
                entity_id: event.entity_id.clone(),
                event_at: Some(event.event_at.clone()),
                observed_at: Some(log.observed_at.clone()),
                sequence: event.sequence,
                scope: config.scope.clone(),
                complete: true,
                source_ref: event.source_ref.clone(),
                attribution: event.attribution.clone(),
                fact,
            });
        }
    }
    for (selected, family, entities, from) in [
        (
            LogKind::Lanes,
            "lane",
            config
                .scope
                .lanes
                .iter()
                .map(|s| s.id.clone())
                .collect::<BTreeSet<_>>(),
            start,
        ),
        (
            LogKind::Checks,
            "check",
            config
                .scope
                .repositories
                .iter()
                .map(|s| s.id.clone())
                .collect(),
            start,
        ),
        (
            LogKind::PinEvents,
            "pin_event",
            config.parameters.dependencies.clone(),
            frame.now
                - (frame
                    .window_secs
                    .saturating_add(config.parameters.repin_seconds.unwrap_or(0)))
                .max(config.parameters.loop_lookback_seconds.unwrap_or(0)) as f64,
        ),
    ] {
        if input.kinds.contains(&selected) {
            if !entities.is_empty()
                && entities
                    .iter()
                    .all(|entity| coverage(log, selected, entity, from, frame.now, false).is_ok())
            {
                normalized.complete.insert(family.into());
            } else {
                normalized.reasons.insert(
                    family.into(),
                    format!("missing configured population/complete {selected:?} coverage"),
                );
            }
        }
    }
    batches.push(normalized);
}
